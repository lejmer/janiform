import json
from collections.abc import Mapping
from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from math import isfinite, sqrt
from typing import NotRequired, TypedDict

from .calendar import CalendarRegistry
from .hourly_factors import (
    HourlyFactorContract,
    HourlyNewsIntensityProviderSource,
    HourlySeasonalityProviderSource,
    provide_hourly_factor_contract,
)
from .simulation import HawkesExcitationState, HawkesKernelParameters

DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA = 1.0


class VanillaOptionPayoffType(StrEnum):
    CALL = 'call'
    PUT = 'put'


class GammaSeverityMarkConfiguration(TypedDict):
    shape_k: float
    scale_theta: float


class NigSharedPlusShiftConfiguration(TypedDict):
    alpha: float
    beta_0: float
    delta: float
    mu: float
    delta_beta: float


class VanillaOptionPayoffConfiguration(TypedDict):
    payoff_type: VanillaOptionPayoffType


class VanillaOptionPricingModelParameters(TypedDict):
    hawkes_kernel_parameters: HawkesKernelParameters
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration


class VanillaOptionPricingMarketInputs(TypedDict):
    spot: float
    strike: float
    maturity_interval: float
    risk_free_rate: float
    dividend_yield: float


class VanillaOptionPricingRequest(TypedDict):
    market_inputs: VanillaOptionPricingMarketInputs
    payoff: VanillaOptionPayoffConfiguration
    boundary_state: HawkesExcitationState
    model_parameters: VanillaOptionPricingModelParameters
    interval_delta: NotRequired[float]


class VanillaOptionPricingDiagnostics(TypedDict):
    forward_price: float
    discount_factor: float
    maturity_interval: float
    boundary_state_norm: float


class VanillaOptionPricingResponse(TypedDict):
    price: float
    diagnostics: VanillaOptionPricingDiagnostics


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingValidationFailure:
    field_path: str
    message: str


class VanillaOptionPricingValidationError(ValueError):
    def __init__(self, failure: VanillaOptionPricingValidationFailure) -> None:
        self.failure: VanillaOptionPricingValidationFailure = failure
        super().__init__(f"{failure.field_path}: {failure.message}")


def load_hourly_factor_contract_for_pricing(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    seasonality_source: HourlySeasonalityProviderSource,
    news_intensity_source: HourlyNewsIntensityProviderSource,
    interval_delta: float = DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA,
) -> HourlyFactorContract:
    """Load interval-aligned hourly factors for the pricing path."""
    parsed_interval_delta = _require_positive_finite_scalar(
        interval_delta,
        field_path="interval_delta",
    )
    if parsed_interval_delta != DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA:
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path="interval_delta",
                message=(
                    "hourly factor provider supports only interval_delta=1.0 "
                    f"for hourly boundaries, received {parsed_interval_delta}"
                ),
            )
        )

    return provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        seasonality_source=seasonality_source,
        news_intensity_source=news_intensity_source,
    )


def validate_vanilla_option_pricing_request(
    raw_request: Mapping[str, object],
) -> VanillaOptionPricingRequest:
    market_inputs = _require_mapping(raw_request, "market_inputs", "market_inputs")
    payoff = _require_mapping(raw_request, "payoff", "payoff")
    boundary_state = _require_mapping(raw_request, "boundary_state", "boundary_state")
    model_parameters = _require_mapping(raw_request, "model_parameters", "model_parameters")

    spot = _require_positive_finite_number(market_inputs, "spot", "market_inputs.spot")
    strike = _require_positive_finite_number(market_inputs, "strike", "market_inputs.strike")
    maturity_interval = _require_positive_finite_number(
        market_inputs,
        "maturity_interval",
        "market_inputs.maturity_interval",
    )
    risk_free_rate = _require_finite_number(
        market_inputs,
        "risk_free_rate",
        "market_inputs.risk_free_rate",
    )
    dividend_yield = _require_finite_number(
        market_inputs,
        "dividend_yield",
        "market_inputs.dividend_yield",
    )

    payoff_type = _require_payoff_type(payoff, "payoff_type", "payoff.payoff_type")

    y_plus_plus = _require_non_negative_finite_number(
        boundary_state,
        "y_plus_plus",
        "boundary_state.y_plus_plus",
    )
    y_plus_minus = _require_non_negative_finite_number(
        boundary_state,
        "y_plus_minus",
        "boundary_state.y_plus_minus",
    )
    y_minus_plus = _require_non_negative_finite_number(
        boundary_state,
        "y_minus_plus",
        "boundary_state.y_minus_plus",
    )
    y_minus_minus = _require_non_negative_finite_number(
        boundary_state,
        "y_minus_minus",
        "boundary_state.y_minus_minus",
    )

    parsed_model_parameters = _validate_model_parameters(model_parameters)
    interval_delta = (
        _require_positive_finite_number(raw_request, "interval_delta", "interval_delta")
        if "interval_delta" in raw_request
        else DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA
    )

    return {
        "market_inputs": {
            "spot": spot,
            "strike": strike,
            "maturity_interval": maturity_interval,
            "risk_free_rate": risk_free_rate,
            "dividend_yield": dividend_yield,
        },
        "payoff": {
            "payoff_type": payoff_type,
        },
        "boundary_state": {
            "y_plus_plus": y_plus_plus,
            "y_plus_minus": y_plus_minus,
            "y_minus_plus": y_minus_plus,
            "y_minus_minus": y_minus_minus,
        },
        "model_parameters": parsed_model_parameters,
        "interval_delta": interval_delta,
    }


def validate_vanilla_option_pricing_response(
    raw_response: Mapping[str, object],
) -> VanillaOptionPricingResponse:
    price = _require_non_negative_finite_number(raw_response, "price", "price")
    diagnostics = _require_mapping(raw_response, "diagnostics", "diagnostics")

    forward_price = _require_positive_finite_number(
        diagnostics,
        "forward_price",
        "diagnostics.forward_price",
    )
    discount_factor = _require_positive_finite_number(
        diagnostics,
        "discount_factor",
        "diagnostics.discount_factor",
    )
    maturity_interval = _require_positive_finite_number(
        diagnostics,
        "maturity_interval",
        "diagnostics.maturity_interval",
    )
    boundary_state_norm = _require_non_negative_finite_number(
        diagnostics,
        "boundary_state_norm",
        "diagnostics.boundary_state_norm",
    )

    return {
        "price": price,
        "diagnostics": {
            "forward_price": forward_price,
            "discount_factor": discount_factor,
            "maturity_interval": maturity_interval,
            "boundary_state_norm": boundary_state_norm,
        },
    }


def serialize_vanilla_option_pricing_request(request: Mapping[str, object]) -> str:
    validated_request = validate_vanilla_option_pricing_request(request)
    return json.dumps(validated_request, sort_keys=True)


def deserialize_vanilla_option_pricing_request(payload_json: str) -> VanillaOptionPricingRequest:
    raw_payload = _parse_json_object(payload_json, "vanilla option request")
    return validate_vanilla_option_pricing_request(raw_payload)


def serialize_vanilla_option_pricing_response(response: Mapping[str, object]) -> str:
    validated_response = validate_vanilla_option_pricing_response(response)
    return json.dumps(validated_response, sort_keys=True)


def deserialize_vanilla_option_pricing_response(payload_json: str) -> VanillaOptionPricingResponse:
    raw_payload = _parse_json_object(payload_json, "vanilla option response")
    return validate_vanilla_option_pricing_response(raw_payload)


def _validate_model_parameters(
    raw_model_parameters: Mapping[str, object],
) -> VanillaOptionPricingModelParameters:
    raw_kernel_parameters = _require_mapping(
        raw_model_parameters,
        "hawkes_kernel_parameters",
        "model_parameters.hawkes_kernel_parameters",
    )
    raw_alpha = _require_mapping(
        raw_kernel_parameters,
        "alpha",
        "model_parameters.hawkes_kernel_parameters.alpha",
    )
    raw_rho = _require_mapping(
        raw_kernel_parameters,
        "rho",
        "model_parameters.hawkes_kernel_parameters.rho",
    )

    lambda_bar = _require_positive_finite_number(
        raw_kernel_parameters,
        "lambda_bar",
        "model_parameters.hawkes_kernel_parameters.lambda_bar",
    )
    beta = _require_positive_finite_number(
        raw_kernel_parameters,
        "beta",
        "model_parameters.hawkes_kernel_parameters.beta",
    )

    alpha_plus_plus = _require_non_negative_finite_number(
        raw_alpha,
        "plus_plus",
        "model_parameters.hawkes_kernel_parameters.alpha.plus_plus",
    )
    alpha_plus_minus = _require_non_negative_finite_number(
        raw_alpha,
        "plus_minus",
        "model_parameters.hawkes_kernel_parameters.alpha.plus_minus",
    )
    alpha_minus_plus = _require_non_negative_finite_number(
        raw_alpha,
        "minus_plus",
        "model_parameters.hawkes_kernel_parameters.alpha.minus_plus",
    )
    alpha_minus_minus = _require_non_negative_finite_number(
        raw_alpha,
        "minus_minus",
        "model_parameters.hawkes_kernel_parameters.alpha.minus_minus",
    )

    rho_plus_plus = _require_non_negative_finite_number(
        raw_rho,
        "plus_plus",
        "model_parameters.hawkes_kernel_parameters.rho.plus_plus",
    )
    rho_plus_minus = _require_non_negative_finite_number(
        raw_rho,
        "plus_minus",
        "model_parameters.hawkes_kernel_parameters.rho.plus_minus",
    )
    rho_minus_plus = _require_non_negative_finite_number(
        raw_rho,
        "minus_plus",
        "model_parameters.hawkes_kernel_parameters.rho.minus_plus",
    )
    rho_minus_minus = _require_non_negative_finite_number(
        raw_rho,
        "minus_minus",
        "model_parameters.hawkes_kernel_parameters.rho.minus_minus",
    )

    spectral_radius = _compute_hawkes_alpha_over_beta_spectral_radius(
        alpha_plus_plus,
        alpha_plus_minus,
        alpha_minus_plus,
        alpha_minus_minus,
        beta,
    )
    if spectral_radius >= 1.0:
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path="model_parameters.hawkes_kernel_parameters",
                message=(
                    "alpha_over_beta spectral radius must be strictly below 1 "
                    f"for stability, found {spectral_radius}"
                ),
            )
        )

    raw_gamma_configuration = _require_mapping(
        raw_model_parameters,
        "gamma_severity_mark_configuration",
        "model_parameters.gamma_severity_mark_configuration",
    )
    shape_k = _require_positive_finite_number(
        raw_gamma_configuration,
        "shape_k",
        "model_parameters.gamma_severity_mark_configuration.shape_k",
    )
    scale_theta = _require_positive_finite_number(
        raw_gamma_configuration,
        "scale_theta",
        "model_parameters.gamma_severity_mark_configuration.scale_theta",
    )

    raw_nig_configuration = _require_mapping(
        raw_model_parameters,
        "nig_shared_plus_shift_configuration",
        "model_parameters.nig_shared_plus_shift_configuration",
    )
    alpha = _require_positive_finite_number(
        raw_nig_configuration,
        "alpha",
        "model_parameters.nig_shared_plus_shift_configuration.alpha",
    )
    beta_0 = _require_finite_number(
        raw_nig_configuration,
        "beta_0",
        "model_parameters.nig_shared_plus_shift_configuration.beta_0",
    )
    delta = _require_positive_finite_number(
        raw_nig_configuration,
        "delta",
        "model_parameters.nig_shared_plus_shift_configuration.delta",
    )
    mu = _require_finite_number(
        raw_nig_configuration,
        "mu",
        "model_parameters.nig_shared_plus_shift_configuration.mu",
    )
    delta_beta = _require_finite_number(
        raw_nig_configuration,
        "delta_beta",
        "model_parameters.nig_shared_plus_shift_configuration.delta_beta",
    )

    if alpha <= abs(beta_0):
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path="model_parameters.nig_shared_plus_shift_configuration.alpha",
                message=(
                    "alpha must be strictly greater than abs(beta_0), "
                    f"received alpha={alpha} and beta_0={beta_0}"
                ),
            )
        )

    derived_sell_beta = beta_0 + delta_beta
    if alpha <= abs(derived_sell_beta):
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path="model_parameters.nig_shared_plus_shift_configuration.delta_beta",
                message=(
                    "alpha must be strictly greater than abs(beta_0 + delta_beta), "
                    f"received alpha={alpha}, beta_0={beta_0}, delta_beta={delta_beta}"
                ),
            )
        )

    return {
        "hawkes_kernel_parameters": {
            "lambda_bar": lambda_bar,
            "beta": beta,
            "alpha": {
                "plus_plus": alpha_plus_plus,
                "plus_minus": alpha_plus_minus,
                "minus_plus": alpha_minus_plus,
                "minus_minus": alpha_minus_minus,
            },
            "rho": {
                "plus_plus": rho_plus_plus,
                "plus_minus": rho_plus_minus,
                "minus_plus": rho_minus_plus,
                "minus_minus": rho_minus_minus,
            },
        },
        "gamma_severity_mark_configuration": {
            "shape_k": shape_k,
            "scale_theta": scale_theta,
        },
        "nig_shared_plus_shift_configuration": {
            "alpha": alpha,
            "beta_0": beta_0,
            "delta": delta,
            "mu": mu,
            "delta_beta": delta_beta,
        },
    }


def _require_payoff_type(
    mapping: Mapping[str, object],
    field_name: str,
    field_path: str,
) -> VanillaOptionPayoffType:
    raw_value = _require_field(mapping, field_name, field_path)
    if not isinstance(raw_value, str):
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=field_path,
                message="must be one of: call, put",
            )
        )

    try:
        return VanillaOptionPayoffType(raw_value)
    except ValueError as error:
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=field_path,
                message=f"must be one of: call, put, received '{raw_value}'",
            )
        ) from error


def _compute_hawkes_alpha_over_beta_spectral_radius(
    alpha_plus_plus: float,
    alpha_plus_minus: float,
    alpha_minus_plus: float,
    alpha_minus_minus: float,
    beta: float,
) -> float:
    a = alpha_plus_plus / beta
    b = alpha_plus_minus / beta
    c = alpha_minus_plus / beta
    d = alpha_minus_minus / beta

    trace = a + d
    determinant = a * d - b * c
    radicand = trace * trace - 4.0 * determinant
    safe_radicand = max(radicand, 0.0)

    return 0.5 * (trace + sqrt(safe_radicand))


def _parse_json_object(payload_json: str, payload_label: str) -> Mapping[str, object]:
    try:
        parsed_payload = json.loads(payload_json)
    except json.JSONDecodeError as error:
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=payload_label,
                message=f"must be valid JSON: {error.msg}",
            )
        ) from error

    if not isinstance(parsed_payload, dict):
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=payload_label,
                message="must decode to a JSON object",
            )
        )

    return parsed_payload


def _require_mapping(
    mapping: Mapping[str, object],
    field_name: str,
    field_path: str,
) -> Mapping[str, object]:
    raw_value = _require_field(mapping, field_name, field_path)
    if not isinstance(raw_value, Mapping):
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=field_path,
                message="must be an object",
            )
        )

    return raw_value


def _require_field(
    mapping: Mapping[str, object],
    field_name: str,
    field_path: str,
) -> object:
    if field_name not in mapping:
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=field_path,
                message="is required",
            )
        )

    return mapping[field_name]


def _require_positive_finite_number(
    mapping: Mapping[str, object],
    field_name: str,
    field_path: str,
) -> float:
    value = _require_finite_number(mapping, field_name, field_path)
    if value <= 0.0:
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=field_path,
                message="must be strictly positive",
            )
        )

    return value


def _require_non_negative_finite_number(
    mapping: Mapping[str, object],
    field_name: str,
    field_path: str,
) -> float:
    value = _require_finite_number(mapping, field_name, field_path)
    if value < 0.0:
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=field_path,
                message="must be non-negative",
            )
        )

    return value


def _require_finite_number(
    mapping: Mapping[str, object],
    field_name: str,
    field_path: str,
) -> float:
    raw_value = _require_field(mapping, field_name, field_path)
    if not isinstance(raw_value, (int, float)):
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=field_path,
                message="must be a finite number",
            )
        )

    numeric_value = float(raw_value)
    if not isfinite(numeric_value):
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=field_path,
                message="must be a finite number",
            )
        )

    return numeric_value


def _require_positive_finite_scalar(value: object, *, field_path: str) -> float:
    if not isinstance(value, (int, float)):
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=field_path,
                message="must be a finite number",
            )
        )

    numeric_value = float(value)
    if not isfinite(numeric_value):
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=field_path,
                message="must be a finite number",
            )
        )

    if numeric_value <= 0.0:
        raise VanillaOptionPricingValidationError(
            VanillaOptionPricingValidationFailure(
                field_path=field_path,
                message="must be strictly positive",
            )
        )

    return numeric_value


__all__ = [
    "GammaSeverityMarkConfiguration",
    "NigSharedPlusShiftConfiguration",
    "VanillaOptionPayoffConfiguration",
    "VanillaOptionPayoffType",
    "VanillaOptionPricingDiagnostics",
    "VanillaOptionPricingMarketInputs",
    "VanillaOptionPricingModelParameters",
    "VanillaOptionPricingRequest",
    "VanillaOptionPricingResponse",
    "VanillaOptionPricingValidationError",
    "VanillaOptionPricingValidationFailure",
    "deserialize_vanilla_option_pricing_request",
    "deserialize_vanilla_option_pricing_response",
    "load_hourly_factor_contract_for_pricing",
    "serialize_vanilla_option_pricing_request",
    "serialize_vanilla_option_pricing_response",
    "validate_vanilla_option_pricing_request",
    "validate_vanilla_option_pricing_response",
]
