"""Public workflow entrypoints for vanilla-option pricing."""

from __future__ import annotations

from types import ModuleType
from typing import Any

from ..contracts import parse_json_object, serialize_deterministic_json
from .._native_api import native_exception_matches, native_module
from .contracts import (
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingRequest,
    VanillaOptionPricingResponse,
    VanillaOptionPricingRunResult,
)
from .errors import (
    PRICING_ERROR_FACTORY,
    PRICING_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    PRICING_INTEGRATION_RESPONSE_ERROR_ID,
    VanillaOptionPricingValidationError,
    _pricing_error,
    _pricing_execution_error,
    _pricing_integration_error,
)


def validate_vanilla_option_pricing_request(
    raw_request: dict[str, Any] | Any,
) -> VanillaOptionPricingRequest:
    return VanillaOptionPricingRequest.from_mapping(raw_request)


def validate_vanilla_option_pricing_response(
    raw_response: dict[str, Any] | Any,
) -> VanillaOptionPricingResponse:
    return VanillaOptionPricingResponse.from_mapping(raw_response)


def validate_vanilla_option_pricing_run_result(
    raw_run_result: dict[str, Any] | Any,
) -> VanillaOptionPricingRunResult:
    return VanillaOptionPricingRunResult.from_mapping(raw_run_result)


def serialize_vanilla_option_pricing_request(request: VanillaOptionPricingRequest) -> str:
    return serialize_deterministic_json(request)


def deserialize_vanilla_option_pricing_request(payload_json: str) -> VanillaOptionPricingRequest:
    raw_payload = parse_json_object(payload_json, "vanilla option request", PRICING_ERROR_FACTORY)
    return validate_vanilla_option_pricing_request(dict(raw_payload))


def serialize_vanilla_option_pricing_response(response: VanillaOptionPricingResponse) -> str:
    return serialize_deterministic_json(response)


def deserialize_vanilla_option_pricing_response(payload_json: str) -> VanillaOptionPricingResponse:
    raw_payload = parse_json_object(payload_json, "vanilla option response", PRICING_ERROR_FACTORY)
    return validate_vanilla_option_pricing_response(dict(raw_payload))


def serialize_vanilla_option_pricing_run_result(run_result: VanillaOptionPricingRunResult) -> str:
    return serialize_deterministic_json(run_result)


def deserialize_vanilla_option_pricing_run_result(payload_json: str) -> VanillaOptionPricingRunResult:
    raw_payload = parse_json_object(
        payload_json,
        "vanilla option pricing run result",
        PRICING_ERROR_FACTORY,
    )
    return validate_vanilla_option_pricing_run_result(dict(raw_payload))


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
        return validate_vanilla_option_pricing_run_result(raw_run_result)
    except VanillaOptionPricingValidationError as error:
        raise _pricing_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint="run_vanilla_option_pricing",
            error_id=PRICING_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": error.error_id},
        ) from error


__all__ = [
    "deserialize_vanilla_option_pricing_request",
    "deserialize_vanilla_option_pricing_response",
    "deserialize_vanilla_option_pricing_run_result",
    "run_vanilla_option_pricing",
    "serialize_vanilla_option_pricing_request",
    "serialize_vanilla_option_pricing_response",
    "serialize_vanilla_option_pricing_run_result",
    "validate_vanilla_option_pricing_request",
    "validate_vanilla_option_pricing_response",
    "validate_vanilla_option_pricing_run_result",
]
