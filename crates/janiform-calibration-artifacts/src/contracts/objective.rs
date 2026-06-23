use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationLossTermKind {
    ExactLikelihood,
    DiagnosticCoordinate,
    DomainMargin,
    OptionQuote,
    InvalidCandidate,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub covariance_diagonal: Option<f64>,
    pub loss: f64,
    pub admissibility_blocker: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationLossFamilyDiagnostic {
    pub family_id: String,
    pub coordinate_count: usize,
    pub rank: usize,
    pub whitened_coordinates: Vec<f64>,
    pub loss: f64,
    pub max_standardized_residual: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worst_term_id: Option<String>,
    pub unidentified: bool,
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
    pub global_loss: f64,
    pub family_losses: Vec<CalibrationLossFamilyDiagnostic>,
    pub term_diagnostics: Vec<CalibrationLossTermDiagnostic>,
    pub hard_constraint_diagnostics: Vec<CalibrationHardConstraintDiagnostic>,
    pub admissibility: CalibrationLossAdmissibility,
}
