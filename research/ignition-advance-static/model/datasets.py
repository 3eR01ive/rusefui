"""Наборы example + reference с эталонной картой."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class ReferenceDataset:
    name: str
    example_json: Path
    reference_csv: Path


def discover_datasets(
    examples_dir: Path | str = "examples",
    reference_dir: Path | str = "reference",
) -> tuple[ReferenceDataset, ...]:
    examples_dir = Path(examples_dir)
    reference_dir = Path(reference_dir)

    datasets: list[ReferenceDataset] = []
    for reference_csv in sorted(reference_dir.glob("*.csv")):
        example_json = examples_dir / f"{reference_csv.stem}.json"
        if example_json.is_file():
            datasets.append(
                ReferenceDataset(
                    name=reference_csv.stem,
                    example_json=example_json,
                    reference_csv=reference_csv,
                )
            )
    return tuple(datasets)
