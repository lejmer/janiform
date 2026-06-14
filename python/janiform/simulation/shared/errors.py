from typing import Optional

from ...errors import (
    ContractValidationFailure,
    JaniformErrorCategory,
    JaniformErrorDetails,
    JaniformExecutionError,
    JaniformIntegrationError,
    JaniformValidationError,
)
from ...contracts import ValidationErrorFactory


SIMULATION_VALIDATION_ERROR_ID = "simulation.validation.contract"
SIMULATION_EXECUTION_ERROR_ID = "simulation.execution.run_failed"
SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID = "simulation.integration.native_module_unavailable"
SIMULATION_INTEGRATION_RESPONSE_ERROR_ID = "simulation.integration.invalid_native_response"
SIMULATION_INTEGRATION_INTERVAL_FACTOR_GRID_ERROR_ID = "simulation.integration.interval_factor_grid_unavailable"


class SeededTickSimulationValidationError(JaniformValidationError):
    pass


def _simulation_failure(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = SIMULATION_VALIDATION_ERROR_ID,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> ContractValidationFailure:
    return ContractValidationFailure.from_context(
        error_id=error_id,
        workflow="simulation",
        entrypoint=entrypoint,
        field_path=field_path,
        message=message,
        context=context,
    )


def _simulation_error(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = SIMULATION_VALIDATION_ERROR_ID,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> SeededTickSimulationValidationError:
    return SeededTickSimulationValidationError(
        _simulation_failure(
            field_path,
            message,
            entrypoint=entrypoint,
            error_id=error_id,
            context=context,
        )
    )


def _simulation_execution_error(
    message: str,
    *,
    entrypoint: str,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> JaniformExecutionError:
    return JaniformExecutionError(
        JaniformErrorDetails.from_context(
            error_id=SIMULATION_EXECUTION_ERROR_ID,
            category=JaniformErrorCategory.EXECUTION,
            workflow="simulation",
            entrypoint=entrypoint,
            message=message,
            context=context,
        )
    )


def _simulation_integration_error(
    message: str,
    *,
    entrypoint: str,
    error_id: str,
    field_path: Optional[str] = None,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> JaniformIntegrationError:
    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=error_id,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="simulation",
            entrypoint=entrypoint,
            message=message,
            field_path=field_path,
            context=context,
        )
    )


SIMULATION_ERROR_FACTORY: ValidationErrorFactory = _simulation_error


__all__ = [
    "SeededTickSimulationValidationError",
    "SIMULATION_ERROR_FACTORY",
    "SIMULATION_EXECUTION_ERROR_ID",
    "SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID",
    "SIMULATION_INTEGRATION_RESPONSE_ERROR_ID",
    "SIMULATION_INTEGRATION_INTERVAL_FACTOR_GRID_ERROR_ID",
    "SIMULATION_VALIDATION_ERROR_ID",
]
