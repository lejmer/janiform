from __future__ import annotations

from dataclasses import asdict
from datetime import datetime, timedelta
from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional, cast

from ....._timestamps import ONE_HOUR_NANOS
from ....._workflow_outputs import (
    SimulationArtifactSink,
    generate_workflow_run_id,
    persist_seeded_tick_simulation,
    simulation_output_preferences_from_model,
    write_simulation_artifacts,
)
from ....._workflow_progress import JaniformOutputController, WorkflowProgressMode
from .....calendar import CalendarRegistry, MarketState
from .....hourly_factors import (
    HourlyFactorContract,
    HourlyNewsIntensityProviderMaterializedSource,
    provide_hourly_factor_contract,
    load_hourly_factor_contract_payload,
)
from .....mean_reversion import MeanReversionBoundaryState
from .....sufficient_statistics import SufficientStatisticsEquationSpecification, SufficientStatisticsModelInputs
from .....simulation.coarse import CoarseSimulationPath, CoarseSimulationRunRequest, run_coarse_simulation
from .....simulation.kinds import SimulationKind
from .....simulation.progress import SimulationProgressEvent
from .....simulation.seeded import (
    SeededTickSimulationRequest,
    SeededTickSimulationResponse,
    run_seeded_tick_event_simulation,
)
from .....simulation.shared import HawkesExcitationState, SeededTickEventStateSnapshotCapture
from ....errors import calibration_error
from ...state import CurrentStateEstimationRequest, CurrentStateEstimationResult
from ..payloads import (
    prepare_model_runtime_factors_for_model,
    resolved_known_news_event_timestamps,
    surprise_baseline_intensity,
)
from ..seasonality import (
    build_runtime_hourly_seasonality_source_from_model,
    surprise_deterministic_time_multiplier_by_interval_index,
)
from ..mean_reversion import prepare_model_mean_reversion_runtime_inputs
from ..state import estimate_model_state_from_observed_data
from ..shared import (
    DEFAULT_SIMULATION_INTERVAL,
    PreparedModelSimulationInputs,
    PreparedModelSimulationRuntimeInput,
    ResolvedModelSimulationInitialState,
)
from .result import JaniformSimulationResult, normalize_interval_label, runtime_datetime_from_utc_nanos

if TYPE_CHECKING:
    from ...model import JaniformModel


def simulate_model(
    model: JaniformModel,
    *,
    seed: int,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: Optional[str | timedelta] = None,
    output_path: Optional[str | Path] = None,
    sink: Optional[SimulationArtifactSink] = None,
    continuation_data: Optional[CurrentStateEstimationRequest] = None,
    initial_excitation_state: Optional[HawkesExcitationState] = None,
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState] = None,
    debug_trace_enabled: bool = False,
    sample_every_nth_transition: int = 1,
    max_records_per_chunk: int = 1024,
    state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED,
    silent: bool = False,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    progress_callback: Optional[Any] = None,
) -> JaniformSimulationResult:
    if output_path is not None and sink is not None:
        raise calibration_error("sink", "output_path and sink are mutually exclusive")

    effective_interval = DEFAULT_SIMULATION_INTERVAL if interval is None else interval
    output_controller = JaniformOutputController(
        suppress_info_output=silent,
        progress_mode=WorkflowProgressMode.NONE if silent else progress_mode,
        default_progress_mode=(WorkflowProgressMode.NONE if silent else WorkflowProgressMode.HUMAN),
    )
    simulation_kind = SimulationKind.SEEDED_TICK if interval is None else SimulationKind.COARSE_INTERVAL

    execution_inputs = prepare_model_simulation_inputs(
        model,
        seed=seed,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        interval=effective_interval,
        simulation_kind=simulation_kind,
        continuation_data=continuation_data,
        initial_excitation_state=initial_excitation_state,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
        debug_trace_enabled=debug_trace_enabled,
        sample_every_nth_transition=sample_every_nth_transition,
        max_records_per_chunk=max_records_per_chunk,
        state_snapshot_capture=state_snapshot_capture,
    )

    with output_controller.progress("simulate") as progress:

        def emit_simulation_progress(event: SimulationProgressEvent) -> None:
            progress.update(
                detail=event.detail,
                completed_units=event.completed_units,
                total_units=event.total_units,
            )
            if progress_callback is not None:
                progress_callback(event)

        if simulation_kind == SimulationKind.SEEDED_TICK:
            if execution_inputs.seeded_request is None:
                raise RuntimeError("JaniformModel seeded simulation inputs were not prepared")
            raw_simulation_result = run_seeded_tick_event_simulation(
                execution_inputs.seeded_request,
                progress_callback=emit_simulation_progress,
            )
        else:
            if execution_inputs.coarse_request is None:
                raise RuntimeError("JaniformModel coarse simulation inputs were not prepared")
            raw_simulation_result = run_coarse_simulation(
                execution_inputs.coarse_request,
                progress_callback=emit_simulation_progress,
            )

    preferences = simulation_output_preferences_from_model(model)
    simulation_result = build_simulation_result(
        model,
        simulation_result=raw_simulation_result,
        preferences=preferences,
    )

    if sink is not None:
        if simulation_result.simulation_kind != SimulationKind.SEEDED_TICK:
            raise calibration_error("sink", "sink output is currently supported only for seeded tick simulation")
        persist_seeded_tick_simulation(
            simulation_result=cast(SeededTickSimulationResponse, simulation_result.raw_result),
            output_path=Path("simulation.csv"),
            preferences=preferences,
            sink=sink,
        )
    elif output_path is not None:
        simulation_run_id = generate_workflow_run_id(f"{model.run_id}_simulate")
        write_simulation_artifacts(
            model=model,
            simulation_result=raw_simulation_result,
            output_path=output_path,
            simulation_run_id=simulation_run_id,
            interval_label=None if interval is None else normalize_interval_label(effective_interval),
        )

    return simulation_result


def build_simulation_result(
    model: JaniformModel,
    *,
    simulation_result: CoarseSimulationPath | SeededTickSimulationResponse,
    preferences: Any,
) -> JaniformSimulationResult:
    if isinstance(simulation_result, SeededTickSimulationResponse):
        return JaniformSimulationResult(
            simulation_kind=SimulationKind.SEEDED_TICK,
            metadata=simulation_result.metadata.to_payload(),
            provenance={
                "model_run_id": model.run_id,
                "event_tape_metadata": simulation_result.event_tape_metadata.to_payload(),
                "simulation_output_preferences": asdict(preferences),
            },
            diagnostics=simulation_result.diagnostics.to_payload(),
            raw_result=simulation_result,
            _preferences=preferences,
        )

    result = JaniformSimulationResult(
        simulation_kind=SimulationKind.COARSE_INTERVAL,
        metadata=simulation_result.output_schema_metadata.to_payload(),
        provenance={
            "model_run_id": model.run_id,
            "simulation_output_preferences": asdict(preferences),
            **simulation_result.provenance.to_payload(),
        },
        diagnostics=simulation_result.diagnostics.to_payload(),
        raw_result=simulation_result,
        _preferences=preferences,
    )
    result._calendar_registry = CalendarRegistry(model.session_resolution.resolved_session_definition_json)
    result._exchange_code = model.session_resolution.exchange_code
    return result


def prepare_model_simulation_inputs(
    model: JaniformModel,
    *,
    seed: int,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: str | timedelta,
    simulation_kind: SimulationKind,
    continuation_data: Optional[CurrentStateEstimationRequest] = None,
    initial_excitation_state: Optional[HawkesExcitationState],
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState] = None,
    debug_trace_enabled: bool,
    sample_every_nth_transition: int,
    max_records_per_chunk: int,
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
) -> PreparedModelSimulationInputs:
    runtime_factors = prepare_model_runtime_factors_for_model(
        model,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        interval=interval,
    )
    initial_state_resolution = resolve_model_simulation_initial_state(
        model,
        start_time_utc=start_time_utc,
        continuation_data=continuation_data,
        default_initial_excitation_state=runtime_factors.kernel_parameters.stationary_excitation_state(),
        initial_excitation_state=initial_excitation_state,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )

    coarse_request: Optional[CoarseSimulationRunRequest] = None
    seeded_request: Optional[SeededTickSimulationRequest] = None
    match simulation_kind:
        case SimulationKind.COARSE_INTERVAL:
            coarse_request = prepare_model_coarse_interval_simulation_request(
                model,
                seed=seed,
                start_time_utc=start_time_utc,
                horizon_interval_count=horizon_interval_count,
                interval=interval,
                runtime_factors=runtime_factors,
                initial_excitation_state=initial_state_resolution.excitation_state,
                start_anchor_value=initial_state_resolution.start_anchor_value,
                initial_mean_reversion_boundary_state=initial_state_resolution.mean_reversion_boundary_state,
            )
        case SimulationKind.SEEDED_TICK:
            seeded_request = prepare_model_seeded_tick_simulation_request(
                model,
                seed=seed,
                start_time_utc=start_time_utc,
                horizon_interval_count=horizon_interval_count,
                interval=interval,
                runtime_factors=runtime_factors,
                initial_excitation_state=initial_state_resolution.excitation_state,
                start_anchor_value=initial_state_resolution.start_anchor_value,
                initial_mean_reversion_boundary_state=initial_state_resolution.mean_reversion_boundary_state,
                debug_trace_enabled=debug_trace_enabled,
                sample_every_nth_transition=sample_every_nth_transition,
                max_records_per_chunk=max_records_per_chunk,
                state_snapshot_capture=state_snapshot_capture,
            )
        case _:
            raise RuntimeError("JaniformModel simulate must prepare either `seeded_tick` or `coarse_interval` inputs")

    return PreparedModelSimulationInputs(coarse_request=coarse_request, seeded_request=seeded_request)


def prepare_model_coarse_interval_simulation_request(
    model: JaniformModel,
    *,
    seed: int,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: str | timedelta,
    runtime_factors: Any,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: float,
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState],
) -> CoarseSimulationRunRequest:
    runtime_input = prepare_model_simulation_runtime_input(
        model,
        start_time_utc=start_time_utc,
        interval=interval,
        runtime_factors=runtime_factors,
        initial_excitation_state=initial_excitation_state,
        start_anchor_value=start_anchor_value,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )
    return CoarseSimulationRunRequest.from_runtime_inputs(
        seed=seed,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        interval_delta=runtime_input.runtime_factors.interval_delta,
        sufficient_statistics_model_inputs=SufficientStatisticsModelInputs(
            equation_specification=SufficientStatisticsEquationSpecification(
                hawkes_kernel_parameters=runtime_input.runtime_factors.kernel_parameters,
                gamma_severity_mark_configuration=runtime_input.runtime_factors.gamma_severity_mark_configuration,
            ),
            nig_shared_plus_shift_configuration=runtime_input.runtime_factors.nig_shared_plus_shift_configuration,
            initial_excitation_state=runtime_input.initial_excitation_state,
            mean_reversion_fitted_parameters=runtime_input.mean_reversion_fitted_parameters,
            mean_reversion_runtime_interval_anchors=runtime_input.mean_reversion_runtime_interval_anchors,
            initial_mean_reversion_boundary_state=runtime_input.initial_mean_reversion_boundary_state,
        ),
        hourly_factor_contract=zero_closed_coarse_interval_seasonality_factors(
            runtime_input.hourly_factor_contract,
            calendar_registry=runtime_input.runtime_factors.calendar_registry,
            exchange_code=runtime_input.runtime_factors.exchange_code,
        ),
        news_process_configuration=runtime_input.runtime_factors.news_process_configuration,
        calendar_registry=runtime_input.runtime_factors.calendar_registry,
        exchange_code=runtime_input.runtime_factors.exchange_code,
    )


def prepare_model_seeded_tick_simulation_request(
    model: JaniformModel,
    *,
    seed: int,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: str | timedelta,
    runtime_factors: Any,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: float,
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState],
    debug_trace_enabled: bool,
    sample_every_nth_transition: int,
    max_records_per_chunk: int,
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
) -> SeededTickSimulationRequest:
    runtime_input = prepare_model_simulation_runtime_input(
        model,
        start_time_utc=start_time_utc,
        interval=interval,
        runtime_factors=runtime_factors,
        initial_excitation_state=initial_excitation_state,
        start_anchor_value=start_anchor_value,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )
    return SeededTickSimulationRequest.from_runtime_inputs(
        seed=seed,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        kernel_parameters=runtime_input.runtime_factors.kernel_parameters,
        nig_shared_plus_shift_configuration=runtime_input.runtime_factors.nig_shared_plus_shift_configuration,
        news_process_configuration=runtime_input.runtime_factors.news_process_configuration,
        initial_excitation_state=runtime_input.initial_excitation_state,
        mean_reversion_fitted_parameters=runtime_input.mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=runtime_input.mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=runtime_input.initial_mean_reversion_boundary_state,
        hourly_factor_contract=runtime_input.hourly_factor_contract,
        debug_trace_enabled=debug_trace_enabled,
        sample_every_nth_transition=sample_every_nth_transition,
        max_records_per_chunk=max_records_per_chunk,
        state_snapshot_capture=state_snapshot_capture,
    )


def prepare_model_simulation_runtime_input(
    model: JaniformModel,
    *,
    start_time_utc: datetime,
    interval: str | timedelta,
    runtime_factors: Any,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: float,
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState],
) -> PreparedModelSimulationRuntimeInput:
    hourly_factor_contract = build_model_simulation_hourly_factor_contract(
        model,
        runtime_factors=runtime_factors,
        start_time_utc=start_time_utc,
        interval=interval,
    )
    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        derived_initial_mean_reversion_boundary_state,
    ) = prepare_model_mean_reversion_runtime_inputs(
        model,
        runtime_factors=runtime_factors,
        hourly_factor_contract=hourly_factor_contract,
        initial_excitation_state=initial_excitation_state,
        start_anchor_value=start_anchor_value,
    )

    return PreparedModelSimulationRuntimeInput(
        runtime_factors=runtime_factors,
        hourly_factor_contract=hourly_factor_contract,
        initial_excitation_state=initial_excitation_state,
        initial_mean_reversion_boundary_state=(
            derived_initial_mean_reversion_boundary_state
            if initial_mean_reversion_boundary_state is None
            else initial_mean_reversion_boundary_state
        ),
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
    )


def build_model_simulation_hourly_factor_contract(
    model: JaniformModel,
    *,
    runtime_factors: Any,
    start_time_utc: datetime,
    interval: str | timedelta,
) -> HourlyFactorContract:
    return provide_hourly_factor_contract(
        calendar_registry=runtime_factors.calendar_registry,
        exchange_code=runtime_factors.exchange_code,
        range_start_utc=start_time_utc,
        range_end_utc=runtime_factors.range_end_utc,
        interval=interval,
        seasonality_source=build_runtime_hourly_seasonality_source_from_model(
            model,
            calendar_registry=runtime_factors.calendar_registry,
            exchange_code=runtime_factors.exchange_code,
            range_start_utc=start_time_utc,
            range_end_utc=runtime_factors.range_end_utc,
            interval=interval,
        ),
        news_intensity_source=HourlyNewsIntensityProviderMaterializedSource(
            known_scheduled_event_timestamps_utc=resolved_known_news_event_timestamps(model),
            surprise_baseline_intensity=surprise_baseline_intensity(model),
            surprise_deterministic_time_multiplier_by_interval_index=(
                surprise_deterministic_time_multiplier_by_interval_index(
                    model,
                    range_start_utc=start_time_utc,
                    range_end_utc=runtime_factors.range_end_utc,
                    interval_duration_nanos=runtime_factors.interval_duration_nanos,
                )
            ),
        ),
    )


def resolve_model_simulation_initial_state(
    model: JaniformModel,
    *,
    start_time_utc: datetime,
    continuation_data: Optional[CurrentStateEstimationRequest],
    default_initial_excitation_state: HawkesExcitationState,
    initial_excitation_state: Optional[HawkesExcitationState],
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState],
) -> ResolvedModelSimulationInitialState:
    estimated_current_state: Optional[CurrentStateEstimationResult] = None
    if continuation_data is not None and (
        initial_excitation_state is None or initial_mean_reversion_boundary_state is None
    ):
        estimated_current_state = estimate_model_state_from_observed_data(
            model,
            state_time_utc=start_time_utc,
            request=continuation_data,
        )

    effective_initial_excitation_state = (
        default_initial_excitation_state
        if initial_excitation_state is None and estimated_current_state is None
        else (
            cast(CurrentStateEstimationResult, estimated_current_state).hawkes_excitation_state
            if initial_excitation_state is None
            else initial_excitation_state
        )
    )
    effective_initial_mean_reversion_boundary_state = (
        initial_mean_reversion_boundary_state
        if initial_mean_reversion_boundary_state is not None
        else (None if estimated_current_state is None else estimated_current_state.mean_reversion_boundary_state)
    )
    start_anchor_value = (
        0.0
        if effective_initial_mean_reversion_boundary_state is None
        else effective_initial_mean_reversion_boundary_state.anchor_value
    )

    return ResolvedModelSimulationInitialState(
        excitation_state=effective_initial_excitation_state,
        mean_reversion_boundary_state=effective_initial_mean_reversion_boundary_state,
        start_anchor_value=start_anchor_value,
    )


def zero_closed_coarse_interval_seasonality_factors(
    hourly_factor_contract: HourlyFactorContract,
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
) -> HourlyFactorContract:
    if hourly_factor_contract.interval_duration_nanos >= ONE_HOUR_NANOS:
        return hourly_factor_contract

    payload = hourly_factor_contract.to_payload()
    gated_seasonality: list[float] = []
    contains_closed_interval = False
    for interval_start_utc_nanos, seasonality_factor in zip(
        payload["interval_start_utc_nanos"],
        payload["s_h"],
        strict=True,
    ):
        if (
            calendar_registry.resolve_market_state(
                exchange_code,
                runtime_datetime_from_utc_nanos(interval_start_utc_nanos),
            )
            == MarketState.OPEN
        ):
            gated_seasonality.append(seasonality_factor)
            continue
        gated_seasonality.append(0.0)
        contains_closed_interval = True

    if not contains_closed_interval:
        return hourly_factor_contract

    payload["s_h"] = gated_seasonality
    return load_hourly_factor_contract_payload(payload)
