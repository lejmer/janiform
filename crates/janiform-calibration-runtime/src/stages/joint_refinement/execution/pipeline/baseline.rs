use std::sync::mpsc::Sender;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};
use janiform_calibration_artifacts::{CalibrationLossTermKind, CalibrationOptimizerSettings};

use super::super::super::objective::evaluate_joint_fitted_parameters;
use super::super::{
    JointRefinementCandidateEvaluation, JointRefinementCandidateOutcome, JointRefinementContext,
    JointRefinementFidelityTier, build_joint_candidate_coarse_consistency_artifacts_result,
    ordered_parameter_bits, with_joint_refinement_thread_pool,
};
use super::infrastructure::ensure_simulation_infrastructure_available;
use super::progress::emit_coarse_consistency_progress;
use super::types::{BaselineEvaluationArtifacts, ResolvedStagePayloads};

pub(super) fn establish_baseline_evaluation(
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    bootstrap_context: &mut JointRefinementContext,
    baseline_parameter_vector: &[f64],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Result<BaselineEvaluationArtifacts, JointRefinementStageExecutionFailure> {
    let baseline_fitted_parameters =
        janiform_calibration_artifacts::JointRefinementStageFittedParameters {
            hawkes_fitted_parameters: resolved_stage_payloads
                .hawkes_payload
                .fitted_parameters
                .clone(),
            return_components_fitted_parameters: bootstrap_context
                .baseline_return_components_fitted_parameters
                .clone(),
            pricing_block_fitted_parameters: None,
        };

    let baseline_parameter_bits = ordered_parameter_bits(baseline_parameter_vector);

    let (baseline_local_evaluation, baseline_final_evaluation) = evaluate_baseline_local_and_final(
        &baseline_fitted_parameters,
        baseline_parameter_bits.as_slice(),
        bootstrap_context,
        &resolved_stage_payloads.stage_solver_settings,
        progress_sender,
    );

    ensure_simulation_infrastructure_available(bootstrap_context)?;

    let baseline_candidate_is_valid = baseline_local_evaluation.invalid_reason.is_none()
        && baseline_final_evaluation.invalid_reason.is_none();

    if let Ok(mut cache_guard) = bootstrap_context.objective_cache.lock() {
        cache_guard.clear();
    }

    let (normalized_baseline_local_evaluation, normalized_baseline_final_evaluation) =
        if baseline_candidate_is_valid {
            evaluate_baseline_local_and_final(
                &baseline_fitted_parameters,
                baseline_parameter_bits.as_slice(),
                bootstrap_context,
                &resolved_stage_payloads.stage_solver_settings,
                progress_sender,
            )
        } else {
            (
                baseline_local_evaluation.clone(),
                baseline_final_evaluation.clone(),
            )
        };

    let baseline_coarse_consistency_artifacts = if baseline_candidate_is_valid {
        emit_coarse_consistency_progress(progress_sender, 0, 1, "baseline coarse_consistency");
        let baseline_coarse_consistency_artifacts =
            build_joint_candidate_coarse_consistency_artifacts_result(
                bootstrap_context,
                &normalized_baseline_final_evaluation.fitted_parameters,
                Some(baseline_parameter_bits.as_slice()),
                Some(
                    super::super::coarse_consistency::JointRefinementCoarseConsistencyProgress {
                        sender: progress_sender,
                        phase:
                            JointRefinementStageProgressPhase::BaselineCoarseConsistencyEvaluation,
                        current_item_label: Some("baseline coarse_consistency"),
                    },
                ),
            )
            .map_err(|source| {
                JointRefinementStageExecutionFailure::SimulationInfrastructureUnavailable {
                    error_message: format!(
                        "baseline coarse consistency evaluation failed: {source}"
                    ),
                }
            })?;

        emit_coarse_consistency_progress(progress_sender, 1, 1, "baseline coarse_consistency");
        Some(baseline_coarse_consistency_artifacts)
    } else {
        None
    };

    let baseline_candidate_outcome = JointRefinementCandidateOutcome {
        candidate_id: "baseline".to_string(),
        parameter_vector: baseline_parameter_vector.to_vec(),
        local_evaluation: normalized_baseline_local_evaluation,
        final_evaluation: normalized_baseline_final_evaluation,
        local_evaluation_count: 0,
        stop_reason: "baseline_incumbent".to_string(),
    };

    if let Ok(mut cache_guard) = bootstrap_context.objective_cache.lock() {
        cache_guard.clear();
    }

    Ok(BaselineEvaluationArtifacts {
        baseline_candidate_outcome,
        baseline_coarse_consistency_artifacts,
    })
}

fn evaluate_baseline_local_and_final(
    baseline_fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    baseline_parameter_bits: &[u64],
    bootstrap_context: &JointRefinementContext,
    stage_solver_settings: &CalibrationOptimizerSettings,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> (
    JointRefinementCandidateEvaluation,
    JointRefinementCandidateEvaluation,
) {
    with_joint_refinement_thread_pool(bootstrap_context, || {
        rayon::join(
            || {
                evaluate_joint_fitted_parameters(
                    baseline_fitted_parameters.clone(),
                    Some(baseline_parameter_bits),
                    bootstrap_context,
                    JointRefinementFidelityTier::Local,
                    stage_solver_settings,
                    progress_sender,
                    Some(JointRefinementStageProgressPhase::BaselineLocalEvaluation),
                )
            },
            || {
                evaluate_joint_fitted_parameters(
                    baseline_fitted_parameters.clone(),
                    Some(baseline_parameter_bits),
                    bootstrap_context,
                    JointRefinementFidelityTier::Final,
                    stage_solver_settings,
                    progress_sender,
                    Some(JointRefinementStageProgressPhase::BaselineFinalEvaluation),
                )
            },
        )
    })
}

pub(crate) fn ensure_valid_selected_candidate_outcome(
    best_candidate: &JointRefinementCandidateOutcome,
    valid_candidate_count: usize,
    admissible_candidate_count: usize,
    treat_warning_as_error: bool,
) -> Result<Option<String>, JointRefinementStageExecutionFailure> {
    if best_candidate.local_evaluation.invalid_reason.is_some()
        || best_candidate.final_evaluation.invalid_reason.is_some()
    {
        let local_reason = best_candidate
            .local_evaluation
            .invalid_reason
            .clone()
            .unwrap_or_else(|| "unknown_local_joint_refinement_failure".to_string());

        let final_reason = best_candidate
            .final_evaluation
            .invalid_reason
            .clone()
            .unwrap_or_else(|| "unknown_final_joint_refinement_failure".to_string());

        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint refinement could not identify a valid candidate after candidate search: selected_candidate={} valid_candidate_count={} local={local_reason} final={final_reason}",
                    best_candidate.candidate_id, valid_candidate_count,
                ),
            },
        );
    }

    if best_candidate
        .final_evaluation
        .breakdown
        .admissibility
        .admissible
    {
        return Ok(None);
    }

    let invalid_term_ids = selected_candidate_invalid_candidate_term_ids(best_candidate);

    let warning = format!(
        "joint refinement did not identify a finite-loss replacement for the baseline handoff: rejected_candidate={} valid_candidate_count={} admissible_candidate_count={} rejected_global_loss={} invalid_term_ids={}",
        best_candidate.candidate_id,
        valid_candidate_count,
        admissible_candidate_count,
        best_candidate.final_evaluation.breakdown.global_loss,
        invalid_term_ids,
    );

    if treat_warning_as_error {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: warning,
            },
        );
    }

    Ok(Some(warning))
}

fn selected_candidate_invalid_candidate_term_ids(
    candidate_outcome: &JointRefinementCandidateOutcome,
) -> String {
    let invalid_term_ids = candidate_outcome
        .final_evaluation
        .breakdown
        .term_diagnostics
        .iter()
        .filter(|term| term.term_kind == CalibrationLossTermKind::InvalidCandidate)
        .take(3)
        .map(|term| term.term_id.as_str())
        .collect::<Vec<_>>()
        .join(",");

    if invalid_term_ids.is_empty() {
        "none".to_string()
    } else {
        invalid_term_ids
    }
}
