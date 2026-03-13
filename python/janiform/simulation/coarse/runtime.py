from .path import (
    CoarseSimulationPath,
    CoarseSimulationRunRequest,
    deserialize_coarse_simulation_path,
    serialize_coarse_simulation_run_request,
    validate_coarse_simulation_run_request,
)
from ..shared import SeededTickSimulationValidationError
from ..shared.errors import (
    SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
    _simulation_error,
    _simulation_execution_error,
    _simulation_integration_error,
)
from ..._native_api import native_module


def run_coarse_simulation(request: CoarseSimulationRunRequest) -> CoarseSimulationPath:
    if not isinstance(request, CoarseSimulationRunRequest):
        raise _simulation_error("request", "must be a CoarseSimulationRunRequest")

    normalized_request = validate_coarse_simulation_run_request(request.to_payload())

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
        path_json: str = native.run_coarse_simulation(
            serialize_coarse_simulation_run_request(normalized_request)
        )
    except native.SimulationExecutionError as e:
        raise _simulation_execution_error(
            "coarse simulation failed during native execution",
            entrypoint="run_coarse_simulation",
            context={"source_exception_type": type(e).__name__},
        ) from e
    except Exception as e:
        raise _simulation_execution_error(
            "coarse simulation failed during native execution",
            entrypoint="run_coarse_simulation",
            context={"source_exception_type": type(e).__name__},
        ) from e

    try:
        return deserialize_coarse_simulation_path(path_json)
    except SeededTickSimulationValidationError as e:
        raise _simulation_integration_error(
            "native coarse simulation returned an invalid response payload",
            entrypoint="run_coarse_simulation",
            error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e


__all__ = ["run_coarse_simulation"]
