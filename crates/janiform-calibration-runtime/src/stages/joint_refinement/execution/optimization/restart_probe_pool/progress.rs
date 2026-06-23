use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::JointRefinementStageRestartEvaluationTimingDiagnostic;
use janiform_foundations::progress::WorkItemStatus;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};

use super::super::super::{
    JointRefinementRestartOutcome, best_joint_restart_outcome,
    joint_restart_outcome_has_better_final_loss,
};
use super::super::workflow::emit_joint_refinement_progress;
use super::{RestartProbeResult, RestartProbeState};

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
        search_direction_position: result.search_direction_position,
        search_direction_label: result.search_direction_label.clone(),
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
                restart_probe_state_status(current_state)
                    .as_str()
                    .to_string(),
            ),
            active_restart_count: Some(active_restart_count(states)),
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
            current_global_loss: current_objective,
            current_restart_best_global_loss: restart_probe_state_best_loss(current_state),
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
            current_work_item_status: Some(restart_outcome_status(outcome).as_str().to_string()),
            active_restart_count: Some(
                total_restart_count.saturating_sub(completed_finalization_count),
            ),
            current_evaluation_count: Some(outcome.local_evaluation_count),
            total_evaluation_budget: Some(completed_evaluation_count),
            evaluation_progress_percent: Some(100),
            cumulative_local_evaluation_count: Some(completed_search_evaluation_count),
            current_global_loss: None,
            current_restart_best_global_loss: None,
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

pub(super) struct RestartFinalizationStartedProgressInput<'a> {
    pub(super) progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(super) state: &'a RestartProbeState,
    pub(super) states: &'a [RestartProbeState],
    pub(super) restart_outcomes: &'a [Option<JointRefinementRestartOutcome>],
    pub(super) total_restart_count: usize,
    pub(super) completed_search_evaluation_count: usize,
    pub(super) completed_finalization_count: usize,
    pub(super) total_progress_units: usize,
}

pub(super) fn emit_restart_finalization_started(
    input: RestartFinalizationStartedProgressInput<'_>,
) {
    let RestartFinalizationStartedProgressInput {
        progress_sender,
        state,
        states,
        restart_outcomes,
        total_restart_count,
        completed_search_evaluation_count,
        completed_finalization_count,
        total_progress_units,
    } = input;

    let best =
        best_partial_restart_outcome(restart_outcomes).or_else(|| best_restart_probe_state(states));

    let completed_progress_units =
        completed_search_evaluation_count.saturating_add(completed_finalization_count);

    let aggregate_progress_percent = completed_progress_units
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
            current_work_item_status: Some(WorkItemStatus::Running.as_str().to_string()),
            active_restart_count: Some(
                total_restart_count.saturating_sub(completed_finalization_count),
            ),
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
            current_restart_best_global_loss: restart_probe_state_best_loss(state),
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
                0,
                total_restart_count,
            )
        },
    );
}

fn restart_probe_state_status(state: &RestartProbeState) -> WorkItemStatus {
    if state.in_flight_count > 0 {
        return WorkItemStatus::Running;
    }

    if state.evaluation_count == 0 {
        return WorkItemStatus::Waiting;
    }

    if state.completed {
        return if state.best_objective.is_some() {
            WorkItemStatus::Finishing
        } else {
            WorkItemStatus::Rejected
        };
    }

    WorkItemStatus::Queued
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
            restart_probe_state_best_loss(state)
                .map(|objective| (state.restart_id.clone(), objective))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
}

fn restart_probe_state_best_loss(state: &RestartProbeState) -> Option<f64> {
    [
        state.best_objective,
        state.round_best_objective,
        state.accepted_objective,
    ]
    .into_iter()
    .flatten()
    .filter(|objective| objective.is_finite())
    .min_by(|left, right| left.total_cmp(right))
}

fn restart_outcome_status(outcome: &JointRefinementRestartOutcome) -> WorkItemStatus {
    if outcome.final_evaluation.invalid_reason.is_some() {
        return WorkItemStatus::Rejected;
    }

    WorkItemStatus::Completed
}

fn best_partial_restart_outcome(
    restart_outcomes: &[Option<JointRefinementRestartOutcome>],
) -> Option<(String, f64)> {
    // The best restart id is copied into progress payloads that outlive this borrowed state view.
    let best_outcome = restart_outcomes.iter().filter_map(Option::as_ref).reduce(
        |best_outcome, candidate_outcome| {
            if joint_restart_outcome_has_better_final_loss(candidate_outcome, best_outcome) {
                candidate_outcome
            } else {
                best_outcome
            }
        },
    )?;

    Some((
        best_outcome.restart_id.clone(),
        best_outcome.final_evaluation.breakdown.global_loss,
    ))
}

fn best_restart_outcome(
    restart_outcomes: &[JointRefinementRestartOutcome],
) -> Option<(String, f64)> {
    // The best restart id is copied into progress payloads that outlive this borrowed state view.
    let best_outcome = best_joint_restart_outcome(restart_outcomes)?;

    Some((
        best_outcome.restart_id.clone(),
        best_outcome.final_evaluation.breakdown.global_loss,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use janiform_calibration_artifacts::JointRefinementStageRestartEvaluationTimingDiagnostic;
    use janiform_foundations::progress::WorkItemStatus;

    use crate::stages::joint_refinement::execution::test_joint_restart_outcome;

    use super::super::RestartProbeState;
    use super::{
        RestartFinalizationStartedProgressInput, emit_batched_restart_progress,
        emit_restart_finalization_progress, emit_restart_finalization_started,
        restart_probe_state_status,
    };

    fn completed_search_state() -> RestartProbeState {
        RestartProbeState {
            restart_id: "tick_moment_04".to_string(),
            seed_vector: vec![0.0],
            accepted_vector: vec![0.0],
            best_vector: vec![0.0],
            initial_objective: Some(34.301131),
            accepted_objective: Some(34.301131),
            best_objective: Some(34.301131),
            round_best_vector: None,
            round_best_objective: None,
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
            evaluation_count: 146,
            max_evaluation_count: 146,
            in_flight_count: 0,
            stop_reason: "restart_pool_budget".to_string(),
            completed: true,
            exact_likelihood_evaluator_counts: None,
        }
    }

    #[test]
    fn completed_probe_search_waits_for_finalization_before_completed_status() {
        let state = completed_search_state();

        assert_eq!(
            restart_probe_state_status(&state),
            WorkItemStatus::Finishing
        );
    }

    #[test]
    fn running_probe_progress_reports_best_seen_restart_loss() {
        let (progress_sender, progress_receiver) = mpsc::channel();
        let mut state = completed_search_state();
        state.completed = false;
        state.in_flight_count = 1;
        state.accepted_objective = Some(10.0);
        state.round_best_objective = Some(6.0);
        state.best_objective = Some(4.0);

        emit_batched_restart_progress(
            Some(&progress_sender),
            &[state],
            0,
            1,
            10,
            Some(JointRefinementStageRestartEvaluationTimingDiagnostic {
                restart_id: "tick_moment_04".to_string(),
                schedule_category: "model_probe".to_string(),
                candidate_evaluation_count: 7,
                search_direction_position: Some(2),
                search_direction_label: "interpolation.nig_alpha.positive".to_string(),
                elapsed_millis: 12,
                objective: Some(5.0),
                admissibility_blocker_count: Some(0),
                max_standardized_residual: Some(1.0),
                exact_closed_form_buy_count: Some(3),
                exact_closed_form_sell_count: Some(4),
                exact_tilted_mixed_count: Some(5),
            }),
        );

        let progress_update = progress_receiver
            .recv()
            .expect("restart progress should be emitted");

        assert_eq!(progress_update.current_global_loss, Some(5.0));
        assert_eq!(progress_update.current_restart_best_global_loss, Some(4.0));
    }

    #[test]
    fn finalization_start_progress_preserves_visible_best_losses() {
        let (progress_sender, progress_receiver) = mpsc::channel();
        let mut state = completed_search_state();
        state.accepted_objective = Some(7.0);
        state.best_objective = Some(5.0);

        emit_restart_finalization_started(RestartFinalizationStartedProgressInput {
            progress_sender: Some(&progress_sender),
            state: &state,
            states: &[state.clone()],
            restart_outcomes: &[
                Some(test_joint_restart_outcome("weaker_restart", 11.0)),
                Some(test_joint_restart_outcome("best_restart", 3.0)),
                None,
            ],
            total_restart_count: 3,
            completed_search_evaluation_count: 8,
            completed_finalization_count: 2,
            total_progress_units: 15,
        });

        let progress_update = progress_receiver
            .recv()
            .expect("finalization start progress should be emitted");

        assert_eq!(progress_update.current_restart_best_global_loss, Some(5.0));
        assert_eq!(
            progress_update.best_restart_id,
            Some("best_restart".to_string())
        );
        assert_eq!(progress_update.best_global_loss, Some(3.0));
    }

    #[test]
    fn finalization_start_progress_keeps_search_best_until_final_loss_exists() {
        let (progress_sender, progress_receiver) = mpsc::channel();
        let mut state = completed_search_state();
        state.best_objective = None;
        state.round_best_objective = Some(5.0);
        state.accepted_objective = Some(7.0);

        emit_restart_finalization_started(RestartFinalizationStartedProgressInput {
            progress_sender: Some(&progress_sender),
            state: &state,
            states: &[state.clone()],
            restart_outcomes: &[None],
            total_restart_count: 1,
            completed_search_evaluation_count: 8,
            completed_finalization_count: 0,
            total_progress_units: 9,
        });

        let progress_update = progress_receiver
            .recv()
            .expect("finalization start progress should be emitted");

        assert_eq!(
            progress_update.best_restart_id,
            Some("tick_moment_04".to_string())
        );
        assert_eq!(progress_update.best_global_loss, Some(5.0));
    }

    #[test]
    fn finalization_progress_reports_final_loss_without_search_loss_overwrite() {
        let (progress_sender, progress_receiver) = mpsc::channel();
        let mut outcome = test_joint_restart_outcome("tick_moment_04", 3.0);
        outcome.local_evaluation.breakdown.global_loss = 5.0;
        outcome.final_evaluation.breakdown.global_loss = 4.0;

        emit_restart_finalization_progress(
            Some(&progress_sender),
            &outcome,
            &[Some(test_joint_restart_outcome("best_restart", 2.0))],
            1,
            1,
            8,
            9,
        );

        let progress_update = progress_receiver
            .recv()
            .expect("finalization progress should be emitted");

        assert_eq!(progress_update.current_global_loss, None);
        assert_eq!(progress_update.current_restart_best_global_loss, None);
        assert_eq!(progress_update.current_final_global_loss, Some(4.0));
        assert_eq!(
            progress_update.best_restart_id,
            Some("best_restart".to_string())
        );
        assert_eq!(progress_update.best_global_loss, Some(2.0));
    }
}
