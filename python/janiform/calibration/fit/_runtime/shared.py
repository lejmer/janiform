from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime

from ....calendar import CalendarRegistry
from ....hourly_factors import HourlyFactorContract
from ....mean_reversion import (
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
)
from ....pricing import (
    GammaSeverityMarkConfiguration,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPricingKernelParameters,
    VanillaOptionPricingModelParameters,
)
from ....return_clock import ReturnClockExposureParameters
from ....simulation.coarse import NewsProcessConfiguration
from ....simulation.shared import HawkesExcitationState, HawkesKernelParameters

MODEL_PERSISTENCE_SCHEMA_VERSION = "calibration.high_level.model.v1"
DEFAULT_SIMULATION_INTERVAL = "1h"
MEAN_REVERSION_RUNTIME_INTERVAL_ALIGNMENT_POLICY = "explicit_runtime_interval_grid_alignment.v1"


@dataclass(frozen=True, slots=True)
class PreparedModelRuntimeFactors:
    calendar_registry: CalendarRegistry
    exchange_code: str
    interval_duration_nanos: int
    interval_delta: float
    range_end_utc: datetime
    kernel_parameters: HawkesKernelParameters
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration
    pricing_kernel_parameters: VanillaOptionPricingKernelParameters
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration
    news_process_configuration: NewsProcessConfiguration
    return_clock_exposure_parameters: ReturnClockExposureParameters


@dataclass(frozen=True, slots=True)
class PreparedModelPricingRuntimeInput:
    runtime_factors: PreparedModelRuntimeFactors
    hourly_factor_contract: HourlyFactorContract
    effective_boundary_state: HawkesExcitationState
    effective_observed_boundary_state: MeanReversionBoundaryState
    mean_reversion_fitted_parameters: MeanReversionFittedParameters
    model_parameters: VanillaOptionPricingModelParameters


@dataclass(frozen=True, slots=True)
class RuntimeStructuralSeasonalityGrid:
    interval_start_utc_nanos: tuple[int, ...]
    seasonality_factors: tuple[float, ...]
