"""Статическая модель VE: VE(rpm, map) = VE_rpm × LoadFactor × BoostFactor."""

from __future__ import annotations

import math
from dataclasses import dataclass

from .engine import EngineParameters

REFERENCE_FLOW_INDEX = 350_000.0

VE_PEAK_BASE = 0.75
VE_PEAK_FLOW_SCALE = 0.25

VE_PEAK_MIN = 0.70
VE_PEAK_MAX_NA = 1.10
VE_PEAK_MAX_TURBO = 1.20

RPM_PEAK_BASE = 1500.0
# Advertised duration ~220–240° — норма; в ТЗ ×20 на полную длительность даёт 5k+ об/мин (см. README).
CAM_DURATION_REFERENCE_DEG = 220.0
RPM_PEAK_PER_CAM_EXCESS_DEG = 12.0
RUNNER_LENGTH_REFERENCE_MM = 350.0
RPM_PEAK_PER_RUNNER_MM = 5.0
RPM_PEAK_MIN_FRACTION = 0.32
RPM_PEAK_MAX_FRACTION = 0.72
RPM_PEAK_HARD_CAP_FRACTION = 0.95

SIGMA_BASE = 1200.0
SIGMA_LSA_REFERENCE_DEG = 106.0
SIGMA_PER_LSA_DEG = 150.0
SIGMA_MIN = 800.0
SIGMA_MAX = 3000.0

IDLE_BLEND_FLOOR = 0.35
IDLE_BLEND_CURVE = 0.65

LOAD_EXPONENT_NA = 0.65
LOAD_EXPONENT_TURBO = 0.85
LOAD_REFERENCE_KPA = 100.0

BOOST_THRESHOLD_KPA = 100.0
BOOST_GAIN_PER_100_KPA = 0.25

VE_CELL_MAX = 1.50
VE_CELL_MIN = 0.0


def _clamp(value: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, value))


@dataclass(frozen=True)
class VeCell:
    rpm: float
    map_kpa: float
    ve: float
    warnings: tuple[str, ...]


class VeMapCalculator:
    """Генератор VE по геометрии и распредвалу (без логов и обучения)."""

    def __init__(self, engine: EngineParameters) -> None:
        self.engine = engine
        self._flow_index = self._compute_flow_index(engine)
        self._flow_ratio = self._flow_index / REFERENCE_FLOW_INDEX
        self._ve_peak = self._compute_ve_peak()
        self._rpm_peak = self._compute_rpm_peak()
        self._sigma = self._compute_sigma()
        self._load_exponent = (
            LOAD_EXPONENT_TURBO if engine.is_turbo else LOAD_EXPONENT_NA
        )

    @staticmethod
    def _compute_flow_index(engine: EngineParameters) -> float:
        d = engine.intake_valve_diameter_mm
        valve_area = math.pi * d * d / 4.0
        return valve_area * engine.cam_lift_mm * engine.cam_duration_deg

    def _compute_ve_peak(self) -> float:
        flow_term = _clamp(self._flow_ratio, 0.0, 1.0)
        ve_peak = VE_PEAK_BASE + VE_PEAK_FLOW_SCALE * flow_term
        peak_max = VE_PEAK_MAX_TURBO if self.engine.is_turbo else VE_PEAK_MAX_NA
        return _clamp(ve_peak, VE_PEAK_MIN, peak_max)

    def _compute_rpm_peak(self) -> float:
        e = self.engine
        cam_excess = max(0.0, e.cam_duration_deg - CAM_DURATION_REFERENCE_DEG)
        runner_term = (
            RUNNER_LENGTH_REFERENCE_MM - e.intake_runner_length_mm
        ) * RPM_PEAK_PER_RUNNER_MM
        rpm_peak = (
            RPM_PEAK_BASE
            + cam_excess * RPM_PEAK_PER_CAM_EXCESS_DEG
            + runner_term
        )
        rpm_peak = _clamp(
            rpm_peak,
            e.max_rpm * RPM_PEAK_MIN_FRACTION,
            e.max_rpm * RPM_PEAK_MAX_FRACTION,
        )
        return min(rpm_peak, e.max_rpm * RPM_PEAK_HARD_CAP_FRACTION)

    def _compute_sigma(self) -> float:
        sigma = (
            SIGMA_BASE
            + (self.engine.cam_lsa_deg - SIGMA_LSA_REFERENCE_DEG) * SIGMA_PER_LSA_DEG
        )
        return _clamp(sigma, SIGMA_MIN, SIGMA_MAX)

    def _ve_rpm_raw(self, rpm: float) -> float:
        if self._sigma <= 0:
            return self._ve_peak
        exponent = -((rpm - self._rpm_peak) ** 2) / (2.0 * self._sigma * self._sigma)
        return self._ve_peak * math.exp(exponent)

    def ve_rpm_at(self, rpm: float) -> float:
        curve = self._ve_rpm_raw(rpm)
        return IDLE_BLEND_FLOOR * self._ve_peak + IDLE_BLEND_CURVE * curve

    def load_factor_at(self, map_kpa: float) -> float:
        if map_kpa <= 0:
            return 0.0
        return (map_kpa / LOAD_REFERENCE_KPA) ** self._load_exponent

    def boost_factor_at(self, map_kpa: float) -> float:
        if not self.engine.is_turbo or map_kpa <= BOOST_THRESHOLD_KPA:
            return 1.0
        return 1.0 + (map_kpa - BOOST_THRESHOLD_KPA) / LOAD_REFERENCE_KPA * BOOST_GAIN_PER_100_KPA

    def _plausibility_warnings(self, rpm: float, map_kpa: float, ve: float) -> tuple[str, ...]:
        warnings: list[str] = []
        ve_pct = ve * 100.0
        if ve < 0.15 and map_kpa >= 80:
            warnings.append(
                f"VE {ve_pct:.1f}% very low at load (rpm={rpm:.0f}, map={map_kpa:.0f} kPa)"
            )
        if ve > 1.35 and not self.engine.is_turbo:
            warnings.append(
                f"VE {ve_pct:.1f}% high for NA (rpm={rpm:.0f}, map={map_kpa:.0f} kPa)"
            )
        if rpm <= 800 and ve < 0.25:
            warnings.append(
                f"VE {ve_pct:.1f}% may be too low for idle (rpm={rpm:.0f}, map={map_kpa:.0f} kPa)"
            )
        return tuple(warnings)

    def ve_at(self, rpm: float, map_kpa: float) -> VeCell:
        ve = self.ve_rpm_at(rpm) * self.load_factor_at(map_kpa) * self.boost_factor_at(map_kpa)
        ve = _clamp(ve, VE_CELL_MIN, VE_CELL_MAX)
        warnings = self._plausibility_warnings(rpm, map_kpa, ve)
        return VeCell(
            rpm=rpm,
            map_kpa=map_kpa,
            ve=round(ve, 2),
            warnings=warnings,
        )

    def generate_map(
        self,
        rpm_axis: list[float],
        map_axis: list[float],
    ) -> list[list[VeCell]]:
        return [
            [self.ve_at(rpm, map_kpa) for map_kpa in map_axis]
            for rpm in rpm_axis
        ]

    @property
    def diagnostics(self) -> dict[str, float | bool]:
        e = self.engine
        cam_excess = max(0.0, e.cam_duration_deg - CAM_DURATION_REFERENCE_DEG)
        runner_term = (
            RUNNER_LENGTH_REFERENCE_MM - e.intake_runner_length_mm
        ) * RPM_PEAK_PER_RUNNER_MM
        return {
            "flow_index": round(self._flow_index, 2),
            "flow_ratio": round(self._flow_ratio, 4),
            "ve_peak": round(self._ve_peak, 3),
            "rpm_peak": round(self._rpm_peak, 0),
            "rpm_peak_cam_excess_deg": round(cam_excess, 1),
            "rpm_peak_runner_term": round(runner_term, 0),
            "sigma": round(self._sigma, 0),
            "load_exponent": self._load_exponent,
            "is_turbo": self.engine.is_turbo,
        }
