"""Public market-snapshot pricing contracts and workflow helpers."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from typing import TYPE_CHECKING, Any, Optional

import pandas

from .....contracts import require_finite_scalar
from .....pricing import (
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffType,
    VanillaOptionPricingFailureStage,
    VanillaOptionPricingStatus,
)
from ....._workflow_progress import WorkflowProgressMode
from ....errors import calibration_error
from ....market_data.contracts import CalibrationOptionSurfaceColumnMapping
from ..._runtime.payloads import current_state_tick_data_column_mapping_for_model
from ...state import CurrentStateEstimationRequest
from ..grids import (
    JaniformGroupedVanillaOptionBatch,
    JaniformGroupedVanillaOptionGroup,
    JaniformVanillaOptionGroupedBatchPricingResult,
)
from .loading import (
    TabularFrameInput,
    filter_observed_tick_data,
    load_option_surface_frame,
    load_tick_data_frame,
    normalize_requested_option_type,
    observed_spot_from_tick_data,
    resolve_snapshot_valuation_time_utc,
)

if TYPE_CHECKING:
    from ...model import JaniformModel


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


from .preparation import (  # noqa: E402
    build_observed_option_snapshot_pricing_result,
    prepare_snapshot_groups,
)


def _price_option_market_snapshot(
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

    tick_column_mapping = current_state_tick_data_column_mapping_for_model(
        model,
        request=CurrentStateEstimationRequest(observed_tick_data=pandas.DataFrame()),
    )

    tick_data_frame, resolved_tick_column_mapping = load_tick_data_frame(
        tick_data,
        tick_column_mapping=tick_column_mapping,
    )
    option_surface_frame, resolved_option_columns = load_option_surface_frame(
        option_surface,
        option_surface_column_mapping=option_surface_column_mapping,
    )

    effective_valuation_time_utc = resolve_snapshot_valuation_time_utc(
        option_surface_frame=option_surface_frame,
        quote_timestamp_column=resolved_option_columns.quote_timestamp_column,
        valuation_time_utc=valuation_time_utc,
    )
    normalized_option_type = normalize_requested_option_type(option_type)

    filtered_tick_data = filter_observed_tick_data(
        tick_data_frame=tick_data_frame,
        timestamp_column=resolved_tick_column_mapping.timestamp_column,
        valuation_time_utc=effective_valuation_time_utc,
    )
    spot = observed_spot_from_tick_data(
        filtered_tick_data,
        price_column=resolved_tick_column_mapping.price_column,
    )

    prepared_snapshot_groups = prepare_snapshot_groups(
        option_surface_frame=option_surface_frame,
        resolved_option_columns=resolved_option_columns,
        valuation_time_utc=effective_valuation_time_utc,
        option_type=normalized_option_type,
    )

    pricing_result = model.price_option_grids(
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

    return build_observed_option_snapshot_pricing_result(
        valuation_time_utc=effective_valuation_time_utc,
        spot=spot,
        prepared_snapshot_groups=prepared_snapshot_groups,
        pricing_result=pricing_result,
    )


__all__ = [
    "JaniformObservedOptionSnapshotNode",
    "JaniformObservedOptionSnapshotPricingResult",
]
