"""Сравнение сгенерированной карты с эталоном."""

from __future__ import annotations

import json
import math
from dataclasses import dataclass
from pathlib import Path

from model import SparkAdvanceCalculator, load_coefficients, load_input
from model.datasets import ReferenceDataset, discover_datasets
from model.io import CsvMap, read_csv_map


@dataclass(frozen=True)
class CompareResult:
    cells: int
    mae_deg: float
    rmse_deg: float
    max_abs_error_deg: float
    worst_map_kpa: float
    worst_rpm: float
    bias_deg: float


@dataclass(frozen=True)
class CompareSummary:
    per_engine: tuple[tuple[str, CompareResult], ...]
    total_cells: int
    mae_deg: float
    rmse_deg: float
    bias_deg: float
    max_abs_error_deg: float
    worst_engine: str
    worst_map_kpa: float
    worst_rpm: float


def _assert_same_axes(reference: CsvMap, candidate: CsvMap, ref_label: str, cand_label: str) -> None:
    if reference.rpm_axis != candidate.rpm_axis:
        raise ValueError(
            f"RPM axis mismatch: {ref_label} {reference.rpm_axis} vs "
            f"{cand_label} {candidate.rpm_axis}"
        )
    if reference.map_axis != candidate.map_axis:
        raise ValueError(
            f"MAP axis mismatch: {ref_label} {reference.map_axis} vs "
            f"{cand_label} {candidate.map_axis}"
        )


def compare_maps(reference: CsvMap, candidate: CsvMap) -> CompareResult:
    _assert_same_axes(reference, candidate, "reference", "candidate")

    errors: list[tuple[float, float, float, float]] = []
    for map_idx, map_kpa in enumerate(reference.map_axis):
        for rpm_idx, rpm in enumerate(reference.rpm_axis):
            err = candidate.values[map_idx][rpm_idx] - reference.values[map_idx][rpm_idx]
            errors.append((map_kpa, rpm, err, abs(err)))

    if not errors:
        raise ValueError("empty maps")

    abs_errors = [item[3] for item in errors]
    signed = [item[2] for item in errors]
    worst = max(errors, key=lambda item: item[3])

    return CompareResult(
        cells=len(errors),
        mae_deg=sum(abs_errors) / len(abs_errors),
        rmse_deg=math.sqrt(sum(e * e for e in signed) / len(signed)),
        max_abs_error_deg=worst[3],
        worst_map_kpa=worst[0],
        worst_rpm=worst[1],
        bias_deg=sum(signed) / len(signed),
    )


def generate_candidate_map(example_json: Path, coefficients_json: Path) -> CsvMap:
    map_input = load_input(example_json)
    calculator = SparkAdvanceCalculator(
        map_input.engine,
        load_coefficients(coefficients_json),
    )
    grid = calculator.generate_map(map_input.rpm_axis, map_input.map_axis)
    values = [
        [grid[rpm_idx][map_idx].advance_deg for rpm_idx in range(len(map_input.rpm_axis))]
        for map_idx in reversed(range(len(map_input.map_axis)))
    ]
    file_map_axis = [
        map_input.map_axis[idx] for idx in reversed(range(len(map_input.map_axis)))
    ]
    return CsvMap(
        rpm_axis=map_input.rpm_axis,
        map_axis=file_map_axis,
        values=values,
    )


def reference_path_for(example_json: Path, reference_dir: Path) -> Path:
    return reference_dir / f"{example_json.stem}.csv"


def compare_dataset(
    dataset: ReferenceDataset,
    coefficients_json: Path,
    candidate_csv: Path | None = None,
) -> CompareResult:
    reference = read_csv_map(dataset.reference_csv)
    if candidate_csv:
        candidate = read_csv_map(candidate_csv)
    else:
        candidate = generate_candidate_map(dataset.example_json, coefficients_json)
    return compare_maps(reference, candidate)


def compare_all(
    coefficients_json: Path,
    examples_dir: Path = Path("examples"),
    reference_dir: Path = Path("reference"),
) -> CompareSummary:
    datasets = discover_datasets(examples_dir, reference_dir)
    if not datasets:
        raise FileNotFoundError("no example/reference pairs found")

    per_engine: list[tuple[str, CompareResult]] = []
    all_signed: list[float] = []
    all_abs: list[float] = []
    worst_engine = ""
    worst_map_kpa = 0.0
    worst_rpm = 0.0
    max_abs = -1.0

    for dataset in datasets:
        reference = read_csv_map(dataset.reference_csv)
        candidate = generate_candidate_map(dataset.example_json, coefficients_json)
        result = compare_maps(reference, candidate)
        per_engine.append((dataset.name, result))

        for map_idx, map_kpa in enumerate(reference.map_axis):
            for rpm_idx, rpm in enumerate(reference.rpm_axis):
                err = candidate.values[map_idx][rpm_idx] - reference.values[map_idx][rpm_idx]
                all_signed.append(err)
                all_abs.append(abs(err))
                if abs(err) > max_abs:
                    max_abs = abs(err)
                    worst_engine = dataset.name
                    worst_map_kpa = map_kpa
                    worst_rpm = rpm

    return CompareSummary(
        per_engine=tuple(per_engine),
        total_cells=len(all_signed),
        mae_deg=sum(all_abs) / len(all_abs),
        rmse_deg=math.sqrt(sum(e * e for e in all_signed) / len(all_signed)),
        bias_deg=sum(all_signed) / len(all_signed),
        max_abs_error_deg=max_abs,
        worst_engine=worst_engine,
        worst_map_kpa=worst_map_kpa,
        worst_rpm=worst_rpm,
    )


def format_report(name: str, result: CompareResult) -> str:
    return (
        f"{name}\n"
        f"  cells: {result.cells}\n"
        f"  MAE:   {result.mae_deg:.2f}°\n"
        f"  RMSE:  {result.rmse_deg:.2f}°\n"
        f"  bias:  {result.bias_deg:+.2f}°\n"
        f"  max:   {result.max_abs_error_deg:.2f}° "
        f"@ map={result.worst_map_kpa:.0f} kPa, rpm={result.worst_rpm:.0f}"
    )


def format_summary(summary: CompareSummary) -> str:
    lines = ["=== all engines ==="]
    for name, result in summary.per_engine:
        lines.append(format_report(name, result))
        lines.append("")
    lines.append("=== combined ===")
    lines.append(f"  engines: {len(summary.per_engine)}")
    lines.append(f"  cells:   {summary.total_cells}")
    lines.append(f"  MAE:     {summary.mae_deg:.2f}°")
    lines.append(f"  RMSE:    {summary.rmse_deg:.2f}°")
    lines.append(f"  bias:    {summary.bias_deg:+.2f}°")
    lines.append(
        f"  max:     {summary.max_abs_error_deg:.2f}° "
        f"@ {summary.worst_engine}, map={summary.worst_map_kpa:.0f} kPa, rpm={summary.worst_rpm:.0f}"
    )
    return "\n".join(lines)


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(
        description="Compare generated ignition maps against reference CSVs.",
    )
    parser.add_argument(
        "example",
        type=Path,
        nargs="?",
        help="Single engine JSON (default: all with reference)",
    )
    parser.add_argument(
        "-r",
        "--reference",
        type=Path,
        help="Reference CSV for single-engine mode",
    )
    parser.add_argument(
        "-c",
        "--coefficients",
        type=Path,
        default=Path("config/coefficients.json"),
    )
    parser.add_argument(
        "--candidate",
        type=Path,
        help="Compare this CSV instead of generating from coefficients",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print metrics as JSON",
    )
    args = parser.parse_args()

    if args.example is None:
        summary = compare_all(args.coefficients)
        if args.json:
            print(
                json.dumps(
                    {
                        "combined": {
                            "cells": summary.total_cells,
                            "mae_deg": round(summary.mae_deg, 3),
                            "rmse_deg": round(summary.rmse_deg, 3),
                            "bias_deg": round(summary.bias_deg, 3),
                            "max_abs_error_deg": round(summary.max_abs_error_deg, 3),
                        },
                        "engines": {
                            name: {
                                "mae_deg": round(result.mae_deg, 3),
                                "rmse_deg": round(result.rmse_deg, 3),
                                "bias_deg": round(result.bias_deg, 3),
                                "max_abs_error_deg": round(result.max_abs_error_deg, 3),
                            }
                            for name, result in summary.per_engine
                        },
                    },
                    indent=2,
                )
            )
        else:
            print(format_summary(summary))
        return 0

    reference_path = args.reference or reference_path_for(args.example, Path("reference"))
    reference = read_csv_map(reference_path)

    if args.candidate:
        candidate = read_csv_map(args.candidate)
    else:
        candidate = generate_candidate_map(args.example, args.coefficients)

    result = compare_maps(reference, candidate)

    if args.json:
        print(
            json.dumps(
                {
                    "example": str(args.example),
                    "reference": str(reference_path),
                    "mae_deg": round(result.mae_deg, 3),
                    "rmse_deg": round(result.rmse_deg, 3),
                    "bias_deg": round(result.bias_deg, 3),
                    "max_abs_error_deg": round(result.max_abs_error_deg, 3),
                    "worst_map_kpa": result.worst_map_kpa,
                    "worst_rpm": result.worst_rpm,
                },
                indent=2,
            )
        )
    else:
        print(format_report(reference_path.name, result))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
