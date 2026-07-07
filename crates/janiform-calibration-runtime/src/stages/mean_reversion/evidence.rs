use super::{
    MeanReversionGridCandidateEvaluation, MeanReversionStageCandidateEvaluation,
    candidate_supports_quasi_likelihood_model_selection,
};

pub(super) const MEAN_REVERSION_BIC_MODEL_SELECTION_POLICY: &str =
    "gaussian_quasi_likelihood_bic_mean_reversion_model_selection.v1";
const MEAN_REVERSION_BIC_SELECTION_REASON: &str =
    "mean_reversion_selected_by_gaussian_quasi_likelihood_bic.v1";

pub(super) struct MeanReversionCandidateSelection {
    pub(super) grid_index: usize,
    pub(super) candidate_index: usize,
    pub(super) model_selection_policy: &'static str,
    pub(super) stage_decision_reason: String,
}

pub(super) fn select_mean_reversion_candidate(
    grid_evaluations: &mut [MeanReversionGridCandidateEvaluation],
) -> Option<MeanReversionCandidateSelection> {
    select_best_bic_ranked_candidate(grid_evaluations)
}

fn select_best_bic_ranked_candidate(
    grid_evaluations: &[MeanReversionGridCandidateEvaluation],
) -> Option<MeanReversionCandidateSelection> {
    let mut selected_candidate: Option<MeanReversionCandidateSelection> = None;

    for (grid_index, grid_evaluation) in grid_evaluations.iter().enumerate() {
        let Some(candidate_index) = best_admissible_bic_candidate_index(grid_evaluation) else {
            continue;
        };

        let proposed_selection = MeanReversionCandidateSelection {
            grid_index,
            candidate_index,
            model_selection_policy: MEAN_REVERSION_BIC_MODEL_SELECTION_POLICY,
            stage_decision_reason: MEAN_REVERSION_BIC_SELECTION_REASON.to_string(),
        };

        selected_candidate = Some(match selected_candidate {
            Some(current_selection)
                if bic_candidate_is_better(
                    grid_evaluations,
                    proposed_selection.grid_index,
                    proposed_selection.candidate_index,
                    current_selection.grid_index,
                    current_selection.candidate_index,
                ) =>
            {
                proposed_selection
            }
            Some(current_selection) => current_selection,
            None => proposed_selection,
        });
    }

    selected_candidate
}

fn best_admissible_bic_candidate_index(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
) -> Option<usize> {
    grid_evaluation
        .evaluated_candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate_supports_quasi_likelihood_model_selection(candidate))
        .min_by(|(_, left_candidate), (_, right_candidate)| {
            left_candidate
                .information_criterion_bic
                .total_cmp(&right_candidate.information_criterion_bic)
        })
        .map(|(candidate_index, _)| candidate_index)
}

fn bic_candidate_is_better(
    grid_evaluations: &[MeanReversionGridCandidateEvaluation],
    proposed_grid_index: usize,
    proposed_candidate_index: usize,
    current_grid_index: usize,
    current_candidate_index: usize,
) -> bool {
    let Some(proposed_candidate) = evaluated_candidate_at(
        grid_evaluations,
        proposed_grid_index,
        proposed_candidate_index,
    ) else {
        return false;
    };

    let Some(current_candidate) = evaluated_candidate_at(
        grid_evaluations,
        current_grid_index,
        current_candidate_index,
    ) else {
        return true;
    };

    proposed_candidate.information_criterion_bic < current_candidate.information_criterion_bic
}

fn evaluated_candidate_at(
    grid_evaluations: &[MeanReversionGridCandidateEvaluation],
    grid_index: usize,
    candidate_index: usize,
) -> Option<&MeanReversionStageCandidateEvaluation> {
    grid_evaluations
        .get(grid_index)
        .and_then(|grid_evaluation| grid_evaluation.evaluated_candidates.get(candidate_index))
}
