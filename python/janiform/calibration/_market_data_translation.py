from collections.abc import Sequence
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from math import log, sqrt
from typing import Optional

from ._common import (
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
    CalibrationExecutionMode,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationOptimizerSettings,
    CalibrationRunDataInputs,
    CalibrationRunSeedSettings,
    CalibrationStage,
    CalibrationStageSolverSettings,
    _calibration_error,
)
from ._market_data_ingestion import (
    LoadedKnownNewsSource,
    LoadedMarketDataCalibrationInputs,
    LoadedMarketDataSourceSummary,
    LoadedOptionSurfaceSource,
    LoadedTickDataSource,
    NormalizedKnownNewsRecord,
    NormalizedOptionSurfaceRecord,
    NormalizedTickDataRecord,
    load_market_data_calibration_inputs,
)
from ._requests import MarketDataCalibrationRequest, StagedCalibrationRequest
from ._staged import (
    CalibrationCoupledGapMarkConfiguration,
    CalibrationDayOfWeekSeasonalityEstimatorSettings,
    CalibrationDayOfYearSeasonalityEstimatorSettings,
    CalibrationDoubleExponentialBaseShockConfiguration,
    CalibrationHawkesStageCandidateInput,
    CalibrationHawkesStructuredAlphaParameterization,
    CalibrationHawkesStructuredRhoParameterization,
    CalibrationHawkesStructuredRhoSelfDominancePolicy,
    CalibrationIntradaySeasonalityEstimatorSettings,
    CalibrationIntradaySmoothingKernel,
    CalibrationObjectiveTermInput,
    CalibrationReturnComponentsCloseOpenGapDetectionPolicy,
    CalibrationReturnComponentsDetectedIntradayJumpCandidate,
    CalibrationReturnComponentsKnownNewsCalendarInput,
    CalibrationReturnComponentsKnownNewsReturnSample,
    CalibrationReturnComponentsObjectiveMetricWeights,
    CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    CalibrationReturnComponentsSignalSourceWindow,
    CalibrationReturnComponentsStageCandidateGenerationInput,
    CalibrationScheduledEventOverlay,
    CalibrationSeasonalityStageExecutionInput,
    CalibrationSeasonalityStageExecutionSettings,
    CalibrationSharedNewsMarkLawConfiguration,
    CalibrationStagedExecutionRuntimeResources,
    CalibrationTimestampEventRecord,
    CalibrationTimestampEventSessionFilter,
    StagedCalibrationExecutionInput,
)
from ..calendar import CalendarRegistry, CalendarRegistryLoadError, MarketState
from ..pricing import GammaSeverityMarkConfiguration, NigSharedPlusShiftConfiguration


CALIBRATION_MARKET_DATA_TRANSLATION_ERROR_ID = "calibration.validation.market_data_translation"
CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT = "market_data_translation"
_TRANSITION_SEARCH_STEP = timedelta(minutes=1)
_ACTIVITY_LIFT_WINDOW = timedelta(minutes=30)
_KNOWN_NEWS_EXCLUSION_WINDOW = timedelta(minutes=15)
_EPOCH = datetime(1970, 1, 1, tzinfo=UTC)


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
class _ResolvedTickRecord:
    record: NormalizedTickDataRecord
    market_state: MarketState


@dataclass(frozen=True, slots=True)
class _ResolvedKnownNewsRecord:
    record: NormalizedKnownNewsRecord
    market_state: MarketState


@dataclass(frozen=True, slots=True)
class _OpenPriceTick:
    timestamp: datetime
    price: float
    source_row_number: int


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
    loaded_inputs = load_market_data_calibration_inputs(normalized_request)

    _validate_tick_records_present(loaded_inputs.tick_data)
    _validate_tick_record_order(
        loaded_inputs.tick_data,
        normalization_policy=normalized_request.normalization_policy,
    )

    selected_stages = _selected_stages_for_scope(normalized_request.workflow_scope)

    resolved_ticks = _resolve_tick_market_states(
        loaded_inputs.tick_data.records,
        exchange_code=normalized_request.session_configuration.exchange_code,
        calendar_registry=calendar_registry,
    )

    open_price_ticks = _build_open_price_ticks(resolved_ticks)
    source_window = _build_source_window(loaded_inputs.tick_data.records)

    retained_known_news, known_news_outside_source_window_count = _resolve_known_news_records(
        loaded_inputs.known_news,
        exchange_code=normalized_request.session_configuration.exchange_code,
        calendar_registry=calendar_registry,
        source_window=source_window,
    )

    seasonality_stage_input = _build_seasonality_stage_input(
        tick_data=loaded_inputs.tick_data,
        retained_known_news=retained_known_news,
        trade_events_data_reference=_build_dataset_reference(
            normalized_request.run_id,
            "ticks",
            loaded_inputs.tick_data.summary.provenance_label,
        ),
    )

    hawkes_candidate_inputs: Optional[tuple[CalibrationHawkesStageCandidateInput, ...]] = None
    if CalibrationStage.HAWKES in selected_stages:
        hawkes_candidate_inputs = _build_hawkes_candidate_inputs(
            resolved_ticks=resolved_ticks,
            retained_known_news=retained_known_news,
        )

    return_components_candidate_generation_input: Optional[
        CalibrationReturnComponentsStageCandidateGenerationInput
    ] = None

    material_close_open_gap_count: int = 0

    if CalibrationStage.RETURN_COMPONENTS in selected_stages:
        return_components_candidate_generation_input, material_close_open_gap_count = (
            _build_return_components_candidate_generation_input(
                open_price_ticks=open_price_ticks,
                option_surface=loaded_inputs.option_surface,
                retained_known_news=retained_known_news,
                exchange_code=normalized_request.session_configuration.exchange_code,
                calendar_registry=calendar_registry,
                source_window=source_window,
            )
        )

    trade_events_data_reference = _build_dataset_reference(
        normalized_request.run_id,
        "ticks",
        loaded_inputs.tick_data.summary.provenance_label,
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
                _default_solver_settings(stage)
                for stage in selected_stages
            ),
        ),
        seasonality_stage_input=(
            seasonality_stage_input
            if CalibrationStage.SEASONALITY in selected_stages
            else None
        ),
        hawkes_candidate_inputs=hawkes_candidate_inputs,
        return_components_candidate_generation_input=return_components_candidate_generation_input,
        runtime_resources=_runtime_resources_from_request(normalized_request),
    )

    diagnostics = MarketDataTranslationDiagnostics(
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
        known_news_event_count=len(retained_known_news),
        known_news_events_outside_source_window_count=known_news_outside_source_window_count,
        session_open_tick_count=sum(
            1 for resolved_tick in resolved_ticks if resolved_tick.market_state == MarketState.OPEN
        ),
        session_closed_tick_count=sum(
            1 for resolved_tick in resolved_ticks if resolved_tick.market_state == MarketState.CLOSED
        ),
        known_news_open_event_count=sum(
            1 for known_news_record in retained_known_news if known_news_record.market_state == MarketState.OPEN
        ),
        known_news_closed_event_count=sum(
            1 for known_news_record in retained_known_news if known_news_record.market_state == MarketState.CLOSED
        ),
        seasonality_overlay_count=len(seasonality_stage_input.scheduled_event_overlays),
        hawkes_candidate_count=len(hawkes_candidate_inputs or ()),
        known_news_return_sample_count=(
            len(return_components_candidate_generation_input.realized_known_news_return_samples)
            if return_components_candidate_generation_input is not None
            else 0
        ),
        intraday_jump_candidate_count=(
            len(return_components_candidate_generation_input.detected_intraday_jump_candidates)
            if return_components_candidate_generation_input is not None
            else 0
        ),
        close_open_boundary_observation_count=(
            len(return_components_candidate_generation_input.observed_close_open_boundary_observations)
            if return_components_candidate_generation_input is not None
            else 0
        ),
        material_close_open_gap_count=material_close_open_gap_count,
    )
    provenance = MarketDataTranslationProvenance(
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
        session_definition_path=normalized_request.session_configuration.session_definition_source.path,
        derived_trade_events_data_reference=trade_events_data_reference,
        derived_option_surface_data_reference=option_surface_data_reference,
        derivation_policies=(
            "calendar_registry.resolve_market_state_at_utc.v1",
            "known_news_outside_tick_window_are_filtered.v1",
            "known_news_overlays_are_point_anchors_with_zero_log_adjustment.v1",
            "hawkes_candidates_are_seeded_from_event_rate_and_interarrival_dispersion.v1",
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
    )

    return MarketDataTranslationResult(
        calendar_registry=calendar_registry,
        loaded_inputs=loaded_inputs,
        execution_input=execution_input,
        diagnostics=diagnostics,
        provenance=provenance,
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


def _resolve_tick_market_states(
    tick_records: Sequence[NormalizedTickDataRecord],
    *,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
) -> tuple[_ResolvedTickRecord, ...]:
    return tuple(
        _ResolvedTickRecord(
            record=tick_record,
            market_state=calendar_registry.resolve_market_state(
                exchange_code,
                tick_record.timestamp,
            ),
        )
        for tick_record in tick_records
    )


def _build_open_price_ticks(
    resolved_ticks: Sequence[_ResolvedTickRecord],
) -> tuple[_OpenPriceTick, ...]:
    open_price_ticks: list[_OpenPriceTick] = []
    for resolved_tick in resolved_ticks:
        if resolved_tick.market_state != MarketState.OPEN:
            continue
        if resolved_tick.record.price is None:
            continue
        if resolved_tick.record.price <= 0.0:
            raise _translation_error(
                "tick_data_source",
                "open-session tick prices must be strictly positive for return derivation",
                context={"source_row_number": resolved_tick.record.source_row_number},
            )

        open_price_ticks.append(
            _OpenPriceTick(
                timestamp=resolved_tick.record.timestamp,
                price=resolved_tick.record.price,
                source_row_number=resolved_tick.record.source_row_number,
            )
        )

    return tuple(open_price_ticks)


def _build_source_window(
    tick_records: Sequence[NormalizedTickDataRecord],
) -> CalibrationReturnComponentsSignalSourceWindow:
    window_start_timestamp = min(record.timestamp for record in tick_records)
    window_end_timestamp = max(record.timestamp for record in tick_records)
    window_end_utc_nanos = _datetime_to_utc_nanos(window_end_timestamp) + 1

    return CalibrationReturnComponentsSignalSourceWindow(
        window_start_utc_nanos=_datetime_to_utc_nanos(window_start_timestamp),
        window_end_utc_nanos=window_end_utc_nanos,
    )


def _resolve_known_news_records(
    known_news_source: Optional[LoadedKnownNewsSource],
    *,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
    source_window: CalibrationReturnComponentsSignalSourceWindow,
) -> tuple[tuple[_ResolvedKnownNewsRecord, ...], int]:
    if known_news_source is None:
        return (), 0

    retained_records: list[_ResolvedKnownNewsRecord] = []
    outside_source_window_count = 0
    for known_news_record in known_news_source.records:
        timestamp_utc_nanos = _datetime_to_utc_nanos(known_news_record.event_timestamp)
        if (
            timestamp_utc_nanos < source_window.window_start_utc_nanos
            or timestamp_utc_nanos >= source_window.window_end_utc_nanos
        ):
            outside_source_window_count += 1
            continue

        retained_records.append(
            _ResolvedKnownNewsRecord(
                record=known_news_record,
                market_state=calendar_registry.resolve_market_state(
                    exchange_code,
                    known_news_record.event_timestamp,
                ),
            )
        )

    return tuple(retained_records), outside_source_window_count


def _build_seasonality_stage_input(
    *,
    tick_data: LoadedTickDataSource,
    retained_known_news: Sequence[_ResolvedKnownNewsRecord],
    trade_events_data_reference: str,
) -> CalibrationSeasonalityStageExecutionInput:
    return CalibrationSeasonalityStageExecutionInput(
        timestamp_event_records=tuple(
            CalibrationTimestampEventRecord(
                source_record_position=tick_record.source_row_number - 1,
                timestamp_utc_nanos=_datetime_to_utc_nanos(tick_record.timestamp),
            )
            for tick_record in tick_data.records
        ),
        scheduled_event_overlays=tuple(
            CalibrationScheduledEventOverlay(
                overlay_id=(
                    known_news_record.record.event_identifier
                    if known_news_record.record.event_identifier is not None
                    else f"known_news_{position + 1}"
                ),
                start_timestamp_utc_nanos=_datetime_to_utc_nanos(
                    known_news_record.record.event_timestamp
                ),
                end_timestamp_utc_nanos=_datetime_to_utc_nanos(
                    known_news_record.record.event_timestamp
                ),
                log_adjustment=0.0,
                precedence=0,
                enabled=True,
                source_label="known_news_calendar",
            )
            for position, known_news_record in enumerate(retained_known_news)
        ),
        settings=CalibrationSeasonalityStageExecutionSettings(
            session_filter=CalibrationTimestampEventSessionFilter.OPEN_ONLY,
            sparse_hour_event_threshold=1,
            intraday_settings=CalibrationIntradaySeasonalityEstimatorSettings(
                minutes_per_bin=1,
                smoothing_kernel=CalibrationIntradaySmoothingKernel.GAUSSIAN,
                smoothing_bandwidth_bins=3.0,
                minimum_bin_events=1,
                pseudocount=1e-6,
            ),
            day_of_week_settings=CalibrationDayOfWeekSeasonalityEstimatorSettings(
                minimum_bucket_events=1,
                pseudocount=1e-6,
            ),
            day_of_year_settings=CalibrationDayOfYearSeasonalityEstimatorSettings(
                minimum_bucket_events=1,
                pseudocount=1e-6,
            ),
        ),
        dataset_id_override=trade_events_data_reference,
    )


def _build_hawkes_candidate_inputs(
    *,
    resolved_ticks: Sequence[_ResolvedTickRecord],
    retained_known_news: Sequence[_ResolvedKnownNewsRecord],
) -> tuple[CalibrationHawkesStageCandidateInput, ...]:
    tick_count: int = len(resolved_ticks)
    open_tick_count: int = sum(
        1 for resolved_tick in resolved_ticks if resolved_tick.market_state == MarketState.OPEN
    )
    closed_tick_count: int = tick_count - open_tick_count
    tick_timestamps: list[datetime] = [
        resolved_tick.record.timestamp
        for resolved_tick in resolved_ticks
    ]
    duration_hours: float = _observation_window_hours(
        tick_timestamps
    )
    interarrival_seconds: list[float] = _adjacent_durations_seconds(
        tick_timestamps
    )
    mean_interarrival_seconds: float = _mean(interarrival_seconds) or 60.0
    interarrival_coefficient_of_variation: float = _coefficient_of_variation(interarrival_seconds)
    lambda_bar: float = max(open_tick_count / duration_hours / 2.0, 0.01)
    beta: float = min(max(60.0 / max(mean_interarrival_seconds, 1.0), 0.5), 5.0)
    known_news_ratio: float = len(retained_known_news) / max(tick_count, 1)
    a_self: float = min(beta * 0.45, 0.25 + interarrival_coefficient_of_variation * 0.15)
    a_cross: float = min(a_self * 0.3, 0.03 + known_news_ratio * 0.2)
    r_self: float = 0.15 + min(0.25, known_news_ratio * 2.0)
    r_cross: float = r_self * 0.25

    return (
        CalibrationHawkesStageCandidateInput(
            candidate_id="market_data_seed_primary",
            lambda_bar=round(lambda_bar, 6),
            beta=round(beta, 6),
            structured_alpha=CalibrationHawkesStructuredAlphaParameterization(
                a_self=round(max(a_self, 0.05), 6),
                a_cross=round(max(a_cross, 0.0), 6),
                delta_self=round(max(a_self * 0.2, 0.0), 6),
                delta_cross=round(max(a_cross * 0.5, 0.0), 6),
            ),
            structured_rho=CalibrationHawkesStructuredRhoParameterization(
                r_self=round(max(r_self, 0.05), 6),
                r_cross=round(max(r_cross, 0.0), 6),
                delta_self_r=round(max(r_self * 0.15, 0.0), 6),
                delta_cross_r=round(max(r_cross * 0.5, 0.0), 6),
            ),
            self_dominance_policy=CalibrationHawkesStructuredRhoSelfDominancePolicy(
                minimum_self_to_cross_ratio=1.0,
            ),
            stability_numeric_tolerance=0.0,
            objective_terms=(
                CalibrationObjectiveTermInput(
                    term_id="event_rate_per_hour",
                    weight=1.0,
                    observed_score=round(open_tick_count / duration_hours, 6),
                ),
                CalibrationObjectiveTermInput(
                    term_id="interarrival_coefficient_of_variation",
                    weight=1.0,
                    observed_score=round(interarrival_coefficient_of_variation, 6),
                ),
                CalibrationObjectiveTermInput(
                    term_id="closed_tick_ratio",
                    weight=1.0,
                    observed_score=round(closed_tick_count / max(tick_count, 1), 6),
                ),
            ),
        ),
    )


def _build_return_components_candidate_generation_input(
    *,
    open_price_ticks: Sequence[_OpenPriceTick],
    option_surface: Optional[LoadedOptionSurfaceSource],
    retained_known_news: Sequence[_ResolvedKnownNewsRecord],
    exchange_code: str,
    calendar_registry: CalendarRegistry,
    source_window: CalibrationReturnComponentsSignalSourceWindow,
) -> tuple[CalibrationReturnComponentsStageCandidateGenerationInput, int]:
    if len(open_price_ticks) < 2:
        raise _translation_error(
            "tick_data_source",
            "full calibration requires at least two open-session price observations",
        )

    close_open_boundary_observations: tuple[
        CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
        ...
    ] = _derive_close_open_boundary_observations(
        open_price_ticks,
        exchange_code=exchange_code,
        calendar_registry=calendar_registry,
    )
    known_news_return_samples: tuple[
        CalibrationReturnComponentsKnownNewsReturnSample,
        ...
    ] = _derive_known_news_return_samples(
        retained_known_news=retained_known_news,
        open_price_ticks=open_price_ticks,
        close_open_boundary_observations=close_open_boundary_observations,
    )
    intraday_jump_candidates: tuple[
        CalibrationReturnComponentsDetectedIntradayJumpCandidate,
        ...
    ] = _derive_intraday_jump_candidates(
        open_price_ticks=open_price_ticks,
        retained_known_news=retained_known_news,
        exchange_code=exchange_code,
        calendar_registry=calendar_registry,
    )
    intraday_returns: list[tuple[datetime, float]] = _same_session_open_returns(
        open_price_ticks,
        exchange_code=exchange_code,
        calendar_registry=calendar_registry,
    )
    median_abs_intraday_return: float = _median_abs(
        [return_value for _, return_value in intraday_returns]
    )
    gap_tolerance: float = max(0.0025, median_abs_intraday_return * 3.0)
    material_close_open_gap_count: int = sum(
        1
        for boundary_observation in close_open_boundary_observations
        if abs(log(boundary_observation.open_price / boundary_observation.close_price))
        > gap_tolerance
    )

    candidate_generation_input = CalibrationReturnComponentsStageCandidateGenerationInput(
        known_news_calendar=CalibrationReturnComponentsKnownNewsCalendarInput(
            source_window=source_window,
            scheduled_event_timestamps_utc_nanos=tuple(
                _datetime_to_utc_nanos(known_news_record.record.event_timestamp)
                for known_news_record in retained_known_news
            ),
        ),
        realized_known_news_return_samples=known_news_return_samples,
        detected_intraday_jump_candidates=intraday_jump_candidates,
        observed_close_open_boundary_observations=close_open_boundary_observations,
        close_open_gap_detection_policy=CalibrationReturnComponentsCloseOpenGapDetectionPolicy(
            material_gap_absolute_return_tolerance=round(gap_tolerance, 6),
        ),
        nig_shared_plus_shift_candidates=_derive_nig_candidates(
            intraday_returns=intraday_returns,
            option_surface_records=(
                option_surface.records
                if option_surface is not None
                else ()
            ),
        ),
        shared_mark_law_template=_derive_shared_mark_law_template(
            intraday_returns=intraday_returns,
            close_open_boundary_observations=close_open_boundary_observations,
        ),
        objective_metric_weights=CalibrationReturnComponentsObjectiveMetricWeights(
            open_jump_fit_weight=1.0,
            closed_gap_fit_weight=1.0,
            activity_coupling_fit_weight=1.0,
        ),
    )

    return candidate_generation_input, material_close_open_gap_count


def _derive_close_open_boundary_observations(
    open_price_ticks: Sequence[_OpenPriceTick],
    *,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
) -> tuple[CalibrationReturnComponentsObservedCloseOpenBoundaryObservation, ...]:
    observations_by_boundary: dict[
        tuple[int, int],
        CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    ] = {}
    for previous_tick, current_tick in zip(open_price_ticks, open_price_ticks[1:]):
        close_boundary: Optional[datetime] = _find_close_boundary_between(
            previous_tick.timestamp,
            current_tick.timestamp,
            exchange_code=exchange_code,
            calendar_registry=calendar_registry,
        )
        if close_boundary is None:
            continue

        open_boundary: Optional[datetime] = _find_open_boundary_between(
            close_boundary,
            current_tick.timestamp,
            exchange_code=exchange_code,
            calendar_registry=calendar_registry,
        )
        if open_boundary is None:
            continue

        observations_by_boundary[
            (
                _datetime_to_utc_nanos(close_boundary),
                _datetime_to_utc_nanos(open_boundary),
            )
        ] = CalibrationReturnComponentsObservedCloseOpenBoundaryObservation(
            close_timestamp_utc_nanos=_datetime_to_utc_nanos(close_boundary),
            close_price=previous_tick.price,
            open_timestamp_utc_nanos=_datetime_to_utc_nanos(open_boundary),
            open_price=current_tick.price,
            post_shock_activity_lift=_activity_lift(
                open_boundary,
                open_price_ticks,
            ),
        )

    return tuple(
        observations_by_boundary[key]
        for key in sorted(observations_by_boundary)
    )


def _derive_known_news_return_samples(
    *,
    retained_known_news: Sequence[_ResolvedKnownNewsRecord],
    open_price_ticks: Sequence[_OpenPriceTick],
    close_open_boundary_observations: Sequence[
        CalibrationReturnComponentsObservedCloseOpenBoundaryObservation
    ],
) -> tuple[CalibrationReturnComponentsKnownNewsReturnSample, ...]:
    return_samples: list[CalibrationReturnComponentsKnownNewsReturnSample] = []
    for known_news_record in retained_known_news:
        event_timestamp: datetime = known_news_record.record.event_timestamp
        event_timestamp_utc_nanos: int = _datetime_to_utc_nanos(event_timestamp)

        match known_news_record.market_state:
            case MarketState.OPEN:
                before_tick: Optional[_OpenPriceTick] = _last_open_tick_at_or_before(open_price_ticks, event_timestamp)
                after_tick: Optional[_OpenPriceTick] = _first_open_tick_after(open_price_ticks, event_timestamp)
                if before_tick is None or after_tick is None:
                    continue

                return_samples.append(
                    CalibrationReturnComponentsKnownNewsReturnSample(
                        event_timestamp_utc_nanos=event_timestamp_utc_nanos,
                        open_window_jump_return=log(after_tick.price / before_tick.price),
                        closed_window_gap_return=0.0,
                    )
                )
            case MarketState.CLOSED:
                boundary_observation: Optional[
                    CalibrationReturnComponentsObservedCloseOpenBoundaryObservation
                ] = _boundary_observation_for_event(
                    close_open_boundary_observations,
                    event_timestamp_utc_nanos=event_timestamp_utc_nanos,
                )
                if boundary_observation is None:
                    continue

                return_samples.append(
                    CalibrationReturnComponentsKnownNewsReturnSample(
                        event_timestamp_utc_nanos=event_timestamp_utc_nanos,
                        open_window_jump_return=0.0,
                        closed_window_gap_return=log(
                            boundary_observation.open_price
                            / boundary_observation.close_price
                        ),
                    )
                )

    return tuple(return_samples)


def _derive_intraday_jump_candidates(
    *,
    open_price_ticks: Sequence[_OpenPriceTick],
    retained_known_news: Sequence[_ResolvedKnownNewsRecord],
    exchange_code: str,
    calendar_registry: CalendarRegistry,
) -> tuple[CalibrationReturnComponentsDetectedIntradayJumpCandidate, ...]:
    candidate_returns: list[tuple[datetime, float]] = [
        (candidate_timestamp, jump_return)
        for candidate_timestamp, jump_return in _same_session_open_returns(
            open_price_ticks,
            exchange_code=exchange_code,
            calendar_registry=calendar_registry,
        )
        if not _is_within_known_news_exclusion_window(
            candidate_timestamp,
            retained_known_news,
        )
    ]
    if len(candidate_returns) == 0:
        raise _translation_error(
            "tick_data_source",
            "full calibration requires at least one non-known-news open-session return candidate",
        )

    threshold: float = max(
        0.005,
        _median_abs([jump_return for _, jump_return in candidate_returns]) * 3.0,
    )
    material_candidates: list[tuple[datetime, float]] = [
        (candidate_timestamp, jump_return)
        for candidate_timestamp, jump_return in candidate_returns
        if abs(jump_return) > threshold
    ]
    if len(material_candidates) == 0:
        largest_candidate: tuple[datetime, float] = max(
            candidate_returns,
            key=lambda candidate: abs(candidate[1]),
        )
        material_candidates: list[tuple[datetime, float]] = [largest_candidate]

    return tuple(
        CalibrationReturnComponentsDetectedIntradayJumpCandidate(
            jump_timestamp_utc_nanos=_datetime_to_utc_nanos(candidate_timestamp),
            jump_return=jump_return,
            post_shock_activity_lift=_activity_lift(
                candidate_timestamp,
                open_price_ticks,
            ),
        )
        for candidate_timestamp, jump_return in material_candidates
    )


def _same_session_open_returns(
    open_price_ticks: Sequence[_OpenPriceTick],
    *,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
) -> list[tuple[datetime, float]]:
    same_session_returns: list[tuple[datetime, float]] = []
    for previous_tick, current_tick in zip(open_price_ticks, open_price_ticks[1:]):
        close_boundary = _find_close_boundary_between(
            previous_tick.timestamp,
            current_tick.timestamp,
            exchange_code=exchange_code,
            calendar_registry=calendar_registry,
        )
        if close_boundary is not None:
            continue

        same_session_returns.append(
            (
                current_tick.timestamp,
                log(current_tick.price / previous_tick.price),
            )
        )

    return same_session_returns


def _derive_nig_candidates(
    *,
    intraday_returns: Sequence[tuple[datetime, float]],
    option_surface_records: Sequence[NormalizedOptionSurfaceRecord],
) -> tuple[NigSharedPlusShiftConfiguration, ...]:
    return_values: list[float] = [return_value for _, return_value in intraday_returns]
    mean_return: float = _mean(return_values) or 0.0
    root_mean_square_return: float = sqrt(
        _mean([return_value * return_value for return_value in return_values]) or 1e-4
    )
    option_quote_level: float = _mean(
        [option_record.quote_value for option_record in option_surface_records]
    ) or 0.0

    beta_0: float = -min(0.9, max(0.35, abs(mean_return) * 500.0))
    delta_beta: float = -0.15
    alpha: float = max(2.0 + min(option_quote_level / 10.0, 1.5), abs(beta_0 + delta_beta) + 0.25)
    delta: float = max(
        0.1,
        min(1.5, root_mean_square_return * 25.0 + (option_quote_level / 100.0)),
    )

    return (
        NigSharedPlusShiftConfiguration(
            alpha=round(alpha, 6),
            beta_0=round(beta_0, 6),
            delta=round(delta, 6),
            mu=round(mean_return, 6),
            delta_beta=delta_beta,
        ),
    )


def _derive_shared_mark_law_template(
    *,
    intraday_returns: Sequence[tuple[datetime, float]],
    close_open_boundary_observations: Sequence[
        CalibrationReturnComponentsObservedCloseOpenBoundaryObservation
    ],
) -> CalibrationSharedNewsMarkLawConfiguration:
    mean_abs_open_return: float = _mean(
        [abs(return_value) for _, return_value in intraday_returns]
    ) or 0.01
    mean_abs_gap_return: float = _mean(
        [
            abs(log(boundary_observation.open_price / boundary_observation.close_price))
            for boundary_observation in close_open_boundary_observations
        ]
    ) or mean_abs_open_return

    return CalibrationSharedNewsMarkLawConfiguration(
        gamma_severity_marks=GammaSeverityMarkConfiguration(
            shape_k=2.0,
            scale_theta=round(max(mean_abs_open_return * 20.0, 0.5), 6),
        ),
        double_exponential_base_shock=CalibrationDoubleExponentialBaseShockConfiguration(
            probability_positive=0.45,
            eta_plus=8.0,
            eta_minus=10.0,
        ),
        coupled_gap_marks=CalibrationCoupledGapMarkConfiguration(
            sigma_g=round(max(mean_abs_gap_return * 10.0, 0.1), 6),
        ),
    )


def _find_close_boundary_between(
    start_timestamp: datetime,
    end_timestamp: datetime,
    *,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
) -> Optional[datetime]:
    start_state: MarketState = calendar_registry.resolve_market_state(exchange_code, start_timestamp)
    if start_state != MarketState.OPEN:
        return None

    start_microseconds: int = _datetime_to_epoch_microseconds(start_timestamp)
    end_microseconds: int = _datetime_to_epoch_microseconds(end_timestamp)
    closed_probe: Optional[int] = _first_probe_with_state(
        start_microseconds,
        end_microseconds,
        target_state=MarketState.CLOSED,
        exchange_code=exchange_code,
        calendar_registry=calendar_registry,
    )
    if closed_probe is None:
        return None

    return _datetime_from_epoch_microseconds(
        _earliest_microsecond_with_state(
            start_microseconds,
            closed_probe,
            target_state=MarketState.CLOSED,
            exchange_code=exchange_code,
            calendar_registry=calendar_registry,
        )
    )


def _find_open_boundary_between(
    start_timestamp: datetime,
    end_timestamp: datetime,
    *,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
) -> Optional[datetime]:
    end_state: MarketState = calendar_registry.resolve_market_state(exchange_code, end_timestamp)
    if end_state != MarketState.OPEN:
        return None

    start_microseconds: int = _datetime_to_epoch_microseconds(start_timestamp)
    end_microseconds: int = _datetime_to_epoch_microseconds(end_timestamp)
    closed_probe: Optional[int] = _last_probe_with_state(
        start_microseconds,
        end_microseconds,
        target_state=MarketState.CLOSED,
        exchange_code=exchange_code,
        calendar_registry=calendar_registry,
    )
    if closed_probe is None:
        return None

    return _datetime_from_epoch_microseconds(
        _earliest_microsecond_with_state(
            closed_probe,
            end_microseconds,
            target_state=MarketState.OPEN,
            exchange_code=exchange_code,
            calendar_registry=calendar_registry,
        )
    )


def _first_probe_with_state(
    start_microseconds: int,
    end_microseconds: int,
    *,
    target_state: MarketState,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
) -> Optional[int]:
    cursor: int = start_microseconds + int(_TRANSITION_SEARCH_STEP.total_seconds() * 1_000_000)
    while cursor < end_microseconds:
        if _market_state_at_microseconds(
            cursor,
            exchange_code=exchange_code,
            calendar_registry=calendar_registry,
        ) == target_state:
            return cursor

        cursor += int(_TRANSITION_SEARCH_STEP.total_seconds() * 1_000_000)

    if _market_state_at_microseconds(
        end_microseconds,
        exchange_code=exchange_code,
        calendar_registry=calendar_registry,
    ) == target_state:
        return end_microseconds

    return None


def _last_probe_with_state(
    start_microseconds: int,
    end_microseconds: int,
    *,
    target_state: MarketState,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
) -> Optional[int]:
    cursor: int = end_microseconds - int(_TRANSITION_SEARCH_STEP.total_seconds() * 1_000_000)
    while cursor > start_microseconds:
        if _market_state_at_microseconds(
            cursor,
            exchange_code=exchange_code,
            calendar_registry=calendar_registry,
        ) == target_state:
            return cursor

        cursor -= int(_TRANSITION_SEARCH_STEP.total_seconds() * 1_000_000)

    if _market_state_at_microseconds(
        start_microseconds,
        exchange_code=exchange_code,
        calendar_registry=calendar_registry,
    ) == target_state:
        return start_microseconds

    return None


def _earliest_microsecond_with_state(
    lower_microseconds: int,
    upper_microseconds: int,
    *,
    target_state: MarketState,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
) -> int:
    lower: int = lower_microseconds
    upper: int = upper_microseconds
    while upper - lower > 1:
        midpoint: int = lower + (upper - lower) // 2
        midpoint_state: MarketState = _market_state_at_microseconds(
            midpoint,
            exchange_code=exchange_code,
            calendar_registry=calendar_registry,
        )
        if midpoint_state == target_state:
            upper = midpoint
        else:
            lower = midpoint

    return upper


def _market_state_at_microseconds(
    timestamp_microseconds: int,
    *,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
) -> MarketState:
    return calendar_registry.resolve_market_state(
        exchange_code,
        _datetime_from_epoch_microseconds(timestamp_microseconds),
    )


def _datetime_to_epoch_microseconds(timestamp_utc: datetime) -> int:
    normalized_timestamp: datetime = timestamp_utc.astimezone(UTC)
    delta: timedelta = normalized_timestamp - _EPOCH

    return (
        ((delta.days * 24 * 60 * 60) + delta.seconds) * 1_000_000
        + delta.microseconds
    )


def _datetime_from_epoch_microseconds(timestamp_microseconds: int) -> datetime:
    return _EPOCH + timedelta(microseconds=timestamp_microseconds)


def _boundary_observation_for_event(
    close_open_boundary_observations: Sequence[
        CalibrationReturnComponentsObservedCloseOpenBoundaryObservation
    ],
    *,
    event_timestamp_utc_nanos: int,
) -> Optional[CalibrationReturnComponentsObservedCloseOpenBoundaryObservation]:
    for boundary_observation in close_open_boundary_observations:
        if (
            boundary_observation.close_timestamp_utc_nanos
            <= event_timestamp_utc_nanos
            < boundary_observation.open_timestamp_utc_nanos
        ):
            return boundary_observation

    return None


def _last_open_tick_at_or_before(
    open_price_ticks: Sequence[_OpenPriceTick],
    event_timestamp: datetime,
) -> Optional[_OpenPriceTick]:
    last_tick: Optional[_OpenPriceTick] = None
    for open_price_tick in open_price_ticks:
        if open_price_tick.timestamp <= event_timestamp:
            last_tick = open_price_tick
            continue

        break

    return last_tick


def _first_open_tick_after(
    open_price_ticks: Sequence[_OpenPriceTick],
    event_timestamp: datetime,
) -> Optional[_OpenPriceTick]:
    for open_price_tick in open_price_ticks:
        if open_price_tick.timestamp > event_timestamp:
            return open_price_tick

    return None


def _is_within_known_news_exclusion_window(
    candidate_timestamp: datetime,
    retained_known_news: Sequence[_ResolvedKnownNewsRecord],
) -> bool:
    for known_news_record in retained_known_news:
        if abs((candidate_timestamp - known_news_record.record.event_timestamp).total_seconds()) <= (
            _KNOWN_NEWS_EXCLUSION_WINDOW.total_seconds()
        ):
            return True

    return False


def _activity_lift(
    anchor_timestamp: datetime,
    open_price_ticks: Sequence[_OpenPriceTick],
) -> float:
    pre_window_start: datetime = anchor_timestamp - _ACTIVITY_LIFT_WINDOW
    post_window_end: datetime = anchor_timestamp + _ACTIVITY_LIFT_WINDOW
    pre_event_count: int = sum(
        1
        for open_price_tick in open_price_ticks
        if pre_window_start <= open_price_tick.timestamp < anchor_timestamp
    )
    post_event_count: int = sum(
        1
        for open_price_tick in open_price_ticks
        if anchor_timestamp <= open_price_tick.timestamp < post_window_end
    )

    if pre_event_count == 0:
        return 1.0

    return round(post_event_count / pre_event_count, 6)


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


def _datetime_to_utc_nanos(timestamp_utc: datetime) -> int:
    normalized_timestamp: datetime = timestamp_utc.astimezone(UTC)
    delta: timedelta = normalized_timestamp - _EPOCH

    return (
        ((delta.days * 24 * 60 * 60) + delta.seconds) * 1_000_000_000
        + delta.microseconds * 1_000
    )


def _observation_window_hours(timestamps: Sequence[datetime]) -> float:
    if len(timestamps) < 2:
        return 1.0 / 60.0

    start_timestamp: datetime = timestamps[0]
    end_timestamp: datetime = timestamps[-1]
    duration_seconds: float = max((end_timestamp - start_timestamp).total_seconds(), 60.0)

    return duration_seconds / 3600.0


def _adjacent_durations_seconds(timestamps: Sequence[datetime]) -> list[float]:
    durations_seconds: list[float] = []
    for previous_timestamp, current_timestamp in zip(timestamps, timestamps[1:]):
        durations_seconds.append(
            max((current_timestamp - previous_timestamp).total_seconds(), 0.0)
        )

    return durations_seconds


def _mean(values: Sequence[float]) -> Optional[float]:
    if len(values) == 0:
        return None

    return sum(values) / len(values)


def _coefficient_of_variation(values: Sequence[float]) -> float:
    mean_value: Optional[float] = _mean(values)
    if mean_value is None or mean_value == 0.0:
        return 0.0

    variance: float = sum((value - mean_value) ** 2 for value in values) / len(values)
    return sqrt(variance) / mean_value


def _median_abs(values: Sequence[float]) -> float:
    if len(values) == 0:
        return 0.0

    sorted_values: list[float] = sorted(abs(value) for value in values)
    midpoint: int = len(sorted_values) // 2
    if len(sorted_values) % 2 == 1:
        return sorted_values[midpoint]

    return (sorted_values[midpoint - 1] + sorted_values[midpoint]) / 2.0
