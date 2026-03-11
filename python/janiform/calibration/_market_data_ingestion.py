import csv
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta, tzinfo
from math import isfinite
from typing import Optional, Protocol
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

import pyarrow
from pyarrow import compute as pyarrow_compute
from pyarrow import parquet as pyarrow_parquet

from .._timestamps import parse_timestamp_value
from ._common import (
    CalibrationTradeSign,
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
    _calibration_error,
)
from ._requests import MarketDataCalibrationRequest
from .._contracts import ContractModel


CALIBRATION_MARKET_DATA_INGESTION_ERROR_ID = "calibration.validation.market_data_ingestion"
CALIBRATION_MARKET_DATA_INGESTION_ENTRYPOINT = "market_data_ingestion"


@dataclass(frozen=True, slots=True)
class NormalizedTickDataRecord(ContractModel):
    timestamp: datetime
    price: Optional[float]
    trade_price: Optional[float]
    trade_sign: Optional[CalibrationTradeSign]
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


@dataclass(frozen=True, slots=True)
class _TradeSignResolutionDiagnostics:
    policy_id: str
    first_inferred_trade_sign: CalibrationTradeSign
    provided_label_count: int
    inferred_label_count: int
    flat_move_carry_count: int
    price_up_inference_count: int
    price_down_inference_count: int


@dataclass(frozen=True, slots=True)
class _NormalizedTickRow:
    timestamp: datetime
    price: Optional[float]
    trade_price: Optional[float]
    trade_sign: Optional[CalibrationTradeSign]
    bid_price: Optional[float]
    ask_price: Optional[float]
    close_price: Optional[float]
    source_row_number: int


@dataclass(frozen=True, slots=True)
class _LoadedTickDataNormalizationResult:
    tick_data: LoadedTickDataSource
    trade_sign_resolution_diagnostics: Optional[_TradeSignResolutionDiagnostics]


@dataclass(frozen=True, slots=True)
class _LoadedMarketDataCalibrationInputsResult:
    inputs: LoadedMarketDataCalibrationInputs
    trade_sign_resolution_diagnostics: Optional[_TradeSignResolutionDiagnostics]


def load_market_data_calibration_inputs(
    request: MarketDataCalibrationRequest,
) -> LoadedMarketDataCalibrationInputs:
    return _load_market_data_calibration_inputs(
        request,
        resolve_unsigned_trade_signs=False,
    ).inputs


def _load_market_data_calibration_inputs(
    request: MarketDataCalibrationRequest,
    *,
    resolve_unsigned_trade_signs: bool,
) -> _LoadedMarketDataCalibrationInputsResult:
    tick_source_rows = _load_source_rows(
        request.tick_data_source,
        field_path="tick_data_source",
    )
    tick_data_result = _normalize_tick_data(
        source_rows=tick_source_rows,
        file_reference=request.tick_data_source,
        column_mapping=request.column_mapping.tick_data,
        normalization_policy=request.normalization_policy,
        resolve_unsigned_trade_signs=resolve_unsigned_trade_signs,
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

    return _LoadedMarketDataCalibrationInputsResult(
        inputs=LoadedMarketDataCalibrationInputs(
            tick_data=tick_data_result.tick_data,
            option_surface=option_surface,
            known_news=known_news,
        ),
        trade_sign_resolution_diagnostics=tick_data_result.trade_sign_resolution_diagnostics,
    )


def _market_data_ingestion_error(
    field_path: str,
    message: str,
    *,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
    normalization_step: Optional[str] = None,
) -> Exception:
    error_context: dict[str, str | int | float | bool | None] = dict(context or {})
    if normalization_step is not None:
        error_context["normalization_step"] = normalization_step

    return _calibration_error(
        field_path,
        message,
        entrypoint=CALIBRATION_MARKET_DATA_INGESTION_ENTRYPOINT,
        error_id=CALIBRATION_MARKET_DATA_INGESTION_ERROR_ID,
        context=error_context or None,
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
                    normalization_step="source_loading",
                )
    except FileNotFoundError as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            "does not exist",
            context={"path": file_reference.path},
            normalization_step="source_loading",
        ) from e
    except OSError as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be read: {e}",
            context={"path": file_reference.path},
            normalization_step="source_loading",
        ) from e
    except pyarrow.ArrowInvalid as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be parsed as {file_reference.file_format.value}: {e}",
            context={"path": file_reference.path},
            normalization_step="source_loading",
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
                    normalization_step="source_loading",
                )

            rows = tuple(dict(source_row) for source_row in reader)
    except FileNotFoundError as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            "does not exist",
            context={"path": file_reference.path},
            normalization_step="source_loading",
        ) from e
    except OSError as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be read: {e}",
            context={"path": file_reference.path},
            normalization_step="source_loading",
        ) from e
    except csv.Error as e:
        raise _market_data_ingestion_error(
            f"{field_path}.path",
            f"could not be parsed as csv: {e}",
            context={"path": file_reference.path},
            normalization_step="source_loading",
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
    resolve_unsigned_trade_signs: bool,
) -> _LoadedTickDataNormalizationResult:
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

    if column_mapping.trade_sign_column is not None:
        required_columns.append(column_mapping.trade_sign_column)

    _validate_mapped_columns_exist(
        available_columns=source_rows.available_columns,
        required_columns=tuple(required_columns),
        field_path="column_mapping.tick_data",
        source_path=file_reference.path,
    )

    normalized_rows: tuple[_NormalizedTickRow, ...] = tuple(
        _normalize_tick_row(
            source_row,
            source_row_number=source_row_number,
            column_mapping=column_mapping,
            normalization_policy=normalization_policy,
            source_path=file_reference.path,
        )
        for source_row_number, source_row in enumerate(source_rows.rows, start=1)
    )

    deduplicated_rows, dropped_duplicate_row_count = _deduplicate_records(
        normalized_rows,
        key_builder=lambda record: (
            record.timestamp,
            record.price,
            record.trade_price,
            record.trade_sign,
            record.bid_price,
            record.ask_price,
            record.close_price,
        ),
        duplicate_record_policy=normalization_policy.duplicate_record_policy,
        field_path="tick_data_source",
        source_path=file_reference.path,
    )

    if normalization_policy.sort_records_by_timestamp:
        deduplicated_rows = tuple(
            sorted(
                deduplicated_rows,
                key=lambda record: (record.timestamp, record.source_row_number),
            )
        )

    resolved_records, trade_sign_resolution_diagnostics = _finalize_tick_data_records(
        deduplicated_rows,
        resolve_unsigned_trade_signs=resolve_unsigned_trade_signs,
        source_path=file_reference.path,
    )

    return _LoadedTickDataNormalizationResult(
        tick_data=LoadedTickDataSource(
            records=resolved_records,
            summary=LoadedMarketDataSourceSummary(
                path=file_reference.path,
                file_format=file_reference.file_format,
                provenance_label=file_reference.provenance_label,
                source_row_count=len(source_rows.rows),
                retained_row_count=len(resolved_records),
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
        ),
        trade_sign_resolution_diagnostics=trade_sign_resolution_diagnostics,
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
) -> _NormalizedTickRow:
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
    trade_sign = _normalize_optional_trade_sign_value(
        source_row.get(column_mapping.trade_sign_column) if column_mapping.trade_sign_column is not None else None,
        field_path=f"{row_field_path}.{column_mapping.trade_sign_column}"
        if column_mapping.trade_sign_column is not None
        else row_field_path,
        source_path=source_path,
        source_row_number=source_row_number,
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

    return _NormalizedTickRow(
        timestamp=timestamp,
        price=price,
        trade_price=trade_price,
        trade_sign=trade_sign,
        bid_price=bid_price,
        ask_price=ask_price,
        close_price=close_price,
        source_row_number=source_row_number,
    )


def _finalize_tick_data_records(
    normalized_rows: Sequence[_NormalizedTickRow],
    *,
    resolve_unsigned_trade_signs: bool,
    source_path: str,
) -> tuple[tuple[NormalizedTickDataRecord, ...], Optional[_TradeSignResolutionDiagnostics]]:
    resolved_signs: tuple[Optional[CalibrationTradeSign], ...]
    trade_sign_resolution_diagnostics: Optional[_TradeSignResolutionDiagnostics]

    if resolve_unsigned_trade_signs:
        resolved_signs, trade_sign_resolution_diagnostics = _resolve_trade_signs_for_tick_rows(
            normalized_rows,
            source_path=source_path,
        )
    else:
        resolved_signs = tuple(row.trade_sign for row in normalized_rows)
        trade_sign_resolution_diagnostics = None

    finalized_records = tuple(
        _build_normalized_tick_record(normalized_row, trade_sign=trade_sign)
        for normalized_row, trade_sign in zip(normalized_rows, resolved_signs, strict=True)
    )

    return finalized_records, trade_sign_resolution_diagnostics


def _build_normalized_tick_record(
    normalized_row: _NormalizedTickRow,
    *,
    trade_sign: Optional[CalibrationTradeSign],
) -> NormalizedTickDataRecord:
    return NormalizedTickDataRecord(
        timestamp=normalized_row.timestamp,
        price=normalized_row.price,
        trade_price=normalized_row.trade_price,
        trade_sign=trade_sign,
        bid_price=normalized_row.bid_price,
        ask_price=normalized_row.ask_price,
        close_price=normalized_row.close_price,
        source_row_number=normalized_row.source_row_number,
    )


def _resolve_trade_signs_for_tick_rows(
    normalized_rows: Sequence[_NormalizedTickRow],
    *,
    source_path: str,
) -> tuple[tuple[Optional[CalibrationTradeSign], ...], _TradeSignResolutionDiagnostics]:
    if len(normalized_rows) == 0:
        return (
            (),
            _TradeSignResolutionDiagnostics(
                policy_id="tick_price_change_with_flat_move_carryover.v1",
                first_inferred_trade_sign=CalibrationTradeSign.BUY,
                provided_label_count=0,
                inferred_label_count=0,
                flat_move_carry_count=0,
                price_up_inference_count=0,
                price_down_inference_count=0,
            ),
        )

    price_array = pyarrow.array(
        [normalized_row.price for normalized_row in normalized_rows],
        type=pyarrow.float64(),
    )
    provided_sign_codes = pyarrow.array(
        [_trade_sign_code(normalized_row.trade_sign) for normalized_row in normalized_rows],
        type=pyarrow.int8(),
    )
    missing_sign_mask = pyarrow_compute.is_null(provided_sign_codes)
    inferred_label_count = _count_true_values(missing_sign_mask)

    if inferred_label_count == 0:
        return (
            tuple(normalized_row.trade_sign for normalized_row in normalized_rows),
            _TradeSignResolutionDiagnostics(
                policy_id="tick_price_change_with_flat_move_carryover.v1",
                first_inferred_trade_sign=CalibrationTradeSign.BUY,
                provided_label_count=len(normalized_rows),
                inferred_label_count=0,
                flat_move_carry_count=0,
                price_up_inference_count=0,
                price_down_inference_count=0,
            ),
        )

    missing_price_mask = pyarrow_compute.and_(
        missing_sign_mask,
        pyarrow_compute.is_null(price_array),
    )
    missing_price_index = _first_true_index(missing_price_mask)
    if missing_price_index is not None:
        raise _market_data_ingestion_error(
            "column_mapping.tick_data.trade_sign_column",
            "trade sign inference requires a canonical tick price for every retained trade event",
            context={
                "source_path": source_path,
                "source_row_number": normalized_rows[missing_price_index].source_row_number,
            },
            normalization_step="trade_sign_inference",
        )

    carried_forward_prices = pyarrow_compute.fill_null_forward(price_array)
    previous_prices = pyarrow.concat_arrays(
        (
            pyarrow.array([None], type=pyarrow.float64()),
            carried_forward_prices.slice(0, len(normalized_rows) - 1),
        )
    )
    price_up_mask = pyarrow_compute.and_(
        missing_sign_mask,
        pyarrow_compute.fill_null(
            pyarrow_compute.greater(price_array, previous_prices),
            False,
        ),
    )
    price_down_mask = pyarrow_compute.and_(
        missing_sign_mask,
        pyarrow_compute.fill_null(
            pyarrow_compute.less(price_array, previous_prices),
            False,
        ),
    )
    first_inference_mask = pyarrow_compute.and_(
        missing_sign_mask,
        pyarrow_compute.is_null(previous_prices),
    )
    buy_sign_code = pyarrow.scalar(1, type=pyarrow.int8())
    sell_sign_code = pyarrow.scalar(-1, type=pyarrow.int8())
    null_sign_code = pyarrow.scalar(None, type=pyarrow.int8())
    inferred_seed_codes = pyarrow_compute.if_else(
        price_up_mask,
        buy_sign_code,
        pyarrow_compute.if_else(
            price_down_mask,
            sell_sign_code,
            pyarrow_compute.if_else(
                first_inference_mask,
                buy_sign_code,
                null_sign_code,
            ),
        ),
    )
    resolved_sign_codes = pyarrow_compute.fill_null_forward(
        pyarrow_compute.coalesce(provided_sign_codes, inferred_seed_codes)
    )
    resolved_signs = tuple(
        _trade_sign_from_code(resolved_sign_code)
        for resolved_sign_code in resolved_sign_codes.to_pylist()
    )
    price_up_inference_count = _count_true_values(price_up_mask)
    price_down_inference_count = _count_true_values(price_down_mask)
    flat_move_carry_count = (
        inferred_label_count
        - _count_true_values(first_inference_mask)
        - price_up_inference_count
        - price_down_inference_count
    )

    return (
        resolved_signs,
        _TradeSignResolutionDiagnostics(
            policy_id="tick_price_change_with_flat_move_carryover.v1",
            first_inferred_trade_sign=CalibrationTradeSign.BUY,
            provided_label_count=len(normalized_rows) - inferred_label_count,
            inferred_label_count=inferred_label_count,
            flat_move_carry_count=flat_move_carry_count,
            price_up_inference_count=price_up_inference_count,
            price_down_inference_count=price_down_inference_count,
        ),
    )


def _trade_sign_code(trade_sign: Optional[CalibrationTradeSign]) -> Optional[int]:
    match trade_sign:
        case CalibrationTradeSign.BUY:
            return 1
        case CalibrationTradeSign.SELL:
            return -1
        case None:
            return None


def _trade_sign_from_code(trade_sign_code: object) -> Optional[CalibrationTradeSign]:
    match trade_sign_code:
        case 1:
            return CalibrationTradeSign.BUY
        case -1:
            return CalibrationTradeSign.SELL
        case None:
            return None
        case _:
            raise ValueError(f"Unsupported trade sign code: {trade_sign_code!r}")


def _count_true_values(mask: pyarrow.Array) -> int:
    count = pyarrow_compute.sum(
        pyarrow_compute.cast(mask, pyarrow.int64())
    ).as_py()
    return int(count or 0)


def _first_true_index(mask: pyarrow.Array) -> Optional[int]:
    for index, value in enumerate(mask.to_pylist()):
        if value:
            return index

    return None


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
                        normalization_step="duplicate_record_normalization",
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
            normalization_step="timestamp_normalization",
        )

    if isinstance(raw_value, str):
        stripped_value = raw_value.strip()
        if not stripped_value:
            raise _market_data_ingestion_error(
                field_path,
                "is required and cannot be blank",
                context={"source_path": source_path},
                normalization_step="timestamp_normalization",
            )

        try:
            parsed_value = parse_timestamp_value(stripped_value)
        except (TypeError, ValueError) as e:
            raise _market_data_ingestion_error(
                field_path,
                "must be a supported timestamp string, Unix epoch value, or a parquet timestamp column",
                context={"source_path": source_path, "raw_value": stripped_value},
                normalization_step="timestamp_normalization",
            ) from e
    elif isinstance(raw_value, datetime | int | float):
        try:
            parsed_value = parse_timestamp_value(raw_value)
        except (TypeError, ValueError) as e:
            raise _market_data_ingestion_error(
                field_path,
                "must be a supported timestamp string, Unix epoch value, or a parquet timestamp column",
                context={"source_path": source_path, "raw_value": raw_value},
                normalization_step="timestamp_normalization",
            ) from e
    else:
        raise _market_data_ingestion_error(
            field_path,
            "must be a supported timestamp string, Unix epoch value, or a parquet timestamp column",
            context={"source_path": source_path, "received_type": type(raw_value).__name__},
            normalization_step="timestamp_normalization",
        )

    if parsed_value.tzinfo is None:
        if source_timezone is None:
            raise _market_data_ingestion_error(
                field_path,
                "contains a naive timestamp. Provide a timezone-aware value or configure the source timezone explicitly.",
                context={"source_path": source_path},
                normalization_step="timestamp_normalization",
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
        time_zone = _resolve_source_timezone(source_timezone)
    except ZoneInfoNotFoundError as e:
        raise _market_data_ingestion_error(
            field_path,
            f"references unknown timezone '{source_timezone}'",
            context={"source_path": source_path},
            normalization_step="timestamp_timezone_normalization",
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
            normalization_step="timestamp_timezone_normalization",
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
            normalization_step="timestamp_timezone_normalization",
        )

    if roundtrip_fold_zero == parsed_value:
        return aware_fold_zero

    return aware_fold_one


def _resolve_source_timezone(source_timezone: str) -> tzinfo:
    normalized_timezone = source_timezone.strip()
    if normalized_timezone.upper() in {"UTC", "Z", "GMT", "UT"}:
        return UTC

    return ZoneInfo(normalized_timezone)


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
            normalization_step="numeric_value_normalization",
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
                normalization_step="numeric_value_normalization",
            ) from e
    else:
        if isinstance(raw_value, bool) or not isinstance(raw_value, int | float):
            raise _market_data_ingestion_error(
                field_path,
                "must be a finite numeric value when provided",
                context={"source_path": source_path, "received_type": type(raw_value).__name__},
                normalization_step="numeric_value_normalization",
            )

        numeric_value = float(raw_value)

    if not isfinite(numeric_value):
        raise _market_data_ingestion_error(
            field_path,
            "must be a finite numeric value when provided",
            context={"source_path": source_path},
            normalization_step="numeric_value_normalization",
        )

    if not allow_negative and numeric_value < 0.0:
        raise _market_data_ingestion_error(
            field_path,
            "must be non-negative",
            context={"source_path": source_path},
            normalization_step="numeric_value_normalization",
        )

    if not allow_zero and numeric_value <= 0.0:
        raise _market_data_ingestion_error(
            field_path,
            "must be strictly positive",
            context={"source_path": source_path},
            normalization_step="numeric_value_normalization",
        )

    return numeric_value


def _normalize_optional_trade_sign_value(
    raw_value: object,
    *,
    field_path: str,
    source_path: str,
    source_row_number: int,
) -> Optional[CalibrationTradeSign]:
    if raw_value is None:
        return None

    if not isinstance(raw_value, str):
        raise _market_data_ingestion_error(
            field_path,
            "must be a string when provided",
            context={
                "source_path": source_path,
                "source_row_number": source_row_number,
                "received_type": type(raw_value).__name__,
            },
            normalization_step="trade_sign_normalization",
        )

    normalized_value = raw_value.strip().lower()
    if not normalized_value:
        return None

    try:
        return CalibrationTradeSign(normalized_value)
    except ValueError as e:
        raise _market_data_ingestion_error(
            field_path,
            "must be either 'buy' or 'sell' when provided",
            context={
                "source_path": source_path,
                "source_row_number": source_row_number,
                "raw_value": raw_value,
            },
            normalization_step="trade_sign_normalization",
        ) from e


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
                normalization_step="option_type_normalization",
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
            normalization_step="text_value_normalization",
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
            normalization_step="text_value_normalization",
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
        normalization_step="column_mapping_validation",
    )
