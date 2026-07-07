"""Shared ES continuous dataset definitions for paper figures."""

from pathlib import Path
from typing import Final

from .datasets import DatasetSpec


EMPIRICAL_TIMESTAMP_COLUMN: Final[str] = "event_timestamp_utc"
SYNTHETIC_TIMESTAMP_COLUMN: Final[str] = "timestamp_utc"
EMPIRICAL_PRICE_COLUMN: Final[str] = "trade_price"
SYNTHETIC_PRICE_COLUMN: Final[str] = "price"

EMPIRICAL_RELATIVE_INPUT_PATH: Final[Path] = Path(
    "data/databento-converted/ES_FUT-TBBO/continuous/es_continuous.parquet"
)
SYNTHETIC_RELATIVE_INPUT_PATH: Final[Path] = Path(
    "paper/data/synthetic_es_continuous/synthetic_es_continuous.parquet"
)


def default_empirical_input_path(repository_root: Path) -> Path:
    return repository_root / EMPIRICAL_RELATIVE_INPUT_PATH


def default_synthetic_input_path(repository_root: Path) -> Path:
    return repository_root / SYNTHETIC_RELATIVE_INPUT_PATH


def es_continuous_dataset_specs(
    *,
    empirical_input_path: Path,
    synthetic_input_path: Path,
) -> tuple[DatasetSpec, DatasetSpec]:
    empirical_spec = DatasetSpec(
        label="Empirical",
        role="empirical_market_data",
        input_path=empirical_input_path,
        timestamp_column=EMPIRICAL_TIMESTAMP_COLUMN,
        dataset_family="databento_converted",
        instrument="ES continuous future",
        color="#2F5D8C",
    )

    synthetic_spec = DatasetSpec(
        label="Janiform synthetic",
        role="synthetic_model_output",
        input_path=synthetic_input_path,
        timestamp_column=SYNTHETIC_TIMESTAMP_COLUMN,
        dataset_family="janiform_synthetic",
        instrument="ES continuous future",
        color="#B34D4A",
    )

    return empirical_spec, synthetic_spec
