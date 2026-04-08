from __future__ import annotations

from collections import OrderedDict
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Optional

import pandas

from ....contracts import require_positive_finite_scalar
from ....pricing import VanillaOptionPayoffType
from ...errors import calibration_error
from .grouped import JaniformVanillaOptionGroupedBatchPricingResult
from .observed_snapshot_contracts import (
    JaniformObservedOptionSnapshotNode,
    JaniformObservedOptionSnapshotPricingResult,
)
from .observed_snapshot_loading import (
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


def prepare_snapshot_groups(
    *,
    option_surface_frame: pandas.DataFrame,
    resolved_option_columns: ResolvedOptionSnapshotColumns,
    valuation_time_utc: datetime,
    option_type: Optional[VanillaOptionPayoffType],
) -> tuple[PreparedSnapshotGroupInput, ...]:
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
        normalize_observed_option_type(
            raw_value,
            field_path=f"option_surface.{resolved_option_columns.option_type_column}[{row_index}]",
        )
        for row_index, raw_value in enumerate(snapshot[resolved_option_columns.option_type_column])
    )
    snapshot["_normalized_option_type"] = normalized_option_types
    snapshot["_expiry_timestamp_utc"] = expiry_timestamps.loc[snapshot.index].dt.to_pydatetime()
    snapshot["_strike"] = pandas.to_numeric(snapshot[resolved_option_columns.strike_column], errors="raise")

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

    grouped_nodes: OrderedDict[tuple[VanillaOptionPayoffType, datetime], list[PreparedSnapshotNodeInput]] = OrderedDict()
    for row in snapshot.to_dict("records"):
        normalized_payoff_type = normalize_requested_option_type(row["_normalized_option_type"])
        if normalized_payoff_type is None:
            raise RuntimeError("normalized option type must be present")
        expiry_timestamp_utc = row["_expiry_timestamp_utc"].astimezone(UTC)
        strike = require_positive_finite_scalar(row["_strike"], "option_surface.strike", calibration_error)
        observed_bid_price = optional_numeric_value(row, column_name=resolved_option_columns.bid_column)
        observed_ask_price = optional_numeric_value(row, column_name=resolved_option_columns.ask_column)
        observed_mid_price = optional_numeric_value(row, column_name=resolved_option_columns.mid_price_column)
        if observed_mid_price is None and observed_bid_price is not None and observed_ask_price is not None:
            observed_mid_price = 0.5 * (observed_bid_price + observed_ask_price)

        grouped_nodes.setdefault((normalized_payoff_type, expiry_timestamp_utc), []).append(
            PreparedSnapshotNodeInput(
                quote_timestamp_utc=valuation_time_utc,
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

    if len(prepared_groups) == 0:
        raise calibration_error("option_surface", "snapshot did not retain any option nodes")

    return tuple(prepared_groups)


def build_observed_option_snapshot_pricing_result(
    *,
    valuation_time_utc: datetime,
    spot: float,
    prepared_snapshot_groups: tuple[PreparedSnapshotGroupInput, ...],
    pricing_result: JaniformVanillaOptionGroupedBatchPricingResult,
) -> JaniformObservedOptionSnapshotPricingResult:
    nodes: list[JaniformObservedOptionSnapshotNode] = []

    for prepared_group, priced_group in zip(prepared_snapshot_groups, pricing_result.groups, strict=True):
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
