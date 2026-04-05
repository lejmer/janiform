"""Public workflow entrypoints for vanilla-option pricing."""

from __future__ import annotations

from types import ModuleType
from typing import Any

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
from .._native_api import native_exception_matches, native_module

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
            normalized_execution_input.request.to_payload(),
            normalized_execution_input.hourly_factor_contract._native_result,
            normalized_execution_input.optional_output_selection.to_payload(),
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
        raw_run_result = native.run_vanilla_option_surface_pricing(
            _surface_pricing_native_execution_input(execution_input)
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
        "interval_delta": request.interval_delta,
        "fourier_inversion_settings": (
            request.fourier_inversion_settings.to_payload()
            if request.fourier_inversion_settings is not None
            else None
        ),
        "interval_factors": [
            {
                "s_h": float(execution_input.hourly_factor_contract.s_h[interval_index]),
                "nu_h_known": float(
                    execution_input.hourly_factor_contract.known_component_nu_h[
                        interval_index
                    ]
                ),
                "nu_h_surprise": float(
                    execution_input.hourly_factor_contract.surprise_component_nu_h[
                        interval_index
                    ]
                ),
            }
            for interval_index in range(execution_input.hourly_factor_contract.interval_count)
        ],
        "optional_output_selection": execution_input.optional_output_selection.to_payload(),
        "cpu_count": execution_input.cpu_count,
    }


__all__ = [
    "run_vanilla_option_pricing",
    "run_vanilla_option_surface_pricing",
]
