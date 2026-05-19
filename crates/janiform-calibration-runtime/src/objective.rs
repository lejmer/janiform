use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_calibration_artifacts::{
    CalibrationHardConstraintDiagnostic, CalibrationLossAdmissibility, CalibrationLossEvaluation,
    CalibrationLossFamilyDiagnostic, CalibrationLossTermDiagnostic, CalibrationLossTermKind,
    CalibrationObjectivePolicy,
};
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

#[derive(Debug, Clone)]
pub(crate) struct CalibrationLossBuilder {
    policy: CalibrationObjectivePolicy,
    terms: Vec<CalibrationLossTermDiagnostic>,
    hard_constraints: Vec<CalibrationHardConstraintDiagnostic>,
}

impl CalibrationLossBuilder {
    pub(crate) fn new(policy: CalibrationObjectivePolicy) -> Self {
        Self {
            policy,
            terms: Vec::new(),
            hard_constraints: Vec::new(),
        }
    }

    pub(crate) fn add_exact_likelihood_term(
        &mut self,
        term_id: impl Into<String>,
        family_id: impl Into<String>,
        shifted_average_negative_log_likelihood: f64,
    ) {
        let loss = if shifted_average_negative_log_likelihood.is_finite() {
            shifted_average_negative_log_likelihood
        } else {
            f64::INFINITY
        };

        self.terms.push(CalibrationLossTermDiagnostic {
            term_id: term_id.into(),
            family_id: family_id.into(),
            term_kind: CalibrationLossTermKind::ExactLikelihood,
            observed_value: None,
            model_value: Some(shifted_average_negative_log_likelihood),
            observed_standard_error: None,
            model_standard_error: None,
            uncertainty_scale: None,
            standardized_residual: None,
            loss,
            admissibility_blocker: !loss.is_finite(),
        });
    }

    pub(crate) fn add_stylized_fact_term(
        &mut self,
        term_id: impl Into<String>,
        family_id: impl Into<String>,
        observed_value: f64,
        model_value: f64,
        observed_standard_error: f64,
        model_standard_error: f64,
    ) {
        let uncertainty_scale =
            pooled_uncertainty_scale(observed_standard_error, model_standard_error);

        let standardized_residual =
            stylized_fact_standardized_residual(observed_value, model_value, uncertainty_scale);

        let absolute_standardized_residual =
            standardized_residual.map(f64::abs).unwrap_or(f64::INFINITY);

        let loss = huber_loss(
            absolute_standardized_residual,
            self.policy.stylized_fact_huber_cutoff,
        );

        self.terms.push(CalibrationLossTermDiagnostic {
            term_id: term_id.into(),
            family_id: family_id.into(),
            term_kind: CalibrationLossTermKind::StylizedFact,
            observed_value: Some(observed_value),
            model_value: Some(model_value),
            observed_standard_error: Some(observed_standard_error),
            model_standard_error: Some(model_standard_error),
            uncertainty_scale,
            standardized_residual,
            loss,
            admissibility_blocker: absolute_standardized_residual
                > self.policy.admissibility_z_threshold,
        });
    }

    pub(crate) fn add_option_quote_term(
        &mut self,
        term_id: impl Into<String>,
        observed_price: f64,
        model_price: f64,
        quote_scale: f64,
    ) {
        let standardized_residual = (quote_scale.is_finite()
            && quote_scale > 0.0
            && observed_price.is_finite()
            && model_price.is_finite())
        .then_some((model_price - observed_price) / quote_scale);

        let absolute_standardized_residual =
            standardized_residual.map(f64::abs).unwrap_or(f64::INFINITY);

        let loss = huber_loss(
            absolute_standardized_residual,
            self.policy.stylized_fact_huber_cutoff,
        );

        self.terms.push(CalibrationLossTermDiagnostic {
            term_id: term_id.into(),
            family_id: "options".to_string(),
            term_kind: CalibrationLossTermKind::OptionQuote,
            observed_value: Some(observed_price),
            model_value: Some(model_price),
            observed_standard_error: Some(quote_scale),
            model_standard_error: None,
            uncertainty_scale: Some(quote_scale),
            standardized_residual,
            loss,
            admissibility_blocker: absolute_standardized_residual
                > self.policy.admissibility_z_threshold,
        });
    }

    pub(crate) fn add_domain_margin_constraint(
        &mut self,
        constraint_id: impl Into<String>,
        margin: f64,
        target_margin: f64,
    ) {
        let standardized_violation =
            if margin.is_finite() && target_margin.is_finite() && target_margin > 0.0 {
                ((target_margin - margin) / target_margin).max(0.0)
            } else {
                f64::INFINITY
            };

        let loss = huber_loss(
            standardized_violation,
            self.policy.stylized_fact_huber_cutoff,
        );

        self.hard_constraints
            .push(CalibrationHardConstraintDiagnostic {
                constraint_id: constraint_id.into(),
                margin,
                target_margin,
                standardized_violation,
                loss,
                admissibility_blocker: standardized_violation
                    > self.policy.admissibility_z_threshold,
            });
    }

    pub(crate) fn build(self) -> CalibrationLossEvaluation {
        let family_losses = family_diagnostics(self.terms.as_slice());

        let family_loss = mean_finite_or_infinity(
            family_losses
                .iter()
                .map(|family_diagnostic| family_diagnostic.loss),
        );

        let domain_margin_loss = mean_finite_or_zero(
            self.hard_constraints
                .iter()
                .map(|constraint| constraint.loss),
        );

        let mut blockers = Vec::new();
        let mut max_standardized_residual = 0.0_f64;
        let mut excess_loss = 0.0_f64;

        for term in &self.terms {
            if let Some(standardized_residual) = term.standardized_residual {
                max_standardized_residual =
                    max_standardized_residual.max(standardized_residual.abs());
                excess_loss += admissibility_excess_loss(
                    standardized_residual.abs(),
                    self.policy.admissibility_z_threshold,
                    self.policy.stylized_fact_huber_cutoff,
                );
            } else if term.admissibility_blocker {
                excess_loss = f64::INFINITY;
            }

            if term.admissibility_blocker {
                blockers.push(term.term_id.clone());
            }
        }

        for constraint in &self.hard_constraints {
            max_standardized_residual =
                max_standardized_residual.max(constraint.standardized_violation.abs());
            excess_loss += admissibility_excess_loss(
                constraint.standardized_violation.abs(),
                self.policy.admissibility_z_threshold,
                self.policy.stylized_fact_huber_cutoff,
            );
            if constraint.admissibility_blocker {
                blockers.push(constraint.constraint_id.clone());
            }
        }

        let global_loss = family_loss + domain_margin_loss + excess_loss;

        CalibrationLossEvaluation {
            policy: self.policy,
            global_loss,
            family_losses,
            term_diagnostics: self.terms,
            hard_constraint_diagnostics: self.hard_constraints,
            admissibility: CalibrationLossAdmissibility {
                admissible: blockers.is_empty() && global_loss.is_finite(),
                max_standardized_residual,
                excess_loss,
                blockers,
            },
        }
    }
}

pub(crate) fn huber_loss(absolute_standardized_residual: f64, cutoff: f64) -> f64 {
    if !absolute_standardized_residual.is_finite() || !cutoff.is_finite() || cutoff <= 0.0 {
        return f64::INFINITY;
    }

    if absolute_standardized_residual <= cutoff {
        0.5 * absolute_standardized_residual * absolute_standardized_residual
    } else {
        cutoff * (absolute_standardized_residual - 0.5 * cutoff)
    }
}

fn admissibility_excess_loss(
    absolute_standardized_residual: f64,
    admissibility_z_threshold: f64,
    huber_cutoff: f64,
) -> f64 {
    if !absolute_standardized_residual.is_finite()
        || !admissibility_z_threshold.is_finite()
        || admissibility_z_threshold <= 0.0
    {
        return f64::INFINITY;
    }

    let excess_residual = (absolute_standardized_residual - admissibility_z_threshold).max(0.0);

    huber_loss(excess_residual, huber_cutoff)
}

fn pooled_uncertainty_scale(
    observed_standard_error: f64,
    model_standard_error: f64,
) -> Option<f64> {
    if !observed_standard_error.is_finite()
        || !model_standard_error.is_finite()
        || observed_standard_error < 0.0
        || model_standard_error < 0.0
    {
        return None;
    }

    let scale = observed_standard_error.hypot(model_standard_error);

    (scale.is_finite() && scale > 0.0).then_some(scale)
}

fn stylized_fact_standardized_residual(
    observed_value: f64,
    model_value: f64,
    uncertainty_scale: Option<f64>,
) -> Option<f64> {
    if !observed_value.is_finite() || !model_value.is_finite() {
        return None;
    }

    let error = model_value - observed_value;
    if error == 0.0 {
        return Some(0.0);
    }

    uncertainty_scale
        .filter(|scale| *scale > 0.0)
        .map(|scale| error / scale)
}

fn family_diagnostics(
    terms: &[CalibrationLossTermDiagnostic],
) -> Vec<CalibrationLossFamilyDiagnostic> {
    let mut grouped_terms = BTreeMap::<&str, Vec<&CalibrationLossTermDiagnostic>>::new();

    for term in terms {
        grouped_terms
            .entry(term.family_id.as_str())
            .or_default()
            .push(term);
    }

    grouped_terms
        .into_iter()
        .map(|(family_id, family_terms)| {
            let loss = mean_finite_or_infinity(family_terms.iter().map(|term| term.loss));
            let (max_standardized_residual, worst_term_id) =
                family_max_standardized_residual(family_terms.as_slice());

            CalibrationLossFamilyDiagnostic {
                family_id: family_id.to_string(),
                term_count: family_terms.len(),
                loss,
                max_standardized_residual,
                worst_term_id,
            }
        })
        .collect()
}

fn family_max_standardized_residual(
    terms: &[&CalibrationLossTermDiagnostic],
) -> (f64, Option<String>) {
    let mut max_standardized_residual = 0.0_f64;
    let mut worst_term_id = None;

    for term in terms {
        let standardized_residual =
            term.standardized_residual
                .map(f64::abs)
                .unwrap_or(if term.admissibility_blocker {
                    f64::INFINITY
                } else {
                    0.0
                });

        if standardized_residual > max_standardized_residual {
            max_standardized_residual = standardized_residual;
            worst_term_id = Some(term.term_id.clone());
        }
    }

    (max_standardized_residual, worst_term_id)
}

fn mean_finite_or_infinity(values: impl Iterator<Item = f64>) -> f64 {
    let mut sum = 0.0;
    let mut count = 0usize;

    for value in values {
        if !value.is_finite() {
            return f64::INFINITY;
        }

        sum += value;
        count += 1;
    }

    if count == 0 {
        f64::INFINITY
    } else {
        sum / count as f64
    }
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
    use super::{CalibrationLossBuilder, huber_loss};
    use janiform_calibration_artifacts::CalibrationObjectivePolicy;

    #[test]
    fn huber_loss_is_quadratic_inside_cutoff_and_linear_outside() {
        assert_eq!(huber_loss(2.0, 3.0), 2.0);
        assert_eq!(huber_loss(4.0, 3.0), 7.5);
    }

    #[test]
    fn loss_builder_aggregates_family_means_into_global_loss() {
        let mut builder = CalibrationLossBuilder::new(CalibrationObjectivePolicy::default());

        builder.add_stylized_fact_term("a.one", "a", 1.0, 2.0, 1.0, 0.0);
        builder.add_stylized_fact_term("a.two", "a", 1.0, 1.0, 1.0, 0.0);
        builder.add_stylized_fact_term("b.one", "b", 1.0, 3.0, 1.0, 0.0);

        let evaluation = builder.build();

        assert_eq!(evaluation.family_losses.len(), 2);
        assert_eq!(evaluation.global_loss, 1.125);
        assert!(evaluation.admissibility.admissible);
    }

    #[test]
    fn loss_builder_marks_admissibility_blockers_by_policy_threshold() {
        let policy =
            CalibrationObjectivePolicy::try_new(3.0, 2.0).expect("test policy should validate");

        let mut builder = CalibrationLossBuilder::new(policy);

        builder.add_stylized_fact_term("shape.q99", "return_shape", 1.0, 4.0, 1.0, 0.0);

        let evaluation = builder.build();

        assert!(!evaluation.admissibility.admissible);
        assert_eq!(evaluation.admissibility.excess_loss, 0.5);
        assert_eq!(evaluation.global_loss, 5.0);
        assert_eq!(evaluation.admissibility.blockers, vec!["shape.q99"]);
    }

    #[test]
    fn loss_builder_accumulates_all_admissibility_excess_losses() {
        let policy =
            CalibrationObjectivePolicy::try_new(3.0, 2.0).expect("test policy should validate");

        let mut builder = CalibrationLossBuilder::new(policy);

        builder.add_stylized_fact_term("shape.q05", "return_shape", 1.0, 4.0, 1.0, 0.0);
        builder.add_stylized_fact_term("shape.q95", "return_shape", 1.0, 4.0, 1.0, 0.0);

        let evaluation = builder.build();

        assert!(!evaluation.admissibility.admissible);
        assert_eq!(evaluation.admissibility.excess_loss, 1.0);
        assert_eq!(evaluation.global_loss, 5.5);
        assert_eq!(
            evaluation.admissibility.blockers,
            vec!["shape.q05", "shape.q95"]
        );
    }

    #[test]
    fn loss_builder_keeps_global_loss_unchanged_inside_admissibility_region() {
        let policy =
            CalibrationObjectivePolicy::try_new(3.0, 4.0).expect("test policy should validate");

        let mut builder = CalibrationLossBuilder::new(policy);

        builder.add_stylized_fact_term("shape.q99", "return_shape", 1.0, 4.0, 1.0, 0.0);

        let evaluation = builder.build();

        assert!(evaluation.admissibility.admissible);
        assert_eq!(evaluation.admissibility.excess_loss, 0.0);
        assert_eq!(evaluation.global_loss, 4.5);
    }

    #[test]
    fn loss_builder_treats_exactly_matched_unidentified_stylized_fact_as_zero_residual() {
        let mut builder = CalibrationLossBuilder::new(CalibrationObjectivePolicy::default());

        builder.add_stylized_fact_term(
            "shape.bowley_skewness",
            "return_shape",
            0.0,
            0.0,
            f64::INFINITY,
            f64::INFINITY,
        );

        let evaluation = builder.build();

        assert_eq!(evaluation.global_loss, 0.0);
        assert!(evaluation.admissibility.admissible);
        assert_eq!(
            evaluation.term_diagnostics[0].standardized_residual,
            Some(0.0)
        );
        assert_eq!(evaluation.term_diagnostics[0].loss, 0.0);
    }

    #[test]
    fn loss_builder_keeps_unmatched_unidentified_stylized_fact_invalid() {
        let mut builder = CalibrationLossBuilder::new(CalibrationObjectivePolicy::default());

        builder.add_stylized_fact_term(
            "shape.bowley_skewness",
            "return_shape",
            0.0,
            1.0,
            f64::INFINITY,
            f64::INFINITY,
        );

        let evaluation = builder.build();

        assert!(!evaluation.global_loss.is_finite());
        assert!(!evaluation.admissibility.admissible);
        assert_eq!(
            evaluation.admissibility.blockers,
            vec!["shape.bowley_skewness"]
        );
        assert_eq!(evaluation.term_diagnostics[0].standardized_residual, None);
    }
}
