from __future__ import annotations

import os
from collections.abc import Callable, Mapping
from contextlib import AbstractContextManager, nullcontext, redirect_stdout
from datetime import datetime
from io import StringIO
from math import isfinite
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any, Optional

from ._execution import fit_with_execution_configuration
from ._inference import FitInference
from ._preparation.configuration import (
    coerce_known_news_reference,
    coerce_market_profile_reference,
    coerce_session_reference,
    exchange_code_from_market_profile_reference,
    exchange_code_from_session_reference,
    resolve_market_profile_configuration,
)
from ._preparation.contracts import (
    FitExecutionConfiguration,
    FitPreparationConfiguration,
    PreparedFitWorkflow,
)
from ._runtime.workflow import prepare_fit_workflow as prepare_native_fit_workflow
from ..diagnostics import CalibrationDiagnosticsRequest
from ..effort import CalibrationEffortOptions, STANDARD_CALIBRATION_EFFORT
from .result import JaniformFitResult
from ...model import JaniformModel
from ...pricing import PricingIntervalSchedulePolicy, default_pricing_interval_schedule_policy
from ..errors import calibration_error
from ..market_data.contracts import (
    CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
    CalibrationExecutionMode,
    CalibrationFlatCarryInput,
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataPreprocessingSettings,
    CalibrationMarketDataWorkflowScope,
    CalibrationMeanReversionConfiguration,
    CalibrationOptionSurfaceColumnMapping,
    CalibrationTickDataColumnMapping,
)
from ..market_data.request import validate_market_data_calibration_request
from ..market_data.workflow import (
    MarketDataCalibrationPreparationDiagnostics,
    MarketDataCalibrationPreparationProvenance,
    PreparedMarketDataCalibration,
)
from ..presets.references import (
    KnownNewsPresetReference,
    MarketProfileExplicitConfiguration,
    MarketProfileReference,
    PresetReferenceUsageMode,
    SessionPresetReference,
)
from ..presets.registry import PresetRegistry
from ..presets.resolution import ResolvedPresetWorkflowConfiguration
from ..sessions.contracts import MarketDataSessionResolution
from ..staged import CalibrationResumeSettings, CalibrationStageSolverSettings
from ..staged.request import StagedCalibrationRequest

_DEFAULT_OUTPUT_PREFERENCES = CalibrationMarketDataOutputPreferences(
    execution_mode=CalibrationExecutionMode.MEMORY,
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
    _calibration_effort: CalibrationEffortOptions
    _treat_calibration_warnings_as_errors: bool
    _joint_refinement_option_refinement_candidate_count_override: Optional[int]
    _joint_refinement_option_refinement_wall_clock_budget_millis: Optional[int]
    _joint_refinement_audit_option_pricing_enabled: bool
    _diagnostics: CalibrationDiagnosticsRequest
    _stylized_fact_huber_cutoff: Optional[float]
    _stylized_fact_admissibility_z_threshold: Optional[float]
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
        calibration_effort: CalibrationEffortOptions = STANDARD_CALIBRATION_EFFORT,
        treat_calibration_warnings_as_errors: bool = False,
        joint_refinement_option_refinement_candidate_count: Optional[int] = None,
        joint_refinement_option_refinement_wall_clock_budget_millis: Optional[int] = None,
        joint_refinement_audit_option_pricing: bool = False,
        diagnostics: Optional[CalibrationDiagnosticsRequest] = None,
        stylized_fact_huber_cutoff: Optional[float] = None,
        stylized_fact_admissibility_z_threshold: Optional[float] = None,
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

        if not isinstance(calibration_effort, CalibrationEffortOptions):
            raise calibration_error(
                "calibration_effort",
                "must be a CalibrationEffortOptions instance",
            )

        if (
            joint_refinement_option_refinement_candidate_count is not None
            and joint_refinement_option_refinement_candidate_count <= 0
        ):
            raise calibration_error(
                "joint_refinement_option_refinement_candidate_count",
                "must be strictly positive when provided",
            )

        if (
            joint_refinement_option_refinement_wall_clock_budget_millis is not None
            and joint_refinement_option_refinement_wall_clock_budget_millis <= 0
        ):
            raise calibration_error(
                "joint_refinement_option_refinement_wall_clock_budget_millis",
                "must be strictly positive when provided",
            )

        if (
            stylized_fact_huber_cutoff is not None
            and (not isfinite(stylized_fact_huber_cutoff) or stylized_fact_huber_cutoff <= 0.0)
        ):
            raise calibration_error(
                "stylized_fact_huber_cutoff",
                "must be finite and strictly positive when provided",
            )

        if (
            stylized_fact_admissibility_z_threshold is not None
            and (
                not isfinite(stylized_fact_admissibility_z_threshold)
                or stylized_fact_admissibility_z_threshold <= 0.0
            )
        ):
            raise calibration_error(
                "stylized_fact_admissibility_z_threshold",
                "must be finite and strictly positive when provided",
            )

        if diagnostics is not None and not isinstance(diagnostics, CalibrationDiagnosticsRequest):
            raise calibration_error(
                "diagnostics",
                "must be a CalibrationDiagnosticsRequest when provided",
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
        self._calibration_effort = calibration_effort
        self._treat_calibration_warnings_as_errors = treat_calibration_warnings_as_errors
        self._joint_refinement_option_refinement_candidate_count_override = (
            joint_refinement_option_refinement_candidate_count
        )
        self._joint_refinement_option_refinement_wall_clock_budget_millis = (
            joint_refinement_option_refinement_wall_clock_budget_millis
        )
        self._joint_refinement_audit_option_pricing_enabled = (
            joint_refinement_audit_option_pricing
        )
        self._diagnostics = CalibrationDiagnosticsRequest() if diagnostics is None else diagnostics
        self._stylized_fact_huber_cutoff = stylized_fact_huber_cutoff
        self._stylized_fact_admissibility_z_threshold = (
            stylized_fact_admissibility_z_threshold
        )
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
        progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
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
            progress_callback=progress_callback,
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
        progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
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
            progress_callback=progress_callback,
        )

        return JaniformFitResult(
            model=model,
            report=fit_report,
        )

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
            calibration_effort=self._calibration_effort,
            treat_calibration_warnings_as_errors=(
                self._treat_calibration_warnings_as_errors
            ),
            joint_refinement_option_refinement_candidate_count_override=(
                self._joint_refinement_option_refinement_candidate_count_override
            ),
            joint_refinement_option_refinement_wall_clock_budget_millis=(
                self._joint_refinement_option_refinement_wall_clock_budget_millis
            ),
            joint_refinement_audit_option_pricing_enabled=(
                self._joint_refinement_audit_option_pricing_enabled
            ),
            diagnostics=self._diagnostics,
            stylized_fact_huber_cutoff=self._stylized_fact_huber_cutoff,
            stylized_fact_admissibility_z_threshold=(
                self._stylized_fact_admissibility_z_threshold
            ),
            default_endogenous_mean_reversion_configuration=(
                self._default_endogenous_mean_reversion_configuration()
            ),
            market_profile_reference=self._market_profile_reference,
            known_news_reference=self._known_news_reference,
            resolved_market_profile_configuration=resolved_market_profile_configuration,
            effective_session_reference=self._effective_session_reference(),
            requires_session_inference=requires_session_inference,
            exchange_code_for_inference=(
                self._optional_exchange_code_for_inference() if requires_session_inference else None
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

        return FitExecutionConfiguration(
            silent=effective_silent,
            preparation=preparation_configuration,
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
    ) -> PreparedFitWorkflow:
        temporary_directory = TemporaryDirectory(prefix="janiform-fit-")

        try:
            raw_prepared = prepare_native_fit_workflow(
                self._build_execution_configuration(
                    silent=self._silent,
                    pricing_interval_schedule_policy=None,
                ),
                working_directory=temporary_directory.name,
                run_id=run_id,
                tick_data_path=tick_data_path,
                tick_data_format=tick_data_format,
                tick_data_provenance_label=tick_data_provenance_label,
                option_surface_path=option_surface_path,
                option_surface_format=option_surface_format,
                option_surface_provenance_label=option_surface_provenance_label,
                option_surface_valuation_time_utc=option_surface_valuation_time_utc,
                carry=carry,
            )

            return _prepared_fit_workflow_from_native(
                raw_prepared,
                temporary_directory=temporary_directory,
            )
        except Exception:
            temporary_directory.cleanup()
            raise

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

    def _requires_session_inference(self) -> bool:
        if self._session_reference is not None:
            return self._session_reference.usage_mode == PresetReferenceUsageMode.AUTOMATIC

        market_profile_configuration = self._resolve_market_profile_configuration()
        if market_profile_configuration is None:
            return True

        return (
            market_profile_configuration.session_reference.usage_mode
            == PresetReferenceUsageMode.AUTOMATIC
        )

    def _resolve_market_profile_configuration(self) -> Optional[MarketProfileExplicitConfiguration]:
        if self._market_profile_reference is None:
            return None

        return resolve_market_profile_configuration(
            self._market_profile_reference,
            self._registry,
        )

    def _resolve_exchange_code_for_inference(self) -> str:
        exchange_code = self._optional_exchange_code_for_inference()
        if exchange_code is not None:
            return exchange_code

        raise calibration_error(
            "exchange_code",
            (
                "is required for automatic session inference when no session preset or market "
                "profile provides the exchange baseline"
            ),
        )

    def _optional_exchange_code_for_inference(self) -> Optional[str]:
        if self._exchange_code is not None:
            return self._exchange_code

        if self._session_reference is not None:
            return exchange_code_from_session_reference(self._session_reference, self._registry)

        if self._market_profile_reference is not None:
            return exchange_code_from_market_profile_reference(
                self._market_profile_reference,
                self._registry,
            )

        return None

    def _stdout_suppression_context(
        self,
        *,
        effective_silent: Optional[bool] = None,
    ) -> AbstractContextManager[Optional[StringIO]]:
        if self._silent if effective_silent is None else effective_silent:
            return redirect_stdout(StringIO())

        return nullcontext()

    def _default_endogenous_mean_reversion_configuration(
        self,
    ) -> CalibrationMeanReversionConfiguration:
        return _DEFAULT_ENDOGENOUS_MEAN_REVERSION_CONFIGURATION


def _prepared_fit_workflow_from_native(
    raw_prepared: dict[str, Any],
    *,
    temporary_directory: TemporaryDirectory[str],
) -> PreparedFitWorkflow:
    raw_preparation_payload = _required_mapping(raw_prepared, "preparation_payload")
    request = validate_market_data_calibration_request(raw_preparation_payload["request"])
    staged_request = StagedCalibrationRequest.from_mapping(
        _required_mapping(raw_prepared, "staged_request"),
        field_path_prefix="staged_request",
    )

    prepared = PreparedMarketDataCalibration(
        schema_version=CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
        request=request,
        staged_request=staged_request,
        diagnostics=MarketDataCalibrationPreparationDiagnostics.from_mapping(
            raw_preparation_payload["diagnostics"]
        ),
        provenance=MarketDataCalibrationPreparationProvenance.from_mapping(
            raw_preparation_payload["provenance"]
        ),
        session_resolution=MarketDataSessionResolution.from_mapping(
            raw_preparation_payload["session_resolution"],
            field_path="session_resolution",
        ),
        fit_warning=raw_preparation_payload.get("fit_warning"),
        reference_price=raw_preparation_payload.get("reference_price"),
        _prepared_handle=raw_prepared["prepared_handle"],
    )

    resolved_configuration = ResolvedPresetWorkflowConfiguration.from_mapping(
        raw_preparation_payload["resolved_configuration"],
        field_path="resolved_configuration",
    )
    input_inference = FitInference.from_mapping(raw_preparation_payload["input_inference"])

    return PreparedFitWorkflow(
        run_id=request.run_id,
        workflow_scope=request.workflow_scope,
        prepared=prepared,
        resolved_configuration=resolved_configuration,
        input_inference=input_inference,
        fit_warning=prepared.fit_warning,
        reference_price=prepared.reference_price,
        temporary_directory=temporary_directory,
    )


def _required_mapping(payload: dict[str, Any], field_name: str) -> dict[str, Any]:
    raw_value = payload.get(field_name)
    if not isinstance(raw_value, dict):
        raise calibration_error(field_name, "must be a mapping")

    return raw_value


__all__ = ["JaniformEstimator"]
