use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::JointRefinementStageRestartEvaluationTimingDiagnostic;
use janiform_foundations::progress::WorkItemStatus;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};

use super::super::super::JointRefinementRestartOutcome;
use super::super::evaluation_timing::{
    RESTART_SEARCH_FIDELITY_TIER_LABEL, RestartEvaluationTimingDiagnosticInput,
    restart_evaluation_timing_diagnostic,
};
use super::super::workflow::emit_joint_refinement_progress;
use super::{ResidualControlResult, ResidualControlState};

pub(super) struct ResidualControlSearchProgressInput<'a> {
    pub(super) progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(super) states: &'a [ResidualControlState],
    pub(super) restart_outcomes: &'a [Option<JointRefinementRestartOutcome>],
    pub(super) current_restart_position: usize,
    pub(super) completed_search_evaluation_count: usize,
    pub(super) completed_finalization_count: usize,
    pub(super) total_progress_units: usize,
    pub(super) restart_evaluation_timing_diagnostic:
        Option<JointRefinementStageRestartEvaluationTimingDiagnostic>,
}

pub(super) fn residual_control_evaluation_timing_diagnostic(
    state: &ResidualControlState,
    result: &ResidualControlResult,
) -> JointRefinementStageRestartEvaluationTimingDiagnostic {
    restart_evaluation_timing_diagnostic(RestartEvaluationTimingDiagnosticInput {
        restart_id: state.restart_id.as_str(),
        schedule_category: result.schedule_category,
        fidelity_tier: RESTART_SEARCH_FIDELITY_TIER_LABEL,
        candidate_evaluation_count: result.candidate_evaluation_count,
        search_direction_position: result.search_direction_position,
        search_direction_label: result.search_direction_label.as_str(),
        elapsed_millis: result.elapsed_millis,
        evaluation: result.evaluation.as_ref(),
    })
}

pub(super) fn emit_residual_control_search_progress(input: ResidualControlSearchProgressInput<'_>) {
    let ResidualControlSearchProgressInput {
        progress_sender,
        states,
        restart_outcomes,
        current_restart_position,
        completed_search_evaluation_count,
        completed_finalization_count,
        total_progress_units,
        restart_evaluation_timing_diagnostic,
    } = input;

    let Some(current_state) = states.get(current_restart_position) else {
        return;
    };

    let best = best_partial_residual_control_outcome(restart_outcomes)
        .or_else(|| best_residual_control_state(states));

    let completed_progress_units =
        completed_search_evaluation_count.saturating_add(completed_finalization_count);

    let aggregate_progress_percent = completed_progress_units
        .saturating_mul(100)
        .checked_div(total_progress_units.max(1))
        .unwrap_or(0)
        .min(99);

    let current_objective = restart_evaluation_timing_diagnostic
        .as_ref()
        .and_then(|diagnostic| diagnostic.objective);

    // Progress payloads own restart labels so UI consumers are decoupled from scheduler state.
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(aggregate_progress_percent),
            current_item_label: Some(current_state.restart_id.clone()),
            current_restart_id: Some(current_state.restart_id.clone()),
            current_work_item_status: Some(
                residual_control_state_status(current_state)
                    .as_str()
                    .to_string(),
            ),
            active_restart_count: Some(active_residual_control_restart_count(states)),
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
            cumulative_local_evaluation_count: Some(completed_search_evaluation_count),
            current_global_loss: current_objective,
            current_restart_best_global_loss: residual_control_state_best_loss(current_state),
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
                completed_residual_control_state_count(states),
                states.len(),
            )
        },
    );
}

pub(super) fn emit_residual_control_finalization_started(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    state: &ResidualControlState,
    states: &[ResidualControlState],
    restart_outcomes: &[Option<JointRefinementRestartOutcome>],
    completed_search_evaluation_count: usize,
    completed_finalization_count: usize,
    total_progress_units: usize,
) {
    let best = best_partial_residual_control_outcome(restart_outcomes)
        .or_else(|| best_residual_control_state(states));

    let completed_progress_units =
        completed_search_evaluation_count.saturating_add(completed_finalization_count);

    let aggregate_progress_percent = completed_progress_units
        .saturating_mul(100)
        .checked_div(total_progress_units.max(1))
        .unwrap_or(0)
        .min(99);

    // Progress payloads own restart labels so UI consumers are decoupled from scheduler state.
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(aggregate_progress_percent),
            current_item_label: Some(state.restart_id.clone()),
            current_restart_id: Some(state.restart_id.clone()),
            current_work_item_status: Some(WorkItemStatus::Running.as_str().to_string()),
            active_restart_count: Some(states.len().saturating_sub(completed_finalization_count)),
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
            current_global_loss: None,
            current_restart_best_global_loss: residual_control_state_best_loss(state),
            best_restart_id: best.as_ref().map(|(restart_id, _)| restart_id.clone()),
            best_global_loss: best.map(|(_, objective)| objective),
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
                completed_finalization_count,
                states.len(),
            )
        },
    );
}

pub(super) fn emit_residual_control_finalization_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    outcome: &JointRefinementRestartOutcome,
    restart_outcomes: &[Option<JointRefinementRestartOutcome>],
    completed_search_evaluation_count: usize,
    completed_finalization_count: usize,
    total_restart_count: usize,
    total_progress_units: usize,
) {
    let best = best_partial_residual_control_outcome(restart_outcomes);

    let completed_progress_units =
        completed_search_evaluation_count.saturating_add(completed_finalization_count);

    let aggregate_progress_percent = completed_progress_units
        .saturating_mul(100)
        .checked_div(total_progress_units.max(1))
        .unwrap_or(0)
        .min(99);

    // Progress payloads own restart labels so UI consumers are decoupled from scheduler state.
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(aggregate_progress_percent),
            current_item_label: Some(outcome.restart_id.clone()),
            current_restart_id: Some(outcome.restart_id.clone()),
            current_work_item_status: Some(
                residual_control_outcome_status(outcome)
                    .as_str()
                    .to_string(),
            ),
            active_restart_count: Some(
                total_restart_count.saturating_sub(completed_finalization_count),
            ),
            current_evaluation_count: Some(outcome.local_evaluation_count),
            total_evaluation_budget: Some(outcome.local_evaluation_count.max(1)),
            evaluation_progress_percent: Some(100),
            cumulative_local_evaluation_count: Some(completed_search_evaluation_count),
            current_final_global_loss: Some(outcome.final_evaluation.breakdown.global_loss),
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

pub(super) fn emit_residual_control_finished(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    restart_outcomes: &[JointRefinementRestartOutcome],
) {
    let best = restart_outcomes
        .iter()
        .filter(|outcome| outcome.final_evaluation.invalid_reason.is_none())
        .min_by(|left, right| {
            left.final_evaluation
                .breakdown
                .global_loss
                .total_cmp(&right.final_evaluation.breakdown.global_loss)
        })
        .map(|outcome| {
            (
                outcome.restart_id.clone(),
                outcome.final_evaluation.breakdown.global_loss,
            )
        });

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

fn residual_control_state_status(state: &ResidualControlState) -> WorkItemStatus {
    if state.in_flight_count > 0 {
        return WorkItemStatus::Running;
    }

    if state.evaluation_count == 0 {
        return WorkItemStatus::Waiting;
    }

    if state.completed {
        return if state.best_global_objective.is_some() {
            WorkItemStatus::Finishing
        } else {
            WorkItemStatus::Rejected
        };
    }

    WorkItemStatus::Queued
}

fn residual_control_outcome_status(outcome: &JointRefinementRestartOutcome) -> WorkItemStatus {
    if outcome.final_evaluation.invalid_reason.is_some() {
        return WorkItemStatus::Rejected;
    }

    WorkItemStatus::Completed
}

fn active_residual_control_restart_count(states: &[ResidualControlState]) -> usize {
    states.iter().filter(|state| !state.completed).count()
}

fn completed_residual_control_state_count(states: &[ResidualControlState]) -> usize {
    states.iter().filter(|state| state.completed).count()
}

fn best_residual_control_state(states: &[ResidualControlState]) -> Option<(String, f64)> {
    states
        .iter()
        .filter_map(|state| {
            residual_control_state_best_loss(state)
                .map(|objective| (state.restart_id.clone(), objective))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
}

fn residual_control_state_best_loss(state: &ResidualControlState) -> Option<f64> {
    [
        state.best_global_objective,
        state.round_best_objective,
        state.accepted_objective,
    ]
    .into_iter()
    .flatten()
    .filter(|objective| objective.is_finite())
    .min_by(|left, right| left.total_cmp(right))
}

fn best_partial_residual_control_outcome(
    restart_outcomes: &[Option<JointRefinementRestartOutcome>],
) -> Option<(String, f64)> {
    restart_outcomes
        .iter()
        .filter_map(Option::as_ref)
        .filter(|outcome| outcome.final_evaluation.invalid_reason.is_none())
        .min_by(|left, right| {
            left.final_evaluation
                .breakdown
                .global_loss
                .total_cmp(&right.final_evaluation.breakdown.global_loss)
        })
        .map(|outcome| {
            (
                outcome.restart_id.clone(),
                outcome.final_evaluation.breakdown.global_loss,
            )
        })
}
