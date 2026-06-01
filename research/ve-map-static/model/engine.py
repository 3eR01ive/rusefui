"""Параметры двигателя из входного JSON (ТЗ v1)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class EngineParameters:
    displacement_cc: float
    cylinders: int
    max_rpm: float

    intake_runner_length_mm: float
    intake_valve_diameter_mm: float

    cam_duration_deg: float
    cam_lift_mm: float
    cam_lsa_deg: float

    is_turbo: bool

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> EngineParameters:
        return cls(
            displacement_cc=float(data["displacement_cc"]),
            cylinders=int(data["cylinders"]),
            max_rpm=float(data["max_rpm"]),
            intake_runner_length_mm=float(data["intake_runner_length_mm"]),
            intake_valve_diameter_mm=float(data["intake_valve_diameter_mm"]),
            cam_duration_deg=float(data["cam_duration_deg"]),
            cam_lift_mm=float(data["cam_lift_mm"]),
            cam_lsa_deg=float(data["cam_lsa_deg"]),
            is_turbo=bool(data["is_turbo"]),
        )
