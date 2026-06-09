"""Shared dataset loaders for paper figure scripts."""

from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pandas as pd
import pyarrow.parquet as pq

from .figure_metadata import file_artifact_metadata


@dataclass(frozen=True)
class DatasetSpec:
    label: str
    role: str
    input_path: Path
    timestamp_column: str
    dataset_family: str
    instrument: str
    color: str


@dataclass(frozen=True)
class TimestampDataset:
    spec: DatasetSpec
    input_metadata: dict[str, Any]
    data_summary: dict[str, Any]
    timestamps: pd.Series


def load_timestamp_dataset(
    spec: DatasetSpec,
    *,
    repository_root: Path,
) -> TimestampDataset:
    print(f"[{spec.label}] Reading parquet metadata: {spec.input_path}")

    parquet_file = pq.ParquetFile(spec.input_path)
    parquet_metadata = parquet_file.metadata

    print(f"[{spec.label}] Loading timestamp column: {spec.timestamp_column}")

    timestamp_table = pq.read_table(spec.input_path, columns=[spec.timestamp_column])
    timestamp_series = timestamp_table.column(spec.timestamp_column).to_pandas()

    event_count = len(timestamp_series)

    print(f"[{spec.label}] Loaded {event_count:,} event timestamps")

    if event_count < 2:
        raise ValueError(f"{spec.label} dataset must contain at least two timestamps")

    print(f"[{spec.label}] Normalizing timestamps and checking order")

    timestamps = pd.to_datetime(timestamp_series, utc=True)
    timestamp_null_count = int(timestamps.isna().sum())
    if timestamp_null_count > 0:
        raise ValueError(
            f"{spec.label} timestamp column contains {timestamp_null_count:,} null values"
        )

    original_timestamp_order_is_monotonic = bool(timestamps.is_monotonic_increasing)
    if not original_timestamp_order_is_monotonic:
        print(f"[{spec.label}] Timestamps are not monotonic, sorting before differencing")
        timestamps = timestamps.sort_values(kind="mergesort").reset_index(drop=True)

    input_metadata = file_artifact_metadata(
        spec.input_path,
        role=spec.role,
        artifact_type="parquet",
        media_type="application/vnd.apache.parquet",
        repository_root=repository_root,
    ) | {
        "dataset_family": spec.dataset_family,
        "instrument": spec.instrument,
        "timestamp_column": spec.timestamp_column,
        "parquet_row_count": parquet_metadata.num_rows,
        "parquet_row_group_count": parquet_metadata.num_row_groups,
        "parquet_columns": parquet_file.schema_arrow.names,
        "loaded_columns": timestamp_table.schema.names,
    }

    data_summary = {
        "label": spec.label,
        "event_count": event_count,
        "timestamp_null_count": timestamp_null_count,
        "original_timestamp_order_is_monotonic": original_timestamp_order_is_monotonic,
        "timestamp_min_utc": timestamps.iloc[0].isoformat(),
        "timestamp_max_utc": timestamps.iloc[-1].isoformat(),
    }

    return TimestampDataset(
        spec=spec,
        input_metadata=input_metadata,
        data_summary=data_summary,
        timestamps=timestamps,
    )
