use std::fmt::{Display, Formatter};

use serde::Serialize;

use super::constants::{
    NIG_PARAMETER_VALIDATION_DETERMINISM_POLICY, NIG_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION,
};

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

    pub(crate) fn from_violations(violations: Vec<NigParameterViolation>) -> Self {
        let status = if violations.is_empty() {
            NigValidationStatus::Pass
        } else {
            NigValidationStatus::Fail
        };

        Self {
            schema_version: NIG_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION,
            determinism_policy: NIG_PARAMETER_VALIDATION_DETERMINISM_POLICY,
            status,
            violations,
        }
    }
}

pub(crate) fn collect_finite_violation(
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

pub(crate) fn collect_positive_violation(
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
