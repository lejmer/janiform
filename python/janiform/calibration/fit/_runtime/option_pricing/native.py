from __future__ import annotations

from collections.abc import Callable, Mapping
from types import ModuleType
from typing import Any, Optional

from ....._native_api import native_exception_matches, native_module
from ....errors import (
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    CalibrationValidationError,
    calibration_execution_error,
    calibration_integration_error,
)
from .....pricing.fitted.grids import (
    JaniformVanillaOptionGridBatchPricingResult,
)
from .....pricing.fitted.surface import (
    JaniformVanillaOptionSurfacePricingResult,
)


def run_model_vanilla_option_grid_batch_pricing(
    native_execution_input: dict[str, Any],
    *,
    progress_mode: str,
    progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
) -> JaniformVanillaOptionGridBatchPricingResult:
    return _run_native_pricing_entrypoint(
        native_runner=_run_native_grid_batch_pricing,
        native_execution_input=native_execution_input,
        progress_mode=progress_mode,
        progress_callback=progress_callback,
        result_parser=JaniformVanillaOptionGridBatchPricingResult.from_mapping,
        entrypoint="JaniformModel.price_option_grid",
        result_field_path="grid_batch_pricing_result",
    )


def run_model_vanilla_option_surface_pricing(
    native_execution_input: dict[str, Any],
    *,
    progress_mode: str,
    progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
) -> JaniformVanillaOptionSurfacePricingResult:
    return _run_native_pricing_entrypoint(
        native_runner=_run_native_surface_pricing,
        native_execution_input=native_execution_input,
        progress_mode=progress_mode,
        progress_callback=progress_callback,
        result_parser=JaniformVanillaOptionSurfacePricingResult.from_mapping,
        entrypoint="JaniformModel.price_option_grid_surface",
        result_field_path="surface_pricing_result",
    )


def _run_native_pricing_entrypoint(
    *,
    native_runner: Callable[
        [
            ModuleType,
            dict[str, Any],
            str,
            Optional[Callable[[dict[str, Any]], None]],
        ],
        Any,
    ],
    native_execution_input: dict[str, Any],
    progress_mode: str,
    progress_callback: Optional[Callable[[Mapping[str, Any]], None]],
    result_parser: Callable[..., Any],
    entrypoint: str,
    result_field_path: str,
) -> Any:
    try:
        native = native_module()
    except ModuleNotFoundError as e:
        raise calibration_integration_error(
            "native pricing module is unavailable",
            entrypoint=entrypoint,
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    try:
        raw_run_result = native_runner(
            native,
            native_execution_input,
            progress_mode,
            progress_callback,
        )
    except Exception as e:
        # Native extension exception types are matched dynamically by helper.
        if native_exception_matches(native, e, "PricingExecutionError"):
            raise calibration_execution_error(
                "pricing failed during native execution",
                entrypoint=entrypoint,
                context={"source_exception_type": type(e).__name__},
            ) from e

        raise calibration_execution_error(
            "pricing failed during native execution",
            entrypoint=entrypoint,
            context={"source_exception_type": type(e).__name__},
        ) from e

    if not isinstance(raw_run_result, dict):
        raise calibration_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint=entrypoint,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_run_result).__name__},
        )

    try:
        return result_parser(raw_run_result, field_path=result_field_path)
    except CalibrationValidationError as e:
        raise calibration_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint=entrypoint,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e


def _run_native_grid_batch_pricing(
    native: ModuleType,
    native_execution_input: dict[str, Any],
    progress_mode: str,
    progress_callback: Optional[Callable[[dict[str, Any]], None]],
) -> Any:
    if progress_callback is not None:
        return native.run_vanilla_option_grid_batch_pricing_with_events(
            native_execution_input,
            progress_callback,
        )

    return native.run_vanilla_option_grid_batch_pricing_with_terminal_progress(
        native_execution_input,
        progress_mode,
    )


def _run_native_surface_pricing(
    native: ModuleType,
    native_execution_input: dict[str, Any],
    progress_mode: str,
    progress_callback: Optional[Callable[[dict[str, Any]], None]],
) -> Any:
    if progress_callback is not None:
        return native.run_vanilla_option_surface_pricing_with_events(
            native_execution_input,
            progress_callback,
        )

    return native.run_vanilla_option_surface_pricing_with_terminal_progress(
        native_execution_input,
        progress_mode,
    )
