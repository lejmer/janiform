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
    use super::{NigParameterConstraint, NigParameterError, NigParameters, NigValidationStatus};

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
}
