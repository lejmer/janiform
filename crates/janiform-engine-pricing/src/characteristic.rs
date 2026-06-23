use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformActivityLoadings, ExponentialAffineTransformEquationSpecification,
    ExponentialAffineTransformInput, ExponentialAffineTransformInputError,
    ExponentialAffineTransformNewsIntensityInput, ExponentialAffineTransformOutput,
    ExponentialAffineTransformOutputError,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_foundations::complex_math::{complex_exponential, complex_multiply};
use janiform_model_kernels::nig::{
    NigBuySellDriverParameters, NigTradingTimeCharacteristicExponentContributions,
};
use janiform_model_kernels::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionValidationError,
};
use janiform_model_kernels::risk_neutral::{EsscherTransformError, NigEsscherTiltDomain};

use super::constants::{
    PRICING_LOCAL_ANCHORED_TILT_SOLVE_POLICY, RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
    RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY,
};
use super::contracts::{
    AnchoredLocalTiltAdmissibilityStatus, AnchoredLocalTiltBoundaryStateSummary,
    AnchoredLocalTiltDomainStatus, AnchoredLocalTiltSolveDiagnostics, VanillaOptionPricingRequest,
    VanillaOptionPricingRequestError,
};
use super::time_scaling::intraday_year_fraction;

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

/// Structured failure modes for anchored local pricing tilt solves.
#[derive(Debug, Clone, PartialEq)]
pub enum AnchoredLocalTiltSolveError {
    InvalidPricingRequest {
        source: VanillaOptionPricingRequestError,
    },
    InvalidReturnDecomposition {
        source: ReturnDecompositionValidationError,
    },
    BuySideEsscherTransformFailure {
        theta: f64,
        source: EsscherTransformError,
    },
    SellSideEsscherTransformFailure {
        theta: f64,
        source: EsscherTransformError,
    },
    NonFiniteTransformInput {
        theta: f64,
        value: ComplexScalar,
    },
    TransformEvaluationFailure {
        theta: f64,
        message: String,
    },
    NonRealObservedLogMomentExponent {
        theta: f64,
        value: ComplexScalar,
    },
    OutOfDomainStateRequest {
        theta: f64,
        argument_imaginary: f64,
        lower_theta_exclusive: f64,
        upper_theta_exclusive: f64,
    },
    InvalidTolerance {
        value: f64,
    },
    NoAdmissibleLocalTilt {
        target_log_moment_exponent: f64,
        lower_theta: f64,
        upper_theta: f64,
        lower_residual: f64,
        upper_residual: f64,
    },
    UndefinedImplicitThetaSensitivity {
        theta: f64,
        residual_theta_derivative: f64,
    },
}

impl Display for AnchoredLocalTiltSolveError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPricingRequest { source } => {
                write!(formatter, "pricing request payload is invalid: {source}")
            }
            Self::InvalidReturnDecomposition { source } => write!(
                formatter,
                "return decomposition payload is invalid for anchored local tilt solving: {source}"
            ),
            Self::BuySideEsscherTransformFailure { theta, source } => write!(
                formatter,
                "buy-side Esscher transform failed during anchored local tilt solving at theta={theta}: {source}"
            ),
            Self::SellSideEsscherTransformFailure { theta, source } => write!(
                formatter,
                "sell-side Esscher transform failed during anchored local tilt solving at theta={theta}: {source}"
            ),
            Self::NonFiniteTransformInput { theta, value } => write!(
                formatter,
                "anchored local tilt solve produced a non-finite transform input at theta={theta}: {value:?}"
            ),
            Self::TransformEvaluationFailure { theta, message } => write!(
                formatter,
                "anchored local tilt solve could not evaluate the transform at theta={theta}: {message}"
            ),
            Self::NonRealObservedLogMomentExponent { theta, value } => write!(
                formatter,
                "anchored local tilt solve produced a non-real observed log moment exponent at theta={theta}: {value:?}"
            ),
            Self::OutOfDomainStateRequest {
                theta,
                argument_imaginary,
                lower_theta_exclusive,
                upper_theta_exclusive,
            } => write!(
                formatter,
                "anchored local tilt solve requested an out-of-domain state at theta={theta} for argument_imaginary={argument_imaginary} with admissible_theta_interval=({lower_theta_exclusive}, {upper_theta_exclusive})"
            ),
            Self::InvalidTolerance { value } => write!(
                formatter,
                "anchored local tilt solve tolerance must be finite and strictly positive, found {value}"
            ),
            Self::NoAdmissibleLocalTilt {
                target_log_moment_exponent,
                lower_theta,
                upper_theta,
                lower_residual,
                upper_residual,
            } => write!(
                formatter,
                "anchored local tilt solve found no admissible theta for target={target_log_moment_exponent} across [{lower_theta}, {upper_theta}] with residuals [{lower_residual}, {upper_residual}]"
            ),
            Self::UndefinedImplicitThetaSensitivity {
                theta,
                residual_theta_derivative,
            } => write!(
                formatter,
                "anchored local tilt implicit sensitivity is undefined at theta={theta} because residual_theta_derivative={residual_theta_derivative}"
            ),
        }
    }
}

impl Error for AnchoredLocalTiltSolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPricingRequest { source } => Some(source),
            Self::InvalidReturnDecomposition { source } => Some(source),
            Self::BuySideEsscherTransformFailure { source, .. } => Some(source),
            Self::SellSideEsscherTransformFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LocalTiltResidualEvaluation {
    pub(crate) observed_log_moment_exponent: f64,
    pub(crate) residual: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg(test)]
pub(crate) struct LocalTiltResidualSharedPlusShiftSensitivity {
    pub(crate) observed_log_moment_exponent: f64,
    pub(crate) residual: f64,
    pub(crate) alpha_derivative: f64,
    pub(crate) beta_0_derivative: f64,
    pub(crate) delta_derivative: f64,
    pub(crate) mu_derivative: f64,
    pub(crate) delta_beta_derivative: f64,
    pub(crate) theta_derivative: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg(test)]
pub(crate) struct LocalTiltImplicitThetaSharedPlusShiftSensitivity {
    pub(crate) theta: f64,
    pub(crate) alpha_derivative: f64,
    pub(crate) beta_0_derivative: f64,
    pub(crate) delta_derivative: f64,
    pub(crate) mu_derivative: f64,
    pub(crate) delta_beta_derivative: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LocalTiltSearchDomain {
    pub(crate) lower_theta: f64,
    pub(crate) upper_theta: f64,
    pub(crate) reference_theta: f64,
}

pub(crate) const MARTINGALE_FIRST_LOG_MOMENT_SHIFT: f64 = 1.0;

impl LocalTiltSearchDomain {
    pub(crate) fn contains(self, theta: f64) -> bool {
        NigEsscherTiltDomain::new(self.lower_theta, self.upper_theta).contains(theta)
    }

    fn side_for_theta(self, theta: f64) -> Option<LocalTiltSearchSide> {
        if !self.contains(theta) || theta == self.reference_theta {
            return None;
        }

        if theta < self.reference_theta {
            Some(LocalTiltSearchSide::Lower)
        } else {
            Some(LocalTiltSearchSide::Upper)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalTiltSearchSide {
    Lower,
    Upper,
}

pub(crate) fn anchored_boundary_state_summary(
    pricing_request: &VanillaOptionPricingRequest,
) -> Result<AnchoredLocalTiltBoundaryStateSummary, AnchoredLocalTiltSolveError> {
    let boundary_input = &pricing_request.boundary_input;

    let current_anchor_value = boundary_input.current_anchor_value().map_err(|source| {
        AnchoredLocalTiltSolveError::InvalidPricingRequest {
            source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
        }
    })?;

    let interval_delta = boundary_input.interval_delta().map_err(|source| {
        AnchoredLocalTiltSolveError::InvalidPricingRequest {
            source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
        }
    })?;

    let deviation_decay_factor = boundary_input.deviation_decay_factor().map_err(|source| {
        AnchoredLocalTiltSolveError::InvalidPricingRequest {
            source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
        }
    })?;

    let local_martingale_deviation_adjustment = boundary_input
        .local_martingale_deviation_adjustment()
        .map_err(
            |source| AnchoredLocalTiltSolveError::InvalidPricingRequest {
                source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
            },
        )?;

    Ok(AnchoredLocalTiltBoundaryStateSummary {
        interval_index: boundary_input.interval_index,
        hawkes_boundary_state: boundary_input.hawkes_boundary_state,
        observed_boundary_state: boundary_input.observed_boundary_state,
        current_anchor_value,
        interval_delta,
        kappa: boundary_input.mean_reversion_fitted_parameters.kappa,
        deviation_decay_factor,
        local_martingale_deviation_adjustment,
    })
}

pub(crate) fn target_log_moment_exponent(
    pricing_request: &VanillaOptionPricingRequest,
) -> Result<f64, AnchoredLocalTiltSolveError> {
    let local_martingale_deviation_adjustment = pricing_request
        .boundary_input
        .local_martingale_deviation_adjustment()
        .map_err(
            |source| AnchoredLocalTiltSolveError::InvalidPricingRequest {
                source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
            },
        )?;

    Ok((pricing_request.market_inputs.risk_free_rate
        - pricing_request.market_inputs.dividend_yield)
        * intraday_year_fraction(pricing_request.interval_delta)
        - local_martingale_deviation_adjustment)
}

fn evaluate_local_tilt_residual(
    driver_parameters: NigBuySellDriverParameters,
    return_decomposition: ReturnDecomposition,
    target_log_moment_exponent: f64,
    theta: f64,
) -> Result<LocalTiltResidualEvaluation, AnchoredLocalTiltSolveError> {
    let observed_log_moment_exponent = driver_parameters
        .evaluate_return_composition_log_moment(
            return_decomposition.a_plus,
            return_decomposition.a_minus,
            theta,
        )
        .map_err(|source| map_local_tilt_evaluation_error(theta, source))?;
    if !observed_log_moment_exponent.is_finite() {
        return Err(AnchoredLocalTiltSolveError::NonFiniteTransformInput {
            theta,
            value: ComplexScalar::new(observed_log_moment_exponent, 0.0),
        });
    }

    Ok(LocalTiltResidualEvaluation {
        observed_log_moment_exponent,
        residual: observed_log_moment_exponent - target_log_moment_exponent,
    })
}

#[cfg(test)]
pub(crate) fn differentiate_local_tilt_residual_for_shared_plus_shift_configuration(
    driver_parameters: NigBuySellDriverParameters,
    return_decomposition: ReturnDecomposition,
    target_log_moment_exponent: f64,
    theta: f64,
) -> Result<LocalTiltResidualSharedPlusShiftSensitivity, AnchoredLocalTiltSolveError> {
    let log_moment_argument = ComplexScalar::new(0.0, -theta);

    let buy_side_sensitivity = driver_parameters
        .differentiate_buy_side_characteristic_exponent_for_shared_plus_shift_configuration(
            log_moment_argument,
        )
        .map_err(|source| map_local_tilt_side_evaluation_error(theta, source, true))?;
    let sell_side_sensitivity = driver_parameters
        .differentiate_sell_side_characteristic_exponent_for_return_composition_for_shared_plus_shift_configuration(
            log_moment_argument,
        )
        .map_err(|source| map_local_tilt_side_evaluation_error(theta, source, false))?;

    let observed_log_moment_exponent = driver_parameters
        .evaluate_return_composition_log_moment(
            return_decomposition.a_plus,
            return_decomposition.a_minus,
            theta,
        )
        .map_err(|source| map_local_tilt_evaluation_error(theta, source))?;

    let theta_derivative = driver_parameters
        .differentiate_return_composition_log_moment(
            return_decomposition.a_plus,
            return_decomposition.a_minus,
            theta,
        )
        .map_err(|source| map_local_tilt_evaluation_error(theta, source))?;

    Ok(LocalTiltResidualSharedPlusShiftSensitivity {
        observed_log_moment_exponent,
        residual: observed_log_moment_exponent - target_log_moment_exponent,
        alpha_derivative: return_decomposition.a_plus.mul_add(
            buy_side_sensitivity.alpha_derivative.real,
            return_decomposition.a_minus * sell_side_sensitivity.alpha_derivative.real,
        ),
        beta_0_derivative: return_decomposition.a_plus.mul_add(
            buy_side_sensitivity.beta_0_derivative.real,
            return_decomposition.a_minus * sell_side_sensitivity.beta_0_derivative.real,
        ),
        delta_derivative: return_decomposition.a_plus.mul_add(
            buy_side_sensitivity.delta_derivative.real,
            return_decomposition.a_minus * sell_side_sensitivity.delta_derivative.real,
        ),
        mu_derivative: return_decomposition.a_plus.mul_add(
            buy_side_sensitivity.mu_derivative.real,
            return_decomposition.a_minus * sell_side_sensitivity.mu_derivative.real,
        ),
        delta_beta_derivative: return_decomposition.a_plus.mul_add(
            buy_side_sensitivity.delta_beta_derivative.real,
            return_decomposition.a_minus * sell_side_sensitivity.delta_beta_derivative.real,
        ),
        theta_derivative,
    })
}

#[cfg(test)]
pub(crate) fn differentiate_local_tilt_root_for_shared_plus_shift_configuration(
    driver_parameters: NigBuySellDriverParameters,
    return_decomposition: ReturnDecomposition,
    target_log_moment_exponent: f64,
    theta: f64,
) -> Result<LocalTiltImplicitThetaSharedPlusShiftSensitivity, AnchoredLocalTiltSolveError> {
    let residual_sensitivity =
        differentiate_local_tilt_residual_for_shared_plus_shift_configuration(
            driver_parameters,
            return_decomposition,
            target_log_moment_exponent,
            theta,
        )?;

    if !residual_sensitivity.theta_derivative.is_finite()
        || residual_sensitivity.theta_derivative == 0.0
    {
        return Err(
            AnchoredLocalTiltSolveError::UndefinedImplicitThetaSensitivity {
                theta,
                residual_theta_derivative: residual_sensitivity.theta_derivative,
            },
        );
    }

    let inverse_residual_theta_derivative = -1.0 / residual_sensitivity.theta_derivative;

    Ok(LocalTiltImplicitThetaSharedPlusShiftSensitivity {
        theta,
        alpha_derivative: residual_sensitivity.alpha_derivative * inverse_residual_theta_derivative,
        beta_0_derivative: residual_sensitivity.beta_0_derivative
            * inverse_residual_theta_derivative,
        delta_derivative: residual_sensitivity.delta_derivative * inverse_residual_theta_derivative,
        mu_derivative: residual_sensitivity.mu_derivative * inverse_residual_theta_derivative,
        delta_beta_derivative: residual_sensitivity.delta_beta_derivative
            * inverse_residual_theta_derivative,
    })
}

fn anchored_local_tilt_domain(
    driver_parameters: NigBuySellDriverParameters,
) -> Result<NigEsscherTiltDomain, AnchoredLocalTiltSolveError> {
    let domain = driver_parameters.derive_return_composition_log_moment_domain();
    if domain.is_empty() {
        return Err(AnchoredLocalTiltSolveError::NoAdmissibleLocalTilt {
            target_log_moment_exponent: f64::NAN,
            lower_theta: domain.lower_theta_exclusive,
            upper_theta: domain.upper_theta_exclusive,
            lower_residual: f64::NAN,
            upper_residual: f64::NAN,
        });
    }

    Ok(domain)
}

pub(crate) fn resolve_local_martingale_tilt_search_domain(
    driver_parameters: NigBuySellDriverParameters,
) -> Result<LocalTiltSearchDomain, AnchoredLocalTiltSolveError> {
    let admissible_domain = anchored_local_tilt_domain(driver_parameters)?;

    let first_moment_domain = admissible_domain.intersect(NigEsscherTiltDomain::new(
        admissible_domain.lower_theta_exclusive - MARTINGALE_FIRST_LOG_MOMENT_SHIFT,
        admissible_domain.upper_theta_exclusive - MARTINGALE_FIRST_LOG_MOMENT_SHIFT,
    ));

    local_tilt_search_domain_from_admissible_domain(first_moment_domain)
}

fn local_tilt_search_domain_from_admissible_domain(
    admissible_domain: NigEsscherTiltDomain,
) -> Result<LocalTiltSearchDomain, AnchoredLocalTiltSolveError> {
    let Some(lower_theta) = admissible_domain.interior_lower_bound() else {
        return Err(AnchoredLocalTiltSolveError::NoAdmissibleLocalTilt {
            target_log_moment_exponent: f64::NAN,
            lower_theta: admissible_domain.lower_theta_exclusive,
            upper_theta: admissible_domain.upper_theta_exclusive,
            lower_residual: f64::NAN,
            upper_residual: f64::NAN,
        });
    };

    let Some(upper_theta) = admissible_domain.interior_upper_bound() else {
        return Err(AnchoredLocalTiltSolveError::NoAdmissibleLocalTilt {
            target_log_moment_exponent: f64::NAN,
            lower_theta: admissible_domain.lower_theta_exclusive,
            upper_theta: admissible_domain.upper_theta_exclusive,
            lower_residual: f64::NAN,
            upper_residual: f64::NAN,
        });
    };

    let Some(reference_theta) = nearest_zero_theta_in_domain(admissible_domain) else {
        return Err(AnchoredLocalTiltSolveError::NoAdmissibleLocalTilt {
            target_log_moment_exponent: f64::NAN,
            lower_theta: admissible_domain.lower_theta_exclusive,
            upper_theta: admissible_domain.upper_theta_exclusive,
            lower_residual: f64::NAN,
            upper_residual: f64::NAN,
        });
    };

    Ok(LocalTiltSearchDomain {
        lower_theta,
        upper_theta,
        reference_theta,
    })
}

fn map_local_tilt_evaluation_error(
    theta: f64,
    source: EsscherTransformError,
) -> AnchoredLocalTiltSolveError {
    match source {
        EsscherTransformError::PureImaginaryArgumentOutOfDomain {
            lower_theta_exclusive,
            upper_theta_exclusive,
            ..
        } => AnchoredLocalTiltSolveError::OutOfDomainStateRequest {
            theta,
            argument_imaginary: 0.0,
            lower_theta_exclusive,
            upper_theta_exclusive,
        },
        EsscherTransformError::NonFinitePhysicalExponentAtShiftedArgument { value }
        | EsscherTransformError::NonFinitePhysicalExponentAtAnchorArgument { value }
        | EsscherTransformError::NonFiniteTransformedExponent { value } => {
            AnchoredLocalTiltSolveError::NonFiniteTransformInput { theta, value }
        }
        other => AnchoredLocalTiltSolveError::TransformEvaluationFailure {
            theta,
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
fn map_local_tilt_side_evaluation_error(
    theta: f64,
    source: EsscherTransformError,
    is_buy_side: bool,
) -> AnchoredLocalTiltSolveError {
    match source {
        EsscherTransformError::PureImaginaryArgumentOutOfDomain {
            lower_theta_exclusive,
            upper_theta_exclusive,
            ..
        } => AnchoredLocalTiltSolveError::OutOfDomainStateRequest {
            theta,
            argument_imaginary: 0.0,
            lower_theta_exclusive,
            upper_theta_exclusive,
        },
        EsscherTransformError::NonFinitePhysicalExponentAtShiftedArgument { value }
        | EsscherTransformError::NonFinitePhysicalExponentAtAnchorArgument { value }
        | EsscherTransformError::NonFiniteTransformedExponent { value } => {
            AnchoredLocalTiltSolveError::NonFiniteTransformInput { theta, value }
        }
        other if is_buy_side => AnchoredLocalTiltSolveError::BuySideEsscherTransformFailure {
            theta,
            source: other,
        },
        other => AnchoredLocalTiltSolveError::SellSideEsscherTransformFailure {
            theta,
            source: other,
        },
    }
}

pub(crate) fn build_local_tilt_bracket_with_residual_evaluator<F>(
    driver_parameters: NigBuySellDriverParameters,
    target_log_moment_exponent: f64,
    tolerance: f64,
    mut evaluate_residual: F,
) -> Result<(f64, LocalTiltResidualEvaluation), AnchoredLocalTiltSolveError>
where
    F: FnMut(f64) -> Result<LocalTiltResidualEvaluation, AnchoredLocalTiltSolveError>,
{
    let search_domain = resolve_local_martingale_tilt_search_domain(driver_parameters).map_err(
        |error| match error {
            AnchoredLocalTiltSolveError::NoAdmissibleLocalTilt {
                lower_theta,
                upper_theta,
                lower_residual,
                upper_residual,
                ..
            } => AnchoredLocalTiltSolveError::NoAdmissibleLocalTilt {
                target_log_moment_exponent,
                lower_theta,
                upper_theta,
                lower_residual,
                upper_residual,
            },
            other => other,
        },
    )?;

    build_local_tilt_bracket_with_search_domain_and_residual_evaluator(
        search_domain,
        target_log_moment_exponent,
        tolerance,
        &mut evaluate_residual,
    )
}

pub(crate) fn build_local_tilt_bracket_with_search_domain_and_residual_evaluator<F>(
    search_domain: LocalTiltSearchDomain,
    target_log_moment_exponent: f64,
    tolerance: f64,
    evaluate_residual: F,
) -> Result<(f64, LocalTiltResidualEvaluation), AnchoredLocalTiltSolveError>
where
    F: FnMut(f64) -> Result<LocalTiltResidualEvaluation, AnchoredLocalTiltSolveError>,
{
    build_local_tilt_bracket_with_search_domain_preferred_theta_and_residual_evaluator(
        search_domain,
        target_log_moment_exponent,
        tolerance,
        None,
        evaluate_residual,
    )
}

pub(crate) fn build_local_tilt_bracket_with_search_domain_preferred_theta_and_residual_evaluator<
    F,
>(
    search_domain: LocalTiltSearchDomain,
    target_log_moment_exponent: f64,
    tolerance: f64,
    preferred_theta: Option<f64>,
    mut evaluate_residual: F,
) -> Result<(f64, LocalTiltResidualEvaluation), AnchoredLocalTiltSolveError>
where
    F: FnMut(f64) -> Result<LocalTiltResidualEvaluation, AnchoredLocalTiltSolveError>,
{
    let lower_theta = search_domain.lower_theta;
    let upper_theta = search_domain.upper_theta;

    let reference_theta = search_domain.reference_theta;

    let (reference_theta, reference_evaluation) = match evaluate_residual(reference_theta) {
        Ok(reference_evaluation) => (reference_theta, reference_evaluation),
        Err(error) if should_retry_local_tilt_side_evaluation(&error) => {
            locate_nearest_valid_reference_evaluation(
                search_domain,
                reference_theta,
                &mut evaluate_residual,
            )?
            .ok_or(AnchoredLocalTiltSolveError::NoAdmissibleLocalTilt {
                target_log_moment_exponent,
                lower_theta,
                upper_theta,
                lower_residual: f64::NAN,
                upper_residual: f64::NAN,
            })?
        }
        Err(error) => return Err(error),
    };

    if reference_evaluation.residual.abs() <= tolerance {
        return Ok((reference_theta, reference_evaluation));
    }

    let preferred_side = preferred_theta.and_then(|theta| search_domain.side_for_theta(theta));

    let left_candidate =
        if lower_theta < reference_theta && preferred_side != Some(LocalTiltSearchSide::Upper) {
            locate_nearest_root_on_side(
                reference_theta,
                reference_evaluation,
                lower_theta,
                preferred_theta.filter(|_| preferred_side == Some(LocalTiltSearchSide::Lower)),
                tolerance,
                &mut evaluate_residual,
            )?
        } else {
            None
        };

    let right_candidate =
        if upper_theta > reference_theta && preferred_side != Some(LocalTiltSearchSide::Lower) {
            locate_nearest_root_on_side(
                reference_theta,
                reference_evaluation,
                upper_theta,
                preferred_theta.filter(|_| preferred_side == Some(LocalTiltSearchSide::Upper)),
                tolerance,
                &mut evaluate_residual,
            )?
        } else {
            None
        };

    let left_candidate = if left_candidate.is_none()
        && lower_theta < reference_theta
        && preferred_side == Some(LocalTiltSearchSide::Upper)
        && right_candidate.is_none()
    {
        locate_nearest_root_on_side(
            reference_theta,
            reference_evaluation,
            lower_theta,
            None,
            tolerance,
            &mut evaluate_residual,
        )?
    } else {
        left_candidate
    };

    let right_candidate = if right_candidate.is_none()
        && upper_theta > reference_theta
        && preferred_side == Some(LocalTiltSearchSide::Lower)
        && left_candidate.is_none()
    {
        locate_nearest_root_on_side(
            reference_theta,
            reference_evaluation,
            upper_theta,
            None,
            tolerance,
            &mut evaluate_residual,
        )?
    } else {
        right_candidate
    };

    select_preferred_root_candidate(left_candidate, right_candidate).ok_or(
        AnchoredLocalTiltSolveError::NoAdmissibleLocalTilt {
            target_log_moment_exponent,
            lower_theta,
            upper_theta,
            lower_residual: f64::NAN,
            upper_residual: f64::NAN,
        },
    )
}

fn build_local_tilt_bracket(
    driver_parameters: NigBuySellDriverParameters,
    return_decomposition: ReturnDecomposition,
    target_log_moment_exponent: f64,
    tolerance: f64,
) -> Result<(f64, LocalTiltResidualEvaluation), AnchoredLocalTiltSolveError> {
    build_local_tilt_bracket_with_residual_evaluator(
        driver_parameters,
        target_log_moment_exponent,
        tolerance,
        |theta| {
            evaluate_local_tilt_residual(
                driver_parameters,
                return_decomposition,
                target_log_moment_exponent,
                theta,
            )
        },
    )
}

fn nearest_zero_theta_in_domain(admissible_domain: NigEsscherTiltDomain) -> Option<f64> {
    if admissible_domain.contains(0.0) {
        return Some(0.0);
    }

    if admissible_domain.upper_theta_exclusive <= 0.0 {
        return admissible_domain.interior_upper_bound();
    }

    if admissible_domain.lower_theta_exclusive >= 0.0 {
        return admissible_domain.interior_lower_bound();
    }

    None
}

fn locate_nearest_valid_reference_evaluation(
    search_domain: LocalTiltSearchDomain,
    reference_theta: f64,
    evaluate_residual: &mut impl FnMut(
        f64,
    ) -> Result<
        LocalTiltResidualEvaluation,
        AnchoredLocalTiltSolveError,
    >,
) -> Result<Option<(f64, LocalTiltResidualEvaluation)>, AnchoredLocalTiltSolveError> {
    let left_candidate = if search_domain.lower_theta < reference_theta {
        locate_nearest_valid_reference_evaluation_on_side(
            reference_theta,
            search_domain.lower_theta,
            evaluate_residual,
        )?
    } else {
        None
    };

    let right_candidate = if search_domain.upper_theta > reference_theta {
        locate_nearest_valid_reference_evaluation_on_side(
            reference_theta,
            search_domain.upper_theta,
            evaluate_residual,
        )?
    } else {
        None
    };

    Ok(select_nearest_reference_candidate(
        reference_theta,
        left_candidate,
        right_candidate,
    ))
}

fn locate_nearest_valid_reference_evaluation_on_side(
    reference_theta: f64,
    side_boundary_theta: f64,
    evaluate_residual: &mut impl FnMut(
        f64,
    ) -> Result<
        LocalTiltResidualEvaluation,
        AnchoredLocalTiltSolveError,
    >,
) -> Result<Option<(f64, LocalTiltResidualEvaluation)>, AnchoredLocalTiltSolveError> {
    let Some((valid_theta, valid_evaluation)) =
        locate_valid_side_evaluation(reference_theta, side_boundary_theta, evaluate_residual)?
    else {
        return Ok(None);
    };

    refine_nearest_valid_reference_evaluation(
        reference_theta,
        valid_theta,
        valid_evaluation,
        evaluate_residual,
    )
}

fn refine_nearest_valid_reference_evaluation(
    reference_theta: f64,
    mut valid_theta: f64,
    mut valid_evaluation: LocalTiltResidualEvaluation,
    evaluate_residual: &mut impl FnMut(
        f64,
    ) -> Result<
        LocalTiltResidualEvaluation,
        AnchoredLocalTiltSolveError,
    >,
) -> Result<Option<(f64, LocalTiltResidualEvaluation)>, AnchoredLocalTiltSolveError> {
    let mut invalid_theta = reference_theta;

    loop {
        let midpoint = (invalid_theta + valid_theta) * 0.5;
        if midpoint == invalid_theta || midpoint == valid_theta {
            return Ok(Some((valid_theta, valid_evaluation)));
        }

        match evaluate_residual(midpoint) {
            Ok(midpoint_evaluation) => {
                valid_theta = midpoint;
                valid_evaluation = midpoint_evaluation;
            }
            Err(error) if should_retry_local_tilt_side_evaluation(&error) => {
                invalid_theta = midpoint;
            }
            Err(error) => return Err(error),
        }
    }
}

fn select_nearest_reference_candidate(
    reference_theta: f64,
    left_candidate: Option<(f64, LocalTiltResidualEvaluation)>,
    right_candidate: Option<(f64, LocalTiltResidualEvaluation)>,
) -> Option<(f64, LocalTiltResidualEvaluation)> {
    match (left_candidate, right_candidate) {
        (Some(left_candidate), Some(right_candidate)) => {
            let left_distance = (left_candidate.0 - reference_theta).abs();
            let right_distance = (right_candidate.0 - reference_theta).abs();

            if left_distance <= right_distance {
                Some(left_candidate)
            } else {
                Some(right_candidate)
            }
        }
        (Some(candidate), None) | (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

fn locate_nearest_root_on_side(
    reference_theta: f64,
    reference_evaluation: LocalTiltResidualEvaluation,
    side_boundary_theta: f64,
    initial_theta: Option<f64>,
    tolerance: f64,
    evaluate_residual: &mut impl FnMut(
        f64,
    ) -> Result<
        LocalTiltResidualEvaluation,
        AnchoredLocalTiltSolveError,
    >,
) -> Result<Option<(f64, LocalTiltResidualEvaluation)>, AnchoredLocalTiltSolveError> {
    let Some(bracket) = locate_side_sign_change_from_interior(
        reference_theta,
        reference_evaluation,
        side_boundary_theta,
        initial_theta,
        tolerance,
        evaluate_residual,
    )?
    else {
        return Ok(None);
    };

    if bracket.opposite_evaluation.residual.abs() <= tolerance {
        return Ok(Some((bracket.opposite_theta, bracket.opposite_evaluation)));
    }

    bisect_local_tilt_root_between_sign_change(
        reference_theta,
        bracket,
        reference_evaluation,
        tolerance,
        evaluate_residual,
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LocalTiltSideBracket {
    same_sign_theta: f64,
    same_sign_evaluation: LocalTiltResidualEvaluation,
    opposite_theta: f64,
    opposite_evaluation: LocalTiltResidualEvaluation,
}

fn locate_side_sign_change_from_interior(
    reference_theta: f64,
    reference_evaluation: LocalTiltResidualEvaluation,
    side_boundary_theta: f64,
    initial_theta: Option<f64>,
    tolerance: f64,
    evaluate_residual: &mut impl FnMut(
        f64,
    ) -> Result<
        LocalTiltResidualEvaluation,
        AnchoredLocalTiltSolveError,
    >,
) -> Result<Option<LocalTiltSideBracket>, AnchoredLocalTiltSolveError> {
    let mut same_sign_theta = reference_theta;
    let mut same_sign_evaluation = reference_evaluation;
    let mut next_candidate_theta = initial_theta;

    loop {
        let candidate_boundary_theta =
            next_candidate_theta.unwrap_or((same_sign_theta + side_boundary_theta) * 0.5);

        next_candidate_theta = None;

        if candidate_boundary_theta == same_sign_theta
            || candidate_boundary_theta == side_boundary_theta
        {
            return Ok(None);
        }

        let Some((candidate_theta, candidate_evaluation)) = locate_valid_side_evaluation(
            same_sign_theta,
            candidate_boundary_theta,
            evaluate_residual,
        )?
        else {
            return Ok(None);
        };

        if candidate_evaluation.residual.abs() <= tolerance
            || candidate_evaluation.residual.signum() != reference_evaluation.residual.signum()
        {
            return Ok(Some(LocalTiltSideBracket {
                same_sign_theta,
                same_sign_evaluation,
                opposite_theta: candidate_theta,
                opposite_evaluation: candidate_evaluation,
            }));
        }

        same_sign_theta = candidate_theta;
        same_sign_evaluation = candidate_evaluation;
    }
}

fn bisect_local_tilt_root_between_sign_change(
    reference_theta: f64,
    bracket: LocalTiltSideBracket,
    reference_evaluation: LocalTiltResidualEvaluation,
    tolerance: f64,
    evaluate_residual: &mut impl FnMut(
        f64,
    ) -> Result<
        LocalTiltResidualEvaluation,
        AnchoredLocalTiltSolveError,
    >,
) -> Result<Option<(f64, LocalTiltResidualEvaluation)>, AnchoredLocalTiltSolveError> {
    let mut same_sign_theta = bracket.same_sign_theta;
    let mut opposite_theta = bracket.opposite_theta;

    let mut best_theta = bracket.same_sign_theta;
    let mut best_evaluation = bracket.same_sign_evaluation;

    if reference_evaluation.residual.abs() < best_evaluation.residual.abs() {
        best_theta = reference_theta;
        best_evaluation = reference_evaluation;
    }

    if bracket.opposite_evaluation.residual.abs() < best_evaluation.residual.abs() {
        best_theta = bracket.opposite_theta;
        best_evaluation = bracket.opposite_evaluation;
    }

    loop {
        let midpoint = (same_sign_theta + opposite_theta) * 0.5;
        if midpoint == same_sign_theta || midpoint == opposite_theta {
            return Ok(Some((best_theta, best_evaluation)));
        }

        let Some((midpoint_theta, midpoint_evaluation)) =
            locate_valid_side_evaluation(same_sign_theta, midpoint, evaluate_residual)?
        else {
            return Ok(Some((best_theta, best_evaluation)));
        };

        if midpoint_evaluation.residual.abs() < best_evaluation.residual.abs() {
            best_theta = midpoint_theta;
            best_evaluation = midpoint_evaluation;
        }

        if midpoint_evaluation.residual.abs() <= tolerance {
            return Ok(Some((midpoint_theta, midpoint_evaluation)));
        }

        if midpoint_evaluation.residual.signum() == reference_evaluation.residual.signum() {
            same_sign_theta = midpoint_theta;
        } else {
            opposite_theta = midpoint_theta;
        }
    }
}

fn locate_valid_side_evaluation(
    reference_theta: f64,
    side_boundary_theta: f64,
    evaluate_residual: &mut impl FnMut(
        f64,
    ) -> Result<
        LocalTiltResidualEvaluation,
        AnchoredLocalTiltSolveError,
    >,
) -> Result<Option<(f64, LocalTiltResidualEvaluation)>, AnchoredLocalTiltSolveError> {
    let mut candidate_theta = side_boundary_theta;

    loop {
        match evaluate_residual(candidate_theta) {
            Ok(evaluation) => return Ok(Some((candidate_theta, evaluation))),
            Err(error) if should_retry_local_tilt_side_evaluation(&error) => {
                let midpoint = (reference_theta + candidate_theta) * 0.5;
                if midpoint == reference_theta || midpoint == candidate_theta {
                    return Ok(None);
                }

                candidate_theta = midpoint;
            }
            Err(error) => return Err(error),
        }
    }
}

pub(crate) fn should_retry_local_tilt_side_evaluation(error: &AnchoredLocalTiltSolveError) -> bool {
    matches!(
        error,
        AnchoredLocalTiltSolveError::BuySideEsscherTransformFailure { .. }
            | AnchoredLocalTiltSolveError::SellSideEsscherTransformFailure { .. }
            | AnchoredLocalTiltSolveError::NonFiniteTransformInput { .. }
            | AnchoredLocalTiltSolveError::TransformEvaluationFailure { .. }
            | AnchoredLocalTiltSolveError::NonRealObservedLogMomentExponent { .. }
            | AnchoredLocalTiltSolveError::OutOfDomainStateRequest { .. }
    )
}

fn select_preferred_root_candidate(
    left_candidate: Option<(f64, LocalTiltResidualEvaluation)>,
    right_candidate: Option<(f64, LocalTiltResidualEvaluation)>,
) -> Option<(f64, LocalTiltResidualEvaluation)> {
    match (left_candidate, right_candidate) {
        (Some(left_candidate), Some(right_candidate)) => {
            let left_abs_theta = left_candidate.0.abs();
            let right_abs_theta = right_candidate.0.abs();

            if left_abs_theta < right_abs_theta {
                Some(left_candidate)
            } else if right_abs_theta < left_abs_theta {
                Some(right_candidate)
            } else if left_candidate.1.residual.abs() <= right_candidate.1.residual.abs() {
                Some(left_candidate)
            } else {
                Some(right_candidate)
            }
        }
        (Some(candidate), None) | (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

/// Solve the canonical state-dependent local pricing tilt for one anchored interval.
pub fn solve_anchored_local_pricing_tilt(
    driver_parameters: NigBuySellDriverParameters,
    return_decomposition: ReturnDecomposition,
    pricing_request: VanillaOptionPricingRequest,
    tolerance: f64,
) -> Result<AnchoredLocalTiltSolveDiagnostics, AnchoredLocalTiltSolveError> {
    pricing_request
        .validate()
        .map_err(|source| AnchoredLocalTiltSolveError::InvalidPricingRequest { source })?;

    return_decomposition
        .validate()
        .map_err(|source| AnchoredLocalTiltSolveError::InvalidReturnDecomposition { source })?;

    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(AnchoredLocalTiltSolveError::InvalidTolerance { value: tolerance });
    }

    let boundary_state_summary = anchored_boundary_state_summary(&pricing_request)?;
    let target_log_moment_exponent = target_log_moment_exponent(&pricing_request)?;

    let (best_theta, best_evaluation) = build_local_tilt_bracket(
        driver_parameters,
        return_decomposition,
        target_log_moment_exponent,
        tolerance,
    )?;

    Ok(AnchoredLocalTiltSolveDiagnostics {
        boundary_state_summary,
        target_log_moment_exponent,
        observed_log_moment_exponent: best_evaluation.observed_log_moment_exponent,
        theta: best_theta,
        residual: best_evaluation.residual,
        absolute_residual: best_evaluation.residual.abs(),
        tolerance,
        domain_status: AnchoredLocalTiltDomainStatus::InDomain,
        admissibility_status: AnchoredLocalTiltAdmissibilityStatus::Admissible,
        policy: PRICING_LOCAL_ANCHORED_TILT_SOLVE_POLICY.to_string(),
    })
}

/// Build the shared affine-oracle equation specification from pricing inputs.
pub(crate) fn build_pricing_equation_specification(
    pricing_request: &VanillaOptionPricingRequest,
) -> ExponentialAffineTransformEquationSpecification {
    ExponentialAffineTransformEquationSpecification::new(
        pricing_request.model_parameters.hawkes_kernel_parameters,
        pricing_request
            .model_parameters
            .news_process_configuration
            .shared_mark_law
            .gamma_severity_marks,
        pricing_request.model_parameters.long_variance_parameters,
        // The affine equation spec owns the pressure contract independently from pricing input.
        pricing_request
            .model_parameters
            .market_pressure_parameters
            .clone(),
    )
}

/// Build one-interval transform input from a validated pricing request.
///
/// This helper keeps interval-delta propagation explicit between pricing contracts
/// and the shared affine-oracle solver API.
#[allow(clippy::too_many_arguments)]
pub fn build_pricing_transform_input(
    pricing_request: &VanillaOptionPricingRequest,
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

    ExponentialAffineTransformInput::try_new(
        build_pricing_equation_specification(pricing_request),
        ExponentialAffineTransformActivityLoadings::total_activity(u_plus, u_minus),
        z_plus,
        z_minus,
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        pricing_request
            .model_parameters
            .pricing_kernel_parameters
            .pressure_signed_imbalance_exposure_risk_price,
        seasonality_factor,
        news_intensity,
        pricing_request
            .model_parameters
            .news_process_configuration
            .known_component_mark_scales,
        pricing_request
            .model_parameters
            .news_process_configuration
            .surprise_component_mark_scales,
        pricing_request.interval_delta,
        pricing_request.boundary_input.hawkes_boundary_state,
        pricing_request
            .boundary_input
            .market_pressure_boundary_state
            .clone(),
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
pub use janiform_model_kernels::return_decomposition::compose_nig_trading_time_exponent_with_edge_mode;

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
