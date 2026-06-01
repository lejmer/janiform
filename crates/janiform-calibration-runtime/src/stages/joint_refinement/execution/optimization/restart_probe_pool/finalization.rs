use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use super::super::super::super::objective::evaluate_joint_candidate;
use super::super::super::{
    JointRefinementContext, JointRefinementFidelityTier, JointRefinementRestartOutcome,
};
use super::super::workflow::{
    build_precheck_rejected_restart_outcome, joint_refinement_has_fatal_execution_failure,
    precheck_restart_seed, refreshed_full_vector_candidate,
};
use super::RestartProbeState;

#[derive(Debug)]
pub(super) struct RestartProbeFinalizationJob {
    pub(super) restart_position: usize,
    pub(super) state: RestartProbeState,
}

#[derive(Debug)]
pub(super) struct RestartProbeFinalizationResult {
    pub(super) restart_position: usize,
    pub(super) outcome: JointRefinementRestartOutcome,
}

pub(super) fn run_restart_probe_finalization_job(
    job: RestartProbeFinalizationJob,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> RestartProbeFinalizationResult {
    RestartProbeFinalizationResult {
        restart_position: job.restart_position,
        outcome: build_restart_probe_outcome(job.state, context, solver_settings),
    }
}

fn build_restart_probe_outcome(
    state: RestartProbeState,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementRestartOutcome {
    if let Err(precheck_failure) = precheck_restart_seed(state.best_vector.as_slice(), context) {
        return build_precheck_rejected_restart_outcome(
            state.restart_id.as_str(),
            state.best_vector,
            context,
            solver_settings,
            precheck_failure.as_str(),
        );
    }

    let refreshed_vector = refreshed_full_vector_candidate(
        state.seed_vector.as_slice(),
        state.best_vector.as_slice(),
        context,
        solver_settings,
    );

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
            restart_id: state.restart_id,
            parameter_vector: refreshed_vector,
            // No final pass is valid after a local failure, so the final slot mirrors local.
            final_evaluation: local_evaluation.clone(),
            local_evaluation,
            local_evaluation_count: state.evaluation_count,
            stop_reason: state.stop_reason,
        };
    }

    let final_evaluation = evaluate_joint_candidate(
        refreshed_vector.as_slice(),
        context,
        JointRefinementFidelityTier::Final,
        solver_settings,
    );

    JointRefinementRestartOutcome {
        restart_id: state.restart_id,
        parameter_vector: refreshed_vector,
        local_evaluation,
        final_evaluation,
        local_evaluation_count: state.evaluation_count,
        stop_reason: state.stop_reason,
    }
}
