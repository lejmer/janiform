use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformInput,
    ExponentialAffineTransformInputError, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformIntegrationStatus, ExponentialAffineTransformIntegratorMethod,
    ExponentialAffineTransformNewsIntensityInput, ExponentialAffineTransformOutput,
    ExponentialAffineTransformOutputError, ExponentialAffineTransformSolverError,
    ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_settings as solve_exponential_affine_transform_core,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_foundations::complex_math::{complex_exponential, complex_multiply};
use janiform_market_time::hourly_factors::HourlyFactorContractOutput;
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelParameters,
    HawkesSeasonalityIntervalError,
};
use janiform_model_kernels::nig::{
    NigBuySellDriverParameters, NigTradingTimeCharacteristicExponentContributions,
};
use janiform_model_kernels::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionValidationError,
};
use janiform_model_kernels::risk_neutral::{
    EsscherTransformError, NigEsscherTiltDomain,
    derive_nig_esscher_tilt_domain_for_pure_imaginary_argument,
    evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument,
};

use super::constants::{
    INTERNAL_AFFINE_TRANSFORM_STEP_SELECTION_POLICY, PRICING_LOCAL_ANCHORED_TILT_SOLVE_POLICY,
    RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
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
pub(crate) struct LocalTiltSearchDomain {
    pub(crate) lower_theta: f64,
    pub(crate) upper_theta: f64,
    pub(crate) reference_theta: f64,
}

const BUY_SIDE_OBSERVED_LOG_MOMENT_ARGUMENT_IMAGINARY: f64 = -1.0;
const SELL_SIDE_OBSERVED_LOG_MOMENT_ARGUMENT_IMAGINARY: f64 = 1.0;

/// Resolve interval-aligned hourly factors for the pricing path.
pub use janiform_market_time::hourly_factors::provide_hourly_factor_contract as load_hourly_factor_contract;

/// Project state to interval intensities for coarse-time sufficient-statistics paths.
pub fn project_hawkes_intensities_coarse_interval(
    state: HawkesExcitationState,
    parameters: HawkesKernelParameters,
    hourly_factor_contract: &HourlyFactorContractOutput,
    interval_index: usize,
) -> Result<HawkesIntensityProjection, HawkesSeasonalityIntervalError> {
    state.project_intensities_for_hourly_interval(
        parameters.lambda_bar,
        &hourly_factor_contract.s_h,
        interval_index,
    )
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
        observed_boundary_state: boundary_input.observed_boundary_state.clone(),
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
    let buy_side_log_moment_exponent =
        evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
            driver_parameters.buy_side_parameters,
            BUY_SIDE_OBSERVED_LOG_MOMENT_ARGUMENT_IMAGINARY,
            theta,
        )
        .map_err(|source| {
            map_local_tilt_axis_evaluation_error(
                theta,
                BUY_SIDE_OBSERVED_LOG_MOMENT_ARGUMENT_IMAGINARY,
                source,
                true,
            )
        })?;

    let sell_side_log_moment_exponent =
        evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
            driver_parameters.sell_side_parameters,
            SELL_SIDE_OBSERVED_LOG_MOMENT_ARGUMENT_IMAGINARY,
            theta,
        )
        .map_err(|source| {
            map_local_tilt_axis_evaluation_error(
                theta,
                SELL_SIDE_OBSERVED_LOG_MOMENT_ARGUMENT_IMAGINARY,
                source,
                false,
            )
        })?;

    let observed_log_moment_exponent = return_decomposition.a_plus.mul_add(
        buy_side_log_moment_exponent,
        return_decomposition.a_minus * sell_side_log_moment_exponent,
    );
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

fn anchored_local_tilt_domain(
    driver_parameters: NigBuySellDriverParameters,
) -> Result<NigEsscherTiltDomain, AnchoredLocalTiltSolveError> {
    let buy_side_domain = derive_nig_esscher_tilt_domain_for_pure_imaginary_argument(
        driver_parameters.buy_side_parameters,
        BUY_SIDE_OBSERVED_LOG_MOMENT_ARGUMENT_IMAGINARY,
    )
    .map_err(
        |source| AnchoredLocalTiltSolveError::BuySideEsscherTransformFailure {
            theta: f64::NAN,
            source,
        },
    )?;

    let sell_side_domain = derive_nig_esscher_tilt_domain_for_pure_imaginary_argument(
        driver_parameters.sell_side_parameters,
        SELL_SIDE_OBSERVED_LOG_MOMENT_ARGUMENT_IMAGINARY,
    )
    .map_err(
        |source| AnchoredLocalTiltSolveError::SellSideEsscherTransformFailure {
            theta: f64::NAN,
            source,
        },
    )?;

    Ok(buy_side_domain.intersect(sell_side_domain))
}

pub(crate) fn resolve_local_tilt_search_domain(
    driver_parameters: NigBuySellDriverParameters,
) -> Result<LocalTiltSearchDomain, AnchoredLocalTiltSolveError> {
    let admissible_domain = anchored_local_tilt_domain(driver_parameters)?;

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

fn map_local_tilt_axis_evaluation_error(
    theta: f64,
    argument_imaginary: f64,
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
            argument_imaginary,
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
    let search_domain =
        resolve_local_tilt_search_domain(driver_parameters).map_err(|error| match error {
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
        })?;

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
    mut evaluate_residual: F,
) -> Result<(f64, LocalTiltResidualEvaluation), AnchoredLocalTiltSolveError>
where
    F: FnMut(f64) -> Result<LocalTiltResidualEvaluation, AnchoredLocalTiltSolveError>,
{
    let lower_theta = search_domain.lower_theta;
    let upper_theta = search_domain.upper_theta;
    let reference_theta = search_domain.reference_theta;

    let reference_evaluation = evaluate_residual(reference_theta)?;
    if reference_evaluation.residual.abs() <= tolerance {
        return Ok((reference_theta, reference_evaluation));
    }

    let left_candidate = if lower_theta < reference_theta {
        locate_nearest_root_on_side(
            reference_theta,
            reference_evaluation,
            lower_theta,
            tolerance,
            &mut evaluate_residual,
        )?
    } else {
        None
    };

    let right_candidate = if upper_theta > reference_theta {
        locate_nearest_root_on_side(
            reference_theta,
            reference_evaluation,
            upper_theta,
            tolerance,
            &mut evaluate_residual,
        )?
    } else {
        None
    };

    match select_preferred_root_candidate(left_candidate, right_candidate) {
        Some(candidate) => Ok(candidate),
        None => Err(AnchoredLocalTiltSolveError::NoAdmissibleLocalTilt {
            target_log_moment_exponent,
            lower_theta,
            upper_theta,
            lower_residual: f64::NAN,
            upper_residual: f64::NAN,
        }),
    }
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

fn locate_nearest_root_on_side(
    reference_theta: f64,
    reference_evaluation: LocalTiltResidualEvaluation,
    side_boundary_theta: f64,
    tolerance: f64,
    evaluate_residual: &mut impl FnMut(
        f64,
    ) -> Result<
        LocalTiltResidualEvaluation,
        AnchoredLocalTiltSolveError,
    >,
) -> Result<Option<(f64, LocalTiltResidualEvaluation)>, AnchoredLocalTiltSolveError> {
    let Some((mut opposite_theta, mut opposite_evaluation)) =
        locate_valid_side_evaluation(reference_theta, side_boundary_theta, evaluate_residual)?
    else {
        return Ok(None);
    };

    while opposite_evaluation.residual.signum() == reference_evaluation.residual.signum() {
        let midpoint = (reference_theta + opposite_theta) * 0.5;
        if midpoint == reference_theta || midpoint == opposite_theta {
            return Ok(None);
        }

        let Some((midpoint_theta, midpoint_evaluation)) =
            locate_valid_side_evaluation(reference_theta, midpoint, evaluate_residual)?
        else {
            return Ok(None);
        };

        opposite_theta = midpoint_theta;
        opposite_evaluation = midpoint_evaluation;

        if opposite_evaluation.residual.abs() <= tolerance {
            return Ok(Some((opposite_theta, opposite_evaluation)));
        }
    }

    let mut same_sign_theta = reference_theta;

    let mut best_theta = if opposite_evaluation.residual.abs() < reference_evaluation.residual.abs()
    {
        opposite_theta
    } else {
        reference_theta
    };

    let mut best_evaluation =
        if opposite_evaluation.residual.abs() < reference_evaluation.residual.abs() {
            opposite_evaluation
        } else {
            reference_evaluation
        };

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

fn should_retry_local_tilt_side_evaluation(error: &AnchoredLocalTiltSolveError) -> bool {
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

/// Solve one-interval exponential-affine transform for pricing workflows.
pub fn solve_exponential_affine_transform(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_core(transform_input, solver_settings)
}

pub(crate) fn solve_exponential_affine_transform_with_internal_selection(
    transform_input: ExponentialAffineTransformInput,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    debug_assert!(!INTERNAL_AFFINE_TRANSFORM_STEP_SELECTION_POLICY.is_empty());

    let mut step_count = 1usize;
    let mut previous_completed_result: Option<ExponentialAffineTransformIntegrationResult> = None;
    let mut last_incomplete_result: Option<ExponentialAffineTransformIntegrationResult> = None;

    loop {
        let solver_settings = ExponentialAffineTransformSolverSettings::try_new(
            ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
            step_count,
        )
        .map_err(|source| {
            ExponentialAffineTransformSolverError::InvalidSolverSettings { source }
        })?;

        // Intentional clone. Internal step refinement reruns the same validated transform input
        // at finer dyadic resolutions until the RK4 truncation estimate reaches the roundoff scale.
        let current_result =
            solve_exponential_affine_transform_core(transform_input.clone(), solver_settings)?;

        if current_result.status != ExponentialAffineTransformIntegrationStatus::Completed {
            last_incomplete_result = Some(current_result);
        } else if let Some(previous_result) = previous_completed_result {
            if affine_transform_results_agree_with_internal_budget(previous_result, current_result)
            {
                return Ok(current_result);
            }

            previous_completed_result = Some(current_result);
        } else {
            previous_completed_result = Some(current_result);
        }

        step_count = match step_count.checked_mul(2) {
            Some(next_step_count) => next_step_count,
            None => {
                if let Some(best_completed_result) = previous_completed_result {
                    return Ok(best_completed_result);
                }
                if let Some(best_incomplete_result) = last_incomplete_result {
                    return Ok(best_incomplete_result);
                }
                return Err(ExponentialAffineTransformSolverError::InvalidOutput {
                    source: ExponentialAffineTransformOutputError::InvalidTransformValue {
                        value: ComplexScalar::new(f64::NAN, f64::NAN),
                    },
                });
            }
        };
    }
}

fn affine_transform_results_agree_with_internal_budget(
    coarse_result: ExponentialAffineTransformIntegrationResult,
    fine_result: ExponentialAffineTransformIntegrationResult,
) -> bool {
    let Some(coarse_output) = coarse_result.output else {
        return false;
    };
    let Some(fine_output) = fine_result.output else {
        return false;
    };

    let truncation_error_estimate =
        affine_transform_output_max_difference_norm(coarse_output, fine_output) / 15.0;
    let output_scale = affine_transform_output_max_norm(fine_output).max(f64::MIN_POSITIVE);
    let roundoff_budget = f64::EPSILON.cbrt() * output_scale;

    truncation_error_estimate <= roundoff_budget
}

fn affine_transform_output_max_difference_norm(
    coarse_output: ExponentialAffineTransformOutput,
    fine_output: ExponentialAffineTransformOutput,
) -> f64 {
    [
        fine_output
            .c_at_delta
            .subtract(coarse_output.c_at_delta)
            .norm(),
        fine_output
            .d_at_delta
            .plus_plus
            .subtract(coarse_output.d_at_delta.plus_plus)
            .norm(),
        fine_output
            .d_at_delta
            .plus_minus
            .subtract(coarse_output.d_at_delta.plus_minus)
            .norm(),
        fine_output
            .d_at_delta
            .minus_plus
            .subtract(coarse_output.d_at_delta.minus_plus)
            .norm(),
        fine_output
            .d_at_delta
            .minus_minus
            .subtract(coarse_output.d_at_delta.minus_minus)
            .norm(),
        fine_output
            .transform_value
            .subtract(coarse_output.transform_value)
            .norm(),
    ]
    .into_iter()
    .fold(0.0_f64, f64::max)
}

fn affine_transform_output_max_norm(output: ExponentialAffineTransformOutput) -> f64 {
    [
        output.c_at_delta.norm(),
        output.d_at_delta.plus_plus.norm(),
        output.d_at_delta.plus_minus.norm(),
        output.d_at_delta.minus_plus.norm(),
        output.d_at_delta.minus_minus.norm(),
        output.transform_value.norm(),
    ]
    .into_iter()
    .fold(0.0_f64, f64::max)
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
        u_plus,
        u_minus,
        z_plus,
        z_minus,
        seasonality_factor,
        news_intensity,
        pricing_request.interval_delta,
        pricing_request.boundary_input.hawkes_boundary_state,
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

/// Apply one coarse-time transform-path Hawkes state transition using canonical call order.
pub use janiform_model_kernels::hawkes::evolve_hawkes_state;
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
