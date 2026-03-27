from __future__ import annotations

from collections.abc import Callable
from pathlib import Path
from types import ModuleType
from typing import TYPE_CHECKING, Any, Optional

from .inputs import (
    StagedCalibrationExecutionInput,
    validate_staged_calibration_execution_input,
)
from .results import StagedCalibrationResponse, validate_staged_calibration_response
from .._errors import (
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    CalibrationValidationError,
    _calibration_error,
    _calibration_execution_error,
    _calibration_integration_error,
)
from ..._native_api import native_exception_matches, native_module
from ...calendar import CalendarRegistry
from ...contracts import parse_json_object

if TYPE_CHECKING:
    from ..._native import PreparedCalibrationExecutionHandle


def _run_prepared_staged_calibration_with_raw_payload(
    prepared_handle: PreparedCalibrationExecutionHandle,
    *,
    calendar_registry: CalendarRegistry,
) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    return _run_native_staged_calibration_with_prepared_handle_and_events(
        prepared_handle,
        calendar_registry=calendar_registry,
        event_callback=None,
    )


def _run_native_staged_calibration_with_prepared_handle_and_events(
    prepared_handle: PreparedCalibrationExecutionHandle,
    *,
    calendar_registry: CalendarRegistry,
    event_callback: Optional[Callable[[dict[str, Any]], None]],
) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    if not isinstance(calendar_registry, CalendarRegistry):
        raise _calibration_error("calendar_registry", "must be a CalendarRegistry")

    native = _native_module_for_staged_runtime()
    try:
        if event_callback is None:
            raw_response_payload = native.run_prepared_staged_calibration(
                prepared_handle,
                calendar_registry._native_registry,
            )
        else:
            raw_response_payload = native.run_prepared_staged_calibration_with_events(
                prepared_handle,
                calendar_registry._native_registry,
                event_callback,
            )
    except Exception as e:
        _raise_staged_execution_error(native, e)

    return _validate_native_staged_response(raw_response_payload)


def _run_staged_calibration_with_raw_payload(
    execution_input: StagedCalibrationExecutionInput,
    *,
    calendar_registry: CalendarRegistry,
) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    return run_staged_calibration_with_raw_payload_and_events(
        execution_input,
        calendar_registry=calendar_registry,
        event_callback=None,
    )


def run_staged_calibration_with_raw_payload_and_events(
    execution_input: StagedCalibrationExecutionInput,
    *,
    calendar_registry: CalendarRegistry,
    event_callback: Optional[Callable[[dict[str, Any]], None]],
) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    if not isinstance(execution_input, StagedCalibrationExecutionInput):
        raise _calibration_error("execution_input", "must be a StagedCalibrationExecutionInput")
    if not isinstance(calendar_registry, CalendarRegistry):
        raise _calibration_error("calendar_registry", "must be a CalendarRegistry")

    normalized_execution_input = validate_staged_calibration_execution_input(
        execution_input.to_payload()
    )
    raw_prepared_payload = _load_prepared_staged_payload(normalized_execution_input)
    raw_execution_payload: dict[str, Any] = {
        **raw_prepared_payload,
        "request": normalized_execution_input.request.to_payload(),
        "runtime_resources": (
            None
            if normalized_execution_input.runtime_resources is None
            else normalized_execution_input.runtime_resources.to_payload()
        ),
    }

    native = _native_module_for_staged_runtime()
    try:
        if event_callback is None:
            raw_response_payload = native.run_staged_calibration(
                raw_execution_payload,
                calendar_registry._native_registry,
            )
        else:
            raw_response_payload = native.run_staged_calibration_with_events(
                raw_execution_payload,
                calendar_registry._native_registry,
                event_callback,
            )
    except Exception as e:
        _raise_staged_execution_error(native, e)

    return _validate_native_staged_response(raw_response_payload)


def run_staged_calibration(
    execution_input: StagedCalibrationExecutionInput,
    *,
    calendar_registry: CalendarRegistry,
) -> StagedCalibrationResponse:
    normalized_response, _ = _run_staged_calibration_with_raw_payload(
        execution_input,
        calendar_registry=calendar_registry,
    )
    return normalized_response


def _load_prepared_staged_payload(
    execution_input: StagedCalibrationExecutionInput,
) -> dict[str, Any]:
    payload_path = Path(execution_input.prepared_payload_file.path)
    try:
        payload_json = payload_path.read_text(encoding="utf-8")
    except OSError as e:
        raise _calibration_error(
            "prepared_payload_file.path",
            f"could not be read: {e}",
        ) from e

    raw_payload = parse_json_object(
        payload_json,
        "prepared staged calibration payload",
        _calibration_error,
    )
    return dict(raw_payload)


def _native_module_for_staged_runtime() -> ModuleType:
    try:
        return native_module()
    except ModuleNotFoundError as e:
        raise _calibration_integration_error(
            "native calibration module is unavailable",
            entrypoint="run_staged_calibration",
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e


def _raise_staged_execution_error(native: ModuleType, error: Exception) -> None:
    if native_exception_matches(native, error, "CalibrationExecutionError"):
        raise _calibration_execution_error(
            "staged calibration failed during native execution",
            entrypoint="run_staged_calibration",
            context={"source_exception_type": type(error).__name__},
        ) from error

    raise _calibration_execution_error(
        "staged calibration failed during native execution",
        entrypoint="run_staged_calibration",
        context={"source_exception_type": type(error).__name__},
    ) from error


def _validate_native_staged_response(
    raw_response_payload: Any,
) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
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


__all__ = [
    "run_staged_calibration",
    "run_staged_calibration_with_raw_payload_and_events",
]
