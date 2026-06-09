"""Shared dataset loaders for paper figure scripts."""

from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

import pandas as pd
import pyarrow as pa
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


@dataclass(frozen=True)
class TimestampValueDataset:
    spec: DatasetSpec
    input_metadata: dict[str, Any]
    data_summary: dict[str, Any]
    timestamps: pd.Series
    values: pd.Series
    value_column: str


def load_timestamp_dataset(
    spec: DatasetSpec,
    *,
    repository_root: Path,
) -> TimestampDataset:
    loaded_dataset = load_timestamp_value_dataset(
        spec,
        value_column=None,
        repository_root=repository_root,
    )

    return TimestampDataset(
        spec=loaded_dataset.spec,
        input_metadata=loaded_dataset.input_metadata,
        data_summary=loaded_dataset.data_summary,
        timestamps=loaded_dataset.timestamps,
    )


def load_timestamp_value_dataset(
    spec: DatasetSpec,
    *,
    value_column: Optional[str],
    repository_root: Path,
) -> TimestampValueDataset:
    print(f"[{spec.label}] Reading parquet metadata: {spec.input_path}")

    parquet_file = pq.ParquetFile(spec.input_path)
    parquet_metadata = parquet_file.metadata

    print(f"[{spec.label}] Loading timestamp column: {spec.timestamp_column}")

    loaded_columns = [spec.timestamp_column]
    if value_column is not None:
        loaded_columns.append(value_column)

    timestamp_table = pq.read_table(spec.input_path, columns=loaded_columns)
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
    value_series = load_optional_value_series(timestamp_table, value_column)
    if not original_timestamp_order_is_monotonic:
        print(f"[{spec.label}] Timestamps are not monotonic, sorting before differencing")
        sorted_timestamps = timestamps.sort_values(kind="mergesort")
        sorted_index = sorted_timestamps.index
        timestamps = sorted_timestamps.reset_index(drop=True)
        if value_series is not None:
            value_series = value_series.loc[sorted_index].reset_index(drop=True)

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
    if value_column is not None and value_series is not None:
        value_null_count = int(value_series.isna().sum())
        if value_null_count > 0:
            raise ValueError(
                f"{spec.label} value column contains {value_null_count:,} null values"
            )

        data_summary["value_column"] = value_column
        data_summary["value_null_count"] = value_null_count

    return TimestampValueDataset(
        spec=spec,
        input_metadata=input_metadata,
        data_summary=data_summary,
        timestamps=timestamps,
        values=value_series if value_series is not None else pd.Series(dtype="float64"),
        value_column=value_column if value_column is not None else "",
    )


def load_optional_value_series(
    timestamp_table: pa.Table,
    value_column: Optional[str],
) -> Optional[pd.Series]:
    if value_column is None:
        return None

    return timestamp_table.column(value_column).to_pandas()
