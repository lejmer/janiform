from __future__ import annotations

from collections.abc import Callable, Mapping
from typing import Any, Optional

from .path import (
    CoarseSimulationPath,
    CoarseSimulationRunRequest,
    validate_coarse_simulation_path,
)
from ..shared import SeededTickSimulationValidationError
from ..shared.errors import (
    SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
    _simulation_error,
    _simulation_execution_error,
    _simulation_integration_error,
)
from ..._native_api import native_exception_matches, native_module
from ..._native_progress import native_terminal_progress_mode
from ..._workflow_progress import WorkflowProgressMode, resolve_workflow_progress_mode


def run_coarse_simulation(
    request: CoarseSimulationRunRequest,
    *,
    progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
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
        native_run_kwargs: dict[str, Any] = {}

        if progress_callback is not None:
            native_run_kwargs["event_callback"] = progress_callback
        else:
            resolved_progress_mode = resolve_workflow_progress_mode(
                progress_mode,
                default_mode=WorkflowProgressMode.HUMAN,
            )

            native_run_kwargs["progress_mode"] = native_terminal_progress_mode(
                resolved_progress_mode
            )

        raw_path = native.run_coarse_simulation(
            {
                "seed": normalized_request.seed,
                "start_time_utc_nanos": normalized_request.start_time_utc_nanos,
                "horizon_interval_count": normalized_request.horizon_interval_count,
                "interval_delta": normalized_request.interval_delta,
                "model_inputs": normalized_request.model_inputs.to_payload(),
                "hourly_factor_contract": normalized_request.hourly_factor_contract._native_result,
            },
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
