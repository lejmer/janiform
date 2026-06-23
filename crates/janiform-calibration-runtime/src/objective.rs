use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_calibration_artifacts::{
    CalibrationDomainMarginDiagnostic, CalibrationLossAdmissibility, CalibrationLossEvaluation,
    CalibrationLossFamilyDiagnostic, CalibrationLossTermDiagnostic, CalibrationLossTermKind,
};
use serde::Serialize;

use crate::objective_families::{
    ObjectiveCoordinateInput, ObjectiveFamilyEvaluation, ObjectiveFamilyInput, diagonal_covariance,
    evaluate_objective_family,
};
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

#[derive(Debug, Clone)]
pub(crate) struct CalibrationLossBuilder {
    family_diagnostics: Vec<CalibrationLossFamilyDiagnostic>,
    terms: Vec<CalibrationLossTermDiagnostic>,
    option_quote_terms: Vec<OptionQuoteTerm>,
    domain_margin_diagnostics: Vec<CalibrationDomainMarginDiagnostic>,
    invalid_candidate_terms: Vec<CalibrationLossTermDiagnostic>,
}

impl CalibrationLossBuilder {
    pub(crate) fn new() -> Self {
        Self {
            family_diagnostics: Vec::new(),
            terms: Vec::new(),
            option_quote_terms: Vec::new(),
            domain_margin_diagnostics: Vec::new(),
            invalid_candidate_terms: Vec::new(),
        }
    }

    pub(crate) fn add_invalid_candidate_sentinel(&mut self, term_id: impl Into<String>) {
        self.invalid_candidate_terms
            .push(CalibrationLossTermDiagnostic {
                term_id: term_id.into(),
                family_id: "invalid".to_string(),
                term_kind: CalibrationLossTermKind::InvalidCandidate,
                observed_value: None,
                model_value: None,
                observed_standard_error: None,
                model_standard_error: None,
                uncertainty_scale: None,
                standardized_residual: None,
                covariance_diagonal: None,
                loss: f64::INFINITY,
            });
    }

    pub(crate) fn add_objective_family(&mut self, family: ObjectiveFamilyEvaluation) {
        self.add_objective_family_with_term_kind(
            family,
            CalibrationLossTermKind::DiagnosticCoordinate,
        );
    }

    pub(crate) fn add_objective_family_with_term_kind(
        &mut self,
        family: ObjectiveFamilyEvaluation,
        term_kind: CalibrationLossTermKind,
    ) {
        let (max_standardized_residual, worst_term_id) =
            family.max_marginal_standardized_residual();
        let worst_term_id = worst_term_id.map(str::to_string);
        let coordinate_count = family.coordinate_count();
        let coordinate_terms = objective_family_coordinate_terms(&family, term_kind);
        let ObjectiveFamilyEvaluation {
            family_id,
            whitened_coordinates,
            loss,
            rank,
            ..
        } = family;

        self.family_diagnostics
            .push(CalibrationLossFamilyDiagnostic {
                family_id,
                coordinate_count,
                rank,
                whitened_coordinates,
                loss,
                max_standardized_residual,
                worst_term_id,
                unidentified: rank == 0,
            });

        self.terms.extend(coordinate_terms);
    }

    pub(crate) fn add_option_quote_term(
        &mut self,
        term_id: impl Into<String>,
        observed_price: f64,
        model_price: f64,
        quote_scale: f64,
    ) {
        if quote_scale.is_finite()
            && quote_scale > 0.0
            && observed_price.is_finite()
            && model_price.is_finite()
        {
            self.option_quote_terms.push(OptionQuoteTerm {
                term_id: term_id.into(),
                observed_price,
                model_price,
                quote_scale,
            });
        }
    }

    pub(crate) fn add_domain_margin_diagnostic(
        &mut self,
        margin_id: impl Into<String>,
        margin: f64,
        target_margin: f64,
    ) {
        let standardized_violation =
            if margin.is_finite() && target_margin.is_finite() && target_margin > 0.0 {
                ((target_margin - margin) / target_margin).max(0.0)
            } else {
                f64::INFINITY
            };

        let loss = standardized_violation * standardized_violation;

        self.domain_margin_diagnostics
            .push(CalibrationDomainMarginDiagnostic {
                margin_id: margin_id.into(),
                margin,
                target_margin,
                standardized_violation,
                loss,
                admissibility_warning: standardized_violation > 0.0,
            });
    }

    pub(crate) fn build(mut self) -> CalibrationLossEvaluation {
        self.add_option_quote_family();

        let family_losses = self.family_diagnostics;

        let family_loss = mean_finite_or_zero(
            family_losses
                .iter()
                .filter(|family_diagnostic| !family_diagnostic.unidentified)
                .map(|family_diagnostic| family_diagnostic.loss),
        );

        let domain_margin_loss = mean_finite_or_zero(
            self.domain_margin_diagnostics
                .iter()
                .map(|diagnostic| diagnostic.loss),
        );

        let mut warnings = Vec::new();
        let mut max_standardized_residual = 0.0_f64;

        for term in &self.terms {
            if let Some(standardized_residual) = term.standardized_residual {
                max_standardized_residual =
                    max_standardized_residual.max(standardized_residual.abs());
            }
        }

        for diagnostic in &self.domain_margin_diagnostics {
            max_standardized_residual =
                max_standardized_residual.max(diagnostic.standardized_violation.abs());
            if diagnostic.admissibility_warning {
                warnings.push(diagnostic.margin_id.clone());
            }
        }

        let invalid_candidate_loss = if self.invalid_candidate_terms.is_empty() {
            0.0
        } else {
            f64::INFINITY
        };

        self.terms.extend(self.invalid_candidate_terms);

        let global_loss = family_loss + domain_margin_loss + invalid_candidate_loss;

        CalibrationLossEvaluation {
            global_loss,
            family_losses,
            term_diagnostics: self.terms,
            domain_margin_diagnostics: self.domain_margin_diagnostics,
            admissibility: CalibrationLossAdmissibility {
                admissible: global_loss.is_finite(),
                max_standardized_residual,
                excess_loss: invalid_candidate_loss,
                warnings,
            },
        }
    }

    fn add_option_quote_family(&mut self) {
        if self.option_quote_terms.is_empty() {
            return;
        }

        let coordinates = self
            .option_quote_terms
            .iter()
            .map(|term| ObjectiveCoordinateInput {
                coordinate_id: term.term_id.clone(),
                observed_value: term.observed_price,
                model_value: term.model_price,
            })
            .collect::<Vec<_>>();

        let variances = self
            .option_quote_terms
            .iter()
            .map(|term| term.quote_scale * term.quote_scale)
            .collect::<Vec<_>>();

        let Some(covariance_rows) = diagonal_covariance(variances.as_slice()) else {
            return;
        };

        let Some(family) = evaluate_objective_family(ObjectiveFamilyInput {
            family_id: "options".to_string(),
            coordinates,
            covariance_rows,
        }) else {
            return;
        };

        let existing_term_count = self.terms.len();
        self.add_objective_family(family);

        for term in self.terms.iter_mut().skip(existing_term_count) {
            term.term_kind = CalibrationLossTermKind::OptionQuote;
        }
    }
}

#[derive(Debug, Clone)]
struct OptionQuoteTerm {
    term_id: String,
    observed_price: f64,
    model_price: f64,
    quote_scale: f64,
}

fn objective_family_coordinate_terms(
    family: &ObjectiveFamilyEvaluation,
    term_kind: CalibrationLossTermKind,
) -> Vec<CalibrationLossTermDiagnostic> {
    family
        .coordinate_ids
        .iter()
        .enumerate()
        .map(|(coordinate_position, coordinate_id)| {
            let covariance_diagonal =
                family.covariance_rows[coordinate_position][coordinate_position];
            let standardized_residual = family.marginal_standardized_residuals[coordinate_position];
            let coordinate_loss = standardized_residual
                .map(|residual| residual * residual / family.rank.max(1) as f64)
                .unwrap_or(0.0);

            CalibrationLossTermDiagnostic {
                term_id: coordinate_id.clone(),
                family_id: family.family_id.clone(),
                term_kind,
                observed_value: Some(family.observed_values[coordinate_position]),
                model_value: Some(family.model_values[coordinate_position]),
                observed_standard_error: None,
                model_standard_error: None,
                uncertainty_scale: covariance_diagonal
                    .is_finite()
                    .then_some(covariance_diagonal.max(0.0).sqrt()),
                standardized_residual,
                covariance_diagonal: Some(covariance_diagonal),
                loss: coordinate_loss,
            }
        })
        .collect()
}

fn mean_finite_or_zero(values: impl Iterator<Item = f64>) -> f64 {
    let mut sum = 0.0;
    let mut count = 0usize;

    for value in values {
        if !value.is_finite() {
            return f64::INFINITY;
        }

        sum += value;
        count += 1;
    }

    if count == 0 { 0.0 } else { sum / count as f64 }
}

#[cfg(test)]
mod loss_tests {
    use super::CalibrationLossBuilder;
    use crate::objective_families::{
        ObjectiveCoordinateInput, ObjectiveFamilyInput, evaluate_objective_family,
    };

    #[test]
    fn loss_builder_aggregates_rank_normalized_family_means() {
        let mut builder = CalibrationLossBuilder::new();

        builder.add_objective_family(
            evaluate_objective_family(ObjectiveFamilyInput {
                family_id: "a".to_string(),
                coordinates: vec![ObjectiveCoordinateInput {
                    coordinate_id: "a.one".to_string(),
                    observed_value: 0.0,
                    model_value: 1.0,
                }],
                covariance_rows: vec![vec![1.0]],
            })
            .expect("identified family should evaluate"),
        );
        builder.add_objective_family(
            evaluate_objective_family(ObjectiveFamilyInput {
                family_id: "b".to_string(),
                coordinates: vec![ObjectiveCoordinateInput {
                    coordinate_id: "b.one".to_string(),
                    observed_value: 0.0,
                    model_value: 2.0,
                }],
                covariance_rows: vec![vec![1.0]],
            })
            .expect("identified family should evaluate"),
        );

        let evaluation = builder.build();

        assert_eq!(evaluation.family_losses.len(), 2);
        assert_eq!(evaluation.global_loss, 2.5);
        assert!(evaluation.admissibility.admissible);
    }

    #[test]
    fn loss_builder_records_domain_margin_warning() {
        let mut builder = CalibrationLossBuilder::new();

        builder.add_domain_margin_diagnostic("domain_margin.test", 0.5, 1.0);

        let evaluation = builder.build();

        assert!(evaluation.admissibility.admissible);
        assert_eq!(evaluation.global_loss, 0.25);
        assert_eq!(
            evaluation.admissibility.warnings,
            vec!["domain_margin.test"]
        );
    }

    #[test]
    fn loss_builder_keeps_invalid_candidate_sentinel_infinite() {
        let mut builder = CalibrationLossBuilder::new();

        builder.add_invalid_candidate_sentinel("invalid_candidate.test");

        let evaluation = builder.build();

        assert!(!evaluation.global_loss.is_finite());
        assert!(!evaluation.admissibility.admissible);
        assert!(evaluation.admissibility.warnings.is_empty());
    }
}
