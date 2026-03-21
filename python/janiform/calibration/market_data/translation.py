import os

from dataclasses import dataclass
from datetime import datetime
from types import ModuleType
from typing import Any, Optional

from .contracts import (
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
)
from .ingestion import (
    LoadedMarketDataCalibrationInputs,
    LoadedMarketDataSourceSummary,
    LoadedTickDataSource,
    _LoadedMarketDataCalibrationInputsResult,
    _TradeSignResolutionDiagnostics,
    _load_market_data_calibration_inputs,
)
from .request import MarketDataCalibrationRequest
from .._errors import (
    CALIBRATION_ERROR_FACTORY,
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    CalibrationValidationError,
    _calibration_error,
    _calibration_integration_error,
)
from ..sessions.contracts import MarketDataSessionResolution
from ..sessions.resolution import (
    _resolve_explicit_session_definition,
    resolve_market_data_session_configuration,
)
from ..staged.contracts import (
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
    CalibrationExecutionMode,
    CalibrationOptimizerSettings,
    CalibrationRunDataInputs,
    CalibrationRunSeedSettings,
    CalibrationStage,
    CalibrationStageSolverSettings,
)
from ..staged.inputs import (
    CalibrationHawkesStageCandidateInput,
    CalibrationReturnComponentsStageCandidateGenerationInput,
    CalibrationSeasonalityStageExecutionInput,
    CalibrationStagedExecutionRuntimeResources,
    StagedCalibrationExecutionInput,
)
from ..staged.request import StagedCalibrationRequest
from ..._contracts import serialize_deterministic_json
from ..._native_api import native_exception_matches, native_module
from ..._timestamps import datetime_to_utc_nanos
from ...calendar import CalendarRegistry


CALIBRATION_MARKET_DATA_TRANSLATION_ERROR_ID = "calibration.validation.market_data_translation"
CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT = "market_data_translation"


@dataclass(frozen=True, slots=True)
class MarketDataTranslationDiagnostics:
    workflow_scope: CalibrationMarketDataWorkflowScope
    selected_stages: tuple[CalibrationStage, ...]
    executed_preprocessing_steps: tuple[str, ...]
    tick_record_count: int
    option_surface_record_count: int
    known_news_event_count: int
    known_news_events_outside_source_window_count: int
    session_open_tick_count: int
    session_closed_tick_count: int
    known_news_open_event_count: int
    known_news_closed_event_count: int
    seasonality_overlay_count: int
    hawkes_candidate_count: int
    known_news_return_sample_count: int
    intraday_jump_candidate_count: int
    close_open_boundary_observation_count: int
    material_close_open_gap_count: int


@dataclass(frozen=True, slots=True)
class MarketDataTranslationProvenance:
    tick_data_summary: LoadedMarketDataSourceSummary
    option_surface_summary: Optional[LoadedMarketDataSourceSummary]
    known_news_summary: Optional[LoadedMarketDataSourceSummary]
    normalization_policy: CalibrationMarketDataNormalizationPolicy
    workflow_scope: CalibrationMarketDataWorkflowScope
    selected_stages: tuple[CalibrationStage, ...]
    session_definition_path: Optional[str]
    derived_trade_events_data_reference: str
    derived_option_surface_data_reference: Optional[str]
    derivation_policies: tuple[str, ...]
    deterministic_defaults_applied: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class MarketDataTranslationResult:
    calendar_registry: CalendarRegistry
    session_resolution: MarketDataSessionResolution
    execution_input: StagedCalibrationExecutionInput
    diagnostics: MarketDataTranslationDiagnostics
    provenance: MarketDataTranslationProvenance
    fit_warning: Optional[str]
    reference_price: Optional[float]


@dataclass(frozen=True, slots=True)
class _MarketDataPreprocessingOutput:
    seasonality_stage_input: CalibrationSeasonalityStageExecutionInput
    hawkes_candidate_inputs: Optional[tuple[CalibrationHawkesStageCandidateInput, ...]]
    return_components_candidate_generation_input: Optional[
        CalibrationReturnComponentsStageCandidateGenerationInput
    ]
    session_open_tick_count: int
    session_closed_tick_count: int
    known_news_event_count: int
    known_news_events_outside_source_window_count: int
    known_news_open_event_count: int
    known_news_closed_event_count: int
    material_close_open_gap_count: int


@dataclass(frozen=True, slots=True)
class _MarketDataSourceSummary:
    path: str
    file_format: str
    provenance_label: str
    source_row_count: int
    retained_row_count: int
    dropped_duplicate_row_count: int
    sort_records_by_timestamp: bool
    applied_timestamp_timezone: Optional[str]
    canonical_value_field: Optional[str]
    value_mapping_policy: Optional[str]


@dataclass(frozen=True, slots=True)
class _MarketDataLoadingOutput:
    preprocessing_output: _MarketDataPreprocessingOutput
    tick_data_summary: _MarketDataSourceSummary
    known_news_summary: Optional[_MarketDataSourceSummary]
    fit_warning: Optional[str]
    reference_price: Optional[float]


def translate_market_data_request_to_staged_execution_input(
    request: MarketDataCalibrationRequest,
    *,
    cpu_count: int | None = None,
) -> MarketDataTranslationResult:
    if not isinstance(request, MarketDataCalibrationRequest):
        raise _translation_error(
            "request",
            "must be a MarketDataCalibrationRequest",
        )

    normalized_request = MarketDataCalibrationRequest.from_mapping(request.to_payload())
    if _can_load_market_data_from_source_files(normalized_request):
        return _translate_market_data_request_from_source_files(
            normalized_request=normalized_request,
            cpu_count=cpu_count,
        )

    loaded_inputs_result = _load_market_data_translation_inputs(normalized_request)

    return _translate_loaded_market_data_request_to_staged_execution_input(
        normalized_request=normalized_request,
        loaded_inputs_result=loaded_inputs_result,
        cpu_count=cpu_count,
    )


def _load_market_data_translation_inputs(
    normalized_request: MarketDataCalibrationRequest,
) -> _LoadedMarketDataCalibrationInputsResult:
    selected_stages = _selected_stages_for_scope(normalized_request.workflow_scope)

    return _load_market_data_calibration_inputs(
        normalized_request,
        resolve_unsigned_trade_signs=CalibrationStage.HAWKES in selected_stages,
    )


def _translate_loaded_market_data_request_to_staged_execution_input(
    *,
    normalized_request: MarketDataCalibrationRequest,
    loaded_inputs_result: _LoadedMarketDataCalibrationInputsResult,
    cpu_count: int | None = None,
) -> MarketDataTranslationResult:
    selected_stages = _selected_stages_for_scope(normalized_request.workflow_scope)
    loaded_inputs = loaded_inputs_result.inputs

    _validate_tick_records_present(loaded_inputs.tick_data)
    _validate_tick_record_order(
        loaded_inputs.tick_data,
        normalization_policy=normalized_request.normalization_policy,
    )

    resolved_session = resolve_market_data_session_configuration(
        normalized_request,
        loaded_inputs.tick_data,
    )
    calendar_registry = resolved_session.calendar_registry

    trade_events_data_reference = _build_dataset_reference(
        normalized_request.run_id,
        "ticks",
        loaded_inputs.tick_data.summary.provenance_label,
    )

    native_preprocessing_output = _preprocess_market_data_translation_inputs(
        loaded_inputs=loaded_inputs,
        exchange_code=normalized_request.session_configuration.exchange_code,
        trade_events_data_reference=trade_events_data_reference,
        selected_stages=selected_stages,
        directional_label_diagnostics=(
            loaded_inputs_result.trade_sign_resolution_diagnostics
        ),
        calendar_registry=calendar_registry,
    )

    option_surface_data_reference = (
        _build_dataset_reference(
            normalized_request.run_id,
            "options",
            loaded_inputs.option_surface.summary.provenance_label,
        )
        if loaded_inputs.option_surface is not None
        else None
    )

    execution_input = StagedCalibrationExecutionInput(
        schema_version=CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
        request=StagedCalibrationRequest(
            run_id=normalized_request.run_id,
            data_inputs=CalibrationRunDataInputs(
                exchange_code=normalized_request.session_configuration.exchange_code,
                trade_events_data_reference=trade_events_data_reference,
                option_surface_data_reference=option_surface_data_reference,
            ),
            execution_mode=normalized_request.output_preferences.execution_mode,
            selected_stages=selected_stages,
            seed_settings=CalibrationRunSeedSettings(
                global_seed=_stable_seed_from_run_id(normalized_request.run_id),
            ),
            optimizer_settings_by_stage=tuple(
                _default_solver_settings(stage, cpu_count=cpu_count)
                for stage in selected_stages
            ),
        ),
        seasonality_stage_input=(
            native_preprocessing_output.seasonality_stage_input
            if CalibrationStage.SEASONALITY in selected_stages
            else None
        ),
        hawkes_candidate_inputs=native_preprocessing_output.hawkes_candidate_inputs,
        return_components_candidate_generation_input=(
            native_preprocessing_output.return_components_candidate_generation_input
        ),
        runtime_resources=_runtime_resources_from_request(normalized_request),
    )

    return MarketDataTranslationResult(
        calendar_registry=calendar_registry,
        session_resolution=resolved_session.session_resolution,
        execution_input=execution_input,
        diagnostics=MarketDataTranslationDiagnostics(
            workflow_scope=normalized_request.workflow_scope,
            selected_stages=selected_stages,
            executed_preprocessing_steps=_executed_preprocessing_steps(
                workflow_scope=normalized_request.workflow_scope,
                option_surface_present=loaded_inputs.option_surface is not None,
                known_news_present=loaded_inputs.known_news is not None,
            ),
            tick_record_count=len(loaded_inputs.tick_data.records),
            option_surface_record_count=(
                len(loaded_inputs.option_surface.records)
                if loaded_inputs.option_surface is not None
                else 0
            ),
            known_news_event_count=native_preprocessing_output.known_news_event_count,
            known_news_events_outside_source_window_count=(
                native_preprocessing_output.known_news_events_outside_source_window_count
            ),
            session_open_tick_count=native_preprocessing_output.session_open_tick_count,
            session_closed_tick_count=native_preprocessing_output.session_closed_tick_count,
            known_news_open_event_count=(
                native_preprocessing_output.known_news_open_event_count
            ),
            known_news_closed_event_count=(
                native_preprocessing_output.known_news_closed_event_count
            ),
            seasonality_overlay_count=len(
                native_preprocessing_output.seasonality_stage_input.scheduled_event_overlays
            ),
            hawkes_candidate_count=len(
                native_preprocessing_output.hawkes_candidate_inputs or ()
            ),
            known_news_return_sample_count=(
                len(
                    native_preprocessing_output.return_components_candidate_generation_input.realized_known_news_return_samples
                )
                if native_preprocessing_output.return_components_candidate_generation_input
                is not None
                else 0
            ),
            intraday_jump_candidate_count=(
                len(
                    native_preprocessing_output.return_components_candidate_generation_input.detected_intraday_jump_candidates
                )
                if native_preprocessing_output.return_components_candidate_generation_input
                is not None
                else 0
            ),
            close_open_boundary_observation_count=(
                len(
                    native_preprocessing_output.return_components_candidate_generation_input.observed_close_open_boundary_observations
                )
                if native_preprocessing_output.return_components_candidate_generation_input
                is not None
                else 0
            ),
            material_close_open_gap_count=(
                native_preprocessing_output.material_close_open_gap_count
            ),
        ),
        provenance=MarketDataTranslationProvenance(
            tick_data_summary=loaded_inputs.tick_data.summary,
            option_surface_summary=(
                loaded_inputs.option_surface.summary
                if loaded_inputs.option_surface is not None
                else None
            ),
            known_news_summary=(
                loaded_inputs.known_news.summary
                if loaded_inputs.known_news is not None
                else None
            ),
            normalization_policy=normalized_request.normalization_policy,
            workflow_scope=normalized_request.workflow_scope,
            selected_stages=selected_stages,
            session_definition_path=resolved_session.session_definition_path,
            derived_trade_events_data_reference=trade_events_data_reference,
            derived_option_surface_data_reference=option_surface_data_reference,
            derivation_policies=(
                resolved_session.session_resolution.selected_policy_id,
                "native_market_data_translation_preprocessing.rust.v1",
                "known_news_outside_tick_window_are_filtered.v1",
                "known_news_overlays_are_point_anchors_with_zero_log_adjustment.v1",
                "unsigned_trade_direction_labels_follow_tick_price_change_with_flat_move_carryover.v1",
                "hawkes_search_family_uses_unsigned_open_session_activity_targets_and_model_implied_fit_terms.v1",
                "surprise_jump_candidates_use_data_driven_material_open_session_return_thresholds_outside_known_news_windows.v1",
                "close_open_boundaries_are_resolved_from_calendar_state_transitions.v1",
                "continuous_nig_fit_uses_filtered_observed_open_session_returns_and_stage_search.v1",
            ),
            deterministic_defaults_applied=(
                "seasonality.session_filter=open_only",
                "seasonality.sparse_hour_event_threshold=1",
                "seasonality.intraday.minutes_per_bin=1",
                "seasonality.intraday.smoothing_kernel=gaussian",
                "seasonality.intraday.smoothing_bandwidth_bins=3.0",
                "seasonality.day_of_week.minimum_bucket_events=1",
                "seasonality.day_of_year.minimum_bucket_events=1",
                "known_news_overlay.log_adjustment=0.0",
                "close_open_gap_detection_policy.material_gap_absolute_return_tolerance=3x_median_abs_intraday_return",
                "intraday_jump_detection.material_threshold=4x_median_abs_open_return",
            ),
        ),
        fit_warning=_detect_interval_data_warning(loaded_inputs.tick_data.records),
        reference_price=_extract_reference_price(loaded_inputs.tick_data.records),
    )


def _executed_preprocessing_steps(
    *,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    option_surface_present: bool,
    known_news_present: bool,
) -> tuple[str, ...]:
    preprocessing_steps: list[str] = [
        "source_loading",
        "column_mapping_validation",
        "timestamp_timezone_normalization",
        "duplicate_record_resolution",
        "timestamp_sorting",
        "session_state_resolution",
        "workflow_scope_selection",
    ]

    if known_news_present:
        preprocessing_steps.extend(
            [
                "known_news_window_filtering",
                "known_news_overlay_derivation",
            ]
        )

    if workflow_scope != CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY:
        preprocessing_steps.append("hawkes_candidate_generation")
        preprocessing_steps.append("return_components_candidate_generation")

    if option_surface_present:
        preprocessing_steps.append("option_surface_quote_normalization")

    return tuple(preprocessing_steps)


def _translation_error(
    field_path: str,
    message: str,
    *,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> Exception:
    return _calibration_error(
        field_path,
        message,
        entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
        error_id=CALIBRATION_MARKET_DATA_TRANSLATION_ERROR_ID,
        context=context,
    )


def _can_load_market_data_from_source_files(request: MarketDataCalibrationRequest) -> bool:
    if request.workflow_scope == CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE:
        return False
    if request.option_surface_source is not None or request.column_mapping.option_surface is not None:
        return False
    if request.session_configuration.session_definition_source is None:
        return False
    if request.tick_data_source.file_format not in (
        CalibrationInputFileFormat.CSV,
        CalibrationInputFileFormat.PARQUET,
    ):
        return False
    if request.known_news_inputs is None:
        return True

    return (
        request.known_news_inputs.calendar_source.file_format
        in (CalibrationInputFileFormat.CSV, CalibrationInputFileFormat.PARQUET)
        and request.column_mapping.known_news is not None
    )


def _translate_market_data_request_from_source_files(
    *,
    normalized_request: MarketDataCalibrationRequest,
    cpu_count: int | None = None,
) -> MarketDataTranslationResult:
    selected_stages = _selected_stages_for_scope(normalized_request.workflow_scope)
    session_source = normalized_request.session_configuration.session_definition_source
    if session_source is None:
        raise _translation_error(
            "session_configuration.session_definition_source",
            "is required for the market-data loading path",
        )

    resolved_session = _resolve_explicit_session_definition(
        exchange_code=normalized_request.session_configuration.exchange_code,
        session_source=session_source,
        configured_session_timezone=normalized_request.session_configuration.session_timezone,
    )
    trade_events_data_reference = _build_dataset_reference(
        normalized_request.run_id,
        "ticks",
        normalized_request.tick_data_source.provenance_label,
    )
    native_loading_output = _load_market_data_translation_inputs_from_source_files(
        normalized_request=normalized_request,
        selected_stages=selected_stages,
        trade_events_data_reference=trade_events_data_reference,
        calendar_registry=resolved_session.calendar_registry,
    )
    execution_input = StagedCalibrationExecutionInput(
        schema_version=CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
        request=StagedCalibrationRequest(
            run_id=normalized_request.run_id,
            data_inputs=CalibrationRunDataInputs(
                exchange_code=normalized_request.session_configuration.exchange_code,
                trade_events_data_reference=trade_events_data_reference,
                option_surface_data_reference=None,
            ),
            execution_mode=normalized_request.output_preferences.execution_mode,
            selected_stages=selected_stages,
            seed_settings=CalibrationRunSeedSettings(
                global_seed=_stable_seed_from_run_id(normalized_request.run_id),
            ),
            optimizer_settings_by_stage=tuple(
                _default_solver_settings(stage, cpu_count=cpu_count)
                for stage in selected_stages
            ),
        ),
        seasonality_stage_input=(
            native_loading_output.preprocessing_output.seasonality_stage_input
            if CalibrationStage.SEASONALITY in selected_stages
            else None
        ),
        hawkes_candidate_inputs=native_loading_output.preprocessing_output.hawkes_candidate_inputs,
        return_components_candidate_generation_input=(
            native_loading_output.preprocessing_output.return_components_candidate_generation_input
        ),
        runtime_resources=_runtime_resources_from_request(normalized_request),
    )

    return MarketDataTranslationResult(
        calendar_registry=resolved_session.calendar_registry,
        session_resolution=resolved_session.session_resolution,
        execution_input=execution_input,
        diagnostics=MarketDataTranslationDiagnostics(
            workflow_scope=normalized_request.workflow_scope,
            selected_stages=selected_stages,
            executed_preprocessing_steps=_executed_preprocessing_steps(
                workflow_scope=normalized_request.workflow_scope,
                option_surface_present=False,
                known_news_present=native_loading_output.known_news_summary is not None,
            ),
            tick_record_count=native_loading_output.tick_data_summary.retained_row_count,
            option_surface_record_count=0,
            known_news_event_count=native_loading_output.preprocessing_output.known_news_event_count,
            known_news_events_outside_source_window_count=(
                native_loading_output.preprocessing_output.known_news_events_outside_source_window_count
            ),
            session_open_tick_count=native_loading_output.preprocessing_output.session_open_tick_count,
            session_closed_tick_count=native_loading_output.preprocessing_output.session_closed_tick_count,
            known_news_open_event_count=(
                native_loading_output.preprocessing_output.known_news_open_event_count
            ),
            known_news_closed_event_count=(
                native_loading_output.preprocessing_output.known_news_closed_event_count
            ),
            seasonality_overlay_count=len(
                native_loading_output.preprocessing_output.seasonality_stage_input.scheduled_event_overlays
            ),
            hawkes_candidate_count=len(
                native_loading_output.preprocessing_output.hawkes_candidate_inputs or ()
            ),
            known_news_return_sample_count=(
                len(
                    native_loading_output.preprocessing_output.return_components_candidate_generation_input.realized_known_news_return_samples
                )
                if native_loading_output.preprocessing_output.return_components_candidate_generation_input
                is not None
                else 0
            ),
            intraday_jump_candidate_count=(
                len(
                    native_loading_output.preprocessing_output.return_components_candidate_generation_input.detected_intraday_jump_candidates
                )
                if native_loading_output.preprocessing_output.return_components_candidate_generation_input
                is not None
                else 0
            ),
            close_open_boundary_observation_count=(
                len(
                    native_loading_output.preprocessing_output.return_components_candidate_generation_input.observed_close_open_boundary_observations
                )
                if native_loading_output.preprocessing_output.return_components_candidate_generation_input
                is not None
                else 0
            ),
            material_close_open_gap_count=(
                native_loading_output.preprocessing_output.material_close_open_gap_count
            ),
        ),
        provenance=MarketDataTranslationProvenance(
            tick_data_summary=_loaded_source_summary_from_payload(
                native_loading_output.tick_data_summary,
            ),
            option_surface_summary=None,
            known_news_summary=(
                _loaded_source_summary_from_payload(native_loading_output.known_news_summary)
                if native_loading_output.known_news_summary is not None
                else None
            ),
            normalization_policy=normalized_request.normalization_policy,
            workflow_scope=normalized_request.workflow_scope,
            selected_stages=selected_stages,
            session_definition_path=resolved_session.session_definition_path,
            derived_trade_events_data_reference=trade_events_data_reference,
            derived_option_surface_data_reference=None,
            derivation_policies=(
                resolved_session.session_resolution.selected_policy_id,
                "market_data_loading_and_translation.rust.v1",
                "known_news_outside_tick_window_are_filtered.v1",
                "known_news_overlays_are_point_anchors_with_zero_log_adjustment.v1",
                "unsigned_trade_direction_labels_follow_tick_price_change_with_flat_move_carryover.v1",
                "hawkes_search_family_uses_unsigned_open_session_activity_targets_and_model_implied_fit_terms.v1",
                "surprise_jump_candidates_use_data_driven_material_open_session_return_thresholds_outside_known_news_windows.v1",
                "close_open_boundaries_are_resolved_from_calendar_state_transitions.v1",
                "continuous_nig_fit_uses_filtered_observed_open_session_returns_and_stage_search.v1",
            ),
            deterministic_defaults_applied=(
                "seasonality.session_filter=open_only",
                "seasonality.sparse_hour_event_threshold=1",
                "seasonality.intraday.minutes_per_bin=1",
                "seasonality.intraday.smoothing_kernel=gaussian",
                "seasonality.intraday.smoothing_bandwidth_bins=3.0",
                "seasonality.day_of_week.minimum_bucket_events=1",
                "seasonality.day_of_year.minimum_bucket_events=1",
                "known_news_overlay.log_adjustment=0.0",
                "close_open_gap_detection_policy.material_gap_absolute_return_tolerance=3x_median_abs_intraday_return",
                "intraday_jump_detection.material_threshold=4x_median_abs_open_return",
            ),
        ),
        fit_warning=native_loading_output.fit_warning,
        reference_price=native_loading_output.reference_price,
    )


def _load_market_data_translation_inputs_from_source_files(
    *,
    normalized_request: MarketDataCalibrationRequest,
    selected_stages: tuple[CalibrationStage, ...],
    trade_events_data_reference: str,
    calendar_registry: CalendarRegistry,
) -> _MarketDataLoadingOutput:
    try:
        native: ModuleType = native_module()
    except ModuleNotFoundError as e:
        raise _calibration_integration_error(
            "market-data loading module is unavailable",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    loading_request: dict[str, Any] = {
        "exchange_code": normalized_request.session_configuration.exchange_code,
        "trade_events_data_reference": trade_events_data_reference,
        "tick_data_source": _input_file_reference_payload(normalized_request.tick_data_source),
        "tick_data_column_mapping": {
            "timestamp_column": normalized_request.column_mapping.tick_data.timestamp_column,
            "price_column": normalized_request.column_mapping.tick_data.price_column,
            "trade_sign_column": normalized_request.column_mapping.tick_data.trade_sign_column,
            "bid_column": normalized_request.column_mapping.tick_data.bid_column,
            "ask_column": normalized_request.column_mapping.tick_data.ask_column,
        },
        "normalization_policy": {
            "timestamp_timezone": normalized_request.normalization_policy.timestamp_timezone,
            "sort_records_by_timestamp": normalized_request.normalization_policy.sort_records_by_timestamp,
            "duplicate_record_policy": (
                normalized_request.normalization_policy.duplicate_record_policy.value
                if normalized_request.normalization_policy.duplicate_record_policy is not None
                else None
            ),
            "tick_price_input_policy": (
                normalized_request.normalization_policy.tick_price_input_policy.value
                if normalized_request.normalization_policy.tick_price_input_policy is not None
                else None
            ),
        },
        "known_news_input": (
            {
                "calendar_source": _input_file_reference_payload(
                    normalized_request.known_news_inputs.calendar_source
                ),
                "column_mapping": {
                    "event_timestamp_column": normalized_request.column_mapping.known_news.event_timestamp_column,
                    "event_identifier_column": normalized_request.column_mapping.known_news.event_identifier_column,
                },
                "calendar_timezone": normalized_request.known_news_inputs.calendar_timezone,
            }
            if (
                normalized_request.known_news_inputs is not None
                and normalized_request.column_mapping.known_news is not None
            )
            else None
        ),
        "require_hawkes": CalibrationStage.HAWKES in selected_stages,
        "require_return_components": (
            CalibrationStage.RETURN_COMPONENTS in selected_stages
        ),
    }

    try:
        raw_payload = native.load_and_preprocess_market_data_translation_request(
            loading_request,
            calendar_registry._native_registry,
        )
    except Exception as e:
        if native_exception_matches(native, e, "CalibrationExecutionError"):
            parsed_native_error = _parse_market_data_loading_validation_error(str(e))
            if parsed_native_error is not None:
                field_path, message = parsed_native_error
                raise _translation_error(
                    field_path,
                    message,
                    context=_market_data_loading_error_context(
                        field_path=field_path,
                        message=message,
                        request=normalized_request,
                    ),
                ) from e

            raise _translation_error(
                "tick_data_source",
                f"market-data loading failed: {e}",
                context={"translation_step": "native_loading"},
            ) from e

        raise _translation_error(
            "tick_data_source",
            f"market-data loading failed: {e}",
            context={
                "translation_step": "native_loading",
                "source_exception_type": type(e).__name__,
            },
        ) from e

    if not isinstance(raw_payload, dict):
        raise _calibration_integration_error(
            "market-data loading returned an invalid response payload",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_payload).__name__},
        )

    try:
        return _market_data_loading_output_from_payload(raw_payload)
    except CalibrationValidationError as e:
        raise _calibration_integration_error(
            "market-data loading returned an invalid response payload",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e


def _input_file_reference_payload(file_reference: Any) -> dict[str, Any]:
    return {
        "path": file_reference.path,
        "file_format": file_reference.file_format.value,
        "provenance_label": file_reference.provenance_label,
    }


def _market_data_loading_output_from_payload(
    raw_payload: dict[str, Any],
) -> _MarketDataLoadingOutput:
    return _MarketDataLoadingOutput(
        preprocessing_output=_market_data_preprocessing_output_from_payload(
            raw_payload["preprocessing_output"]
        ),
        tick_data_summary=_source_summary_payload_from_mapping(
            raw_payload["tick_data_summary"],
            field_path="tick_data_summary",
        ),
        known_news_summary=(
            _source_summary_payload_from_mapping(
                raw_payload["known_news_summary"],
                field_path="known_news_summary",
            )
            if raw_payload.get("known_news_summary") is not None
            else None
        ),
        fit_warning=(
            str(raw_payload["fit_warning"])
            if raw_payload.get("fit_warning") is not None
            else None
        ),
        reference_price=(
            float(raw_payload["reference_price"])
            if raw_payload.get("reference_price") is not None
            else None
        ),
    )


def _source_summary_payload_from_mapping(
    raw_payload: dict[str, Any],
    *,
    field_path: str,
) -> _MarketDataSourceSummary:
    return _MarketDataSourceSummary(
        path=str(raw_payload["path"]),
        file_format=str(raw_payload["file_format"]),
        provenance_label=str(raw_payload["provenance_label"]),
        source_row_count=int(raw_payload["source_row_count"]),
        retained_row_count=int(raw_payload["retained_row_count"]),
        dropped_duplicate_row_count=int(raw_payload["dropped_duplicate_row_count"]),
        sort_records_by_timestamp=bool(raw_payload["sort_records_by_timestamp"]),
        applied_timestamp_timezone=(
            str(raw_payload["applied_timestamp_timezone"])
            if raw_payload.get("applied_timestamp_timezone") is not None
            else None
        ),
        canonical_value_field=(
            str(raw_payload["canonical_value_field"])
            if raw_payload.get("canonical_value_field") is not None
            else None
        ),
        value_mapping_policy=(
            str(raw_payload["value_mapping_policy"])
            if raw_payload.get("value_mapping_policy") is not None
            else None
        ),
    )


def _loaded_source_summary_from_payload(
    source_summary: _MarketDataSourceSummary,
) -> LoadedMarketDataSourceSummary:
    return LoadedMarketDataSourceSummary(
        path=source_summary.path,
        file_format=CalibrationInputFileFormat(source_summary.file_format),
        provenance_label=source_summary.provenance_label,
        source_row_count=source_summary.source_row_count,
        retained_row_count=source_summary.retained_row_count,
        dropped_duplicate_row_count=source_summary.dropped_duplicate_row_count,
        sort_records_by_timestamp=source_summary.sort_records_by_timestamp,
        applied_timestamp_timezone=source_summary.applied_timestamp_timezone,
        canonical_value_field=source_summary.canonical_value_field,
        value_mapping_policy=source_summary.value_mapping_policy,
    )


def _parse_market_data_loading_validation_error(
    message: str,
) -> tuple[str, str] | None:
    field_path, separator, detail = message.partition(": ")
    if not separator:
        return None
    if "." not in field_path and "[" not in field_path:
        return None

    return field_path, detail


def _market_data_loading_error_context(
    *,
    field_path: str,
    message: str,
    request: MarketDataCalibrationRequest,
) -> dict[str, Optional[str | int | float | bool]]:
    context: dict[str, Optional[str | int | float | bool]] = {
        "translation_step": "native_loading",
    }
    source_path = _source_path_for_market_data_loading_error(field_path, request)
    if source_path is not None:
        context["path" if field_path.endswith(".path") else "source_path"] = source_path

    extracted_source_path = _extract_source_path_from_native_error_message(message)
    if extracted_source_path is not None:
        context["source_path"] = extracted_source_path

    normalization_step = _normalization_step_for_market_data_loading_error(field_path)
    if normalization_step is not None:
        context["normalization_step"] = normalization_step

    return context


def _source_path_for_market_data_loading_error(
    field_path: str,
    request: MarketDataCalibrationRequest,
) -> Optional[str]:
    if field_path.startswith("tick_data_source") or field_path.startswith("column_mapping.tick_data"):
        return request.tick_data_source.path
    if (
        request.known_news_inputs is not None
        and (
            field_path.startswith("known_news_inputs.calendar_source")
            or field_path.startswith("column_mapping.known_news")
        )
    ):
        return request.known_news_inputs.calendar_source.path

    return None


def _extract_source_path_from_native_error_message(message: str) -> Optional[str]:
    marker = "source_path="
    marker_index = message.find(marker)
    if marker_index < 0:
        return None

    return message[marker_index + len(marker) :].strip()


def _normalization_step_for_market_data_loading_error(field_path: str) -> Optional[str]:
    if field_path.endswith(".path"):
        return "source_loading"
    if field_path.startswith("column_mapping."):
        return "column_mapping_validation"
    if ".rows[" in field_path:
        return "timestamp_timezone_normalization"

    return None


def _preprocess_market_data_translation_inputs(
    *,
    loaded_inputs: LoadedMarketDataCalibrationInputs,
    exchange_code: str,
    trade_events_data_reference: str,
    selected_stages: tuple[CalibrationStage, ...],
    directional_label_diagnostics: Optional[_TradeSignResolutionDiagnostics],
    calendar_registry: CalendarRegistry,
) -> _MarketDataPreprocessingOutput:
    try:
        native: ModuleType = native_module()
    except ModuleNotFoundError as e:
        raise _calibration_integration_error(
            "native market-data preprocessing module is unavailable",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    preprocessing_request: dict[str, Any] = {
        "exchange_code": exchange_code,
        "trade_events_data_reference": trade_events_data_reference,
        "tick_records": [
            {
                "timestamp_utc_nanos": datetime_to_utc_nanos(record.timestamp),
                "price": record.price,
                "trade_sign": (
                    record.trade_sign.value if record.trade_sign is not None else None
                ),
                "source_row_number": record.source_row_number,
            }
            for record in loaded_inputs.tick_data.records
        ],
        "known_news_records": (
            [
                {
                    "event_timestamp_utc_nanos": datetime_to_utc_nanos(
                        record.event_timestamp
                    ),
                    "event_identifier": record.event_identifier,
                }
                for record in loaded_inputs.known_news.records
            ]
            if loaded_inputs.known_news is not None
            else []
        ),
        "option_surface_quote_values": (
            [record.quote_value for record in loaded_inputs.option_surface.records]
            if loaded_inputs.option_surface is not None
            else []
        ),
        "directional_label_diagnostics": (
            {
                "policy_id": directional_label_diagnostics.policy_id,
                "first_inferred_trade_sign": (
                    directional_label_diagnostics.first_inferred_trade_sign.value
                ),
                "provided_label_count": directional_label_diagnostics.provided_label_count,
                "inferred_label_count": directional_label_diagnostics.inferred_label_count,
                "flat_move_carry_count": directional_label_diagnostics.flat_move_carry_count,
                "price_up_inference_count": (
                    directional_label_diagnostics.price_up_inference_count
                ),
                "price_down_inference_count": (
                    directional_label_diagnostics.price_down_inference_count
                ),
            }
            if directional_label_diagnostics is not None
            else None
        ),
        "require_hawkes": CalibrationStage.HAWKES in selected_stages,
        "require_return_components": (
            CalibrationStage.RETURN_COMPONENTS in selected_stages
        ),
    }

    try:
        raw_payload = native.preprocess_market_data_translation_request(
            preprocessing_request,
            calendar_registry._native_registry,
        )
    except Exception as e:
        if native_exception_matches(native, e, "CalibrationExecutionError"):
            raise _translation_error(
                "tick_data_source",
                f"native market-data preprocessing failed: {e}",
                context={"translation_step": "native_preprocessing"},
            ) from e

        raise _translation_error(
            "tick_data_source",
            f"native market-data preprocessing failed: {e}",
            context={
                "translation_step": "native_preprocessing",
                "source_exception_type": type(e).__name__,
            },
        ) from e

    if not isinstance(raw_payload, dict):
        raise _calibration_integration_error(
            "native market-data preprocessing returned an invalid response payload",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_payload).__name__},
        )

    try:
        return _market_data_preprocessing_output_from_payload(raw_payload)
    except CalibrationValidationError as e:
        raise _calibration_integration_error(
            "native market-data preprocessing returned an invalid response payload",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e


def _market_data_preprocessing_output_from_payload(
    raw_payload: dict[str, Any],
) -> _MarketDataPreprocessingOutput:
    raw_hawkes_candidates = raw_payload.get("hawkes_candidate_inputs")
    if raw_hawkes_candidates is not None and not isinstance(
        raw_hawkes_candidates,
        list,
    ):
        raise _calibration_error("hawkes_candidate_inputs", "must be a sequence")

    return _MarketDataPreprocessingOutput(
        seasonality_stage_input=(
            CalibrationSeasonalityStageExecutionInput.from_mapping(
                raw_payload["seasonality_stage_input"],
                field_path="seasonality_stage_input",
            )
        ),
        hawkes_candidate_inputs=(
            tuple(
                CalibrationHawkesStageCandidateInput.from_mapping(
                    raw_candidate,
                    field_path=f"hawkes_candidate_inputs[{candidate_index}]",
                )
                for candidate_index, raw_candidate in enumerate(
                    raw_hawkes_candidates
                )
            )
            if raw_hawkes_candidates is not None
            else None
        ),
        return_components_candidate_generation_input=(
            CalibrationReturnComponentsStageCandidateGenerationInput.from_mapping(
                raw_payload["return_components_candidate_generation_input"],
                field_path="return_components_candidate_generation_input",
            )
            if raw_payload.get("return_components_candidate_generation_input")
            is not None
            else None
        ),
        session_open_tick_count=int(raw_payload["session_open_tick_count"]),
        session_closed_tick_count=int(raw_payload["session_closed_tick_count"]),
        known_news_event_count=int(raw_payload["known_news_event_count"]),
        known_news_events_outside_source_window_count=int(
            raw_payload["known_news_events_outside_source_window_count"]
        ),
        known_news_open_event_count=int(
            raw_payload["known_news_open_event_count"]
        ),
        known_news_closed_event_count=int(
            raw_payload["known_news_closed_event_count"]
        ),
        material_close_open_gap_count=int(
            raw_payload["material_close_open_gap_count"]
        ),
    )


def _validate_tick_records_present(tick_data: LoadedTickDataSource) -> None:
    if len(tick_data.records) == 0:
        raise _translation_error(
            "tick_data_source",
            "must contain at least one normalized tick record for staged translation",
        )


def _validate_tick_record_order(
    tick_data: LoadedTickDataSource,
    *,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
) -> None:
    if normalization_policy.sort_records_by_timestamp:
        return

    previous_timestamp: Optional[datetime] = None
    for record in tick_data.records:
        if previous_timestamp is not None and record.timestamp < previous_timestamp:
            raise _translation_error(
                "normalization_policy.sort_records_by_timestamp",
                "cannot be false when tick records are not already sorted by timestamp",
                context={"source_row_number": record.source_row_number},
            )

        previous_timestamp = record.timestamp


def _selected_stages_for_scope(
    workflow_scope: CalibrationMarketDataWorkflowScope,
) -> tuple[CalibrationStage, ...]:
    match workflow_scope:
        case CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY:
            return (CalibrationStage.SEASONALITY,)
        case CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE:
            return (
                CalibrationStage.SEASONALITY,
                CalibrationStage.HAWKES,
                CalibrationStage.RETURN_COMPONENTS,
                CalibrationStage.JOINT_REFINEMENT,
            )
        case CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE:
            return (
                CalibrationStage.SEASONALITY,
                CalibrationStage.HAWKES,
                CalibrationStage.RETURN_COMPONENTS,
                CalibrationStage.JOINT_REFINEMENT,
            )


def _build_dataset_reference(run_id: str, dataset_role: str, provenance_label: str) -> str:
    return (
        f"market_data://{_sanitize_reference_component(run_id)}"
        f"/{dataset_role}/{_sanitize_reference_component(provenance_label)}"
    )


def _sanitize_reference_component(value: str) -> str:
    sanitized_characters: list[str] = [
        character.lower() if character.isalnum() else "_"
        for character in value.strip()
    ]

    return "".join(sanitized_characters).strip("_") or "unknown"


def _stable_seed_from_run_id(run_id: str) -> int:
    seed: int = 0
    for character_position, character in enumerate(run_id, start=1):
        seed += character_position * ord(character)

    return seed


def _default_cpu_count() -> int:
    return max(1, os.cpu_count() or 1)


def _default_solver_settings(
    stage: CalibrationStage,
    *,
    cpu_count: int | None = None,
) -> CalibrationStageSolverSettings:
    max_iterations: int = 150 if stage == CalibrationStage.SEASONALITY else 250

    return CalibrationStageSolverSettings(
        stage=stage,
        optimizer_settings=CalibrationOptimizerSettings(
            solver_name="lbfgs",
            max_iterations=max_iterations,
            cpu_count=_default_cpu_count() if cpu_count is None else cpu_count,
            absolute_tolerance=1e-8,
            relative_tolerance=1e-6,
            constraint_penalty_weight=10.0,
        ),
    )


def _runtime_resources_from_request(
    request: MarketDataCalibrationRequest,
) -> Optional[CalibrationStagedExecutionRuntimeResources]:
    if request.output_preferences.execution_mode != CalibrationExecutionMode.DISK:
        return None

    return CalibrationStagedExecutionRuntimeResources(
        disk_checkpoint_directory=request.output_preferences.disk_checkpoint_directory,
    )


def _extract_reference_price(normalized_records: tuple[Any, ...]) -> Optional[float]:
    if len(normalized_records) == 0:
        return None

    last_record = normalized_records[-1]
    return float(last_record.price) if last_record.price is not None else None


def _detect_interval_data_warning(normalized_records: tuple[Any, ...]) -> Optional[str]:
    if len(normalized_records) < 4:
        return None

    deltas_in_seconds: list[int] = []
    for previous_record, current_record in zip(normalized_records, normalized_records[1:]):
        previous_timestamp = previous_record.timestamp
        current_timestamp = current_record.timestamp
        delta_seconds = int((current_timestamp - previous_timestamp).total_seconds())
        if delta_seconds > 0:
            deltas_in_seconds.append(delta_seconds)

    if len(deltas_in_seconds) < 3:
        return None

    aligned_to_minute_count = sum(delta_seconds % 60 == 0 for delta_seconds in deltas_in_seconds)
    dominant_delta_seconds = max(
        set(deltas_in_seconds),
        key=deltas_in_seconds.count,
    )
    dominant_delta_share = deltas_in_seconds.count(dominant_delta_seconds) / len(deltas_in_seconds)

    if dominant_delta_seconds < 60:
        return None
    if aligned_to_minute_count / len(deltas_in_seconds) < 0.8:
        return None
    if dominant_delta_share < 0.6:
        return None

    return (
        "The supplied fit input appears to be interval data rather than tick data. "
        f"Janiform detected a dominant spacing of {_format_interval_warning_delta(dominant_delta_seconds)}, "
        "so fitting the model on this data will likely produce an invalid fit."
    )


def _format_interval_warning_delta(delta_seconds: int) -> str:
    if delta_seconds % 86_400 == 0:
        return f"{delta_seconds // 86_400}D"
    if delta_seconds % 3_600 == 0:
        return f"{delta_seconds // 3_600}h"
    if delta_seconds % 60 == 0:
        return f"{delta_seconds // 60}min"

    return f"{delta_seconds}s"


__all__ = [
    "MarketDataTranslationDiagnostics",
    "MarketDataTranslationProvenance",
    "MarketDataTranslationResult",
    "translate_market_data_request_to_staged_execution_input",
]
