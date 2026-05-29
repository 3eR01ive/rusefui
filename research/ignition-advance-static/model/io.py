"""Чтение входного JSON и запись CSV."""

from __future__ import annotations

import csv
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .calculator import SparkCell
from .engine import EngineParams


@dataclass(frozen=True)
class MapInput:
    engine: EngineParams
    rpm_axis: list[float]
    map_axis: list[float]


@dataclass(frozen=True)
class CsvMap:
    rpm_axis: list[float]
    map_axis: list[float]
    values: list[list[float]]

    @property
    def shape(self) -> tuple[int, int]:
        return len(self.map_axis), len(self.rpm_axis)


def load_input(path: Path | str) -> MapInput:
    with open(path, encoding="utf-8") as f:
        data: dict[str, Any] = json.load(f)

    axes = data["axes"]
    return MapInput(
        engine=EngineParams.from_dict(data["engine"]),
        rpm_axis=[float(v) for v in axes["rpm"]],
        map_axis=[float(v) for v in axes["map_kpa"]],
    )


def read_csv_map(path: Path | str) -> CsvMap:
    path = Path(path)
    with open(path, newline="", encoding="utf-8") as f:
        rows = list(csv.reader(f))

    if len(rows) < 2:
        raise ValueError(f"{path}: expected header and at least one data row")

    header = rows[0]
    if len(header) < 2:
        raise ValueError(f"{path}: expected RPM values in header")

    rpm_axis = [float(v) for v in header[1:]]
    map_axis: list[float] = []
    values: list[list[float]] = []

    for row in rows[1:]:
        if not row:
            continue
        map_axis.append(float(row[0]))
        if len(row) - 1 != len(rpm_axis):
            raise ValueError(
                f"{path}: row for map {row[0]} has {len(row) - 1} cells, "
                f"expected {len(rpm_axis)}"
            )
        values.append([float(v) for v in row[1:]])

    return CsvMap(rpm_axis=rpm_axis, map_axis=map_axis, values=values)


def write_csv_map(
    path: Path | str,
    rpm_axis: list[float],
    map_axis: list[float],
    values: list[list[float]],
) -> None:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)

    if len(values) != len(map_axis):
        raise ValueError("values row count must match map_axis length")
    for row in values:
        if len(row) != len(rpm_axis):
            raise ValueError("each values row must match rpm_axis length")

    with open(path, "w", newline="", encoding="utf-8") as f:
        writer = csv.writer(f)
        writer.writerow(["map_kpa \\ rpm", *[f"{r:.0f}" for r in rpm_axis]])
        for map_kpa, row in zip(map_axis, values):
            writer.writerow([f"{map_kpa:.0f}", *[f"{v:.1f}" for v in row]])


def write_csv(
    path: Path | str,
    rpm_axis: list[float],
    map_axis: list[float],
    grid: list[list[SparkCell]],
) -> None:
    values = [
        [grid[rpm_idx][map_idx].advance_deg for rpm_idx in range(len(rpm_axis))]
        for map_idx in reversed(range(len(map_axis)))
    ]
    file_map_axis = [map_axis[idx] for idx in reversed(range(len(map_axis)))]
    write_csv_map(path, rpm_axis, file_map_axis, values)
