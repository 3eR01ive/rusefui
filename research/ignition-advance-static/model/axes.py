"""Стандартные оси (эталон — 3000GT) и ресемплинг карт."""

from __future__ import annotations

from model.io import CsvMap, read_csv_map, write_csv_map

# Эталон: examples/turbo_6g72tt_3000gt.json
STANDARD_RPM: tuple[float, ...] = (
    600, 1000, 1500, 2000, 2500, 3000, 3500, 4000, 4500, 5000,
    5500, 6000, 6500, 7000, 7500, 8000,
)

STANDARD_MAP_KPA: tuple[float, ...] = (
    15, 30, 45, 60, 70, 80, 90, 100, 110, 130,
    160, 190, 210, 240, 270, 300, 320,
)


def standard_rpm_list() -> list[float]:
    return list(STANDARD_RPM)


def standard_map_kpa_list() -> list[float]:
    return list(STANDARD_MAP_KPA)


def _lerp(x: float, x0: float, x1: float, y0: float, y1: float) -> float:
    if x1 == x0:
        return y0
    t = (x - x0) / (x1 - x0)
    return y0 + t * (y1 - y0)


def _bracket(axis: list[float], value: float) -> tuple[int, int]:
    if value <= axis[0]:
        return 0, 0
    if value >= axis[-1]:
        n = len(axis) - 1
        return n, n
    for i in range(len(axis) - 1):
        if axis[i] <= value <= axis[i + 1]:
            return i, i + 1
    n = len(axis) - 1
    return n, n


def _ascending_map_grid(source: CsvMap) -> tuple[list[float], list[float], list[list[float]]]:
    rpm_axis = source.rpm_axis[:]
    pairs = sorted(zip(source.map_axis, source.values), key=lambda item: item[0])
    map_axis = [p[0] for p in pairs]
    values = [p[1] for p in pairs]
    return rpm_axis, map_axis, values


def sample_map(source: CsvMap, map_kpa: float, rpm: float) -> float:
    rpm_axis, map_axis, values = _ascending_map_grid(source)

    mi0, mi1 = _bracket(map_axis, map_kpa)
    ri0, ri1 = _bracket(rpm_axis, rpm)

    v00 = values[mi0][ri0]
    v01 = values[mi0][ri1]
    v10 = values[mi1][ri0]
    v11 = values[mi1][ri1]

    m0, m1 = map_axis[mi0], map_axis[mi1]
    r0, r1 = rpm_axis[ri0], rpm_axis[ri1]

    if mi0 == mi1 and ri0 == ri1:
        return v00
    if mi0 == mi1:
        return _lerp(rpm, r0, r1, v00, v01)
    if ri0 == ri1:
        return _lerp(map_kpa, m0, m1, v00, v10)

    v_m0 = _lerp(rpm, r0, r1, v00, v01)
    v_m1 = _lerp(rpm, r0, r1, v10, v11)
    return _lerp(map_kpa, m0, m1, v_m0, v_m1)


def resample_map(
    source: CsvMap,
    rpm_axis: list[float] | None = None,
    map_kpa_axis: list[float] | None = None,
) -> CsvMap:
    rpm_axis = rpm_axis or standard_rpm_list()
    map_kpa_axis = map_kpa_axis or standard_map_kpa_list()

    file_map = list(reversed(map_kpa_axis))
    values = [
        [sample_map(source, map_kpa, rpm) for rpm in rpm_axis]
        for map_kpa in file_map
    ]
    return CsvMap(rpm_axis=rpm_axis, map_axis=file_map, values=values)


def same_axes_as_standard(source: CsvMap) -> bool:
    return (
        source.rpm_axis == standard_rpm_list()
        and source.map_axis == list(reversed(standard_map_kpa_list()))
    )


def resample_csv_file(
    input_path: str,
    output_path: str | None = None,
) -> CsvMap:
    source = read_csv_map(input_path)
    resampled = resample_map(source)
    write_csv_map(output_path or input_path, resampled.rpm_axis, resampled.map_axis, resampled.values)
    return resampled
