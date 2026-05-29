#!/usr/bin/env python3
"""CLI: JSON с параметрами ДВС и осями → CSV с картой УОЗ."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from model import SparkAdvanceCalculator, load_coefficients, load_input, write_csv
from model.datasets import discover_datasets

DEFAULT_COEFFICIENTS = Path(__file__).parent / "config" / "coefficients.json"


def generate_one(input_path: Path, output_path: Path, coefficients_path: Path, diagnostics: bool) -> None:
    map_input = load_input(input_path)
    coefficients = load_coefficients(coefficients_path)
    calculator = SparkAdvanceCalculator(map_input.engine, coefficients)
    grid = calculator.generate_map(map_input.rpm_axis, map_input.map_axis)

    write_csv(output_path, map_input.rpm_axis, map_input.map_axis, grid)

    if diagnostics:
        print(json.dumps(calculator.diagnostics, indent=2), file=sys.stderr)

    seen_warnings: set[str] = set()
    for row in grid:
        for cell in row:
            for warning in cell.warnings:
                if warning not in seen_warnings:
                    print(f"WARNING: {warning}", file=sys.stderr)
                    seen_warnings.add(warning)

    print(f"Wrote {output_path}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Generate static ignition advance map (CSV) from engine JSON.",
    )
    parser.add_argument(
        "input",
        type=Path,
        nargs="?",
        help="Input JSON path (omit with --all)",
    )
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        help="Output CSV path (default: output/<input_stem>.csv)",
    )
    parser.add_argument(
        "-c",
        "--coefficients",
        type=Path,
        default=DEFAULT_COEFFICIENTS,
        help=f"Coefficients JSON (default: {DEFAULT_COEFFICIENTS})",
    )
    parser.add_argument(
        "--all",
        action="store_true",
        help="Generate maps for all examples with reference CSV",
    )
    parser.add_argument(
        "--diagnostics",
        action="store_true",
        help="Print intermediate model values as JSON to stderr",
    )
    args = parser.parse_args()

    if args.all:
        datasets = discover_datasets()
        if not datasets:
            print("No example/reference pairs found", file=sys.stderr)
            return 1
        for dataset in datasets:
            output = Path("output") / f"{dataset.name}.csv"
            generate_one(dataset.example_json, output, args.coefficients, args.diagnostics)
        return 0

    if args.input is None:
        parser.error("input JSON required unless --all is set")

    output = args.output or Path("output") / f"{args.input.stem}.csv"
    generate_one(args.input, output, args.coefficients, args.diagnostics)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
