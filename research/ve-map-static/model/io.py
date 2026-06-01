"""Чтение входного JSON и запись CSV."""

from __future__ import annotations

import csv
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .calculator import VeCell
from .engine import EngineParameters


@dataclass(frozen=True)
class MapInput:
    engine: EngineParameters
    rpm_axis: list[float]
    map_axis: list[float]


def load_input(path: Path | str) -> MapInput:
    with open(path, encoding="utf-8") as f:
        data: dict[str, Any] = json.load(f)

    axes = data["axes"]
    return MapInput(
        engine=EngineParameters.from_dict(data["engine"]),
        rpm_axis=[float(v) for v in axes["rpm"]],
        map_axis=[float(v) for v in axes["map_kpa"]],
    )


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
            writer.writerow(
                [f"{map_kpa:.0f}", *[f"{v * 100:.1f}" for v in row]],
            )


def write_csv(
    path: Path | str,
    rpm_axis: list[float],
    map_axis: list[float],
    grid: list[list[VeCell]],
) -> None:
    values = [
        [grid[rpm_idx][map_idx].ve for rpm_idx in range(len(rpm_axis))]
        for map_idx in reversed(range(len(map_axis)))
    ]
    file_map_axis = [map_axis[idx] for idx in reversed(range(len(map_axis)))]
    write_csv_map(path, rpm_axis, file_map_axis, values)
