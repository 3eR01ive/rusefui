#!/usr/bin/env python3
"""Подбор коэффициентов модели по эталонным CSV (усреднение по всем ДВС)."""

from __future__ import annotations

import copy
import json
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from compare_map import compare_all, format_summary
from model import SparkAdvanceCalculator, load_input
from model.coefficients import ModelCoefficients
from model.datasets import ReferenceDataset, discover_datasets
from model.io import read_csv_map


@dataclass(frozen=True)
class TunableParam:
    name: str
    path: tuple[str, ...]
    low: float
    high: float
    initial: float


DEFAULT_TUNABLES: tuple[TunableParam, ...] = (
    TunableParam("burn_duration_ref_deg", ("reference_engine", "burn_duration_ref_deg"), 28.0, 42.0, 38.0),
    TunableParam("peak_pressure_target_deg", ("peak_pressure_target_deg",), 10.0, 18.0, 14.0),
    TunableParam("flame_delay_base_deg", ("flame_delay", "base_deg"), 5.0, 10.0, 8.0),
    TunableParam("rpm_correction_factor", ("rpm_correction", "factor"), 3.0, 11.0, 9.0),
    TunableParam("vacuum_deg_per_10_kpa", ("load_correction", "vacuum", "deg_per_10_kpa"), 0.5, 2.5, 2.0),
    TunableParam("boost_deg_per_0_1_bar", ("load_correction", "boost", "deg_per_0_1_bar"), -2.0, -0.6, -1.2),
    TunableParam("overlap_retard_per_deg", ("cam_timing", "overlap_retard_per_deg"), 0.0, 0.15, 0.05),
    TunableParam("stock_overlap_deg", ("cam_timing", "stock_overlap_deg"), 10.0, 30.0, 20.0),
    TunableParam("fuel_gasoline_92", ("fuel_factors", "gasoline_92"), 0.85, 1.05, 1.0),
    TunableParam("fuel_gasoline_98", ("fuel_factors", "gasoline_98"), 0.85, 1.05, 0.96),
)


REFINE_SPAN: dict[str, float] = {
    "burn_duration_ref_deg": 4.0,
    "peak_pressure_target_deg": 2.0,
    "flame_delay_base_deg": 1.5,
    "rpm_correction_factor": 1.5,
    "vacuum_deg_per_10_kpa": 0.35,
    "boost_deg_per_0_1_bar": 0.25,
    "overlap_retard_per_deg": 0.03,
    "stock_overlap_deg": 5.0,
    "fuel_gasoline_92": 0.06,
    "fuel_gasoline_98": 0.06,
}


def _set_nested(data: dict[str, Any], path: tuple[str, ...], value: float) -> None:
    node = data
    for key in path[:-1]:
        node = node[key]
    node[path[-1]] = value


def _get_nested(data: dict[str, Any], path: tuple[str, ...]) -> float:
    node = data
    for key in path:
        node = node[key]
    return float(node)


def load_coefficients_data(path: Path) -> dict[str, Any]:
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def coefficients_from_vector(
    base_data: dict[str, Any],
    params: tuple[TunableParam, ...],
    vector: list[float],
) -> ModelCoefficients:
    data = copy.deepcopy(base_data)
    for param, value in zip(params, vector):
        clipped = max(param.low, min(param.high, value))
        _set_nested(data, param.path, clipped)
    return ModelCoefficients.from_dict(data)


def count_reference_cells(datasets: tuple[ReferenceDataset, ...]) -> int:
    total = 0
    for dataset in datasets:
        reference = read_csv_map(dataset.reference_csv)
        total += len(reference.map_axis) * len(reference.rpm_axis)
    return total


def _cell_weight(map_kpa: float, rpm: float) -> float:
    weight = 1.0
    if map_kpa <= 100.0:
        weight *= 1.5
    if map_kpa <= 60.0:
        weight *= 1.5
    if rpm <= 2000.0:
        weight *= 1.3
    if map_kpa >= 200.0:
        weight *= 1.2
    return weight


def collect_weighted_errors(
    coefficients: ModelCoefficients,
    datasets: tuple[ReferenceDataset, ...],
    weighted: bool,
    skip_negative_reference: bool = True,
) -> tuple[list[float], list[float], int]:
    all_sq: list[float] = []
    all_w: list[float] = []
    skipped = 0

    for dataset in datasets:
        map_input = load_input(dataset.example_json)
        reference = read_csv_map(dataset.reference_csv)
        calculator = SparkAdvanceCalculator(map_input.engine, coefficients)
        grid = calculator.generate_map(map_input.rpm_axis, map_input.map_axis)

        for map_idx in reversed(range(len(map_input.map_axis))):
            file_row = len(map_input.map_axis) - 1 - map_idx
            map_kpa = map_input.map_axis[map_idx]
            for rpm_idx in range(len(map_input.rpm_axis)):
                actual = reference.values[file_row][rpm_idx]
                if skip_negative_reference and actual < 0.0:
                    skipped += 1
                    continue

                rpm = map_input.rpm_axis[rpm_idx]
                err = grid[rpm_idx][map_idx].advance_deg - actual
                w = _cell_weight(map_kpa, rpm) if weighted else 1.0
                all_sq.append(w * err * err)
                all_w.append(w)

    if not all_sq:
        raise ValueError("no training cells left after filtering")

    return all_sq, all_w, skipped


def combined_loss_for_vector(
    vector: list[float],
    base_data: dict[str, Any],
    params: tuple[TunableParam, ...],
    datasets: tuple[ReferenceDataset, ...],
    weighted: bool,
    skip_negative_reference: bool = True,
) -> float:
    coefficients = coefficients_from_vector(base_data, params, vector)
    all_sq, all_w, _ = collect_weighted_errors(
        coefficients,
        datasets,
        weighted,
        skip_negative_reference=skip_negative_reference,
    )
    return math.sqrt(sum(all_sq) / sum(all_w))


def refine_bounds(
    params: tuple[TunableParam, ...],
    current: list[float],
) -> list[tuple[float, float]]:
    bounds: list[tuple[float, float]] = []
    for param, value in zip(params, current):
        span = REFINE_SPAN.get(param.name, abs(value) * 0.15 + 0.1)
        low = max(param.low, value - span)
        high = min(param.high, value + span)
        if low >= high:
            low, high = param.low, param.high
        bounds.append((low, high))
    return bounds


def fit_coefficients(
    datasets: tuple[ReferenceDataset, ...],
    base_coefficients: Path,
    params: tuple[TunableParam, ...] = DEFAULT_TUNABLES,
    refine: bool = False,
    weighted: bool = False,
    seed: int = 42,
) -> tuple[dict[str, Any], dict[str, float]]:
    from scipy.optimize import differential_evolution, minimize

    if not datasets:
        raise FileNotFoundError("no datasets to fit")

    base_data = load_coefficients_data(base_coefficients)

    x0 = [
        _get_nested(base_data, param.path) if _path_exists(base_data, param.path) else param.initial
        for param in params
    ]
    for param, value in zip(params, x0):
        if not _path_exists(base_data, param.path):
            _set_nested(base_data, param.path, value)

    bounds = refine_bounds(params, x0) if refine else [(p.low, p.high) for p in params]

    def objective(vector: list[float]) -> float:
        return combined_loss_for_vector(vector, base_data, params, datasets, weighted)

    print(f"datasets: {', '.join(d.name for d in datasets)}")
    _, _, skipped = collect_weighted_errors(
        coefficients_from_vector(base_data, params, x0),
        datasets,
        weighted,
    )
    print(f"training cells: {count_reference_cells(datasets) - skipped} "
          f"(skipped {skipped} with reference advance < 0°)")
    print(f"baseline combined loss: {objective(x0):.3f}°")
    print(f"mode: {'refine' if refine else 'global'}{' + weighted' if weighted else ''}")
    print("optimizing...")

    result = differential_evolution(
        objective,
        bounds=bounds,
        seed=seed,
        maxiter=400 if refine else 300,
        polish=True,
        tol=0.005 if refine else 0.01,
        workers=1,
    )

    local = minimize(objective, result.x, method="Powell", options={"maxiter": 800})
    best = local.x if local.fun < result.fun else result.x
    best_loss = objective(best)

    fitted = copy.deepcopy(base_data)
    fitted_values: dict[str, float] = {}
    for param, value in zip(params, best):
        clipped = max(param.low, min(param.high, float(value)))
        _set_nested(fitted, param.path, round(clipped, 4))
        fitted_values[param.name] = clipped

    fitted_values["combined_loss_deg"] = best_loss
    return fitted, fitted_values


def _path_exists(data: dict[str, Any], path: tuple[str, ...]) -> bool:
    node: Any = data
    for key in path:
        if not isinstance(node, dict) or key not in node:
            return False
        node = node[key]
    return True


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(
        description="Fit model coefficients against all reference datasets.",
    )
    parser.add_argument(
        "-c",
        "--coefficients",
        type=Path,
        default=Path("config/coefficients.json"),
        help="Base coefficients JSON to tune",
    )
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        help="Write fitted coefficients here (default: overwrite --coefficients)",
    )
    parser.add_argument(
        "--refine",
        action="store_true",
        help="Narrow search around current coefficient values",
    )
    parser.add_argument(
        "--weighted",
        action="store_true",
        help="Weight vacuum / low-rpm cells higher in the loss",
    )
    parser.add_argument(
        "--seed",
        type=int,
        default=42,
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print result without writing file",
    )
    args = parser.parse_args()

    datasets = discover_datasets()
    fitted_data, fitted_values = fit_coefficients(
        datasets,
        args.coefficients,
        refine=args.refine,
        weighted=args.weighted,
        seed=args.seed,
    )

    output_path = args.output or args.coefficients
    if not args.dry_run:
        with open(output_path, "w", encoding="utf-8") as f:
            json.dump(fitted_data, f, indent=2, ensure_ascii=False)
            f.write("\n")

    print("\nFitted parameters:")
    for name, value in fitted_values.items():
        if name != "combined_loss_deg":
            print(f"  {name}: {value:.4f}")
    print(f"  combined loss: {fitted_values['combined_loss_deg']:.3f}°")

    if not args.dry_run:
        summary = compare_all(output_path)
        print("\n" + format_summary(summary))
        print(f"\nWrote {output_path}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
