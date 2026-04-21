"""Internal market-snapshot result assembly helpers."""

from __future__ import annotations

from collections import OrderedDict
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Optional

import pandas

from ......contracts.base import require_datetime_utc, require_positive_finite_scalar
from ......pricing import VanillaOptionPayoffType
from .....errors import calibration_error
from ...grids import JaniformVanillaOptionGridBatchPricingResult
from ..result import (
    JaniformObservedOptionSnapshotNode,
    JaniformObservedOptionSnapshotPricingResult,
)
from .loading import (
    ResolvedOptionSnapshotColumns,
    normalize_observed_option_type,
    normalize_requested_option_type,
    optional_numeric_value,
)


@dataclass(frozen=True, slots=True)
class PreparedSnapshotNodeInput:
    quote_timestamp_utc: datetime
    expiry_timestamp_utc: datetime
    strike: float
    option_type: VanillaOptionPayoffType
    observed_bid_price: Optional[float]
    observed_ask_price: Optional[float]
    observed_mid_price: Optional[float]


@dataclass(frozen=True, slots=True)
class PreparedSnapshotGroupInput:
    payoff_type: VanillaOptionPayoffType
    expiry_timestamp_utc: datetime
    strikes: tuple[float, ...]
    nodes: tuple[PreparedSnapshotNodeInput, ...]


def prepare_as_of_snapshot_frame(
    *,
    option_surface_frame: pandas.DataFrame,
    resolved_option_columns: ResolvedOptionSnapshotColumns,
    valuation_time_utc: datetime,
    option_type: Optional[VanillaOptionPayoffType],
) -> pandas.DataFrame:
    quote_timestamps, expiry_timestamps, normalized_option_types, normalized_strikes = (
        normalize_snapshot_helper_columns(
            option_surface_frame=option_surface_frame,
            resolved_option_columns=resolved_option_columns,
        )
    )

    snapshot = option_surface_frame.loc[quote_timestamps <= valuation_time_utc].copy()
    if snapshot.empty:
        raise calibration_error(
            "valuation_time_utc",
            f"could not find option quotes at or before {valuation_time_utc.isoformat()}",
        )

    snapshot["_normalized_option_type"] = tuple(
        normalized_option_types[snapshot_index]
        for snapshot_index in snapshot.index
    )
    snapshot["_quote_timestamp_utc"] = quote_timestamps.loc[snapshot.index].dt.to_pydatetime()
    snapshot["_expiry_timestamp_utc"] = expiry_timestamps.loc[snapshot.index].dt.to_pydatetime()
    snapshot["_strike"] = normalized_strikes.loc[snapshot.index]

    if option_type is not None:
        snapshot = snapshot.loc[snapshot["_normalized_option_type"] == option_type].copy()
        if snapshot.empty:
            raise calibration_error(
                "option_type",
                f"snapshot does not contain any `{option_type.value}` options",
            )

    snapshot.sort_values(
        by=["_normalized_option_type", "_expiry_timestamp_utc", "_strike", "_quote_timestamp_utc"],
        inplace=True,
        kind="stable",
    )

    snapshot.drop_duplicates(
        subset=["_normalized_option_type", "_expiry_timestamp_utc", "_strike"],
        keep="last",
        inplace=True,
    )

    snapshot.sort_values(
        by=["_normalized_option_type", "_expiry_timestamp_utc", "_strike"],
        inplace=True,
        kind="stable",
    )

    if snapshot.empty:
        raise calibration_error("option_surface", "snapshot did not retain any option nodes")

    return snapshot


def prepare_snapshot_groups(
    *,
    option_surface_frame: pandas.DataFrame,
    resolved_option_columns: ResolvedOptionSnapshotColumns,
    valuation_time_utc: datetime,
    option_type: Optional[VanillaOptionPayoffType],
) -> tuple[PreparedSnapshotGroupInput, ...]:
    snapshot = prepare_as_of_snapshot_frame(
        option_surface_frame=option_surface_frame,
        resolved_option_columns=resolved_option_columns,
        valuation_time_utc=valuation_time_utc,
        option_type=option_type,
    )

    grouped_nodes: OrderedDict[tuple[VanillaOptionPayoffType, datetime], list[PreparedSnapshotNodeInput]] = OrderedDict()
    for row in snapshot.to_dict("records"):
        normalized_payoff_type = normalize_requested_option_type(row["_normalized_option_type"])

        if normalized_payoff_type is None:
            raise RuntimeError("normalized option type must be present")

        expiry_timestamp_utc = resolved_snapshot_timestamp_utc(
            row["_expiry_timestamp_utc"],
            field_path="option_surface._expiry_timestamp_utc",
        )

        strike = require_positive_finite_scalar(row["_strike"], "option_surface.strike", calibration_error)

        observed_bid_price = optional_numeric_value(row, column_name=resolved_option_columns.bid_column)
        observed_ask_price = optional_numeric_value(row, column_name=resolved_option_columns.ask_column)
        observed_mid_price = optional_numeric_value(row, column_name=resolved_option_columns.mid_price_column)
        if observed_mid_price is None and observed_bid_price is not None and observed_ask_price is not None:
            observed_mid_price = 0.5 * (observed_bid_price + observed_ask_price)

        grouped_nodes.setdefault((normalized_payoff_type, expiry_timestamp_utc), []).append(
            PreparedSnapshotNodeInput(
                quote_timestamp_utc=resolved_snapshot_timestamp_utc(
                    row["_quote_timestamp_utc"],
                    field_path="option_surface._quote_timestamp_utc",
                ),
                expiry_timestamp_utc=expiry_timestamp_utc,
                strike=strike,
                option_type=normalized_payoff_type,
                observed_bid_price=observed_bid_price,
                observed_ask_price=observed_ask_price,
                observed_mid_price=observed_mid_price,
            )
        )

    prepared_groups: list[PreparedSnapshotGroupInput] = []
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
            PreparedSnapshotGroupInput(
                payoff_type=payoff_type,
                expiry_timestamp_utc=expiry_timestamp_utc,
                strikes=tuple(node.strike for node in nodes),
                nodes=tuple(nodes),
            )
    )

    return tuple(prepared_groups)


def prepare_selected_expiry_history_frame(
    *,
    option_surface_frame: pandas.DataFrame,
    resolved_option_columns: ResolvedOptionSnapshotColumns,
    valuation_time_utc: datetime,
    option_type: Optional[VanillaOptionPayoffType],
    selected_expiry_timestamp_utc: datetime,
) -> pandas.DataFrame:
    quote_timestamps, expiry_timestamps, normalized_option_types, normalized_strikes = (
        normalize_snapshot_helper_columns(
            option_surface_frame=option_surface_frame,
            resolved_option_columns=resolved_option_columns,
        )
    )

    history = option_surface_frame.loc[
        (quote_timestamps <= valuation_time_utc)
        & (expiry_timestamps == selected_expiry_timestamp_utc)
    ].copy()

    if history.empty:
        raise calibration_error(
            "valuation_time_utc",
            (
                "could not find option quotes for the selected expiry at or before "
                f"{valuation_time_utc.isoformat()}"
            ),
        )

    history["_normalized_option_type"] = tuple(
        normalized_option_types[history_index]
        for history_index in history.index
    )

    history["_quote_timestamp_utc"] = quote_timestamps.loc[history.index].dt.to_pydatetime()
    history["_expiry_timestamp_utc"] = expiry_timestamps.loc[history.index].dt.to_pydatetime()
    history["_strike"] = normalized_strikes.loc[history.index]

    if option_type is not None:
        history = history.loc[history["_normalized_option_type"] == option_type].copy()
        if history.empty:
            raise calibration_error(
                "option_type",
                (
                    "surface history does not contain any "
                    f"`{option_type.value}` options for the selected expiry"
                ),
            )

    history.sort_values(
        by=["_quote_timestamp_utc", "_strike"],
        inplace=True,
        kind="stable",
    )

    return history


def prepare_selected_expiry_snapshot_groups_by_valuation_time(
    *,
    selected_expiry_history: pandas.DataFrame,
    valuation_times_utc: tuple[datetime, ...],
    resolved_option_columns: ResolvedOptionSnapshotColumns,
) -> dict[datetime, tuple[PreparedSnapshotGroupInput, ...]]:
    if len(valuation_times_utc) == 0:
        return {}

    sorted_history = selected_expiry_history.sort_values(
        by=["_quote_timestamp_utc", "_strike"],
        kind="stable",
    )

    history_rows = tuple(sorted_history.to_dict("records"))
    history_row_index = 0
    latest_rows_by_strike: dict[float, dict[str, object]] = {}
    prepared_groups_by_time: dict[datetime, tuple[PreparedSnapshotGroupInput, ...]] = {}

    for valuation_time_utc in valuation_times_utc:
        while history_row_index < len(history_rows):
            current_row = history_rows[history_row_index]
            current_quote_timestamp_utc = resolved_snapshot_timestamp_utc(
                current_row["_quote_timestamp_utc"],
                field_path="option_surface._quote_timestamp_utc",
            )
            if current_quote_timestamp_utc > valuation_time_utc:
                break

            current_strike = require_positive_finite_scalar(
                current_row["_strike"],
                "option_surface.strike",
                calibration_error,
            )
            latest_rows_by_strike[current_strike] = current_row

            history_row_index += 1

        if len(latest_rows_by_strike) == 0:
            raise calibration_error(
                "valuation_time_utc",
                (
                    "could not find option quotes for the selected expiry at or before "
                    f"{valuation_time_utc.isoformat()}"
                ),
            )

        prepared_groups_by_time[valuation_time_utc] = (
            build_prepared_snapshot_group_from_rows(
                rows=tuple(
                    latest_rows_by_strike[strike]
                    for strike in sorted(latest_rows_by_strike)
                ),
                resolved_option_columns=resolved_option_columns,
            ),
        )

    return prepared_groups_by_time


def normalize_snapshot_helper_columns(
    *,
    option_surface_frame: pandas.DataFrame,
    resolved_option_columns: ResolvedOptionSnapshotColumns,
) -> tuple[pandas.Series, pandas.Series, pandas.Series, pandas.Series]:
    quote_source = (
        option_surface_frame["_quote_timestamp_utc"]
        if "_quote_timestamp_utc" in option_surface_frame.columns
        else option_surface_frame[resolved_option_columns.quote_timestamp_column]
    )

    expiry_source = (
        option_surface_frame["_expiry_timestamp_utc"]
        if "_expiry_timestamp_utc" in option_surface_frame.columns
        else option_surface_frame[resolved_option_columns.expiry_timestamp_column]
    )

    strike_source = (
        option_surface_frame["_strike"]
        if "_strike" in option_surface_frame.columns
        else option_surface_frame[resolved_option_columns.strike_column]
    )

    option_type_source = (
        option_surface_frame["_normalized_option_type"]
        if "_normalized_option_type" in option_surface_frame.columns
        else option_surface_frame[resolved_option_columns.option_type_column]
    )

    quote_timestamps = pandas.to_datetime(
        quote_source,
        utc=True,
        errors="raise",
    )

    expiry_timestamps = pandas.to_datetime(
        expiry_source,
        utc=True,
        errors="raise",
    )

    normalized_option_types = pandas.Series(
        tuple(
            normalize_observed_option_type(
                raw_value,
                field_path=f"option_surface.{resolved_option_columns.option_type_column}[{row_index}]",
            )
            for row_index, raw_value in enumerate(option_type_source)
        ),
        index=option_surface_frame.index,
        dtype="object",
    )

    normalized_strikes = pandas.to_numeric(strike_source, errors="raise")

    return (
        quote_timestamps,
        expiry_timestamps,
        normalized_option_types,
        normalized_strikes,
    )


def resolved_snapshot_timestamp_utc(
    raw_value: object,
    *,
    field_path: str,
) -> datetime:
    return require_datetime_utc(
        raw_value,
        field_path=field_path,
        error_factory=calibration_error,
    ).astimezone(UTC)


def build_prepared_snapshot_group_from_rows(
    *,
    rows: tuple[dict[str, object], ...],
    resolved_option_columns: ResolvedOptionSnapshotColumns,
) -> PreparedSnapshotGroupInput:
    if len(rows) == 0:
        raise calibration_error(
            "option_surface",
            "cannot build a snapshot group from an empty row set",
        )

    first_row = rows[0]
    normalized_payoff_type = normalize_requested_option_type(first_row["_normalized_option_type"])
    if normalized_payoff_type is None:
        raise RuntimeError("normalized option type must be present")

    expiry_timestamp_utc = resolved_snapshot_timestamp_utc(
        first_row["_expiry_timestamp_utc"],
        field_path="option_surface._expiry_timestamp_utc",
    )

    prepared_nodes: list[PreparedSnapshotNodeInput] = []
    for row in rows:
        current_expiry_timestamp_utc = resolved_snapshot_timestamp_utc(
            row["_expiry_timestamp_utc"],
            field_path="option_surface._expiry_timestamp_utc",
        )
        if current_expiry_timestamp_utc != expiry_timestamp_utc:
            raise calibration_error(
                "option_surface",
                "selected expiry snapshot rows must all share the same expiry",
            )

        current_payoff_type = normalize_requested_option_type(row["_normalized_option_type"])
        if current_payoff_type is None or current_payoff_type != normalized_payoff_type:
            raise calibration_error(
                "option_surface",
                "selected expiry snapshot rows must all share the same option type",
            )

        strike = require_positive_finite_scalar(row["_strike"], "option_surface.strike", calibration_error)

        observed_bid_price = optional_numeric_value(row, column_name=resolved_option_columns.bid_column)
        observed_ask_price = optional_numeric_value(row, column_name=resolved_option_columns.ask_column)
        observed_mid_price = optional_numeric_value(row, column_name=resolved_option_columns.mid_price_column)

        if observed_mid_price is None and observed_bid_price is not None and observed_ask_price is not None:
            observed_mid_price = 0.5 * (observed_bid_price + observed_ask_price)

        prepared_nodes.append(
            PreparedSnapshotNodeInput(
                quote_timestamp_utc=resolved_snapshot_timestamp_utc(
                    row["_quote_timestamp_utc"],
                    field_path="option_surface._quote_timestamp_utc",
                ),
                expiry_timestamp_utc=expiry_timestamp_utc,
                strike=strike,
                option_type=normalized_payoff_type,
                observed_bid_price=observed_bid_price,
                observed_ask_price=observed_ask_price,
                observed_mid_price=observed_mid_price,
            )
        )

    return PreparedSnapshotGroupInput(
        payoff_type=normalized_payoff_type,
        expiry_timestamp_utc=expiry_timestamp_utc,
        strikes=tuple(node.strike for node in prepared_nodes),
        nodes=tuple(prepared_nodes),
    )


def build_observed_option_snapshot_pricing_result(
    *,
    valuation_time_utc: datetime,
    spot: float,
    prepared_snapshot_groups: tuple[PreparedSnapshotGroupInput, ...],
    pricing_result: JaniformVanillaOptionGridBatchPricingResult,
) -> JaniformObservedOptionSnapshotPricingResult:
    nodes: list[JaniformObservedOptionSnapshotNode] = []

    for prepared_group, priced_group in zip(prepared_snapshot_groups, pricing_result.grids, strict=True):
        if len(priced_group.maturity_intervals) != 1:
            raise RuntimeError(
                "price_option_snapshot expected exactly one maturity per pricing group"
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
