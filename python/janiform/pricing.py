from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from math import exp
from pathlib import Path
from types import ModuleType
from typing import TYPE_CHECKING, Any, Optional

from .contracts.base import (
    require_boolean,
    require_optional_boolean_value,
    require_optional_mapping_value,
    require_optional_string_value,
    require_str_enum_value,
)
from .contracts import (
    ContractModel,
    MeanReversionAnchorModel,
    MeanReversionAnchorModelKind,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    ValidationErrorFactory,
    parse_json_object,
    require_finite_number,
    require_mapping,
    require_non_negative_integer,
    require_non_negative_finite_number,
    require_positive_finite_number,
    require_positive_integer,
    require_positive_finite_scalar,
    require_string,
    serialize_deterministic_json,
    serialize_pretty_json,
    validate_exact_string,
    validate_required_field_list,
)
from ._native_api import native_exception_matches, native_module
from ._timestamps import hours_to_exact_nanoseconds
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
)

if TYPE_CHECKING:
    from .simulation import HawkesExcitationState, HawkesKernelParameters
    from .simulation.coarse.news import NewsProcessConfiguration

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
    "anchored_recursion",
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
PRICING_LOCAL_ANCHORED_TILT_SOLVE_POLICY = (
    "solve_state_dependent_local_tilt_against_endogenous_q_martingale_target.v1"
)
_PRICING_MEAN_REVERSION_ALIGNMENT_TOLERANCE = 1e-12

VanillaOptionPricingValidationFailure = ContractValidationFailure


class VanillaOptionPricingValidationError(JaniformValidationError):
    pass


class FourierInversionQuadratureRule(StrEnum):
    MIDPOINT = "midpoint"
    TRAPEZOID = "trapezoid"


class FourierInversionNumericQualityGate(StrEnum):
    DIAGNOSTICS_ONLY = "diagnostics_only"
    ENFORCE_FAILURE = "enforce_failure"


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
):
    from .simulation import HawkesExcitationState

    return HawkesExcitationState.from_mapping(raw_payload, field_path=field_path)


def _parse_news_process_configuration(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
):
    from .simulation.coarse.news import NewsProcessConfiguration

    try:
        return NewsProcessConfiguration.from_mapping(raw_payload)
    except JaniformValidationError as e:
        raise _pricing_error(f"{field_path}.{e.failure.field_path}", e.failure.message) from e


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

        payoff_type = require_str_enum_value(
            require_string(
                raw_payload,
                "payoff_type",
                f"{field_path}.payoff_type",
                PRICING_ERROR_FACTORY,
            ),
            f"{field_path}.payoff_type",
            PRICING_ERROR_FACTORY,
            enum_type=VanillaOptionPayoffType,
            allowed_values_message="must be one of: call, put",
            include_received_value=True,
        )

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
    news_process_configuration: NewsProcessConfiguration
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration

    def __post_init__(self) -> None:
        from .simulation import HawkesKernelParameters
        from .simulation.coarse.news import NewsProcessConfiguration

        if not isinstance(self.hawkes_kernel_parameters, HawkesKernelParameters):
            raise _pricing_error(
                "model_parameters.hawkes_kernel_parameters",
                "must be a HawkesKernelParameters",
            )

        if not isinstance(self.news_process_configuration, NewsProcessConfiguration):
            raise _pricing_error(
                "model_parameters.news_process_configuration",
                "must be a NewsProcessConfiguration",
            )

        if not isinstance(
            self.nig_shared_plus_shift_configuration,
            NigSharedPlusShiftConfiguration,
        ):
            raise _pricing_error(
                "model_parameters.nig_shared_plus_shift_configuration",
                "must be a NigSharedPlusShiftConfiguration",
            )

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
                news_process_configuration=_parse_news_process_configuration(
                    require_mapping(
                        raw_payload,
                        "news_process_configuration",
                        f"{field_path}.news_process_configuration",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.news_process_configuration",
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
class AnchoredVanillaOptionPricingBoundaryInput(ContractModel):
    interval_index: int
    hawkes_boundary_state: HawkesExcitationState
    mean_reversion_fitted_parameters: MeanReversionFittedParameters
    interval_delta: float
    observed_boundary_state: MeanReversionBoundaryState

    def __post_init__(self) -> None:
        from .simulation import HawkesExcitationState

        if not isinstance(self.hawkes_boundary_state, HawkesExcitationState):
            raise _pricing_error(
                "boundary_input.hawkes_boundary_state",
                "must be a HawkesExcitationState",
            )

        if not isinstance(
            self.mean_reversion_fitted_parameters,
            MeanReversionFittedParameters,
        ):
            raise _pricing_error(
                "boundary_input.mean_reversion_fitted_parameters",
                "must be a MeanReversionFittedParameters",
            )

        if not isinstance(self.observed_boundary_state, MeanReversionBoundaryState):
            raise _pricing_error(
                "boundary_input.observed_boundary_state",
                "must be a MeanReversionBoundaryState",
            )

        if self.interval_index < 0:
            raise _pricing_error("boundary_input.interval_index", "must be non-negative")

        require_positive_finite_scalar(
            self.interval_delta,
            "boundary_input.interval_delta",
            PRICING_ERROR_FACTORY,
        )

        if (
            self.mean_reversion_fitted_parameters.anchor_model.anchor_model_kind
            != MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL
        ):
            raise _pricing_error(
                "boundary_input.mean_reversion_fitted_parameters.anchor_model.anchor_model_kind",
                "must be endogenous_core_pressure_level",
            )

    @property
    def current_anchor_value(self) -> float:
        return self.observed_boundary_state.anchor_value

    @property
    def deviation_decay_factor(self) -> float:
        return exp(-self.mean_reversion_fitted_parameters.kappa * self.interval_delta)

    @property
    def local_martingale_deviation_adjustment(self) -> float:
        return (self.deviation_decay_factor - 1.0) * self.observed_boundary_state.deviation

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> AnchoredVanillaOptionPricingBoundaryInput:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        try:
            return AnchoredVanillaOptionPricingBoundaryInput(
                interval_index=require_non_negative_integer(
                    raw_payload,
                    "interval_index",
                    f"{field_path}.interval_index",
                    PRICING_ERROR_FACTORY,
                ),
                hawkes_boundary_state=_parse_hawkes_excitation_state(
                    require_mapping(
                        raw_payload,
                        "hawkes_boundary_state",
                        f"{field_path}.hawkes_boundary_state",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.hawkes_boundary_state",
                ),
                mean_reversion_fitted_parameters=MeanReversionFittedParameters.from_mapping(
                    require_mapping(
                        raw_payload,
                        "mean_reversion_fitted_parameters",
                        f"{field_path}.mean_reversion_fitted_parameters",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.mean_reversion_fitted_parameters",
                ),
                interval_delta=require_positive_finite_number(
                    raw_payload,
                    "interval_delta",
                    f"{field_path}.interval_delta",
                    PRICING_ERROR_FACTORY,
                ),
                observed_boundary_state=MeanReversionBoundaryState.from_mapping(
                    require_mapping(
                        raw_payload,
                        "observed_boundary_state",
                        f"{field_path}.observed_boundary_state",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.observed_boundary_state",
                ),
            )
        except JaniformValidationError as e:
            raise _pricing_error(e.failure.field_path, e.failure.message) from e
        except ValueError as e:
            raise _pricing_error(field_path, str(e)) from e


class AnchoredLocalTiltDomainStatus(StrEnum):
    IN_DOMAIN = 'in_domain'
    NON_FINITE_TRANSFORM_INPUT = 'non_finite_transform_input'
    OUT_OF_DOMAIN_STATE_REQUEST = 'out_of_domain_state_request'


class AnchoredLocalTiltAdmissibilityStatus(StrEnum):
    ADMISSIBLE = 'admissible'
    NO_ADMISSIBLE_LOCAL_TILT = 'no_admissible_local_tilt'


@dataclass(frozen=True, slots=True)
class AnchoredLocalTiltBoundaryStateSummary(ContractModel):
    interval_index: int
    hawkes_boundary_state: HawkesExcitationState
    observed_boundary_state: MeanReversionBoundaryState
    current_anchor_value: float
    interval_delta: float
    kappa: float
    deviation_decay_factor: float
    local_martingale_deviation_adjustment: float

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> AnchoredLocalTiltBoundaryStateSummary:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return AnchoredLocalTiltBoundaryStateSummary(
            interval_index=require_non_negative_integer(
                raw_payload,
                "interval_index",
                f"{field_path}.interval_index",
                PRICING_ERROR_FACTORY,
            ),
            hawkes_boundary_state=_parse_hawkes_excitation_state(
                require_mapping(
                    raw_payload,
                    "hawkes_boundary_state",
                    f"{field_path}.hawkes_boundary_state",
                    PRICING_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.hawkes_boundary_state",
            ),
            observed_boundary_state=MeanReversionBoundaryState.from_mapping(
                require_mapping(
                    raw_payload,
                    "observed_boundary_state",
                    f"{field_path}.observed_boundary_state",
                    PRICING_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.observed_boundary_state",
            ),
            current_anchor_value=require_finite_number(
                raw_payload,
                "current_anchor_value",
                f"{field_path}.current_anchor_value",
                PRICING_ERROR_FACTORY,
            ),
            interval_delta=require_positive_finite_number(
                raw_payload,
                "interval_delta",
                f"{field_path}.interval_delta",
                PRICING_ERROR_FACTORY,
            ),
            kappa=require_non_negative_finite_number(
                raw_payload,
                "kappa",
                f"{field_path}.kappa",
                PRICING_ERROR_FACTORY,
            ),
            deviation_decay_factor=require_finite_number(
                raw_payload,
                "deviation_decay_factor",
                f"{field_path}.deviation_decay_factor",
                PRICING_ERROR_FACTORY,
            ),
            local_martingale_deviation_adjustment=require_finite_number(
                raw_payload,
                "local_martingale_deviation_adjustment",
                f"{field_path}.local_martingale_deviation_adjustment",
                PRICING_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class AnchoredLocalTiltSolveDiagnostics(ContractModel):
    boundary_state_summary: AnchoredLocalTiltBoundaryStateSummary
    target_log_moment_exponent: float
    observed_log_moment_exponent: float
    theta: float
    residual: float
    absolute_residual: float
    tolerance: float
    domain_status: AnchoredLocalTiltDomainStatus
    admissibility_status: AnchoredLocalTiltAdmissibilityStatus
    policy: str

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "anchored_local_tilt_solve_diagnostics",
    ) -> AnchoredLocalTiltSolveDiagnostics:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        try:
            return AnchoredLocalTiltSolveDiagnostics(
                boundary_state_summary=AnchoredLocalTiltBoundaryStateSummary.from_mapping(
                    require_mapping(
                        raw_payload,
                        "boundary_state_summary",
                        f"{field_path}.boundary_state_summary",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.boundary_state_summary",
                ),
                target_log_moment_exponent=require_finite_number(
                    raw_payload,
                    "target_log_moment_exponent",
                    f"{field_path}.target_log_moment_exponent",
                    PRICING_ERROR_FACTORY,
                ),
                observed_log_moment_exponent=require_finite_number(
                    raw_payload,
                    "observed_log_moment_exponent",
                    f"{field_path}.observed_log_moment_exponent",
                    PRICING_ERROR_FACTORY,
                ),
                theta=require_finite_number(
                    raw_payload,
                    "theta",
                    f"{field_path}.theta",
                    PRICING_ERROR_FACTORY,
                ),
                residual=require_finite_number(
                    raw_payload,
                    "residual",
                    f"{field_path}.residual",
                    PRICING_ERROR_FACTORY,
                ),
                absolute_residual=require_non_negative_finite_number(
                    raw_payload,
                    "absolute_residual",
                    f"{field_path}.absolute_residual",
                    PRICING_ERROR_FACTORY,
                ),
                tolerance=require_positive_finite_number(
                    raw_payload,
                    "tolerance",
                    f"{field_path}.tolerance",
                    PRICING_ERROR_FACTORY,
                ),
                domain_status=AnchoredLocalTiltDomainStatus(
                    require_string(
                        raw_payload,
                        "domain_status",
                        f"{field_path}.domain_status",
                        PRICING_ERROR_FACTORY,
                    )
                ),
                admissibility_status=AnchoredLocalTiltAdmissibilityStatus(
                    require_string(
                        raw_payload,
                        "admissibility_status",
                        f"{field_path}.admissibility_status",
                        PRICING_ERROR_FACTORY,
                    )
                ),
                policy=require_string(
                    raw_payload,
                    "policy",
                    f"{field_path}.policy",
                    PRICING_ERROR_FACTORY,
                ),
            )
        except JaniformValidationError as e:
            raise _pricing_error(e.failure.field_path, e.failure.message) from e
        except ValueError as e:
            raise _pricing_error(field_path, str(e)) from e


@dataclass(frozen=True, slots=True)
class FourierInversionContour(ContractModel):
    damping_factor: float = 1.5

    def __post_init__(self) -> None:
        require_positive_finite_scalar(
            self.damping_factor,
            "damping_factor",
            PRICING_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> FourierInversionContour:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return FourierInversionContour(
            damping_factor=require_positive_finite_number(
                raw_payload,
                "damping_factor",
                f"{field_path}.damping_factor",
                PRICING_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class FourierInversionNumericSettings(ContractModel):
    integration_upper_bound: float = 150.0
    integration_step_count: int = 4_096
    quadrature_rule: FourierInversionQuadratureRule = FourierInversionQuadratureRule.MIDPOINT

    def __post_init__(self) -> None:
        require_positive_finite_scalar(
            self.integration_upper_bound,
            "integration_upper_bound",
            PRICING_ERROR_FACTORY,
        )

        if not isinstance(self.integration_step_count, int) or isinstance(
            self.integration_step_count,
            bool,
        ):
            raise _pricing_error(
                "integration_step_count",
                "must be an integer",
            )

        if self.integration_step_count <= 0:
            raise _pricing_error(
                "integration_step_count",
                "must be strictly positive",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> FourierInversionNumericSettings:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        try:
            return FourierInversionNumericSettings(
                integration_upper_bound=require_positive_finite_number(
                    raw_payload,
                    "integration_upper_bound",
                    f"{field_path}.integration_upper_bound",
                    PRICING_ERROR_FACTORY,
                ),
                integration_step_count=require_positive_integer(
                    raw_payload,
                    "integration_step_count",
                    f"{field_path}.integration_step_count",
                    PRICING_ERROR_FACTORY,
                ),
                quadrature_rule=FourierInversionQuadratureRule(
                    require_string(
                        raw_payload,
                        "quadrature_rule",
                        f"{field_path}.quadrature_rule",
                        PRICING_ERROR_FACTORY,
                    )
                ),
            )
        except ValueError as e:
            raise _pricing_error(f"{field_path}.quadrature_rule", str(e)) from e


@dataclass(frozen=True, slots=True)
class FourierInversionIntegrationStabilityControls(ContractModel):
    convergence_absolute_tolerance: float = 1.0e-5
    convergence_relative_tolerance: float = 1.0e-4
    terminal_integrand_absolute_tolerance: float = 5.0e-4
    terminal_integrand_ratio_tolerance: float = 1.0e-2
    quality_gate: FourierInversionNumericQualityGate = FourierInversionNumericQualityGate.DIAGNOSTICS_ONLY

    def __post_init__(self) -> None:
        require_positive_finite_scalar(
            self.convergence_absolute_tolerance,
            "convergence_absolute_tolerance",
            PRICING_ERROR_FACTORY,
        )
        require_positive_finite_scalar(
            self.convergence_relative_tolerance,
            "convergence_relative_tolerance",
            PRICING_ERROR_FACTORY,
        )
        require_positive_finite_scalar(
            self.terminal_integrand_absolute_tolerance,
            "terminal_integrand_absolute_tolerance",
            PRICING_ERROR_FACTORY,
        )
        require_positive_finite_scalar(
            self.terminal_integrand_ratio_tolerance,
            "terminal_integrand_ratio_tolerance",
            PRICING_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> FourierInversionIntegrationStabilityControls:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        try:
            return FourierInversionIntegrationStabilityControls(
                convergence_absolute_tolerance=require_positive_finite_number(
                    raw_payload,
                    "convergence_absolute_tolerance",
                    f"{field_path}.convergence_absolute_tolerance",
                    PRICING_ERROR_FACTORY,
                ),
                convergence_relative_tolerance=require_positive_finite_number(
                    raw_payload,
                    "convergence_relative_tolerance",
                    f"{field_path}.convergence_relative_tolerance",
                    PRICING_ERROR_FACTORY,
                ),
                terminal_integrand_absolute_tolerance=require_positive_finite_number(
                    raw_payload,
                    "terminal_integrand_absolute_tolerance",
                    f"{field_path}.terminal_integrand_absolute_tolerance",
                    PRICING_ERROR_FACTORY,
                ),
                terminal_integrand_ratio_tolerance=require_positive_finite_number(
                    raw_payload,
                    "terminal_integrand_ratio_tolerance",
                    f"{field_path}.terminal_integrand_ratio_tolerance",
                    PRICING_ERROR_FACTORY,
                ),
                quality_gate=FourierInversionNumericQualityGate(
                    require_string(
                        raw_payload,
                        "quality_gate",
                        f"{field_path}.quality_gate",
                        PRICING_ERROR_FACTORY,
                    )
                ),
            )
        except ValueError as e:
            raise _pricing_error(f"{field_path}.quality_gate", str(e)) from e


@dataclass(frozen=True, slots=True)
class FourierInversionEngineSettings(ContractModel):
    contour: FourierInversionContour = field(default_factory=FourierInversionContour)
    numeric_settings: FourierInversionNumericSettings = field(default_factory=FourierInversionNumericSettings)
    stability_controls: FourierInversionIntegrationStabilityControls = field(
        default_factory=FourierInversionIntegrationStabilityControls
    )

    def __post_init__(self) -> None:
        if not isinstance(self.contour, FourierInversionContour):
            raise _pricing_error("contour", "must be a FourierInversionContour")
        if not isinstance(self.numeric_settings, FourierInversionNumericSettings):
            raise _pricing_error("numeric_settings", "must be a FourierInversionNumericSettings")
        if not isinstance(
            self.stability_controls,
            FourierInversionIntegrationStabilityControls,
        ):
            raise _pricing_error(
                "stability_controls",
                "must be a FourierInversionIntegrationStabilityControls",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> FourierInversionEngineSettings:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return FourierInversionEngineSettings(
            contour=FourierInversionContour.from_mapping(
                require_mapping(raw_payload, "contour", f"{field_path}.contour", PRICING_ERROR_FACTORY),
                field_path=f"{field_path}.contour",
            ),
            numeric_settings=FourierInversionNumericSettings.from_mapping(
                require_mapping(
                    raw_payload,
                    "numeric_settings",
                    f"{field_path}.numeric_settings",
                    PRICING_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.numeric_settings",
            ),
            stability_controls=FourierInversionIntegrationStabilityControls.from_mapping(
                require_mapping(
                    raw_payload,
                    "stability_controls",
                    f"{field_path}.stability_controls",
                    PRICING_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.stability_controls",
            ),
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingRequest(ContractModel):
    """Typed pricing request with hourly intraday default `interval_delta=1.0`."""

    market_inputs: VanillaOptionPricingMarketInputs
    payoff: VanillaOptionPayoffConfiguration
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput
    model_parameters: VanillaOptionPricingModelParameters
    interval_delta: float = DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA
    fourier_inversion_settings: Optional[FourierInversionEngineSettings] = None

    def __post_init__(self) -> None:
        require_positive_finite_scalar(
            self.interval_delta,
            "interval_delta",
            PRICING_ERROR_FACTORY,
        )

        if not isinstance(self.boundary_input, AnchoredVanillaOptionPricingBoundaryInput):
            raise _pricing_error(
                "boundary_input",
                "must be an AnchoredVanillaOptionPricingBoundaryInput",
            )

        if (
            abs(self.boundary_input.interval_delta - self.interval_delta)
            > _PRICING_MEAN_REVERSION_ALIGNMENT_TOLERANCE
        ):
            raise _pricing_error(
                "interval_delta",
                (
                    "must match boundary_input.interval_delta "
                    f"({self.boundary_input.interval_delta})"
                ),
            )

        if (
            self.fourier_inversion_settings is not None
            and not isinstance(self.fourier_inversion_settings, FourierInversionEngineSettings)
        ):
            raise _pricing_error(
                "fourier_inversion_settings",
                "must be a FourierInversionEngineSettings when provided",
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
                boundary_input=AnchoredVanillaOptionPricingBoundaryInput.from_mapping(
                    require_mapping(raw_payload, "boundary_input", "boundary_input", PRICING_ERROR_FACTORY),
                    field_path="boundary_input",
                ),
                model_parameters=VanillaOptionPricingModelParameters.from_mapping(
                    require_mapping(raw_payload, "model_parameters", "model_parameters", PRICING_ERROR_FACTORY),
                    field_path="model_parameters",
                ),
                interval_delta=interval_delta,
                fourier_inversion_settings=(
                    FourierInversionEngineSettings.from_mapping(
                        require_mapping(
                            raw_payload,
                            "fourier_inversion_settings",
                            "fourier_inversion_settings",
                            PRICING_ERROR_FACTORY,
                        ),
                        field_path="fourier_inversion_settings",
                    )
                    if "fourier_inversion_settings" in raw_payload
                    and raw_payload["fourier_inversion_settings"] is not None
                    else None
                ),
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
                PRICING_ERROR_FACTORY,
            ),
            include_integration_quality_diagnostics=require_boolean(
                raw_payload,
                "include_integration_quality_diagnostics",
                f"{field_path}.include_integration_quality_diagnostics",
                PRICING_ERROR_FACTORY,
            ),
            include_calibration_hooks=require_boolean(
                raw_payload,
                "include_calibration_hooks",
                f"{field_path}.include_calibration_hooks",
                PRICING_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingExecutionInput(ContractModel):
    request: VanillaOptionPricingRequest
    hourly_factor_contract: HourlyFactorContract
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection = (
        VanillaOptionPricingOptionalOutputSelection()
    )

    def __post_init__(self) -> None:
        if not isinstance(self.request, VanillaOptionPricingRequest):
            raise _pricing_error("request", "must be a VanillaOptionPricingRequest")
        if not isinstance(self.hourly_factor_contract, HourlyFactorContract):
            raise _pricing_error(
                "hourly_factor_contract",
                "must be a HourlyFactorContract runtime artifact",
            )

        if not isinstance(
            self.optional_output_selection,
            VanillaOptionPricingOptionalOutputSelection,
        ):
            raise _pricing_error(
                "optional_output_selection",
                "must be VanillaOptionPricingOptionalOutputSelection",
            )
        if self.hourly_factor_contract.interval_count == 0:
            raise _pricing_error(
                "hourly_factor_contract.interval_count",
                "must contain at least one interval",
            )

        try:
            expected_interval_duration_nanos = hours_to_exact_nanoseconds(self.request.interval_delta)
        except ValueError as e:
            raise _pricing_error("request.interval_delta", str(e)) from e

        if self.hourly_factor_contract.interval_duration_nanos != expected_interval_duration_nanos:
            raise _pricing_error(
                "hourly_factor_contract.interval_duration_nanos",
                (
                    "must match request.interval_delta exactly under explicit factor alignment "
                    f"(request.interval_delta={self.request.interval_delta}, "
                    "hourly_factor_contract.interval_duration_nanos="
                    f"{self.hourly_factor_contract.interval_duration_nanos})"
                ),
            )

        maturity_interval_from_runtime_grid = (
            self.hourly_factor_contract.interval_count * self.request.interval_delta
        )
        if maturity_interval_from_runtime_grid != self.request.market_inputs.maturity_interval:
            raise _pricing_error(
                "hourly_factor_contract.interval_count",
                (
                    "must satisfy interval_count * request.interval_delta = "
                    f"request.market_inputs.maturity_interval ({self.request.market_inputs.maturity_interval}), "
                    f"found {maturity_interval_from_runtime_grid}"
                ),
            )

    def to_payload(self) -> dict[str, Any]:
        return {
            "request": self.request.to_payload(),
            "hourly_factor_contract": self.hourly_factor_contract.to_payload(),
            "optional_output_selection": self.optional_output_selection.to_payload(),
        }


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
    anchored_recursion: Optional[dict[str, Any]]

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
        if "anchored_recursion" not in raw_payload:
            raise _pricing_error(f"{field_path}.anchored_recursion", "is required")

        transform = require_optional_mapping_value(
            raw_payload["transform"],
            f"{field_path}.transform",
            PRICING_ERROR_FACTORY,
        )

        integration_quality = require_optional_mapping_value(
            raw_payload["integration_quality"],
            f"{field_path}.integration_quality",
            PRICING_ERROR_FACTORY,
        )

        anchored_recursion = require_optional_mapping_value(
            raw_payload["anchored_recursion"],
            f"{field_path}.anchored_recursion",
            PRICING_ERROR_FACTORY,
        )

        if transform is None and integration_quality is None and anchored_recursion is None:
            raise _pricing_error(
                field_path,
                (
                    "must include transform diagnostics, integration quality diagnostics, "
                    "anchored recursion diagnostics, or any non-empty combination of them"
                ),
            )

        return VanillaOptionPricingRunDiagnostics(
            schema_version=VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION,
            transform=transform,
            integration_quality=integration_quality,
            anchored_recursion=anchored_recursion,
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

        transform_status_raw = require_optional_string_value(
            raw_payload.get("transform_status"),
            f"{field_path}.transform_status",
            PRICING_ERROR_FACTORY,
        )

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

    return require_optional_boolean_value(
        raw_payload[field_name],
        field_path,
        PRICING_ERROR_FACTORY,
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
    except ModuleNotFoundError as e:
        raise _pricing_integration_error(
            "native pricing module is unavailable",
            entrypoint="run_vanilla_option_pricing",
            error_id=PRICING_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    try:
        raw_run_result = native.run_vanilla_option_pricing(
            normalized_execution_input.request.to_payload(),
            normalized_execution_input.hourly_factor_contract._native_result,
            normalized_execution_input.optional_output_selection.to_payload(),
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
    "AnchoredLocalTiltAdmissibilityStatus",
    "AnchoredLocalTiltBoundaryStateSummary",
    "AnchoredLocalTiltDomainStatus",
    "AnchoredLocalTiltSolveDiagnostics",
    "AnchoredVanillaOptionPricingBoundaryInput",
    "FourierInversionContour",
    "FourierInversionEngineSettings",
    "FourierInversionIntegrationStabilityControls",
    "FourierInversionNumericQualityGate",
    "FourierInversionNumericSettings",
    "FourierInversionQuadratureRule",
    "GammaSeverityMarkConfiguration",
    "MeanReversionAnchorModel",
    "MeanReversionAnchorModelKind",
    "MeanReversionBoundaryState",
    "MeanReversionFittedParameters",
    "NigSharedPlusShiftConfiguration",
    "VanillaOptionPricingCalibrationHooks",
    "VanillaOptionPricingExecutionInput",
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
