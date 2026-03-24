from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from types import ModuleType
from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional

from .contracts import (
    ContractModel,
    ValidationErrorFactory,
    parse_json_object,
    require_finite_number,
    require_mapping,
    require_non_negative_finite_number,
    require_non_negative_finite_scalar,
    require_positive_finite_number,
    require_positive_finite_scalar,
    require_string,
    serialize_deterministic_json,
    serialize_pretty_json,
    validate_exact_string,
    validate_required_field_list,
)
from ._native_api import native_exception_matches, native_module
from .calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
)
from .errors import (
    ContractValidationFailure,
    JaniformErrorDetails,
    JaniformErrorCategory,
    JaniformExecutionError,
    JaniformIntegrationError,
    JaniformValidationError,
)
from .hourly_factors import (
    HourlyFactorContract,
    HourlyFactorProviderMissingInputError,
    HourlyFactorProviderUnsupportedRangeError,
    HourlyNewsIntensityProviderSource,
    HourlySeasonalityProviderSource,
    provide_hourly_factor_contract,
)
from .seasonality import SeasonalityInputValidationError
if TYPE_CHECKING:
    from .simulation import HawkesExcitationState, HawkesKernelParameters

DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA = 1.0
VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION = "pricing.vanilla_option.run_result.v1"
VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION = "pricing.vanilla_option.run_diagnostics.v1"
VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION = "pricing.vanilla_option.calibration_hooks.v1"
VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA_VERSION = (
    "pricing.vanilla_option.run_output_schema_metadata.v1"
)
VANILLA_OPTION_CALIBRATION_HOOK_POLICY = (
    "derive_option_surface_inputs_from_pricing_response_and_optional_run_diagnostics.v1"
)
VANILLA_OPTION_PRICING_RUN_RESULT_REQUIRED_FIELDS: tuple[str, ...] = (
    "schema_version",
    "output_schema_metadata",
    "response",
    "run_diagnostics",
    "calibration_hooks",
)
VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_REQUIRED_FIELDS: tuple[str, ...] = (
    "schema_version",
    "transform",
    "integration_quality",
)
VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_REQUIRED_FIELDS: tuple[str, ...] = (
    "schema_version",
    "hook_policy",
    "option_price",
    "log_moneyness",
    "forward_moneyness",
    "maturity_interval",
    "transform_status",
    "integration_numeric_quality_is_poor",
    "integration_convergence_check_passed",
    "integration_truncation_check_passed",
)
PRICING_VALIDATION_ERROR_ID = "pricing.validation.contract"
PRICING_EXECUTION_ERROR_ID = "pricing.execution.run_failed"
PRICING_INTEGRATION_NATIVE_MODULE_ERROR_ID = "pricing.integration.native_module_unavailable"
PRICING_INTEGRATION_RESPONSE_ERROR_ID = "pricing.integration.invalid_native_response"
PRICING_INTEGRATION_HOURLY_FACTOR_ERROR_ID = "pricing.integration.hourly_factor_contract_unavailable"

VanillaOptionPricingValidationFailure = ContractValidationFailure


class VanillaOptionPricingValidationError(JaniformValidationError):
    pass


def _pricing_failure(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = PRICING_VALIDATION_ERROR_ID,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> VanillaOptionPricingValidationFailure:
    return VanillaOptionPricingValidationFailure.from_context(
        error_id=error_id,
        workflow="pricing",
        entrypoint=entrypoint,
        field_path=field_path,
        message=message,
        context=context,
    )


def _pricing_error(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = PRICING_VALIDATION_ERROR_ID,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> VanillaOptionPricingValidationError:
    return VanillaOptionPricingValidationError(
        _pricing_failure(
            field_path,
            message,
            entrypoint=entrypoint,
            error_id=error_id,
            context=context,
        )
    )


def _pricing_integration_error(
    message: str,
    *,
    entrypoint: str,
    error_id: str,
    field_path: Optional[str] = None,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> JaniformIntegrationError:
    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=error_id,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="pricing",
            entrypoint=entrypoint,
            message=message,
            field_path=field_path,
            context=context,
        )
    )


def _pricing_execution_error(
    message: str,
    *,
    entrypoint: str,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> JaniformExecutionError:
    return JaniformExecutionError(
        JaniformErrorDetails.from_context(
            error_id=PRICING_EXECUTION_ERROR_ID,
            category=JaniformErrorCategory.EXECUTION,
            workflow="pricing",
            entrypoint=entrypoint,
            message=message,
            context=context,
        )
    )


PRICING_ERROR_FACTORY: ValidationErrorFactory = _pricing_error


def _parse_hawkes_kernel_parameters(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> Any:
    from .simulation import HawkesKernelParameters

    return HawkesKernelParameters.from_mapping(raw_payload, field_path=field_path)


def _parse_hawkes_excitation_state(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> Any:
    from .simulation import HawkesExcitationState

    return HawkesExcitationState.from_mapping(raw_payload, field_path=field_path)


def require_boolean(
    mapping: dict[str, Any] | Any,
    field_name: str,
    field_path: str,
) -> bool:
    if field_name not in mapping:
        raise _pricing_error(field_path, "is required")

    raw_value = mapping[field_name]
    if not isinstance(raw_value, bool):
        raise _pricing_error(field_path, "must be a boolean")

    return raw_value


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
                hawkes_kernel_parameters=_parse_hawkes_kernel_parameters(
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
        except JaniformValidationError as e:
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
                boundary_state=_parse_hawkes_excitation_state(
                    require_mapping(raw_payload, "boundary_state", "boundary_state", PRICING_ERROR_FACTORY),
                    field_path="boundary_state",
                ),
                model_parameters=VanillaOptionPricingModelParameters.from_mapping(
                    require_mapping(raw_payload, "model_parameters", "model_parameters", PRICING_ERROR_FACTORY),
                    field_path="model_parameters",
                ),
                interval_delta=interval_delta,
            )
        except JaniformValidationError as e:
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


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingOptionalOutputSelection(ContractModel):
    include_transform_diagnostics: bool = False
    include_integration_quality_diagnostics: bool = False
    include_calibration_hooks: bool = False

    def __post_init__(self) -> None:
        if not isinstance(self.include_transform_diagnostics, bool):
            raise _pricing_error("include_transform_diagnostics", "must be a boolean")
        if not isinstance(self.include_integration_quality_diagnostics, bool):
            raise _pricing_error(
                "include_integration_quality_diagnostics",
                "must be a boolean",
            )
        if not isinstance(self.include_calibration_hooks, bool):
            raise _pricing_error("include_calibration_hooks", "must be a boolean")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> VanillaOptionPricingOptionalOutputSelection:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=require_boolean(
                raw_payload,
                "include_transform_diagnostics",
                f"{field_path}.include_transform_diagnostics",
            ),
            include_integration_quality_diagnostics=require_boolean(
                raw_payload,
                "include_integration_quality_diagnostics",
                f"{field_path}.include_integration_quality_diagnostics",
            ),
            include_calibration_hooks=require_boolean(
                raw_payload,
                "include_calibration_hooks",
                f"{field_path}.include_calibration_hooks",
            ),
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingIntervalFactors(ContractModel):
    """One-interval pricing factors with split known and surprise news intensity."""

    s_h: float
    nu_h_known: float
    nu_h_surprise: float

    @property
    def nu_h(self) -> float:
        return self.nu_h_known + self.nu_h_surprise

    def __post_init__(self) -> None:
        require_non_negative_finite_scalar(self.s_h, "s_h", PRICING_ERROR_FACTORY)
        require_non_negative_finite_scalar(self.nu_h_known, "nu_h_known", PRICING_ERROR_FACTORY)
        require_non_negative_finite_scalar(
            self.nu_h_surprise,
            "nu_h_surprise",
            PRICING_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> VanillaOptionPricingIntervalFactors:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return VanillaOptionPricingIntervalFactors(
            s_h=require_non_negative_finite_number(
                raw_payload,
                "s_h",
                f"{field_path}.s_h",
                PRICING_ERROR_FACTORY,
            ),
            nu_h_known=require_non_negative_finite_number(
                raw_payload,
                "nu_h_known",
                f"{field_path}.nu_h_known",
                PRICING_ERROR_FACTORY,
            ),
            nu_h_surprise=require_non_negative_finite_number(
                raw_payload,
                "nu_h_surprise",
                f"{field_path}.nu_h_surprise",
                PRICING_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingExecutionInput(ContractModel):
    request: VanillaOptionPricingRequest
    interval_factors: VanillaOptionPricingIntervalFactors
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection = (
        VanillaOptionPricingOptionalOutputSelection()
    )

    def __post_init__(self) -> None:
        if not isinstance(self.request, VanillaOptionPricingRequest):
            raise _pricing_error("request", "must be a VanillaOptionPricingRequest")
        if not isinstance(self.interval_factors, VanillaOptionPricingIntervalFactors):
            raise _pricing_error("interval_factors", "must be VanillaOptionPricingIntervalFactors")
        if not isinstance(
            self.optional_output_selection,
            VanillaOptionPricingOptionalOutputSelection,
        ):
            raise _pricing_error(
                "optional_output_selection",
                "must be VanillaOptionPricingOptionalOutputSelection",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
    ) -> VanillaOptionPricingExecutionInput:
        if not isinstance(raw_payload, dict):
            raise _pricing_error("vanilla_option_pricing_execution_input", "must be an object")

        optional_output_selection = (
            VanillaOptionPricingOptionalOutputSelection.from_mapping(
                require_mapping(
                    raw_payload,
                    "optional_output_selection",
                    "optional_output_selection",
                    PRICING_ERROR_FACTORY,
                ),
                field_path="optional_output_selection",
            )
            if "optional_output_selection" in raw_payload
            else VanillaOptionPricingOptionalOutputSelection()
        )

        return VanillaOptionPricingExecutionInput(
            request=VanillaOptionPricingRequest.from_mapping(
                require_mapping(raw_payload, "request", "request", PRICING_ERROR_FACTORY)
            ),
            interval_factors=VanillaOptionPricingIntervalFactors.from_mapping(
                require_mapping(raw_payload, "interval_factors", "interval_factors", PRICING_ERROR_FACTORY),
                field_path="interval_factors",
            ),
            optional_output_selection=optional_output_selection,
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingRunOutputSchemaMetadata(ContractModel):
    schema_version: str
    run_result_schema_version: str
    run_diagnostics_schema_version: str
    calibration_hooks_schema_version: str
    required_run_result_fields: tuple[str, ...]
    required_run_diagnostics_fields: tuple[str, ...]
    required_calibration_hooks_fields: tuple[str, ...]

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> VanillaOptionPricingRunOutputSchemaMetadata:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        validate_exact_string(
            raw_payload,
            "schema_version",
            f"{field_path}.schema_version",
            VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA_VERSION,
            PRICING_ERROR_FACTORY,
        )
        validate_exact_string(
            raw_payload,
            "run_result_schema_version",
            f"{field_path}.run_result_schema_version",
            VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION,
            PRICING_ERROR_FACTORY,
        )
        validate_exact_string(
            raw_payload,
            "run_diagnostics_schema_version",
            f"{field_path}.run_diagnostics_schema_version",
            VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION,
            PRICING_ERROR_FACTORY,
        )
        validate_exact_string(
            raw_payload,
            "calibration_hooks_schema_version",
            f"{field_path}.calibration_hooks_schema_version",
            VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION,
            PRICING_ERROR_FACTORY,
        )
        validate_required_field_list(
            raw_payload,
            "required_run_result_fields",
            f"{field_path}.required_run_result_fields",
            VANILLA_OPTION_PRICING_RUN_RESULT_REQUIRED_FIELDS,
            PRICING_ERROR_FACTORY,
        )
        validate_required_field_list(
            raw_payload,
            "required_run_diagnostics_fields",
            f"{field_path}.required_run_diagnostics_fields",
            VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_REQUIRED_FIELDS,
            PRICING_ERROR_FACTORY,
        )
        validate_required_field_list(
            raw_payload,
            "required_calibration_hooks_fields",
            f"{field_path}.required_calibration_hooks_fields",
            VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_REQUIRED_FIELDS,
            PRICING_ERROR_FACTORY,
        )

        return VanillaOptionPricingRunOutputSchemaMetadata(
            schema_version=VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA_VERSION,
            run_result_schema_version=VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION,
            run_diagnostics_schema_version=VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION,
            calibration_hooks_schema_version=VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION,
            required_run_result_fields=VANILLA_OPTION_PRICING_RUN_RESULT_REQUIRED_FIELDS,
            required_run_diagnostics_fields=VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_REQUIRED_FIELDS,
            required_calibration_hooks_fields=VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_REQUIRED_FIELDS,
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingRunDiagnostics(ContractModel):
    schema_version: str
    transform: Optional[dict[str, Any]]
    integration_quality: Optional[dict[str, Any]]

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> VanillaOptionPricingRunDiagnostics:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        validate_exact_string(
            raw_payload,
            "schema_version",
            f"{field_path}.schema_version",
            VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION,
            PRICING_ERROR_FACTORY,
        )

        if "transform" not in raw_payload:
            raise _pricing_error(f"{field_path}.transform", "is required")
        if "integration_quality" not in raw_payload:
            raise _pricing_error(f"{field_path}.integration_quality", "is required")

        raw_transform = raw_payload["transform"]
        transform = (
            dict(raw_transform)
            if isinstance(raw_transform, dict)
            else None
            if raw_transform is None
            else None
        )
        if raw_transform is not None and transform is None:
            raise _pricing_error(f"{field_path}.transform", "must be an object or null")

        raw_integration_quality = raw_payload["integration_quality"]
        integration_quality = (
            dict(raw_integration_quality)
            if isinstance(raw_integration_quality, dict)
            else None
            if raw_integration_quality is None
            else None
        )
        if raw_integration_quality is not None and integration_quality is None:
            raise _pricing_error(
                f"{field_path}.integration_quality",
                "must be an object or null",
            )

        if transform is None and integration_quality is None:
            raise _pricing_error(
                field_path,
                "must include transform diagnostics, integration quality diagnostics, or both",
            )

        return VanillaOptionPricingRunDiagnostics(
            schema_version=VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION,
            transform=transform,
            integration_quality=integration_quality,
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingCalibrationHooks(ContractModel):
    schema_version: str
    hook_policy: str
    option_price: float
    log_moneyness: float
    forward_moneyness: float
    maturity_interval: float
    transform_status: Optional[str]
    integration_numeric_quality_is_poor: Optional[bool]
    integration_convergence_check_passed: Optional[bool]
    integration_truncation_check_passed: Optional[bool]

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> VanillaOptionPricingCalibrationHooks:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        validate_exact_string(
            raw_payload,
            "schema_version",
            f"{field_path}.schema_version",
            VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION,
            PRICING_ERROR_FACTORY,
        )
        validate_exact_string(
            raw_payload,
            "hook_policy",
            f"{field_path}.hook_policy",
            VANILLA_OPTION_CALIBRATION_HOOK_POLICY,
            PRICING_ERROR_FACTORY,
        )

        transform_status_raw = raw_payload.get("transform_status")
        if transform_status_raw is not None and not isinstance(transform_status_raw, str):
            raise _pricing_error(f"{field_path}.transform_status", "must be a string or null")

        return VanillaOptionPricingCalibrationHooks(
            schema_version=VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION,
            hook_policy=VANILLA_OPTION_CALIBRATION_HOOK_POLICY,
            option_price=require_non_negative_finite_number(
                raw_payload,
                "option_price",
                f"{field_path}.option_price",
                PRICING_ERROR_FACTORY,
            ),
            log_moneyness=require_finite_number(
                raw_payload,
                "log_moneyness",
                f"{field_path}.log_moneyness",
                PRICING_ERROR_FACTORY,
            ),
            forward_moneyness=require_positive_finite_number(
                raw_payload,
                "forward_moneyness",
                f"{field_path}.forward_moneyness",
                PRICING_ERROR_FACTORY,
            ),
            maturity_interval=require_positive_finite_number(
                raw_payload,
                "maturity_interval",
                f"{field_path}.maturity_interval",
                PRICING_ERROR_FACTORY,
            ),
            transform_status=transform_status_raw,
            integration_numeric_quality_is_poor=_optional_boolean(
                raw_payload,
                "integration_numeric_quality_is_poor",
                f"{field_path}.integration_numeric_quality_is_poor",
            ),
            integration_convergence_check_passed=_optional_boolean(
                raw_payload,
                "integration_convergence_check_passed",
                f"{field_path}.integration_convergence_check_passed",
            ),
            integration_truncation_check_passed=_optional_boolean(
                raw_payload,
                "integration_truncation_check_passed",
                f"{field_path}.integration_truncation_check_passed",
            ),
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingRunResult(ContractModel):
    schema_version: str
    output_schema_metadata: VanillaOptionPricingRunOutputSchemaMetadata
    response: VanillaOptionPricingResponse
    run_diagnostics: Optional[VanillaOptionPricingRunDiagnostics]
    calibration_hooks: Optional[VanillaOptionPricingCalibrationHooks]

    def save(self, path: str | Path) -> None:
        Path(path).write_text(serialize_pretty_json(self), encoding="utf-8")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
    ) -> VanillaOptionPricingRunResult:
        if not isinstance(raw_payload, dict):
            raise _pricing_error("vanilla_option_pricing_run_result", "must be an object")

        validate_exact_string(
            raw_payload,
            "schema_version",
            "schema_version",
            VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION,
            PRICING_ERROR_FACTORY,
        )

        run_diagnostics = None
        if "run_diagnostics" not in raw_payload:
            raise _pricing_error("run_diagnostics", "is required")

        raw_run_diagnostics = raw_payload["run_diagnostics"]
        if raw_run_diagnostics is not None:
            run_diagnostics = VanillaOptionPricingRunDiagnostics.from_mapping(
                raw_run_diagnostics,
                field_path="run_diagnostics",
            )

        calibration_hooks = None
        if "calibration_hooks" not in raw_payload:
            raise _pricing_error("calibration_hooks", "is required")

        raw_calibration_hooks = raw_payload["calibration_hooks"]
        if raw_calibration_hooks is not None:
            calibration_hooks = VanillaOptionPricingCalibrationHooks.from_mapping(
                raw_calibration_hooks,
                field_path="calibration_hooks",
            )

        return VanillaOptionPricingRunResult(
            schema_version=VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION,
            output_schema_metadata=VanillaOptionPricingRunOutputSchemaMetadata.from_mapping(
                require_mapping(
                    raw_payload,
                    "output_schema_metadata",
                    "output_schema_metadata",
                    PRICING_ERROR_FACTORY,
                ),
                field_path="output_schema_metadata",
            ),
            response=VanillaOptionPricingResponse.from_mapping(
                require_mapping(raw_payload, "response", "response", PRICING_ERROR_FACTORY)
            ),
            run_diagnostics=run_diagnostics,
            calibration_hooks=calibration_hooks,
        )


def _optional_boolean(
    raw_payload: dict[str, Any],
    field_name: str,
    field_path: str,
) -> Optional[bool]:
    if field_name not in raw_payload:
        raise _pricing_error(field_path, "is required")

    raw_value = raw_payload[field_name]
    if raw_value is None:
        return None
    if not isinstance(raw_value, bool):
        raise _pricing_error(field_path, "must be a boolean or null")

    return raw_value


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

    try:
        return provide_hourly_factor_contract(
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            seasonality_source=seasonality_source,
            news_intensity_source=news_intensity_source,
        )
    except (
        CalendarInvalidTimestampError,
        CalendarInvalidTimezoneError,
        CalendarMissingExchangeError,
        HourlyFactorProviderMissingInputError,
        HourlyFactorProviderUnsupportedRangeError,
        SeasonalityInputValidationError,
    ) as e:
        raise _pricing_error(
            "hourly_factor_contract",
            str(e),
            entrypoint="load_hourly_factor_contract_for_pricing",
            context={"exchange_code": exchange_code},
        ) from e
    except ModuleNotFoundError as e:
        raise _pricing_integration_error(
            "native hourly-factor support is unavailable",
            entrypoint="load_hourly_factor_contract_for_pricing",
            error_id=PRICING_INTEGRATION_HOURLY_FACTOR_ERROR_ID,
            field_path="hourly_factor_contract",
            context={"exchange_code": exchange_code},
        ) from e


def validate_vanilla_option_pricing_request(
    raw_request: dict[str, Any] | Any,
) -> VanillaOptionPricingRequest:
    return VanillaOptionPricingRequest.from_mapping(raw_request)


def validate_vanilla_option_pricing_response(
    raw_response: dict[str, Any] | Any,
) -> VanillaOptionPricingResponse:
    return VanillaOptionPricingResponse.from_mapping(raw_response)


def validate_vanilla_option_pricing_execution_input(
    raw_execution_input: dict[str, Any] | Any,
) -> VanillaOptionPricingExecutionInput:
    return VanillaOptionPricingExecutionInput.from_mapping(raw_execution_input)


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


def serialize_vanilla_option_pricing_execution_input(
    execution_input: VanillaOptionPricingExecutionInput,
) -> str:
    return serialize_deterministic_json(execution_input)


def deserialize_vanilla_option_pricing_execution_input(
    payload_json: str,
) -> VanillaOptionPricingExecutionInput:
    raw_payload = parse_json_object(
        payload_json,
        "vanilla option pricing execution input",
        PRICING_ERROR_FACTORY,
    )
    return validate_vanilla_option_pricing_execution_input(dict(raw_payload))


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

    normalized_execution_input = validate_vanilla_option_pricing_execution_input(
        execution_input.to_payload()
    )

    try:
        native: ModuleType = native_module()
    except ModuleNotFoundError as e:
        raise _pricing_integration_error(
            "native pricing module is unavailable",
            entrypoint="run_vanilla_option_pricing",
            error_id=PRICING_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    try:
        raw_run_result = native.run_vanilla_option_pricing(
            normalized_execution_input.to_payload()
        )
    except Exception as e:
        if native_exception_matches(native, e, "PricingExecutionError"):
            raise _pricing_execution_error(
                "vanilla option pricing failed during native execution",
                entrypoint="run_vanilla_option_pricing",
                context={"source_exception_type": type(e).__name__},
            ) from e

        raise _pricing_execution_error(
            "vanilla option pricing failed during native execution",
            entrypoint="run_vanilla_option_pricing",
            context={"source_exception_type": type(e).__name__},
        ) from e

    if not isinstance(raw_run_result, dict):
        raise _pricing_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint="run_vanilla_option_pricing",
            error_id=PRICING_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_run_result).__name__},
        )

    try:
        return validate_vanilla_option_pricing_run_result(raw_run_result)
    except VanillaOptionPricingValidationError as e:
        raise _pricing_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint="run_vanilla_option_pricing",
            error_id=PRICING_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e


__all__ = [
    "GammaSeverityMarkConfiguration",
    "NigSharedPlusShiftConfiguration",
    "VanillaOptionPricingCalibrationHooks",
    "VanillaOptionPricingExecutionInput",
    "VanillaOptionPricingIntervalFactors",
    "VanillaOptionPricingOptionalOutputSelection",
    "VanillaOptionPayoffConfiguration",
    "VanillaOptionPayoffType",
    "VanillaOptionPricingDiagnostics",
    "VanillaOptionPricingMarketInputs",
    "VanillaOptionPricingModelParameters",
    "VanillaOptionPricingRequest",
    "VanillaOptionPricingResponse",
    "VanillaOptionPricingRunDiagnostics",
    "VanillaOptionPricingRunOutputSchemaMetadata",
    "VanillaOptionPricingRunResult",
    "VanillaOptionPricingValidationError",
    "VanillaOptionPricingValidationFailure",
    "deserialize_vanilla_option_pricing_execution_input",
    "deserialize_vanilla_option_pricing_request",
    "deserialize_vanilla_option_pricing_response",
    "deserialize_vanilla_option_pricing_run_result",
    "load_hourly_factor_contract_for_pricing",
    "run_vanilla_option_pricing",
    "serialize_vanilla_option_pricing_execution_input",
    "serialize_vanilla_option_pricing_request",
    "serialize_vanilla_option_pricing_response",
    "serialize_vanilla_option_pricing_run_result",
    "validate_vanilla_option_pricing_execution_input",
    "validate_vanilla_option_pricing_request",
    "validate_vanilla_option_pricing_response",
    "validate_vanilla_option_pricing_run_result",
]
