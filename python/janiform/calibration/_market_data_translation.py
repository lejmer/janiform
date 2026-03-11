from dataclasses import dataclass
from datetime import datetime
from types import ModuleType
from typing import Optional

from ._common import (
    CALIBRATION_ERROR_FACTORY,
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
    CalibrationExecutionMode,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationOptimizerSettings,
    CalibrationRunDataInputs,
    CalibrationRunSeedSettings,
    CalibrationStage,
    CalibrationStageSolverSettings,
    CalibrationValidationError,
    _calibration_error,
    _calibration_integration_error,
)
from ._market_data_ingestion import (
    LoadedMarketDataCalibrationInputs,
    LoadedMarketDataSourceSummary,
    LoadedTickDataSource,
    _TradeSignResolutionDiagnostics,
    _load_market_data_calibration_inputs,
)
from ._requests import MarketDataCalibrationRequest, StagedCalibrationRequest
from ._staged import (
    CalibrationHawkesStageCandidateInput,
    CalibrationReturnComponentsStageCandidateGenerationInput,
    CalibrationSeasonalityStageExecutionInput,
    CalibrationStagedExecutionRuntimeResources,
    StagedCalibrationExecutionInput,
)
from .._contracts import parse_json_object, serialize_deterministic_json
from .._native_api import native_module
from .._timestamps import datetime_to_utc_nanos
from ..calendar import CalendarRegistry, CalendarRegistryLoadError


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
    session_definition_path: str
    derived_trade_events_data_reference: str
    derived_option_surface_data_reference: Optional[str]
    derivation_policies: tuple[str, ...]
    deterministic_defaults_applied: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class MarketDataTranslationResult:
    calendar_registry: CalendarRegistry
    loaded_inputs: LoadedMarketDataCalibrationInputs
    execution_input: StagedCalibrationExecutionInput
    diagnostics: MarketDataTranslationDiagnostics
    provenance: MarketDataTranslationProvenance


@dataclass(frozen=True, slots=True)
class _NativeMarketDataTranslationPreprocessingOutput:
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


def translate_market_data_request_to_staged_execution_input(
    request: MarketDataCalibrationRequest,
) -> MarketDataTranslationResult:
    if not isinstance(request, MarketDataCalibrationRequest):
        raise _translation_error(
            "request",
            "must be a MarketDataCalibrationRequest",
        )

    normalized_request = MarketDataCalibrationRequest.from_mapping(request.to_payload())

    calendar_registry = _load_calendar_registry(normalized_request)
    selected_stages = _selected_stages_for_scope(normalized_request.workflow_scope)
    loaded_inputs_result = _load_market_data_calibration_inputs(
        normalized_request,
        resolve_unsigned_trade_signs=CalibrationStage.HAWKES in selected_stages,
    )
    loaded_inputs = loaded_inputs_result.inputs

    _validate_tick_records_present(loaded_inputs.tick_data)
    _validate_tick_record_order(
        loaded_inputs.tick_data,
        normalization_policy=normalized_request.normalization_policy,
    )

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
                _default_solver_settings(stage) for stage in selected_stages
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
        loaded_inputs=loaded_inputs,
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
            session_definition_path=(
                normalized_request.session_configuration.session_definition_source.path
            ),
            derived_trade_events_data_reference=trade_events_data_reference,
            derived_option_surface_data_reference=option_surface_data_reference,
            derivation_policies=(
                "native_market_data_translation_preprocessing.rust.v1",
                "known_news_outside_tick_window_are_filtered.v1",
                "known_news_overlays_are_point_anchors_with_zero_log_adjustment.v1",
                "unsigned_trade_direction_labels_follow_tick_price_change_with_flat_move_carryover.v1",
                "hawkes_search_family_uses_unsigned_open_session_activity_targets_and_model_implied_fit_terms.v1",
                "surprise_jump_candidates_use_open_session_log_returns_and_known_news_exclusion_windows.v1",
                "close_open_boundaries_are_resolved_from_calendar_state_transitions.v1",
                "nig_candidates_use_tick_return_scale_with_option_surface_adjustment.v1",
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
                "close_open_gap_detection_policy.material_gap_absolute_return_tolerance=max(0.0025,3x_median_abs_intraday_return)",
                "intraday_jump_detection.material_threshold=max(0.0050,3x_median_abs_open_return)",
            ),
        ),
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
    context: Optional[dict[str, str | int | float | bool | None]] = None,
) -> Exception:
    return _calibration_error(
        field_path,
        message,
        entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
        error_id=CALIBRATION_MARKET_DATA_TRANSLATION_ERROR_ID,
        context=context,
    )


def _load_calendar_registry(request: MarketDataCalibrationRequest) -> CalendarRegistry:
    session_source = request.session_configuration.session_definition_source
    if session_source.file_format.value != "json":
        raise _translation_error(
            "session_configuration.session_definition_source.file_format",
            "must be json for market-data translation",
            context={"translation_step": "session_definition_loading"},
        )

    try:
        with open(session_source.path, encoding="utf-8") as input_file:
            calendar_registry_json = input_file.read()
    except FileNotFoundError as e:
        raise _translation_error(
            "session_configuration.session_definition_source.path",
            "does not exist",
            context={
                "path": session_source.path,
                "translation_step": "session_definition_loading",
            },
        ) from e
    except OSError as e:
        raise _translation_error(
            "session_configuration.session_definition_source.path",
            f"could not be read: {e}",
            context={
                "path": session_source.path,
                "translation_step": "session_definition_loading",
            },
        ) from e

    try:
        return CalendarRegistry(calendar_registry_json)
    except CalendarRegistryLoadError as e:
        raise _translation_error(
            "session_configuration.session_definition_source.path",
            f"does not contain a valid calendar registry: {e}",
            context={
                "path": session_source.path,
                "translation_step": "session_definition_loading",
            },
        ) from e


def _preprocess_market_data_translation_inputs(
    *,
    loaded_inputs: LoadedMarketDataCalibrationInputs,
    exchange_code: str,
    trade_events_data_reference: str,
    selected_stages: tuple[CalibrationStage, ...],
    directional_label_diagnostics: Optional[_TradeSignResolutionDiagnostics],
    calendar_registry: CalendarRegistry,
) -> _NativeMarketDataTranslationPreprocessingOutput:
    try:
        native: ModuleType = native_module()
    except ModuleNotFoundError as e:
        raise _calibration_integration_error(
            "native market-data preprocessing module is unavailable",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    preprocessing_request: dict[str, object] = {
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
        response_json: str = native.preprocess_market_data_translation_request(
            serialize_deterministic_json(preprocessing_request),
            calendar_registry._native_registry,
        )
    except native.CalibrationExecutionError as e:
        raise _translation_error(
            "tick_data_source",
            f"native market-data preprocessing failed: {e}",
            context={"translation_step": "native_preprocessing"},
        ) from e
    except Exception as e:
        raise _translation_error(
            "tick_data_source",
            f"native market-data preprocessing failed: {e}",
            context={
                "translation_step": "native_preprocessing",
                "source_exception_type": type(e).__name__,
            },
        ) from e

    try:
        raw_payload = parse_json_object(
            response_json,
            "market-data translation preprocessing output",
            CALIBRATION_ERROR_FACTORY,
        )
        raw_hawkes_candidates = raw_payload.get("hawkes_candidate_inputs")
        if raw_hawkes_candidates is not None and not isinstance(
            raw_hawkes_candidates,
            list,
        ):
            raise _calibration_error("hawkes_candidate_inputs", "must be a sequence")

        return _NativeMarketDataTranslationPreprocessingOutput(
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
    except CalibrationValidationError as e:
        raise _calibration_integration_error(
            "native market-data preprocessing returned an invalid response payload",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e


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
            )
        case CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE:
            return (
                CalibrationStage.SEASONALITY,
                CalibrationStage.HAWKES,
                CalibrationStage.RETURN_COMPONENTS,
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


def _default_solver_settings(stage: CalibrationStage) -> CalibrationStageSolverSettings:
    max_iterations: int = 150 if stage == CalibrationStage.SEASONALITY else 250
    return CalibrationStageSolverSettings(
        stage=stage,
        optimizer_settings=CalibrationOptimizerSettings(
            solver_name="lbfgs",
            max_iterations=max_iterations,
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
