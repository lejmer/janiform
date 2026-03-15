from types import ModuleType
from typing import Any

from .inputs import (
    StagedCalibrationExecutionInput,
    validate_staged_calibration_execution_input,
)
from .results import StagedCalibrationResponse, validate_staged_calibration_response
from .._errors import (
    CALIBRATION_ERROR_FACTORY,
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    CalibrationValidationError,
    _calibration_error,
    _calibration_execution_error,
    _calibration_integration_error,
)
from ..._native_api import native_exception_matches, native_module
from ...calendar import CalendarRegistry


def _run_staged_calibration_with_raw_payload(
    execution_input: StagedCalibrationExecutionInput,
    *,
    calendar_registry: CalendarRegistry,
) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    if not isinstance(execution_input, StagedCalibrationExecutionInput):
        raise _calibration_error("execution_input", "must be a StagedCalibrationExecutionInput")
    if not isinstance(calendar_registry, CalendarRegistry):
        raise _calibration_error("calendar_registry", "must be a CalendarRegistry")

    normalized_execution_input = validate_staged_calibration_execution_input(
        execution_input.to_payload()
    )

    try:
        native: ModuleType = native_module()
    except ModuleNotFoundError as e:
        raise _calibration_integration_error(
            "native calibration module is unavailable",
            entrypoint="run_staged_calibration",
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    try:
        raw_response_payload = native.run_staged_calibration(
            normalized_execution_input.to_payload(),
            calendar_registry._native_registry,
        )
    except Exception as e:
        if native_exception_matches(native, e, "CalibrationExecutionError"):
            raise _calibration_execution_error(
                "staged calibration failed during native execution",
                entrypoint="run_staged_calibration",
                context={"source_exception_type": type(e).__name__},
            ) from e

        raise _calibration_execution_error(
            "staged calibration failed during native execution",
            entrypoint="run_staged_calibration",
            context={"source_exception_type": type(e).__name__},
        ) from e

    if not isinstance(raw_response_payload, dict):
        raise _calibration_integration_error(
            "native calibration returned an invalid response payload",
            entrypoint="run_staged_calibration",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_response_payload).__name__},
        )

    try:
        normalized_response = validate_staged_calibration_response(raw_response_payload)
    except CalibrationValidationError as e:
        raise _calibration_integration_error(
            "native calibration returned an invalid response payload",
            entrypoint="run_staged_calibration",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e

    return normalized_response, raw_response_payload


def run_staged_calibration(
    execution_input: StagedCalibrationExecutionInput,
    *,
    calendar_registry: CalendarRegistry,
) -> StagedCalibrationResponse:
    """Run staged calibration from the explicit execution contract.

    Example:
        response = run_staged_calibration(
            execution_input,
            calendar_registry=calendar_registry,
        )

    Return-components stage inputs keep known-news open-window jumps and
    closed-window gaps explicit while deriving surprise close-open gaps
    from raw boundary observations inside Janiform.
    """

    normalized_response, _ = _run_staged_calibration_with_raw_payload(
        execution_input,
        calendar_registry=calendar_registry,
    )

    return normalized_response


__all__ = [
    "run_staged_calibration",
]
