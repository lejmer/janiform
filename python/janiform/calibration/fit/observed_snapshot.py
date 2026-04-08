from __future__ import annotations

from collections import OrderedDict
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from datetime import UTC, datetime
from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional

import pandas
from pyarrow import parquet as pyarrow_parquet

from .input_resolution import _GENERIC_COLUMN_ALIASES
from .pricing import (
    JaniformGroupedVanillaOptionBatch,
    JaniformGroupedVanillaOptionGroup,
    JaniformVanillaOptionGroupedBatchPricingResult,
)
from .state_estimation import CurrentStateEstimationRequest
from ..errors import calibration_error
from ..market_data.contracts import (
    CalibrationOptionSurfaceColumnMapping,
    CalibrationTickDataColumnMapping,
)
from ...contracts import (
    require_finite_scalar,
    require_positive_finite_scalar,
    require_timezone_aware_datetime,
)
from ...pricing import (
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffType,
    VanillaOptionPricingFailureStage,
    VanillaOptionPricingStatus,
)
from ..._workflow_progress import WorkflowProgressMode

if TYPE_CHECKING:
    from .model import JaniformModel


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
_OPTION_BID_ALIASES = (
    "bid",
    "bid_price",
    "bidprice",
    "best_bid",
    "bid_px",
    "bestbid",
)
_OPTION_ASK_ALIASES = (
    "ask",
    "ask_price",
    "askprice",
    "best_ask",
    "ask_px",
    "bestask",
)


@dataclass(frozen=True, slots=True)
class JaniformObservedOptionSnapshotNode:
    quote_timestamp_utc: datetime
    expiry_timestamp_utc: datetime
    strike: float
    option_type: VanillaOptionPayoffType
    risk_free_rate: float
    dividend_yield: float
    maturity_interval: float
    observed_bid_price: Optional[float]
    observed_ask_price: Optional[float]
    observed_mid_price: Optional[float]
    model_price: Optional[float]
    model_implied_volatility: Optional[float]
    implied_volatility_inversion_status: Optional[VanillaOptionImpliedVolatilityInversionStatus]
    pricing_status: VanillaOptionPricingStatus
    pricing_failure_stage: Optional[VanillaOptionPricingFailureStage]
    pricing_failure_reason: Optional[str]
    forward_price: Optional[float]
    discount_factor: Optional[float]
    boundary_state_norm: Optional[float]


@dataclass(frozen=True, slots=True)
class JaniformObservedOptionSnapshotPricingResult:
    valuation_time_utc: datetime
    spot: float
    nodes: tuple[JaniformObservedOptionSnapshotNode, ...]
    raw_result: JaniformVanillaOptionGroupedBatchPricingResult = field(repr=False)

    def to_pandas(self) -> Any:
        rows = [
            {
                "quote_timestamp_utc": node.quote_timestamp_utc,
                "expiry_timestamp_utc": node.expiry_timestamp_utc,
                "strike": node.strike,
                "option_type": node.option_type.value,
                "risk_free_rate": node.risk_free_rate,
                "dividend_yield": node.dividend_yield,
                "maturity_interval": node.maturity_interval,
                "observed_bid_price": node.observed_bid_price,
                "observed_ask_price": node.observed_ask_price,
                "observed_mid_price": node.observed_mid_price,
                "model_price": node.model_price,
                "model_implied_volatility": node.model_implied_volatility,
                "implied_volatility_inversion_status": (
                    None
                    if node.implied_volatility_inversion_status is None
                    else node.implied_volatility_inversion_status.value
                ),
                "pricing_status": node.pricing_status.value,
                "pricing_failure_stage": (
                    None
                    if node.pricing_failure_stage is None
                    else node.pricing_failure_stage.value
                ),
                "pricing_failure_reason": node.pricing_failure_reason,
                "forward_price": node.forward_price,
                "discount_factor": node.discount_factor,
                "boundary_state_norm": node.boundary_state_norm,
            }
            for node in self.nodes
        ]

        return pandas.DataFrame.from_records(rows)

    def to_surface_frame(
        self,
        *,
        value_column: str = "model_implied_volatility",
    ) -> Any:
        frame = self.to_pandas()
        if value_column not in frame.columns:
            raise calibration_error(
                "value_column",
                f"must be one of: {', '.join(str(value) for value in frame.columns)}",
            )

        index_columns = (
            ["expiry_timestamp_utc"]
            if frame["option_type"].nunique(dropna=False) == 1
            else ["option_type", "expiry_timestamp_utc"]
        )

        return (
            frame
            .pivot(index=index_columns, columns="strike", values=value_column)
            .sort_index(axis=0)
            .sort_index(axis=1)
        )


@dataclass(frozen=True, slots=True)
class _ResolvedOptionSnapshotColumns:
    quote_timestamp_column: str
    expiry_timestamp_column: str
    strike_column: str
    option_type_column: str
    mid_price_column: Optional[str]
    bid_column: Optional[str]
    ask_column: Optional[str]


@dataclass(frozen=True, slots=True)
class _PreparedSnapshotNodeInput:
    quote_timestamp_utc: datetime
    expiry_timestamp_utc: datetime
    strike: float
    option_type: VanillaOptionPayoffType
    observed_bid_price: Optional[float]
    observed_ask_price: Optional[float]
    observed_mid_price: Optional[float]


@dataclass(frozen=True, slots=True)
class _PreparedSnapshotGroupInput:
    payoff_type: VanillaOptionPayoffType
    expiry_timestamp_utc: datetime
    strikes: tuple[float, ...]
    nodes: tuple[_PreparedSnapshotNodeInput, ...]


def price_observed_option_snapshot_for_model(
    model: JaniformModel,
    *,
    tick_data: TabularFrameInput,
    option_surface: TabularFrameInput,
    valuation_time_utc: Optional[datetime] = None,
    option_type: Optional[VanillaOptionPayoffType | str] = None,
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping] = None,
    risk_free_rate: float = 0.0,
    dividend_yield: float = 0.0,
    cpu_count: Optional[int] = None,
    silent: bool = False,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
) -> JaniformObservedOptionSnapshotPricingResult:
    require_finite_scalar(risk_free_rate, "risk_free_rate", calibration_error)
    require_finite_scalar(dividend_yield, "dividend_yield", calibration_error)

    tick_column_mapping = model.current_state_tick_data_column_mapping(
        request=CurrentStateEstimationRequest(observed_tick_data=pandas.DataFrame()),
    )

    tick_data_frame, resolved_tick_column_mapping = _load_tick_data_frame(
        tick_data,
        tick_column_mapping=tick_column_mapping,
    )

    option_surface_frame, resolved_option_columns = _load_option_surface_frame(
        option_surface,
        option_surface_column_mapping=option_surface_column_mapping,
    )

    effective_valuation_time_utc = _resolve_snapshot_valuation_time_utc(
        option_surface_frame=option_surface_frame,
        quote_timestamp_column=resolved_option_columns.quote_timestamp_column,
        valuation_time_utc=valuation_time_utc,
    )
    normalized_option_type = _normalize_requested_option_type(option_type)

    filtered_tick_data = _filter_observed_tick_data(
        tick_data_frame=tick_data_frame,
        timestamp_column=resolved_tick_column_mapping.timestamp_column,
        valuation_time_utc=effective_valuation_time_utc,
    )
    spot = _observed_spot_from_tick_data(
        filtered_tick_data,
        price_column=resolved_tick_column_mapping.price_column,
    )

    prepared_snapshot_groups = _prepare_snapshot_groups(
        option_surface_frame=option_surface_frame,
        resolved_option_columns=resolved_option_columns,
        valuation_time_utc=effective_valuation_time_utc,
        option_type=normalized_option_type,
    )

    pricing_result = model.price_option_groups(
        valuation_time_utc=effective_valuation_time_utc,
        batch=JaniformGroupedVanillaOptionBatch(
            spot=spot,
            groups=tuple(
                JaniformGroupedVanillaOptionGroup(
                    payoff_type=group.payoff_type,
                    risk_free_rate=risk_free_rate,
                    dividend_yield=dividend_yield,
                    strikes=group.strikes,
                    expiry_timestamps_utc=(group.expiry_timestamp_utc,),
                )
                for group in prepared_snapshot_groups
            ),
        ),
        observed_data=CurrentStateEstimationRequest(
            observed_tick_data=filtered_tick_data,
            tick_data_column_mapping=resolved_tick_column_mapping,
            observed_price=spot,
        ),
        cpu_count=cpu_count,
        silent=silent,
        progress_mode=progress_mode,
    )

    return _build_observed_option_snapshot_pricing_result(
        valuation_time_utc=effective_valuation_time_utc,
        spot=spot,
        prepared_snapshot_groups=prepared_snapshot_groups,
        pricing_result=pricing_result,
    )


def _load_tick_data_frame(
    tick_data: TabularFrameInput,
    *,
    tick_column_mapping: CalibrationTickDataColumnMapping,
) -> tuple[pandas.DataFrame, CalibrationTickDataColumnMapping]:
    available_columns = _load_source_columns_for_tabular_input(
        tick_data,
        field_path="tick_data",
    )
    resolved_tick_column_mapping = _resolve_observed_tick_column_mapping(
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
    frame = _load_tabular_frame(
        tick_data,
        field_path="tick_data",
        required_columns=required_columns,
    )

    return frame, resolved_tick_column_mapping


def _load_option_surface_frame(
    option_surface: TabularFrameInput,
    *,
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping],
) -> tuple[pandas.DataFrame, _ResolvedOptionSnapshotColumns]:
    available_columns = _load_source_columns_for_tabular_input(
        option_surface,
        field_path="option_surface",
    )
    resolved_option_columns = _resolve_option_snapshot_columns(
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
    frame = _load_tabular_frame(
        option_surface,
        field_path="option_surface",
        required_columns=required_columns,
    )

    return frame, resolved_option_columns


def _load_source_columns_for_tabular_input(
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


def _load_tabular_frame(
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


def _resolve_option_snapshot_columns(
    available_columns: Sequence[Any],
    *,
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping],
) -> _ResolvedOptionSnapshotColumns:
    available_by_normalized = {
        _normalize_identifier(str(column_name)): str(column_name)
        for column_name in available_columns
    }
    explicit_mapping = (
        CalibrationOptionSurfaceColumnMapping()
        if option_surface_column_mapping is None
        else option_surface_column_mapping
    )

    return _ResolvedOptionSnapshotColumns(
        quote_timestamp_column=_resolve_required_column(
            explicit_mapping.quote_timestamp_column,
            aliases=_OPTION_QUOTE_TIMESTAMP_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.quote_timestamp_column",
        ),
        expiry_timestamp_column=_resolve_required_column(
            explicit_mapping.expiry_timestamp_column,
            aliases=_OPTION_EXPIRY_TIMESTAMP_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.expiry_timestamp_column",
        ),
        strike_column=_resolve_required_column(
            explicit_mapping.strike_column,
            aliases=_OPTION_STRIKE_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.strike_column",
        ),
        option_type_column=_resolve_required_column(
            explicit_mapping.option_type_column,
            aliases=_OPTION_TYPE_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.option_type_column",
        ),
        mid_price_column=_resolve_optional_column(
            explicit_mapping.mid_price_column,
            aliases=_OPTION_MID_PRICE_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.mid_price_column",
        ),
        bid_column=_resolve_optional_column(
            explicit_mapping.bid_column,
            aliases=_OPTION_BID_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.bid_column",
        ),
        ask_column=_resolve_optional_column(
            explicit_mapping.ask_column,
            aliases=_OPTION_ASK_ALIASES,
            available_by_normalized=available_by_normalized,
            field_path="option_surface.ask_column",
        ),
    )


def _resolve_observed_tick_column_mapping(
    available_columns: Sequence[str],
    *,
    tick_column_mapping: CalibrationTickDataColumnMapping,
) -> CalibrationTickDataColumnMapping:
    available_by_normalized = {
        _normalize_identifier(column_name): column_name
        for column_name in available_columns
    }

    return CalibrationTickDataColumnMapping(
        timestamp_column=_resolve_required_column(
            _optional_existing_column_name(
                tick_column_mapping.timestamp_column,
                available_by_normalized=available_by_normalized,
            ),
            aliases=_GENERIC_COLUMN_ALIASES["timestamp"],
            available_by_normalized=available_by_normalized,
            field_path="tick_data.timestamp_column",
        ),
        price_column=_resolve_required_column(
            _optional_existing_column_name(
                tick_column_mapping.price_column,
                available_by_normalized=available_by_normalized,
            ),
            aliases=_GENERIC_COLUMN_ALIASES["price"],
            available_by_normalized=available_by_normalized,
            field_path="tick_data.price_column",
        ),
        trade_sign_column=_resolve_required_column(
            _optional_existing_column_name(
                tick_column_mapping.trade_sign_column,
                available_by_normalized=available_by_normalized,
            ),
            aliases=_GENERIC_COLUMN_ALIASES["trade_sign"],
            available_by_normalized=available_by_normalized,
            field_path="tick_data.trade_sign_column",
        ),
    )


def _resolve_snapshot_valuation_time_utc(
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


def _filter_observed_tick_data(
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


def _observed_spot_from_tick_data(
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
        raise calibration_error(
            "tick_data",
            "must contain at least one observation",
        )

    return require_positive_finite_scalar(
        tick_data_frame[price_column].iloc[-1],
        "tick_data.last_price",
        calibration_error,
    )


def _prepare_snapshot_groups(
    *,
    option_surface_frame: pandas.DataFrame,
    resolved_option_columns: _ResolvedOptionSnapshotColumns,
    valuation_time_utc: datetime,
    option_type: Optional[VanillaOptionPayoffType],
) -> tuple[_PreparedSnapshotGroupInput, ...]:
    quote_timestamps = pandas.to_datetime(
        option_surface_frame[resolved_option_columns.quote_timestamp_column],
        utc=True,
        errors="raise",
    )
    expiry_timestamps = pandas.to_datetime(
        option_surface_frame[resolved_option_columns.expiry_timestamp_column],
        utc=True,
        errors="raise",
    )

    snapshot = option_surface_frame.loc[quote_timestamps == valuation_time_utc].copy()
    if snapshot.empty:
        raise calibration_error(
            "valuation_time_utc",
            f"could not find option quotes for {valuation_time_utc.isoformat()}",
        )

    normalized_option_types = tuple(
        _normalize_observed_option_type(
            raw_value,
            field_path=(
                f"option_surface.{resolved_option_columns.option_type_column}"
                f"[{row_index}]"
            ),
        )
        for row_index, raw_value in enumerate(
            snapshot[resolved_option_columns.option_type_column]
        )
    )
    snapshot["_normalized_option_type"] = normalized_option_types
    snapshot["_expiry_timestamp_utc"] = expiry_timestamps.loc[snapshot.index].dt.to_pydatetime()
    snapshot["_strike"] = pandas.to_numeric(
        snapshot[resolved_option_columns.strike_column],
        errors="raise",
    )

    if option_type is not None:
        snapshot = snapshot.loc[snapshot["_normalized_option_type"] == option_type].copy()
        if snapshot.empty:
            raise calibration_error(
                "option_type",
                f"snapshot does not contain any `{option_type.value}` options",
            )

    snapshot.sort_values(
        by=["_normalized_option_type", "_expiry_timestamp_utc", "_strike"],
        inplace=True,
    )

    grouped_nodes: OrderedDict[
        tuple[VanillaOptionPayoffType, datetime],
        list[_PreparedSnapshotNodeInput],
    ] = OrderedDict()
    for row in snapshot.to_dict("records"):
        normalized_payoff_type = _normalize_requested_option_type(
            row["_normalized_option_type"]
        )
        if normalized_payoff_type is None:
            raise RuntimeError("normalized option type must be present")
        expiry_timestamp_utc = row["_expiry_timestamp_utc"].astimezone(UTC)
        strike = require_positive_finite_scalar(
            row["_strike"],
            "option_surface.strike",
            calibration_error,
        )
        observed_bid_price = _optional_numeric_value(
            row,
            column_name=resolved_option_columns.bid_column,
        )
        observed_ask_price = _optional_numeric_value(
            row,
            column_name=resolved_option_columns.ask_column,
        )
        observed_mid_price = _optional_numeric_value(
            row,
            column_name=resolved_option_columns.mid_price_column,
        )
        if (
            observed_mid_price is None
            and observed_bid_price is not None
            and observed_ask_price is not None
        ):
            observed_mid_price = 0.5 * (observed_bid_price + observed_ask_price)

        grouped_nodes.setdefault(
            (normalized_payoff_type, expiry_timestamp_utc),
            [],
        ).append(
            _PreparedSnapshotNodeInput(
                quote_timestamp_utc=valuation_time_utc,
                expiry_timestamp_utc=expiry_timestamp_utc,
                strike=strike,
                option_type=normalized_payoff_type,
                observed_bid_price=observed_bid_price,
                observed_ask_price=observed_ask_price,
                observed_mid_price=observed_mid_price,
            )
        )

    prepared_groups: list[_PreparedSnapshotGroupInput] = []
    for (payoff_type, expiry_timestamp_utc), nodes in grouped_nodes.items():
        unique_strikes = {node.strike for node in nodes}
        if len(unique_strikes) != len(nodes):
            raise calibration_error(
                "option_surface",
                (
                    "snapshot contains duplicate nodes for the same "
                    "option_type, expiry_timestamp_utc, and strike"
                ),
            )

        prepared_groups.append(
            _PreparedSnapshotGroupInput(
                payoff_type=payoff_type,
                expiry_timestamp_utc=expiry_timestamp_utc,
                strikes=tuple(node.strike for node in nodes),
                nodes=tuple(nodes),
            )
        )

    if len(prepared_groups) == 0:
        raise calibration_error(
            "option_surface",
            "snapshot did not retain any option nodes",
        )

    return tuple(prepared_groups)


def _build_observed_option_snapshot_pricing_result(
    *,
    valuation_time_utc: datetime,
    spot: float,
    prepared_snapshot_groups: tuple[_PreparedSnapshotGroupInput, ...],
    pricing_result: JaniformVanillaOptionGroupedBatchPricingResult,
) -> JaniformObservedOptionSnapshotPricingResult:
    nodes: list[JaniformObservedOptionSnapshotNode] = []

    for prepared_group, priced_group in zip(
        prepared_snapshot_groups,
        pricing_result.groups,
        strict=True,
    ):
        if len(priced_group.maturity_intervals) != 1:
            raise RuntimeError(
                "price_observed_option_snapshot expected exactly one maturity per pricing group"
            )

        maturity_interval = priced_group.maturity_intervals[0]
        for node_index, prepared_node in enumerate(prepared_group.nodes):
            nodes.append(
                JaniformObservedOptionSnapshotNode(
                    quote_timestamp_utc=prepared_node.quote_timestamp_utc,
                    expiry_timestamp_utc=prepared_node.expiry_timestamp_utc,
                    strike=prepared_node.strike,
                    option_type=prepared_node.option_type,
                    risk_free_rate=priced_group.risk_free_rate,
                    dividend_yield=priced_group.dividend_yield,
                    maturity_interval=maturity_interval,
                    observed_bid_price=prepared_node.observed_bid_price,
                    observed_ask_price=prepared_node.observed_ask_price,
                    observed_mid_price=prepared_node.observed_mid_price,
                    model_price=priced_group.price_grid[0][node_index],
                    model_implied_volatility=priced_group.implied_volatility_grid[0][node_index],
                    implied_volatility_inversion_status=priced_group.implied_volatility_inversion_status_grid[0][node_index],
                    pricing_status=priced_group.pricing_status_grid[0][node_index],
                    pricing_failure_stage=priced_group.pricing_failure_stage_grid[0][node_index],
                    pricing_failure_reason=priced_group.pricing_failure_reason_grid[0][node_index],
                    forward_price=priced_group.forward_price_grid[0][node_index],
                    discount_factor=priced_group.discount_factor_grid[0][node_index],
                    boundary_state_norm=priced_group.boundary_state_norm_grid[0][node_index],
                )
            )

    return JaniformObservedOptionSnapshotPricingResult(
        valuation_time_utc=valuation_time_utc,
        spot=spot,
        nodes=tuple(nodes),
        raw_result=pricing_result,
    )


def _resolve_required_column(
    explicit_column: Optional[str],
    *,
    aliases: Sequence[str],
    available_by_normalized: Mapping[str, str],
    field_path: str,
) -> str:
    resolved = _resolve_optional_column(
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


def _optional_existing_column_name(
    explicit_column: Optional[str],
    *,
    available_by_normalized: Mapping[str, str],
) -> Optional[str]:
    if explicit_column is None:
        return None

    return available_by_normalized.get(_normalize_identifier(explicit_column))


def _resolve_optional_column(
    explicit_column: Optional[str],
    *,
    aliases: Sequence[str],
    available_by_normalized: Mapping[str, str],
    field_path: str,
) -> Optional[str]:
    if explicit_column is not None:
        resolved_explicit = available_by_normalized.get(_normalize_identifier(explicit_column))
        if resolved_explicit is None:
            raise calibration_error(
                field_path,
                f"explicit column `{explicit_column}` is not present in the source data",
            )

        return resolved_explicit

    for alias in aliases:
        resolved = available_by_normalized.get(_normalize_identifier(alias))
        if resolved is not None:
            return resolved

    return None


def _normalize_identifier(identifier: str) -> str:
    return "".join(
        character.lower()
        for character in identifier
        if character.isascii() and character.isalnum()
    )


def _normalize_requested_option_type(
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

    raise calibration_error(
        "option_type",
        "must be one of: call, put",
    )


def _normalize_observed_option_type(
    raw_value: Any,
    *,
    field_path: str,
) -> VanillaOptionPayoffType:
    if not isinstance(raw_value, str):
        raise calibration_error(field_path, "must be a string")

    return _normalize_requested_option_type(raw_value)


def _optional_numeric_value(
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


__all__ = [
    "JaniformObservedOptionSnapshotNode",
    "JaniformObservedOptionSnapshotPricingResult",
    "TabularFrameInput",
    "price_observed_option_snapshot_for_model",
]
