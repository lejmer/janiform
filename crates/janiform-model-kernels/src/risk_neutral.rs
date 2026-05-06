//! SufficientStatistics-neutral Esscher tilt utilities shared by pricing and calibration paths.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::nig::math::{principal_branch_square_root, stable_gamma};
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

/// Exact parameter sensitivities for one physical NIG characteristic exponent evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigCharacteristicExponentParameterSensitivity {
    pub argument: ComplexScalar,
    pub exponent: ComplexScalar,
    pub alpha_derivative: ComplexScalar,
    pub beta_derivative: ComplexScalar,
    pub delta_derivative: ComplexScalar,
    pub mu_derivative: ComplexScalar,
}

/// Exact parameter sensitivities for one Esscher-transformed NIG exponent evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigEsscherTransformedExponentSensitivity {
    pub argument: ComplexScalar,
    pub shifted_argument: ComplexScalar,
    pub anchor_argument: ComplexScalar,
    pub transformed_exponent: ComplexScalar,
    pub alpha_derivative: ComplexScalar,
    pub beta_derivative: ComplexScalar,
    pub delta_derivative: ComplexScalar,
    pub mu_derivative: ComplexScalar,
    pub theta_derivative: ComplexScalar,
    pub theta: f64,
    pub policy: &'static str,
}

/// Exact real-valued sensitivities for pure-imaginary Esscher log-moment evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NigPureImaginaryEsscherLogMomentSensitivity {
    pub argument_imaginary: f64,
    pub observed_log_moment_exponent: f64,
    pub alpha_derivative: f64,
    pub beta_derivative: f64,
    pub delta_derivative: f64,
    pub mu_derivative: f64,
    pub theta_derivative: f64,
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
    InvalidPhysicalNigParameters {
        source: NigParameterError,
    },
    InvalidTiltedNigParameters {
        source: NigParameterError,
    },
    InvalidTheta {
        value: f64,
    },
    InvalidArgument {
        value: ComplexScalar,
    },
    PureImaginaryArgumentOutOfDomain {
        theta: f64,
        lower_theta_exclusive: f64,
        upper_theta_exclusive: f64,
        argument_imaginary: f64,
    },
    NonFinitePhysicalExponentAtShiftedArgument {
        value: ComplexScalar,
    },
    NonFinitePhysicalExponentAtAnchorArgument {
        value: ComplexScalar,
    },
    NonFiniteTransformedExponent {
        value: ComplexScalar,
    },
    UndefinedCharacteristicExponentSensitivity {
        context: &'static str,
        denominator: ComplexScalar,
    },
    NonFiniteCharacteristicExponentSensitivity {
        parameter: &'static str,
        value: ComplexScalar,
    },
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
            Self::UndefinedCharacteristicExponentSensitivity {
                context,
                denominator,
            } => write!(
                formatter,
                "characteristic-exponent sensitivity is undefined for {context} because the local square-root denominator is invalid: {denominator:?}"
            ),
            Self::NonFiniteCharacteristicExponentSensitivity { parameter, value } => write!(
                formatter,
                "characteristic-exponent sensitivity for {parameter} became non-finite, found {value:?}"
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

    let admissible_domain = derive_nig_esscher_tilt_domain_for_pure_imaginary_argument(
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
        return Err(
            EsscherTransformError::NonFinitePhysicalExponentAtShiftedArgument {
                value: ComplexScalar::new(shifted_exponent, 0.0),
            },
        );
    }
    if !anchor_exponent.is_finite() {
        return Err(
            EsscherTransformError::NonFinitePhysicalExponentAtAnchorArgument {
                value: ComplexScalar::new(anchor_exponent, 0.0),
            },
        );
    }

    let transformed_exponent = shifted_exponent - anchor_exponent;
    if !transformed_exponent.is_finite() {
        return Err(EsscherTransformError::NonFiniteTransformedExponent {
            value: ComplexScalar::new(transformed_exponent, 0.0),
        });
    }

    Ok(transformed_exponent)
}

/// Evaluate one physical NIG exponent together with exact parameter sensitivities.
pub fn differentiate_nig_characteristic_exponent(
    physical_parameters: NigParameters,
    argument: ComplexScalar,
) -> Result<NigCharacteristicExponentParameterSensitivity, EsscherTransformError> {
    validate_nig_parameters(physical_parameters, "physical")?;
    validate_argument(argument)?;

    let shifted_beta_argument =
        ComplexScalar::new(physical_parameters.beta - argument.imaginary, argument.real);
    let shifted_square_root_term = principal_branch_square_root(
        ComplexScalar::new(physical_parameters.alpha * physical_parameters.alpha, 0.0)
            .subtract(shifted_beta_argument.square()),
    );
    let shifted_square_root_reciprocal = ComplexScalar::new(1.0, 0.0)
        .divide(shifted_square_root_term)
        .ok_or(
            EsscherTransformError::UndefinedCharacteristicExponentSensitivity {
                context: "physical exponent",
                denominator: shifted_square_root_term,
            },
        )?;
    let gamma = stable_gamma(physical_parameters.alpha, physical_parameters.beta);
    let gamma_reciprocal = 1.0 / gamma;
    let exponent = physical_parameters.characteristic_exponent(argument);
    if !exponent.is_finite() {
        return Err(
            EsscherTransformError::NonFinitePhysicalExponentAtShiftedArgument { value: exponent },
        );
    }

    let alpha_derivative = ComplexScalar::new(
        physical_parameters.delta
            * physical_parameters.alpha
            * (gamma_reciprocal - shifted_square_root_reciprocal.real),
        -physical_parameters.delta
            * physical_parameters.alpha
            * shifted_square_root_reciprocal.imaginary,
    );
    validate_sensitivity_component("alpha", alpha_derivative)?;

    let beta_derivative = ComplexScalar::new(
        physical_parameters.delta
            * (shifted_beta_argument.real * shifted_square_root_reciprocal.real
                - shifted_beta_argument.imaginary * shifted_square_root_reciprocal.imaginary
                - physical_parameters.beta * gamma_reciprocal),
        physical_parameters.delta
            * (shifted_beta_argument.real * shifted_square_root_reciprocal.imaginary
                + shifted_beta_argument.imaginary * shifted_square_root_reciprocal.real),
    );
    validate_sensitivity_component("beta", beta_derivative)?;

    let delta_derivative = ComplexScalar::new(
        gamma - shifted_square_root_term.real,
        -shifted_square_root_term.imaginary,
    );
    validate_sensitivity_component("delta", delta_derivative)?;

    let mu_derivative = ComplexScalar::new(-argument.imaginary, argument.real);
    validate_sensitivity_component("mu", mu_derivative)?;

    Ok(NigCharacteristicExponentParameterSensitivity {
        argument,
        exponent,
        alpha_derivative,
        beta_derivative,
        delta_derivative,
        mu_derivative,
    })
}

/// Evaluate one Esscher-transformed exponent together with exact parameter sensitivities.
pub fn differentiate_nig_characteristic_exponent_under_esscher_tilt(
    input: NigEsscherTransformationInput,
) -> Result<NigEsscherTransformedExponentSensitivity, EsscherTransformError> {
    input.validate()?;

    let anchor_argument = ComplexScalar::new(0.0, input.theta);
    let shifted_argument = input.argument.plus(anchor_argument);

    let shifted_sensitivity =
        differentiate_nig_characteristic_exponent(input.physical_parameters, shifted_argument)?;
    let anchor_sensitivity =
        differentiate_nig_characteristic_exponent(input.physical_parameters, anchor_argument)?;

    let transformed_exponent = shifted_sensitivity
        .exponent
        .subtract(anchor_sensitivity.exponent);
    if !transformed_exponent.is_finite() {
        return Err(EsscherTransformError::NonFiniteTransformedExponent {
            value: transformed_exponent,
        });
    }

    let alpha_derivative = shifted_sensitivity
        .alpha_derivative
        .subtract(anchor_sensitivity.alpha_derivative);
    validate_sensitivity_component("alpha", alpha_derivative)?;

    let beta_derivative = shifted_sensitivity
        .beta_derivative
        .subtract(anchor_sensitivity.beta_derivative);
    validate_sensitivity_component("beta", beta_derivative)?;

    let delta_derivative = shifted_sensitivity
        .delta_derivative
        .subtract(anchor_sensitivity.delta_derivative);
    validate_sensitivity_component("delta", delta_derivative)?;

    let mu_derivative = shifted_sensitivity
        .mu_derivative
        .subtract(anchor_sensitivity.mu_derivative);
    validate_sensitivity_component("mu", mu_derivative)?;

    let theta_derivative = differentiate_shifted_characteristic_exponent_wrt_theta(
        input.physical_parameters,
        shifted_argument,
        "shifted Esscher exponent",
    )?
    .subtract(differentiate_shifted_characteristic_exponent_wrt_theta(
        input.physical_parameters,
        anchor_argument,
        "anchor Esscher exponent",
    )?);
    validate_sensitivity_component("theta", theta_derivative)?;

    Ok(NigEsscherTransformedExponentSensitivity {
        argument: input.argument,
        shifted_argument,
        anchor_argument,
        transformed_exponent,
        alpha_derivative,
        beta_derivative,
        delta_derivative,
        mu_derivative,
        theta_derivative,
        theta: input.theta,
        policy: ESSCHER_TRANSFORMATION_POLICY,
    })
}

/// Evaluate one pure-imaginary Esscher log moment together with exact parameter sensitivities.
pub fn differentiate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
    physical_parameters: NigParameters,
    argument_imaginary: f64,
    theta: f64,
) -> Result<NigPureImaginaryEsscherLogMomentSensitivity, EsscherTransformError> {
    let transformed_sensitivity = differentiate_nig_characteristic_exponent_under_esscher_tilt(
        NigEsscherTransformationInput::new(
            physical_parameters,
            ComplexScalar::new(0.0, argument_imaginary),
            theta,
        ),
    )?;

    Ok(NigPureImaginaryEsscherLogMomentSensitivity {
        argument_imaginary,
        observed_log_moment_exponent: transformed_sensitivity.transformed_exponent.real,
        alpha_derivative: transformed_sensitivity.alpha_derivative.real,
        beta_derivative: transformed_sensitivity.beta_derivative.real,
        delta_derivative: transformed_sensitivity.delta_derivative.real,
        mu_derivative: transformed_sensitivity.mu_derivative.real,
        theta_derivative: transformed_sensitivity.theta_derivative.real,
        theta,
        policy: ESSCHER_TRANSFORMATION_POLICY,
    })
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
    let shifted_argument = input.argument.plus(anchor_argument);

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

fn validate_sensitivity_component(
    parameter: &'static str,
    value: ComplexScalar,
) -> Result<(), EsscherTransformError> {
    if !value.is_finite() {
        return Err(
            EsscherTransformError::NonFiniteCharacteristicExponentSensitivity { parameter, value },
        );
    }

    Ok(())
}

pub(crate) fn differentiate_shifted_characteristic_exponent_wrt_theta(
    physical_parameters: NigParameters,
    shifted_argument: ComplexScalar,
    context: &'static str,
) -> Result<ComplexScalar, EsscherTransformError> {
    let shifted_beta_argument = ComplexScalar::new(
        physical_parameters.beta - shifted_argument.imaginary,
        shifted_argument.real,
    );
    let shifted_square_root_term = principal_branch_square_root(
        ComplexScalar::new(physical_parameters.alpha * physical_parameters.alpha, 0.0)
            .subtract(shifted_beta_argument.square()),
    );
    let shifted_square_root_reciprocal = ComplexScalar::new(1.0, 0.0)
        .divide(shifted_square_root_term)
        .ok_or(
            EsscherTransformError::UndefinedCharacteristicExponentSensitivity {
                context,
                denominator: shifted_square_root_term,
            },
        )?;

    let theta_derivative = ComplexScalar::new(
        -physical_parameters.mu
            - physical_parameters.delta
                * (shifted_beta_argument.real * shifted_square_root_reciprocal.real
                    - shifted_beta_argument.imaginary * shifted_square_root_reciprocal.imaginary),
        -physical_parameters.delta
            * (shifted_beta_argument.real * shifted_square_root_reciprocal.imaginary
                + shifted_beta_argument.imaginary * shifted_square_root_reciprocal.real),
    );
    validate_sensitivity_component("theta", theta_derivative)?;

    Ok(theta_derivative)
}

#[cfg(test)]
mod tests {
    use crate::nig::NigParameters;
    use crate::risk_neutral::{
        ESSCHER_MARTINGALE_CONDITION_POLICY, ESSCHER_TRANSFORMATION_POLICY,
        EsscherMartingaleConditionError, EsscherMartingaleConditionInput, EsscherTransformError,
        NigEsscherTransformationInput, derive_nig_esscher_tilt_domain_for_pure_imaginary_argument,
        differentiate_nig_characteristic_exponent,
        differentiate_nig_characteristic_exponent_under_esscher_tilt,
        differentiate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument,
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

        let expected_shifted_argument = argument.plus(ComplexScalar::new(0.0, theta));
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
        assert_eq!(
            buy_domain.upper_theta_exclusive,
            parameters.beta + parameters.alpha
        );
        assert_eq!(
            sell_domain.lower_theta_exclusive,
            parameters.beta - parameters.alpha
        );
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
    fn characteristic_exponent_parameter_sensitivities_match_central_differences() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("fixture parameters should construct");
        let argument = ComplexScalar::new(0.45, -0.35);
        let sensitivity = differentiate_nig_characteristic_exponent(parameters, argument)
            .expect("characteristic exponent sensitivities should evaluate");
        let step = 1.0e-7;

        assert_complex_close(
            sensitivity.alpha_derivative,
            central_difference_complex(
                |perturbed_alpha| {
                    NigParameters::try_new(
                        perturbed_alpha,
                        parameters.beta,
                        parameters.delta,
                        parameters.mu,
                    )
                    .expect("perturbed alpha should remain valid")
                    .characteristic_exponent(argument)
                },
                parameters.alpha,
                step,
            ),
            1.0e-6,
        );
        assert_complex_close(
            sensitivity.beta_derivative,
            central_difference_complex(
                |perturbed_beta| {
                    NigParameters::try_new(
                        parameters.alpha,
                        perturbed_beta,
                        parameters.delta,
                        parameters.mu,
                    )
                    .expect("perturbed beta should remain valid")
                    .characteristic_exponent(argument)
                },
                parameters.beta,
                step,
            ),
            1.0e-6,
        );
        assert_complex_close(
            sensitivity.delta_derivative,
            central_difference_complex(
                |perturbed_delta| {
                    NigParameters::try_new(
                        parameters.alpha,
                        parameters.beta,
                        perturbed_delta,
                        parameters.mu,
                    )
                    .expect("perturbed delta should remain valid")
                    .characteristic_exponent(argument)
                },
                parameters.delta,
                step,
            ),
            1.0e-6,
        );
        assert_complex_close(
            sensitivity.mu_derivative,
            central_difference_complex(
                |perturbed_mu| {
                    NigParameters::try_new(
                        parameters.alpha,
                        parameters.beta,
                        parameters.delta,
                        perturbed_mu,
                    )
                    .expect("perturbed mu should remain valid")
                    .characteristic_exponent(argument)
                },
                parameters.mu,
                step,
            ),
            1.0e-6,
        );
    }

    #[test]
    fn esscher_transformed_exponent_sensitivities_match_central_differences() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("fixture parameters should construct");
        let argument = ComplexScalar::new(0.45, -0.35);
        let theta = -0.15;
        let sensitivity = differentiate_nig_characteristic_exponent_under_esscher_tilt(
            NigEsscherTransformationInput::new(parameters, argument, theta),
        )
        .expect("Esscher transformed exponent sensitivities should evaluate");
        let step = 1.0e-7;

        assert_complex_close(
            sensitivity.alpha_derivative,
            central_difference_complex(
                |perturbed_alpha| {
                    transform_nig_characteristic_exponent_under_esscher_tilt(
                        NigEsscherTransformationInput::new(
                            NigParameters::try_new(
                                perturbed_alpha,
                                parameters.beta,
                                parameters.delta,
                                parameters.mu,
                            )
                            .expect("perturbed alpha should remain valid"),
                            argument,
                            theta,
                        ),
                    )
                    .expect("transformed exponent should evaluate")
                    .transformed_exponent
                },
                parameters.alpha,
                step,
            ),
            1.0e-6,
        );
        assert_complex_close(
            sensitivity.beta_derivative,
            central_difference_complex(
                |perturbed_beta| {
                    transform_nig_characteristic_exponent_under_esscher_tilt(
                        NigEsscherTransformationInput::new(
                            NigParameters::try_new(
                                parameters.alpha,
                                perturbed_beta,
                                parameters.delta,
                                parameters.mu,
                            )
                            .expect("perturbed beta should remain valid"),
                            argument,
                            theta,
                        ),
                    )
                    .expect("transformed exponent should evaluate")
                    .transformed_exponent
                },
                parameters.beta,
                step,
            ),
            1.0e-6,
        );
        assert_complex_close(
            sensitivity.delta_derivative,
            central_difference_complex(
                |perturbed_delta| {
                    transform_nig_characteristic_exponent_under_esscher_tilt(
                        NigEsscherTransformationInput::new(
                            NigParameters::try_new(
                                parameters.alpha,
                                parameters.beta,
                                perturbed_delta,
                                parameters.mu,
                            )
                            .expect("perturbed delta should remain valid"),
                            argument,
                            theta,
                        ),
                    )
                    .expect("transformed exponent should evaluate")
                    .transformed_exponent
                },
                parameters.delta,
                step,
            ),
            1.0e-6,
        );
        assert_complex_close(
            sensitivity.mu_derivative,
            central_difference_complex(
                |perturbed_mu| {
                    transform_nig_characteristic_exponent_under_esscher_tilt(
                        NigEsscherTransformationInput::new(
                            NigParameters::try_new(
                                parameters.alpha,
                                parameters.beta,
                                parameters.delta,
                                perturbed_mu,
                            )
                            .expect("perturbed mu should remain valid"),
                            argument,
                            theta,
                        ),
                    )
                    .expect("transformed exponent should evaluate")
                    .transformed_exponent
                },
                parameters.mu,
                step,
            ),
            1.0e-6,
        );
        assert_complex_close(
            sensitivity.theta_derivative,
            central_difference_complex(
                |perturbed_theta| {
                    transform_nig_characteristic_exponent_under_esscher_tilt(
                        NigEsscherTransformationInput::new(parameters, argument, perturbed_theta),
                    )
                    .expect("transformed exponent should evaluate")
                    .transformed_exponent
                },
                theta,
                step,
            ),
            1.0e-6,
        );
    }

    #[test]
    fn pure_imaginary_log_moment_sensitivities_match_central_differences() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("fixture parameters should construct");
        let argument_imaginary = -1.0;
        let theta = -0.15;
        let sensitivity =
            differentiate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
                parameters,
                argument_imaginary,
                theta,
            )
            .expect("pure-imaginary sensitivities should evaluate");
        let step = 1.0e-7;

        assert_close(
            sensitivity.alpha_derivative,
            central_difference_real(
                |perturbed_alpha| {
                    evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
                        NigParameters::try_new(
                            perturbed_alpha,
                            parameters.beta,
                            parameters.delta,
                            parameters.mu,
                        )
                        .expect("perturbed alpha should remain valid"),
                        argument_imaginary,
                        theta,
                    )
                    .expect("pure-imaginary log moment should evaluate")
                },
                parameters.alpha,
                step,
            ),
            1.0e-6,
        );
        assert_close(
            sensitivity.beta_derivative,
            central_difference_real(
                |perturbed_beta| {
                    evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
                        NigParameters::try_new(
                            parameters.alpha,
                            perturbed_beta,
                            parameters.delta,
                            parameters.mu,
                        )
                        .expect("perturbed beta should remain valid"),
                        argument_imaginary,
                        theta,
                    )
                    .expect("pure-imaginary log moment should evaluate")
                },
                parameters.beta,
                step,
            ),
            1.0e-6,
        );
        assert_close(
            sensitivity.delta_derivative,
            central_difference_real(
                |perturbed_delta| {
                    evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
                        NigParameters::try_new(
                            parameters.alpha,
                            parameters.beta,
                            perturbed_delta,
                            parameters.mu,
                        )
                        .expect("perturbed delta should remain valid"),
                        argument_imaginary,
                        theta,
                    )
                    .expect("pure-imaginary log moment should evaluate")
                },
                parameters.delta,
                step,
            ),
            1.0e-6,
        );
        assert_close(
            sensitivity.mu_derivative,
            central_difference_real(
                |perturbed_mu| {
                    evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
                        NigParameters::try_new(
                            parameters.alpha,
                            parameters.beta,
                            parameters.delta,
                            perturbed_mu,
                        )
                        .expect("perturbed mu should remain valid"),
                        argument_imaginary,
                        theta,
                    )
                    .expect("pure-imaginary log moment should evaluate")
                },
                parameters.mu,
                step,
            ),
            1.0e-6,
        );
        assert_close(
            sensitivity.theta_derivative,
            central_difference_real(
                |perturbed_theta| {
                    evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
                        parameters,
                        argument_imaginary,
                        perturbed_theta,
                    )
                    .expect("pure-imaginary log moment should evaluate")
                },
                theta,
                step,
            ),
            1.0e-6,
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

    fn assert_close(observed: f64, expected: f64, tolerance: f64) {
        let difference = (observed - expected).abs();
        assert!(
            difference <= tolerance,
            "difference {difference} exceeded tolerance {tolerance} for observed {observed:?} and expected {expected:?}"
        );
    }

    fn central_difference_complex(
        evaluator: impl Fn(f64) -> ComplexScalar,
        center: f64,
        step: f64,
    ) -> ComplexScalar {
        evaluator(center + step)
            .subtract(evaluator(center - step))
            .scale(0.5 / step)
    }

    fn central_difference_real(evaluator: impl Fn(f64) -> f64, center: f64, step: f64) -> f64 {
        (evaluator(center + step) - evaluator(center - step)) * 0.5 / step
    }
}
