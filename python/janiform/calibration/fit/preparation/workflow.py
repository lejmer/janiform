from __future__ import annotations

from datetime import datetime
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any, Callable, Optional

from ...._workflow_progress import AbstractProgress
from ..inference import (
    FitInference,
    column_overrides_from_mapping,
    infer_fit_inputs,
    normalization_overrides_from_policy,
)
from .contracts import (
    DEFAULT_KNOWN_NEWS_COLUMNS,
    FitWorkflowEstimator,
    PreparedFitWorkflow,
)
from ...market_data.contracts import (
    CalibrationFlatCarryInput,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationMarketDataColumnMapping,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationTickDataColumnMapping,
    CalibrationSessionConfigurationInput,
)
from ...market_data.request import MarketDataCalibrationRequest
from ...market_data.translation import (
    NormalizedMarketDataSource,
    normalize_market_data_source,
    resolve_market_data_session,
    translate_market_data_request_to_prepared_calibration_from_phases,
)
from ...market_data.workflow import _prepare_market_data_calibration_from_translation_result, PreparedMarketDataCalibration
from ...presets.resolution import ResolvedPresetWorkflowConfiguration, resolve_preset_workflow_configuration
from ...sessions.contracts import MarketDataSessionResolution
from .configuration import build_known_news_input, build_session_configuration_input, resolved_configuration_requires_materialization


def fit_progress_total_steps(estimator: FitWorkflowEstimator) -> int:
    if estimator._requires_session_inference():
        return 9

    return 8


def build_market_data_request(
    estimator: FitWorkflowEstimator,
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
    session_configuration = build_session_configuration_input(
        resolved_configuration.session,
        working_directory=working_directory,
    )
    known_news_input = build_known_news_input(
        resolved_configuration.known_news.configuration,
        working_directory=working_directory,
    )

    column_mapping = CalibrationMarketDataColumnMapping(
        tick_data=tick_data_column_mapping,
        option_surface=estimator._option_surface_column_mapping,
        known_news=(DEFAULT_KNOWN_NEWS_COLUMNS if known_news_input is not None else None),
    )

    return MarketDataCalibrationRequest(
        schema_version="calibration.market_data.request.v1",
        run_id=run_id,
        tick_data_source=tick_data_source,
        session_configuration=session_configuration,
        column_mapping=column_mapping,
        workflow_scope=workflow_scope,
        normalization_policy=normalization_policy,
        output_preferences=estimator._output_preferences,
        optimizer_settings_by_stage=estimator._optimizer_settings_by_stage,
        preprocessing_settings=estimator._preprocessing_settings,
        option_surface_source=option_surface_source,
        option_surface_valuation_time_utc=option_surface_valuation_time_utc,
        carry=carry,
        known_news_inputs=known_news_input,
        mean_reversion=(
            None
            if workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY
            else (
                estimator._mean_reversion
                if estimator._mean_reversion is not None
                else estimator._default_endogenous_mean_reversion_configuration()
            )
        ),
    )


def infer_session_resolution(
    estimator: FitWorkflowEstimator,
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
    if not estimator._requires_session_inference():
        return None

    exchange_code = estimator._resolve_exchange_code_for_inference()
    request = MarketDataCalibrationRequest(
        schema_version="calibration.market_data.request.v1",
        run_id=run_id,
        tick_data_source=tick_data_source,
        session_configuration=CalibrationSessionConfigurationInput(
            exchange_code=exchange_code,
            session_timezone=estimator._session_timezone,
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=tick_data_column_mapping,
            option_surface=estimator._option_surface_column_mapping,
        ),
        workflow_scope=workflow_scope,
        normalization_policy=normalization_policy,
        output_preferences=estimator._output_preferences,
        optimizer_settings_by_stage=estimator._optimizer_settings_by_stage,
        preprocessing_settings=estimator._preprocessing_settings,
        option_surface_source=option_surface_source,
        option_surface_valuation_time_utc=option_surface_valuation_time_utc,
        carry=carry,
    )

    progress.update(detail="resolving session from market data")

    return resolve_market_data_session(
        session_configuration=request.session_configuration,
        normalized_market_data=normalized_market_data,
    )


def prepare_fit_workflow(
    estimator: FitWorkflowEstimator,
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
    temporary_directory_factory: Callable[..., TemporaryDirectory[str]] = TemporaryDirectory,
    normalize_market_data_source_method: Callable[..., NormalizedMarketDataSource] = normalize_market_data_source,
    resolve_market_data_session_method: Callable[..., MarketDataSessionResolution] = resolve_market_data_session,
    resolve_preset_workflow_configuration_method: Callable[..., ResolvedPresetWorkflowConfiguration] = resolve_preset_workflow_configuration,
    translate_market_data_request_to_prepared_calibration_from_phases_method: Callable[..., Any] = translate_market_data_request_to_prepared_calibration_from_phases,
    prepare_market_data_calibration_from_translation_result_method: Callable[..., PreparedMarketDataCalibration] = _prepare_market_data_calibration_from_translation_result,
) -> PreparedFitWorkflow:
    effective_progress = estimator._null_progress() if progress is None else progress
    workflow_scope = estimator._effective_workflow_scope(option_surface_path=option_surface_path)
    resolved_market_profile_configuration = estimator._resolve_market_profile_configuration()

    input_inference = infer_fit_inputs(
        tick_data_path=Path(tick_data_path),
        tick_data_format=tick_data_format,
        tick_data_provenance_label=tick_data_provenance_label,
        workflow_scope=workflow_scope,
        run_id=run_id,
        column_overrides=column_overrides_from_mapping(estimator._tick_data_column_mapping),
        normalization_overrides=normalization_overrides_from_policy(estimator._normalization_policy),
        market_profile_configuration=resolved_market_profile_configuration,
    )

    option_surface_source = (
        CalibrationInputFileReference(
            path=str(Path(option_surface_path)),
            file_format=option_surface_format,
            provenance_label=option_surface_provenance_label,
        )
        if option_surface_path is not None
        else None
    )

    temporary_directory: Optional[TemporaryDirectory[str]] = None
    try:
        effective_progress.update(detail="normalize source")
        normalized_market_data = normalize_market_data_source_method(
            tick_data_source=input_inference.tick_data_source,
            tick_data_column_mapping=input_inference.tick_data_column_mapping,
            normalization_policy=input_inference.normalization_policy,
            require_hawkes=(workflow_scope != CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY),
        )

        effective_progress.update(detail="resolve session")
        inferred_session_resolution = infer_session_resolution(
            estimator,
            run_id=input_inference.run_id,
            workflow_scope=workflow_scope,
            tick_data_source=input_inference.tick_data_source,
            option_surface_source=option_surface_source,
            option_surface_valuation_time_utc=option_surface_valuation_time_utc,
            carry=carry,
            tick_data_column_mapping=input_inference.tick_data_column_mapping,
            normalization_policy=input_inference.normalization_policy,
            normalized_market_data=normalized_market_data,
            progress=effective_progress,
        )

        effective_progress.update(detail="resolve presets")
        resolved_configuration = resolve_preset_workflow_configuration_method(
            registry=estimator._registry,
            market_profile_reference=estimator._market_profile_reference,
            session_reference=estimator._effective_session_reference(),
            known_news_reference=estimator._known_news_reference,
            inferred_session_resolution=inferred_session_resolution,
        )

        if resolved_configuration_requires_materialization(resolved_configuration):
            temporary_directory = temporary_directory_factory(prefix="janiform-fit-")
            working_directory = Path(temporary_directory.name)
        else:
            working_directory: Optional[Path] = None

        effective_progress.update(detail="build market-data request")
        request = build_market_data_request(
            estimator,
            run_id=input_inference.run_id,
            workflow_scope=workflow_scope,
            tick_data_source=input_inference.tick_data_source,
            option_surface_source=option_surface_source,
            option_surface_valuation_time_utc=option_surface_valuation_time_utc,
            carry=carry,
            resolved_configuration=resolved_configuration,
            tick_data_column_mapping=input_inference.tick_data_column_mapping,
            normalization_policy=input_inference.normalization_policy,
            working_directory=working_directory,
        )

        effective_progress.update(detail="resolve session")
        session_resolution = resolve_market_data_session_method(
            session_configuration=request.session_configuration,
            normalized_market_data=normalized_market_data,
        )

        effective_progress.update(detail="prepare stage inputs")
        translation_result = translate_market_data_request_to_prepared_calibration_from_phases_method(
            request=request,
            normalized_market_data=normalized_market_data,
            session_resolution=session_resolution,
            cpu_count=estimator._cpu_count,
        )

        prepared = prepare_market_data_calibration_from_translation_result_method(
            request=request,
            translation_result=translation_result,
        )
    except Exception:
        if temporary_directory is not None:
            temporary_directory.cleanup()

        raise

    return PreparedFitWorkflow(
        run_id=input_inference.run_id,
        workflow_scope=workflow_scope,
        prepared=prepared,
        resolved_configuration=resolved_configuration,
        input_inference=input_inference,
        fit_warning=translation_result.fit_warning,
        reference_price=translation_result.reference_price,
        temporary_directory=temporary_directory,
    )
