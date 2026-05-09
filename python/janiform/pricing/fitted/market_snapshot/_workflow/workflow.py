"""Internal market-snapshot pricing workflow boundary."""

from __future__ import annotations

from datetime import datetime
from pathlib import Path
from typing import TYPE_CHECKING, Optional

from ....._workflow_progress import WorkflowProgressMode
from .....calibration.market_data.contracts import CalibrationOptionSurfaceColumnMapping
from .....pricing import VanillaOptionPayoffType
from ...internal.native_observed import (
    observed_snapshot_result_from_native_payload,
    run_native_observed_options_pricing,
)
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
    output_path: Optional[str | Path] = None,
    silent: bool = False,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
) -> JaniformObservedOptionSnapshotPricingResult:
    native_payload = run_native_observed_options_pricing(
        model,
        tick_data=tick_data,
        option_surface=option_surface,
        valuation_time_utc=valuation_time_utc,
        option_type=option_type,
        option_surface_column_mapping=option_surface_column_mapping,
        risk_free_rate=risk_free_rate,
        dividend_yield=dividend_yield,
        valuation_count=None,
        use_all_valuation_times=False,
        cpu_count=cpu_count,
        output_path=output_path,
        silent=silent,
        progress_mode=progress_mode,
        scope="snapshot",
    )

    return observed_snapshot_result_from_native_payload(native_payload)


__all__ = ["price_option_snapshot_for_model"]
