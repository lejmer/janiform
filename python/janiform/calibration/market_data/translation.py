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
from .ingestion import LoadedMarketDataSourceSummary
from .request import MarketDataCalibrationRequest
from .._errors import (
    CALIBRATION_ERROR_FACTORY,
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    CalibrationValidationError,
    _calibration_error,
    _calibration_integration_error,
)
from ..sessions.contracts import (
    MarketDataSessionRecurringClosedWindow,
    MarketDataSessionRegularWindow,
    MarketDataSessionResolution,
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
from ...contracts import serialize_deterministic_json
from ..._native_api import native_exception_matches, native_module
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
class _MarketDataLoadingOutput:
    session_resolution: MarketDataSessionResolution
    preprocessing_output: _MarketDataPreprocessingOutput
    tick_data_summary: LoadedMarketDataSourceSummary
    option_surface_summary: Optional[LoadedMarketDataSourceSummary]
    known_news_summary: Optional[LoadedMarketDataSourceSummary]
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
    return _translate_market_data_request_from_source_files(
        normalized_request=normalized_request,
        cpu_count=cpu_count,
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


def _translate_market_data_request_from_source_files(
    *,
    normalized_request: MarketDataCalibrationRequest,
    cpu_count: int | None = None,
) -> MarketDataTranslationResult:
    selected_stages = _selected_stages_for_scope(normalized_request.workflow_scope)
    trade_events_data_reference = _build_dataset_reference(
        normalized_request.run_id,
        "ticks",
        normalized_request.tick_data_source.provenance_label,
    )
    option_surface_data_reference = (
        _build_dataset_reference(
            normalized_request.run_id,
            "options",
            normalized_request.option_surface_source.provenance_label,
        )
        if normalized_request.option_surface_source is not None
        else None
    )
    native_loading_output = _load_market_data_translation_inputs_from_source_files(
        normalized_request=normalized_request,
        selected_stages=selected_stages,
        trade_events_data_reference=trade_events_data_reference,
    )
    calendar_registry = CalendarRegistry(
        native_loading_output.session_resolution.resolved_session_definition_json
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
        calendar_registry=calendar_registry,
        session_resolution=native_loading_output.session_resolution,
        execution_input=execution_input,
        diagnostics=MarketDataTranslationDiagnostics(
            workflow_scope=normalized_request.workflow_scope,
            selected_stages=selected_stages,
            executed_preprocessing_steps=_executed_preprocessing_steps(
                workflow_scope=normalized_request.workflow_scope,
                option_surface_present=native_loading_output.option_surface_summary is not None,
                known_news_present=native_loading_output.known_news_summary is not None,
            ),
            tick_record_count=native_loading_output.tick_data_summary.retained_row_count,
            option_surface_record_count=(
                native_loading_output.option_surface_summary.retained_row_count
                if native_loading_output.option_surface_summary is not None
                else 0
            ),
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
            tick_data_summary=native_loading_output.tick_data_summary,
            option_surface_summary=native_loading_output.option_surface_summary,
            known_news_summary=native_loading_output.known_news_summary,
            normalization_policy=normalized_request.normalization_policy,
            workflow_scope=normalized_request.workflow_scope,
            selected_stages=selected_stages,
            session_definition_path=(
                normalized_request.session_configuration.session_definition_source.path
                if native_loading_output.session_resolution.manual_override_applied
                and normalized_request.session_configuration.session_definition_source is not None
                else None
            ),
            derived_trade_events_data_reference=trade_events_data_reference,
            derived_option_surface_data_reference=option_surface_data_reference,
            derivation_policies=(
                native_loading_output.session_resolution.selected_policy_id,
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
        "session_configuration": {
            "exchange_code": normalized_request.session_configuration.exchange_code,
            "session_definition_source": (
                _input_file_reference_payload(
                    normalized_request.session_configuration.session_definition_source
                )
                if normalized_request.session_configuration.session_definition_source
                is not None
                else None
            ),
            "session_timezone": normalized_request.session_configuration.session_timezone,
        },
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
            "option_quote_input_policy": (
                normalized_request.normalization_policy.option_quote_input_policy.value
                if normalized_request.normalization_policy.option_quote_input_policy
                is not None
                else None
            ),
        },
        "option_surface_input": (
            {
                "source": _input_file_reference_payload(
                    normalized_request.option_surface_source
                ),
                "column_mapping": {
                    "quote_timestamp_column": normalized_request.column_mapping.option_surface.quote_timestamp_column,
                    "expiry_timestamp_column": normalized_request.column_mapping.option_surface.expiry_timestamp_column,
                    "strike_column": normalized_request.column_mapping.option_surface.strike_column,
                    "option_type_column": normalized_request.column_mapping.option_surface.option_type_column,
                    "mid_price_column": normalized_request.column_mapping.option_surface.mid_price_column,
                    "bid_column": normalized_request.column_mapping.option_surface.bid_column,
                    "ask_column": normalized_request.column_mapping.option_surface.ask_column,
                    "implied_volatility_column": normalized_request.column_mapping.option_surface.implied_volatility_column,
                },
            }
            if normalized_request.option_surface_source is not None
            and normalized_request.column_mapping.option_surface is not None
            else None
        ),
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
        raw_payload = native.load_and_preprocess_market_data_translation_request(loading_request)
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


def resolve_market_data_session_configuration_from_source_file(
    *,
    session_configuration: Any,
    tick_data_source: Any,
    tick_data_column_mapping: Any,
    normalization_policy: Any,
) -> MarketDataSessionResolution:
    try:
        native: ModuleType = native_module()
    except ModuleNotFoundError as e:
        raise _calibration_integration_error(
            "market-data session-resolution module is unavailable",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    try:
        raw_payload = native.resolve_market_data_session_configuration_from_source_file(
            session_configuration.to_payload(),
            tick_data_source.to_payload(),
            tick_data_column_mapping.to_payload(),
            normalization_policy.to_payload(),
        )
    except Exception as e:
        if native_exception_matches(native, e, "CalibrationExecutionError"):
            parsed_native_error = _parse_market_data_loading_validation_error(str(e))
            if parsed_native_error is not None:
                field_path, message = parsed_native_error
                raise _translation_error(
                    field_path,
                    message,
                    context={
                        "translation_step": "native_session_resolution",
                        "source_path": tick_data_source.path,
                    },
                ) from e

        raise _translation_error(
            "session_configuration",
            f"market-data session resolution failed: {e}",
            context={"translation_step": "native_session_resolution"},
        ) from e

    if not isinstance(raw_payload, dict):
        raise _calibration_integration_error(
            "market-data session resolution returned an invalid response payload",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_payload).__name__},
        )

    return _market_data_session_resolution_from_payload(raw_payload)


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
        session_resolution=_market_data_session_resolution_from_payload(
            raw_payload["session_resolution"],
        ),
        preprocessing_output=_market_data_preprocessing_output_from_payload(
            raw_payload["preprocessing_output"]
        ),
        tick_data_summary=_source_summary_from_payload(
            raw_payload["tick_data_summary"],
            field_path="tick_data_summary",
        ),
        option_surface_summary=(
            _source_summary_from_payload(
                raw_payload["option_surface_summary"],
                field_path="option_surface_summary",
            )
            if raw_payload.get("option_surface_summary") is not None
            else None
        ),
        known_news_summary=(
            _source_summary_from_payload(
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


def _source_summary_from_payload(
    raw_payload: dict[str, Any],
    *,
    field_path: str,
) -> LoadedMarketDataSourceSummary:
    return LoadedMarketDataSourceSummary(
        path=str(raw_payload["path"]),
        file_format=CalibrationInputFileFormat(str(raw_payload["file_format"])),
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


def _market_data_session_resolution_from_payload(
    raw_payload: dict[str, Any],
) -> MarketDataSessionResolution:
    return MarketDataSessionResolution(
        schema_version=str(raw_payload["schema_version"]),
        resolution_kind=str(raw_payload["resolution_kind"]),
        selected_policy_id=str(raw_payload["selected_policy_id"]),
        manual_override_applied=bool(raw_payload["manual_override_applied"]),
        exchange_code=str(raw_payload["exchange_code"]),
        exchange_name=str(raw_payload["exchange_name"]),
        resolved_timezone_name=str(raw_payload["resolved_timezone_name"]),
        timezone_source=str(raw_payload["timezone_source"]),
        open_session_duration_hours_per_week=float(
            raw_payload["open_session_duration_hours_per_week"]
        ),
        confidence_score=float(raw_payload["confidence_score"]),
        confidence_label=str(raw_payload["confidence_label"]),
        regular_session_windows=tuple(
            MarketDataSessionRegularWindow.from_mapping(
                raw_window,
                field_path=f"session_resolution.regular_session_windows[{index}]",
            )
            for index, raw_window in enumerate(raw_payload["regular_session_windows"])
        ),
        recurring_closed_windows=tuple(
            MarketDataSessionRecurringClosedWindow.from_mapping(
                raw_window,
                field_path=f"session_resolution.recurring_closed_windows[{index}]",
            )
            for index, raw_window in enumerate(raw_payload["recurring_closed_windows"])
        ),
        reasons=tuple(str(value) for value in raw_payload["reasons"]),
        ambiguity_warnings=tuple(
            str(value) for value in raw_payload["ambiguity_warnings"]
        ),
        metadata_discrepancies=tuple(
            str(value) for value in raw_payload["metadata_discrepancies"]
        ),
        resolved_session_definition_json=str(
            raw_payload["resolved_session_definition_json"]
        ),
    )


def _parse_market_data_loading_validation_error(
    message: str,
) -> tuple[str, str] | None:
    field_path, separator, detail = message.partition(": ")
    if not separator:
        return None
    if " " in field_path or not field_path.strip():
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


__all__ = [
    "MarketDataTranslationDiagnostics",
    "MarketDataTranslationProvenance",
    "MarketDataTranslationResult",
    "translate_market_data_request_to_staged_execution_input",
]
