from __future__ import annotations

from collections.abc import Callable, Mapping
from datetime import UTC
from time import perf_counter
from typing import Any, Optional

from .contracts import (
    SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT,
    SeededTickSimulationRequest,
)
from .response import SeededTickSimulationResponse
from ..shared import SeededTickSimulationValidationError
from ..shared.errors import (
    SIMULATION_ERROR_FACTORY,
    SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
    _simulation_error,
    _simulation_execution_error,
    _simulation_integration_error,
)
from ...contracts.base import require_datetime_utc
from ..._native_api import native_exception_matches, native_module
from ..._native_progress import native_terminal_progress_mode
from ..._workflow_progress import WorkflowProgressMode, resolve_workflow_progress_mode


def run_seeded_tick_event_simulation(
    request: SeededTickSimulationRequest,
    *,
    progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
) -> SeededTickSimulationResponse:
    if not isinstance(request, SeededTickSimulationRequest):
        raise _simulation_error("request", "must be a SeededTickSimulationRequest")

    normalized_request = SeededTickSimulationRequest(
        seed=request.seed,
        start_time_utc=require_datetime_utc(
            request.start_time_utc,
            "start_time_utc",
            SIMULATION_ERROR_FACTORY,
        ),
        horizon_interval_count=request.horizon_interval_count,
        kernel_parameters=request.kernel_parameters,
        nig_shared_plus_shift_configuration=request.nig_shared_plus_shift_configuration,
        news_process_configuration=request.news_process_configuration,
        initial_excitation_state=request.initial_excitation_state,
        mean_reversion_fitted_parameters=request.mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=request.mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=request.initial_mean_reversion_boundary_state,
        interval_factor_grid_summary=request.interval_factor_grid_summary,
        return_clock_exposure_parameters=request.return_clock_exposure_parameters,
        long_variance_parameters=request.long_variance_parameters,
        initial_long_variance_state=request.initial_long_variance_state,
        observed_price_tick_size=request.observed_price_tick_size,
        debug_trace_enabled=request.debug_trace_enabled,
        sample_every_nth_transition=request.sample_every_nth_transition,
        max_records_per_chunk=request.max_records_per_chunk,
        state_snapshot_capture=request.state_snapshot_capture,
        _interval_factor_grid=request.interval_factor_grid,
    )

    try:
        native = native_module()
    except ModuleNotFoundError as e:
        raise _simulation_integration_error(
            "native simulation module is unavailable",
            entrypoint="run_seeded_tick_event_simulation",
            error_id=SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    try:
        native_run_started = perf_counter()
        native_run_kwargs: dict[str, Any] = {}

        if progress_callback is not None:
            native_run_kwargs["event_callback"] = progress_callback
        else:
            resolved_progress_mode = resolve_workflow_progress_mode(
                progress_mode,
                default_mode=WorkflowProgressMode.TERMINAL,
            )

            native_run_kwargs["progress_mode"] = native_terminal_progress_mode(
                resolved_progress_mode
            )

        native_result: Any = native.run_seeded_tick_event_simulation(
            {
                "seed": normalized_request.seed,
                "start_time_utc": normalized_request.start_time_utc.astimezone(UTC).isoformat(),
                "horizon_interval_count": normalized_request.horizon_interval_count,
                "kernel_parameters": normalized_request.kernel_parameters.to_payload(),
                "nig_shared_plus_shift_configuration": (
                    normalized_request.nig_shared_plus_shift_configuration.to_payload()
                ),
                "initial_excitation_state": normalized_request.initial_excitation_state.to_payload(),
                "interval_factor_grid": normalized_request.interval_factor_grid._native_result,
                "news_process_configuration": normalized_request.news_process_configuration.to_payload(),
                "mean_reversion_fitted_parameters": (
                    normalized_request.mean_reversion_fitted_parameters.to_payload()
                ),
                "mean_reversion_runtime_interval_anchors": (
                    normalized_request.mean_reversion_runtime_interval_anchors.to_payload()
                ),
                "initial_mean_reversion_boundary_state": (
                    normalized_request.initial_mean_reversion_boundary_state.to_payload()
                ),
                "return_clock_exposure_parameters": (
                    normalized_request.return_clock_exposure_parameters.to_payload()
                ),
                "long_variance_parameters": normalized_request.long_variance_parameters.to_payload(),
                "initial_long_variance_state": (
                    normalized_request.initial_long_variance_state.to_payload()
                ),
                "observed_price_tick_size": normalized_request.observed_price_tick_size,
                "debug_trace_enabled": normalized_request.debug_trace_enabled,
                "sample_every_nth_transition": normalized_request.sample_every_nth_transition,
                "max_records_per_chunk": normalized_request.max_records_per_chunk,
                "state_snapshot_capture": normalized_request.state_snapshot_capture.value,
            },
            **native_run_kwargs,
        )

        native_runtime_seconds = perf_counter() - native_run_started
    except Exception as e:
        if native_exception_matches(native, e, "SeasonalityInputValidationError"):
            raise _simulation_execution_error(
                "native simulation rejected the normalized run request",
                entrypoint="run_seeded_tick_event_simulation",
                context={"source_category": "native_validation"},
            ) from e

        raise _simulation_execution_error(
            "seeded tick simulation failed during native execution",
            entrypoint="run_seeded_tick_event_simulation",
            context={"source_exception_type": type(e).__name__},
        ) from e

    try:
        response_build_started = perf_counter()

        response = SeededTickSimulationResponse.from_native_result(
            native_result=native_result,
            native_runtime_seconds=native_runtime_seconds,
            python_response_construction_seconds=0.0,
            chunk_record_count=SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT,
        )
        response.diagnostics.python_response_construction_seconds = (
            perf_counter() - response_build_started
        )

        return response
    except SeededTickSimulationValidationError as e:
        raise _simulation_integration_error(
            "native simulation returned an invalid response payload",
            entrypoint="run_seeded_tick_event_simulation",
            error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e
    except Exception as e:
        raise _simulation_integration_error(
            "native simulation returned an unreadable response handle",
            entrypoint="run_seeded_tick_event_simulation",
            error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_exception_type": type(e).__name__},
        ) from e


__all__ = [
    "run_seeded_tick_event_simulation",
]
