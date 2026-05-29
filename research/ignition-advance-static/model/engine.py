"""Параметры двигателя из входного JSON."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class EngineParams:
    bore_mm: float
    stroke_mm: float
    rod_length_mm: float | None
    cylinder_count: int
    displacement_cc: float | None
    compression_ratio: float

    valves_per_cylinder: int
    spark_location: str
    chamber_type: str

    intake_duration_deg: float | None
    exhaust_duration_deg: float | None
    overlap_deg: float | None

    fuel: str
    aspiration: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> EngineParams:
        return cls(
            bore_mm=float(data["bore_mm"]),
            stroke_mm=float(data["stroke_mm"]),
            rod_length_mm=_optional_float(data.get("rod_length_mm")),
            cylinder_count=int(data["cylinder_count"]),
            displacement_cc=_optional_float(data.get("displacement_cc")),
            compression_ratio=float(data["compression_ratio"]),
            valves_per_cylinder=int(data["valves_per_cylinder"]),
            spark_location=str(data["spark_location"]),
            chamber_type=str(data["chamber_type"]),
            intake_duration_deg=_optional_float(data.get("intake_duration_deg")),
            exhaust_duration_deg=_optional_float(data.get("exhaust_duration_deg")),
            overlap_deg=_optional_float(data.get("overlap_deg")),
            fuel=str(data.get("fuel", "gasoline_95")),
            aspiration=str(data.get("aspiration", "naturally_aspirated")),
        )

    @property
    def stroke_ratio(self) -> float:
        return self.stroke_mm / self.bore_mm

    @property
    def is_forced_induction(self) -> bool:
        return self.aspiration in ("turbocharged", "supercharged")


def _optional_float(value: Any) -> float | None:
    if value is None:
        return None
    return float(value)
