use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformInput,
    ExponentialAffineTransformInputError, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformNewsIntensityInput, ExponentialAffineTransformOutput,
    ExponentialAffineTransformOutputError, ExponentialAffineTransformSolverError,
    ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_optional_settings,
};
use crate::complex_math::ComplexScalar;
use crate::complex_math::{complex_exponential, complex_multiply};
use crate::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelParameters,
    HawkesSeasonalityIntervalError,
};
use crate::hourly_factors::HourlyFactorContractOutput;
use crate::nig::{NigBuySellDriverParameters, NigTradingTimeCharacteristicExponentContributions};
use crate::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionValidationError,
};
use crate::risk_neutral::{
    EsscherMartingaleConditionError, EsscherMartingaleConditionInput,
    EsscherMartingaleConditionReport, EsscherTransformError, NigEsscherTransformationInput,
    evaluate_esscher_martingale_condition,
    transform_nig_characteristic_exponent_under_esscher_tilt,
};

use super::constants::{
    PRICING_MARTINGALE_CONDITION_VALIDATION_POLICY,
    RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
    RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY,
};
use super::contracts::{
    VanillaOptionPricingMarketInputs, VanillaOptionPricingMarketInputsError,
    VanillaOptionPricingRequest, VanillaOptionPricingRequestError,
};
/// Full return characteristic-function assembly output.
///
/// Equation reference:
/// \f[
/// \exp\!\left(A^+\psi_+(u) + A^-\psi_-(-u)\right)\,\varphi_{\Delta M}(u)\,\varphi_{\Delta G}(u).
/// \f]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnCharacteristicFunctionAssembly {
    pub trading_time_exponent_contributions: NigTradingTimeCharacteristicExponentContributions,
    pub trading_time_characteristic_factor: ComplexScalar,
    pub open_jump_characteristic_factor: ComplexScalar,
    pub closed_gap_characteristic_factor: ComplexScalar,
    pub combined_news_jump_characteristic_factor: ComplexScalar,
    pub combined_characteristic_factor: ComplexScalar,
    pub composition_policy: &'static str,
    pub domain_guard_policy: &'static str,
}

/// Deterministic factor-only composition payload for oracle-derived trading-time CF.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnCharacteristicFunctionFactorComposition {
    pub trading_time_characteristic_factor: ComplexScalar,
    pub open_jump_characteristic_factor: ComplexScalar,
    pub closed_gap_characteristic_factor: ComplexScalar,
    pub combined_news_jump_characteristic_factor: ComplexScalar,
    pub combined_characteristic_factor: ComplexScalar,
    pub composition_policy: &'static str,
    pub domain_guard_policy: &'static str,
}

/// Structured failure modes for full return characteristic-function assembly.
#[derive(Debug, Clone, PartialEq)]
pub enum ReturnCharacteristicFunctionAssemblyError {
    InvalidCharacteristicFunctionArgument {
        argument: ComplexScalar,
    },
    InvalidOracleTransformOutput {
        source: ExponentialAffineTransformOutputError,
    },
    InvalidOpenJumpCharacteristicFactor {
        value: ComplexScalar,
    },
    InvalidClosedGapCharacteristicFactor {
        value: ComplexScalar,
    },
    InvalidReturnDecomposition {
        source: ReturnDecompositionValidationError,
    },
    NonFiniteTradingTimeExponent {
        value: ComplexScalar,
    },
    NonFiniteTradingTimeCharacteristicFactor {
        value: ComplexScalar,
    },
    NonFiniteCombinedNewsJumpCharacteristicFactor {
        value: ComplexScalar,
    },
    NonFiniteCombinedCharacteristicFactor {
        value: ComplexScalar,
    },
}

impl Display for ReturnCharacteristicFunctionAssemblyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCharacteristicFunctionArgument { argument } => write!(
                formatter,
                "characteristic-function argument must be finite, found {argument:?}"
            ),
            Self::InvalidOracleTransformOutput { source } => write!(
                formatter,
                "oracle transform output is invalid for characteristic-function assembly: {source}"
            ),
            Self::InvalidOpenJumpCharacteristicFactor { value } => write!(
                formatter,
                "open-jump characteristic factor must be finite, found {value:?}"
            ),
            Self::InvalidClosedGapCharacteristicFactor { value } => write!(
                formatter,
                "closed-gap characteristic factor must be finite, found {value:?}"
            ),
            Self::InvalidReturnDecomposition { source } => {
                write!(
                    formatter,
                    "return decomposition payload is invalid for characteristic-function assembly: {source}"
                )
            }
            Self::NonFiniteTradingTimeExponent { value } => write!(
                formatter,
                "trading-time weighted exponent must be finite, found {value:?}"
            ),
            Self::NonFiniteTradingTimeCharacteristicFactor { value } => write!(
                formatter,
                "trading-time characteristic factor must be finite, found {value:?}"
            ),
            Self::NonFiniteCombinedNewsJumpCharacteristicFactor { value } => write!(
                formatter,
                "combined news-jump characteristic factor must be finite, found {value:?}"
            ),
            Self::NonFiniteCombinedCharacteristicFactor { value } => write!(
                formatter,
                "combined return characteristic factor must be finite, found {value:?}"
            ),
        }
    }
}

impl Error for ReturnCharacteristicFunctionAssemblyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidOracleTransformOutput { source } => Some(source),
            Self::InvalidReturnDecomposition { source } => Some(source),
            _ => None,
        }
    }
}

/// Deterministic diagnostics for one pricing-path martingale-condition validation run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricingMartingaleConditionValidationReport {
    pub theta: f64,
    pub buy_side_log_moment_exponent: ComplexScalar,
    pub sell_side_log_moment_exponent: ComplexScalar,
    pub combined_trading_log_moment_exponent: ComplexScalar,
    pub martingale_condition: EsscherMartingaleConditionReport,
    pub policy: &'static str,
}

/// Structured failure modes for pricing-path martingale-condition validation.
#[derive(Debug, Clone, PartialEq)]
pub enum PricingMartingaleConditionValidationError {
    InvalidMarketInputs {
        source: VanillaOptionPricingMarketInputsError,
    },
    InvalidReturnDecomposition {
        source: ReturnDecompositionValidationError,
    },
    BuySideEsscherTransformFailure {
        source: EsscherTransformError,
    },
    SellSideEsscherTransformFailure {
        source: EsscherTransformError,
    },
    InvalidMartingaleInput {
        source: EsscherMartingaleConditionError,
    },
    MartingaleConditionViolation {
        observed_log_moment_exponent: ComplexScalar,
        target_log_moment_exponent: f64,
        residual_norm: f64,
        tolerance: f64,
    },
}

impl Display for PricingMartingaleConditionValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMarketInputs { source } => {
                write!(formatter, "market_inputs payload is invalid: {source}")
            }
            Self::InvalidReturnDecomposition { source } => write!(
                formatter,
                "return decomposition payload is invalid for martingale validation: {source}"
            ),
            Self::BuySideEsscherTransformFailure { source } => {
                write!(
                    formatter,
                    "buy-side Esscher transform failed during martingale validation: {source}"
                )
            }
            Self::SellSideEsscherTransformFailure { source } => {
                write!(
                    formatter,
                    "sell-side Esscher transform failed during martingale validation: {source}"
                )
            }
            Self::InvalidMartingaleInput { source } => {
                write!(formatter, "martingale input is invalid: {source}")
            }
            Self::MartingaleConditionViolation {
                observed_log_moment_exponent,
                target_log_moment_exponent,
                residual_norm,
                tolerance,
            } => write!(
                formatter,
                "pricing martingale condition failed with observed={observed_log_moment_exponent:?}, target={target_log_moment_exponent}, residual_norm={residual_norm}, tolerance={tolerance}"
            ),
        }
    }
}

impl Error for PricingMartingaleConditionValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMarketInputs { source } => Some(source),
            Self::InvalidReturnDecomposition { source } => Some(source),
            Self::BuySideEsscherTransformFailure { source } => Some(source),
            Self::SellSideEsscherTransformFailure { source } => Some(source),
            Self::InvalidMartingaleInput { source } => Some(source),
            _ => None,
        }
    }
}

/// Resolve interval-aligned hourly factors for the pricing path.
pub use crate::hourly_factors::provide_hourly_factor_contract as load_hourly_factor_contract;

/// Project state to interval intensities for coarse-time sufficient-statistics paths.
pub fn project_hawkes_intensities_coarse_interval(
    state: HawkesExcitationState,
    parameters: HawkesKernelParameters,
    hourly_factor_contract: &HourlyFactorContractOutput,
    interval_index: usize,
) -> Result<HawkesIntensityProjection, HawkesSeasonalityIntervalError> {
    state.project_intensities_for_hourly_interval(
        parameters.lambda_bar,
        hourly_factor_contract,
        interval_index,
    )
}

/// Solve one-interval exponential-affine transform for pricing workflows.
///
/// This adapter keeps the pricing layer conversion-focused and delegates to the
/// shared solver entrypoint in `affine_oracle`.
pub fn solve_exponential_affine_transform(
    transform_input: ExponentialAffineTransformInput,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(transform_input, None)
}

/// Solve one-interval exponential-affine transform for pricing workflows with explicit settings.
pub fn solve_exponential_affine_transform_with_settings(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(
        transform_input,
        Some(solver_settings),
    )
}

/// Build one-interval transform input from a validated pricing request.
///
/// This helper keeps interval-delta propagation explicit between pricing contracts
/// and the shared affine-oracle solver API.
#[allow(clippy::too_many_arguments)]
pub fn build_pricing_transform_input(
    pricing_request: VanillaOptionPricingRequest,
    u_plus: ComplexScalar,
    u_minus: ComplexScalar,
    z_plus: ComplexScalar,
    z_minus: ComplexScalar,
    seasonality_factor: f64,
    news_intensity: ExponentialAffineTransformNewsIntensityInput,
) -> Result<ExponentialAffineTransformInput, PricingTransformInputBuildError> {
    pricing_request
        .validate()
        .map_err(|source| PricingTransformInputBuildError::InvalidPricingRequest { source })?;

    let equation_specification = ExponentialAffineTransformEquationSpecification::new(
        pricing_request.model_parameters.hawkes_kernel_parameters,
        pricing_request
            .model_parameters
            .gamma_severity_mark_configuration,
    );

    ExponentialAffineTransformInput::try_new(
        equation_specification,
        u_plus,
        u_minus,
        z_plus,
        z_minus,
        seasonality_factor,
        news_intensity,
        pricing_request.interval_delta,
        pricing_request.boundary_state,
    )
    .map_err(|source| PricingTransformInputBuildError::InvalidTransformInput { source })
}

/// Structured failure for pricing-request to solver-input conversion.
#[derive(Debug, Clone, PartialEq)]
pub enum PricingTransformInputBuildError {
    InvalidPricingRequest {
        source: VanillaOptionPricingRequestError,
    },
    InvalidTransformInput {
        source: ExponentialAffineTransformInputError,
    },
}

impl Display for PricingTransformInputBuildError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPricingRequest { source } => {
                write!(formatter, "pricing request is invalid: {source}")
            }
            Self::InvalidTransformInput { source } => {
                write!(
                    formatter,
                    "failed to build affine-oracle transform input from pricing request: {source}"
                )
            }
        }
    }
}

impl Error for PricingTransformInputBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPricingRequest { source } => Some(source),
            Self::InvalidTransformInput { source } => Some(source),
        }
    }
}

/// Compose trading-time characteristic exponent for pricing using canonical sign handling.
///
/// Equation reference:
/// \f[
/// A^+\psi_+(u) + A^-\psi_-(-u).
/// \f]
pub fn compose_nig_trading_time_exponent(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
) -> NigTradingTimeCharacteristicExponentContributions {
    compose_nig_trading_time_exponent_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        ReturnDecompositionEdgeMode::Disabled,
    )
}

/// Compose trading-time characteristic exponent for pricing with optional edge-mode gating.
pub use crate::return_decomposition::compose_nig_trading_time_exponent_with_edge_mode;

/// Compose full return characteristic function for pricing through one shared assembler.
///
/// This path reuses the canonical trading-time exponent composition helper, which
/// keeps sell-side `-u` convention logic in a single module.
pub fn compose_total_return_characteristic_function(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    open_jump_characteristic_factor: ComplexScalar,
    closed_gap_characteristic_factor: ComplexScalar,
) -> Result<ReturnCharacteristicFunctionAssembly, ReturnCharacteristicFunctionAssemblyError> {
    compose_total_return_characteristic_function_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        open_jump_characteristic_factor,
        closed_gap_characteristic_factor,
        ReturnDecompositionEdgeMode::Disabled,
    )
}

/// Validate pricing-path martingale wiring for Esscher-tilted NIG components.
///
/// This check is trading-time only.
/// Gap martingale adjustments are intentionally left to the gap-pricing path.
pub fn validate_pricing_martingale_condition_under_esscher_tilt(
    driver_parameters: NigBuySellDriverParameters,
    return_decomposition: ReturnDecomposition,
    market_inputs: VanillaOptionPricingMarketInputs,
    theta: f64,
    tolerance: f64,
) -> Result<PricingMartingaleConditionValidationReport, PricingMartingaleConditionValidationError> {
    market_inputs.validate().map_err(|source| {
        PricingMartingaleConditionValidationError::InvalidMarketInputs { source }
    })?;
    return_decomposition.validate().map_err(|source| {
        PricingMartingaleConditionValidationError::InvalidReturnDecomposition { source }
    })?;

    let buy_side_log_moment_exponent =
        transform_nig_characteristic_exponent_under_esscher_tilt(
            NigEsscherTransformationInput::new(
                driver_parameters.buy_side_parameters,
                ComplexScalar::new(0.0, -1.0),
                theta,
            ),
        )
        .map_err(|source| {
            PricingMartingaleConditionValidationError::BuySideEsscherTransformFailure { source }
        })?
        .transformed_exponent;

    let sell_side_log_moment_exponent =
        transform_nig_characteristic_exponent_under_esscher_tilt(
            NigEsscherTransformationInput::new(
                driver_parameters.sell_side_parameters,
                ComplexScalar::new(0.0, 1.0),
                theta,
            ),
        )
        .map_err(|source| {
            PricingMartingaleConditionValidationError::SellSideEsscherTransformFailure { source }
        })?
        .transformed_exponent;

    let combined_trading_log_moment_exponent = buy_side_log_moment_exponent
        .scale(return_decomposition.a_plus)
        .add(sell_side_log_moment_exponent.scale(return_decomposition.a_minus));
    let target_log_moment_exponent = (market_inputs.risk_free_rate - market_inputs.dividend_yield)
        * market_inputs.maturity_interval;
    let martingale_condition =
        evaluate_esscher_martingale_condition(EsscherMartingaleConditionInput::new(
            combined_trading_log_moment_exponent,
            target_log_moment_exponent,
            tolerance,
        ))
        .map_err(|source| {
            PricingMartingaleConditionValidationError::InvalidMartingaleInput { source }
        })?;

    if !martingale_condition.is_satisfied {
        return Err(
            PricingMartingaleConditionValidationError::MartingaleConditionViolation {
                observed_log_moment_exponent: combined_trading_log_moment_exponent,
                target_log_moment_exponent,
                residual_norm: martingale_condition.residual_norm,
                tolerance: martingale_condition.tolerance,
            },
        );
    }

    Ok(PricingMartingaleConditionValidationReport {
        theta,
        buy_side_log_moment_exponent,
        sell_side_log_moment_exponent,
        combined_trading_log_moment_exponent,
        martingale_condition,
        policy: PRICING_MARTINGALE_CONDITION_VALIDATION_POLICY,
    })
}

/// Apply one coarse-time transform-path Hawkes state transition using canonical call order.
pub use crate::hawkes::evolve_hawkes_state;
pub fn compose_total_return_characteristic_function_with_edge_mode(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    open_jump_characteristic_factor: ComplexScalar,
    closed_gap_characteristic_factor: ComplexScalar,
    edge_mode: ReturnDecompositionEdgeMode,
) -> Result<ReturnCharacteristicFunctionAssembly, ReturnCharacteristicFunctionAssemblyError> {
    if !argument.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::InvalidCharacteristicFunctionArgument {
                argument,
            },
        );
    }

    let effective_decomposition = return_decomposition.with_edge_mode(edge_mode);
    let effective_open_jump_characteristic_factor = if edge_mode.open_jump_component_enabled() {
        open_jump_characteristic_factor
    } else {
        ComplexScalar::new(1.0, 0.0)
    };
    let effective_closed_gap_characteristic_factor = if edge_mode.closed_gap_component_enabled() {
        closed_gap_characteristic_factor
    } else {
        ComplexScalar::new(1.0, 0.0)
    };

    effective_decomposition.validate().map_err(|source| {
        ReturnCharacteristicFunctionAssemblyError::InvalidReturnDecomposition { source }
    })?;

    let trading_time_exponent_contributions =
        effective_decomposition.compose_nig_trading_time_exponent(driver_parameters, argument);
    let combined_weighted_exponent = trading_time_exponent_contributions.combined_weighted_exponent;
    if !combined_weighted_exponent.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::NonFiniteTradingTimeExponent {
                value: combined_weighted_exponent,
            },
        );
    }

    let trading_time_characteristic_factor = complex_exponential(combined_weighted_exponent);
    let factor_composition = compose_return_characteristic_factor_shared(
        trading_time_characteristic_factor,
        effective_open_jump_characteristic_factor,
        effective_closed_gap_characteristic_factor,
    )?;

    Ok(ReturnCharacteristicFunctionAssembly {
        trading_time_exponent_contributions,
        trading_time_characteristic_factor: factor_composition.trading_time_characteristic_factor,
        open_jump_characteristic_factor: factor_composition.open_jump_characteristic_factor,
        closed_gap_characteristic_factor: factor_composition.closed_gap_characteristic_factor,
        combined_news_jump_characteristic_factor: factor_composition
            .combined_news_jump_characteristic_factor,
        combined_characteristic_factor: factor_composition.combined_characteristic_factor,
        composition_policy: factor_composition.composition_policy,
        domain_guard_policy: factor_composition.domain_guard_policy,
    })
}

/// Compose full return characteristic factors from one oracle solve output.
pub fn compose_total_return_characteristic_function_from_oracle_output(
    oracle_output: ExponentialAffineTransformOutput,
    open_jump_characteristic_factor: ComplexScalar,
    closed_gap_characteristic_factor: ComplexScalar,
) -> Result<ReturnCharacteristicFunctionFactorComposition, ReturnCharacteristicFunctionAssemblyError>
{
    oracle_output.validate().map_err(|source| {
        ReturnCharacteristicFunctionAssemblyError::InvalidOracleTransformOutput { source }
    })?;

    compose_return_characteristic_factor_shared(
        oracle_output.transform_value,
        open_jump_characteristic_factor,
        closed_gap_characteristic_factor,
    )
}

fn compose_return_characteristic_factor_shared(
    trading_time_characteristic_factor: ComplexScalar,
    open_jump_characteristic_factor: ComplexScalar,
    closed_gap_characteristic_factor: ComplexScalar,
) -> Result<ReturnCharacteristicFunctionFactorComposition, ReturnCharacteristicFunctionAssemblyError>
{
    if !trading_time_characteristic_factor.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::NonFiniteTradingTimeCharacteristicFactor {
                value: trading_time_characteristic_factor,
            },
        );
    }

    if !open_jump_characteristic_factor.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::InvalidOpenJumpCharacteristicFactor {
                value: open_jump_characteristic_factor,
            },
        );
    }

    if !closed_gap_characteristic_factor.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::InvalidClosedGapCharacteristicFactor {
                value: closed_gap_characteristic_factor,
            },
        );
    }

    let combined_news_jump_characteristic_factor = complex_multiply(
        open_jump_characteristic_factor,
        closed_gap_characteristic_factor,
    );
    if !combined_news_jump_characteristic_factor.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::NonFiniteCombinedNewsJumpCharacteristicFactor {
                value: combined_news_jump_characteristic_factor,
            },
        );
    }

    let combined_characteristic_factor = complex_multiply(
        trading_time_characteristic_factor,
        combined_news_jump_characteristic_factor,
    );
    if !combined_characteristic_factor.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::NonFiniteCombinedCharacteristicFactor {
                value: combined_characteristic_factor,
            },
        );
    }

    Ok(ReturnCharacteristicFunctionFactorComposition {
        trading_time_characteristic_factor,
        open_jump_characteristic_factor,
        closed_gap_characteristic_factor,
        combined_news_jump_characteristic_factor,
        combined_characteristic_factor,
        composition_policy: RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
        domain_guard_policy: RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY,
    })
}
