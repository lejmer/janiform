from typing import Optional

from ..errors import (
    ContractValidationFailure,
    JaniformErrorCategory,
    JaniformErrorDetails,
    JaniformExecutionError,
    JaniformIntegrationError,
    JaniformValidationError,
)


CALIBRATION_VALIDATION_ERROR_ID = "calibration.validation.contract"
CALIBRATION_EXECUTION_ERROR_ID = "calibration.execution.run_failed"
CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID = "calibration.integration.native_module_unavailable"
CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID = "calibration.integration.invalid_native_response"

CalibrationValidationFailure = ContractValidationFailure


class CalibrationValidationError(JaniformValidationError):
    pass


def calibration_error(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = CALIBRATION_VALIDATION_ERROR_ID,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> CalibrationValidationError:
    return CalibrationValidationError(
        CalibrationValidationFailure.from_context(
            error_id=error_id,
            workflow="calibration",
            entrypoint=entrypoint,
            field_path=field_path,
            message=message,
            context=context,
        )
    )


def calibration_execution_error(
    message: str,
    *,
    entrypoint: str,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> JaniformExecutionError:
    return JaniformExecutionError(
        JaniformErrorDetails.from_context(
            error_id=CALIBRATION_EXECUTION_ERROR_ID,
            category=JaniformErrorCategory.EXECUTION,
            workflow="calibration",
            entrypoint=entrypoint,
            message=message,
            context=context,
        )
    )


def calibration_integration_error(
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
            workflow="calibration",
            entrypoint=entrypoint,
            message=message,
            field_path=field_path,
            context=context,
        )
    )


__all__ = [
    "CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID",
    "CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID",
    "CalibrationValidationError",
    "CalibrationValidationFailure",
    "calibration_error",
    "calibration_execution_error",
    "calibration_integration_error",
]
