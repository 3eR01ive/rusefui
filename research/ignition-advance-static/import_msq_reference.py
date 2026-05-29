#!/usr/bin/env python3
"""Импорт эталонной карты УОЗ из TunerStudio .msq → reference/*.csv."""

from __future__ import annotations

import xml.etree.ElementTree as ET
from pathlib import Path

from model.io import load_input, write_csv_map

MSQ_NS = {"msq": "http://www.msefi.com/:msq"}


def _parse_floats(text: str) -> list[float]:
    return [float(x) for x in text.split()]


def read_msq_ignition(path: Path) -> tuple[list[float], list[float], list[list[float]]]:
    tree = ET.parse(path)
    root = tree.getroot()

    rpm_axis: list[float] | None = None
    map_axis: list[float] | None = None
    values: list[list[float]] | None = None

    for constant in root.findall(".//msq:constant", MSQ_NS):
        name = constant.attrib.get("name")
        if name == "ignitionRpmBins":
            rpm_axis = _parse_floats(constant.text or "")
        elif name == "ignitionLoadBins":
            map_axis = _parse_floats(constant.text or "")
        elif name == "ignitionTable":
            rows = (constant.text or "").strip().splitlines()
            values = [_parse_floats(row) for row in rows if row.strip()]

    if rpm_axis is None or map_axis is None or values is None:
        raise ValueError(f"{path}: ignition table constants not found")
    if len(values) != len(map_axis):
        raise ValueError(
            f"{path}: {len(values)} table rows != {len(map_axis)} load bins"
        )
    for row in values:
        if len(row) != len(rpm_axis):
            raise ValueError(
                f"{path}: row length {len(row)} != {len(rpm_axis)} rpm bins"
            )

    return rpm_axis, map_axis, values


def _interpolate_row(
    target_map: float,
    map_axis: list[float],
    values: list[list[float]],
) -> list[float]:
    if target_map <= map_axis[0]:
        return values[0][:]
    if target_map >= map_axis[-1]:
        return values[-1][:]

    for left_idx in range(len(map_axis) - 1):
        left_map = map_axis[left_idx]
        right_map = map_axis[left_idx + 1]
        if left_map <= target_map <= right_map:
            if right_map == left_map:
                return values[left_idx][:]
            t = (target_map - left_map) / (right_map - left_map)
            left_row = values[left_idx]
            right_row = values[left_idx + 1]
            return [left + t * (right - left) for left, right in zip(left_row, right_row)]

    return values[-1][:]


def align_to_target_axes(
    rpm_axis: list[float],
    map_axis: list[float],
    values: list[list[float]],
    target_rpm: list[float],
    target_map: list[float],
) -> list[list[float]]:
    if rpm_axis != target_rpm:
        raise ValueError(
            f"RPM mismatch: msq {rpm_axis} vs target {target_rpm}"
        )

    by_load = {load: row for load, row in zip(map_axis, values)}
    missing = [load for load in target_map if load not in by_load]
    if missing:
        for load in missing:
            by_load[load] = _interpolate_row(load, map_axis, values)

    file_map_axis = [target_map[idx] for idx in reversed(range(len(target_map)))]
    return [by_load[load] for load in file_map_axis]


def import_reference(
    msq_path: Path,
    example_json: Path,
    output_csv: Path,
) -> None:
    rpm_axis, map_axis, values = read_msq_ignition(msq_path)
    map_input = load_input(example_json)
    aligned = align_to_target_axes(
        rpm_axis,
        map_axis,
        values,
        map_input.rpm_axis,
        map_input.map_axis,
    )
    file_map_axis = [
        map_input.map_axis[idx] for idx in reversed(range(len(map_input.map_axis)))
    ]
    write_csv_map(output_csv, map_input.rpm_axis, file_map_axis, aligned)


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Import ignition reference CSV from .msq")
    parser.add_argument("msq", type=Path)
    parser.add_argument("example", type=Path, help="Example JSON with target axes")
    parser.add_argument("-o", "--output", type=Path, required=True)
    args = parser.parse_args()

    import_reference(args.msq, args.example, args.output)
    print(f"Wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
