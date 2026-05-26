use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_work_scheduler::{DynamicWorkPoolStatus, StaticWorkPoolInput, run_static_work_pool};

use crate::stages::joint_refinement::contracts::JointRefinementStageProgressUpdate;

use super::super::super::super::objective::evaluate_joint_candidate;
use super::super::super::{
    JointRefinementContext, JointRefinementFidelityTier, JointRefinementRestartOutcome,
    joint_refinement_parallel_worker_count,
};
use super::super::workflow::{
    build_precheck_rejected_restart_outcome, joint_refinement_has_fatal_execution_failure,
    precheck_restart_seed, refreshed_full_vector_candidate,
};
use super::progress::{emit_restart_finalization_progress, emit_restart_finalization_started};
use super::{RestartProbeFinalizationResult, RestartProbeState};

struct RestartProbeFinalizationJob {
    restart_position: usize,
    state: RestartProbeState,
}

struct RestartProbeFinalizationPoolState {
    restart_outcomes: Vec<Option<JointRefinementRestartOutcome>>,
}

pub(super) fn finalize_restart_probe_outcomes(
    states: Vec<RestartProbeState>,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    completed_search_evaluation_count: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Vec<JointRefinementRestartOutcome> {
    let total_restart_count = states.len();

    let jobs = states
        .into_iter()
        .enumerate()
        .map(|(restart_position, state)| {
            emit_restart_finalization_started(
                progress_sender,
                &state,
                total_restart_count,
                completed_search_evaluation_count,
                total_progress_units,
            );

            RestartProbeFinalizationJob {
                restart_position,
                state,
            }
        })
        .collect::<Vec<_>>();

    let mut pool_state = RestartProbeFinalizationPoolState {
        restart_outcomes: std::iter::repeat_with(|| None)
            .take(total_restart_count)
            .collect(),
    };

    run_static_work_pool(
        context.stage_thread_pool.as_deref(),
        StaticWorkPoolInput::new(
            &mut pool_state,
            jobs,
            joint_refinement_parallel_worker_count(context),
            |job: RestartProbeFinalizationJob| RestartProbeFinalizationResult {
                restart_position: job.restart_position,
                outcome: build_restart_probe_outcome(job.state, context, solver_settings),
            },
            |pool_state: &mut RestartProbeFinalizationPoolState,
             result: RestartProbeFinalizationResult,
             status: DynamicWorkPoolStatus| {
                let restart_position = result.restart_position;
                if let Some(outcome_slot) = pool_state.restart_outcomes.get_mut(restart_position) {
                    *outcome_slot = Some(result.outcome);
                }

                if let Some(outcome) = pool_state
                    .restart_outcomes
                    .get(restart_position)
                    .and_then(Option::as_ref)
                {
                    emit_restart_finalization_progress(
                        progress_sender,
                        outcome,
                        pool_state.restart_outcomes.as_slice(),
                        status.completed_work_count,
                        total_restart_count,
                        completed_search_evaluation_count,
                        total_progress_units,
                    );
                }
            },
        ),
    );

    pool_state.restart_outcomes.into_iter().flatten().collect()
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
