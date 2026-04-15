from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from tempfile import TemporaryDirectory
from typing import Any, Optional

from ...market_data.contracts import (
    CalibrationKnownNewsColumnMapping,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataPreprocessingSettings,
    CalibrationMarketDataWorkflowScope,
    CalibrationMeanReversionConfiguration,
    CalibrationOptionSurfaceColumnMapping,
    CalibrationTickDataColumnMapping,
)
from ...market_data.workflow import PreparedMarketDataCalibration
from ...presets.references import (
    KnownNewsPresetReference,
    MarketProfileExplicitConfiguration,
    MarketProfileReference,
    SessionPresetReference,
)
from ...presets.registry import PresetRegistry
from ...presets.resolution import ResolvedPresetWorkflowConfiguration
from ...staged import CalibrationStageSolverSettings
from .._inference import FitInference


DEFAULT_KNOWN_NEWS_COLUMNS = CalibrationKnownNewsColumnMapping(
    event_timestamp_column="event_timestamp_utc",
    event_identifier_column="event_id",
)


@dataclass(frozen=True, slots=True)
class PreparedFitWorkflow:
    run_id: str
    workflow_scope: CalibrationMarketDataWorkflowScope
    prepared: PreparedMarketDataCalibration
    resolved_configuration: ResolvedPresetWorkflowConfiguration
    input_inference: FitInference
    fit_warning: Optional[str]
    reference_price: Optional[float]
    temporary_directory: Optional[TemporaryDirectory[str]]


@dataclass(frozen=True, slots=True)
class FitPreparationConfiguration:
    workflow_scope_override: Optional[CalibrationMarketDataWorkflowScope]
    tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping]
    normalization_policy: Optional[CalibrationMarketDataNormalizationPolicy]
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping]
    output_preferences: CalibrationMarketDataOutputPreferences
    optimizer_settings_by_stage: tuple[CalibrationStageSolverSettings, ...]
    preprocessing_settings: Optional[CalibrationMarketDataPreprocessingSettings]
    mean_reversion: Optional[CalibrationMeanReversionConfiguration]
    dynamics_search_restart_count: Optional[int]
    default_endogenous_mean_reversion_configuration: CalibrationMeanReversionConfiguration
    market_profile_reference: Optional[MarketProfileReference]
    known_news_reference: Optional[KnownNewsPresetReference]
    resolved_market_profile_configuration: Optional[MarketProfileExplicitConfiguration]
    effective_session_reference: Optional[SessionPresetReference]
    requires_session_inference: bool
    exchange_code_for_inference: Optional[str]
    session_timezone: Optional[str]
    registry: PresetRegistry
    cpu_count: int


@dataclass(frozen=True, slots=True)
class FitExecutionConfiguration:
    silent: bool
    progress_total_steps: int
    preparation: FitPreparationConfiguration
    output_controller_factory: Callable[..., Any]
    prepare_fit_workflow: Callable[..., PreparedFitWorkflow]
    prepared_staged_runner: Callable[..., tuple[Any, dict[str, Any]]]
    use_default_prepared_runner: bool
    evented_prepared_runner: Callable[..., tuple[Any, dict[str, Any]]]
    build_fit_inference_metadata: Callable[[PreparedFitWorkflow], dict[str, Any]]
