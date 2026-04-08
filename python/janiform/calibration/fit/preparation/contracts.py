from __future__ import annotations

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
    def _requires_session_inference(self) -> bool: ...

    @abstractmethod
    def _default_endogenous_mean_reversion_configuration(
        self,
    ) -> CalibrationMeanReversionConfiguration: ...

    @abstractmethod
    def _resolve_exchange_code_for_inference(self) -> str: ...

    @abstractmethod
    def _null_progress(self) -> AbstractProgress: ...

    @abstractmethod
    def _effective_workflow_scope(
        self,
        *,
        option_surface_path: Optional[str | Path],
    ) -> CalibrationMarketDataWorkflowScope: ...

    @abstractmethod
    def _resolve_market_profile_configuration(
        self,
    ) -> Optional[MarketProfileExplicitConfiguration]: ...

    @abstractmethod
    def _effective_session_reference(self) -> Optional[SessionPresetReference]: ...


class FitExecutionEstimator(FitWorkflowEstimator):
    _silent: bool

    @abstractmethod
    def _fit_progress_total_steps(self) -> int: ...

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
    ) -> PreparedFitWorkflow: ...

    @abstractmethod
    def _uses_default_prepared_runner(self) -> bool: ...

    @abstractmethod
    def _prepared_staged_runner(self) -> Callable[..., tuple[Any, dict[str, Any]]]: ...

    @abstractmethod
    def _build_fit_inference_metadata(
        self,
        prepared_workflow: PreparedFitWorkflow,
    ) -> dict[str, Any]: ...
