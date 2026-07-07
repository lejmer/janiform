use janiform_calibration_artifacts::JointRefinementStageCandidateEvaluationTimingDiagnostic;

use super::super::JointRefinementCandidateOutcome;
use super::workflow::CandidateObjectiveEvaluation;

pub(super) const RESTART_SEARCH_FIDELITY_TIER_LABEL: &str = "coarse_local";
pub(super) const RESTART_FINALIZATION_FIDELITY_TIER_LABEL: &str = "local_refresh_final";

pub(super) struct CandidateEvaluationTimingDiagnosticInput<'a> {
    pub(super) candidate_id: &'a str,
    pub(super) schedule_category: &'a str,
    pub(super) fidelity_tier: &'a str,
    pub(super) candidate_evaluation_count: usize,
    pub(super) search_direction_position: Option<usize>,
    pub(super) search_direction_label: &'a str,
    pub(super) elapsed_millis: u64,
    pub(super) evaluation: Option<&'a CandidateObjectiveEvaluation>,
}

pub(super) struct CandidateFinalizationTimingDiagnosticInput<'a> {
    pub(super) schedule_category: &'a str,
    pub(super) elapsed_millis: u64,
    pub(super) outcome: &'a JointRefinementCandidateOutcome,
}

pub(super) fn candidate_evaluation_timing_diagnostic(
    input: CandidateEvaluationTimingDiagnosticInput<'_>,
) -> JointRefinementStageCandidateEvaluationTimingDiagnostic {
    let exact_likelihood_evaluator_counts = input
        .evaluation
        .and_then(|evaluation| evaluation.exact_likelihood_evaluator_counts);

    // Timing diagnostics own labels because they are retained after scheduler state is released.
    JointRefinementStageCandidateEvaluationTimingDiagnostic {
        candidate_id: input.candidate_id.to_string(),
        schedule_category: input.schedule_category.to_string(),
        fidelity_tier: input.fidelity_tier.to_string(),
        candidate_evaluation_count: input.candidate_evaluation_count,
        search_direction_position: input.search_direction_position,
        search_direction_label: input.search_direction_label.to_string(),
        elapsed_millis: input.elapsed_millis,
        objective: input.evaluation.map(|evaluation| evaluation.objective),
        admissibility_warning_count: input
            .evaluation
            .map(|evaluation| evaluation.admissibility_warning_count),
        max_standardized_residual: input
            .evaluation
            .and_then(|evaluation| evaluation.max_standardized_residual),
        exact_closed_form_buy_count: exact_likelihood_evaluator_counts
            .map(|counts| counts.closed_form_buy),
        exact_closed_form_sell_count: exact_likelihood_evaluator_counts
            .map(|counts| counts.closed_form_sell),
        exact_tilted_mixed_count: exact_likelihood_evaluator_counts
            .map(|counts| counts.tilted_mixed),
    }
}

pub(super) fn candidate_finalization_timing_diagnostic(
    input: CandidateFinalizationTimingDiagnosticInput<'_>,
) -> JointRefinementStageCandidateEvaluationTimingDiagnostic {
    let final_evaluation = &input.outcome.final_evaluation;
    let exact_likelihood_evaluator_counts = final_evaluation
        .invalid_reason
        .is_none()
        .then_some(final_evaluation.exact_likelihood_evaluator_counts);

    JointRefinementStageCandidateEvaluationTimingDiagnostic {
        candidate_id: input.outcome.candidate_id.clone(),
        schedule_category: input.schedule_category.to_string(),
        fidelity_tier: RESTART_FINALIZATION_FIDELITY_TIER_LABEL.to_string(),
        candidate_evaluation_count: input.outcome.local_evaluation_count,
        search_direction_position: None,
        search_direction_label: "finalization".to_string(),
        elapsed_millis: input.elapsed_millis,
        objective: final_evaluation
            .invalid_reason
            .is_none()
            .then_some(final_evaluation.breakdown.global_loss),
        admissibility_warning_count: final_evaluation
            .invalid_reason
            .is_none()
            .then_some(final_evaluation.breakdown.admissibility.warnings.len()),
        max_standardized_residual: final_evaluation.invalid_reason.is_none().then_some(
            final_evaluation
                .breakdown
                .admissibility
                .max_standardized_residual,
        ),
        exact_closed_form_buy_count: exact_likelihood_evaluator_counts
            .map(|counts| counts.closed_form_buy),
        exact_closed_form_sell_count: exact_likelihood_evaluator_counts
            .map(|counts| counts.closed_form_sell),
        exact_tilted_mixed_count: exact_likelihood_evaluator_counts
            .map(|counts| counts.tilted_mixed),
    }
}
