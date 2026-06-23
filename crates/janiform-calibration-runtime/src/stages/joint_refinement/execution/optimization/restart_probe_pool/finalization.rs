use std::sync::Arc;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use super::super::super::super::objective::evaluate_joint_candidate;
use super::super::super::{
    JointRefinementContext, JointRefinementFidelityTier, JointRefinementRestartOutcome,
    select_best_joint_restart_outcome,
};
use super::super::workflow::{
    build_precheck_rejected_restart_outcome, joint_refinement_has_fatal_execution_failure,
    precheck_restart_seed, refreshed_full_vector_candidate,
};
use super::RestartProbeState;

#[derive(Debug)]
pub(super) struct RestartProbeFinalizationJob {
    pub(super) restart_position: usize,
    pub(super) candidate_position: usize,
    pub(super) restart_context: Arc<RestartProbeFinalizationRestartContext>,
    pub(super) candidate_vector: Vec<f64>,
}

#[derive(Debug)]
pub(super) struct RestartProbeFinalizationRestartContext {
    pub(super) restart_id: String,
    pub(super) seed_vector: Vec<f64>,
    pub(super) evaluation_count: usize,
    pub(super) stop_reason: String,
}

#[derive(Debug)]
pub(super) struct RestartProbeFinalizationResult {
    pub(super) restart_position: usize,
    pub(super) candidate_position: usize,
    pub(super) outcome: JointRefinementRestartOutcome,
}

pub(super) fn run_restart_probe_finalization_job(
    job: RestartProbeFinalizationJob,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> RestartProbeFinalizationResult {
    RestartProbeFinalizationResult {
        restart_position: job.restart_position,
        candidate_position: job.candidate_position,
        outcome: evaluate_restart_probe_finalization_candidate(
            job.restart_context.restart_id.as_str(),
            job.restart_context.seed_vector.as_slice(),
            job.candidate_vector.as_slice(),
            context,
            solver_settings,
            job.restart_context.evaluation_count,
            job.restart_context.stop_reason.as_str(),
        ),
    }
}

pub(super) fn restart_probe_finalization_jobs(
    restart_position: usize,
    state: &RestartProbeState,
) -> Vec<RestartProbeFinalizationJob> {
    let candidate_vectors = restart_probe_finalization_candidate_vectors(state);
    if candidate_vectors.is_empty() {
        return Vec::new();
    }

    // Jobs own candidate context so the scheduler can continue mutating restart state.
    let restart_context = Arc::new(RestartProbeFinalizationRestartContext {
        restart_id: state.restart_id.clone(),
        seed_vector: state.seed_vector.clone(),
        evaluation_count: state.evaluation_count,
        stop_reason: state.stop_reason.clone(),
    });

    candidate_vectors
        .into_iter()
        .enumerate()
        .map(
            |(candidate_position, candidate_vector)| RestartProbeFinalizationJob {
                restart_position,
                candidate_position,
                restart_context: Arc::clone(&restart_context),
                candidate_vector,
            },
        )
        .collect()
}

pub(super) fn select_restart_probe_finalization_outcome(
    state: &RestartProbeState,
    candidate_outcomes: Vec<JointRefinementRestartOutcome>,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementRestartOutcome {
    select_best_joint_restart_outcome(candidate_outcomes).unwrap_or_else(|| {
        build_precheck_rejected_restart_outcome(
            state.restart_id.as_str(),
            state.seed_vector.clone(),
            context,
            solver_settings,
            "empty_finalization_candidate_set",
        )
    })
}

fn restart_probe_finalization_candidate_vectors(state: &RestartProbeState) -> Vec<Vec<f64>> {
    let mut candidate_vectors = Vec::new();

    push_distinct_finalization_candidate(&mut candidate_vectors, state.best_vector.as_slice());
    push_distinct_finalization_candidate(&mut candidate_vectors, state.accepted_vector.as_slice());

    if let Some(round_best_vector) = &state.round_best_vector {
        push_distinct_finalization_candidate(&mut candidate_vectors, round_best_vector.as_slice());
    }

    push_distinct_finalization_candidate(&mut candidate_vectors, state.seed_vector.as_slice());

    candidate_vectors
}

fn push_distinct_finalization_candidate(
    candidate_vectors: &mut Vec<Vec<f64>>,
    candidate_vector: &[f64],
) {
    if candidate_vectors
        .iter()
        .any(|existing_vector| existing_vector.as_slice() == candidate_vector)
    {
        return;
    }

    candidate_vectors.push(candidate_vector.to_vec());
}

fn evaluate_restart_probe_finalization_candidate(
    restart_id: &str,
    seed_vector: &[f64],
    candidate_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    evaluation_count: usize,
    stop_reason: &str,
) -> JointRefinementRestartOutcome {
    if let Err(precheck_failure) = precheck_restart_seed(candidate_vector, context) {
        return build_precheck_rejected_restart_outcome(
            restart_id,
            candidate_vector.to_vec(),
            context,
            solver_settings,
            precheck_failure.as_str(),
        );
    }

    let refreshed_vector =
        refreshed_full_vector_candidate(seed_vector, candidate_vector, context, solver_settings);

    let local_evaluation = evaluate_joint_candidate(
        refreshed_vector.as_slice(),
        context,
        JointRefinementFidelityTier::Local,
        solver_settings,
    );

    if local_evaluation.invalid_reason.is_some()
        || joint_refinement_has_fatal_execution_failure(context)
    {
        return JointRefinementRestartOutcome {
            restart_id: restart_id.to_string(),
            parameter_vector: refreshed_vector,
            // No final pass is valid after a local failure, so the final slot mirrors local.
            final_evaluation: local_evaluation.clone(),
            local_evaluation,
            local_evaluation_count: evaluation_count,
            stop_reason: stop_reason.to_string(),
        };
    }

    let final_evaluation = evaluate_joint_candidate(
        refreshed_vector.as_slice(),
        context,
        JointRefinementFidelityTier::Final,
        solver_settings,
    );

    JointRefinementRestartOutcome {
        restart_id: restart_id.to_string(),
        parameter_vector: refreshed_vector,
        local_evaluation,
        final_evaluation,
        local_evaluation_count: evaluation_count,
        stop_reason: stop_reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{RestartProbeState, restart_probe_finalization_candidate_vectors};

    fn finalization_state() -> RestartProbeState {
        RestartProbeState {
            restart_id: "restart".to_string(),
            seed_vector: vec![0.0, 0.0],
            accepted_vector: vec![1.0, 0.0],
            best_vector: vec![2.0, 0.0],
            initial_objective: Some(3.0),
            accepted_objective: Some(3.0),
            best_objective: Some(1.0),
            round_best_vector: Some(vec![1.0, 0.0]),
            round_best_objective: Some(2.0),
            round_best_residuals: None,
            round_best_predicted_residual_objective: None,
            accepted_residuals: None,
            evaluated_records: Vec::new(),
            scheduled_parameter_bits: Vec::new(),
            model_probe_vectors: Vec::new(),
            next_model_probe_position: 0,
            model_probes_prepared: false,
            trust_region_proposal_vectors: Vec::new(),
            next_trust_region_proposal_position: 0,
            trust_region_proposals_prepared: false,
            trust_region_radius: 0.25,
            completed_search_rounds: 0,
            evaluation_count: 4,
            max_evaluation_count: 4,
            in_flight_count: 0,
            stop_reason: "budget".to_string(),
            completed: true,
            exact_likelihood_evaluator_counts: None,
        }
    }

    #[test]
    fn finalization_candidates_preserve_distinct_search_vectors() {
        let state = finalization_state();

        let candidates = restart_probe_finalization_candidate_vectors(&state);

        assert_eq!(
            candidates,
            vec![
                state.best_vector.clone(),
                state.accepted_vector.clone(),
                state.seed_vector.clone(),
            ]
        );
    }
}
