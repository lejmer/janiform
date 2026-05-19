use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use crate::stages::joint_refinement::contracts::JointRefinementStageProgressUpdate;

use super::super::super::super::objective::evaluate_joint_candidate;
use super::super::super::{
    JointRefinementContext, JointRefinementFidelityTier, JointRefinementRestartOutcome,
};
use super::super::{
    build_precheck_rejected_restart_outcome, joint_refinement_has_fatal_execution_failure,
    precheck_restart_seed, refreshed_full_vector_candidate,
};
use super::progress::{emit_restart_finalization_progress, emit_restart_finalization_started};
use super::{RestartProbeFinalizationResult, RestartProbeState};

pub(super) fn finalize_restart_probe_outcomes(
    states: Vec<RestartProbeState>,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    completed_search_evaluation_count: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Vec<JointRefinementRestartOutcome> {
    match context.stage_thread_pool.as_ref() {
        Some(thread_pool) => thread_pool.scope(|scope| {
            finalize_restart_probe_outcomes_in_scope(
                scope,
                states,
                context,
                solver_settings,
                completed_search_evaluation_count,
                total_progress_units,
                progress_sender,
            )
        }),
        None => rayon::scope(|scope| {
            finalize_restart_probe_outcomes_in_scope(
                scope,
                states,
                context,
                solver_settings,
                completed_search_evaluation_count,
                total_progress_units,
                progress_sender,
            )
        }),
    }
}

fn finalize_restart_probe_outcomes_in_scope<'scope>(
    scope: &rayon::Scope<'scope>,
    states: Vec<RestartProbeState>,
    context: &'scope JointRefinementContext,
    solver_settings: &'scope CalibrationOptimizerSettings,
    completed_search_evaluation_count: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Vec<JointRefinementRestartOutcome> {
    let total_restart_count = states.len();

    let mut restart_outcomes = std::iter::repeat_with(|| None)
        .take(total_restart_count)
        .collect::<Vec<Option<JointRefinementRestartOutcome>>>();

    let (result_sender, result_receiver) =
        std::sync::mpsc::channel::<RestartProbeFinalizationResult>();

    for (restart_position, state) in states.into_iter().enumerate() {
        emit_restart_finalization_started(
            progress_sender,
            &state,
            total_restart_count,
            completed_search_evaluation_count,
            total_progress_units,
        );

        let result_sender = result_sender.clone();

        scope.spawn(move |_| {
            let outcome = build_restart_probe_outcome(state, context, solver_settings);

            let _ = result_sender.send(RestartProbeFinalizationResult {
                restart_position,
                outcome,
            });
        });
    }

    drop(result_sender);

    let mut completed_finalization_count = 0usize;

    while completed_finalization_count < total_restart_count {
        let Ok(result) = result_receiver.recv() else {
            break;
        };

        completed_finalization_count = completed_finalization_count.saturating_add(1);

        let restart_position = result.restart_position;
        if let Some(outcome_slot) = restart_outcomes.get_mut(restart_position) {
            *outcome_slot = Some(result.outcome);
        }

        if let Some(outcome) = restart_outcomes
            .get(restart_position)
            .and_then(Option::as_ref)
        {
            emit_restart_finalization_progress(
                progress_sender,
                outcome,
                restart_outcomes.as_slice(),
                completed_finalization_count,
                total_restart_count,
                completed_search_evaluation_count,
                total_progress_units,
            );
        }
    }

    restart_outcomes.into_iter().flatten().collect()
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
