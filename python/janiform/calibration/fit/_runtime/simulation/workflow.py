from __future__ import annotations

from collections.abc import Callable, Mapping
from dataclasses import asdict
from datetime import datetime, timedelta
from pathlib import Path
from time import perf_counter
from typing import TYPE_CHECKING, Any, Optional, cast

from ....._native_api import native_module
from ....._native_progress import native_terminal_progress_mode
from ....._timestamps import datetime_to_utc_nanos
from ....._workflow_progress import WorkflowProgressMode, resolve_workflow_progress_mode
from ....._workflow_outputs import (
    SimulationArtifactSink,
    _file_format_for_simulation_path,
    _resolve_output_path,
    generate_workflow_run_id,
    persist_seeded_tick_simulation,
    simulation_output_preferences_from_model,
    simulation_output_price_column,
    simulation_output_side_column,
)
from .....calendar import CalendarRegistry
from .....mean_reversion import MeanReversionBoundaryState
from .....simulation.coarse import CoarseSimulationPath
from .....simulation.kinds import SimulationKind
from .....simulation.result import JaniformSimulationResult, normalize_interval_label
from .....simulation.seeded import SeededTickSimulationResponse
from .....simulation.shared import HawkesExcitationState, SeededTickEventStateSnapshotCapture
from ....errors import calibration_error
from ...state import CurrentStateEstimationRequest
from ..payloads import current_state_tick_data_column_mapping_for_model
from ..shared import DEFAULT_SIMULATION_INTERVAL

if TYPE_CHECKING:
    from .....model import JaniformModel


def simulate_model(
    model: JaniformModel,
    *,
    seed: int,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: Optional[str | timedelta] = None,
    coarse: bool = False,
    output_path: Optional[str | Path] = None,
    sink: Optional[SimulationArtifactSink] = None,
    continuation_data: Optional[CurrentStateEstimationRequest] = None,
    initial_excitation_state: Optional[HawkesExcitationState] = None,
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState] = None,
    debug_trace_enabled: bool = False,
    sample_every_nth_transition: int = 1,
    max_records_per_chunk: int = 1024,
    state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED,
    progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
) -> JaniformSimulationResult:
    if output_path is not None and sink is not None:
        raise calibration_error("sink", "output_path and sink are mutually exclusive")

    effective_interval = DEFAULT_SIMULATION_INTERVAL if interval is None else interval
    simulation_kind = SimulationKind.COARSE_INTERVAL if coarse else SimulationKind.SEEDED_TICK

    preferences = simulation_output_preferences_from_model(model)
    simulation_run_id = generate_workflow_run_id(f"{model.run_id}_simulate")

    resolved_output_path = _resolve_native_output_path(
        run_id=simulation_run_id,
        output_path=output_path,
        interval=effective_interval if coarse else None,
        default_file_format=preferences.file_format,
    )

    request_payload = _build_simulation_from_model_artifact_request(
        model,
        run_id=simulation_run_id,
        seed=seed,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        interval=effective_interval,
        simulation_kind=simulation_kind,
        output_path=resolved_output_path,
        continuation_data=continuation_data,
        initial_excitation_state=initial_excitation_state,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
        debug_trace_enabled=debug_trace_enabled,
        sample_every_nth_transition=sample_every_nth_transition,
        max_records_per_chunk=max_records_per_chunk,
        state_snapshot_capture=state_snapshot_capture,
    )

    raw_output = native_module().run_simulation_from_model_artifact(
        request_payload,
        progress_callback,
        progress_mode=(
            "none"
            if progress_callback is not None
            else native_terminal_progress_mode(
                resolve_workflow_progress_mode(
                    progress_mode,
                    default_mode=WorkflowProgressMode.TERMINAL,
                )
            )
        ),
    )

    simulation_result = _adapt_native_simulation_output(
        raw_output,
        simulation_kind=simulation_kind,
        max_records_per_chunk=max_records_per_chunk,
    )

    result = build_simulation_result(
        model,
        simulation_result=simulation_result,
        preferences=preferences,
    )

    if sink is not None:
        if result.simulation_kind != SimulationKind.SEEDED_TICK:
            raise calibration_error("sink", "sink output is currently supported only for seeded tick simulation")

        persist_seeded_tick_simulation(
            simulation_result=cast(SeededTickSimulationResponse, result.raw_result),
            output_path=Path("simulation.csv"),
            preferences=preferences,
            sink=sink,
        )

    return result


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


def _build_simulation_from_model_artifact_request(
    model: JaniformModel,
    *,
    run_id: str,
    seed: int,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: str | timedelta,
    simulation_kind: SimulationKind,
    output_path: Optional[Path],
    continuation_data: Optional[CurrentStateEstimationRequest],
    initial_excitation_state: Optional[HawkesExcitationState],
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState],
    debug_trace_enabled: bool,
    sample_every_nth_transition: int,
    max_records_per_chunk: int,
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
) -> dict[str, Any]:
    preferences = simulation_output_preferences_from_model(model)

    side_column = (
        None
        if simulation_kind == SimulationKind.COARSE_INTERVAL
        else simulation_output_side_column(preferences)
    )

    return {
        "run_id": run_id,
        "model_payload": model._native_model_payload(),
        "simulation_kind": simulation_kind.value,
        "seed": seed,
        "start_time_utc": start_time_utc.isoformat(),
        "horizon_interval_count": horizon_interval_count,
        "interval_duration_nanos": _interval_duration_nanos(interval),
        "output_path": None if output_path is None else str(output_path),
        "output_columns": {
            "timestamp_column": preferences.timestamp_column,
            "price_column": simulation_output_price_column(preferences),
            "side_column": side_column,
        },
        "reference_price": preferences.reference_price,
        "continuation_data": (
            None
            if continuation_data is None
            else _build_continuation_data_payload(model, continuation_data=continuation_data)
        ),
        "initial_excitation_state": (
            None if initial_excitation_state is None else initial_excitation_state.to_payload()
        ),
        "initial_mean_reversion_boundary_state": (
            None
            if initial_mean_reversion_boundary_state is None
            else initial_mean_reversion_boundary_state.to_payload()
        ),
        "debug_trace_enabled": debug_trace_enabled,
        "sample_every_nth_transition": sample_every_nth_transition,
        "max_records_per_chunk": max_records_per_chunk,
        "state_snapshot_capture": state_snapshot_capture.value,
    }


def _build_continuation_data_payload(
    model: JaniformModel,
    *,
    continuation_data: CurrentStateEstimationRequest,
) -> dict[str, Any]:
    from ...state._estimation import _extract_observed_tick_data_columns

    column_mapping = current_state_tick_data_column_mapping_for_model(
        model,
        request=continuation_data,
    )

    (
        observed_timestamps_utc,
        observed_trade_prices,
        observed_trade_sides,
    ) = _extract_observed_tick_data_columns(
        continuation_data.observed_tick_data,
        column_mapping=column_mapping,
    )

    return {
        "observation_timestamp_utc_nanos": tuple(
            datetime_to_utc_nanos(datetime.fromisoformat(timestamp.replace("Z", "+00:00")))
            for timestamp in observed_timestamps_utc
        ),
        "observation_trade_prices": observed_trade_prices,
        "observation_trade_sides": observed_trade_sides,
        "observed_price": continuation_data.observed_price,
    }


def _adapt_native_simulation_output(
    raw_output: dict[str, Any],
    *,
    simulation_kind: SimulationKind,
    max_records_per_chunk: int,
) -> CoarseSimulationPath | SeededTickSimulationResponse:
    if simulation_kind == SimulationKind.SEEDED_TICK:
        response_build_started = perf_counter()

        response = SeededTickSimulationResponse.from_native_result(
            native_result=raw_output["native_result"],
            native_runtime_seconds=float(raw_output["execution_duration_seconds"]),
            python_response_construction_seconds=0.0,
            chunk_record_count=max_records_per_chunk,
        )

        response.diagnostics.python_response_construction_seconds = perf_counter() - response_build_started

        return response

    return CoarseSimulationPath.from_mapping(dict(raw_output["raw_result_payload"]))


def _resolve_native_output_path(
    *,
    run_id: str,
    output_path: Optional[str | Path],
    interval: Optional[str | timedelta],
    default_file_format: str,
) -> Optional[Path]:
    if output_path is None:
        return None

    interval_label = None if interval is None else normalize_interval_label(interval)

    return _resolve_output_path(
        run_id=run_id,
        output_path=output_path,
        default_stem="simulation" if interval_label is None else f"simulation_{interval_label}",
        default_file_format=_file_format_for_simulation_path(Path(output_path), default_file_format),
    )


def _interval_duration_nanos(interval: str | timedelta) -> int:
    from .....interval_factor_grids.workflow import _parse_interval_duration_nanos

    return _parse_interval_duration_nanos(interval, field_name="interval")
