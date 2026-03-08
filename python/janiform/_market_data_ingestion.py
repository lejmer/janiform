import csv
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from math import isfinite
from typing import Optional, Protocol
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

import pyarrow
from pyarrow import compute as pyarrow_compute
from pyarrow import parquet as pyarrow_parquet

from ._contracts import ContractModel
from .calibration import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationKnownNewsInput,
    CalibrationKnownNewsColumnMapping,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationOptionQuoteInputPolicy,
    CalibrationOptionSurfaceColumnMapping,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
    MarketDataCalibrationRequest,
    _calibration_error,
)


CALIBRATION_MARKET_DATA_INGESTION_ERROR_ID = "calibration.validation.market_data_ingestion"
CALIBRATION_MARKET_DATA_INGESTION_ENTRYPOINT = "market_data_ingestion"


@dataclass(frozen=True, slots=True)
class NormalizedTickDataRecord(ContractModel):
    timestamp: datetime
    price: Optional[float]
    trade_price: Optional[float]
    bid_price: Optional[float]
    ask_price: Optional[float]
    close_price: Optional[float]
    source_row_number: int


@dataclass(frozen=True, slots=True)
class NormalizedOptionSurfaceRecord(ContractModel):
    quote_timestamp: datetime
    expiry_timestamp: datetime
    strike: float
    option_type: str
    quote_value: float
    mid_price: Optional[float]
    bid_price: Optional[float]
    ask_price: Optional[float]
    implied_volatility: Optional[float]
    source_row_number: int


@dataclass(frozen=True, slots=True)
class NormalizedKnownNewsRecord(ContractModel):
    event_timestamp: datetime
    event_identifier: Optional[str]
    source_row_number: int


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


@dataclass(frozen=True, slots=True)
class LoadedTickDataSource(ContractModel):
    records: tuple[NormalizedTickDataRecord, ...]
    summary: LoadedMarketDataSourceSummary


@dataclass(frozen=True, slots=True)
class LoadedOptionSurfaceSource(ContractModel):
    records: tuple[NormalizedOptionSurfaceRecord, ...]
    summary: LoadedMarketDataSourceSummary


@dataclass(frozen=True, slots=True)
class LoadedKnownNewsSource(ContractModel):
    records: tuple[NormalizedKnownNewsRecord, ...]
    summary: LoadedMarketDataSourceSummary


@dataclass(frozen=True, slots=True)
class LoadedMarketDataCalibrationInputs(ContractModel):
    tick_data: LoadedTickDataSource
    option_surface: Optional[LoadedOptionSurfaceSource] = None
    known_news: Optional[LoadedKnownNewsSource] = None


class _HasSourceRowNumber(Protocol):
    source_row_number: int


@dataclass(frozen=True, slots=True)
class _LoadedSourceRows:
    rows: tuple[Mapping[str, object], ...]
    available_columns: tuple[str, ...]


def load_market_data_calibration_inputs(
    request: MarketDataCalibrationRequest,
) -> LoadedMarketDataCalibrationInputs:
    tick_source_rows = _load_source_rows(
        request.tick_data_source,
        field_path="tick_data_source",
    )
    tick_data = _normalize_tick_data(
        source_rows=tick_source_rows,
        file_reference=request.tick_data_source,
        column_mapping=request.column_mapping.tick_data,
        normalization_policy=request.normalization_policy,
    )

    option_surface: Optional[LoadedOptionSurfaceSource] = None
    if request.option_surface_source is not None and request.column_mapping.option_surface is not None:
        option_source_rows = _load_source_rows(
            request.option_surface_source,
            field_path="option_surface_source",
        )
        option_surface = _normalize_option_surface(
            source_rows=option_source_rows,
            file_reference=request.option_surface_source,
            column_mapping=request.column_mapping.option_surface,
            normalization_policy=request.normalization_policy,
        )

    known_news: Optional[LoadedKnownNewsSource] = None
    if request.known_news_inputs is not None:
        if request.known_news_inputs.calendar_source.file_format in (
            CalibrationInputFileFormat.CSV,
            CalibrationInputFileFormat.PARQUET,
        ):
            if request.column_mapping.known_news is None:
                raise _market_data_ingestion_error(
                    "column_mapping.known_news",
                    "is required to load known_news_inputs from csv or parquet sources",
                )

            known_news_source_rows = _load_source_rows(
                request.known_news_inputs.calendar_source,
                field_path="known_news_inputs.calendar_source",
            )
            known_news = _normalize_known_news(
                source_rows=known_news_source_rows,
                known_news_inputs=request.known_news_inputs,
                column_mapping=request.column_mapping.known_news,
            )

    return LoadedMarketDataCalibrationInputs(
        tick_data=tick_data,
        option_surface=option_surface,
        known_news=known_news,
    )


def _market_data_ingestion_error(
    field_path: str,
    message: str,
    *,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
) -> Exception:
    return _calibration_error(
        field_path,
        message,
        entrypoint=CALIBRATION_MARKET_DATA_INGESTION_ENTRYPOINT,
        error_id=CALIBRATION_MARKET_DATA_INGESTION_ERROR_ID,
        context=context,
    )


def _load_source_rows(
    file_reference: CalibrationInputFileReference,
    *,
    field_path: str,
) -> _LoadedSourceRows:
    try:
        match file_reference.file_format:
            case CalibrationInputFileFormat.CSV:
                return _load_csv_source_rows(
                    file_reference,
                    field_path=field_path,
                )
            case CalibrationInputFileFormat.PARQUET:
                table = pyarrow_parquet.read_table(file_reference.path)
            case _:
                raise _market_data_ingestion_error(
                    f"{field_path}.file_format",
                    f"must be one of: csv, parquet, received '{file_reference.file_format.value}'",
                )
    except FileNotFoundError as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            "does not exist",
            context={"path": file_reference.path},
        ) from e
    except OSError as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be read: {e}",
            context={"path": file_reference.path},
        ) from e
    except pyarrow.ArrowInvalid as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be parsed as {file_reference.file_format.value}: {e}",
            context={"path": file_reference.path},
        ) from e

    table = _stringify_timestamp_columns(table)

    return _LoadedSourceRows(
        rows=tuple(table.to_pylist()),
        available_columns=tuple(str(column_name) for column_name in table.column_names),
    )


def _load_csv_source_rows(
    file_reference: CalibrationInputFileReference,
    *,
    field_path: str,
) -> _LoadedSourceRows:
    try:
        with open(file_reference.path, encoding="utf-8", newline="") as input_file:
            reader = csv.DictReader(input_file)
            field_names = reader.fieldnames
            if field_names is None or len(field_names) == 0:
                raise _market_data_ingestion_error(
                    f"{field_path}.path",
                    "could not be parsed as csv: missing header row",
                    context={"path": file_reference.path},
                )

            rows = tuple(dict(source_row) for source_row in reader)
    except FileNotFoundError as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            "does not exist",
            context={"path": file_reference.path},
        ) from e
    except OSError as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be read: {e}",
            context={"path": file_reference.path},
        ) from e
    except csv.Error as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be parsed as csv: {e}",
            context={"path": file_reference.path},
        ) from e

    return _LoadedSourceRows(
        rows=rows,
        available_columns=tuple(field_names),
    )


def _stringify_timestamp_columns(table: pyarrow.Table) -> pyarrow.Table:
    normalized_table = table
    for column_index, field in enumerate(normalized_table.schema):
        if not pyarrow.types.is_timestamp(field.type):
            continue

        normalized_table = normalized_table.set_column(
            column_index,
            field.name,
            _timestamp_chunked_array_to_strings(
                normalized_table.column(column_index),
                timestamp_type=field.type,
            ),
        )

    return normalized_table


def _timestamp_chunked_array_to_strings(
    chunked_array: pyarrow.ChunkedArray,
    *,
    timestamp_type: pyarrow.TimestampType,
) -> pyarrow.ChunkedArray:
    return pyarrow.chunked_array(
        [
            pyarrow.array(
                [
                    _format_arrow_timestamp_value(
                        raw_value,
                        time_unit=timestamp_type.unit,
                        timezone_aware=timestamp_type.tz is not None,
                    )
                    for raw_value in pyarrow_compute.cast(chunk, pyarrow.int64()).to_pylist()
                ],
                type=pyarrow.string(),
            )
            for chunk in chunked_array.chunks
        ]
    )


def _format_arrow_timestamp_value(
    raw_value: Optional[int],
    *,
    time_unit: str,
    timezone_aware: bool,
) -> Optional[str]:
    if raw_value is None:
        return None

    seconds_since_epoch: int
    microseconds: int
    match time_unit:
        case "s":
            seconds_since_epoch = raw_value
            microseconds = 0
        case "ms":
            seconds_since_epoch, milliseconds = divmod(raw_value, 1_000)
            microseconds = milliseconds * 1_000
        case "us":
            seconds_since_epoch, microseconds = divmod(raw_value, 1_000_000)
        case "ns":
            seconds_since_epoch, nanoseconds = divmod(raw_value, 1_000_000_000)
            microseconds = nanoseconds // 1_000
        case _:
            raise ValueError(f"Unsupported Arrow timestamp unit: {time_unit}")

    if timezone_aware:
        timestamp = datetime(1970, 1, 1, tzinfo=UTC) + timedelta(
            seconds=seconds_since_epoch,
            microseconds=microseconds,
        )
    else:
        timestamp = datetime(1970, 1, 1) + timedelta(
            seconds=seconds_since_epoch,
            microseconds=microseconds,
        )

    return timestamp.isoformat(timespec="microseconds")


def _normalize_tick_data(
    *,
    source_rows: _LoadedSourceRows,
    file_reference: CalibrationInputFileReference,
    column_mapping: CalibrationTickDataColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
) -> LoadedTickDataSource:
    tick_price_input_policy = normalization_policy.tick_price_input_policy
    required_columns: list[str] = [column_mapping.timestamp_column]

    match tick_price_input_policy:
        case CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN:
            if column_mapping.price_column is not None:
                required_columns.append(column_mapping.price_column)
        case CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN:
            if column_mapping.trade_price_column is not None:
                required_columns.append(column_mapping.trade_price_column)
        case CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT:
            if column_mapping.bid_column is not None:
                required_columns.append(column_mapping.bid_column)
            if column_mapping.ask_column is not None:
                required_columns.append(column_mapping.ask_column)
        case CalibrationTickPriceInputPolicy.CLOSE_PRICE_COLUMN:
            if column_mapping.close_price_column is not None:
                required_columns.append(column_mapping.close_price_column)
        case None:
            pass

    _validate_mapped_columns_exist(
        available_columns=source_rows.available_columns,
        required_columns=tuple(required_columns),
        field_path="column_mapping.tick_data",
        source_path=file_reference.path,
    )

    normalized_records: tuple[NormalizedTickDataRecord, ...] = tuple(
        _normalize_tick_row(
            source_row,
            source_row_number=source_row_number,
            column_mapping=column_mapping,
            normalization_policy=normalization_policy,
            source_path=file_reference.path,
        )
        for source_row_number, source_row in enumerate(source_rows.rows, start=1)
    )

    deduplicated_records, dropped_duplicate_row_count = _deduplicate_records(
        normalized_records,
        key_builder=lambda record: (
            record.timestamp,
            record.price,
            record.trade_price,
            record.bid_price,
            record.ask_price,
            record.close_price,
        ),
        duplicate_record_policy=normalization_policy.duplicate_record_policy,
        field_path="tick_data_source",
        source_path=file_reference.path,
    )

    if normalization_policy.sort_records_by_timestamp:
        deduplicated_records = tuple(
            sorted(
                deduplicated_records,
                key=lambda record: (record.timestamp, record.source_row_number),
            )
        )

    return LoadedTickDataSource(
        records=deduplicated_records,
        summary=LoadedMarketDataSourceSummary(
            path=file_reference.path,
            file_format=file_reference.file_format,
            provenance_label=file_reference.provenance_label,
            source_row_count=len(source_rows.rows),
            retained_row_count=len(deduplicated_records),
            dropped_duplicate_row_count=dropped_duplicate_row_count,
            sort_records_by_timestamp=normalization_policy.sort_records_by_timestamp,
            applied_timestamp_timezone=normalization_policy.timestamp_timezone,
            canonical_value_field="price" if tick_price_input_policy is not None else None,
            value_mapping_policy=(
                tick_price_input_policy.value
                if tick_price_input_policy is not None
                else None
            ),
        ),
    )


def _normalize_option_surface(
    *,
    source_rows: _LoadedSourceRows,
    file_reference: CalibrationInputFileReference,
    column_mapping: CalibrationOptionSurfaceColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
) -> LoadedOptionSurfaceSource:
    option_quote_input_policy = normalization_policy.option_quote_input_policy
    required_columns: list[str] = [
        column_mapping.quote_timestamp_column,
        column_mapping.expiry_timestamp_column,
        column_mapping.strike_column,
        column_mapping.option_type_column,
    ]

    match option_quote_input_policy:
        case CalibrationOptionQuoteInputPolicy.MID_PRICE_COLUMN:
            if column_mapping.mid_price_column is not None:
                required_columns.append(column_mapping.mid_price_column)
        case CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT:
            if column_mapping.bid_column is not None:
                required_columns.append(column_mapping.bid_column)
            if column_mapping.ask_column is not None:
                required_columns.append(column_mapping.ask_column)
        case CalibrationOptionQuoteInputPolicy.IMPLIED_VOLATILITY_COLUMN:
            if column_mapping.implied_volatility_column is not None:
                required_columns.append(column_mapping.implied_volatility_column)
        case None:
            raise _market_data_ingestion_error(
                "normalization_policy.option_quote_input_policy",
                "is required to normalize option_surface_source",
            )

    _validate_mapped_columns_exist(
        available_columns=source_rows.available_columns,
        required_columns=tuple(required_columns),
        field_path="column_mapping.option_surface",
        source_path=file_reference.path,
    )

    normalized_records: tuple[NormalizedOptionSurfaceRecord, ...] = tuple(
        _normalize_option_surface_row(
            source_row,
            source_row_number=source_row_number,
            column_mapping=column_mapping,
            normalization_policy=normalization_policy,
            source_path=file_reference.path,
        )
        for source_row_number, source_row in enumerate(source_rows.rows, start=1)
    )

    deduplicated_records, dropped_duplicate_row_count = _deduplicate_records(
        normalized_records,
        key_builder=lambda record: (
            record.quote_timestamp,
            record.expiry_timestamp,
            record.strike,
            record.option_type,
            record.quote_value,
            record.mid_price,
            record.bid_price,
            record.ask_price,
            record.implied_volatility,
        ),
        duplicate_record_policy=normalization_policy.duplicate_record_policy,
        field_path="option_surface_source",
        source_path=file_reference.path,
    )

    if normalization_policy.sort_records_by_timestamp:
        deduplicated_records = tuple(
            sorted(
                deduplicated_records,
                key=lambda record: (
                    record.quote_timestamp,
                    record.expiry_timestamp,
                    record.strike,
                    record.option_type,
                    record.source_row_number,
                ),
            )
        )

    return LoadedOptionSurfaceSource(
        records=deduplicated_records,
        summary=LoadedMarketDataSourceSummary(
            path=file_reference.path,
            file_format=file_reference.file_format,
            provenance_label=file_reference.provenance_label,
            source_row_count=len(source_rows.rows),
            retained_row_count=len(deduplicated_records),
            dropped_duplicate_row_count=dropped_duplicate_row_count,
            sort_records_by_timestamp=normalization_policy.sort_records_by_timestamp,
            applied_timestamp_timezone=normalization_policy.timestamp_timezone,
            canonical_value_field="quote_value",
            value_mapping_policy=option_quote_input_policy.value,
        ),
    )


def _normalize_known_news(
    *,
    source_rows: _LoadedSourceRows,
    known_news_inputs: CalibrationKnownNewsInput,
    column_mapping: CalibrationKnownNewsColumnMapping,
) -> LoadedKnownNewsSource:
    _validate_mapped_columns_exist(
        available_columns=source_rows.available_columns,
        required_columns=(column_mapping.event_timestamp_column,),
        field_path="column_mapping.known_news",
        source_path=known_news_inputs.calendar_source.path,
    )

    normalized_records: tuple[NormalizedKnownNewsRecord, ...] = tuple(
        _normalize_known_news_row(
            source_row,
            source_row_number=source_row_number,
            known_news_inputs=known_news_inputs,
            column_mapping=column_mapping,
        )
        for source_row_number, source_row in enumerate(source_rows.rows, start=1)
    )

    deduplicated_records, dropped_duplicate_row_count = _deduplicate_records(
        normalized_records,
        key_builder=lambda record: (record.event_timestamp, record.event_identifier),
        duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
        field_path="known_news_inputs.calendar_source",
        source_path=known_news_inputs.calendar_source.path,
    )

    return LoadedKnownNewsSource(
        records=tuple(sorted(deduplicated_records, key=lambda record: (record.event_timestamp, record.source_row_number))),
        summary=LoadedMarketDataSourceSummary(
            path=known_news_inputs.calendar_source.path,
            file_format=known_news_inputs.calendar_source.file_format,
            provenance_label=known_news_inputs.calendar_source.provenance_label,
            source_row_count=len(source_rows.rows),
            retained_row_count=len(deduplicated_records),
            dropped_duplicate_row_count=dropped_duplicate_row_count,
            sort_records_by_timestamp=True,
            applied_timestamp_timezone=known_news_inputs.calendar_timezone,
        ),
    )


def _normalize_tick_row(
    source_row: Mapping[str, object],
    *,
    source_row_number: int,
    column_mapping: CalibrationTickDataColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
    source_path: str,
) -> NormalizedTickDataRecord:
    row_field_path = f"tick_data_source.rows[{source_row_number}]"
    timestamp = _normalize_timestamp_value(
        source_row.get(column_mapping.timestamp_column),
        field_path=f"{row_field_path}.{column_mapping.timestamp_column}",
        source_timezone=normalization_policy.timestamp_timezone,
        source_path=source_path,
    )
    trade_price = _normalize_optional_numeric_value(
        source_row.get(column_mapping.trade_price_column) if column_mapping.trade_price_column is not None else None,
        field_path=f"{row_field_path}.{column_mapping.trade_price_column}"
        if column_mapping.trade_price_column is not None
        else row_field_path,
        source_path=source_path,
        allow_zero=True,
        allow_negative=False,
    )
    bid_price = _normalize_optional_numeric_value(
        source_row.get(column_mapping.bid_column) if column_mapping.bid_column is not None else None,
        field_path=f"{row_field_path}.{column_mapping.bid_column}" if column_mapping.bid_column is not None else row_field_path,
        source_path=source_path,
        allow_zero=True,
        allow_negative=False,
    )
    ask_price = _normalize_optional_numeric_value(
        source_row.get(column_mapping.ask_column) if column_mapping.ask_column is not None else None,
        field_path=f"{row_field_path}.{column_mapping.ask_column}" if column_mapping.ask_column is not None else row_field_path,
        source_path=source_path,
        allow_zero=True,
        allow_negative=False,
    )
    close_price = _normalize_optional_numeric_value(
        source_row.get(column_mapping.close_price_column) if column_mapping.close_price_column is not None else None,
        field_path=f"{row_field_path}.{column_mapping.close_price_column}"
        if column_mapping.close_price_column is not None
        else row_field_path,
        source_path=source_path,
        allow_zero=True,
        allow_negative=False,
    )
    explicit_price = _normalize_optional_numeric_value(
        source_row.get(column_mapping.price_column) if column_mapping.price_column is not None else None,
        field_path=f"{row_field_path}.{column_mapping.price_column}" if column_mapping.price_column is not None else row_field_path,
        source_path=source_path,
        allow_zero=True,
        allow_negative=False,
    )

    match normalization_policy.tick_price_input_policy:
        case CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN:
            if explicit_price is None:
                raise _market_data_ingestion_error(
                    f"{row_field_path}.{column_mapping.price_column}",
                    "is required for explicit_price_column tick normalization and cannot be blank",
                    context={"source_path": source_path, "source_row_number": source_row_number},
                )

            price = explicit_price
        case CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN:
            if trade_price is None:
                raise _market_data_ingestion_error(
                    f"{row_field_path}.{column_mapping.trade_price_column}",
                    "is required for trade_price_column tick normalization and cannot be blank",
                    context={"source_path": source_path, "source_row_number": source_row_number},
                )

            price = trade_price
        case CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT:
            if bid_price is None or ask_price is None:
                raise _market_data_ingestion_error(
                    row_field_path,
                    "bid_column and ask_column must both contain values to derive bid_ask_midpoint",
                    context={"source_path": source_path, "source_row_number": source_row_number},
                )

            if ask_price < bid_price:
                raise _market_data_ingestion_error(
                    row_field_path,
                    "ask price must be greater than or equal to bid price for bid_ask_midpoint derivation",
                    context={"source_path": source_path, "source_row_number": source_row_number},
                )

            price = (bid_price + ask_price) / 2.0
        case CalibrationTickPriceInputPolicy.CLOSE_PRICE_COLUMN:
            if close_price is None:
                raise _market_data_ingestion_error(
                    f"{row_field_path}.{column_mapping.close_price_column}",
                    "is required for close_price_column tick normalization and cannot be blank",
                    context={"source_path": source_path, "source_row_number": source_row_number},
                )

            price = close_price
        case None:
            price = None

    return NormalizedTickDataRecord(
        timestamp=timestamp,
        price=price,
        trade_price=trade_price,
        bid_price=bid_price,
        ask_price=ask_price,
        close_price=close_price,
        source_row_number=source_row_number,
    )


def _normalize_option_surface_row(
    source_row: Mapping[str, object],
    *,
    source_row_number: int,
    column_mapping: CalibrationOptionSurfaceColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
    source_path: str,
) -> NormalizedOptionSurfaceRecord:
    row_field_path = f"option_surface_source.rows[{source_row_number}]"
    quote_timestamp = _normalize_timestamp_value(
        source_row.get(column_mapping.quote_timestamp_column),
        field_path=f"{row_field_path}.{column_mapping.quote_timestamp_column}",
        source_timezone=normalization_policy.timestamp_timezone,
        source_path=source_path,
    )
    expiry_timestamp = _normalize_timestamp_value(
        source_row.get(column_mapping.expiry_timestamp_column),
        field_path=f"{row_field_path}.{column_mapping.expiry_timestamp_column}",
        source_timezone=normalization_policy.timestamp_timezone,
        source_path=source_path,
    )
    strike = _normalize_required_numeric_value(
        source_row.get(column_mapping.strike_column),
        field_path=f"{row_field_path}.{column_mapping.strike_column}",
        source_path=source_path,
        allow_zero=False,
        allow_negative=False,
    )
    option_type = _normalize_option_type(
        source_row.get(column_mapping.option_type_column),
        field_path=f"{row_field_path}.{column_mapping.option_type_column}",
        source_path=source_path,
    )
    mid_price = _normalize_optional_numeric_value(
        source_row.get(column_mapping.mid_price_column) if column_mapping.mid_price_column is not None else None,
        field_path=f"{row_field_path}.{column_mapping.mid_price_column}"
        if column_mapping.mid_price_column is not None
        else row_field_path,
        source_path=source_path,
        allow_zero=True,
        allow_negative=False,
    )
    bid_price = _normalize_optional_numeric_value(
        source_row.get(column_mapping.bid_column) if column_mapping.bid_column is not None else None,
        field_path=f"{row_field_path}.{column_mapping.bid_column}" if column_mapping.bid_column is not None else row_field_path,
        source_path=source_path,
        allow_zero=True,
        allow_negative=False,
    )
    ask_price = _normalize_optional_numeric_value(
        source_row.get(column_mapping.ask_column) if column_mapping.ask_column is not None else None,
        field_path=f"{row_field_path}.{column_mapping.ask_column}" if column_mapping.ask_column is not None else row_field_path,
        source_path=source_path,
        allow_zero=True,
        allow_negative=False,
    )
    implied_volatility = _normalize_optional_numeric_value(
        source_row.get(column_mapping.implied_volatility_column)
        if column_mapping.implied_volatility_column is not None
        else None,
        field_path=f"{row_field_path}.{column_mapping.implied_volatility_column}"
        if column_mapping.implied_volatility_column is not None
        else row_field_path,
        source_path=source_path,
        allow_zero=True,
        allow_negative=False,
    )

    match normalization_policy.option_quote_input_policy:
        case CalibrationOptionQuoteInputPolicy.MID_PRICE_COLUMN:
            if mid_price is None:
                raise _market_data_ingestion_error(
                    f"{row_field_path}.{column_mapping.mid_price_column}",
                    "is required for mid_price_column option normalization and cannot be blank",
                    context={"source_path": source_path, "source_row_number": source_row_number},
                )

            quote_value = mid_price
        case CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT:
            if bid_price is None or ask_price is None:
                raise _market_data_ingestion_error(
                    row_field_path,
                    "bid_column and ask_column must both contain values to derive option bid_ask_midpoint",
                    context={"source_path": source_path, "source_row_number": source_row_number},
                )

            if ask_price < bid_price:
                raise _market_data_ingestion_error(
                    row_field_path,
                    "ask price must be greater than or equal to bid price for option bid_ask_midpoint derivation",
                    context={"source_path": source_path, "source_row_number": source_row_number},
                )

            quote_value = (bid_price + ask_price) / 2.0
        case CalibrationOptionQuoteInputPolicy.IMPLIED_VOLATILITY_COLUMN:
            if implied_volatility is None:
                raise _market_data_ingestion_error(
                    f"{row_field_path}.{column_mapping.implied_volatility_column}",
                    "is required for implied_volatility_column option normalization and cannot be blank",
                    context={"source_path": source_path, "source_row_number": source_row_number},
                )

            quote_value = implied_volatility
        case None:
            raise _market_data_ingestion_error(
                "normalization_policy.option_quote_input_policy",
                "is required to normalize option_surface_source",
            )

    return NormalizedOptionSurfaceRecord(
        quote_timestamp=quote_timestamp,
        expiry_timestamp=expiry_timestamp,
        strike=strike,
        option_type=option_type,
        quote_value=quote_value,
        mid_price=mid_price,
        bid_price=bid_price,
        ask_price=ask_price,
        implied_volatility=implied_volatility,
        source_row_number=source_row_number,
    )


def _normalize_known_news_row(
    source_row: Mapping[str, object],
    *,
    source_row_number: int,
    known_news_inputs: CalibrationKnownNewsInput,
    column_mapping: CalibrationKnownNewsColumnMapping,
) -> NormalizedKnownNewsRecord:
    row_field_path = f"known_news_inputs.calendar_source.rows[{source_row_number}]"
    event_timestamp = _normalize_timestamp_value(
        source_row.get(column_mapping.event_timestamp_column),
        field_path=f"{row_field_path}.{column_mapping.event_timestamp_column}",
        source_timezone=known_news_inputs.calendar_timezone,
        source_path=known_news_inputs.calendar_source.path,
    )
    event_identifier = _normalize_optional_text_value(
        source_row.get(column_mapping.event_identifier_column)
        if column_mapping.event_identifier_column is not None
        else None,
        field_path=f"{row_field_path}.{column_mapping.event_identifier_column}"
        if column_mapping.event_identifier_column is not None
        else row_field_path,
        source_path=known_news_inputs.calendar_source.path,
    )

    return NormalizedKnownNewsRecord(
        event_timestamp=event_timestamp,
        event_identifier=event_identifier,
        source_row_number=source_row_number,
    )


def _deduplicate_records[RecordType: _HasSourceRowNumber](
    records: tuple[RecordType, ...],
    *,
    key_builder: Callable[[RecordType], object],
    duplicate_record_policy: CalibrationDuplicateRecordPolicy,
    field_path: str,
    source_path: str,
) -> tuple[tuple[RecordType, ...], int]:
    match duplicate_record_policy:
        case CalibrationDuplicateRecordPolicy.ERROR:
            seen_row_numbers: dict[object, int] = {}
            for record in records:
                deduplication_key = key_builder(record)
                existing_row_number = seen_row_numbers.get(deduplication_key)
                if existing_row_number is not None:
                    raise _market_data_ingestion_error(
                        field_path,
                        "contains duplicate normalized rows. Use duplicate_record_policy=keep_first "
                        "or keep_last if the feed legitimately repeats identical rows.",
                        context={
                            "first_source_row_number": existing_row_number,
                            "duplicate_source_row_number": record.source_row_number,
                            "source_path": source_path,
                        },
                    )
                seen_row_numbers[deduplication_key] = record.source_row_number

            return records, 0
        case CalibrationDuplicateRecordPolicy.KEEP_FIRST:
            retained_records: list[RecordType] = []
            seen_keys: set[object] = set()
            for record in records:
                deduplication_key = key_builder(record)
                if deduplication_key in seen_keys:
                    continue
                seen_keys.add(deduplication_key)
                retained_records.append(record)

            return tuple(retained_records), len(records) - len(retained_records)
        case CalibrationDuplicateRecordPolicy.KEEP_LAST:
            last_index_by_key: dict[object, int] = {}
            for record_index, record in enumerate(records):
                last_index_by_key[key_builder(record)] = record_index

            retained_records = tuple(
                record
                for record_index, record in enumerate(records)
                if last_index_by_key[key_builder(record)] == record_index
            )

            return retained_records, len(records) - len(retained_records)


def _normalize_timestamp_value(
    raw_value: object,
    *,
    field_path: str,
    source_timezone: Optional[str],
    source_path: str,
) -> datetime:
    if raw_value is None:
        raise _market_data_ingestion_error(
            field_path,
            "is required and cannot be null",
            context={"source_path": source_path},
        )

    if isinstance(raw_value, datetime):
        parsed_value = raw_value
    elif isinstance(raw_value, str):
        stripped_value = raw_value.strip()
        if not stripped_value:
            raise _market_data_ingestion_error(
                field_path,
                "is required and cannot be blank",
                context={"source_path": source_path},
            )

        try:
            parsed_value = datetime.fromisoformat(stripped_value.replace("Z", "+00:00"))
        except ValueError as e:
            raise _market_data_ingestion_error(
                field_path,
                "must be an ISO 8601 timestamp string or a parquet timestamp column",
                context={"source_path": source_path, "raw_value": stripped_value},
            ) from e
    else:
        raise _market_data_ingestion_error(
            field_path,
            "must be an ISO 8601 timestamp string or a parquet timestamp column",
            context={"source_path": source_path, "received_type": type(raw_value).__name__},
        )

    if parsed_value.tzinfo is None:
        if source_timezone is None:
            raise _market_data_ingestion_error(
                field_path,
                "contains a naive timestamp. Provide a timezone-aware value or configure the source timezone explicitly.",
                context={"source_path": source_path},
            )

        return _localize_naive_datetime(
            parsed_value,
            source_timezone=source_timezone,
            field_path=field_path,
            source_path=source_path,
        ).astimezone(UTC)

    return parsed_value.astimezone(UTC)


def _localize_naive_datetime(
    parsed_value: datetime,
    *,
    source_timezone: str,
    field_path: str,
    source_path: str,
) -> datetime:
    try:
        time_zone = ZoneInfo(source_timezone)
    except ZoneInfoNotFoundError as e:
        raise _market_data_ingestion_error(
            field_path,
            f"references unknown timezone '{source_timezone}'",
            context={"source_path": source_path},
        ) from e

    aware_fold_zero = parsed_value.replace(tzinfo=time_zone, fold=0)
    aware_fold_one = parsed_value.replace(tzinfo=time_zone, fold=1)
    roundtrip_fold_zero = aware_fold_zero.astimezone(UTC).astimezone(time_zone).replace(tzinfo=None)
    roundtrip_fold_one = aware_fold_one.astimezone(UTC).astimezone(time_zone).replace(tzinfo=None)

    if roundtrip_fold_zero != parsed_value and roundtrip_fold_one != parsed_value:
        raise _market_data_ingestion_error(
            field_path,
            "falls into a nonexistent local-time gap. Provide explicit UTC offsets or normalize the source file.",
            context={"source_path": source_path, "source_timezone": source_timezone},
        )

    if (
        roundtrip_fold_zero == parsed_value
        and roundtrip_fold_one == parsed_value
        and aware_fold_zero.utcoffset() != aware_fold_one.utcoffset()
    ):
        raise _market_data_ingestion_error(
            field_path,
            "is ambiguous in the configured source timezone due to a daylight-saving transition. "
            "Provide explicit UTC offsets in the file.",
            context={"source_path": source_path, "source_timezone": source_timezone},
        )

    if roundtrip_fold_zero == parsed_value:
        return aware_fold_zero

    return aware_fold_one


def _normalize_required_numeric_value(
    raw_value: object,
    *,
    field_path: str,
    source_path: str,
    allow_zero: bool,
    allow_negative: bool,
) -> float:
    normalized_value = _normalize_optional_numeric_value(
        raw_value,
        field_path=field_path,
        source_path=source_path,
        allow_zero=allow_zero,
        allow_negative=allow_negative,
    )
    if normalized_value is None:
        raise _market_data_ingestion_error(
            field_path,
            "is required and cannot be blank",
            context={"source_path": source_path},
        )

    return normalized_value


def _normalize_optional_numeric_value(
    raw_value: object,
    *,
    field_path: str,
    source_path: str,
    allow_zero: bool,
    allow_negative: bool,
) -> Optional[float]:
    if raw_value is None:
        return None

    if isinstance(raw_value, str):
        stripped_value = raw_value.strip()
        if not stripped_value:
            return None

        try:
            numeric_value = float(stripped_value)
        except ValueError as e:
            raise _market_data_ingestion_error(
                field_path,
                "must be a finite numeric value when provided",
                context={"source_path": source_path, "raw_value": stripped_value},
            ) from e
    else:
        if isinstance(raw_value, bool) or not isinstance(raw_value, int | float):
            raise _market_data_ingestion_error(
                field_path,
                "must be a finite numeric value when provided",
                context={"source_path": source_path, "received_type": type(raw_value).__name__},
            )

        numeric_value = float(raw_value)

    if not isfinite(numeric_value):
        raise _market_data_ingestion_error(
            field_path,
            "must be a finite numeric value when provided",
            context={"source_path": source_path},
        )

    if not allow_negative and numeric_value < 0.0:
        raise _market_data_ingestion_error(
            field_path,
            "must be non-negative",
            context={"source_path": source_path},
        )

    if not allow_zero and numeric_value <= 0.0:
        raise _market_data_ingestion_error(
            field_path,
            "must be strictly positive",
            context={"source_path": source_path},
        )

    return numeric_value


def _normalize_option_type(
    raw_value: object,
    *,
    field_path: str,
    source_path: str,
) -> str:
    text_value = _normalize_required_text_value(
        raw_value,
        field_path=field_path,
        source_path=source_path,
    ).lower()
    match text_value:
        case "call" | "c":
            return "call"
        case "put" | "p":
            return "put"
        case _:
            raise _market_data_ingestion_error(
                field_path,
                "must be one of: call, put, c, p",
                context={"source_path": source_path, "raw_value": text_value},
            )


def _normalize_required_text_value(
    raw_value: object,
    *,
    field_path: str,
    source_path: str,
) -> str:
    normalized_value = _normalize_optional_text_value(
        raw_value,
        field_path=field_path,
        source_path=source_path,
    )
    if normalized_value is None:
        raise _market_data_ingestion_error(
            field_path,
            "is required and cannot be blank",
            context={"source_path": source_path},
        )

    return normalized_value


def _normalize_optional_text_value(
    raw_value: object,
    *,
    field_path: str,
    source_path: str,
) -> Optional[str]:
    if raw_value is None:
        return None
    if not isinstance(raw_value, str):
        raise _market_data_ingestion_error(
            field_path,
            "must be a string when provided",
            context={"source_path": source_path, "received_type": type(raw_value).__name__},
        )

    stripped_value = raw_value.strip()
    if not stripped_value:
        return None

    return stripped_value


def _validate_mapped_columns_exist(
    *,
    available_columns: Sequence[str],
    required_columns: Sequence[str],
    field_path: str,
    source_path: str,
) -> None:
    available_column_set = set(available_columns)
    missing_columns = [column_name for column_name in required_columns if column_name not in available_column_set]
    if not missing_columns:
        return

    missing_column_text = ", ".join(missing_columns)
    available_column_text = ", ".join(available_columns)

    raise _market_data_ingestion_error(
        field_path,
        "references source columns that are not present in the file. "
        f"Missing columns: {missing_column_text}. Available columns: {available_column_text}. "
        "Update the explicit column mapping so Janiform does not have to guess the schema.",
        context={"source_path": source_path},
    )
