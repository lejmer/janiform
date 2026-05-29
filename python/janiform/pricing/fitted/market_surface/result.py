"""Public market-surface pricing result types."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from typing import TYPE_CHECKING, Any, Optional

import numpy
import pandas

from janiform._pricing_field_names import (
    BOUNDARY_STATE_NORM_FIELD_NAME,
    DISCOUNT_FACTOR_FIELD_NAME,
    DIVIDEND_YIELD_FIELD_NAME,
    EXPIRY_TIMESTAMP_UTC_FIELD_NAME,
    FORWARD_PRICE_FIELD_NAME,
    IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME,
    LOG_FORWARD_MONEYNESS_FIELD_NAME,
    MATURITY_INTERVAL_FIELD_NAME,
    MODEL_IMPLIED_VOLATILITY_FIELD_NAME,
    MODEL_PRICE_FIELD_NAME,
    OBSERVED_ASK_PRICE_FIELD_NAME,
    OBSERVED_BID_PRICE_FIELD_NAME,
    OBSERVED_MID_PRICE_FIELD_NAME,
    OPTION_TYPE_FIELD_NAME,
    PRICING_FAILURE_REASON_FIELD_NAME,
    PRICING_FAILURE_STAGE_FIELD_NAME,
    PRICING_STATUS_FIELD_NAME,
    QUOTE_TIMESTAMP_UTC_FIELD_NAME,
    RISK_FREE_RATE_FIELD_NAME,
    STRIKE_FIELD_NAME,
    TIME_TO_EXPIRY_DAYS_FIELD_NAME,
    TIME_TO_EXPIRY_HOURS_FIELD_NAME,
    VALUATION_TIME_UTC_FIELD_NAME,
)
from janiform.pricing import (
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffType,
    VanillaOptionPricingFailureStage,
    VanillaOptionPricingStatus,
)
from janiform.pricing._moneyness import calculate_optional_log_forward_moneyness
from janiform.pricing.errors import _pricing_error
from ..internal.common import native_datetime
from ..internal.price_coordinates import resolve_price_coordinate_from_native_mapping
from ..market_snapshot.result import JaniformObservedOptionSnapshotPricingResult
from ..physical_distribution import ObservedOptionsPhysicalDistributionDiagnosticsOutput

if TYPE_CHECKING:
    from ....calibration.market_data.price_coordinates import ResolvedPriceCoordinateSet


@dataclass(frozen=True, slots=True)
class JaniformObservedOptionSurfaceNode:
    valuation_time_utc: datetime
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

    @property
    def log_forward_moneyness(self) -> Optional[float]:
        return calculate_optional_log_forward_moneyness(
            strike=self.strike,
            forward_price=self.forward_price,
        )

    @property
    def time_to_expiry_hours(self) -> float:
        return (self.expiry_timestamp_utc - self.valuation_time_utc).total_seconds() / 3600.0

    @property
    def time_to_expiry_days(self) -> float:
        return (self.expiry_timestamp_utc - self.valuation_time_utc).total_seconds() / 86_400.0


@dataclass(frozen=True, slots=True)
class JaniformObservedOptionSurfacePricingResult:
    valuation_time_utc: datetime
    nodes: tuple[JaniformObservedOptionSurfaceNode, ...]
    physical_distribution: Optional[ObservedOptionsPhysicalDistributionDiagnosticsOutput] = None
    price_coordinate_resolution: Optional[ResolvedPriceCoordinateSet] = None
    warnings: tuple[str, ...] = ()
    raw_results: tuple[JaniformObservedOptionSnapshotPricingResult, ...] = field(
        default=(),
        repr=False,
    )

    @staticmethod
    def from_native_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "observed_options_pricing_result",
    ) -> JaniformObservedOptionSurfacePricingResult:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        raw_snapshot_results = raw_payload.get("snapshot_results")
        if not isinstance(raw_snapshot_results, list):
            raise _pricing_error(f"{field_path}.snapshot_results", "must be an array")

        snapshot_results = tuple(
            JaniformObservedOptionSnapshotPricingResult.from_native_snapshot_mapping(
                raw_snapshot_result,
                field_path=f"{field_path}.snapshot_results[{snapshot_index}]",
            )
            for snapshot_index, raw_snapshot_result in enumerate(raw_snapshot_results)
        )

        return JaniformObservedOptionSurfacePricingResult(
            valuation_time_utc=native_datetime(
                raw_payload.get("valuation_time_utc"),
                field_path=f"{field_path}.valuation_time_utc",
            ),
            nodes=tuple(
                JaniformObservedOptionSurfaceNode(
                    valuation_time_utc=snapshot_result.valuation_time_utc,
                    quote_timestamp_utc=snapshot_node.quote_timestamp_utc,
                    expiry_timestamp_utc=snapshot_node.expiry_timestamp_utc,
                    strike=snapshot_node.strike,
                    option_type=snapshot_node.option_type,
                    risk_free_rate=snapshot_node.risk_free_rate,
                    dividend_yield=snapshot_node.dividend_yield,
                    maturity_interval=snapshot_node.maturity_interval,
                    observed_bid_price=snapshot_node.observed_bid_price,
                    observed_ask_price=snapshot_node.observed_ask_price,
                    observed_mid_price=snapshot_node.observed_mid_price,
                    model_price=snapshot_node.model_price,
                    model_implied_volatility=snapshot_node.model_implied_volatility,
                    implied_volatility_inversion_status=snapshot_node.implied_volatility_inversion_status,
                    pricing_status=snapshot_node.pricing_status,
                    pricing_failure_stage=snapshot_node.pricing_failure_stage,
                    pricing_failure_reason=snapshot_node.pricing_failure_reason,
                    forward_price=snapshot_node.forward_price,
                    discount_factor=snapshot_node.discount_factor,
                    boundary_state_norm=snapshot_node.boundary_state_norm,
                )
                for snapshot_result in snapshot_results
                for snapshot_node in snapshot_result.nodes
            ),
            physical_distribution=(
                ObservedOptionsPhysicalDistributionDiagnosticsOutput.from_native_mapping(
                    raw_payload.get("physical_distribution"),
                    field_path=f"{field_path}.physical_distribution",
                )
            ),
            price_coordinate_resolution=(
                resolve_price_coordinate_from_native_mapping(
                    raw_payload["price_coordinate_resolution"],
                    field_path=f"{field_path}.price_coordinate_resolution",
                )
                if raw_payload.get("price_coordinate_resolution") is not None
                else None
            ),
            warnings=tuple(str(warning) for warning in raw_payload.get("warnings", [])),
            raw_results=snapshot_results,
        )

    @property
    def valuation_times_utc(self) -> tuple[datetime, ...]:
        return tuple(
            dict.fromkeys(node.valuation_time_utc for node in self.nodes)
        )

    def to_pandas(self) -> Any:
        rows = [
            {
                VALUATION_TIME_UTC_FIELD_NAME: node.valuation_time_utc,
                QUOTE_TIMESTAMP_UTC_FIELD_NAME: node.quote_timestamp_utc,
                EXPIRY_TIMESTAMP_UTC_FIELD_NAME: node.expiry_timestamp_utc,
                TIME_TO_EXPIRY_HOURS_FIELD_NAME: node.time_to_expiry_hours,
                TIME_TO_EXPIRY_DAYS_FIELD_NAME: node.time_to_expiry_days,
                STRIKE_FIELD_NAME: node.strike,
                OPTION_TYPE_FIELD_NAME: node.option_type.value,
                RISK_FREE_RATE_FIELD_NAME: node.risk_free_rate,
                DIVIDEND_YIELD_FIELD_NAME: node.dividend_yield,
                MATURITY_INTERVAL_FIELD_NAME: node.maturity_interval,
                OBSERVED_BID_PRICE_FIELD_NAME: node.observed_bid_price,
                OBSERVED_ASK_PRICE_FIELD_NAME: node.observed_ask_price,
                OBSERVED_MID_PRICE_FIELD_NAME: node.observed_mid_price,
                MODEL_PRICE_FIELD_NAME: node.model_price,
                MODEL_IMPLIED_VOLATILITY_FIELD_NAME: node.model_implied_volatility,
                IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME: (
                    None
                    if node.implied_volatility_inversion_status is None
                    else node.implied_volatility_inversion_status.value
                ),
                PRICING_STATUS_FIELD_NAME: node.pricing_status.value,
                PRICING_FAILURE_STAGE_FIELD_NAME: (
                    None
                    if node.pricing_failure_stage is None
                    else node.pricing_failure_stage.value
                ),
                PRICING_FAILURE_REASON_FIELD_NAME: node.pricing_failure_reason,
                FORWARD_PRICE_FIELD_NAME: node.forward_price,
                LOG_FORWARD_MONEYNESS_FIELD_NAME: node.log_forward_moneyness,
                DISCOUNT_FACTOR_FIELD_NAME: node.discount_factor,
                BOUNDARY_STATE_NORM_FIELD_NAME: node.boundary_state_norm,
            }
            for node in self.nodes
        ]

        return pandas.DataFrame.from_records(rows)

    def to_surface_df(
        self,
        *,
        value_column: str = MODEL_IMPLIED_VOLATILITY_FIELD_NAME,
    ) -> Any:
        frame = self.to_pandas()
        if value_column not in frame.columns:
            raise _pricing_error(
                MODEL_IMPLIED_VOLATILITY_FIELD_NAME,
                f"must be one of: {', '.join(str(value) for value in frame.columns)}",
            )

        valuation_count = frame[VALUATION_TIME_UTC_FIELD_NAME].nunique(dropna=False)
        expiry_count = frame[EXPIRY_TIMESTAMP_UTC_FIELD_NAME].nunique(dropna=False)
        if expiry_count == 1 and valuation_count > 1:
            return _build_single_expiry_surface_df(
                frame=frame,
                value_column=value_column,
                valuation_time_utc=self.valuation_time_utc,
            )

        index_columns = (
            [EXPIRY_TIMESTAMP_UTC_FIELD_NAME]
            if frame[OPTION_TYPE_FIELD_NAME].nunique(dropna=False) == 1
            else [OPTION_TYPE_FIELD_NAME, EXPIRY_TIMESTAMP_UTC_FIELD_NAME]
        )

        return (
            frame
            .pivot(index=index_columns, columns=STRIKE_FIELD_NAME, values=value_column)
            .sort_index(axis=0)
            .sort_index(axis=1)
        )


def _build_single_expiry_surface_df(
    *,
    frame: pandas.DataFrame,
    value_column: str,
    valuation_time_utc: datetime,
) -> Any:
    reference_slice = frame.loc[
        frame[VALUATION_TIME_UTC_FIELD_NAME] == pandas.Timestamp(valuation_time_utc)
    ].copy()

    reference_moneyness_grid = tuple(
        sorted(
            float(value)
            for value in reference_slice[LOG_FORWARD_MONEYNESS_FIELD_NAME].dropna().drop_duplicates()
        )
    )
    if len(reference_moneyness_grid) == 0:
        raise _pricing_error(
            LOG_FORWARD_MONEYNESS_FIELD_NAME,
            "surface did not retain any finite log-forward moneyness values",
        )

    shared_moneyness_grid = _shared_moneyness_support(
        frame=frame,
        reference_moneyness_grid=reference_moneyness_grid,
    )

    interpolated_rows: list[list[float]] = []
    time_to_expiry_days: list[float] = []

    for _, priced_slice in frame.groupby(VALUATION_TIME_UTC_FIELD_NAME, sort=True):
        interpolated_rows.append(
            _interpolated_slice(
                reference_moneyness_grid=shared_moneyness_grid,
                priced_slice=priced_slice,
                value_column=value_column,
            )
        )
        time_to_expiry_days.append(float(priced_slice[TIME_TO_EXPIRY_DAYS_FIELD_NAME].iloc[0]))

    return (
        pandas.DataFrame(
            interpolated_rows,
            index=time_to_expiry_days,
            columns=shared_moneyness_grid,
        )
        .sort_index(axis=0)
        .sort_index(axis=1)
    )


def _interpolated_slice(
    *,
    reference_moneyness_grid: tuple[float, ...],
    priced_slice: pandas.DataFrame,
    value_column: str,
) -> list[float]:
    finite_slice = priced_slice.loc[
        priced_slice[LOG_FORWARD_MONEYNESS_FIELD_NAME].notna()
        & priced_slice[value_column].notna()
    ].sort_values(by=LOG_FORWARD_MONEYNESS_FIELD_NAME, kind="stable")

    if len(finite_slice.index) == 0:
        return [float("nan")] * len(reference_moneyness_grid)

    deduplicated_slice = finite_slice.drop_duplicates(
        subset=[LOG_FORWARD_MONEYNESS_FIELD_NAME],
        keep="last",
    )

    observed_moneyness = deduplicated_slice[LOG_FORWARD_MONEYNESS_FIELD_NAME].to_numpy(dtype=float)
    observed_values = deduplicated_slice[value_column].to_numpy(dtype=float)

    if len(observed_moneyness) == 1:
        observed_value = float(observed_values[0])
        observed_moneyness_value = float(observed_moneyness[0])

        return [
            observed_value if grid_value == observed_moneyness_value else float("nan")
            for grid_value in reference_moneyness_grid
        ]

    interpolated = numpy.interp(
        numpy.asarray(reference_moneyness_grid, dtype=float),
        observed_moneyness,
        observed_values,
        left=float("nan"),
        right=float("nan"),
    )

    observed_minimum = float(observed_moneyness[0])
    observed_maximum = float(observed_moneyness[-1])

    for grid_index, grid_value in enumerate(reference_moneyness_grid):
        if grid_value < observed_minimum or grid_value > observed_maximum:
            interpolated[grid_index] = float("nan")

    return interpolated.tolist()


def _shared_moneyness_support(
    *,
    frame: pandas.DataFrame,
    reference_moneyness_grid: tuple[float, ...],
) -> tuple[float, ...]:
    support_by_valuation_time = [
        (
            float(priced_slice[LOG_FORWARD_MONEYNESS_FIELD_NAME].min()),
            float(priced_slice[LOG_FORWARD_MONEYNESS_FIELD_NAME].max()),
        )
        for _, priced_slice in frame.loc[
            frame[LOG_FORWARD_MONEYNESS_FIELD_NAME].notna()
        ].groupby(VALUATION_TIME_UTC_FIELD_NAME, sort=True)
    ]

    if len(support_by_valuation_time) == 0:
        raise _pricing_error(
            LOG_FORWARD_MONEYNESS_FIELD_NAME,
            "surface did not retain any finite log-forward moneyness values",
        )

    shared_lower_bound = max(lower_bound for lower_bound, _ in support_by_valuation_time)
    shared_upper_bound = min(upper_bound for _, upper_bound in support_by_valuation_time)

    shared_grid = tuple(
        grid_value
        for grid_value in reference_moneyness_grid
        if shared_lower_bound <= grid_value <= shared_upper_bound
    )

    if len(shared_grid) == 0:
        raise _pricing_error(
            LOG_FORWARD_MONEYNESS_FIELD_NAME,
            "surface did not retain any shared log-forward moneyness support",
        )

    return shared_grid


__all__ = [
    "JaniformObservedOptionSurfaceNode",
    "JaniformObservedOptionSurfacePricingResult",
]
