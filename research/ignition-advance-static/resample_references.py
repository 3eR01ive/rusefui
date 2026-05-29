#!/usr/bin/env python3
"""Привести все эталоны и example JSON к осям 3000GT (бilinear resample)."""

from __future__ import annotations

import json
from pathlib import Path

from model.axes import (
    same_axes_as_standard,
    standard_map_kpa_list,
    standard_rpm_list,
)
from model.datasets import discover_datasets
from model.io import read_csv_map, write_csv_map
from model.axes import resample_map

ROOT = Path(__file__).parent
RAW_DIR = ROOT / "reference" / "raw"


def update_example_axes(example_json: Path) -> None:
    with open(example_json, encoding="utf-8") as f:
        data = json.load(f)
    data["axes"] = {
        "rpm": standard_rpm_list(),
        "map_kpa": standard_map_kpa_list(),
    }
    with open(example_json, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")


def resample_dataset(name: str, reference_csv: Path, example_json: Path, keep_raw: bool) -> bool:
    source = read_csv_map(reference_csv)
    if same_axes_as_standard(source):
        update_example_axes(example_json)
        print(f"  {name}: already on standard grid")
        return False

    if keep_raw:
        RAW_DIR.mkdir(parents=True, exist_ok=True)
        raw_path = RAW_DIR / reference_csv.name
        if not raw_path.exists():
            raw_path.write_bytes(reference_csv.read_bytes())
            print(f"  {name}: archived raw → {raw_path.relative_to(ROOT)}")

    resampled = resample_map(source)
    write_csv_map(reference_csv, resampled.rpm_axis, resampled.map_axis, resampled.values)
    update_example_axes(example_json)
    print(
        f"  {name}: {len(source.map_axis)}×{len(source.rpm_axis)} "
        f"→ {len(resampled.map_axis)}×{len(resampled.rpm_axis)}"
    )
    return True


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Resample all references to 3000GT axes.")
    parser.add_argument(
        "--no-raw",
        action="store_true",
        help="Do not archive original CSV to reference/raw/",
    )
    args = parser.parse_args()

    datasets = discover_datasets()
    if not datasets:
        print("No datasets found")
        return 1

    print(f"Standard grid: {len(standard_map_kpa_list())}×{len(standard_rpm_list())} "
          f"(MAP {standard_map_kpa_list()[0]}–{standard_map_kpa_list()[-1]} kPa, "
          f"RPM {standard_rpm_list()[0]}–{standard_rpm_list()[-1]})")
    print("Resampling:")
    changed = 0
    for dataset in datasets:
        if resample_dataset(
            dataset.name,
            dataset.reference_csv,
            dataset.example_json,
            keep_raw=not args.no_raw,
        ):
            changed += 1

    print(f"\nDone: {changed} resampled, {len(datasets) - changed} unchanged")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
