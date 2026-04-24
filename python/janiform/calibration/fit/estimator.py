from __future__ import annotations

import os
from contextlib import AbstractContextManager, nullcontext, redirect_stdout
from datetime import datetime
from io import StringIO
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any, Optional

from ._execution import fit_with_execution_configuration
from ._inference import build_fit_inference_payload
from ._preparation.configuration import (
    coerce_known_news_reference,
    coerce_market_profile_reference,
    coerce_session_reference,
    exchange_code_from_market_profile_reference,
    exchange_code_from_session_reference,
)
from ._preparation.contracts import (
    FitExecutionConfiguration,
    FitPreparationConfiguration,
)
from ._preparation.workflow import (
    PreparedFitWorkflow,
    build_market_data_request,
    fit_progress_total_steps,
    infer_session_resolution,
    prepare_fit_workflow,
)
from .model import JaniformModel
from .result import JaniformFitResult
from ..errors import calibration_error
from ..market_data.contracts import (
    CalibrationExecutionMode,
    CalibrationFlatCarryInput,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataPreprocessingSettings,
    CalibrationMarketDataWorkflowScope,
    CalibrationMeanReversionConfiguration,
    CalibrationOptionSurfaceColumnMapping,
    CalibrationTickDataColumnMapping,
)
from ..market_data.request import MarketDataCalibrationRequest
from ..market_data.translation import (
    NormalizedMarketDataSource,
    normalize_market_data_source,
    resolve_market_data_session,
    translate_market_data_request_to_prepared_calibration_from_phases,
)
from ..market_data.workflow import (
    _prepare_market_data_calibration_from_translation_result,
)
from ...pricing import PricingIntervalSchedulePolicy, default_pricing_interval_schedule_policy
from ..presets.references import (
    KnownNewsPresetReference,
    MarketProfileExplicitConfiguration,
    MarketProfileReference,
    PresetReferenceUsageMode,
    SessionPresetReference,
)
from ..presets.registry import PresetCategory, PresetRegistry
from ..presets.resolution import ResolvedPresetWorkflowConfiguration, resolve_preset_workflow_configuration
from ..sessions.contracts import MarketDataSessionResolution
from ..staged import CalibrationResumeSettings, CalibrationStageSolverSettings
from ..staged import runtime as staged_runtime_module
from ..staged.runtime import (
    _run_staged_calibration_with_prepared_handle_resume_and_events,
    run_prepared_staged_calibration_with_raw_payload,
    run_staged_calibration_with_prepared_handle_and_events,
)
from ..._workflow_progress import AbstractProgress, NullProgress
from ..._workflow_progress import JaniformOutputController

_DEFAULT_OUTPUT_PREFERENCES = CalibrationMarketDataOutputPreferences(
    execution_mode=CalibrationExecutionMode.MEMORY,
    emit_derived_staged_execution_input=False,
    emit_input_provenance_report=True,
)
_DEFAULT_ENDOGENOUS_MEAN_REVERSION_CONFIGURATION = CalibrationMeanReversionConfiguration()


class JaniformEstimator:
    _workflow_scope_override: Optional[CalibrationMarketDataWorkflowScope]
    _tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping]
    _normalization_policy: Optional[CalibrationMarketDataNormalizationPolicy]
    _option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping]
    _output_preferences: CalibrationMarketDataOutputPreferences
    _optimizer_settings_by_stage: tuple[CalibrationStageSolverSettings, ...]
    _preprocessing_settings: Optional[CalibrationMarketDataPreprocessingSettings]
    _mean_reversion: Optional[CalibrationMeanReversionConfiguration]
    _pricing_interval_schedule_policy: PricingIntervalSchedulePolicy
    _dynamics_search_restart_candidate_budget: Optional[int]
    _market_profile_reference: Optional[MarketProfileReference]
    _session_reference: Optional[SessionPresetReference]
    _known_news_reference: Optional[KnownNewsPresetReference]
    _exchange_code: Optional[str]
    _session_timezone: Optional[str]
    _registry: PresetRegistry
    _cpu_count: int
    _silent: bool

    def __init__(
        self,
        *,
        workflow_scope: Optional[CalibrationMarketDataWorkflowScope] = None,
        tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping] = None,
        normalization_policy: Optional[CalibrationMarketDataNormalizationPolicy] = None,
        option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping] = None,
        output_preferences: Optional[CalibrationMarketDataOutputPreferences] = None,
        optimizer_settings_by_stage: tuple[CalibrationStageSolverSettings, ...] = (),
        preprocessing_settings: Optional[CalibrationMarketDataPreprocessingSettings] = None,
        mean_reversion: Optional[CalibrationMeanReversionConfiguration] = None,
        pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
        dynamics_search_restart_candidate_budget: Optional[int] = None,
        market_profile: Optional[str | MarketProfileReference] = None,
        session: Optional[str | Path | SessionPresetReference] = None,
        known_news: Optional[str | KnownNewsPresetReference] = None,
        exchange_code: Optional[str] = None,
        session_timezone: Optional[str] = None,
        registry: Optional[PresetRegistry] = None,
        cpu_count: Optional[int] = None,
        silent: bool = False,
    ) -> None:
        if exchange_code is not None and not exchange_code.strip():
            raise calibration_error("exchange_code", "must be a non-empty string when provided")

        if session_timezone is not None and not session_timezone.strip():
            raise calibration_error(
                "session_timezone",
                "must be a non-empty string when provided",
            )

        if cpu_count is not None and cpu_count <= 0:
            raise calibration_error("cpu_count", "must be strictly positive when provided")

        if (
            preprocessing_settings is not None
            and not isinstance(preprocessing_settings, CalibrationMarketDataPreprocessingSettings)
        ):
            raise calibration_error(
                "preprocessing_settings",
                "must be a CalibrationMarketDataPreprocessingSettings when provided",
            )

        if mean_reversion is not None and not isinstance(mean_reversion, CalibrationMeanReversionConfiguration):
            raise calibration_error(
                "mean_reversion",
                "must be a CalibrationMeanReversionConfiguration when provided",
            )
        if (
            dynamics_search_restart_candidate_budget is not None
            and dynamics_search_restart_candidate_budget <= 0
        ):
            raise calibration_error(
                "dynamics_search_restart_candidate_budget",
                "must be strictly positive when provided",
            )

        self._workflow_scope_override = workflow_scope
        self._tick_data_column_mapping = tick_data_column_mapping
        self._normalization_policy = normalization_policy
        self._option_surface_column_mapping = option_surface_column_mapping
        self._output_preferences = _DEFAULT_OUTPUT_PREFERENCES if output_preferences is None else output_preferences
        self._optimizer_settings_by_stage = optimizer_settings_by_stage
        self._preprocessing_settings = preprocessing_settings
        self._mean_reversion = mean_reversion
        self._pricing_interval_schedule_policy = (
            default_pricing_interval_schedule_policy()
            if pricing_interval_schedule_policy is None
            else pricing_interval_schedule_policy
        )
        self._dynamics_search_restart_candidate_budget = dynamics_search_restart_candidate_budget
        self._market_profile_reference = coerce_market_profile_reference(market_profile)
        self._session_reference = coerce_session_reference(session, exchange_code=exchange_code)
        self._known_news_reference = coerce_known_news_reference(known_news)
        self._exchange_code = exchange_code
        self._session_timezone = session_timezone
        self._registry = PresetRegistry() if registry is None else registry
        self._cpu_count = max(1, os.cpu_count() or 1) if cpu_count is None else cpu_count
        self._silent = silent

    def fit(
        self,
        *,
        run_id: Optional[str] = None,
        resume_settings: Optional[CalibrationResumeSettings] = None,
        tick_data_path: str | Path,
        tick_data_format: Optional[CalibrationInputFileFormat] = None,
        tick_data_provenance_label: str = "tick_data",
        option_surface_path: Optional[str | Path] = None,
        option_surface_format: CalibrationInputFileFormat = CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label: str = "option_surface",
        option_surface_valuation_time_utc: Optional[datetime] = None,
        carry: Optional[CalibrationFlatCarryInput] = None,
        pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
        output_dir: Optional[str | Path] = None,
        silent: Optional[bool] = None,
    ) -> JaniformModel:
        return fit_with_execution_configuration(
            self._build_execution_configuration(
                silent=silent,
                pricing_interval_schedule_policy=pricing_interval_schedule_policy,
            ),
            run_id=run_id,
            resume_settings=resume_settings,
            tick_data_path=tick_data_path,
            tick_data_format=tick_data_format,
            tick_data_provenance_label=tick_data_provenance_label,
            option_surface_path=option_surface_path,
            option_surface_format=option_surface_format,
            option_surface_provenance_label=option_surface_provenance_label,
            option_surface_valuation_time_utc=option_surface_valuation_time_utc,
            carry=carry,
            output_dir=output_dir,
            report=False,
        )

    def fit_with_report(
        self,
        *,
        run_id: Optional[str] = None,
        resume_settings: Optional[CalibrationResumeSettings] = None,
        tick_data_path: str | Path,
        tick_data_format: Optional[CalibrationInputFileFormat] = None,
        tick_data_provenance_label: str = "tick_data",
        option_surface_path: Optional[str | Path] = None,
        option_surface_format: CalibrationInputFileFormat = CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label: str = "option_surface",
        option_surface_valuation_time_utc: Optional[datetime] = None,
        carry: Optional[CalibrationFlatCarryInput] = None,
        pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
        output_dir: Optional[str | Path] = None,
        silent: Optional[bool] = None,
    ) -> JaniformFitResult:
        model, fit_report = fit_with_execution_configuration(
            self._build_execution_configuration(
                silent=silent,
                pricing_interval_schedule_policy=pricing_interval_schedule_policy,
            ),
            run_id=run_id,
            resume_settings=resume_settings,
            tick_data_path=tick_data_path,
            tick_data_format=tick_data_format,
            tick_data_provenance_label=tick_data_provenance_label,
            option_surface_path=option_surface_path,
            option_surface_format=option_surface_format,
            option_surface_provenance_label=option_surface_provenance_label,
            option_surface_valuation_time_utc=option_surface_valuation_time_utc,
            carry=carry,
            output_dir=output_dir,
            report=True,
        )

        return JaniformFitResult(
            model=model,
            report=fit_report,
        )

    def _fit_progress_total_steps(self) -> int:
        return fit_progress_total_steps(self._build_preparation_configuration())

    def _build_preparation_configuration(self) -> FitPreparationConfiguration:
        return self._build_preparation_configuration_with_override(
            pricing_interval_schedule_policy=None,
        )

    def _build_preparation_configuration_with_override(
        self,
        *,
        pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy],
    ) -> FitPreparationConfiguration:
        resolved_market_profile_configuration = self._resolve_market_profile_configuration()
        requires_session_inference = self._requires_session_inference()
        return FitPreparationConfiguration(
            workflow_scope_override=self._workflow_scope_override,
            tick_data_column_mapping=self._tick_data_column_mapping,
            normalization_policy=self._normalization_policy,
            option_surface_column_mapping=self._option_surface_column_mapping,
            output_preferences=self._output_preferences,
            optimizer_settings_by_stage=self._optimizer_settings_by_stage,
            preprocessing_settings=self._preprocessing_settings,
            mean_reversion=self._mean_reversion,
            pricing_interval_schedule_policy=(
                self._pricing_interval_schedule_policy
                if pricing_interval_schedule_policy is None
                else pricing_interval_schedule_policy
            ),
            dynamics_search_restart_candidate_budget=self._dynamics_search_restart_candidate_budget,
            default_endogenous_mean_reversion_configuration=(
                self._default_endogenous_mean_reversion_configuration()
            ),
            market_profile_reference=self._market_profile_reference,
            known_news_reference=self._known_news_reference,
            resolved_market_profile_configuration=resolved_market_profile_configuration,
            effective_session_reference=self._effective_session_reference(),
            requires_session_inference=requires_session_inference,
            exchange_code_for_inference=(
                self._resolve_exchange_code_for_inference() if requires_session_inference else None
            ),
            session_timezone=self._session_timezone,
            registry=self._registry,
            cpu_count=self._cpu_count,
        )

    def _build_execution_configuration(
        self,
        *,
        silent: Optional[bool],
        pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy],
    ) -> FitExecutionConfiguration:
        preparation_configuration = self._build_preparation_configuration_with_override(
            pricing_interval_schedule_policy=pricing_interval_schedule_policy,
        )
        effective_silent = self._silent if silent is None else silent
        prepared_staged_runner = run_prepared_staged_calibration_with_raw_payload
        return FitExecutionConfiguration(
            silent=effective_silent,
            progress_total_steps=fit_progress_total_steps(preparation_configuration),
            preparation=preparation_configuration,
            output_controller_factory=JaniformOutputController,
            prepare_fit_workflow=lambda **kwargs: prepare_fit_workflow(
                preparation_configuration,
                **kwargs,
                temporary_directory_factory=TemporaryDirectory,
                normalize_market_data_source_method=normalize_market_data_source,
                resolve_market_data_session_method=resolve_market_data_session,
                resolve_preset_workflow_configuration_method=resolve_preset_workflow_configuration,
            translate_market_data_request_to_prepared_calibration_from_phases_method=(
                translate_market_data_request_to_prepared_calibration_from_phases
            ),
                prepare_market_data_calibration_from_translation_result_method=(
                    _prepare_market_data_calibration_from_translation_result
                ),
            ),
            prepared_staged_runner=prepared_staged_runner,
            use_default_prepared_runner=(
                prepared_staged_runner is staged_runtime_module.run_prepared_staged_calibration_with_raw_payload
            ),
            evented_prepared_runner=run_staged_calibration_with_prepared_handle_and_events,
            resume_evented_prepared_runner=(
                _run_staged_calibration_with_prepared_handle_resume_and_events
            ),
            build_fit_inference_metadata=self._build_fit_inference_metadata,
        )

    def _build_market_data_request(
        self,
        *,
        run_id: str,
        workflow_scope: CalibrationMarketDataWorkflowScope,
        tick_data_source: CalibrationInputFileReference,
        option_surface_source: Optional[CalibrationInputFileReference],
        option_surface_valuation_time_utc: Optional[datetime],
        carry: Optional[CalibrationFlatCarryInput],
        resolved_configuration: ResolvedPresetWorkflowConfiguration,
        tick_data_column_mapping: CalibrationTickDataColumnMapping,
        normalization_policy: CalibrationMarketDataNormalizationPolicy,
        working_directory: Optional[Path],
    ) -> MarketDataCalibrationRequest:
        return build_market_data_request(
            self._build_preparation_configuration(),
            run_id=run_id,
            workflow_scope=workflow_scope,
            tick_data_source=tick_data_source,
            option_surface_source=option_surface_source,
            option_surface_valuation_time_utc=option_surface_valuation_time_utc,
            carry=carry,
            resolved_configuration=resolved_configuration,
            tick_data_column_mapping=tick_data_column_mapping,
            normalization_policy=normalization_policy,
            working_directory=working_directory,
        )

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
        return prepare_fit_workflow(
            self._build_preparation_configuration(),
            run_id=run_id,
            tick_data_path=tick_data_path,
            tick_data_format=tick_data_format,
            tick_data_provenance_label=tick_data_provenance_label,
            option_surface_path=option_surface_path,
            option_surface_format=option_surface_format,
            option_surface_provenance_label=option_surface_provenance_label,
            option_surface_valuation_time_utc=option_surface_valuation_time_utc,
            carry=carry,
            progress=progress,
            temporary_directory_factory=TemporaryDirectory,
            normalize_market_data_source_method=normalize_market_data_source,
            resolve_market_data_session_method=resolve_market_data_session,
            resolve_preset_workflow_configuration_method=resolve_preset_workflow_configuration,
            translate_market_data_request_to_prepared_calibration_from_phases_method=(
                translate_market_data_request_to_prepared_calibration_from_phases
            ),
            prepare_market_data_calibration_from_translation_result_method=(
                _prepare_market_data_calibration_from_translation_result
            ),
        )

    def _effective_session_reference(self) -> Optional[SessionPresetReference]:
        if self._session_reference is not None:
            return self._session_reference
        if not self._requires_session_inference():
            return None

        return SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.AUTOMATIC,
            exchange_code_hint=self._exchange_code,
            timezone_name_hint=self._session_timezone,
        )

    def _effective_workflow_scope(
        self,
        *,
        option_surface_path: Optional[str | Path],
    ) -> CalibrationMarketDataWorkflowScope:
        if self._workflow_scope_override is not None:
            return self._workflow_scope_override
        if option_surface_path is not None:
            return CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE

        return CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE

    def _infer_session_resolution(
        self,
        *,
        run_id: str,
        workflow_scope: CalibrationMarketDataWorkflowScope,
        tick_data_source: CalibrationInputFileReference,
        option_surface_source: Optional[CalibrationInputFileReference],
        option_surface_valuation_time_utc: Optional[datetime],
        carry: Optional[CalibrationFlatCarryInput],
        tick_data_column_mapping: CalibrationTickDataColumnMapping,
        normalization_policy: CalibrationMarketDataNormalizationPolicy,
        normalized_market_data: NormalizedMarketDataSource,
        progress: AbstractProgress,
    ) -> Optional[MarketDataSessionResolution]:
        return infer_session_resolution(
            self._build_preparation_configuration(),
            run_id=run_id,
            workflow_scope=workflow_scope,
            tick_data_source=tick_data_source,
            option_surface_source=option_surface_source,
            option_surface_valuation_time_utc=option_surface_valuation_time_utc,
            carry=carry,
            tick_data_column_mapping=tick_data_column_mapping,
            normalization_policy=normalization_policy,
            normalized_market_data=normalized_market_data,
            progress=progress,
        )

    def _requires_session_inference(self) -> bool:
        if self._session_reference is not None:
            return self._session_reference.usage_mode == PresetReferenceUsageMode.AUTOMATIC

        market_profile_configuration = self._resolve_market_profile_configuration()
        if market_profile_configuration is None:
            return True

        return market_profile_configuration.session_reference.usage_mode == PresetReferenceUsageMode.AUTOMATIC

    def _resolve_market_profile_configuration(self) -> Optional[MarketProfileExplicitConfiguration]:
        if self._market_profile_reference is None:
            return None

        if self._market_profile_reference.usage_mode == PresetReferenceUsageMode.EXPLICIT:
            configuration = self._market_profile_reference.configuration
            if configuration is None:
                raise calibration_error(
                    "market_profile.configuration",
                    "is required when usage_mode=explicit",
                )

            return configuration

        if self._market_profile_reference.preset_name is None:
            raise calibration_error(
                "market_profile.preset_name",
                "is required for named market-profile references",
            )

        if self._market_profile_reference.registry_scope is None:
            raise calibration_error(
                "market_profile.registry_scope",
                "is required for named market-profile references",
            )

        _, artifact = self._registry.resolve(
            category=PresetCategory.MARKET_PROFILE,
            preset_name=self._market_profile_reference.preset_name,
            registry_scope=self._market_profile_reference.registry_scope,
        )

        return artifact.configuration

    def _resolve_exchange_code_for_inference(self) -> str:
        if self._exchange_code is not None:
            return self._exchange_code

        if self._session_reference is not None:
            return exchange_code_from_session_reference(self._session_reference, self._registry)

        if self._market_profile_reference is not None:
            return exchange_code_from_market_profile_reference(
                self._market_profile_reference,
                self._registry,
            )

        raise calibration_error(
            "exchange_code",
            (
                "is required for automatic session inference when no session preset or market "
                "profile provides the exchange baseline"
            ),
        )

    def _stdout_suppression_context(
        self,
        *,
        effective_silent: Optional[bool] = None,
    ) -> AbstractContextManager[Optional[StringIO]]:
        if self._silent if effective_silent is None else effective_silent:
            return redirect_stdout(StringIO())

        return nullcontext()

    def _null_progress(self) -> NullProgress:
        return NullProgress()

    def _default_endogenous_mean_reversion_configuration(
        self,
    ) -> CalibrationMeanReversionConfiguration:
        return _DEFAULT_ENDOGENOUS_MEAN_REVERSION_CONFIGURATION

    def _build_fit_inference_metadata(
        self,
        prepared_workflow: PreparedFitWorkflow,
    ) -> dict[str, Any]:
        return build_fit_inference_payload(
            inference=prepared_workflow.input_inference,
            workflow_scope=prepared_workflow.workflow_scope,
            market_profile_name=(
                prepared_workflow.resolved_configuration.market_profile.selected_preset.preset_name
                if (
                    prepared_workflow.resolved_configuration.market_profile is not None
                    and prepared_workflow.resolved_configuration.market_profile.selected_preset is not None
                )
                else None
            ),
            known_news_preset_name=(
                prepared_workflow.resolved_configuration.known_news.selected_preset.preset_name
                if prepared_workflow.resolved_configuration.known_news.selected_preset is not None
                else None
            ),
            known_news_recommended_name=(
                prepared_workflow.resolved_configuration.known_news.recommended_reference.preset_name
                if prepared_workflow.resolved_configuration.known_news.recommended_reference is not None
                else None
            ),
            known_news_recommendation_applied=(
                prepared_workflow.resolved_configuration.known_news.recommendation_applied
            ),
            session_source_kind=prepared_workflow.resolved_configuration.session.source_kind.value,
            known_news_source_kind=prepared_workflow.resolved_configuration.known_news.source_kind.value,
        )


__all__ = ["JaniformEstimator"]
