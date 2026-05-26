use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

/// Default Huber cutoff for standardized stylized-fact residuals.
pub const DEFAULT_STYLIZED_FACT_HUBER_CUTOFF: f64 = 3.0;

/// Default absolute standardized-residual threshold used for admissibility.
pub const DEFAULT_STYLIZED_FACT_ADMISSIBILITY_Z: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CalibrationObjectivePolicy {
    pub stylized_fact_huber_cutoff: f64,
    pub admissibility_z_threshold: f64,
}

impl CalibrationObjectivePolicy {
    pub fn try_new(
        stylized_fact_huber_cutoff: f64,
        admissibility_z_threshold: f64,
    ) -> Result<Self, CalibrationObjectivePolicyError> {
        let policy = Self {
            stylized_fact_huber_cutoff,
            admissibility_z_threshold,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), CalibrationObjectivePolicyError> {
        validate_strictly_positive_finite(
            "stylized_fact_huber_cutoff",
            self.stylized_fact_huber_cutoff,
        )?;
        validate_strictly_positive_finite(
            "admissibility_z_threshold",
            self.admissibility_z_threshold,
        )?;

        Ok(())
    }
}

impl Default for CalibrationObjectivePolicy {
    fn default() -> Self {
        Self {
            stylized_fact_huber_cutoff: DEFAULT_STYLIZED_FACT_HUBER_CUTOFF,
            admissibility_z_threshold: DEFAULT_STYLIZED_FACT_ADMISSIBILITY_Z,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationObjectivePolicyError {
    InvalidStrictlyPositiveFiniteField {
        field_name: &'static str,
        value: f64,
    },
}

impl Display for CalibrationObjectivePolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStrictlyPositiveFiniteField { field_name, value } => write!(
                formatter,
                "{field_name} must be finite and strictly positive, found {value}"
            ),
        }
    }
}

impl Error for CalibrationObjectivePolicyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationLossTermKind {
    ExactLikelihood,
    StylizedFact,
    DomainMargin,
    OptionQuote,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationLossTermDiagnostic {
    pub term_id: String,
    pub family_id: String,
    pub term_kind: CalibrationLossTermKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_standard_error: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_standard_error: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uncertainty_scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standardized_residual: Option<f64>,
    pub loss: f64,
    pub admissibility_blocker: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationLossFamilyDiagnostic {
    pub family_id: String,
    pub term_count: usize,
    pub loss: f64,
    pub max_standardized_residual: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worst_term_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationHardConstraintDiagnostic {
    pub constraint_id: String,
    pub margin: f64,
    pub target_margin: f64,
    pub standardized_violation: f64,
    pub loss: f64,
    pub admissibility_blocker: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationLossAdmissibility {
    pub admissible: bool,
    pub max_standardized_residual: f64,
    pub excess_loss: f64,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationLossEvaluation {
    pub policy: CalibrationObjectivePolicy,
    pub global_loss: f64,
    pub family_losses: Vec<CalibrationLossFamilyDiagnostic>,
    pub term_diagnostics: Vec<CalibrationLossTermDiagnostic>,
    pub hard_constraint_diagnostics: Vec<CalibrationHardConstraintDiagnostic>,
    pub admissibility: CalibrationLossAdmissibility,
}

fn validate_strictly_positive_finite(
    field_name: &'static str,
    value: f64,
) -> Result<(), CalibrationObjectivePolicyError> {
    if value.is_finite() && value > 0.0 {
        return Ok(());
    }

    Err(CalibrationObjectivePolicyError::InvalidStrictlyPositiveFiniteField { field_name, value })
}

#[cfg(test)]
mod tests {
    use super::{
        CalibrationObjectivePolicy, DEFAULT_STYLIZED_FACT_ADMISSIBILITY_Z,
        DEFAULT_STYLIZED_FACT_HUBER_CUTOFF,
    };

    #[test]
    fn default_objective_policy_uses_named_contract_constants() {
        assert_eq!(
            CalibrationObjectivePolicy::default(),
            CalibrationObjectivePolicy {
                stylized_fact_huber_cutoff: DEFAULT_STYLIZED_FACT_HUBER_CUTOFF,
                admissibility_z_threshold: DEFAULT_STYLIZED_FACT_ADMISSIBILITY_Z,
            }
        );
    }

    #[test]
    fn objective_policy_rejects_invalid_controls() {
        assert!(CalibrationObjectivePolicy::try_new(0.0, 3.0).is_err());
        assert!(CalibrationObjectivePolicy::try_new(3.0, f64::INFINITY).is_err());
    }
}
