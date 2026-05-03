"""Internal market-snapshot pricing workflow helpers."""

from __future__ import annotations

from datetime import datetime
from typing import TYPE_CHECKING, Optional

from ....._workflow_progress import WorkflowProgressMode
from janiform.contracts.base import require_finite_scalar
from janiform.pricing import VanillaOptionPayoffType
from janiform.pricing.errors import _pricing_error
from janiform.calibration.market_data.contracts import CalibrationOptionSurfaceColumnMapping
from .context import prepare_observed_option_market_context
from .pricing import price_prepared_snapshot_groups_for_model
from .preparation import prepare_snapshot_groups
from ..result import JaniformObservedOptionSnapshotPricingResult

if TYPE_CHECKING:
    from .....model import JaniformModel
    from ..inputs import TabularFrameInput


def price_option_snapshot_for_model(
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
    require_finite_scalar(risk_free_rate, "risk_free_rate", _pricing_error)
    require_finite_scalar(dividend_yield, "dividend_yield", _pricing_error)

    context = prepare_observed_option_market_context(
        model,
        tick_data=tick_data,
        option_surface=option_surface,
        valuation_time_utc=valuation_time_utc,
        option_type=option_type,
        option_surface_column_mapping=option_surface_column_mapping,
    )

    prepared_snapshot_groups = prepare_snapshot_groups(
        option_surface_frame=context.option_surface_frame,
        resolved_option_columns=context.resolved_option_columns,
        valuation_time_utc=context.valuation_time_utc,
        option_type=context.option_type,
    )

    return price_prepared_snapshot_groups_for_model(
        model,
        valuation_time_utc=context.valuation_time_utc,
        spot=context.spot,
        prepared_snapshot_groups=prepared_snapshot_groups,
        filtered_tick_data=context.filtered_tick_data,
        resolved_tick_column_mapping=context.resolved_tick_column_mapping,
        risk_free_rate=risk_free_rate,
        dividend_yield=dividend_yield,
        cpu_count=cpu_count,
        silent=silent,
        progress_mode=progress_mode,
    )
