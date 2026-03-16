use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;
pub const CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION: &str =
    "calibration.optimizer.objective_composition.v1";
pub const CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_SELECTION_POLICY: &str =
    "minimum_penalized_then_minimum_raw_then_seeded_tiebreak.v1";
pub const CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_PENALTY_POLICY: &str =
    "constraint_penalty_weight_times_total_violation_count.v1";
pub const CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_BOUNDS_POLICY: &str =
    "inclusive_named_parameter_bounds.v1";

/// Canonical weighted objective term shape shared across calibration stages.
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationObjectiveTermInput {
    pub term_id: String,
    pub weight: f64,
    pub observed_score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationObjectiveTermInputError {
    MissingTermId,
    InvalidWeight { value: f64 },
    InvalidObservedScore { value: f64 },
}

impl Display for CalibrationObjectiveTermInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTermId => write!(formatter, "objective term id must be non-empty"),
            Self::InvalidWeight { value } => write!(
                formatter,
                "objective term weight must be finite and non-negative, found {value}"
            ),
            Self::InvalidObservedScore { value } => write!(
                formatter,
                "objective term observed score must be finite, found {value}"
            ),
        }
    }
}

impl Error for CalibrationObjectiveTermInputError {}

impl CalibrationObjectiveTermInput {
    pub(crate) fn validate(&self) -> Result<(), CalibrationObjectiveTermInputError> {
        if self.term_id.trim().is_empty() {
            return Err(CalibrationObjectiveTermInputError::MissingTermId);
        }
        if !self.weight.is_finite() || self.weight < 0.0 {
            return Err(CalibrationObjectiveTermInputError::InvalidWeight { value: self.weight });
        }
        if !self.observed_score.is_finite() {
            return Err(CalibrationObjectiveTermInputError::InvalidObservedScore {
                value: self.observed_score,
            });
        }

        Ok(())
    }
}

/// One weighted objective term used by Hawkes-stage candidate evaluation.
pub type HawkesStageObjectiveTermInput = CalibrationObjectiveTermInput;
pub type HawkesStageObjectiveTermInputError = CalibrationObjectiveTermInputError;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationObjectiveTermDiagnostic {
    pub term_id: String,
    pub weight: f64,
    pub observed_score: f64,
    pub weighted_contribution: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationObjectiveCompositionReport {
    pub schema_version: &'static str,
    pub weighted_objective_value: f64,
    pub objective_terms: Vec<CalibrationObjectiveTermDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationObjectiveCompositionError {
    MissingObjectiveTerms,
    InvalidObjectiveTerm {
        term_id: String,
        source: CalibrationObjectiveTermInputError,
    },
    InvalidWeightedObjectiveValue {
        value: f64,
    },
}

impl Display for CalibrationObjectiveCompositionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingObjectiveTerms => {
                write!(
                    formatter,
                    "objective_terms must include at least one weighted term"
                )
            }
            Self::InvalidObjectiveTerm { term_id, source } => {
                write!(formatter, "objective term `{term_id}` is invalid: {source}")
            }
            Self::InvalidWeightedObjectiveValue { value } => write!(
                formatter,
                "weighted objective sum must be finite, found {value}"
            ),
        }
    }
}

impl Error for CalibrationObjectiveCompositionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidObjectiveTerm { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn compose_calibration_weighted_objective(
    objective_terms: &[CalibrationObjectiveTermInput],
) -> Result<CalibrationObjectiveCompositionReport, CalibrationObjectiveCompositionError> {
    if objective_terms.is_empty() {
        return Err(CalibrationObjectiveCompositionError::MissingObjectiveTerms);
    }

    let mut weighted_objective_value = 0.0;
    let mut diagnostics = Vec::with_capacity(objective_terms.len());

    for objective_term in objective_terms {
        objective_term.validate().map_err(|source| {
            CalibrationObjectiveCompositionError::InvalidObjectiveTerm {
                term_id: objective_term.term_id.clone(),
                source,
            }
        })?;

        let weighted_contribution = objective_term.weight * objective_term.observed_score;
        if !weighted_contribution.is_finite() {
            return Err(
                CalibrationObjectiveCompositionError::InvalidWeightedObjectiveValue {
                    value: weighted_contribution,
                },
            );
        }

        weighted_objective_value += weighted_contribution;
        if !weighted_objective_value.is_finite() {
            return Err(
                CalibrationObjectiveCompositionError::InvalidWeightedObjectiveValue {
                    value: weighted_objective_value,
                },
            );
        }

        diagnostics.push(CalibrationObjectiveTermDiagnostic {
            term_id: objective_term.term_id.clone(),
            weight: objective_term.weight,
            observed_score: objective_term.observed_score,
            weighted_contribution,
        });
    }

    Ok(CalibrationObjectiveCompositionReport {
        schema_version: CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION,
        weighted_objective_value,
        objective_terms: diagnostics,
    })
}
