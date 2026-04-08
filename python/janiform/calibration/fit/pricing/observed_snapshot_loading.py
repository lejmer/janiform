from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Optional

import pandas
from pyarrow import parquet as pyarrow_parquet

from ....contracts import require_positive_finite_scalar, require_timezone_aware_datetime
from ....pricing import VanillaOptionPayoffType
from ...errors import calibration_error
from ...market_data.contracts import CalibrationOptionSurfaceColumnMapping, CalibrationTickDataColumnMapping
from ..inference.aliases import GENERIC_COLUMN_ALIASES


type TabularFrameInput = str | Path | pandas.DataFrame | Mapping[str, Sequence[Any]]


_OPTION_QUOTE_TIMESTAMP_ALIASES = (
    "quote_timestamp",
    "quote_timestamp_utc",
    "timestamp",
    "timestamp_utc",
    "quote_time",
    "quote_datetime",
    "event_timestamp_utc",
    "event_timestamp",
    "ts",
    "ts_recv",
)
_OPTION_EXPIRY_TIMESTAMP_ALIASES = (
    "expiry_timestamp",
    "expiry_timestamp_utc",
    "expiry_utc",
    "expiration_timestamp",
    "expiration_timestamp_utc",
    "expiration_utc",
    "maturity_timestamp",
    "maturity_utc",
    "expiry",
    "expiration",
)
_OPTION_STRIKE_ALIASES = (
    "strike",
    "strike_price",
    "strikeprice",
    "exercise_price",
    "exerciseprice",
    "k",
)
_OPTION_TYPE_ALIASES = (
    "option_type",
    "option_kind",
    "payoff_type",
    "right",
    "call_put",
    "put_call",
    "cp",
)
_OPTION_MID_PRICE_ALIASES = (
    "mid_price",
    "midprice",
    "mid",
    "midpoint",
    "mid_point",
    "mark_price",
)
_OPTION_BID_ALIASES = ("bid", "bid_price", "bidprice", "best_bid", "bid_px", "bestbid")
_OPTION_ASK_ALIASES = ("ask", "ask_price", "askprice", "best_ask", "ask_px", "bestask")


@dataclass(frozen=True, slots=True)
class ResolvedOptionSnapshotColumns:
    quote_timestamp_column: str
    expiry_timestamp_column: str
    strike_column: str
    option_type_column: str
    mid_price_column: Optional[str]
    bid_column: Optional[str]
    ask_column: Optional[str]


def load_tick_data_frame(
    tick_data: TabularFrameInput,
    *,
    tick_column_mapping: CalibrationTickDataColumnMapping,
) -> tuple[pandas.DataFrame, CalibrationTickDataColumnMapping]:
    available_columns = load_source_columns_for_tabular_input(
        tick_data,
        field_path="tick_data",
    )
    resolved_tick_column_mapping = resolve_observed_tick_column_mapping(
        available_columns,
        tick_column_mapping=tick_column_mapping,
    )
    required_columns = tuple(
        column_name
        for column_name in (
            resolved_tick_column_mapping.timestamp_column,
            resolved_tick_column_mapping.price_column,
            resolved_tick_column_mapping.trade_sign_column,
        )
        if column_name is not None
    )
    frame = load_tabular_frame(
        tick_data,
        field_path="tick_data",
        required_columns=required_columns,
    )

    return frame, resolved_tick_column_mapping


def load_option_surface_frame(
    option_surface: TabularFrameInput,
    *,
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping],
) -> tuple[pandas.DataFrame, ResolvedOptionSnapshotColumns]:
    available_columns = load_source_columns_for_tabular_input(
        option_surface,
        field_path="option_surface",
    )
    resolved_option_columns = resolve_option_snapshot_columns(
        available_columns,
        option_surface_column_mapping=option_surface_column_mapping,
    )
    required_columns = tuple(
        column_name
        for column_name in (
            resolved_option_columns.quote_timestamp_column,
            resolved_option_columns.expiry_timestamp_column,
            resolved_option_columns.strike_column,
            resolved_option_columns.option_type_column,
            resolved_option_columns.mid_price_column,
            resolved_option_columns.bid_column,
            resolved_option_columns.ask_column,
        )
        if column_name is not None
    )
    frame = load_tabular_frame(
        option_surface,
        field_path="option_surface",
        required_columns=required_columns,
    )

    return frame, resolved_option_columns


def load_source_columns_for_tabular_input(
    tabular_input: TabularFrameInput,
    *,
    field_path: str,
) -> tuple[str, ...]:
    if isinstance(tabular_input, pandas.DataFrame):
        return tuple(str(column_name) for column_name in tabular_input.columns)

    if isinstance(tabular_input, Mapping):
        return tuple(str(column_name) for column_name in tabular_input.keys())

    input_path = Path(tabular_input)
    suffix = input_path.suffix.lower()

    if suffix in (".parquet", ".pq"):
        return tuple(pyarrow_parquet.read_schema(input_path).names)

    if suffix == ".csv":
        return tuple(str(column_name) for column_name in pandas.read_csv(input_path, nrows=0).columns)

    raise calibration_error(
        field_path,
        "must be a pandas.DataFrame, a mapping of column arrays, or a .csv/.parquet path",
    )


def load_tabular_frame(
    tabular_input: TabularFrameInput,
    *,
    field_path: str,
    required_columns: Sequence[str],
) -> pandas.DataFrame:
    if isinstance(tabular_input, pandas.DataFrame):
        return tabular_input.copy()

    if isinstance(tabular_input, Mapping):
        return pandas.DataFrame(tabular_input)

    input_path = Path(tabular_input)
    suffix = input_path.suffix.lower()

    if suffix in (".parquet", ".pq"):
        columns = list(required_columns) if len(required_columns) > 0 else None
        return pandas.read_parquet(input_path, columns=columns)

    if suffix == ".csv":
        use_columns = list(required_columns) if len(required_columns) > 0 else None
        return pandas.read_csv(input_path, usecols=use_columns)

    raise calibration_error(
        field_path,
        "must be a pandas.DataFrame, a mapping of column arrays, or a .csv/.parquet path",
    )


def resolve_option_snapshot_columns(
    available_columns: Sequence[Any],
    *,
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping],
) -> ResolvedOptionSnapshotColumns:
    available_by_normalized = {
        normalize_identifier(str(column_name)): str(column_name)
        for column_name in available_columns
    }
    explicit_mapping = (
        CalibrationOptionSurfaceColumnMapping()
        if option_surface_column_mapping is None
        else option_surface_column_mapping
    )

    return ResolvedOptionSnapshotColumns(
        quote_timestamp_column=resolve_required_column(
            explicit_mapping.quote_timestamp_column,
            aliases=_OPTION_QUOTE_TIMESTAMP_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.quote_timestamp_column",
        ),
        expiry_timestamp_column=resolve_required_column(
            explicit_mapping.expiry_timestamp_column,
            aliases=_OPTION_EXPIRY_TIMESTAMP_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.expiry_timestamp_column",
        ),
        strike_column=resolve_required_column(
            explicit_mapping.strike_column,
            aliases=_OPTION_STRIKE_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.strike_column",
        ),
        option_type_column=resolve_required_column(
            explicit_mapping.option_type_column,
            aliases=_OPTION_TYPE_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.option_type_column",
        ),
        mid_price_column=resolve_optional_column(
            explicit_mapping.mid_price_column,
            aliases=_OPTION_MID_PRICE_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.mid_price_column",
        ),
        bid_column=resolve_optional_column(
            explicit_mapping.bid_column,
            aliases=_OPTION_BID_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.bid_column",
        ),
        ask_column=resolve_optional_column(
            explicit_mapping.ask_column,
            aliases=_OPTION_ASK_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.ask_column",
        ),
    )


def resolve_observed_tick_column_mapping(
    available_columns: Sequence[str],
    *,
    tick_column_mapping: CalibrationTickDataColumnMapping,
) -> CalibrationTickDataColumnMapping:
    available_by_normalized = {
        normalize_identifier(column_name): column_name
        for column_name in available_columns
    }

    return CalibrationTickDataColumnMapping(
        timestamp_column=resolve_required_column(
            optional_existing_column_name(
                tick_column_mapping.timestamp_column,
                available_by_normalized=available_by_normalized,
            ),
            aliases=GENERIC_COLUMN_ALIASES["timestamp"],
            available_by_normalized=available_by_normalized,
            field_path="tick_data.timestamp_column",
        ),
        price_column=resolve_required_column(
            optional_existing_column_name(
                tick_column_mapping.price_column,
                available_by_normalized=available_by_normalized,
            ),
            aliases=GENERIC_COLUMN_ALIASES["price"],
            available_by_normalized=available_by_normalized,
            field_path="tick_data.price_column",
        ),
        trade_sign_column=resolve_required_column(
            optional_existing_column_name(
                tick_column_mapping.trade_sign_column,
                available_by_normalized=available_by_normalized,
            ),
            aliases=GENERIC_COLUMN_ALIASES["trade_sign"],
            available_by_normalized=available_by_normalized,
            field_path="tick_data.trade_sign_column",
        ),
    )


def resolve_snapshot_valuation_time_utc(
    *,
    option_surface_frame: pandas.DataFrame,
    quote_timestamp_column: str,
    valuation_time_utc: Optional[datetime],
) -> datetime:
    quote_timestamps = pandas.to_datetime(
        option_surface_frame[quote_timestamp_column],
        utc=True,
        errors="raise",
    )

    if valuation_time_utc is not None:
        require_timezone_aware_datetime(
            valuation_time_utc,
            field_path="valuation_time_utc",
            error_factory=calibration_error,
        )
        return valuation_time_utc.astimezone(UTC)

    unique_quote_timestamps = tuple(
        quote_timestamps.sort_values().drop_duplicates().dt.to_pydatetime()
    )
    if len(unique_quote_timestamps) == 1:
        return unique_quote_timestamps[0].astimezone(UTC)

    raise calibration_error(
        "valuation_time_utc",
        "is required when option_surface contains more than one quote timestamp",
    )


def filter_observed_tick_data(
    *,
    tick_data_frame: pandas.DataFrame,
    timestamp_column: Optional[str],
    valuation_time_utc: datetime,
) -> pandas.DataFrame:
    if timestamp_column is None:
        raise calibration_error(
            "tick_data_column_mapping.timestamp_column",
            "is required for current-state estimation",
        )

    tick_timestamps = pandas.to_datetime(
        tick_data_frame[timestamp_column],
        utc=True,
        errors="raise",
    )
    filtered = tick_data_frame.loc[tick_timestamps <= valuation_time_utc].copy()
    if filtered.empty:
        raise calibration_error(
            "tick_data",
            "does not contain any observations at or before valuation_time_utc",
        )

    return filtered


def observed_spot_from_tick_data(
    tick_data_frame: pandas.DataFrame,
    *,
    price_column: Optional[str],
) -> float:
    if price_column is None:
        raise calibration_error(
            "tick_data_column_mapping.price_column",
            "is required to derive observed spot",
        )

    if tick_data_frame.empty:
        raise calibration_error("tick_data", "must contain at least one observation")

    return require_positive_finite_scalar(
        tick_data_frame[price_column].iloc[-1],
        "tick_data.last_price",
        calibration_error,
    )


def normalize_identifier(identifier: str) -> str:
    return "".join(
        character.lower()
        for character in identifier
        if character.isascii() and character.isalnum()
    )


def normalize_requested_option_type(
    option_type: Optional[VanillaOptionPayoffType | str],
) -> Optional[VanillaOptionPayoffType]:
    if option_type is None:
        return None

    if isinstance(option_type, VanillaOptionPayoffType):
        return option_type

    normalized = option_type.strip().lower()
    if normalized in ("call", "c"):
        return VanillaOptionPayoffType.CALL
    if normalized in ("put", "p"):
        return VanillaOptionPayoffType.PUT

    raise calibration_error("option_type", "must be one of: call, put")


def normalize_observed_option_type(
    raw_value: Any,
    *,
    field_path: str,
) -> VanillaOptionPayoffType:
    if not isinstance(raw_value, str):
        raise calibration_error(field_path, "must be a string")

    normalized = normalize_requested_option_type(raw_value)
    if normalized is None:
        raise RuntimeError("option type normalization returned null")

    return normalized


def optional_numeric_value(
    row: Any,
    *,
    column_name: Optional[str],
) -> Optional[float]:
    if column_name is None:
        return None

    raw_value = row[column_name]
    if pandas.isna(raw_value):
        return None

    return float(raw_value)


def resolve_required_column(
    explicit_column: Optional[str],
    *,
    aliases: Sequence[str],
    available_by_normalized: Mapping[str, str],
    field_path: str,
) -> str:
    resolved = resolve_optional_column(
        explicit_column,
        aliases=aliases,
        available_by_normalized=available_by_normalized,
        field_path=field_path,
    )
    if resolved is None:
        raise calibration_error(
            field_path,
            f"could not resolve a source column from common aliases [{', '.join(aliases)}]",
        )

    return resolved


def optional_existing_column_name(
    explicit_column: Optional[str],
    *,
    available_by_normalized: Mapping[str, str],
) -> Optional[str]:
    if explicit_column is None:
        return None

    return available_by_normalized.get(normalize_identifier(explicit_column))


def resolve_optional_column(
    explicit_column: Optional[str],
    *,
    aliases: Sequence[str],
    available_by_normalized: Mapping[str, str],
    field_path: str,
) -> Optional[str]:
    if explicit_column is not None:
        resolved_explicit = available_by_normalized.get(normalize_identifier(explicit_column))
        if resolved_explicit is None:
            raise calibration_error(
                field_path,
                f"explicit column `{explicit_column}` is not present in the source data",
            )

        return resolved_explicit

    for alias in aliases:
        resolved = available_by_normalized.get(normalize_identifier(alias))
        if resolved is not None:
            return resolved

    return None
