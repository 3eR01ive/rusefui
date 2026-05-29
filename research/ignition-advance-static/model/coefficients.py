"""Загрузка и доступ к коэффициентам модели."""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass(frozen=True)
class CrCorrection:
    min_cr: float
    delta_deg: float


@dataclass(frozen=True)
class ModelCoefficients:
    """Все настраиваемые константы модели."""

    reference_bore_mm: float
    reference_compression_ratio: float
    burn_duration_ref_deg: float

    chamber_factors: dict[str, float]
    spark_factors: dict[str, float]
    valve_factors: dict[int, float]

    bore_reference_mm: float
    bore_exponent: float
    compression_reference: float
    compression_exponent: float

    flame_delay_base_deg: float
    flame_delay_cr_corrections: tuple[CrCorrection, ...]

    peak_pressure_target_deg: float

    rpm_correction_factor: float
    rpm_reference: float

    load_reference_map_kpa: float
    vacuum_deg_per_10_kpa: float
    boost_deg_per_0_1_bar: float
    boost_aspiration_scale: dict[str, float]

    fuel_factors: dict[str, float]

    stock_overlap_deg: float
    overlap_retard_per_deg: float

    min_idle_deg: float
    max_wot_deg: float
    max_partial_load_deg: float
    wot_map_threshold_kpa: float
    idle_rpm_max: float
    idle_map_max_kpa: float
    min_advance_deg: float

    plausibility_max_wot_deg: float
    plausibility_max_turbo_deg: float
    plausibility_max_idle_deg: float
    plausibility_min_operating_deg: float

    def boost_scale(self, aspiration: str) -> float:
        return self.boost_aspiration_scale.get(
            aspiration,
            self.boost_aspiration_scale["naturally_aspirated"],
        )

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ModelCoefficients:
        ref = data["reference_engine"]
        burn = data["burn_index"]
        flame = data["flame_delay"]
        rpm = data["rpm_correction"]
        load = data["load_correction"]
        limits = data["limits"]
        plaus = data["plausibility"]
        cam = data.get("cam_timing", {})

        cr_corrections = tuple(
            CrCorrection(c["min_cr"], c["delta_deg"])
            for c in sorted(
                flame.get("cr_corrections", []),
                key=lambda c: c["min_cr"],
                reverse=True,
            )
        )

        valve_factors = {
            int(k): float(v) for k, v in data["valve_factors"].items()
        }

        boost = load["boost"]

        return cls(
            reference_bore_mm=float(ref["bore_mm"]),
            reference_compression_ratio=float(ref["compression_ratio"]),
            burn_duration_ref_deg=float(ref["burn_duration_ref_deg"]),
            chamber_factors=dict(data["chamber_factors"]),
            spark_factors=dict(data["spark_factors"]),
            valve_factors=valve_factors,
            bore_reference_mm=float(burn["bore_reference_mm"]),
            bore_exponent=float(burn["bore_exponent"]),
            compression_reference=float(burn["compression_reference"]),
            compression_exponent=float(burn["compression_exponent"]),
            flame_delay_base_deg=float(flame["base_deg"]),
            flame_delay_cr_corrections=cr_corrections,
            peak_pressure_target_deg=float(data["peak_pressure_target_deg"]),
            rpm_correction_factor=float(rpm["factor"]),
            rpm_reference=float(rpm["reference_rpm"]),
            load_reference_map_kpa=float(load["reference_map_kpa"]),
            vacuum_deg_per_10_kpa=float(load["vacuum"]["deg_per_10_kpa"]),
            boost_deg_per_0_1_bar=float(boost["deg_per_0_1_bar"]),
            boost_aspiration_scale={
                k: float(v) for k, v in boost["aspiration_scale"].items()
            },
            fuel_factors=dict(data.get("fuel_factors", {})),
            stock_overlap_deg=float(cam.get("stock_overlap_deg", 20.0)),
            overlap_retard_per_deg=float(cam.get("overlap_retard_per_deg", 0.05)),
            min_idle_deg=float(limits["min_idle_deg"]),
            max_wot_deg=float(limits["max_wot_deg"]),
            max_partial_load_deg=float(limits["max_partial_load_deg"]),
            wot_map_threshold_kpa=float(limits["wot_map_threshold_kpa"]),
            idle_rpm_max=float(limits["idle_rpm_max"]),
            idle_map_max_kpa=float(limits["idle_map_max_kpa"]),
            min_advance_deg=float(limits.get("min_advance_deg", -5.0)),
            plausibility_max_wot_deg=float(plaus["max_wot_deg"]),
            plausibility_max_turbo_deg=float(plaus["max_turbo_deg"]),
            plausibility_max_idle_deg=float(plaus["max_idle_deg"]),
            plausibility_min_operating_deg=float(plaus["min_operating_deg"]),
        )


def load_coefficients(path: Path | str) -> ModelCoefficients:
    with open(path, encoding="utf-8") as f:
        return ModelCoefficients.from_dict(json.load(f))
