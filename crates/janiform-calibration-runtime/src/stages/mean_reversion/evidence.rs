use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, MeanReversionStagePredictiveEvidenceDiagnostics,
};

use super::{
    MeanReversionGridCandidateEvaluation, MeanReversionStageCandidateEvaluation,
    MeanReversionStageExecutionFailure, MeanReversionStageProgressUpdate,
    RANDOM_WALK_NULL_CANDIDATE_FAMILY, candidate_supports_quasi_likelihood_model_selection,
};

mod predictive_validation;

pub(super) const PREDICTIVE_EVIDENCE_MODEL_SELECTION_POLICY: &str =
    "rolling_origin_predictive_sign_test_mean_reversion_evidence_gate.v1";
const PREDICTIVE_EVIDENCE_REFIT_SCHEDULE_POLICY: &str =
    "dyadic_expanding_window_rolling_origin_refit_schedule.v1";
const PREDICTIVE_EVIDENCE_WITH_BIC_FALLBACK_POLICY: &str = "rolling_origin_predictive_sign_test_with_gaussian_quasi_likelihood_bic_thin_sample_fallback.v1";
const POSITIVE_PREDICTIVE_EVIDENCE_REASON: &str = "positive_predictive_evidence_gate_passed.v1";
const NO_POSITIVE_PREDICTIVE_EVIDENCE_REASON: &str =
    "random_walk_null_selected_no_positive_predictive_evidence.v1";
const PREDICTIVE_EVIDENCE_THIN_SAMPLE_BIC_FALLBACK_REASON: &str =
    "predictive_evidence_support_too_thin_gaussian_quasi_likelihood_bic_fallback.v1";

const PREDICTIVE_EVIDENCE_GATE_PASSED_STATUS: &str = "predictive_evidence_passed.v1";
const PREDICTIVE_EVIDENCE_GATE_FAILED_STATUS: &str = "predictive_evidence_failed.v1";
const PREDICTIVE_EVIDENCE_GATE_TOO_THIN_STATUS: &str = "predictive_evidence_support_too_thin.v1";

pub(super) struct MeanReversionCandidateSelection {
    pub(super) grid_index: usize,
    pub(super) candidate_index: usize,
    pub(super) model_selection_policy: &'static str,
    pub(super) stage_decision_reason: String,
}

struct PredictiveEvidenceRecord {
    grid_index: usize,
    candidate_index: usize,
    diagnostics: MeanReversionStagePredictiveEvidenceDiagnostics,
}

pub(super) fn select_mean_reversion_candidate(
    grid_evaluations: &mut [MeanReversionGridCandidateEvaluation],
    solver_settings: &CalibrationOptimizerSettings,
    predictive_evidence_alpha: f64,
    progress_sender: Option<&Sender<MeanReversionStageProgressUpdate>>,
) -> Result<Option<MeanReversionCandidateSelection>, MeanReversionStageExecutionFailure> {
    let mut evidence_records = predictive_validation::predictive_evidence_records(
        grid_evaluations,
        solver_settings,
        predictive_evidence_alpha,
        progress_sender,
    )?;

    apply_holm_adjusted_p_values(evidence_records.as_mut_slice());
    finalize_predictive_gate_statuses(evidence_records.as_mut_slice(), predictive_evidence_alpha);

    let predictive_candidate_was_tested = !evidence_records.is_empty();

    let predictive_evidence_is_supported = evidence_records
        .iter()
        .any(predictive_evidence_has_test_support);

    assign_predictive_evidence(grid_evaluations, evidence_records);

    if let Some(selection) = select_passing_predictive_candidate(grid_evaluations) {
        return Ok(Some(selection));
    }

    if predictive_candidate_was_tested && !predictive_evidence_is_supported {
        return Ok(select_best_bic_fallback_candidate(grid_evaluations));
    }

    Ok(select_best_random_walk_null_candidate(grid_evaluations))
}

fn apply_holm_adjusted_p_values(evidence_records: &mut [PredictiveEvidenceRecord]) {
    let mut ordered_indices = evidence_records
        .iter()
        .enumerate()
        .filter_map(|(record_index, evidence_record)| {
            evidence_record
                .diagnostics
                .raw_sign_test_p_value
                .map(|raw_p_value| (record_index, raw_p_value))
        })
        .collect::<Vec<_>>();

    ordered_indices.sort_by(|left, right| left.1.total_cmp(&right.1));

    let tested_count = ordered_indices.len();

    let mut running_adjusted_p_value = 0.0_f64;

    for (ordered_position, (record_index, raw_p_value)) in ordered_indices.into_iter().enumerate() {
        let multiplier = tested_count.saturating_sub(ordered_position) as f64;

        let adjusted_p_value = (multiplier * raw_p_value)
            .min(1.0)
            .max(running_adjusted_p_value);

        running_adjusted_p_value = adjusted_p_value;

        if let Some(evidence_record) = evidence_records.get_mut(record_index) {
            evidence_record.diagnostics.holm_adjusted_p_value = Some(adjusted_p_value);
        }
    }
}

fn finalize_predictive_gate_statuses(
    evidence_records: &mut [PredictiveEvidenceRecord],
    predictive_evidence_alpha: f64,
) {
    for evidence_record in evidence_records {
        if !predictive_evidence_has_test_support(evidence_record) {
            evidence_record.diagnostics.gate_status =
                PREDICTIVE_EVIDENCE_GATE_TOO_THIN_STATUS.to_string();
            continue;
        }

        let loss_improved = evidence_record
            .diagnostics
            .relative_loss_reduction
            .is_some_and(|loss_reduction| loss_reduction > 0.0);

        let adjusted_p_value_passes = evidence_record
            .diagnostics
            .holm_adjusted_p_value
            .is_some_and(|adjusted_p_value| adjusted_p_value <= predictive_evidence_alpha);

        evidence_record.diagnostics.gate_status = if loss_improved && adjusted_p_value_passes {
            PREDICTIVE_EVIDENCE_GATE_PASSED_STATUS.to_string()
        } else {
            PREDICTIVE_EVIDENCE_GATE_FAILED_STATUS.to_string()
        };
    }
}

fn predictive_evidence_has_test_support(evidence_record: &PredictiveEvidenceRecord) -> bool {
    evidence_record.diagnostics.raw_sign_test_p_value.is_some()
}

fn assign_predictive_evidence(
    grid_evaluations: &mut [MeanReversionGridCandidateEvaluation],
    evidence_records: Vec<PredictiveEvidenceRecord>,
) {
    for evidence_record in evidence_records {
        if let Some(candidate) = grid_evaluations
            .get_mut(evidence_record.grid_index)
            .and_then(|grid_evaluation| {
                grid_evaluation
                    .evaluated_candidates
                    .get_mut(evidence_record.candidate_index)
            })
        {
            candidate.predictive_evidence = Some(evidence_record.diagnostics);
        }
    }
}

fn select_passing_predictive_candidate(
    grid_evaluations: &[MeanReversionGridCandidateEvaluation],
) -> Option<MeanReversionCandidateSelection> {
    let mut selected_candidate: Option<MeanReversionCandidateSelection> = None;

    for (grid_index, grid_evaluation) in grid_evaluations.iter().enumerate() {
        for (candidate_index, candidate) in grid_evaluation.evaluated_candidates.iter().enumerate()
        {
            if !candidate_predictive_gate_passed(candidate) {
                continue;
            }

            let proposed_selection = MeanReversionCandidateSelection {
                grid_index,
                candidate_index,
                model_selection_policy: PREDICTIVE_EVIDENCE_MODEL_SELECTION_POLICY,
                stage_decision_reason: POSITIVE_PREDICTIVE_EVIDENCE_REASON.to_string(),
            };

            selected_candidate = Some(match selected_candidate {
                Some(current_selection)
                    if candidate_is_better_predictive_selection(
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
    }

    selected_candidate
}

fn candidate_predictive_gate_passed(candidate: &MeanReversionStageCandidateEvaluation) -> bool {
    candidate
        .predictive_evidence
        .as_ref()
        .is_some_and(|evidence| evidence.gate_status == PREDICTIVE_EVIDENCE_GATE_PASSED_STATUS)
}

fn candidate_is_better_predictive_selection(
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

    let Some(proposed_evidence) = proposed_candidate.predictive_evidence.as_ref() else {
        return false;
    };

    let Some(current_evidence) = current_candidate.predictive_evidence.as_ref() else {
        return true;
    };

    match (
        proposed_evidence.holm_adjusted_p_value,
        current_evidence.holm_adjusted_p_value,
    ) {
        (Some(proposed_p_value), Some(current_p_value)) if proposed_p_value != current_p_value => {
            return proposed_p_value < current_p_value;
        }
        _ => {}
    }

    match (
        proposed_evidence.relative_loss_reduction,
        current_evidence.relative_loss_reduction,
    ) {
        (Some(proposed_loss_reduction), Some(current_loss_reduction))
            if proposed_loss_reduction != current_loss_reduction =>
        {
            return proposed_loss_reduction > current_loss_reduction;
        }
        _ => {}
    }

    proposed_candidate.information_criterion_bic < current_candidate.information_criterion_bic
}

fn select_best_bic_fallback_candidate(
    grid_evaluations: &[MeanReversionGridCandidateEvaluation],
) -> Option<MeanReversionCandidateSelection> {
    select_best_bic_ranked_candidate(
        grid_evaluations,
        best_admissible_bic_candidate_index,
        PREDICTIVE_EVIDENCE_WITH_BIC_FALLBACK_POLICY,
        PREDICTIVE_EVIDENCE_THIN_SAMPLE_BIC_FALLBACK_REASON,
    )
}

fn select_best_random_walk_null_candidate(
    grid_evaluations: &[MeanReversionGridCandidateEvaluation],
) -> Option<MeanReversionCandidateSelection> {
    select_best_bic_ranked_candidate(
        grid_evaluations,
        random_walk_null_candidate_index,
        PREDICTIVE_EVIDENCE_MODEL_SELECTION_POLICY,
        NO_POSITIVE_PREDICTIVE_EVIDENCE_REASON,
    )
}

fn select_best_bic_ranked_candidate(
    grid_evaluations: &[MeanReversionGridCandidateEvaluation],
    candidate_index_for_grid: impl Fn(&MeanReversionGridCandidateEvaluation) -> Option<usize>,
    model_selection_policy: &'static str,
    stage_decision_reason: &'static str,
) -> Option<MeanReversionCandidateSelection> {
    let mut selected_candidate: Option<MeanReversionCandidateSelection> = None;

    for (grid_index, grid_evaluation) in grid_evaluations.iter().enumerate() {
        let Some(candidate_index) = candidate_index_for_grid(grid_evaluation) else {
            continue;
        };

        let proposed_selection = MeanReversionCandidateSelection {
            grid_index,
            candidate_index,
            model_selection_policy,
            stage_decision_reason: stage_decision_reason.to_string(),
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

fn random_walk_null_candidate_index(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
) -> Option<usize> {
    grid_evaluation
        .evaluated_candidates
        .iter()
        .position(|candidate| candidate.candidate_family == RANDOM_WALK_NULL_CANDIDATE_FAMILY)
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

#[cfg(test)]
mod tests {
    use super::{
        PREDICTIVE_EVIDENCE_GATE_FAILED_STATUS, PREDICTIVE_EVIDENCE_GATE_PASSED_STATUS,
        PREDICTIVE_EVIDENCE_REFIT_SCHEDULE_POLICY, PredictiveEvidenceRecord,
        apply_holm_adjusted_p_values, finalize_predictive_gate_statuses,
    };
    use janiform_calibration_artifacts::MeanReversionStagePredictiveEvidenceDiagnostics;

    fn evidence_record_with_raw_p_value(raw_sign_test_p_value: f64) -> PredictiveEvidenceRecord {
        PredictiveEvidenceRecord {
            grid_index: 0,
            candidate_index: 0,
            diagnostics: MeanReversionStagePredictiveEvidenceDiagnostics {
                gate_status: String::new(),
                significance_level: 0.05,
                validation_refit_schedule_policy: PREDICTIVE_EVIDENCE_REFIT_SCHEDULE_POLICY
                    .to_string(),
                validation_refit_count: 1,
                validation_transition_count: 12,
                validation_block_length: 1,
                validation_block_count: 12,
                positive_block_count: 12,
                negative_block_count: 0,
                tied_block_count: 0,
                random_walk_validation_loss: Some(1.0),
                candidate_validation_loss: Some(0.5),
                relative_loss_reduction: Some(0.5),
                raw_sign_test_p_value: Some(raw_sign_test_p_value),
                holm_adjusted_p_value: None,
            },
        }
    }

    #[test]
    fn holm_correction_adjusts_multiple_predictive_evidence_tests_monotonically() {
        let mut evidence_records = vec![
            evidence_record_with_raw_p_value(0.04),
            evidence_record_with_raw_p_value(0.01),
            evidence_record_with_raw_p_value(0.20),
        ];

        apply_holm_adjusted_p_values(evidence_records.as_mut_slice());
        finalize_predictive_gate_statuses(evidence_records.as_mut_slice(), 0.05);

        assert_eq!(
            evidence_records[0].diagnostics.holm_adjusted_p_value,
            Some(2.0 * 0.04)
        );
        assert_eq!(
            evidence_records[1].diagnostics.holm_adjusted_p_value,
            Some(3.0 * 0.01)
        );
        assert_eq!(
            evidence_records[2].diagnostics.holm_adjusted_p_value,
            Some(0.20)
        );
        assert_eq!(
            evidence_records[0].diagnostics.gate_status,
            PREDICTIVE_EVIDENCE_GATE_FAILED_STATUS
        );
        assert_eq!(
            evidence_records[1].diagnostics.gate_status,
            PREDICTIVE_EVIDENCE_GATE_PASSED_STATUS
        );
        assert_eq!(
            evidence_records[2].diagnostics.gate_status,
            PREDICTIVE_EVIDENCE_GATE_FAILED_STATUS
        );
    }
}
