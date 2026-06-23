use std::sync::Arc;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use crate::stages::joint_refinement::contracts::JointRefinementStageProgressUpdate;

use super::super::super::{JointRefinementContext, JointRefinementRestartOutcome};
use super::super::workflow::{
    push_distinct_restart_finalization_candidate, select_finalized_restart_candidate_outcome,
};
use super::progress::emit_residual_control_finalization_started;
use super::{
    ResidualControlFinalizationJob, ResidualControlFinalizationRestartContext,
    ResidualControlState, ResidualControlWorkerPoolState,
};

pub(super) fn next_residual_control_finalization_job(
    pool_state: &mut ResidualControlWorkerPoolState<'_>,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Option<ResidualControlFinalizationJob> {
    if let Some(finalization_job) = pool_state.pending_finalization_jobs.pop_front() {
        return Some(finalization_job);
    }

    if pool_state.states.is_empty() {
        return None;
    }

    for _ in 0..pool_state.states.len() {
        let restart_position =
            pool_state.next_finalization_restart_position % pool_state.states.len();

        pool_state.next_finalization_restart_position = pool_state
            .next_finalization_restart_position
            .saturating_add(1)
            % pool_state.states.len();

        if !residual_control_finalization_is_schedulable(pool_state, restart_position) {
            continue;
        }

        let Some(state) = pool_state.states.get(restart_position) else {
            continue;
        };

        let finalization_jobs = residual_control_finalization_jobs(restart_position, state);
        if finalization_jobs.is_empty() {
            continue;
        }

        emit_residual_control_finalization_started(
            progress_sender,
            state,
            pool_state.states,
            pool_state.restart_outcomes.as_slice(),
            pool_state.completed_search_evaluation_count,
            pool_state.completed_finalization_count,
            total_progress_units,
        );

        if let Some(finalization_slot) = pool_state.finalization_scheduled.get_mut(restart_position)
        {
            *finalization_slot = true;
        }

        if let Some(candidate_outcome_slots) = pool_state
            .finalization_candidate_outcomes
            .get_mut(restart_position)
        {
            *candidate_outcome_slots = std::iter::repeat_with(|| None)
                .take(finalization_jobs.len())
                .collect();
        }

        pool_state
            .pending_finalization_jobs
            .extend(finalization_jobs);

        return pool_state.pending_finalization_jobs.pop_front();
    }

    None
}

fn residual_control_finalization_is_schedulable(
    pool_state: &ResidualControlWorkerPoolState<'_>,
    restart_position: usize,
) -> bool {
    let Some(state) = pool_state.states.get(restart_position) else {
        return false;
    };

    if !state.completed || state.in_flight_count > 0 {
        return false;
    }

    if pool_state
        .finalization_scheduled
        .get(restart_position)
        .copied()
        .unwrap_or(false)
    {
        return false;
    }

    pool_state
        .restart_outcomes
        .get(restart_position)
        .is_some_and(Option::is_none)
}

fn residual_control_finalization_jobs(
    restart_position: usize,
    state: &ResidualControlState,
) -> Vec<ResidualControlFinalizationJob> {
    let candidate_vectors = residual_control_finalization_candidate_vectors(state);
    if candidate_vectors.is_empty() {
        return Vec::new();
    }

    // Jobs own restart context so the scheduler can continue mutating search state.
    let restart_context = Arc::new(ResidualControlFinalizationRestartContext {
        restart_id: state.restart_id.clone(),
        seed_vector: state.seed_vector.clone(),
        evaluation_count: state.evaluation_count,
        stop_reason: state.stop_reason.clone(),
    });

    candidate_vectors
        .into_iter()
        .enumerate()
        .map(
            |(candidate_position, candidate_vector)| ResidualControlFinalizationJob {
                restart_position,
                candidate_position,
                restart_context: Arc::clone(&restart_context),
                candidate_vector,
            },
        )
        .collect()
}

fn residual_control_finalization_candidate_vectors(state: &ResidualControlState) -> Vec<Vec<f64>> {
    let mut candidate_vectors = Vec::new();

    push_distinct_restart_finalization_candidate(
        &mut candidate_vectors,
        state.best_global_vector.as_slice(),
    );

    push_distinct_restart_finalization_candidate(
        &mut candidate_vectors,
        state.best_profile_vector.as_slice(),
    );

    push_distinct_restart_finalization_candidate(
        &mut candidate_vectors,
        state.accepted_vector.as_slice(),
    );

    if let Some(round_best_vector) = &state.round_best_vector {
        push_distinct_restart_finalization_candidate(
            &mut candidate_vectors,
            round_best_vector.as_slice(),
        );
    }

    push_distinct_restart_finalization_candidate(
        &mut candidate_vectors,
        state.seed_vector.as_slice(),
    );

    candidate_vectors
}

pub(super) fn select_residual_control_finalization_outcome(
    state: &ResidualControlState,
    candidate_outcomes: Vec<JointRefinementRestartOutcome>,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementRestartOutcome {
    select_finalized_restart_candidate_outcome(
        state.restart_id.as_str(),
        state.seed_vector.as_slice(),
        candidate_outcomes,
        context,
        solver_settings,
    )
}
