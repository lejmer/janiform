from __future__ import annotations

from collections.abc import Callable
from typing import Any, Optional

from ....._native_api import native_exception_matches, native_module
from .....pricing.progress import PricingProgressEvent, pricing_progress_event_from_mapping
from ....._workflow_progress import AbstractProgress, NullProgress
from ....errors import (
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    CalibrationValidationError,
    calibration_execution_error,
    calibration_integration_error,
)
from ...pricing.grids import (
    JaniformVanillaOptionGridBatchPricingResult,
)
from ...pricing.surface import (
    JaniformVanillaOptionSurfacePricingResult,
)


def run_model_vanilla_option_grid_batch_pricing(
    native_execution_input: dict[str, Any],
    *,
    progress: Optional[AbstractProgress] = None,
) -> JaniformVanillaOptionGridBatchPricingResult:
    return run_model_native_pricing_workflow(
        native_method_name="run_vanilla_option_grid_batch_pricing",
        native_execution_input=native_execution_input,
        result_parser=JaniformVanillaOptionGridBatchPricingResult.from_mapping,
        entrypoint="JaniformModel.price_option_grid",
        result_field_path="grid_batch_pricing_result",
        progress=progress,
        progress_detail="running option grid pricing workflow",
    )


def run_model_vanilla_option_surface_pricing(
    native_execution_input: dict[str, Any],
    *,
    progress: Optional[AbstractProgress] = None,
) -> JaniformVanillaOptionSurfacePricingResult:
    return run_model_native_pricing_workflow(
        native_method_name="run_vanilla_option_surface_pricing",
        native_execution_input=native_execution_input,
        result_parser=JaniformVanillaOptionSurfacePricingResult.from_mapping,
        entrypoint="JaniformModel.price_option_grid_surface",
        result_field_path="surface_pricing_result",
        progress=progress,
        progress_detail="running option grid surface pricing workflow",
    )


def run_model_native_pricing_workflow(
    *,
    native_method_name: str,
    native_execution_input: dict[str, Any],
    result_parser: Callable[..., Any],
    entrypoint: str,
    result_field_path: str,
    progress: Optional[AbstractProgress] = None,
    progress_detail: Optional[str] = None,
) -> Any:
    try:
        native = native_module()
    except ModuleNotFoundError as error:
        raise calibration_integration_error(
            "native pricing module is unavailable",
            entrypoint=entrypoint,
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from error

    native_progress_callback: Optional[Callable[[dict[str, Any]], None]] = None
    if progress is not None and progress_detail is not None and not isinstance(progress, NullProgress):

        def native_progress_callback(raw_event: dict[str, Any]) -> None:
            update_cli_progress_from_pricing_native_event(
                progress,
                raw_event,
                detail=progress_detail,
            )

    try:
        raw_run_result = run_native_pricing_workflow_method(
            native,
            native_method_name=native_method_name,
            native_execution_input=native_execution_input,
            native_progress_callback=native_progress_callback,
        )
    except Exception as error:
        if native_exception_matches(native, error, "PricingExecutionError"):
            raise calibration_execution_error(
                "pricing failed during native execution",
                entrypoint=entrypoint,
                context={"source_exception_type": type(error).__name__},
            ) from error

        raise calibration_execution_error(
            "pricing failed during native execution",
            entrypoint=entrypoint,
            context={"source_exception_type": type(error).__name__},
        ) from error

    if not isinstance(raw_run_result, dict):
        raise calibration_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint=entrypoint,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_run_result).__name__},
        )

    try:
        return result_parser(raw_run_result, field_path=result_field_path)
    except CalibrationValidationError as error:
        raise calibration_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint=entrypoint,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": error.error_id},
        ) from error


def update_cli_progress_from_pricing_native_event(
    progress: AbstractProgress,
    raw_event: dict[str, Any],
    *,
    detail: str,
) -> PricingProgressEvent:
    event = pricing_progress_event_from_mapping(raw_event)

    progress.update(
        detail=detail,
        subdetail=event.subdetail,
        completed_units=event.completed_units,
        total_units=event.total_units,
    )

    return event


def run_native_pricing_workflow_method(
    native: Any,
    *,
    native_method_name: str,
    native_execution_input: dict[str, Any],
    native_progress_callback: Optional[Callable[[dict[str, Any]], None]],
) -> Any:
    match native_method_name:
        case "run_vanilla_option_grid_batch_pricing":
            if native_progress_callback is None:
                return native.run_vanilla_option_grid_batch_pricing(native_execution_input)

            return native.run_vanilla_option_grid_batch_pricing_with_events(
                native_execution_input,
                native_progress_callback,
            )
        case "run_vanilla_option_surface_pricing":
            if native_progress_callback is None:
                return native.run_vanilla_option_surface_pricing(native_execution_input)

            return native.run_vanilla_option_surface_pricing_with_events(
                native_execution_input,
                native_progress_callback,
            )
        case _:
            raise RuntimeError(f"unsupported native pricing method: {native_method_name}")
