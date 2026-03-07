from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from typing import Any

from ._contracts import (
    ContractModel,
    ValidationErrorFactory,
    parse_json_object,
    require_finite_number,
    require_mapping,
    require_non_negative_finite_number,
    require_positive_finite_number,
    require_positive_finite_scalar,
    require_string,
    serialize_deterministic_json,
)
from .calendar import CalendarRegistry
from .hourly_factors import (
    HourlyFactorContract,
    HourlyNewsIntensityProviderSource,
    HourlySeasonalityProviderSource,
    provide_hourly_factor_contract,
)
from .simulation import (
    HawkesExcitationState,
    HawkesKernelParameters,
    SeededTickSimulationValidationError,
)

DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA = 1.0


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingValidationFailure:
    field_path: str
    message: str


class VanillaOptionPricingValidationError(ValueError):
    def __init__(self, failure: VanillaOptionPricingValidationFailure) -> None:
        self.failure: VanillaOptionPricingValidationFailure = failure
        super().__init__(f"{failure.field_path}: {failure.message}")


def _pricing_error(field_path: str, message: str) -> VanillaOptionPricingValidationError:
    return VanillaOptionPricingValidationError(
        VanillaOptionPricingValidationFailure(field_path=field_path, message=message)
    )


PRICING_ERROR_FACTORY: ValidationErrorFactory = _pricing_error


class VanillaOptionPayoffType(StrEnum):
    CALL = 'call'
    PUT = 'put'


@dataclass(frozen=True, slots=True)
class GammaSeverityMarkConfiguration(ContractModel):
    """Gamma mark law used for news severity in the pricing contract."""

    shape_k: float
    scale_theta: float

    def __post_init__(self) -> None:
        require_positive_finite_number(
            {"shape_k": self.shape_k},
            "shape_k",
            "shape_k",
            PRICING_ERROR_FACTORY,
        )
        require_positive_finite_number(
            {"scale_theta": self.scale_theta},
            "scale_theta",
            "scale_theta",
            PRICING_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> GammaSeverityMarkConfiguration:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return GammaSeverityMarkConfiguration(
            shape_k=require_positive_finite_number(
                raw_payload,
                "shape_k",
                f"{field_path}.shape_k",
                PRICING_ERROR_FACTORY,
            ),
            scale_theta=require_positive_finite_number(
                raw_payload,
                "scale_theta",
                f"{field_path}.scale_theta",
                PRICING_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class NigSharedPlusShiftConfiguration(ContractModel):
    """Shared NIG core with one sell-side skew shift."""

    alpha: float
    beta_0: float
    delta: float
    mu: float
    delta_beta: float

    def __post_init__(self) -> None:
        require_positive_finite_number(
            {"alpha": self.alpha},
            "alpha",
            "alpha",
            PRICING_ERROR_FACTORY,
        )
        require_finite_number({"beta_0": self.beta_0}, "beta_0", "beta_0", PRICING_ERROR_FACTORY)
        require_positive_finite_number(
            {"delta": self.delta},
            "delta",
            "delta",
            PRICING_ERROR_FACTORY,
        )
        require_finite_number({"mu": self.mu}, "mu", "mu", PRICING_ERROR_FACTORY)
        require_finite_number(
            {"delta_beta": self.delta_beta},
            "delta_beta",
            "delta_beta",
            PRICING_ERROR_FACTORY,
        )

        if self.delta_beta >= 0.0:
            raise _pricing_error("delta_beta", "must be strictly negative")
        if self.alpha <= abs(self.beta_0):
            raise _pricing_error("alpha", "must be strictly greater than abs(beta_0)")
        if self.alpha <= abs(self.beta_0 + self.delta_beta):
            raise _pricing_error(
                "delta_beta",
                "must satisfy abs(beta_0 + delta_beta) < alpha",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> NigSharedPlusShiftConfiguration:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        alpha = require_positive_finite_number(
            raw_payload,
            "alpha",
            f"{field_path}.alpha",
            PRICING_ERROR_FACTORY,
        )
        beta_0 = require_finite_number(
            raw_payload,
            "beta_0",
            f"{field_path}.beta_0",
            PRICING_ERROR_FACTORY,
        )
        delta = require_positive_finite_number(
            raw_payload,
            "delta",
            f"{field_path}.delta",
            PRICING_ERROR_FACTORY,
        )
        mu = require_finite_number(raw_payload, "mu", f"{field_path}.mu", PRICING_ERROR_FACTORY)
        delta_beta = require_finite_number(
            raw_payload,
            "delta_beta",
            f"{field_path}.delta_beta",
            PRICING_ERROR_FACTORY,
        )

        if delta_beta >= 0.0:
            raise _pricing_error(f"{field_path}.delta_beta", "must be strictly negative")

        if alpha <= abs(beta_0):
            raise _pricing_error(
                f"{field_path}.alpha",
                f"alpha must be strictly greater than abs(beta_0), received alpha={alpha} and beta_0={beta_0}",
            )

        if alpha <= abs(beta_0 + delta_beta):
            raise _pricing_error(
                f"{field_path}.delta_beta",
                (
                    "alpha must be strictly greater than abs(beta_0 + delta_beta), "
                    f"received alpha={alpha}, beta_0={beta_0}, delta_beta={delta_beta}"
                ),
            )

        return NigSharedPlusShiftConfiguration(
            alpha=alpha,
            beta_0=beta_0,
            delta=delta,
            mu=mu,
            delta_beta=delta_beta,
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPayoffConfiguration(ContractModel):
    """Payoff selection for vanilla pricing."""

    payoff_type: VanillaOptionPayoffType

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> VanillaOptionPayoffConfiguration:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        raw_payoff_type = require_string(
            raw_payload,
            "payoff_type",
            f"{field_path}.payoff_type",
            PRICING_ERROR_FACTORY,
        )
        try:
            payoff_type = VanillaOptionPayoffType(raw_payoff_type)
        except ValueError as e:
            raise _pricing_error(
                f"{field_path}.payoff_type",
                f"must be one of: call, put, received '{raw_payoff_type}'",
            ) from e

        return VanillaOptionPayoffConfiguration(payoff_type=payoff_type)


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingMarketInputs(ContractModel):
    """Market inputs required for one vanilla option valuation."""

    spot: float
    strike: float
    maturity_interval: float
    risk_free_rate: float
    dividend_yield: float

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> VanillaOptionPricingMarketInputs:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return VanillaOptionPricingMarketInputs(
            spot=require_positive_finite_number(
                raw_payload,
                "spot",
                f"{field_path}.spot",
                PRICING_ERROR_FACTORY,
            ),
            strike=require_positive_finite_number(
                raw_payload,
                "strike",
                f"{field_path}.strike",
                PRICING_ERROR_FACTORY,
            ),
            maturity_interval=require_positive_finite_number(
                raw_payload,
                "maturity_interval",
                f"{field_path}.maturity_interval",
                PRICING_ERROR_FACTORY,
            ),
            risk_free_rate=require_finite_number(
                raw_payload,
                "risk_free_rate",
                f"{field_path}.risk_free_rate",
                PRICING_ERROR_FACTORY,
            ),
            dividend_yield=require_finite_number(
                raw_payload,
                "dividend_yield",
                f"{field_path}.dividend_yield",
                PRICING_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingModelParameters(ContractModel):
    """Model-side pricing inputs across Hawkes, news, and NIG layers."""

    hawkes_kernel_parameters: HawkesKernelParameters
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> VanillaOptionPricingModelParameters:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        try:
            return VanillaOptionPricingModelParameters(
                hawkes_kernel_parameters=HawkesKernelParameters.from_mapping(
                    require_mapping(
                        raw_payload,
                        "hawkes_kernel_parameters",
                        f"{field_path}.hawkes_kernel_parameters",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.hawkes_kernel_parameters",
                ),
                gamma_severity_mark_configuration=GammaSeverityMarkConfiguration.from_mapping(
                    require_mapping(
                        raw_payload,
                        "gamma_severity_mark_configuration",
                        f"{field_path}.gamma_severity_mark_configuration",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.gamma_severity_mark_configuration",
                ),
                nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration.from_mapping(
                    require_mapping(
                        raw_payload,
                        "nig_shared_plus_shift_configuration",
                        f"{field_path}.nig_shared_plus_shift_configuration",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.nig_shared_plus_shift_configuration",
                ),
            )
        except SeededTickSimulationValidationError as e:
            raise _pricing_error(e.failure.field_path, e.failure.message) from e


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingRequest(ContractModel):
    """Typed pricing request with hourly intraday default `interval_delta=1.0`."""

    market_inputs: VanillaOptionPricingMarketInputs
    payoff: VanillaOptionPayoffConfiguration
    boundary_state: HawkesExcitationState
    model_parameters: VanillaOptionPricingModelParameters
    interval_delta: float = DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA

    def __post_init__(self) -> None:
        require_positive_finite_scalar(
            self.interval_delta,
            "interval_delta",
            PRICING_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> VanillaOptionPricingRequest:
        if not isinstance(raw_payload, dict):
            raise _pricing_error("vanilla_option_pricing_request", "must be an object")

        interval_delta = (
            require_positive_finite_number(
                raw_payload,
                "interval_delta",
                "interval_delta",
                PRICING_ERROR_FACTORY,
            )
            if "interval_delta" in raw_payload
            else DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA
        )

        try:
            return VanillaOptionPricingRequest(
                market_inputs=VanillaOptionPricingMarketInputs.from_mapping(
                    require_mapping(raw_payload, "market_inputs", "market_inputs", PRICING_ERROR_FACTORY),
                    field_path="market_inputs",
                ),
                payoff=VanillaOptionPayoffConfiguration.from_mapping(
                    require_mapping(raw_payload, "payoff", "payoff", PRICING_ERROR_FACTORY),
                    field_path="payoff",
                ),
                boundary_state=HawkesExcitationState.from_mapping(
                    require_mapping(raw_payload, "boundary_state", "boundary_state", PRICING_ERROR_FACTORY),
                    field_path="boundary_state",
                ),
                model_parameters=VanillaOptionPricingModelParameters.from_mapping(
                    require_mapping(raw_payload, "model_parameters", "model_parameters", PRICING_ERROR_FACTORY),
                    field_path="model_parameters",
                ),
                interval_delta=interval_delta,
            )
        except SeededTickSimulationValidationError as e:
            raise _pricing_error(e.failure.field_path, e.failure.message) from e


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingDiagnostics(ContractModel):
    """User-facing diagnostics returned alongside the option price."""

    forward_price: float
    discount_factor: float
    maturity_interval: float
    boundary_state_norm: float

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> VanillaOptionPricingDiagnostics:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return VanillaOptionPricingDiagnostics(
            forward_price=require_positive_finite_number(
                raw_payload,
                "forward_price",
                f"{field_path}.forward_price",
                PRICING_ERROR_FACTORY,
            ),
            discount_factor=require_positive_finite_number(
                raw_payload,
                "discount_factor",
                f"{field_path}.discount_factor",
                PRICING_ERROR_FACTORY,
            ),
            maturity_interval=require_positive_finite_number(
                raw_payload,
                "maturity_interval",
                f"{field_path}.maturity_interval",
                PRICING_ERROR_FACTORY,
            ),
            boundary_state_norm=require_non_negative_finite_number(
                raw_payload,
                "boundary_state_norm",
                f"{field_path}.boundary_state_norm",
                PRICING_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingResponse(ContractModel):
    """Typed pricing response."""

    price: float
    diagnostics: VanillaOptionPricingDiagnostics

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> VanillaOptionPricingResponse:
        if not isinstance(raw_payload, dict):
            raise _pricing_error("vanilla_option_pricing_response", "must be an object")

        return VanillaOptionPricingResponse(
            price=require_non_negative_finite_number(
                raw_payload,
                "price",
                "price",
                PRICING_ERROR_FACTORY,
            ),
            diagnostics=VanillaOptionPricingDiagnostics.from_mapping(
                require_mapping(raw_payload, "diagnostics", "diagnostics", PRICING_ERROR_FACTORY),
                field_path="diagnostics",
            ),
        )


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

    parsed_interval_delta = require_positive_finite_scalar(
        interval_delta,
        "interval_delta",
        PRICING_ERROR_FACTORY,
    )
    if parsed_interval_delta != DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA:
        raise _pricing_error(
            "interval_delta",
            (
                "hourly factor provider supports only interval_delta=1.0 "
                f"for hourly boundaries, received {parsed_interval_delta}"
            ),
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
    raw_request: dict[str, Any] | Any,
) -> VanillaOptionPricingRequest:
    return VanillaOptionPricingRequest.from_mapping(raw_request)


def validate_vanilla_option_pricing_response(
    raw_response: dict[str, Any] | Any,
) -> VanillaOptionPricingResponse:
    return VanillaOptionPricingResponse.from_mapping(raw_response)


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
