//! Canonical NIG driver parameter schema and domain validation.
//!
//! This module centralizes NIG parameter constraints so simulation and pricing
//! use one validation path.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

pub const NIG_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION: &str =
    "validation.nig.parameter_report.v1";
pub const NIG_PARAMETER_VALIDATION_DETERMINISM_POLICY: &str =
    "stable_check_order_and_aggregated_failures.v1";

/// Minimal deterministic complex scalar used by transform-side APIs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComplexScalar {
    pub real: f64,
    pub imaginary: f64,
}

impl ComplexScalar {
    pub fn new(real: f64, imaginary: f64) -> Self {
        Self { real, imaginary }
    }

    pub fn conjugate(self) -> Self {
        Self::new(self.real, -self.imaginary)
    }

    pub fn square(self) -> Self {
        Self::new(
            self.real
                .mul_add(self.real, -(self.imaginary * self.imaginary)),
            2.0 * self.real * self.imaginary,
        )
    }

    pub fn norm(self) -> f64 {
        self.real.hypot(self.imaginary)
    }

    pub fn add(self, other: Self) -> Self {
        Self::new(self.real + other.real, self.imaginary + other.imaginary)
    }

    pub fn subtract(self, other: Self) -> Self {
        Self::new(self.real - other.real, self.imaginary - other.imaginary)
    }

    pub fn scale(self, value: f64) -> Self {
        Self::new(self.real * value, self.imaginary * value)
    }
}

impl Display for ComplexScalar {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "({}+{}i)", self.real, self.imaginary)
    }
}

/// Unified pass or fail status for NIG parameter validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NigValidationStatus {
    Pass,
    Fail,
}

/// Machine-readable constraint labels for NIG parameter validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NigParameterConstraint {
    Finite,
    FiniteAndStrictlyPositive,
    StrictlyGreaterThanAbsoluteComparisonField,
}

impl Display for NigParameterConstraint {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finite => write!(formatter, "finite"),
            Self::FiniteAndStrictlyPositive => {
                write!(formatter, "finite_and_strictly_positive")
            }
            Self::StrictlyGreaterThanAbsoluteComparisonField => {
                write!(formatter, "strictly_greater_than_absolute_comparison_field")
            }
        }
    }
}

/// Structured validation violation payload for one failed NIG domain check.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NigParameterViolation {
    pub field: &'static str,
    pub constraint: NigParameterConstraint,
    pub value: f64,
    pub comparison_field: Option<&'static str>,
    pub comparison_value: Option<f64>,
}

/// Full validation report for standalone NIG domain checks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NigParameterValidationReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub status: NigValidationStatus,
    pub violations: Vec<NigParameterViolation>,
}

impl NigParameterValidationReport {
    pub fn is_valid(&self) -> bool {
        self.status == NigValidationStatus::Pass
    }

    pub fn first_violation(&self) -> Option<NigParameterViolation> {
        self.violations.first().copied()
    }
}

/// Canonical NIG parameter payload used by pricing and simulation drivers.
///
/// Domain constraints:
/// - `alpha > abs(beta)`
/// - `delta > 0`
/// - all fields are finite
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NigParameters {
    pub alpha: f64,
    pub beta: f64,
    pub delta: f64,
    pub mu: f64,
}

impl NigParameters {
    /// Evaluate the NIG characteristic exponent for a complex argument.
    ///
    /// Parameter mapping follows the model notation \f$(\alpha,\beta,\delta,\mu)\f$:
    /// \f[
    /// \psi_{\mathrm{NIG}}(u)
    /// = i\mu u
    /// + \delta\left(
    /// \sqrt{\alpha^2-\beta^2}
    /// -
    /// \sqrt{\alpha^2-(\beta+i u)^2}
    /// \right).
    /// \f]
    ///
    /// The complex square root uses the principal branch with a positive
    /// imaginary value on the negative real axis to keep branch handling explicit.
    pub fn characteristic_exponent(self, argument: ComplexScalar) -> ComplexScalar {
        let beta_plus_imaginary_argument =
            ComplexScalar::new(self.beta - argument.imaginary, argument.real);

        let alpha_squared = self.alpha * self.alpha;
        let radicand =
            ComplexScalar::new(alpha_squared, 0.0).subtract(beta_plus_imaginary_argument.square());
        let square_root_term = principal_branch_square_root(radicand);

        let gamma = stable_gamma(self.alpha, self.beta);
        let location_term =
            ComplexScalar::new(-self.mu * argument.imaginary, self.mu * argument.real);
        let shape_term = ComplexScalar::new(gamma, 0.0)
            .subtract(square_root_term)
            .scale(self.delta);

        location_term.add(shape_term)
    }

    /// Run pure domain validation without constructing a parameter payload.
    pub fn validate_domain(
        alpha: f64,
        beta: f64,
        delta: f64,
        mu: f64,
    ) -> NigParameterValidationReport {
        let mut violations: Vec<NigParameterViolation> = Vec::new();

        collect_finite_violation("alpha", alpha, &mut violations);
        collect_finite_violation("beta", beta, &mut violations);
        collect_positive_violation("delta", delta, &mut violations);
        collect_finite_violation("mu", mu, &mut violations);

        if alpha.is_finite() && beta.is_finite() {
            let beta_absolute_value = beta.abs();
            if alpha <= beta_absolute_value {
                violations.push(NigParameterViolation {
                    field: "alpha",
                    constraint: NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField,
                    value: alpha,
                    comparison_field: Some("beta"),
                    comparison_value: Some(beta_absolute_value),
                });
            }
        }

        let status = if violations.is_empty() {
            NigValidationStatus::Pass
        } else {
            NigValidationStatus::Fail
        };

        NigParameterValidationReport {
            schema_version: NIG_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION,
            determinism_policy: NIG_PARAMETER_VALIDATION_DETERMINISM_POLICY,
            status,
            violations,
        }
    }

    /// Validate and construct one canonical NIG parameter payload.
    pub fn try_new(alpha: f64, beta: f64, delta: f64, mu: f64) -> Result<Self, NigParameterError> {
        let validation_report = Self::validate_domain(alpha, beta, delta, mu);
        if let Some(violation) = validation_report.first_violation() {
            return Err(NigParameterError::DomainViolation {
                field: violation.field,
                constraint: violation.constraint,
                value: violation.value,
                comparison_field: violation.comparison_field,
                comparison_value: violation.comparison_value,
            });
        }

        Ok(Self {
            alpha,
            beta,
            delta,
            mu,
        })
    }

    /// Re-run domain validation for an already-built payload.
    pub fn validate(self) -> NigParameterValidationReport {
        Self::validate_domain(self.alpha, self.beta, self.delta, self.mu)
    }
}

/// Structured validation failure for `NigParameters::try_new`.
#[derive(Debug, Clone, PartialEq)]
pub enum NigParameterError {
    DomainViolation {
        field: &'static str,
        constraint: NigParameterConstraint,
        value: f64,
        comparison_field: Option<&'static str>,
        comparison_value: Option<f64>,
    },
}

impl Display for NigParameterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DomainViolation {
                field,
                constraint,
                value,
                comparison_field,
                comparison_value,
            } => match constraint {
                NigParameterConstraint::Finite => {
                    write!(formatter, "{field} must be finite, found {value}")
                }
                NigParameterConstraint::FiniteAndStrictlyPositive => write!(
                    formatter,
                    "{field} must be finite and strictly positive, found {value}"
                ),
                NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField => {
                    let comparison_field_name = comparison_field.unwrap_or("comparison_field");
                    let comparison_absolute_value = comparison_value.unwrap_or(f64::NAN);
                    write!(
                        formatter,
                        "{field} must be strictly greater than abs({comparison_field_name}), found {field}={value} and abs({comparison_field_name})={comparison_absolute_value}"
                    )
                }
            },
        }
    }
}

impl Error for NigParameterError {}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NigParametersPayload {
    alpha: f64,
    beta: f64,
    delta: f64,
    mu: f64,
}

impl<'de> Deserialize<'de> for NigParameters {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = NigParametersPayload::deserialize(deserializer)?;
        Self::try_new(payload.alpha, payload.beta, payload.delta, payload.mu)
            .map_err(serde::de::Error::custom)
    }
}

fn stable_gamma(alpha: f64, beta: f64) -> f64 {
    ((alpha - beta.abs()) * (alpha + beta.abs())).sqrt()
}

fn principal_branch_square_root(value: ComplexScalar) -> ComplexScalar {
    if value.imaginary == 0.0 {
        if value.real >= 0.0 {
            return ComplexScalar::new(value.real.sqrt(), 0.0);
        }

        return ComplexScalar::new(0.0, (-value.real).sqrt());
    }

    let magnitude = value.real.hypot(value.imaginary);
    let half_sum = (0.5 * (magnitude + value.real)).max(0.0);
    let half_difference = (0.5 * (magnitude - value.real)).max(0.0);

    let real_component = half_sum.sqrt();
    let imaginary_magnitude = half_difference.sqrt();
    let imaginary_component = if value.imaginary.is_sign_negative() {
        -imaginary_magnitude
    } else {
        imaginary_magnitude
    };

    ComplexScalar::new(real_component, imaginary_component)
}

fn collect_finite_violation(
    field: &'static str,
    value: f64,
    violations: &mut Vec<NigParameterViolation>,
) {
    if !value.is_finite() {
        violations.push(NigParameterViolation {
            field,
            constraint: NigParameterConstraint::Finite,
            value,
            comparison_field: None,
            comparison_value: None,
        });
    }
}

fn collect_positive_violation(
    field: &'static str,
    value: f64,
    violations: &mut Vec<NigParameterViolation>,
) {
    if !value.is_finite() || value <= 0.0 {
        violations.push(NigParameterViolation {
            field,
            constraint: NigParameterConstraint::FiniteAndStrictlyPositive,
            value,
            comparison_field: None,
            comparison_value: Some(0.0),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ComplexScalar, NigParameterConstraint, NigParameterError, NigParameters,
        NigValidationStatus,
    };

    #[test]
    fn valid_interior_parameter_payload_constructs_and_validates() {
        let parameters = NigParameters::try_new(2.5, -0.9, 0.35, -0.05)
            .expect("interior NIG parameters must construct successfully");

        assert_eq!(parameters.alpha, 2.5);
        assert_eq!(parameters.beta, -0.9);
        assert_eq!(parameters.delta, 0.35);
        assert_eq!(parameters.mu, -0.05);

        let report = parameters.validate();
        assert_eq!(report.status, NigValidationStatus::Pass);
        assert!(report.violations.is_empty());
    }

    #[test]
    fn parameter_payload_serialization_round_trip_preserves_values() {
        let parameters = NigParameters::try_new(3.2, 0.6, 0.8, 0.01)
            .expect("valid parameters must be serializable");

        let serialized =
            serde_json::to_string(&parameters).expect("canonical payload should serialize");
        let parsed: NigParameters =
            serde_json::from_str(&serialized).expect("serialized payload should deserialize");

        assert_eq!(parsed, parameters);
    }

    #[test]
    fn boundary_and_out_of_domain_inputs_report_field_and_constraint() {
        let alpha_beta_boundary_error =
            NigParameters::try_new(1.2, -1.2, 0.5, 0.0).expect_err("alpha == abs(beta) must fail");
        assert_eq!(
            alpha_beta_boundary_error,
            NigParameterError::DomainViolation {
                field: "alpha",
                constraint: NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField,
                value: 1.2,
                comparison_field: Some("beta"),
                comparison_value: Some(1.2),
            }
        );

        let delta_boundary_error =
            NigParameters::try_new(2.0, 0.5, 0.0, 0.0).expect_err("delta == 0 must fail");
        assert_eq!(
            delta_boundary_error,
            NigParameterError::DomainViolation {
                field: "delta",
                constraint: NigParameterConstraint::FiniteAndStrictlyPositive,
                value: 0.0,
                comparison_field: None,
                comparison_value: Some(0.0),
            }
        );

        let report = NigParameters::validate_domain(f64::NAN, f64::NEG_INFINITY, -1.0, f64::NAN);
        assert_eq!(report.status, NigValidationStatus::Fail);
        assert_eq!(
            report
                .violations
                .iter()
                .map(|violation| (violation.field, violation.constraint))
                .collect::<Vec<_>>(),
            vec![
                ("alpha", NigParameterConstraint::Finite),
                ("beta", NigParameterConstraint::Finite),
                ("delta", NigParameterConstraint::FiniteAndStrictlyPositive),
                ("mu", NigParameterConstraint::Finite),
            ],
        );
    }

    #[test]
    fn deserialization_rejects_invalid_domain_payload() {
        let payload = serde_json::json!({
            "alpha": 1.0,
            "beta": -1.0,
            "delta": 0.25,
            "mu": 0.0
        });

        let error = serde_json::from_value::<NigParameters>(payload)
            .expect_err("invalid payload must fail deserialization");
        let error_message = error.to_string();
        assert!(error_message.contains("alpha"));
        assert!(error_message.contains("strictly greater than abs(beta)"));
    }

    #[test]
    fn characteristic_exponent_matches_reference_values_for_selected_complex_arguments() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("interior NIG parameters should construct");

        let reference_fixture = [
            (ComplexScalar::new(0.0, 0.0), ComplexScalar::new(0.0, 0.0)),
            (
                ComplexScalar::new(0.75, 0.0),
                ComplexScalar::new(-0.09910996139538936, -0.10776988880696167),
            ),
            (
                ComplexScalar::new(-1.25, 0.5),
                ComplexScalar::new(-0.20167135219211368, 0.3543390690583961),
            ),
            (
                ComplexScalar::new(2.0, -1.75),
                ComplexScalar::new(-0.39180925343099193, 0.6483982349995301),
            ),
            (
                ComplexScalar::new(-3.4, 1.2),
                ComplexScalar::new(-1.3912349251792828, 0.8582572631758696),
            ),
        ];

        for (argument, expected) in reference_fixture {
            let observed = parameters.characteristic_exponent(argument);
            assert_complex_close(observed, expected, 1.0e-13);
        }
    }

    #[test]
    fn characteristic_exponent_respects_conjugate_symmetry_for_real_arguments() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("interior NIG parameters should construct");

        let argument_grid = [0.2, 0.75, 2.5, 8.0];
        for argument in argument_grid {
            let positive = parameters.characteristic_exponent(ComplexScalar::new(argument, 0.0));
            let negative = parameters.characteristic_exponent(ComplexScalar::new(-argument, 0.0));
            assert_complex_close(negative, positive.conjugate(), 1.0e-13);
        }
    }

    #[test]
    fn characteristic_exponent_is_deterministic_for_repeated_calls() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("interior NIG parameters should construct");
        let argument = ComplexScalar::new(-1.7, 0.35);
        let expected = parameters.characteristic_exponent(argument);

        for _ in 0..128 {
            let observed = parameters.characteristic_exponent(argument);
            assert_eq!(observed, expected);
        }
    }

    #[test]
    fn characteristic_exponent_is_finite_for_representative_pricing_grid_arguments() {
        let parameters = NigParameters::try_new(1.000001, 1.0, 0.2, -0.03)
            .expect("near-boundary but valid parameters should construct");

        let real_argument_grid = [-150.0, -50.0, -10.0, 0.0, 10.0, 50.0, 150.0];
        let imaginary_argument_grid = [-2.0, -0.5, 0.0, 0.5, 2.0];

        for real_argument in real_argument_grid {
            for imaginary_argument in imaginary_argument_grid {
                let value = parameters
                    .characteristic_exponent(ComplexScalar::new(real_argument, imaginary_argument));
                assert!(value.real.is_finite());
                assert!(value.imaginary.is_finite());
            }
        }
    }

    fn assert_complex_close(observed: ComplexScalar, expected: ComplexScalar, tolerance: f64) {
        let difference = observed.subtract(expected).norm();
        assert!(
            difference <= tolerance,
            "difference {difference} exceeded tolerance {tolerance} for observed {observed} and expected {expected}"
        );
    }
}
