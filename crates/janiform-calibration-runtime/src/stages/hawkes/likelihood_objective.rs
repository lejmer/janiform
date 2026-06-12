use super::contracts::HawkesStageObjectiveTermDiagnostic;
use super::execution::HawkesPreparedCandidate;
use crate::objective::CalibrationObjectiveTermInput;

const SIGNED_LIKELIHOOD_TERM_ID: &str =
    "signed_hawkes_likelihood_vs_homogeneous_signed_poisson_baseline";
const SIGNED_LIKELIHOOD_TERM_WEIGHT: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct HawkesSignedLikelihoodDiagnostic {
    pub(super) hawkes_negative_log_likelihood_per_event: f64,
    pub(super) baseline_negative_log_likelihood_per_event: f64,
    pub(super) relative_negative_log_likelihood_per_event: f64,
}

pub(super) fn append_signed_likelihood_objective_term(
    prepared_candidate: &mut HawkesPreparedCandidate,
    diagnostic: HawkesSignedLikelihoodDiagnostic,
) {
    prepared_candidate
        .objective_terms
        .push(CalibrationObjectiveTermInput {
            term_id: SIGNED_LIKELIHOOD_TERM_ID.to_string(),
            weight: SIGNED_LIKELIHOOD_TERM_WEIGHT,
            observed_score: diagnostic.relative_negative_log_likelihood_per_event,
        });

    prepared_candidate
        .objective_term_diagnostics
        .push(HawkesStageObjectiveTermDiagnostic {
            term_id: SIGNED_LIKELIHOOD_TERM_ID.to_string(),
            weight: SIGNED_LIKELIHOOD_TERM_WEIGHT,
            observed_value: diagnostic.baseline_negative_log_likelihood_per_event,
            model_implied_value: diagnostic.hawkes_negative_log_likelihood_per_event,
            scale: 1.0,
            absolute_error: diagnostic.relative_negative_log_likelihood_per_event.abs(),
            normalized_error: diagnostic.relative_negative_log_likelihood_per_event,
            weighted_contribution: diagnostic.relative_negative_log_likelihood_per_event
                * SIGNED_LIKELIHOOD_TERM_WEIGHT,
        });
}
