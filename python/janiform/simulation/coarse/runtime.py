from __future__ import annotations

from collections.abc import Callable
from typing import Any, Optional

from .path import (
    CoarseSimulationPath,
    CoarseSimulationRunRequest,
    validate_coarse_simulation_path,
)
from ..progress import SimulationProgressEvent, simulation_progress_event_from_mapping
from ..shared import SeededTickSimulationValidationError
from ..shared.errors import (
    SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
    _simulation_error,
    _simulation_execution_error,
    _simulation_integration_error,
)
from ..._native_api import native_exception_matches, native_module


def run_coarse_simulation(
    request: CoarseSimulationRunRequest,
    *,
    progress_callback: Optional[Callable[[SimulationProgressEvent], None]] = None,
) -> CoarseSimulationPath:
    if not isinstance(request, CoarseSimulationRunRequest):
        raise _simulation_error("request", "must be a CoarseSimulationRunRequest")

    normalized_request = request

    try:
        native = native_module()
    except ModuleNotFoundError as e:
        raise _simulation_integration_error(
            "native simulation module is unavailable",
            entrypoint="run_coarse_simulation",
            error_id=SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    try:
        def native_progress_callback(raw_event: dict[str, Any]) -> None:
            if progress_callback is None:
                return

            progress_callback(simulation_progress_event_from_mapping(raw_event))

        native_run_kwargs: dict[str, Any] = {}
        if progress_callback is not None:
            native_run_kwargs["event_callback"] = native_progress_callback

        raw_path = native.run_coarse_simulation(
            normalized_request.seed,
            normalized_request.start_time_utc_nanos,
            normalized_request.horizon_interval_count,
            normalized_request.interval_delta,
            normalized_request.model_inputs.to_payload(),
            normalized_request.hourly_factor_contract._native_result,
            **native_run_kwargs,
        )
    except Exception as e:
        if native_exception_matches(native, e, "SimulationExecutionError"):
            raise _simulation_execution_error(
                "coarse simulation failed during native execution",
                entrypoint="run_coarse_simulation",
                context={
                    "source_exception_type": type(e).__name__,
                    "source_exception_message": str(e),
                },
            ) from e

        raise _simulation_execution_error(
            "coarse simulation failed during native execution",
            entrypoint="run_coarse_simulation",
            context={
                "source_exception_type": type(e).__name__,
                "source_exception_message": str(e),
            },
        ) from e

    if not isinstance(raw_path, dict):
        raise _simulation_integration_error(
            "native coarse simulation returned an invalid response payload",
            entrypoint="run_coarse_simulation",
            error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_path).__name__},
        )

    try:
        return validate_coarse_simulation_path(raw_path)
    except SeededTickSimulationValidationError as e:
        raise _simulation_integration_error(
            "native coarse simulation returned an invalid response payload",
            entrypoint="run_coarse_simulation",
            error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e


__all__ = ["run_coarse_simulation"]
