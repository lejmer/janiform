from typing import Any, Optional

from .._contracts import ValidationErrorFactory
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


def _calibration_error(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = CALIBRATION_VALIDATION_ERROR_ID,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
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


def _calibration_execution_error(
    message: str,
    *,
    entrypoint: str,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
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


def _calibration_integration_error(
    message: str,
    *,
    entrypoint: str,
    error_id: str,
    field_path: Optional[str] = None,
    context: Optional[dict[str, str | int | float | bool | None]] = None,
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


CALIBRATION_ERROR_FACTORY: ValidationErrorFactory = _calibration_error


def _require_bool(raw_payload: dict[str, Any], field_name: str, field_path: str) -> bool:
    if field_name not in raw_payload:
        raise _calibration_error(field_path, "is required")

    value = raw_payload[field_name]
    if not isinstance(value, bool):
        raise _calibration_error(field_path, "must be a boolean")

    return value
