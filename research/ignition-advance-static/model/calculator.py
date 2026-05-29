"""Расчёт карты УОЗ по статической модели."""

from __future__ import annotations

import math
from dataclasses import dataclass

from .coefficients import ModelCoefficients
from .engine import EngineParams


@dataclass(frozen=True)
class SparkCell:
    rpm: float
    map_kpa: float
    advance_deg: float
    warnings: tuple[str, ...]


class SparkAdvanceCalculator:
    def __init__(
        self,
        engine: EngineParams,
        coefficients: ModelCoefficients,
    ) -> None:
        self.engine = engine
        self.coef = coefficients
        self._boost_scale = coefficients.boost_scale(engine.aspiration)
        self._burn_index = self._compute_burn_index()
        self._burn_duration = (
            coefficients.burn_duration_ref_deg * self._burn_index
        )
        self._flame_delay = self._compute_flame_delay()
        self._mbt = self._compute_mbt()
        self._fuel_factor = coefficients.fuel_factors.get(engine.fuel, 1.0)

    def _factor(
        self,
        mapping: dict,
        key: str | int,
        label: str,
    ) -> float:
        if key not in mapping:
            raise KeyError(f"Unknown {label}: {key!r}")
        return float(mapping[key])

    def _compute_burn_index(self) -> float:
        c = self.coef
        e = self.engine

        chamber = self._factor(c.chamber_factors, e.chamber_type, "chamber_type")
        spark = self._factor(c.spark_factors, e.spark_location, "spark_location")
        valves = self._factor(c.valve_factors, e.valves_per_cylinder, "valves_per_cylinder")

        bore_term = (e.bore_mm / c.bore_reference_mm) ** c.bore_exponent
        cr_term = (c.compression_reference / e.compression_ratio) ** c.compression_exponent

        return chamber * spark * valves * bore_term * cr_term

    def _compute_flame_delay(self) -> float:
        delay = self.coef.flame_delay_base_deg
        cr = self.engine.compression_ratio
        for correction in self.coef.flame_delay_cr_corrections:
            if cr > correction.min_cr:
                delay += correction.delta_deg
        return delay

    def _compute_mbt(self) -> float:
        return (
            self._burn_duration / 2.0
            + self._flame_delay
            - self.coef.peak_pressure_target_deg
        )

    def _rpm_correction(self, rpm: float) -> float:
        c = self.coef
        if rpm <= 0:
            return 0.0
        return c.rpm_correction_factor * math.log2(rpm / c.rpm_reference)

    def _load_correction(self, map_kpa: float) -> float:
        c = self.coef
        ref = c.load_reference_map_kpa

        if map_kpa <= ref:
            steps = (ref - map_kpa) / 10.0
            return steps * c.vacuum_deg_per_10_kpa

        if self._boost_scale <= 0:
            return 0.0

        boost_bar = (map_kpa - ref) / 100.0
        steps = boost_bar / 0.1
        return steps * c.boost_deg_per_0_1_bar * self._boost_scale

    def _cam_retard(self) -> float:
        overlap = self.engine.overlap_deg
        if overlap is None:
            return 0.0
        delta = overlap - self.coef.stock_overlap_deg
        if delta <= 0:
            return 0.0
        return delta * self.coef.overlap_retard_per_deg

    def _apply_limits(self, rpm: float, map_kpa: float, advance: float) -> float:
        c = self.coef

        if map_kpa >= c.wot_map_threshold_kpa:
            advance = min(advance, c.max_wot_deg)
        else:
            advance = min(advance, c.max_partial_load_deg)

        if rpm <= c.idle_rpm_max and map_kpa <= c.idle_map_max_kpa:
            advance = max(advance, c.min_idle_deg)

        return max(advance, c.min_advance_deg)

    def _plausibility_warnings(
        self,
        rpm: float,
        map_kpa: float,
        advance: float,
    ) -> tuple[str, ...]:
        c = self.coef
        e = self.engine
        warnings: list[str] = []

        is_wot = map_kpa >= c.wot_map_threshold_kpa
        is_idle = rpm <= c.idle_rpm_max and map_kpa <= c.idle_map_max_kpa

        if is_wot and advance > c.plausibility_max_wot_deg:
            warnings.append(
                f"WOT advance {advance:.1f}° > {c.plausibility_max_wot_deg}° "
                f"(rpm={rpm:.0f}, map={map_kpa:.0f} kPa)"
            )

        if e.is_forced_induction and advance > c.plausibility_max_turbo_deg:
            warnings.append(
                f"Turbo advance {advance:.1f}° > {c.plausibility_max_turbo_deg}° "
                f"(rpm={rpm:.0f}, map={map_kpa:.0f} kPa)"
            )

        if is_idle and advance > c.plausibility_max_idle_deg:
            warnings.append(
                f"Idle advance {advance:.1f}° > {c.plausibility_max_idle_deg}° "
                f"(rpm={rpm:.0f}, map={map_kpa:.0f} kPa)"
            )

        if advance < c.plausibility_min_operating_deg:
            warnings.append(
                f"Advance {advance:.1f}° < {c.plausibility_min_operating_deg}° "
                f"(rpm={rpm:.0f}, map={map_kpa:.0f} kPa)"
            )

        return tuple(warnings)

    def advance_at(self, rpm: float, map_kpa: float) -> SparkCell:
        advance = (
            self._mbt
            + self._rpm_correction(rpm)
            + self._load_correction(map_kpa)
            - self._cam_retard()
        )
        advance *= self._fuel_factor
        advance = self._apply_limits(rpm, map_kpa, advance)
        warnings = self._plausibility_warnings(rpm, map_kpa, advance)

        return SparkCell(
            rpm=rpm,
            map_kpa=map_kpa,
            advance_deg=round(advance, 1),
            warnings=warnings,
        )

    def generate_map(
        self,
        rpm_axis: list[float],
        map_axis: list[float],
    ) -> list[list[SparkCell]]:
        return [
            [self.advance_at(rpm, map_kpa) for map_kpa in map_axis]
            for rpm in rpm_axis
        ]

    @property
    def diagnostics(self) -> dict[str, float]:
        return {
            "burn_index": round(self._burn_index, 4),
            "burn_duration_deg": round(self._burn_duration, 2),
            "flame_delay_deg": round(self._flame_delay, 2),
            "mbt_deg": round(self._mbt, 2),
            "fuel_factor": self._fuel_factor,
            "cam_retard_deg": round(self._cam_retard(), 2),
        }
