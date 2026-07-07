from __future__ import annotations

from collections.abc import Callable, Mapping
from types import ModuleType
from typing import Any, Optional, cast

from ..._native_api import native_error_parts, native_exception_matches
from ..errors import calibration_error


def call_native_preset_object(
    native_module: ModuleType,
    native_function: Callable[[dict[str, Any]], Any],
    request: dict[str, Any],
) -> dict[str, Any]:
    raw_payload = _call_native_preset_function(
        native_module,
        native_function,
        request,
    )

    return _native_preset_object(raw_payload)


def call_native_optional_preset_object(
    native_module: ModuleType,
    native_function: Callable[[dict[str, Any]], Any],
    request: dict[str, Any],
) -> Optional[dict[str, Any]]:
    raw_payload = _call_native_preset_function(
        native_module,
        native_function,
        request,
    )

    if raw_payload is None:
        return None

    return _native_preset_object(raw_payload)


def call_native_preset_string(
    native_module: ModuleType,
    native_function: Callable[[dict[str, Any]], Any],
    request: dict[str, Any],
) -> str:
    raw_payload = _call_native_preset_function(
        native_module,
        native_function,
        request,
    )

    return cast(str, raw_payload)


def _call_native_preset_function(
    native_module: ModuleType,
    native_function: Callable[[dict[str, Any]], Any],
    request: dict[str, Any],
) -> Any:
    try:
        raw_payload = native_function(request)

    # PyO3 exception inheritance is extension-defined, so match by native type
    # name and re-raise unknown exceptions unchanged.
    except Exception as error:
        if native_exception_matches(native_module, error, "CalibrationExecutionError"):
            field_path, message = native_error_parts(
                str(error),
                default_field_path="preset_resolution",
            )

            raise calibration_error(
                field_path,
                message,
                entrypoint="preset_resolution",
            ) from error

        raise

    return raw_payload


def _native_preset_object(raw_payload: Any) -> dict[str, Any]:
    return dict(cast(Mapping[str, Any], raw_payload))
