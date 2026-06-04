use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, MeanReversionStagePredictiveEvidenceDiagnostics,
};

use super::evidence_statistics::{
    minimum_supported_sign_test_block_count, one_sided_exact_sign_test_p_value,
    random_walk_innovation_dependence_block_length, random_walk_residuals, signed_block_counts,
    validation_block_improvements,
};
use super::{
    FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY, FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY,
    MeanReversionGridCandidateEvaluation, MeanReversionStageCandidateEvaluation,
    MeanReversionStageExecutionFailure, RANDOM_WALK_NULL_CANDIDATE_FAMILY,
    candidate_supports_quasi_likelihood_model_selection, materialize_pressure_state_path,
    profile_flat_anchor_mean_reversion_candidate, profile_full_pressure_to_level_candidate,
};

pub(super) const PREDICTIVE_EVIDENCE_MODEL_SELECTION_POLICY: &str =
    "rolling_origin_predictive_sign_test_mean_reversion_evidence_gate.v1";
const PREDICTIVE_EVIDENCE_WITH_BIC_FALLBACK_POLICY: &str = "rolling_origin_predictive_sign_test_with_gaussian_quasi_likelihood_bic_thin_sample_fallback.v1";
const POSITIVE_PREDICTIVE_EVIDENCE_REASON: &str =
    "positive_kappa_predictive_evidence_gate_passed.v1";
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
) -> Result<Option<MeanReversionCandidateSelection>, MeanReversionStageExecutionFailure> {
    let mut evidence_records =
        predictive_evidence_records(grid_evaluations, solver_settings, predictive_evidence_alpha)?;

    apply_holm_adjusted_p_values(evidence_records.as_mut_slice());
    finalize_predictive_gate_statuses(evidence_records.as_mut_slice(), predictive_evidence_alpha);

    let positive_candidate_was_tested = !evidence_records.is_empty();
    let predictive_evidence_is_supported = evidence_records
        .iter()
        .any(predictive_evidence_has_test_support);

    assign_predictive_evidence(grid_evaluations, evidence_records);

    if let Some(selection) = select_passing_predictive_candidate(grid_evaluations) {
        return Ok(Some(selection));
    }

    if positive_candidate_was_tested && !predictive_evidence_is_supported {
        return Ok(select_best_bic_fallback_candidate(grid_evaluations));
    }

    Ok(select_best_random_walk_null_candidate(grid_evaluations))
}

fn predictive_evidence_records(
    grid_evaluations: &[MeanReversionGridCandidateEvaluation],
    solver_settings: &CalibrationOptimizerSettings,
    predictive_evidence_alpha: f64,
) -> Result<Vec<PredictiveEvidenceRecord>, MeanReversionStageExecutionFailure> {
    let mut evidence_records = Vec::new();

    for (grid_index, grid_evaluation) in grid_evaluations.iter().enumerate() {
        for (candidate_index, candidate) in grid_evaluation.evaluated_candidates.iter().enumerate()
        {
            if candidate.kappa <= 0.0 {
                continue;
            }

            let diagnostics = evaluate_predictive_evidence(
                grid_evaluation,
                candidate,
                solver_settings,
                predictive_evidence_alpha,
            )?;

            evidence_records.push(PredictiveEvidenceRecord {
                grid_index,
                candidate_index,
                diagnostics,
            });
        }
    }

    Ok(evidence_records)
}

fn evaluate_predictive_evidence(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    candidate: &MeanReversionStageCandidateEvaluation,
    solver_settings: &CalibrationOptimizerSettings,
    predictive_evidence_alpha: f64,
) -> Result<MeanReversionStagePredictiveEvidenceDiagnostics, MeanReversionStageExecutionFailure> {
    let random_walk_residuals =
        random_walk_residuals(grid_evaluation.observation_levels.as_slice());

    let validation_block_length =
        random_walk_innovation_dependence_block_length(random_walk_residuals.as_slice());

    let mut transition_improvements = Vec::new();
    let mut random_walk_validation_loss = 0.0;
    let mut candidate_validation_loss = 0.0;

    for (validation_transition_index, observation_pair) in
        grid_evaluation.observation_levels.windows(2).enumerate()
    {
        let Some(candidate_prediction) = rolling_origin_candidate_prediction(
            grid_evaluation,
            candidate.candidate_family,
            validation_transition_index,
            solver_settings,
        )?
        else {
            continue;
        };

        let [current_observation, next_observation] = observation_pair else {
            continue;
        };

        let random_walk_error = *next_observation - *current_observation;
        let candidate_error = *next_observation - candidate_prediction;
        let random_walk_loss = random_walk_error * random_walk_error;
        let candidate_loss = candidate_error * candidate_error;

        random_walk_validation_loss += random_walk_loss;
        candidate_validation_loss += candidate_loss;

        transition_improvements.push(random_walk_loss - candidate_loss);
    }

    let block_improvements =
        validation_block_improvements(transition_improvements.as_slice(), validation_block_length);

    let block_counts = signed_block_counts(block_improvements.as_slice());
    let signed_block_count = block_counts.positive_block_count + block_counts.negative_block_count;

    let minimum_supported_block_count =
        minimum_supported_sign_test_block_count(predictive_evidence_alpha);

    let relative_loss_reduction = if random_walk_validation_loss > 0.0 {
        Some(
            (random_walk_validation_loss - candidate_validation_loss) / random_walk_validation_loss,
        )
    } else {
        None
    };

    let raw_sign_test_p_value = if signed_block_count >= minimum_supported_block_count {
        Some(one_sided_exact_sign_test_p_value(
            block_counts.positive_block_count,
            signed_block_count,
        ))
    } else {
        None
    };

    Ok(MeanReversionStagePredictiveEvidenceDiagnostics {
        gate_status: PREDICTIVE_EVIDENCE_GATE_TOO_THIN_STATUS.to_string(),
        significance_level: predictive_evidence_alpha,
        validation_transition_count: transition_improvements.len(),
        validation_block_length,
        validation_block_count: block_improvements.len(),
        positive_block_count: block_counts.positive_block_count,
        negative_block_count: block_counts.negative_block_count,
        tied_block_count: block_counts.tied_block_count,
        random_walk_validation_loss: Some(random_walk_validation_loss),
        candidate_validation_loss: Some(candidate_validation_loss),
        relative_loss_reduction,
        raw_sign_test_p_value,
        holm_adjusted_p_value: None,
    })
}

fn rolling_origin_candidate_prediction(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    candidate_family: &str,
    validation_transition_index: usize,
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<Option<f64>, MeanReversionStageExecutionFailure> {
    let training_observation_count = validation_transition_index.saturating_add(1);
    if training_observation_count < 2 {
        return Ok(None);
    }

    let Some(training_observation_levels) = grid_evaluation
        .observation_levels
        .get(..training_observation_count)
    else {
        return Ok(None);
    };

    let Some(training_candidate) = fit_training_candidate(
        grid_evaluation,
        candidate_family,
        training_observation_levels,
        solver_settings,
    )?
    else {
        return Ok(None);
    };

    if !candidate_supports_quasi_likelihood_model_selection(&training_candidate) {
        return Ok(None);
    }

    predict_next_observation(
        grid_evaluation,
        &training_candidate,
        validation_transition_index,
    )
    .map(Some)
}

fn fit_training_candidate(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    candidate_family: &str,
    training_observation_levels: &[f64],
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<Option<MeanReversionStageCandidateEvaluation>, MeanReversionStageExecutionFailure> {
    match candidate_family {
        FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY => {
            let Some(calibration_context) = grid_evaluation.calibration_context.as_ref() else {
                return Ok(None);
            };

            let training_transition_count = training_observation_levels.len().saturating_sub(1);

            let Some(training_core_expected_increments) = calibration_context
                .core_expected_increments
                .get(..training_transition_count)
            else {
                return Ok(None);
            };

            let Some(training_interval_deltas) = calibration_context
                .interval_deltas
                .get(..training_transition_count)
            else {
                return Ok(None);
            };

            profile_full_pressure_to_level_candidate(
                training_observation_levels,
                grid_evaluation
                    .calibration_grid
                    .calibration_grid_interval_hours,
                training_core_expected_increments,
                training_interval_deltas,
                solver_settings,
            )
            .map(Some)
        }
        FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY => {
            profile_flat_anchor_mean_reversion_candidate(
                training_observation_levels,
                grid_evaluation
                    .calibration_grid
                    .calibration_grid_interval_hours,
                solver_settings,
            )
            .map(Some)
        }
        _ => Ok(None),
    }
}

fn predict_next_observation(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    candidate: &MeanReversionStageCandidateEvaluation,
    validation_transition_index: usize,
) -> Result<f64, MeanReversionStageExecutionFailure> {
    let Some(current_observation) = grid_evaluation
        .observation_levels
        .get(validation_transition_index)
        .copied()
    else {
        return Err(
            MeanReversionStageExecutionFailure::InvalidObservationTimestampGrid {
                error_message: format!(
                    "mean-reversion predictive validation received missing current observation at transition_index={validation_transition_index}"
                ),
            },
        );
    };

    match candidate.candidate_family {
        FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY => predict_full_pressure_to_level_next_observation(
            grid_evaluation,
            candidate,
            validation_transition_index,
            current_observation,
        ),
        FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY => {
            let anchor_origin_level = candidate.anchor_origin_level.unwrap_or(0.0);
            Ok(candidate.phi_reference * current_observation
                + (1.0 - candidate.phi_reference) * anchor_origin_level)
        }
        _ => Ok(current_observation),
    }
}

fn predict_full_pressure_to_level_next_observation(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    candidate: &MeanReversionStageCandidateEvaluation,
    validation_transition_index: usize,
    current_observation: f64,
) -> Result<f64, MeanReversionStageExecutionFailure> {
    let Some(calibration_context) = grid_evaluation.calibration_context.as_ref() else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message:
                    "mean-reversion predictive validation requires endogenous calibration context"
                        .to_string(),
            },
        );
    };

    let prediction_transition_count = validation_transition_index.saturating_add(1);

    let Some(core_expected_increments) = calibration_context
        .core_expected_increments
        .get(..prediction_transition_count)
    else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: format!(
                    "mean-reversion predictive validation missing core increment for transition_index={validation_transition_index}"
                ),
            },
        );
    };

    let Some(interval_deltas) = calibration_context
        .interval_deltas
        .get(..prediction_transition_count)
    else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: format!(
                    "mean-reversion predictive validation missing interval delta for transition_index={validation_transition_index}"
                ),
            },
        );
    };

    let pressure_state_path = materialize_pressure_state_path(
        grid_evaluation
            .calibration_grid
            .calibration_grid_interval_hours,
        candidate
            .anchor_model
            .pressure_persistence_per_reference_interval,
        core_expected_increments,
        interval_deltas,
    )?;

    let Some(current_pressure_state) = pressure_state_path
        .get(validation_transition_index)
        .copied()
    else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: format!(
                    "mean-reversion predictive validation missing current pressure state for transition_index={validation_transition_index}"
                ),
            },
        );
    };

    let Some(next_pressure_state) = pressure_state_path
        .get(validation_transition_index.saturating_add(1))
        .copied()
    else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: format!(
                    "mean-reversion predictive validation missing next pressure state for transition_index={validation_transition_index}"
                ),
            },
        );
    };

    let Some(interval_delta) = interval_deltas.get(validation_transition_index).copied() else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: format!(
                    "mean-reversion predictive validation missing prediction interval delta for transition_index={validation_transition_index}"
                ),
            },
        );
    };

    let phi = candidate.phi_reference.powf(
        interval_delta
            / grid_evaluation
                .calibration_grid
                .calibration_grid_interval_hours,
    );

    let anchor_origin_level = candidate.anchor_origin_level.unwrap_or(0.0);

    Ok(phi * current_observation
        + (1.0 - phi) * anchor_origin_level
        + candidate.anchor_model.pressure_level_gain
            * (next_pressure_state - phi * current_pressure_state))
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
        PredictiveEvidenceRecord, apply_holm_adjusted_p_values, finalize_predictive_gate_statuses,
    };
    use janiform_calibration_artifacts::MeanReversionStagePredictiveEvidenceDiagnostics;

    fn evidence_record_with_raw_p_value(raw_sign_test_p_value: f64) -> PredictiveEvidenceRecord {
        PredictiveEvidenceRecord {
            grid_index: 0,
            candidate_index: 0,
            diagnostics: MeanReversionStagePredictiveEvidenceDiagnostics {
                gate_status: String::new(),
                significance_level: 0.05,
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
