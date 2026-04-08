from __future__ import annotations

from collections.abc import Callable
from abc import ABC, abstractmethod
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any, Optional

from ...._workflow_progress import AbstractProgress
from ...market_data.contracts import (
    CalibrationFlatCarryInput,
    CalibrationInputFileFormat,
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
from ..inference import FitInference


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


class FitWorkflowEstimator(ABC):
    _option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping]
    _output_preferences: CalibrationMarketDataOutputPreferences
    _optimizer_settings_by_stage: tuple[CalibrationStageSolverSettings, ...]
    _preprocessing_settings: Optional[CalibrationMarketDataPreprocessingSettings]
    _mean_reversion: Optional[CalibrationMeanReversionConfiguration]
    _session_timezone: Optional[str]
    _cpu_count: int
    _registry: PresetRegistry
    _market_profile_reference: Optional[MarketProfileReference]
    _known_news_reference: Optional[KnownNewsPresetReference]
    _tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping]
    _normalization_policy: Optional[CalibrationMarketDataNormalizationPolicy]

    @abstractmethod
    def _requires_session_inference(self) -> bool:
        raise NotImplementedError()

    @abstractmethod
    def _default_endogenous_mean_reversion_configuration(
        self,
    ) -> CalibrationMeanReversionConfiguration:
        raise NotImplementedError()

    @abstractmethod
    def _resolve_exchange_code_for_inference(self) -> str:
        raise NotImplementedError()

    @abstractmethod
    def _null_progress(self) -> AbstractProgress:
        raise NotImplementedError()

    @abstractmethod
    def _effective_workflow_scope(
        self,
        *,
        option_surface_path: Optional[str | Path],
    ) -> CalibrationMarketDataWorkflowScope:
        raise NotImplementedError()

    @abstractmethod
    def _resolve_market_profile_configuration(
        self,
    ) -> Optional[MarketProfileExplicitConfiguration]:
        raise NotImplementedError()

    @abstractmethod
    def _effective_session_reference(self) -> Optional[SessionPresetReference]:
        raise NotImplementedError()


class FitExecutionEstimator(FitWorkflowEstimator):
    _silent: bool

    @abstractmethod
    def _fit_progress_total_steps(self) -> int:
        raise NotImplementedError()

    @abstractmethod
    def _prepare_fit_workflow(
        self,
        *,
        run_id: Optional[str],
        tick_data_path: str | Path,
        tick_data_format: Optional[CalibrationInputFileFormat],
        tick_data_provenance_label: str,
        option_surface_path: Optional[str | Path],
        option_surface_format: CalibrationInputFileFormat,
        option_surface_provenance_label: str,
        option_surface_valuation_time_utc: Optional[datetime] = None,
        carry: Optional[CalibrationFlatCarryInput] = None,
        progress: Optional[AbstractProgress] = None,
    ) -> PreparedFitWorkflow:
        raise NotImplementedError()

    @abstractmethod
    def _uses_default_prepared_runner(self) -> bool:
        raise NotImplementedError()

    @abstractmethod
    def _prepared_staged_runner(self) -> Callable[..., tuple[Any, dict[str, Any]]]:
        raise NotImplementedError()

    @abstractmethod
    def _build_fit_inference_metadata(
        self,
        prepared_workflow: PreparedFitWorkflow,
    ) -> dict[str, Any]:
        raise NotImplementedError()
