use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::JointRefinementStageRestartEvaluationTimingDiagnostic;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};

use super::super::super::JointRefinementRestartOutcome;
use super::super::emit_joint_refinement_progress;
use super::{
    RESTART_WORK_ITEM_STATUS_COMPLETED, RESTART_WORK_ITEM_STATUS_QUEUED,
    RESTART_WORK_ITEM_STATUS_REJECTED, RESTART_WORK_ITEM_STATUS_RUNNING,
    RESTART_WORK_ITEM_STATUS_WAITING, RestartProbeResult, RestartProbeState,
};

pub(super) fn restart_probe_evaluation_timing_diagnostic(
    state: &RestartProbeState,
    result: &RestartProbeResult,
) -> JointRefinementStageRestartEvaluationTimingDiagnostic {
    let exact_likelihood_evaluator_counts = result
        .evaluation
        .as_ref()
        .and_then(|evaluation| evaluation.exact_likelihood_evaluator_counts);

    // Diagnostics own labels because they are stored after the scheduler state is gone.
    JointRefinementStageRestartEvaluationTimingDiagnostic {
        restart_id: state.restart_id.clone(),
        schedule_category: result.schedule_category.to_string(),
        candidate_evaluation_count: result.candidate_evaluation_count,
        poll_direction_position: result.poll_direction_position,
        poll_direction_label: result.poll_direction_label.clone(),
        elapsed_millis: result.elapsed_millis,
        objective: result
            .evaluation
            .as_ref()
            .map(|evaluation| evaluation.objective),
        admissibility_blocker_count: result
            .evaluation
            .as_ref()
            .map(|evaluation| evaluation.admissibility_blocker_count),
        max_standardized_residual: result
            .evaluation
            .as_ref()
            .and_then(|evaluation| evaluation.max_standardized_residual),
        exact_closed_form_buy_count: exact_likelihood_evaluator_counts
            .map(|counts| counts.closed_form_buy),
        exact_closed_form_sell_count: exact_likelihood_evaluator_counts
            .map(|counts| counts.closed_form_sell),
        exact_tilted_mixed_count: exact_likelihood_evaluator_counts
            .map(|counts| counts.tilted_mixed),
    }
}

pub(super) fn emit_batched_restart_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    states: &[RestartProbeState],
    current_restart_position: usize,
    completed_evaluation_count: usize,
    total_pool_budget: usize,
    restart_evaluation_timing_diagnostic: Option<
        JointRefinementStageRestartEvaluationTimingDiagnostic,
    >,
) {
    let Some(current_state) = states.get(current_restart_position) else {
        return;
    };

    let best = best_restart_probe_state(states);

    let aggregate_progress_percent = completed_evaluation_count
        .saturating_mul(100)
        .checked_div(total_pool_budget.max(1))
        .unwrap_or(0)
        .min(99);

    // Progress payloads own restart labels so UI consumers are decoupled from scheduler state.
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(aggregate_progress_percent),
            current_item_label: Some(current_state.restart_id.clone()),
            current_restart_id: Some(current_state.restart_id.clone()),
            current_work_item_status: Some(restart_probe_state_status(current_state).to_string()),
            active_restart_count: Some(active_restart_count(states)),
            current_parameter_block: None,
            current_evaluation_count: Some(current_state.evaluation_count),
            total_evaluation_budget: Some(current_state.max_evaluation_count),
            evaluation_progress_percent: Some(
                current_state
                    .evaluation_count
                    .saturating_mul(100)
                    .checked_div(current_state.max_evaluation_count.max(1))
                    .unwrap_or(0)
                    .min(99),
            ),
            cumulative_local_evaluation_count: Some(completed_evaluation_count),
            current_global_loss: current_state.best_objective,
            current_block_best_global_loss: current_state.best_objective,
            best_restart_id: best.as_ref().map(|(restart_id, _)| restart_id.clone()),
            best_global_loss: best.map(|(_, objective)| objective),
            exact_closed_form_buy_count: current_state
                .exact_likelihood_evaluator_counts
                .map(|counts| counts.closed_form_buy),
            exact_closed_form_sell_count: current_state
                .exact_likelihood_evaluator_counts
                .map(|counts| counts.closed_form_sell),
            exact_tilted_mixed_count: current_state
                .exact_likelihood_evaluator_counts
                .map(|counts| counts.tilted_mixed),
            restart_evaluation_timing_diagnostic,
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::RestartOptimization,
                completed_restart_probe_state_count(states),
                states.len(),
            )
        },
    );
}

pub(super) fn emit_batched_restart_pool_finished(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    restart_outcomes: &[JointRefinementRestartOutcome],
) {
    let best = best_restart_outcome(restart_outcomes);

    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(100),
            active_restart_count: Some(0),
            valid_completed_units: Some(
                restart_outcomes
                    .iter()
                    .filter(|outcome| outcome.final_evaluation.invalid_reason.is_none())
                    .count(),
            ),
            best_restart_id: best.as_ref().map(|(restart_id, _)| restart_id.clone()),
            best_global_loss: best.map(|(_, objective)| objective),
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::RestartOptimization,
                restart_outcomes.len(),
                restart_outcomes.len(),
            )
        },
    );
}

pub(super) fn emit_restart_finalization_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    outcome: &JointRefinementRestartOutcome,
    restart_outcomes: &[Option<JointRefinementRestartOutcome>],
    completed_finalization_count: usize,
    total_restart_count: usize,
    completed_search_evaluation_count: usize,
    total_progress_units: usize,
) {
    let best = best_partial_restart_outcome(restart_outcomes);

    let completed_progress_units =
        completed_search_evaluation_count.saturating_add(completed_finalization_count);

    let aggregate_progress_percent = completed_progress_units
        .saturating_mul(100)
        .checked_div(total_progress_units.max(1))
        .unwrap_or(0)
        .min(99);

    let completed_evaluation_count = outcome.local_evaluation_count.max(1);

    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(aggregate_progress_percent),
            current_item_label: Some(outcome.restart_id.clone()),
            current_restart_id: Some(outcome.restart_id.clone()),
            current_work_item_status: Some(restart_outcome_status(outcome).to_string()),
            active_restart_count: Some(
                total_restart_count.saturating_sub(completed_finalization_count),
            ),
            current_parameter_block: None,
            current_evaluation_count: Some(outcome.local_evaluation_count),
            total_evaluation_budget: Some(completed_evaluation_count),
            evaluation_progress_percent: Some(100),
            cumulative_local_evaluation_count: Some(completed_search_evaluation_count),
            current_global_loss: Some(outcome.final_evaluation.breakdown.global_loss),
            current_block_best_global_loss: Some(outcome.local_evaluation.breakdown.global_loss),
            best_restart_id: best.as_ref().map(|(restart_id, _)| restart_id.clone()),
            best_global_loss: best.map(|(_, objective)| objective),
            exact_closed_form_buy_count: Some(
                outcome
                    .final_evaluation
                    .exact_likelihood_evaluator_counts
                    .closed_form_buy,
            ),
            exact_closed_form_sell_count: Some(
                outcome
                    .final_evaluation
                    .exact_likelihood_evaluator_counts
                    .closed_form_sell,
            ),
            exact_tilted_mixed_count: Some(
                outcome
                    .final_evaluation
                    .exact_likelihood_evaluator_counts
                    .tilted_mixed,
            ),
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::RestartOptimization,
                completed_finalization_count,
                total_restart_count,
            )
        },
    );
}

pub(super) fn emit_restart_finalization_started(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    state: &RestartProbeState,
    total_restart_count: usize,
    completed_search_evaluation_count: usize,
    total_progress_units: usize,
) {
    let aggregate_progress_percent = completed_search_evaluation_count
        .saturating_mul(100)
        .checked_div(total_progress_units.max(1))
        .unwrap_or(0)
        .min(99);

    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(aggregate_progress_percent),
            current_item_label: Some(state.restart_id.clone()),
            current_restart_id: Some(state.restart_id.clone()),
            current_work_item_status: Some(RESTART_WORK_ITEM_STATUS_RUNNING.to_string()),
            active_restart_count: Some(total_restart_count),
            current_parameter_block: None,
            current_evaluation_count: Some(state.evaluation_count),
            total_evaluation_budget: Some(state.max_evaluation_count),
            evaluation_progress_percent: Some(
                state
                    .evaluation_count
                    .saturating_mul(100)
                    .checked_div(state.max_evaluation_count.max(1))
                    .unwrap_or(0)
                    .min(99),
            ),
            cumulative_local_evaluation_count: Some(completed_search_evaluation_count),
            current_global_loss: state.best_objective,
            current_block_best_global_loss: state.best_objective,
            exact_closed_form_buy_count: state
                .exact_likelihood_evaluator_counts
                .map(|counts| counts.closed_form_buy),
            exact_closed_form_sell_count: state
                .exact_likelihood_evaluator_counts
                .map(|counts| counts.closed_form_sell),
            exact_tilted_mixed_count: state
                .exact_likelihood_evaluator_counts
                .map(|counts| counts.tilted_mixed),
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::RestartOptimization,
                0,
                total_restart_count,
            )
        },
    );
}

fn restart_probe_state_status(state: &RestartProbeState) -> &'static str {
    if state.in_flight_count > 0 {
        return RESTART_WORK_ITEM_STATUS_RUNNING;
    }

    if state.evaluation_count == 0 {
        return RESTART_WORK_ITEM_STATUS_WAITING;
    }

    if state.completed {
        return if state.best_objective.is_some() {
            RESTART_WORK_ITEM_STATUS_COMPLETED
        } else {
            RESTART_WORK_ITEM_STATUS_REJECTED
        };
    }

    RESTART_WORK_ITEM_STATUS_QUEUED
}

fn active_restart_count(states: &[RestartProbeState]) -> usize {
    states.iter().filter(|state| !state.completed).count()
}

fn completed_restart_probe_state_count(states: &[RestartProbeState]) -> usize {
    states.iter().filter(|state| state.completed).count()
}

fn best_restart_probe_state(states: &[RestartProbeState]) -> Option<(String, f64)> {
    // The best restart id is copied into progress payloads that outlive this borrowed state view.
    states
        .iter()
        .filter_map(|state| {
            state
                .best_objective
                .map(|objective| (state.restart_id.clone(), objective))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
}

fn restart_outcome_status(outcome: &JointRefinementRestartOutcome) -> &'static str {
    if outcome.final_evaluation.invalid_reason.is_some() {
        return RESTART_WORK_ITEM_STATUS_REJECTED;
    }

    RESTART_WORK_ITEM_STATUS_COMPLETED
}

fn best_partial_restart_outcome(
    restart_outcomes: &[Option<JointRefinementRestartOutcome>],
) -> Option<(String, f64)> {
    // The best restart id is copied into progress payloads that outlive this borrowed state view.
    restart_outcomes
        .iter()
        .filter_map(Option::as_ref)
        .map(|outcome| {
            (
                outcome.restart_id.clone(),
                outcome.final_evaluation.breakdown.global_loss,
            )
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
}

fn best_restart_outcome(
    restart_outcomes: &[JointRefinementRestartOutcome],
) -> Option<(String, f64)> {
    // The best restart id is copied into progress payloads that outlive this borrowed state view.
    restart_outcomes
        .iter()
        .map(|outcome| {
            (
                outcome.restart_id.clone(),
                outcome.final_evaluation.breakdown.global_loss,
            )
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
}
