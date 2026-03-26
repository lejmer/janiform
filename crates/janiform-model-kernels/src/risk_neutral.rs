//! Risk-neutral Esscher tilt utilities shared by pricing and calibration paths.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::nig::{NigParameterError, NigParameters};
use janiform_foundations::complex_math::ComplexScalar;

pub const ESSCHER_TRANSFORMATION_POLICY: &str =
    "psi_q_of_u_equals_psi_p_of_u_plus_i_theta_minus_psi_p_of_i_theta.v1";
pub const ESSCHER_MARTINGALE_CONDITION_POLICY: &str =
    "observed_log_moment_exponent_minus_target_must_be_within_absolute_tolerance.v1";

/// Typed input payload for one Esscher-transformed NIG exponent evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigEsscherTransformationInput {
    pub physical_parameters: NigParameters,
    pub argument: ComplexScalar,
    pub theta: f64,
}

impl NigEsscherTransformationInput {
    pub const fn new(
        physical_parameters: NigParameters,
        argument: ComplexScalar,
        theta: f64,
    ) -> Self {
        Self {
            physical_parameters,
            argument,
            theta,
        }
    }

    pub fn validate(self) -> Result<(), EsscherTransformError> {
        validate_nig_parameters(self.physical_parameters, "physical")?;
        validate_theta(self.theta)?;
        validate_argument(self.argument)?;

        let tilted_beta = self.physical_parameters.beta - self.theta;
        validate_nig_parameters(
            NigParameters {
                alpha: self.physical_parameters.alpha,
                beta: tilted_beta,
                delta: self.physical_parameters.delta,
                mu: self.physical_parameters.mu,
            },
            "tilted",
        )?;

        Ok(())
    }
}

/// Full deterministic output payload for one Esscher-transformed exponent evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigEsscherTransformedExponent {
    pub argument: ComplexScalar,
    pub shifted_argument: ComplexScalar,
    pub anchor_argument: ComplexScalar,
    pub physical_exponent_at_shifted_argument: ComplexScalar,
    pub physical_exponent_at_anchor_argument: ComplexScalar,
    pub transformed_exponent: ComplexScalar,
    pub theta: f64,
    pub policy: &'static str,
}

/// Exact open theta-domain for one Esscher transform evaluated at `u = i * y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigEsscherTiltDomain {
    pub lower_theta_exclusive: f64,
    pub upper_theta_exclusive: f64,
}

impl NigEsscherTiltDomain {
    pub const fn new(lower_theta_exclusive: f64, upper_theta_exclusive: f64) -> Self {
        Self {
            lower_theta_exclusive,
            upper_theta_exclusive,
        }
    }

    pub fn is_empty(self) -> bool {
        !self.lower_theta_exclusive.is_finite()
            || !self.upper_theta_exclusive.is_finite()
            || self.lower_theta_exclusive >= self.upper_theta_exclusive
    }

    pub fn contains(self, theta: f64) -> bool {
        !self.is_empty()
            && theta.is_finite()
            && theta > self.lower_theta_exclusive
            && theta < self.upper_theta_exclusive
    }

    pub fn intersect(self, other: Self) -> Self {
        Self {
            lower_theta_exclusive: self.lower_theta_exclusive.max(other.lower_theta_exclusive),
            upper_theta_exclusive: self.upper_theta_exclusive.min(other.upper_theta_exclusive),
        }
    }

    pub fn interior_lower_bound(self) -> Option<f64> {
        if self.is_empty() {
            return None;
        }

        let candidate = self.lower_theta_exclusive.next_up();
        if candidate < self.upper_theta_exclusive {
            Some(candidate)
        } else {
            None
        }
    }

    pub fn interior_upper_bound(self) -> Option<f64> {
        if self.is_empty() {
            return None;
        }

        let candidate = self.upper_theta_exclusive.next_down();
        if candidate > self.lower_theta_exclusive {
            Some(candidate)
        } else {
            None
        }
    }
}

/// Structured validation and evaluation failures for Esscher-transformed exponents.
#[derive(Debug, Clone, PartialEq)]
pub enum EsscherTransformError {
    InvalidPhysicalNigParameters { source: NigParameterError },
    InvalidTiltedNigParameters { source: NigParameterError },
    InvalidTheta { value: f64 },
    InvalidArgument { value: ComplexScalar },
    PureImaginaryArgumentOutOfDomain {
        theta: f64,
        lower_theta_exclusive: f64,
        upper_theta_exclusive: f64,
        argument_imaginary: f64,
    },
    NonFinitePhysicalExponentAtShiftedArgument { value: ComplexScalar },
    NonFinitePhysicalExponentAtAnchorArgument { value: ComplexScalar },
    NonFiniteTransformedExponent { value: ComplexScalar },
}

impl Display for EsscherTransformError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPhysicalNigParameters { source } => {
                write!(formatter, "physical NIG parameters are invalid: {source}")
            }
            Self::InvalidTiltedNigParameters { source } => {
                write!(
                    formatter,
                    "Esscher-tilted NIG parameters are invalid: {source}"
                )
            }
            Self::InvalidTheta { value } => write!(
                formatter,
                "Esscher tilt theta must be finite, found {value}"
            ),
            Self::InvalidArgument { value } => write!(
                formatter,
                "Esscher transform argument must be finite, found {value:?}"
            ),
            Self::PureImaginaryArgumentOutOfDomain {
                theta,
                lower_theta_exclusive,
                upper_theta_exclusive,
                argument_imaginary,
            } => write!(
                formatter,
                "pure-imaginary Esscher evaluation is out of domain for theta={theta}, argument_imaginary={argument_imaginary}, admissible_theta_interval=({lower_theta_exclusive}, {upper_theta_exclusive})"
            ),
            Self::NonFinitePhysicalExponentAtShiftedArgument { value } => write!(
                formatter,
                "physical exponent at shifted argument became non-finite, found {value:?}"
            ),
            Self::NonFinitePhysicalExponentAtAnchorArgument { value } => write!(
                formatter,
                "physical exponent at anchor argument became non-finite, found {value:?}"
            ),
            Self::NonFiniteTransformedExponent { value } => write!(
                formatter,
                "Esscher-transformed exponent became non-finite, found {value:?}"
            ),
        }
    }
}

impl Error for EsscherTransformError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPhysicalNigParameters { source } => Some(source),
            Self::InvalidTiltedNigParameters { source } => Some(source),
            _ => None,
        }
    }
}

/// Derive the exact open theta-domain for one Esscher transform evaluated at `u = i * y`.
pub fn derive_nig_esscher_tilt_domain_for_pure_imaginary_argument(
    physical_parameters: NigParameters,
    argument_imaginary: f64,
) -> Result<NigEsscherTiltDomain, EsscherTransformError> {
    validate_nig_parameters(physical_parameters, "physical")?;
    validate_argument(ComplexScalar::new(0.0, argument_imaginary))?;

    let anchor_domain = NigEsscherTiltDomain::new(
        physical_parameters.beta - physical_parameters.alpha,
        physical_parameters.beta + physical_parameters.alpha,
    );
    let shifted_domain = NigEsscherTiltDomain::new(
        physical_parameters.beta - argument_imaginary - physical_parameters.alpha,
        physical_parameters.beta - argument_imaginary + physical_parameters.alpha,
    );

    Ok(anchor_domain.intersect(shifted_domain))
}

/// Evaluate one Esscher-transformed exponent exactly on the pure-imaginary axis.
///
/// This path is intended for real log-moment solves. For `u = i * y`, the NIG
/// exponent is real on its admissible domain, so the value can be evaluated
/// directly without introducing an imaginary-part tolerance.
pub fn evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
    physical_parameters: NigParameters,
    argument_imaginary: f64,
    theta: f64,
) -> Result<f64, EsscherTransformError> {
    validate_nig_parameters(physical_parameters, "physical")?;
    validate_theta(theta)?;
    validate_argument(ComplexScalar::new(0.0, argument_imaginary))?;

    let admissible_domain =
        derive_nig_esscher_tilt_domain_for_pure_imaginary_argument(
            physical_parameters,
            argument_imaginary,
        )?;
    if !admissible_domain.contains(theta) {
        return Err(EsscherTransformError::PureImaginaryArgumentOutOfDomain {
            theta,
            lower_theta_exclusive: admissible_domain.lower_theta_exclusive,
            upper_theta_exclusive: admissible_domain.upper_theta_exclusive,
            argument_imaginary,
        });
    }

    let anchor_lower_bound = physical_parameters.beta - physical_parameters.alpha;
    let anchor_upper_bound = physical_parameters.beta + physical_parameters.alpha;

    let shifted_lower_bound =
        physical_parameters.beta - argument_imaginary - physical_parameters.alpha;
    let shifted_upper_bound =
        physical_parameters.beta - argument_imaginary + physical_parameters.alpha;

    let anchor_square_root_term =
        ((theta - anchor_lower_bound) * (anchor_upper_bound - theta)).sqrt();
    let shifted_square_root_term =
        ((theta - shifted_lower_bound) * (shifted_upper_bound - theta)).sqrt();

    let gamma = ((physical_parameters.alpha - physical_parameters.beta.abs())
        * (physical_parameters.alpha + physical_parameters.beta.abs()))
    .sqrt();

    let shifted_exponent = (-physical_parameters.mu).mul_add(
        theta + argument_imaginary,
        physical_parameters.delta * (gamma - shifted_square_root_term),
    );
    let anchor_exponent = (-physical_parameters.mu).mul_add(
        theta,
        physical_parameters.delta * (gamma - anchor_square_root_term),
    );

    if !shifted_exponent.is_finite() {
        return Err(EsscherTransformError::NonFinitePhysicalExponentAtShiftedArgument {
            value: ComplexScalar::new(shifted_exponent, 0.0),
        });
    }
    if !anchor_exponent.is_finite() {
        return Err(EsscherTransformError::NonFinitePhysicalExponentAtAnchorArgument {
            value: ComplexScalar::new(anchor_exponent, 0.0),
        });
    }

    let transformed_exponent = shifted_exponent - anchor_exponent;
    if !transformed_exponent.is_finite() {
        return Err(EsscherTransformError::NonFiniteTransformedExponent {
            value: ComplexScalar::new(transformed_exponent, 0.0),
        });
    }

    Ok(transformed_exponent)
}

/// Deterministic input payload for one martingale-condition check.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EsscherMartingaleConditionInput {
    pub observed_log_moment_exponent: ComplexScalar,
    pub target_log_moment_exponent: f64,
    pub tolerance: f64,
}

impl EsscherMartingaleConditionInput {
    pub const fn new(
        observed_log_moment_exponent: ComplexScalar,
        target_log_moment_exponent: f64,
        tolerance: f64,
    ) -> Self {
        Self {
            observed_log_moment_exponent,
            target_log_moment_exponent,
            tolerance,
        }
    }

    pub fn validate(self) -> Result<(), EsscherMartingaleConditionError> {
        if !self.observed_log_moment_exponent.is_finite() {
            return Err(
                EsscherMartingaleConditionError::InvalidObservedLogMomentExponent {
                    value: self.observed_log_moment_exponent,
                },
            );
        }
        if !self.target_log_moment_exponent.is_finite() {
            return Err(
                EsscherMartingaleConditionError::InvalidTargetLogMomentExponent {
                    value: self.target_log_moment_exponent,
                },
            );
        }
        if !self.tolerance.is_finite() || self.tolerance <= 0.0 {
            return Err(EsscherMartingaleConditionError::InvalidTolerance {
                value: self.tolerance,
            });
        }

        Ok(())
    }
}

/// Deterministic martingale-check diagnostics for one configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EsscherMartingaleConditionReport {
    pub observed_log_moment_exponent: ComplexScalar,
    pub target_log_moment_exponent: f64,
    pub residual: ComplexScalar,
    pub residual_norm: f64,
    pub tolerance: f64,
    pub is_satisfied: bool,
    pub policy: &'static str,
}

/// Structured martingale-check validation failures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EsscherMartingaleConditionError {
    InvalidObservedLogMomentExponent { value: ComplexScalar },
    InvalidTargetLogMomentExponent { value: f64 },
    InvalidTolerance { value: f64 },
}

impl Display for EsscherMartingaleConditionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidObservedLogMomentExponent { value } => write!(
                formatter,
                "observed log-moment exponent must be finite, found {value:?}"
            ),
            Self::InvalidTargetLogMomentExponent { value } => write!(
                formatter,
                "target log-moment exponent must be finite, found {value}"
            ),
            Self::InvalidTolerance { value } => write!(
                formatter,
                "martingale tolerance must be finite and strictly positive, found {value}"
            ),
        }
    }
}

impl Error for EsscherMartingaleConditionError {}

/// Evaluate `psi_Q(u) = psi_P(u + i * theta) - psi_P(i * theta)` for one NIG driver.
pub fn transform_nig_characteristic_exponent_under_esscher_tilt(
    input: NigEsscherTransformationInput,
) -> Result<NigEsscherTransformedExponent, EsscherTransformError> {
    input.validate()?;

    let anchor_argument = ComplexScalar::new(0.0, input.theta);
    let shifted_argument = input.argument.add(anchor_argument);

    let physical_exponent_at_shifted_argument = input
        .physical_parameters
        .characteristic_exponent(shifted_argument);
    if !physical_exponent_at_shifted_argument.is_finite() {
        return Err(
            EsscherTransformError::NonFinitePhysicalExponentAtShiftedArgument {
                value: physical_exponent_at_shifted_argument,
            },
        );
    }

    let physical_exponent_at_anchor_argument = input
        .physical_parameters
        .characteristic_exponent(anchor_argument);
    if !physical_exponent_at_anchor_argument.is_finite() {
        return Err(
            EsscherTransformError::NonFinitePhysicalExponentAtAnchorArgument {
                value: physical_exponent_at_anchor_argument,
            },
        );
    }

    let transformed_exponent =
        physical_exponent_at_shifted_argument.subtract(physical_exponent_at_anchor_argument);
    if !transformed_exponent.is_finite() {
        return Err(EsscherTransformError::NonFiniteTransformedExponent {
            value: transformed_exponent,
        });
    }

    Ok(NigEsscherTransformedExponent {
        argument: input.argument,
        shifted_argument,
        anchor_argument,
        physical_exponent_at_shifted_argument,
        physical_exponent_at_anchor_argument,
        transformed_exponent,
        theta: input.theta,
        policy: ESSCHER_TRANSFORMATION_POLICY,
    })
}

/// Evaluate one martingale-condition residual against an absolute tolerance.
pub fn evaluate_esscher_martingale_condition(
    input: EsscherMartingaleConditionInput,
) -> Result<EsscherMartingaleConditionReport, EsscherMartingaleConditionError> {
    input.validate()?;

    let target = ComplexScalar::new(input.target_log_moment_exponent, 0.0);
    let residual = input.observed_log_moment_exponent.subtract(target);
    let residual_norm = residual.norm();

    Ok(EsscherMartingaleConditionReport {
        observed_log_moment_exponent: input.observed_log_moment_exponent,
        target_log_moment_exponent: input.target_log_moment_exponent,
        residual,
        residual_norm,
        tolerance: input.tolerance,
        is_satisfied: residual_norm <= input.tolerance,
        policy: ESSCHER_MARTINGALE_CONDITION_POLICY,
    })
}

fn validate_nig_parameters(
    parameters: NigParameters,
    mode: &'static str,
) -> Result<(), EsscherTransformError> {
    NigParameters::try_new(
        parameters.alpha,
        parameters.beta,
        parameters.delta,
        parameters.mu,
    )
    .map(|_| ())
    .map_err(|source| match mode {
        "tilted" => EsscherTransformError::InvalidTiltedNigParameters { source },
        _ => EsscherTransformError::InvalidPhysicalNigParameters { source },
    })
}

fn validate_theta(theta: f64) -> Result<(), EsscherTransformError> {
    if !theta.is_finite() {
        return Err(EsscherTransformError::InvalidTheta { value: theta });
    }

    Ok(())
}

fn validate_argument(argument: ComplexScalar) -> Result<(), EsscherTransformError> {
    if !argument.is_finite() {
        return Err(EsscherTransformError::InvalidArgument { value: argument });
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use crate::nig::NigParameters;
    use crate::risk_neutral::{
        ESSCHER_MARTINGALE_CONDITION_POLICY, ESSCHER_TRANSFORMATION_POLICY,
        EsscherMartingaleConditionError, EsscherMartingaleConditionInput, EsscherTransformError,
        NigEsscherTransformationInput, derive_nig_esscher_tilt_domain_for_pure_imaginary_argument,
        evaluate_esscher_martingale_condition,
        evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument,
        transform_nig_characteristic_exponent_under_esscher_tilt,
    };
    use janiform_foundations::complex_math::ComplexScalar;

    #[test]
    fn esscher_transform_matches_formula_on_controlled_fixture() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("fixture parameters should construct");
        let argument = ComplexScalar::new(0.45, -0.35);
        let theta = -0.15;

        let transformed = transform_nig_characteristic_exponent_under_esscher_tilt(
            NigEsscherTransformationInput::new(parameters, argument, theta),
        )
        .expect("Esscher transform should evaluate for a valid fixture");

        let expected_shifted_argument = argument.add(ComplexScalar::new(0.0, theta));
        let expected_anchor_argument = ComplexScalar::new(0.0, theta);
        let expected = parameters
            .characteristic_exponent(expected_shifted_argument)
            .subtract(parameters.characteristic_exponent(expected_anchor_argument));

        assert_eq!(transformed.shifted_argument, expected_shifted_argument);
        assert_eq!(transformed.anchor_argument, expected_anchor_argument);
        assert_complex_close(transformed.transformed_exponent, expected, 1.0e-13);
        assert_eq!(transformed.policy, ESSCHER_TRANSFORMATION_POLICY);
    }

    #[test]
    fn esscher_transform_rejects_invalid_theta_and_invalid_tilt_domain() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("fixture parameters should construct");

        let nan_theta_error = transform_nig_characteristic_exponent_under_esscher_tilt(
            NigEsscherTransformationInput::new(parameters, ComplexScalar::new(0.1, 0.0), f64::NAN),
        )
        .expect_err("non-finite theta must fail");
        assert!(matches!(
            nan_theta_error,
            EsscherTransformError::InvalidTheta { value } if value.is_nan()
        ));

        let tilted_domain_error = transform_nig_characteristic_exponent_under_esscher_tilt(
            NigEsscherTransformationInput::new(parameters, ComplexScalar::new(0.1, 0.0), -6.0),
        )
        .expect_err("tilt outside NIG domain must fail");
        assert!(matches!(
            tilted_domain_error,
            EsscherTransformError::InvalidTiltedNigParameters { .. }
        ));
    }

    #[test]
    fn pure_imaginary_esscher_domain_matches_exact_nig_bounds() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("fixture parameters should construct");

        let buy_domain =
            derive_nig_esscher_tilt_domain_for_pure_imaginary_argument(parameters, -1.0)
                .expect("buy-side pure-imaginary domain should derive");
        let sell_domain =
            derive_nig_esscher_tilt_domain_for_pure_imaginary_argument(parameters, 1.0)
                .expect("sell-side pure-imaginary domain should derive");

        assert_eq!(
            buy_domain.lower_theta_exclusive,
            parameters.beta - parameters.alpha + 1.0
        );
        assert_eq!(buy_domain.upper_theta_exclusive, parameters.beta + parameters.alpha);
        assert_eq!(sell_domain.lower_theta_exclusive, parameters.beta - parameters.alpha);
        assert_eq!(
            sell_domain.upper_theta_exclusive,
            parameters.beta + parameters.alpha - 1.0
        );
    }

    #[test]
    fn pure_imaginary_esscher_log_moment_matches_complex_transform_exactly() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("fixture parameters should construct");
        let theta = -0.15;

        let direct_buy_log_moment =
            evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
                parameters, -1.0, theta,
            )
            .expect("pure-imaginary buy-side log moment should evaluate");
        let direct_sell_log_moment =
            evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
                parameters, 1.0, theta,
            )
            .expect("pure-imaginary sell-side log moment should evaluate");

        let complex_buy_log_moment = transform_nig_characteristic_exponent_under_esscher_tilt(
            NigEsscherTransformationInput::new(parameters, ComplexScalar::new(0.0, -1.0), theta),
        )
        .expect("complex buy-side transform should evaluate")
        .transformed_exponent;

        let complex_sell_log_moment = transform_nig_characteristic_exponent_under_esscher_tilt(
            NigEsscherTransformationInput::new(parameters, ComplexScalar::new(0.0, 1.0), theta),
        )
        .expect("complex sell-side transform should evaluate")
        .transformed_exponent;

        assert_complex_close(
            ComplexScalar::new(direct_buy_log_moment, 0.0),
            complex_buy_log_moment,
            1.0e-13,
        );
        assert_complex_close(
            ComplexScalar::new(direct_sell_log_moment, 0.0),
            complex_sell_log_moment,
            1.0e-13,
        );
    }

    #[test]
    fn martingale_condition_report_and_validation_are_deterministic() {
        let report = evaluate_esscher_martingale_condition(EsscherMartingaleConditionInput::new(
            ComplexScalar::new(0.125, 0.0),
            0.12,
            0.01,
        ))
        .expect("valid martingale input should evaluate");

        assert_eq!(report.policy, ESSCHER_MARTINGALE_CONDITION_POLICY);
        assert!(report.is_satisfied);
        assert!(report.residual_norm <= report.tolerance);
        assert_complex_close(report.residual, ComplexScalar::new(0.005, 0.0), 1.0e-13);

        let tolerance_error = evaluate_esscher_martingale_condition(
            EsscherMartingaleConditionInput::new(ComplexScalar::new(0.0, 0.0), 0.0, 0.0),
        )
        .expect_err("zero tolerance must fail martingale validation");
        assert!(matches!(
            tolerance_error,
            EsscherMartingaleConditionError::InvalidTolerance { value } if value == 0.0
        ));
    }

    fn assert_complex_close(observed: ComplexScalar, expected: ComplexScalar, tolerance: f64) {
        let difference = observed.subtract(expected).norm();
        assert!(
            difference <= tolerance,
            "difference {difference} exceeded tolerance {tolerance} for observed {observed:?} and expected {expected:?}"
        );
    }
}
