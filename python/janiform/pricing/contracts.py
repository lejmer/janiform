"""Public vanilla-option pricing contracts."""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from math import exp
from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional

from .errors import PRICING_ERROR_FACTORY, _pricing_error
from ..contracts.base import (
    require_boolean,
    require_optional_finite_number,
    require_optional_boolean_value,
    require_optional_mapping_value,
    require_optional_string_value,
    require_str_enum_value,
)
from ..mean_reversion import (
    MeanReversionAnchorModel,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
)
from ..contracts import (
    ContractModel,
    serialize_pretty_json,
    validate_exact_string,
    validate_required_field_list,
)
from ..contracts.base import (
    require_finite_number,
    require_mapping,
    require_non_negative_finite_number,
    require_non_negative_integer,
    require_positive_finite_number,
    require_positive_finite_scalar,
    require_positive_integer,
    require_sequence,
    require_string,
)
from ..errors import JaniformValidationError
from ..hourly_factors import HourlyFactorContract
from .._timestamps import ONE_HOUR_NANOS

if TYPE_CHECKING:
    from ..simulation import HawkesExcitationState, HawkesKernelParameters
    from ..simulation.coarse.news import NewsProcessConfiguration

# Public pricing-request default in hours, and callers can override it on the request payload.
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
    "runtime_interval_schedule",
    "maturity_terminal_interval_indices",
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
PRICING_LOCAL_ANCHORED_TILT_SOLVE_POLICY = (
    "solve_state_dependent_local_tilt_against_endogenous_q_martingale_target.v1"
)
_PRICING_MEAN_REVERSION_ALIGNMENT_TOLERANCE = 1e-12

class FourierInversionQuadratureRule(StrEnum):
    MIDPOINT = "midpoint"
    TRAPEZOID = "trapezoid"


class FourierInversionNumericQualityGate(StrEnum):
    DIAGNOSTICS_ONLY = "diagnostics_only"
    ENFORCE_FAILURE = "enforce_failure"


def _parse_hawkes_kernel_parameters(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> Any:
    from ..simulation import HawkesKernelParameters

    return HawkesKernelParameters.from_mapping(raw_payload, field_path=field_path)


def _parse_hawkes_excitation_state(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
):
    from ..simulation import HawkesExcitationState

    return HawkesExcitationState.from_mapping(raw_payload, field_path=field_path)


def _parse_news_process_configuration(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
):
    from ..simulation.coarse.news import NewsProcessConfiguration

    try:
        return NewsProcessConfiguration.from_mapping(raw_payload)
    except JaniformValidationError as e:
        raise _pricing_error(f"{field_path}.{e.failure.field_path}", e.failure.message) from e


class VanillaOptionPayoffType(StrEnum):
    CALL = 'call'
    PUT = 'put'


class VanillaOptionImpliedVolatilityInversionStatus(StrEnum):
    SUCCEEDED = "succeeded"
    PRICE_BELOW_NO_ARBITRAGE_LOWER_BOUND = "price_below_no_arbitrage_lower_bound"
    PRICE_EQUALS_FINITE_UPPER_BOUND = "price_equals_finite_upper_bound"
    PRICE_ABOVE_FINITE_UPPER_BOUND = "price_above_finite_upper_bound"


class VanillaOptionPricingStatus(StrEnum):
    SUCCEEDED = "succeeded"
    FAILED = "failed"


class VanillaOptionPricingFailureStage(StrEnum):
    NODE_SETUP = "node_setup"
    TRANSFORM_INPUT_BUILD = "transform_input_build"
    TRANSFORM_SOLVE = "transform_solve"
    TRANSFORM_OUTPUT = "transform_output"
    CHARACTERISTIC_FUNCTION_ASSEMBLY = "characteristic_function_assembly"
    FOURIER_PRICING = "fourier_pricing"
    RISK_NEUTRAL_CHARACTERISTIC_FUNCTION = "risk_neutral_characteristic_function"
    ENDOGENOUS_ANCHOR_MATERIALIZATION = "endogenous_anchor_materialization"
    EXPECTED_HAWKES_STATE_UPDATE = "expected_hawkes_state_update"
    ANCHORED_LOCAL_TILT_SOLVE = "anchored_local_tilt_solve"
    ANCHORED_RECURSION_OBSERVED_BOUNDARY_STATE = "anchored_recursion_observed_boundary_state"
    ANCHORED_RECURSION_DIAGNOSTICS = "anchored_recursion_diagnostics"
    GRID_INTERPOLATION_SHAPE = "grid_interpolation_shape"
    RESPONSE_DIAGNOSTICS = "response_diagnostics"
    RESPONSE_ASSEMBLY = "response_assembly"
    RUN_RESULT_ASSEMBLY = "run_result_assembly"
    FORWARD_LAW = "forward_law"
    WORKER_EXECUTION = "worker_execution"


def _tuple_of_positive_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    values = require_sequence(
        raw_payload,
        field_name,
        field_path,
        PRICING_ERROR_FACTORY,
    )

    return tuple(
        require_positive_finite_scalar(
            value,
            f"{field_path}[{value_index}]",
            PRICING_ERROR_FACTORY,
        )
        for value_index, value in enumerate(values)
    )


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
    """Market inputs required for one pricing scope."""

    spot: float
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    risk_free_rate: float
    dividend_yield: float

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", PRICING_ERROR_FACTORY)
        require_finite_number(
            {"risk_free_rate": self.risk_free_rate},
            "risk_free_rate",
            "risk_free_rate",
            PRICING_ERROR_FACTORY,
        )
        require_finite_number(
            {"dividend_yield": self.dividend_yield},
            "dividend_yield",
            "dividend_yield",
            PRICING_ERROR_FACTORY,
        )

        if len(self.strikes) == 0:
            raise _pricing_error("strikes", "must contain at least one strike")

        if len(self.maturity_intervals) == 0:
            raise _pricing_error(
                "maturity_intervals",
                "must contain at least one maturity interval",
            )

        for strike_index, strike in enumerate(self.strikes):
            require_positive_finite_scalar(
                strike,
                f"strikes[{strike_index}]",
                PRICING_ERROR_FACTORY,
            )

        for maturity_index, maturity_interval in enumerate(self.maturity_intervals):
            require_positive_finite_scalar(
                maturity_interval,
                f"maturity_intervals[{maturity_index}]",
                PRICING_ERROR_FACTORY,
            )

    @property
    def shape(self) -> tuple[int, int]:
        return len(self.maturity_intervals), len(self.strikes)

    @property
    def single_strike(self) -> float:
        if len(self.strikes) != 1 or len(self.maturity_intervals) != 1:
            raise _pricing_error(
                "market_inputs",
                (
                    "single-option pricing requires exactly one strike and one maturity "
                    f"interval, found strikes={len(self.strikes)} "
                    f"maturity_intervals={len(self.maturity_intervals)}"
                ),
            )

        return self.strikes[0]

    @property
    def single_maturity_interval(self) -> float:
        if len(self.strikes) != 1 or len(self.maturity_intervals) != 1:
            raise _pricing_error(
                "market_inputs",
                (
                    "single-option pricing requires exactly one strike and one maturity "
                    f"interval, found strikes={len(self.strikes)} "
                    f"maturity_intervals={len(self.maturity_intervals)}"
                ),
            )

        return self.maturity_intervals[0]

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
            strikes=_tuple_of_positive_floats(
                raw_payload,
                field_name="strikes",
                field_path=f"{field_path}.strikes",
            ),
            maturity_intervals=_tuple_of_positive_floats(
                raw_payload,
                field_name="maturity_intervals",
                field_path=f"{field_path}.maturity_intervals",
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
        from ..simulation import HawkesKernelParameters
        from ..simulation.coarse.news import NewsProcessConfiguration

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
        from ..simulation import HawkesExcitationState

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

        if not isinstance(self.market_inputs, VanillaOptionPricingMarketInputs):
            raise _pricing_error(
                "market_inputs",
                "must be a VanillaOptionPricingMarketInputs",
            )

        _ = self.market_inputs.single_strike
        _ = self.market_inputs.single_maturity_interval

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
class AutoExactSparsePricingIntervalSchedulePolicy(ContractModel):
    policy: str = "auto_exact_sparse"

    def __post_init__(self) -> None:
        if self.policy != "auto_exact_sparse":
            raise _pricing_error("policy", "must equal 'auto_exact_sparse'")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> AutoExactSparsePricingIntervalSchedulePolicy:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return AutoExactSparsePricingIntervalSchedulePolicy(
            policy=require_string(
                raw_payload,
                "policy",
                f"{field_path}.policy",
                PRICING_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class ExplicitIntervalEndTimestampsUtcPricingIntervalSchedulePolicy(ContractModel):
    interval_end_timestamps_utc_nanos: tuple[int, ...]
    policy: str = "explicit_interval_end_timestamps_utc"

    def __post_init__(self) -> None:
        if self.policy != "explicit_interval_end_timestamps_utc":
            raise _pricing_error(
                "policy",
                "must equal 'explicit_interval_end_timestamps_utc'",
            )

        if len(self.interval_end_timestamps_utc_nanos) == 0:
            raise _pricing_error(
                "interval_end_timestamps_utc_nanos",
                "must contain at least one interval endpoint",
            )

        previous_endpoint_utc_nanos: Optional[int] = None
        for interval_index, interval_end_utc_nanos in enumerate(
            self.interval_end_timestamps_utc_nanos
        ):
            require_non_negative_integer(
                {"value": interval_end_utc_nanos},
                "value",
                f"interval_end_timestamps_utc_nanos[{interval_index}]",
                PRICING_ERROR_FACTORY,
            )

            if (
                previous_endpoint_utc_nanos is not None
                and interval_end_utc_nanos <= previous_endpoint_utc_nanos
            ):
                raise _pricing_error(
                    f"interval_end_timestamps_utc_nanos[{interval_index}]",
                    "must be strictly greater than the previous endpoint",
                )

            previous_endpoint_utc_nanos = interval_end_utc_nanos

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> ExplicitIntervalEndTimestampsUtcPricingIntervalSchedulePolicy:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        raw_interval_end_timestamps = require_sequence(
            raw_payload,
            "interval_end_timestamps_utc_nanos",
            f"{field_path}.interval_end_timestamps_utc_nanos",
            PRICING_ERROR_FACTORY,
        )

        return ExplicitIntervalEndTimestampsUtcPricingIntervalSchedulePolicy(
            policy=require_string(
                raw_payload,
                "policy",
                f"{field_path}.policy",
                PRICING_ERROR_FACTORY,
            ),
            interval_end_timestamps_utc_nanos=tuple(
                require_non_negative_integer(
                    {"value": value},
                    "value",
                    f"{field_path}.interval_end_timestamps_utc_nanos[{index}]",
                    PRICING_ERROR_FACTORY,
                )
                for index, value in enumerate(raw_interval_end_timestamps)
            ),
        )


type PricingIntervalSchedulePolicy = (
    AutoExactSparsePricingIntervalSchedulePolicy
    | ExplicitIntervalEndTimestampsUtcPricingIntervalSchedulePolicy
)


def default_pricing_interval_schedule_policy() -> PricingIntervalSchedulePolicy:
    return AutoExactSparsePricingIntervalSchedulePolicy()


def pricing_interval_schedule_policy_from_mapping(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> PricingIntervalSchedulePolicy:
    if not isinstance(raw_payload, dict):
        raise _pricing_error(field_path, "must be an object")

    policy = require_string(
        raw_payload,
        "policy",
        f"{field_path}.policy",
        PRICING_ERROR_FACTORY,
    )

    match policy:
        case "auto_exact_sparse":
            return AutoExactSparsePricingIntervalSchedulePolicy.from_mapping(
                raw_payload,
                field_path=field_path,
            )
        case "explicit_interval_end_timestamps_utc":
            return ExplicitIntervalEndTimestampsUtcPricingIntervalSchedulePolicy.from_mapping(
                raw_payload,
                field_path=field_path,
            )
        case _:
            raise _pricing_error(
                f"{field_path}.policy",
                "must be one of: auto_exact_sparse, explicit_interval_end_timestamps_utc",
            )


@dataclass(frozen=True, slots=True)
class PricingIntervalScheduleMetadata(ContractModel):
    policy_id: str
    derivation_policy: str
    factor_assignment_policy: str

    def __post_init__(self) -> None:
        require_string({"policy_id": self.policy_id}, "policy_id", "policy_id", PRICING_ERROR_FACTORY)
        require_string(
            {"derivation_policy": self.derivation_policy},
            "derivation_policy",
            "derivation_policy",
            PRICING_ERROR_FACTORY,
        )
        require_string(
            {"factor_assignment_policy": self.factor_assignment_policy},
            "factor_assignment_policy",
            "factor_assignment_policy",
            PRICING_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> PricingIntervalScheduleMetadata:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return PricingIntervalScheduleMetadata(
            policy_id=require_string(
                raw_payload,
                "policy_id",
                f"{field_path}.policy_id",
                PRICING_ERROR_FACTORY,
            ),
            derivation_policy=require_string(
                raw_payload,
                "derivation_policy",
                f"{field_path}.derivation_policy",
                PRICING_ERROR_FACTORY,
            ),
            factor_assignment_policy=require_string(
                raw_payload,
                "factor_assignment_policy",
                f"{field_path}.factor_assignment_policy",
                PRICING_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class PricingIntervalSchedule(ContractModel):
    valuation_time_utc_nanos: int
    metadata: PricingIntervalScheduleMetadata
    interval_end_timestamps_utc_nanos: tuple[int, ...]
    interval_deltas_hours: tuple[float, ...]

    def __post_init__(self) -> None:
        if not isinstance(self.metadata, PricingIntervalScheduleMetadata):
            raise _pricing_error("runtime_interval_schedule.metadata", "must be a PricingIntervalScheduleMetadata")

        if len(self.interval_end_timestamps_utc_nanos) == 0:
            raise _pricing_error(
                "runtime_interval_schedule.interval_end_timestamps_utc_nanos",
                "must contain at least one interval endpoint",
            )

        if len(self.interval_end_timestamps_utc_nanos) != len(self.interval_deltas_hours):
            raise _pricing_error(
                "runtime_interval_schedule.interval_deltas_hours",
                (
                    "must contain one entry per interval_end_timestamps_utc_nanos "
                    f"({len(self.interval_end_timestamps_utc_nanos)}), found {len(self.interval_deltas_hours)}"
                ),
            )

        previous_endpoint_utc_nanos = self.valuation_time_utc_nanos
        for interval_index, interval_end_utc_nanos in enumerate(self.interval_end_timestamps_utc_nanos):
            if interval_end_utc_nanos <= previous_endpoint_utc_nanos:
                raise _pricing_error(
                    f"runtime_interval_schedule.interval_end_timestamps_utc_nanos[{interval_index}]",
                    "must be strictly increasing and strictly after the previous endpoint",
                )

            require_positive_finite_scalar(
                self.interval_deltas_hours[interval_index],
                f"runtime_interval_schedule.interval_deltas_hours[{interval_index}]",
                PRICING_ERROR_FACTORY,
            )

            previous_endpoint_utc_nanos = interval_end_utc_nanos

    @property
    def interval_count(self) -> int:
        return len(self.interval_end_timestamps_utc_nanos)

    @property
    def first_interval_delta_hours(self) -> float:
        return self.interval_deltas_hours[0]

    @property
    def terminal_maturity_interval_hours(self) -> float:
        return (
            self.interval_end_timestamps_utc_nanos[-1] - self.valuation_time_utc_nanos
        ) / ONE_HOUR_NANOS

    def maturity_interval_hours(self, terminal_interval_index: int) -> float:
        if terminal_interval_index < 0 or terminal_interval_index >= self.interval_count:
            raise _pricing_error(
                "terminal_interval_index",
                (
                    "must be smaller than interval_count "
                    f"({self.interval_count}), found {terminal_interval_index}"
                ),
            )

        return (
            self.interval_end_timestamps_utc_nanos[terminal_interval_index]
            - self.valuation_time_utc_nanos
        ) / ONE_HOUR_NANOS

    def terminal_interval_index_for_maturity_interval(self, maturity_interval: float) -> int:
        for terminal_interval_index in range(self.interval_count):
            if self.maturity_interval_hours(terminal_interval_index) == maturity_interval:
                return terminal_interval_index

        raise _pricing_error(
            "maturity_interval",
            (
                "must match a runtime_interval_schedule terminal boundary exactly "
                f"(maturity_interval={maturity_interval})"
            ),
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> PricingIntervalSchedule:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        raw_interval_end_timestamps = require_sequence(
            raw_payload,
            "interval_end_timestamps_utc_nanos",
            f"{field_path}.interval_end_timestamps_utc_nanos",
            PRICING_ERROR_FACTORY,
        )
        raw_interval_deltas = require_sequence(
            raw_payload,
            "interval_deltas_hours",
            f"{field_path}.interval_deltas_hours",
            PRICING_ERROR_FACTORY,
        )

        return PricingIntervalSchedule(
            valuation_time_utc_nanos=require_non_negative_integer(
                raw_payload,
                "valuation_time_utc_nanos",
                f"{field_path}.valuation_time_utc_nanos",
                PRICING_ERROR_FACTORY,
            ),
            metadata=PricingIntervalScheduleMetadata.from_mapping(
                require_mapping(
                    raw_payload,
                    "metadata",
                    f"{field_path}.metadata",
                    PRICING_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.metadata",
            ),
            interval_end_timestamps_utc_nanos=tuple(
                require_non_negative_integer(
                    {"value": value},
                    "value",
                    f"{field_path}.interval_end_timestamps_utc_nanos[{index}]",
                    PRICING_ERROR_FACTORY,
                )
                for index, value in enumerate(raw_interval_end_timestamps)
            ),
            interval_deltas_hours=tuple(
                require_positive_finite_scalar(
                    value,
                    f"{field_path}.interval_deltas_hours[{index}]",
                    PRICING_ERROR_FACTORY,
                )
                for index, value in enumerate(raw_interval_deltas)
            ),
        )


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
class VanillaOptionPricingFailureDiagnostic(ContractModel):
    stage: VanillaOptionPricingFailureStage
    reason: str

    def __post_init__(self) -> None:
        if not isinstance(self.stage, VanillaOptionPricingFailureStage):
            raise _pricing_error("stage", "must be a VanillaOptionPricingFailureStage")

        require_string(
            {"reason": self.reason},
            "reason",
            "reason",
            PRICING_ERROR_FACTORY,
        )

        if self.reason.strip() == "":
            raise _pricing_error("reason", "must be non-empty")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> VanillaOptionPricingFailureDiagnostic:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return VanillaOptionPricingFailureDiagnostic(
            stage=require_str_enum_value(
                raw_payload.get("stage"),
                f"{field_path}.stage",
                PRICING_ERROR_FACTORY,
                enum_type=VanillaOptionPricingFailureStage,
                allowed_values_message=(
                    "must be one of: node_setup, transform_input_build, "
                    "transform_solve, transform_output, "
                    "characteristic_function_assembly, fourier_pricing, "
                    "risk_neutral_characteristic_function, "
                    "endogenous_anchor_materialization, "
                    "expected_hawkes_state_update, anchored_local_tilt_solve, "
                    "anchored_recursion_observed_boundary_state, "
                    "anchored_recursion_diagnostics, grid_interpolation_shape, "
                    "response_diagnostics, response_assembly, "
                    "run_result_assembly, forward_law, worker_execution"
                ),
            ),
            reason=require_string(
                raw_payload,
                "reason",
                f"{field_path}.reason",
                PRICING_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionPricingResponse(ContractModel):
    """Typed pricing response."""

    pricing_status: VanillaOptionPricingStatus
    pricing_failure_diagnostic: Optional[VanillaOptionPricingFailureDiagnostic]
    price: Optional[float]
    diagnostics: Optional[VanillaOptionPricingDiagnostics]
    implied_volatility: Optional[float]
    implied_volatility_inversion_status: Optional[VanillaOptionImpliedVolatilityInversionStatus]

    def __post_init__(self) -> None:
        if not isinstance(self.pricing_status, VanillaOptionPricingStatus):
            raise _pricing_error(
                "pricing_status",
                "must be a VanillaOptionPricingStatus",
            )

        if (
            self.pricing_failure_diagnostic is not None
            and not isinstance(
                self.pricing_failure_diagnostic,
                VanillaOptionPricingFailureDiagnostic,
            )
        ):
            raise _pricing_error(
                "pricing_failure_diagnostic",
                "must be a VanillaOptionPricingFailureDiagnostic when provided",
            )

        if self.pricing_status is VanillaOptionPricingStatus.SUCCEEDED:
            if self.pricing_failure_diagnostic is not None:
                raise _pricing_error(
                    "pricing_failure_diagnostic",
                    "must be null when pricing_status=succeeded",
                )

            if self.price is None:
                raise _pricing_error("price", "must be present when pricing_status=succeeded")

            require_non_negative_finite_number(
                {"price": self.price},
                "price",
                "price",
                PRICING_ERROR_FACTORY,
            )

            if self.diagnostics is None:
                raise _pricing_error(
                    "diagnostics",
                    "must be present when pricing_status=succeeded",
                )

            if not isinstance(self.diagnostics, VanillaOptionPricingDiagnostics):
                raise _pricing_error(
                    "diagnostics",
                    "must be a VanillaOptionPricingDiagnostics",
                )

            if self.implied_volatility_inversion_status is None:
                raise _pricing_error(
                    "implied_volatility_inversion_status",
                    "must be present when pricing_status=succeeded",
                )

            _validate_optional_implied_volatility_inversion_outcome(
                implied_volatility=self.implied_volatility,
                implied_volatility_inversion_status=self.implied_volatility_inversion_status,
                field_path="vanilla_option_pricing_response",
            )

            return

        if self.pricing_failure_diagnostic is None:
            raise _pricing_error(
                "pricing_failure_diagnostic",
                "must be present when pricing_status=failed",
            )

        if self.price is not None:
            raise _pricing_error("price", "must be null when pricing_status=failed")

        if self.diagnostics is not None:
            raise _pricing_error("diagnostics", "must be null when pricing_status=failed")

        if self.implied_volatility is not None:
            raise _pricing_error(
                "implied_volatility",
                "must be null when pricing_status=failed",
            )

        if self.implied_volatility_inversion_status is not None:
            raise _pricing_error(
                "implied_volatility_inversion_status",
                "must be null when pricing_status=failed",
            )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> VanillaOptionPricingResponse:
        if not isinstance(raw_payload, dict):
            raise _pricing_error("vanilla_option_pricing_response", "must be an object")

        implied_volatility = require_optional_finite_number(
            raw_payload,
            "implied_volatility",
            "implied_volatility",
            PRICING_ERROR_FACTORY,
        )

        return VanillaOptionPricingResponse(
            pricing_status=require_str_enum_value(
                raw_payload.get("pricing_status"),
                "pricing_status",
                PRICING_ERROR_FACTORY,
                enum_type=VanillaOptionPricingStatus,
                allowed_values_message="must be one of: succeeded, failed",
            ),
            pricing_failure_diagnostic=(
                VanillaOptionPricingFailureDiagnostic.from_mapping(
                    require_mapping(
                        raw_payload,
                        "pricing_failure_diagnostic",
                        "pricing_failure_diagnostic",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path="pricing_failure_diagnostic",
                )
                if raw_payload.get("pricing_failure_diagnostic") is not None
                else None
            ),
            price=require_optional_finite_number(
                raw_payload,
                "price",
                "price",
                PRICING_ERROR_FACTORY,
            ),
            diagnostics=(
                VanillaOptionPricingDiagnostics.from_mapping(
                    require_mapping(
                        raw_payload,
                        "diagnostics",
                        "diagnostics",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path="diagnostics",
                )
                if raw_payload.get("diagnostics") is not None
                else None
            ),
            implied_volatility=implied_volatility,
            implied_volatility_inversion_status=(
                _require_optional_implied_volatility_inversion_status(
                    raw_payload,
                    field_name="implied_volatility_inversion_status",
                    field_path="implied_volatility_inversion_status",
                )
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
    runtime_interval_schedule: PricingIntervalSchedule
    hourly_factor_contract: HourlyFactorContract
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection = (
        VanillaOptionPricingOptionalOutputSelection()
    )

    def __post_init__(self) -> None:
        if not isinstance(self.request, VanillaOptionPricingRequest):
            raise _pricing_error("request", "must be a VanillaOptionPricingRequest")

        if not isinstance(self.runtime_interval_schedule, PricingIntervalSchedule):
            raise _pricing_error(
                "runtime_interval_schedule",
                "must be a PricingIntervalSchedule",
            )

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

        if self.hourly_factor_contract.interval_duration_nanos != ONE_HOUR_NANOS:
            raise _pricing_error(
                "hourly_factor_contract.interval_duration_nanos",
                (
                    "must equal the canonical hourly source interval duration "
                    "(hourly_factor_contract.interval_duration_nanos="
                    f"{self.hourly_factor_contract.interval_duration_nanos})"
                ),
            )

        if self.request.interval_delta != self.runtime_interval_schedule.first_interval_delta_hours:
            raise _pricing_error(
                "request.interval_delta",
                (
                    "must equal runtime_interval_schedule.first_interval_delta_hours "
                    f"({self.runtime_interval_schedule.first_interval_delta_hours})"
                ),
            )

        maturity_interval_from_runtime_grid = self.runtime_interval_schedule.terminal_maturity_interval_hours
        requested_maturity_interval = self.request.market_inputs.single_maturity_interval
        if maturity_interval_from_runtime_grid != requested_maturity_interval:
            raise _pricing_error(
                "runtime_interval_schedule.interval_end_timestamps_utc_nanos",
                (
                    "must satisfy runtime_interval_schedule terminal maturity = "
                    "request.market_inputs.single_maturity_interval "
                    f"({requested_maturity_interval}), "
                    f"found {maturity_interval_from_runtime_grid}"
                ),
            )

    def to_payload(self) -> dict[str, Any]:
        return {
            "request": self.request.to_payload(),
            "runtime_interval_schedule": self.runtime_interval_schedule.to_payload(),
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
    runtime_interval_schedule: PricingIntervalSchedule
    maturity_terminal_interval_indices: tuple[int, ...]

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

        raw_maturity_terminal_interval_indices = require_sequence(
            raw_payload,
            "maturity_terminal_interval_indices",
            "maturity_terminal_interval_indices",
            PRICING_ERROR_FACTORY,
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
            runtime_interval_schedule=PricingIntervalSchedule.from_mapping(
                require_mapping(
                    raw_payload,
                    "runtime_interval_schedule",
                    "runtime_interval_schedule",
                    PRICING_ERROR_FACTORY,
                ),
                field_path="runtime_interval_schedule",
            ),
            maturity_terminal_interval_indices=tuple(
                require_non_negative_integer(
                    {"value": raw_index},
                    "value",
                    f"maturity_terminal_interval_indices[{index}]",
                    PRICING_ERROR_FACTORY,
                )
                for index, raw_index in enumerate(raw_maturity_terminal_interval_indices)
            ),
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


def _require_implied_volatility_inversion_status(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> VanillaOptionImpliedVolatilityInversionStatus:
    return require_str_enum_value(
        require_string(
            raw_payload,
            field_name,
            field_path,
            PRICING_ERROR_FACTORY,
        ),
        field_path,
        PRICING_ERROR_FACTORY,
        enum_type=VanillaOptionImpliedVolatilityInversionStatus,
        allowed_values_message=(
            "must be one of: succeeded, price_below_no_arbitrage_lower_bound, "
            "price_equals_finite_upper_bound, price_above_finite_upper_bound"
        ),
    )


def _require_optional_implied_volatility_inversion_status(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> Optional[VanillaOptionImpliedVolatilityInversionStatus]:
    if raw_payload.get(field_name) is None:
        return None

    return _require_implied_volatility_inversion_status(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
    )


def _validate_implied_volatility_inversion_outcome(
    *,
    implied_volatility: Optional[float],
    implied_volatility_inversion_status: VanillaOptionImpliedVolatilityInversionStatus,
    field_path: str,
) -> None:
    match implied_volatility_inversion_status:
        case VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED:
            if implied_volatility is None:
                raise _pricing_error(
                    f"{field_path}.implied_volatility",
                    (
                        "must be present when "
                        "implied_volatility_inversion_status=succeeded"
                    ),
                )
        case _:
            if implied_volatility is not None:
                raise _pricing_error(
                    f"{field_path}.implied_volatility",
                    (
                        "must be omitted when "
                        "implied_volatility_inversion_status indicates inversion failure"
                    ),
                )


def _validate_optional_implied_volatility_inversion_outcome(
    *,
    implied_volatility: Optional[float],
    implied_volatility_inversion_status: Optional[VanillaOptionImpliedVolatilityInversionStatus],
    field_path: str,
) -> None:
    if implied_volatility_inversion_status is None:
        if implied_volatility is not None:
            raise _pricing_error(
                f"{field_path}.implied_volatility_inversion_status",
                "must be present when implied_volatility is present",
            )

        return

    _validate_implied_volatility_inversion_outcome(
        implied_volatility=implied_volatility,
        implied_volatility_inversion_status=implied_volatility_inversion_status,
        field_path=field_path,
    )


from .surface.contracts import (
    VanillaOptionSurfacePricingExecutionInput,
    VanillaOptionSurfacePricingNodeTable,
    VanillaOptionSurfacePricingRequest,
    VanillaOptionSurfacePricingResponse,
    VanillaOptionSurfacePricingRunResult,
)


__all__ = [
    "AutoExactSparsePricingIntervalSchedulePolicy",
    "AnchoredLocalTiltAdmissibilityStatus",
    "AnchoredLocalTiltBoundaryStateSummary",
    "AnchoredLocalTiltDomainStatus",
    "AnchoredLocalTiltSolveDiagnostics",
    "AnchoredVanillaOptionPricingBoundaryInput",
    "ExplicitIntervalEndTimestampsUtcPricingIntervalSchedulePolicy",
    "FourierInversionContour",
    "FourierInversionEngineSettings",
    "FourierInversionIntegrationStabilityControls",
    "FourierInversionNumericQualityGate",
    "FourierInversionNumericSettings",
    "FourierInversionQuadratureRule",
    "GammaSeverityMarkConfiguration",
    "MeanReversionAnchorModel",
    "MeanReversionBoundaryState",
    "MeanReversionFittedParameters",
    "NigSharedPlusShiftConfiguration",
    "PricingIntervalSchedule",
    "PricingIntervalScheduleMetadata",
    "PricingIntervalSchedulePolicy",
    "VanillaOptionImpliedVolatilityInversionStatus",
    "VanillaOptionPricingCalibrationHooks",
    "VanillaOptionPricingFailureDiagnostic",
    "VanillaOptionPricingFailureStage",
    "VanillaOptionPricingExecutionInput",
    "VanillaOptionPricingOptionalOutputSelection",
    "VanillaOptionPricingStatus",
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
    "VanillaOptionSurfacePricingExecutionInput",
    "VanillaOptionSurfacePricingNodeTable",
    "VanillaOptionSurfacePricingRequest",
    "VanillaOptionSurfacePricingResponse",
    "VanillaOptionSurfacePricingRunResult",
    "default_pricing_interval_schedule_policy",
    "pricing_interval_schedule_policy_from_mapping",
]
