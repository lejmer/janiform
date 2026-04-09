"""Internal market-snapshot pricing workflow helpers."""

from __future__ import annotations

from datetime import datetime
from typing import TYPE_CHECKING, Optional

import pandas

from ......_workflow_progress import WorkflowProgressMode
from ......contracts import require_finite_scalar
from ......pricing import VanillaOptionPayoffType
from .....errors import calibration_error
from .....market_data.contracts import CalibrationOptionSurfaceColumnMapping
from ...._runtime.payloads import current_state_tick_data_column_mapping_for_model
from ....state import CurrentStateEstimationRequest
from ...grids import (
    JaniformVanillaOptionGrid,
    JaniformVanillaOptionGridBatch,
)
from ..result import JaniformObservedOptionSnapshotPricingResult
from .loading import (
    TabularFrameInput,
    filter_observed_tick_data,
    load_option_surface_frame,
    load_tick_data_frame,
    normalize_requested_option_type,
    observed_spot_from_tick_data,
    resolve_snapshot_valuation_time_utc,
)
from .preparation import (
    build_observed_option_snapshot_pricing_result,
    prepare_snapshot_groups,
)

if TYPE_CHECKING:
    from ....model import JaniformModel


def price_option_market_snapshot_for_model(
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
        valuation_time_utc=effective_valuation_time_utc,
        spot=spot,
        prepared_snapshot_groups=prepared_snapshot_groups,
        pricing_result=pricing_result,
    )
