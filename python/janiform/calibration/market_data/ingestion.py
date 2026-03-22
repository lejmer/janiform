import csv
from dataclasses import dataclass
from typing import Optional

import pyarrow
from pyarrow import parquet

from .contracts import CalibrationInputFileFormat, CalibrationInputFileReference
from .._errors import _calibration_error
from ..._contracts import ContractModel


CALIBRATION_MARKET_DATA_INGESTION_ERROR_ID = "calibration.validation.market_data_ingestion"
CALIBRATION_MARKET_DATA_INGESTION_ENTRYPOINT = "market_data_ingestion"


@dataclass(frozen=True, slots=True)
class LoadedMarketDataSourceSummary(ContractModel):
    path: str
    file_format: CalibrationInputFileFormat
    provenance_label: str
    source_row_count: int
    retained_row_count: int
    dropped_duplicate_row_count: int
    sort_records_by_timestamp: bool
    applied_timestamp_timezone: Optional[str]
    canonical_value_field: Optional[str] = None
    value_mapping_policy: Optional[str] = None


def _load_source_columns(
    file_reference: CalibrationInputFileReference,
    *,
    field_path: str,
) -> tuple[str, ...]:
    try:
        match file_reference.file_format:
            case CalibrationInputFileFormat.CSV:
                return _load_csv_source_columns(
                    file_reference,
                    field_path=field_path,
                )
            case CalibrationInputFileFormat.PARQUET:
                schema = parquet.read_schema(file_reference.path)
            case _:
                raise _market_data_ingestion_error(
                    f"{field_path}.file_format",
                    f"must be one of: csv, parquet, received '{file_reference.file_format.value}'",
                    normalization_step="source_loading",
                )
    except FileNotFoundError as error:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            "does not exist",
            context={"path": file_reference.path},
            normalization_step="source_loading",
        ) from error
    except OSError as error:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be read: {error}",
            context={"path": file_reference.path},
            normalization_step="source_loading",
        ) from error
    except pyarrow.ArrowInvalid as error:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be parsed as {file_reference.file_format.value}: {error}",
            context={"path": file_reference.path},
            normalization_step="source_loading",
        ) from error

    return tuple(str(column_name) for column_name in schema.names)


def _load_csv_source_columns(
    file_reference: CalibrationInputFileReference,
    *,
    field_path: str,
) -> tuple[str, ...]:
    try:
        with open(file_reference.path, encoding="utf-8", newline="") as input_file:
            reader = csv.reader(input_file)
            field_names = next(reader, None)
            if field_names is None or len(field_names) == 0:
                raise _market_data_ingestion_error(
                    f"{field_path}.path",
                    "could not be parsed as csv: missing header row",
                    context={"path": file_reference.path},
                    normalization_step="source_loading",
                )
    except FileNotFoundError as error:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            "does not exist",
            context={"path": file_reference.path},
            normalization_step="source_loading",
        ) from error
    except OSError as error:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be read: {error}",
            context={"path": file_reference.path},
            normalization_step="source_loading",
        ) from error
    except csv.Error as error:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be parsed as csv: {error}",
            context={"path": file_reference.path},
            normalization_step="source_loading",
        ) from error

    return tuple(field_names)


def _market_data_ingestion_error(
    field_path: str,
    message: str,
    *,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
    normalization_step: Optional[str] = None,
) -> Exception:
    error_context = {} if context is None else dict(context)
    if normalization_step is not None:
        error_context["normalization_step"] = normalization_step

    return _calibration_error(
        field_path,
        message,
        entrypoint=CALIBRATION_MARKET_DATA_INGESTION_ENTRYPOINT,
        error_id=CALIBRATION_MARKET_DATA_INGESTION_ERROR_ID,
        context=error_context,
    )


__all__ = [
    "LoadedMarketDataSourceSummary",
]
