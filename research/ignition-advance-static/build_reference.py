#!/usr/bin/env python3
"""Собрать эталонный CSV из якорных строк (линейная интерполяция по MAP и RPM)."""

from __future__ import annotations

from pathlib import Path

from model.io import load_input, write_csv_map


def _lerp(x: float, x0: float, x1: float, y0: float, y1: float) -> float:
    if x1 == x0:
        return y0
    t = (x - x0) / (x1 - x0)
    return y0 + t * (y1 - y0)


def _row_at_rpm(anchors: dict[float, tuple[float, float]], rpm: float) -> float:
    rpms = sorted(anchors)
    if rpm <= rpms[0]:
        low, high = anchors[rpms[0]]
        return low
    if rpm >= rpms[-1]:
        low, high = anchors[rpms[-1]]
        return high

    for left, right in zip(rpms, rpms[1:]):
        if left <= rpm <= right:
            low_left, high_left = anchors[left]
            low_right, high_right = anchors[right]
            return _lerp(rpm, left, right, low_left, low_right)

    low, high = anchors[rpms[-1]]
    return high


def _value_at(map_kpa: float, rpm: float, load_anchors: dict[float, dict[float, tuple[float, float]]]) -> float:
    maps = sorted(load_anchors)
    row_values = {load: _row_at_rpm(anchors, rpm) for load, anchors in load_anchors.items()}

    if map_kpa <= maps[0]:
        return row_values[maps[0]]
    if map_kpa >= maps[-1]:
        return row_values[maps[-1]]

    for left, right in zip(maps, maps[1:]):
        if left <= map_kpa <= right:
            return _lerp(map_kpa, left, right, row_values[left], row_values[right])

    return row_values[maps[-1]]


# Якорные строки из дефолтной таблицы TunerStudio (скриншот rusEFI).
TS_DEFAULT_ANCHORS: dict[float, dict[float, tuple[float, float]]] = {
    15.0: {600.0: (16.5, 16.5), 8000.0: (44.0, 44.0)},
    100.0: {600.0: (13.5, 13.5), 4000.0: (21.8, 21.8), 8000.0: (31.0, 31.0)},
    110.0: {4000.0: (18.9, 18.9)},
    130.0: {4000.0: (12.8, 12.8)},
    320.0: {600.0: (-9.5, -9.5), 8000.0: (5.1, 5.1)},
}


def build_reference_values(rpm_axis: list[float], map_axis: list[float]) -> list[list[float]]:
    file_map_axis = [map_axis[idx] for idx in reversed(range(len(map_axis)))]
    return [
        [
            round(_value_at(map_kpa, rpm, TS_DEFAULT_ANCHORS), 1)
            for rpm in rpm_axis
        ]
        for map_kpa in file_map_axis
    ]


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Build bootstrap reference CSV from TS anchors.")
    parser.add_argument("example", type=Path, help="Example JSON with axes")
    parser.add_argument("-o", "--output", type=Path, required=True)
    args = parser.parse_args()

    map_input = load_input(args.example)
    values = build_reference_values(map_input.rpm_axis, map_input.map_axis)
    file_map_axis = [
        map_input.map_axis[idx] for idx in reversed(range(len(map_input.map_axis)))
    ]
    write_csv_map(args.output, map_input.rpm_axis, file_map_axis, values)
    print(f"Wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
