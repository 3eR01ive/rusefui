#!/usr/bin/env python3
"""Эталоны и конфиги из известных карт УОЗ (K20, EJ207, Evo VIII stock)."""

from __future__ import annotations

import json
from pathlib import Path

from model.axes import resample_map, standard_map_kpa_list, standard_rpm_list
from model.io import CsvMap, write_csv_map

ROOT = Path(__file__).parent
EXAMPLES = ROOT / "examples"
REFERENCE = ROOT / "reference"
RAW = REFERENCE / "raw"


def write_pair(
    stem: str,
    engine: dict,
    rpm_axis: list[float],
    map_axis: list[float],
    table: list[list[float]],
) -> None:
    """table[map_idx][rpm_idx] — map ascending, rpm ascending."""
    if len(table) != len(map_axis):
        raise ValueError(f"{stem}: map rows {len(table)} != {len(map_axis)}")
    for row in table:
        if len(row) != len(rpm_axis):
            raise ValueError(f"{stem}: rpm cols {len(row)} != {len(rpm_axis)}")

    example = {
        "engine": engine,
        "axes": {
            "rpm": standard_rpm_list(),
            "map_kpa": standard_map_kpa_list(),
        },
    }
    with open(EXAMPLES / f"{stem}.json", "w", encoding="utf-8") as f:
        json.dump(example, f, indent=2, ensure_ascii=False)
        f.write("\n")

    file_map = list(reversed(map_axis))
    values = [table[map_axis.index(m)] for m in file_map]
    native = CsvMap(rpm_axis=rpm_axis, map_axis=file_map, values=values)

    RAW.mkdir(parents=True, exist_ok=True)
    write_csv_map(RAW / f"{stem}.csv", rpm_axis, file_map, values)

    resampled = resample_map(native)
    write_csv_map(REFERENCE / f"{stem}.csv", resampled.rpm_axis, resampled.map_axis, resampled.values)
    print(f"  {stem}: native {len(map_axis)}×{len(rpm_axis)} → standard {len(resampled.map_axis)}×{len(resampled.rpm_axis)}")


def main() -> int:
    EXAMPLES.mkdir(parents=True, exist_ok=True)
    REFERENCE.mkdir(parents=True, exist_ok=True)

    # --- Honda K20 (NA, Hondata-style MAP kPa × RPM) ---
    k20_rpm = [
        500, 800, 1000, 1250, 1500, 1750, 2000, 2250, 2500, 2750,
        3000, 3500, 4000, 4500, 5000, 5500, 5800, 6000, 7000, 8100,
    ]
    k20_map = [11, 20, 28, 40, 51, 63, 74, 85, 96, 102]
    # rows = RPM, cols = MAP in source image → transpose to table[map][rpm]
    k20_by_rpm = [
        [27.00, 25.00, 23.00, 20.00, 17.00, 14.00, 12.00, 11.00, 10.00, 8.00],
        [27.50, 25.50, 23.50, 21.00, 18.00, 15.00, 13.00, 12.00, 10.50, 9.20],
        [10.00, 10.00, 24.00, 21.50, 18.50, 16.00, 14.00, 13.00, 11.50, 10.50],
        [13.50, 13.50, 24.20, 22.40, 20.00, 18.40, 17.00, 15.50, 14.00, 12.80],
        [15.00, 15.00, 25.50, 24.00, 22.30, 21.00, 19.50, 18.50, 17.50, 16.50],
        [15.00, 15.00, 28.20, 26.90, 25.00, 23.10, 21.90, 20.70, 20.00, 19.30],
        [15.00, 15.00, 32.50, 30.00, 27.00, 24.50, 23.50, 22.50, 21.50, 20.50],
        [22.50, 22.50, 38.40, 35.10, 31.10, 27.00, 25.50, 23.50, 22.00, 20.50],
        [30.00, 30.00, 43.00, 39.50, 34.30, 29.00, 27.00, 25.00, 22.50, 21.00],
        [32.50, 32.50, 44.90, 41.30, 37.00, 32.30, 29.70, 27.00, 25.70, 24.00],
        [35.00, 35.00, 45.00, 42.50, 38.00, 34.00, 32.00, 29.00, 28.00, 27.50],
        [35.00, 35.00, 45.00, 43.00, 39.00, 35.00, 33.00, 31.00, 30.30, 29.80],
        [35.00, 35.00, 45.00, 45.00, 40.00, 35.50, 33.50, 31.50, 31.20, 31.00],
        [40.00, 40.00, 45.00, 43.00, 38.50, 33.50, 32.00, 30.50, 30.00, 29.80],
        [45.00, 45.00, 42.00, 37.50, 34.50, 31.00, 29.80, 28.50, 27.30, 26.70],
        [44.00, 41.00, 38.00, 34.50, 32.00, 29.00, 28.00, 27.50, 26.00, 25.00],
        [40.00, 38.00, 36.40, 33.70, 31.60, 29.50, 28.00, 26.80, 25.50, 24.60],
        [39.00, 37.50, 36.00, 34.00, 32.00, 30.00, 28.50, 27.00, 25.80, 25.20],
        [44.00, 42.00, 40.50, 37.00, 34.20, 31.00, 30.50, 30.00, 28.80, 28.20],
        [44.00, 42.00, 40.50, 37.00, 34.20, 31.00, 30.50, 30.00, 28.80, 28.20],
    ]
    k20_table = [[k20_by_rpm[r][m] for r in range(len(k20_rpm))] for m in range(len(k20_map))]

    # --- Subaru EJ207 safe timing (Link ECU, MAP kPa × RPM) ---
    ej207_rpm = [
        0, 500, 750, 1000, 1500, 2000, 2500, 3000, 3500, 4000,
        4500, 5000, 5500, 6000, 6500, 7000, 8000,
    ]
    ej207_map = [0, 20, 40, 60, 80, 100, 120, 140, 160, 180, 200, 220, 240, 260, 280, 300]
    ej207_table = [
        [12.0, 12.0, 12.0, 12.0, 15.0, 22.0, 28.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0],
        [12.0, 12.0, 12.0, 12.0, 15.0, 22.0, 28.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0],
        [12.0, 12.0, 12.0, 10.0, 10.0, 22.0, 28.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0, 32.0],
        [12.0, 12.0, 12.0, 10.0, 10.0, 22.0, 28.0, 30.0, 30.0, 30.0, 30.0, 30.0, 30.0, 30.0, 30.0, 30.0, 30.0],
        [12.0, 12.0, 12.0, 12.0, 18.0, 23.0, 25.0, 30.0, 30.0, 30.0, 30.0, 30.0, 30.0, 30.0, 30.0, 30.0, 30.0],
        [12.0, 12.0, 12.0, 12.0, 18.0, 23.0, 26.0, 30.0, 30.0, 30.0, 29.0, 28.0, 28.0, 28.0, 29.0, 30.0, 30.0],
        [12.0, 12.0, 12.0, 12.0, 15.0, 18.0, 24.0, 28.0, 26.0, 24.0, 24.0, 24.0, 24.0, 24.0, 24.0, 24.0, 24.0],
        [12.0, 12.0, 12.0, 12.0, 15.0, 18.0, 23.0, 26.0, 24.0, 22.0, 22.0, 22.0, 22.0, 22.0, 22.0, 22.0, 22.0],
        [12.0, 12.0, 12.0, 12.0, 15.0, 18.0, 22.0, 18.0, 18.0, 18.0, 19.0, 19.0, 19.0, 19.0, 19.0, 19.0, 19.0],
        [12.0, 12.0, 12.0, 12.0, 15.0, 16.0, 19.0, 18.0, 18.0, 18.0, 18.0, 18.0, 18.0, 18.0, 18.0, 18.0, 18.0],
        [12.0, 12.0, 12.0, 12.0, 15.0, 16.0, 18.0, 18.0, 18.0, 18.0, 17.0, 17.0, 17.0, 17.0, 18.0, 18.0, 18.0],
        [12.0, 12.0, 12.0, 12.0, 15.0, 16.0, 17.0, 16.0, 16.0, 16.0, 15.0, 14.0, 14.0, 14.0, 15.0, 16.0, 16.0],
        [12.0, 12.0, 12.0, 12.0, 15.0, 16.0, 16.0, 16.0, 16.0, 16.0, 15.0, 14.0, 13.0, 13.0, 14.0, 15.0, 15.0],
        [12.0, 12.0, 12.0, 12.0, 15.0, 16.0, 16.0, 16.0, 16.0, 16.0, 15.0, 14.0, 13.0, 13.0, 14.0, 15.0, 15.0],
        [12.0, 12.0, 12.0, 12.0, 15.0, 16.0, 16.0, 16.0, 16.0, 16.0, 15.0, 14.0, 13.0, 13.0, 14.0, 15.0, 15.0],
        [12.0, 12.0, 12.0, 12.0, 15.0, 16.0, 16.0, 16.0, 16.0, 16.0, 15.0, 14.0, 13.0, 13.0, 14.0, 15.0, 15.0],
    ]

    # --- Mitsubishi 4G63 Evo VIII stock (Load % × RPM, load ≈ kPa proxy) ---
    evo8_rpm = [
        0, 500, 750, 1000, 1250, 1500, 1750, 2000, 2500, 3000,
        3500, 4000, 4500, 5000, 5500, 6000, 6500, 7000, 7500, 11000,
    ]
    evo8_load = [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 120, 140, 160, 180, 200, 220, 240, 260]
    evo8_by_rpm = [
        [5, 5, 5, 5, 5, 11, 15, 6, 3, 2, -1, -2, -5, -8, -10, -10, -10, -10, -10],
        [5, 5, 5, 5, 5, 8, 13, 6, 3, 2, -1, -2, -5, -8, -10, -10, -10, -10, -10],
        [5, 5, 5, 5, 5, 9, 13, 10, 6, 2, -2, -3, -5, -8, -10, -10, -10, -10, -10],
        [5, 5, 5, 5, 9, 13, 18, 10, 8, 7, 7, 4, 1, -3, -6, -10, -10, -10, -10],
        [8, 8, 12, 12, 16, 20, 20, 17, 15, 9, 8, 4, 2, -1, -4, -7, -10, -10, -10],
        [13, 13, 19, 19, 26, 24, 23, 21, 19, 14, 12, 10, 7, 4, 1, -2, -5, -8, -10],
        [18, 18, 25, 25, 27, 25, 24, 23, 21, 17, 14, 11, 8, 5, 2, -1, -4, -7, -9],
        [24, 24, 32, 32, 29, 26, 25, 24, 21, 17, 14, 12, 10, 7, 4, 1, -2, -5, -8],
        [24, 24, 34, 34, 32, 30, 29, 27, 25, 20, 20, 15, 9, 7, 4, 1, -2, -5, -8],
        [28, 28, 38, 38, 35, 32, 30, 29, 28, 26, 22, 18, 12, 7, 6, 5, 1, -2, -5],
        [28, 28, 38, 38, 35, 32, 31, 30, 28, 27, 25, 20, 15, 11, 8, 7, 5, 2, -1],
        [28, 28, 38, 38, 35, 32, 31, 30, 28, 27, 25, 20, 15, 12, 9, 8, 7, 3, 0],
        [33, 33, 38, 38, 35, 32, 31, 30, 28, 27, 25, 20, 16, 13, 10, 8, 5, 2, -1],
        [38, 38, 38, 38, 35, 32, 31, 30, 28, 27, 25, 20, 16, 12, 10, 10, 7, 4, 1],
        [38, 38, 38, 38, 35, 32, 31, 30, 28, 27, 25, 20, 15, 13, 11, 9, 9, 6, 3],
        [38, 38, 38, 38, 35, 34, 32, 31, 30, 28, 26, 23, 18, 16, 14, 13, 10, 7, 4],
        [38, 38, 38, 38, 38, 37, 34, 34, 34, 32, 30, 27, 22, 20, 18, 15, 12, 9, 6],
        [38, 38, 38, 38, 38, 37, 36, 35, 35, 34, 34, 31, 26, 24, 21, 18, 15, 12, 9],
        [38, 38, 38, 38, 38, 37, 36, 35, 35, 34, 34, 31, 26, 24, 21, 18, 15, 12, 9],
        [38, 38, 38, 38, 38, 37, 36, 35, 35, 34, 31, 26, 24, 21, 18, 15, 12, 9, 9],
    ]
    evo8_table = [[evo8_by_rpm[r][l] for r in range(len(evo8_rpm))] for l in range(len(evo8_load))]

    print("Writing examples + references:")
    write_pair(
        "na_k20_honda",
        {
            "bore_mm": 86.0,
            "stroke_mm": 86.0,
            "rod_length_mm": 137.0,
            "cylinder_count": 4,
            "displacement_cc": 1998,
            "compression_ratio": 11.0,
            "valves_per_cylinder": 4,
            "spark_location": "center",
            "chamber_type": "pentroof",
            "intake_duration_deg": 240,
            "exhaust_duration_deg": 240,
            "overlap_deg": 25,
            "fuel": "gasoline_98",
            "aspiration": "naturally_aspirated",
        },
        k20_rpm,
        k20_map,
        k20_table,
    )
    write_pair(
        "turbo_ej207_subaru",
        {
            "bore_mm": 92.0,
            "stroke_mm": 75.0,
            "rod_length_mm": 130.0,
            "cylinder_count": 4,
            "displacement_cc": 1994,
            "compression_ratio": 8.0,
            "valves_per_cylinder": 4,
            "spark_location": "center",
            "chamber_type": "pentroof",
            "intake_duration_deg": 240,
            "exhaust_duration_deg": 240,
            "overlap_deg": 30,
            "fuel": "gasoline_98",
            "aspiration": "turbocharged",
        },
        ej207_rpm,
        ej207_map,
        ej207_table,
    )
    write_pair(
        "turbo_4g63_evo8_stock",
        {
            "bore_mm": 85.0,
            "stroke_mm": 88.0,
            "rod_length_mm": 150.0,
            "cylinder_count": 4,
            "displacement_cc": 1997,
            "compression_ratio": 8.5,
            "valves_per_cylinder": 4,
            "spark_location": "center",
            "chamber_type": "pentroof",
            "intake_duration_deg": 264,
            "exhaust_duration_deg": 264,
            "overlap_deg": 45,
            "fuel": "gasoline_98",
            "aspiration": "turbocharged",
        },
        evo8_rpm,
        evo8_load,
        evo8_table,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
