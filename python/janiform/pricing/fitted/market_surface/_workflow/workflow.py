"""Internal market-surface pricing workflow boundary."""

from __future__ import annotations

from datetime import datetime
from pathlib import Path
from typing import TYPE_CHECKING, Optional

from ....._workflow_progress import WorkflowProgressMode
from .....calibration.market_data.contracts import CalibrationOptionSurfaceColumnMapping
from .....calibration.market_data.price_coordinates import PriceCoordinatePolicy
from .....pricing import VanillaOptionPayoffType
from ...internal.native_observed import (
    observed_surface_result_from_native_payload,
    run_native_observed_options_pricing,
)
from ...market_surface.result import JaniformObservedOptionSurfacePricingResult
from ...physical_distribution import ObservedOptionsPhysicalDistributionDiagnosticsRequest

if TYPE_CHECKING:
    from .....model import JaniformModel
    from ...market_snapshot.inputs import TabularFrameInput


def price_option_surface_for_model(
    model: JaniformModel,
    *,
    tick_data: TabularFrameInput,
    option_surface: TabularFrameInput,
    valuation_time_utc: Optional[datetime] = None,
    option_type: Optional[VanillaOptionPayoffType | str] = None,
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping] = None,
    risk_free_rate: float = 0.0,
    dividend_yield: float = 0.0,
    valuation_count: Optional[int] = None,
    use_all_valuation_times: bool = False,
    cpu_count: Optional[int] = None,
    price_coordinate_policy: PriceCoordinatePolicy | str = PriceCoordinatePolicy.AUTO,
    option_forward_parity_warning_threshold: Optional[float] = None,
    physical_distribution_diagnostics: (
        bool | ObservedOptionsPhysicalDistributionDiagnosticsRequest
    ) = False,
    output_path: Optional[str | Path] = None,
    silent: bool = False,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
) -> JaniformObservedOptionSurfacePricingResult:
    native_payload = run_native_observed_options_pricing(
        model,
        tick_data=tick_data,
        option_surface=option_surface,
        valuation_time_utc=valuation_time_utc,
        option_type=option_type,
        option_surface_column_mapping=option_surface_column_mapping,
        risk_free_rate=risk_free_rate,
        dividend_yield=dividend_yield,
        valuation_count=valuation_count,
        use_all_valuation_times=use_all_valuation_times,
        cpu_count=cpu_count,
        price_coordinate_policy=price_coordinate_policy,
        option_forward_parity_warning_threshold=option_forward_parity_warning_threshold,
        physical_distribution_diagnostics=physical_distribution_diagnostics,
        output_path=output_path,
        silent=silent,
        progress_mode=progress_mode,
        scope="surface",
    )

    return observed_surface_result_from_native_payload(native_payload)


__all__ = ["price_option_surface_for_model"]
