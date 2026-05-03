"""Shared observed-option snapshot pricing helpers."""

from __future__ import annotations

from datetime import datetime
from typing import TYPE_CHECKING, Optional

import pandas

from ....._workflow_progress import WorkflowProgressMode
from janiform.calibration.market_data.contracts import CalibrationTickDataColumnMapping
from janiform.calibration.fit._runtime.option_pricing.workflow import price_option_grid_for_model
from janiform.calibration.fit.state import CurrentStateEstimationRequest
from ...grids import (
    JaniformVanillaOptionGrid,
    JaniformVanillaOptionGridBatch,
)
from ..result import JaniformObservedOptionSnapshotPricingResult
from .preparation import (
    PreparedSnapshotGroupInput,
    build_observed_option_snapshot_pricing_result,
)

if TYPE_CHECKING:
    from .....model import JaniformModel


def price_prepared_snapshot_groups_for_model(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    spot: float,
    prepared_snapshot_groups: tuple[PreparedSnapshotGroupInput, ...],
    filtered_tick_data: pandas.DataFrame,
    resolved_tick_column_mapping: CalibrationTickDataColumnMapping,
    risk_free_rate: float,
    dividend_yield: float,
    cpu_count: Optional[int],
    silent: bool,
    progress_mode: WorkflowProgressMode | str,
) -> JaniformObservedOptionSnapshotPricingResult:
    pricing_result = price_option_grid_for_model(
        model,
        valuation_time_utc=valuation_time_utc,
        batch=JaniformVanillaOptionGridBatch(
            spot=spot,
            grids=tuple(
                JaniformVanillaOptionGrid(
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
        valuation_time_utc=valuation_time_utc,
        spot=spot,
        prepared_snapshot_groups=prepared_snapshot_groups,
        pricing_result=pricing_result,
    )


__all__ = ["price_prepared_snapshot_groups_for_model"]
