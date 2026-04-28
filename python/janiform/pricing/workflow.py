"""Public workflow entrypoints for vanilla-option pricing."""

from __future__ import annotations

from collections.abc import Callable
from types import ModuleType
from typing import Any, Optional

from .._native_api import native_exception_matches, native_module
from .contracts import (
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingRunResult,
    VanillaOptionSurfacePricingExecutionInput,
    VanillaOptionSurfacePricingRunResult,
)
from .errors import (
    PRICING_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    PRICING_INTEGRATION_RESPONSE_ERROR_ID,
    VanillaOptionPricingValidationError,
    _pricing_error,
    _pricing_execution_error,
    _pricing_integration_error,
)
from .progress import (
    PricingProgressEvent,
    _pricing_progress_callback_from_mapping,
)


def run_vanilla_option_pricing(
    execution_input: VanillaOptionPricingExecutionInput,
) -> VanillaOptionPricingRunResult:
    """Run vanilla option pricing from the explicit execution contract.

    Example:
        run_result = run_vanilla_option_pricing(execution_input)

    The execution input keeps `nu_h_known` and `nu_h_surprise` separate so the
    pricing path does not collapse corrected known-surprise news semantics.
    """

    if not isinstance(execution_input, VanillaOptionPricingExecutionInput):
        raise _pricing_error(
            "execution_input",
            "must be a VanillaOptionPricingExecutionInput",
            entrypoint="run_vanilla_option_pricing",
        )

    normalized_execution_input = execution_input

    try:
        native: ModuleType = native_module()
    except ModuleNotFoundError as error:
        raise _pricing_integration_error(
            "native pricing module is unavailable",
            entrypoint="run_vanilla_option_pricing",
            error_id=PRICING_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from error

    try:
        raw_run_result = native.run_vanilla_option_pricing(
            normalized_execution_input.to_payload()
        )
    except Exception as error:
        if native_exception_matches(native, error, "PricingExecutionError"):
            raise _pricing_execution_error(
                "vanilla option pricing failed during native execution",
                entrypoint="run_vanilla_option_pricing",
                context={"source_exception_type": type(error).__name__},
            ) from error

        raise _pricing_execution_error(
            "vanilla option pricing failed during native execution",
            entrypoint="run_vanilla_option_pricing",
            context={"source_exception_type": type(error).__name__},
        ) from error

    if not isinstance(raw_run_result, dict):
        raise _pricing_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint="run_vanilla_option_pricing",
            error_id=PRICING_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_run_result).__name__},
        )

    try:
        return VanillaOptionPricingRunResult.from_mapping(raw_run_result)
    except VanillaOptionPricingValidationError as error:
        raise _pricing_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint="run_vanilla_option_pricing",
            error_id=PRICING_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": error.error_id},
        ) from error


def run_vanilla_option_surface_pricing(
    execution_input: VanillaOptionSurfacePricingExecutionInput,
    *,
    progress_callback: Optional[Callable[[PricingProgressEvent], None]] = None,
) -> VanillaOptionSurfacePricingRunResult:
    if not isinstance(execution_input, VanillaOptionSurfacePricingExecutionInput):
        raise _pricing_error(
            "execution_input",
            "must be a VanillaOptionSurfacePricingExecutionInput",
            entrypoint="run_vanilla_option_surface_pricing",
        )

    try:
        native: ModuleType = native_module()
    except ModuleNotFoundError as error:
        raise _pricing_integration_error(
            "native pricing module is unavailable",
            entrypoint="run_vanilla_option_surface_pricing",
            error_id=PRICING_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from error

    try:
        native_progress_callback = _pricing_progress_callback_from_mapping(progress_callback)
        native_execution_input = _surface_pricing_native_execution_input(execution_input)

        if native_progress_callback is None:
            raw_run_result = native.run_vanilla_option_surface_pricing(native_execution_input)
        else:
            raw_run_result = native.run_vanilla_option_surface_pricing_with_events(
                native_execution_input,
                native_progress_callback,
            )
    except Exception as error:
        if native_exception_matches(native, error, "PricingExecutionError"):
            raise _pricing_execution_error(
                "vanilla option surface pricing failed during native execution",
                entrypoint="run_vanilla_option_surface_pricing",
                context={"source_exception_type": type(error).__name__},
            ) from error

        raise _pricing_execution_error(
            "vanilla option surface pricing failed during native execution",
            entrypoint="run_vanilla_option_surface_pricing",
            context={"source_exception_type": type(error).__name__},
        ) from error

    if not isinstance(raw_run_result, dict):
        raise _pricing_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint="run_vanilla_option_surface_pricing",
            error_id=PRICING_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_run_result).__name__},
        )

    try:
        return VanillaOptionSurfacePricingRunResult.from_mapping(raw_run_result)
    except VanillaOptionPricingValidationError as error:
        raise _pricing_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint="run_vanilla_option_surface_pricing",
            error_id=PRICING_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": error.error_id},
        ) from error


def _surface_pricing_native_execution_input(
    execution_input: VanillaOptionSurfacePricingExecutionInput,
) -> dict[str, Any]:
    request = execution_input.request
    market_inputs = request.market_inputs

    return {
        "request": {
            "market_inputs": {
                "spot": market_inputs.spot,
                "strikes": list(market_inputs.strikes),
                "maturity_intervals": list(market_inputs.maturity_intervals),
                "risk_free_rate": market_inputs.risk_free_rate,
                "dividend_yield": market_inputs.dividend_yield,
            },
            "payoff": request.payoff.to_payload(),
        },
        "boundary_input": request.boundary_input.to_payload(),
        "model_parameters": request.model_parameters.to_payload(),
        "runtime_interval_schedule": execution_input.runtime_interval_schedule.to_payload(),
        "interval_delta": request.interval_delta,
        "hourly_factor_contract": execution_input.hourly_factor_contract.to_payload(),
        "optional_output_selection": execution_input.optional_output_selection.to_payload(),
        "cpu_count": execution_input.cpu_count,
    }


__all__ = [
    "run_vanilla_option_pricing",
    "run_vanilla_option_surface_pricing",
]
