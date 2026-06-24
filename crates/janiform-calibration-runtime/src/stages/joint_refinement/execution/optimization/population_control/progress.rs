use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::JointRefinementStageCandidateEvaluationTimingDiagnostic;
use janiform_foundations::progress::WorkItemStatus;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};

use super::super::super::JointRefinementCandidateOutcome;
use super::super::evaluation_timing::{
    CandidateEvaluationTimingDiagnosticInput, RESTART_SEARCH_FIDELITY_TIER_LABEL,
    candidate_evaluation_timing_diagnostic,
};
use super::super::workflow::emit_joint_refinement_progress;
use super::POPULATION_CONTROL_PROGRESS_LABEL;
use super::policy::population_control_started_and_completed_progress_units;
use super::state::PopulationControlState;
use super::types::{
    PopulationControlFitSnapshot, PopulationControlSearchJob, PopulationControlSearchResult,
};

#[derive(Debug, Clone, Copy)]
struct PopulationControlProgressSnapshots<'a> {
    latest_score: Option<&'a PopulationControlFitSnapshot>,
    best_global: Option<&'a PopulationControlFitSnapshot>,
    exact_counts: Option<crate::stages::return_components::ContinuousLikelihoodEvaluatorCounts>,
}

pub(super) fn population_control_evaluation_timing_diagnostic(
    result: &PopulationControlSearchResult,
) -> JointRefinementStageCandidateEvaluationTimingDiagnostic {
    candidate_evaluation_timing_diagnostic(CandidateEvaluationTimingDiagnosticInput {
        candidate_id: result.candidate_id.as_str(),
        schedule_category: result.schedule_category,
        fidelity_tier: RESTART_SEARCH_FIDELITY_TIER_LABEL,
        candidate_evaluation_count: result.candidate_evaluation_count,
        search_direction_position: result.search_direction_position,
        search_direction_label: result.search_direction_label.as_str(),
        elapsed_millis: result.elapsed_millis,
        evaluation: result.evaluation.as_ref(),
    })
}

pub(super) fn emit_population_control_search_scheduled_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    state: &PopulationControlState,
    _search_job: &PopulationControlSearchJob,
    search_budget: usize,
    total_progress_units: usize,
) {
    emit_population_control_running_progress(
        progress_sender,
        state,
        search_budget,
        total_progress_units,
        None,
    );
}

pub(super) fn emit_population_control_search_completed_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    state: &PopulationControlState,
    search_budget: usize,
    total_progress_units: usize,
    candidate_evaluation_timing_diagnostic: Option<
        JointRefinementStageCandidateEvaluationTimingDiagnostic,
    >,
) {
    emit_population_control_running_progress(
        progress_sender,
        state,
        search_budget,
        total_progress_units,
        candidate_evaluation_timing_diagnostic,
    );
}

pub(super) fn emit_population_control_finalization_scheduled_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    state: &PopulationControlState,
    search_budget: usize,
    total_progress_units: usize,
) {
    emit_population_control_running_progress(
        progress_sender,
        state,
        search_budget,
        total_progress_units,
        None,
    );
}

pub(super) fn emit_population_control_finalization_completed_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    state: &PopulationControlState,
    outcome: &JointRefinementCandidateOutcome,
    total_progress_units: usize,
) {
    let aggregate_progress_percent =
        population_control_aggregate_progress_percent(state, total_progress_units);

    let snapshots = population_control_progress_snapshots(state);

    // Progress payloads own candidate labels so UI consumers are decoupled from scheduler state.
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(aggregate_progress_percent),
            current_item_label: Some(POPULATION_CONTROL_PROGRESS_LABEL.to_string()),
            current_candidate_id: snapshots
                .latest_score
                .map(|snapshot| snapshot.candidate_id.clone()),
            current_work_item_status: Some(WorkItemStatus::Running.as_str().to_string()),
            active_candidate_count: Some(
                state
                    .candidate_count()
                    .saturating_sub(state.completed_finalization_count()),
            ),
            current_evaluation_count: Some(state.scheduled_search_evaluation_count()),
            total_evaluation_budget: Some(state.scheduled_search_evaluation_count().max(1)),
            evaluation_progress_percent: Some(100),
            cumulative_local_evaluation_count: Some(state.completed_search_evaluation_count()),
            current_global_loss: snapshots.latest_score.map(|snapshot| snapshot.objective),
            current_candidate_best_global_loss: snapshots
                .best_global
                .map(|snapshot| snapshot.objective),
            current_final_global_loss: Some(outcome.final_evaluation.breakdown.global_loss),
            best_candidate_id: snapshots
                .best_global
                .map(|snapshot| snapshot.candidate_id.clone()),
            best_global_loss: snapshots.best_global.map(|snapshot| snapshot.objective),
            exact_closed_form_buy_count: snapshots
                .exact_counts
                .map(|counts| counts.closed_form_buy),
            exact_closed_form_sell_count: snapshots
                .exact_counts
                .map(|counts| counts.closed_form_sell),
            exact_tilted_mixed_count: snapshots.exact_counts.map(|counts| counts.tilted_mixed),
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::PopulationRefinement,
                state.completed_finalization_count(),
                state.candidate_count(),
            )
        },
    );
}

pub(super) fn emit_population_control_finished(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    candidate_outcomes: &[JointRefinementCandidateOutcome],
) {
    // Completion reports both search and final validation loss because the two evaluations
    // can use different fidelity.
    let best_final = candidate_outcomes
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
                outcome.candidate_id.clone(),
                outcome.final_evaluation.breakdown.global_loss,
            )
        });
    let best_search = candidate_outcomes
        .iter()
        .filter(|outcome| outcome.local_evaluation.invalid_reason.is_none())
        .min_by(|left, right| {
            left.local_evaluation
                .breakdown
                .global_loss
                .total_cmp(&right.local_evaluation.breakdown.global_loss)
        })
        .map(|outcome| outcome.local_evaluation.breakdown.global_loss);

    // Progress payloads own candidate labels so UI consumers are decoupled from scheduler state.
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(100),
            current_item_label: Some(POPULATION_CONTROL_PROGRESS_LABEL.to_string()),
            current_work_item_status: Some(WorkItemStatus::Completed.as_str().to_string()),
            active_candidate_count: Some(0),
            valid_completed_units: Some(
                candidate_outcomes
                    .iter()
                    .filter(|outcome| outcome.final_evaluation.invalid_reason.is_none())
                    .count(),
            ),
            best_candidate_id: best_final
                .as_ref()
                .map(|(candidate_id, _)| candidate_id.clone()),
            current_candidate_best_global_loss: best_search,
            best_global_loss: best_final.map(|(_, objective)| objective),
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::PopulationRefinement,
                candidate_outcomes.len(),
                candidate_outcomes.len(),
            )
        },
    );
}

fn emit_population_control_running_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    state: &PopulationControlState,
    search_budget: usize,
    total_progress_units: usize,
    candidate_evaluation_timing_diagnostic: Option<
        JointRefinementStageCandidateEvaluationTimingDiagnostic,
    >,
) {
    let aggregate_progress_percent =
        population_control_aggregate_progress_percent(state, total_progress_units);

    let snapshots = population_control_progress_snapshots(state);

    // Progress payloads own candidate labels so UI consumers are decoupled from scheduler state.
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(aggregate_progress_percent),
            current_item_label: Some(POPULATION_CONTROL_PROGRESS_LABEL.to_string()),
            current_candidate_id: snapshots
                .latest_score
                .map(|snapshot| snapshot.candidate_id.clone()),
            current_work_item_status: Some(WorkItemStatus::Running.as_str().to_string()),
            active_candidate_count: Some(state.in_flight_search_count()),
            current_evaluation_count: Some(state.scheduled_search_evaluation_count()),
            total_evaluation_budget: Some(search_budget.max(1)),
            evaluation_progress_percent: Some(
                state
                    .scheduled_search_evaluation_count()
                    .saturating_mul(100)
                    .checked_div(search_budget.max(1))
                    .unwrap_or(0)
                    .min(99),
            ),
            cumulative_local_evaluation_count: Some(state.completed_search_evaluation_count()),
            current_global_loss: snapshots.latest_score.map(|snapshot| snapshot.objective),
            current_candidate_best_global_loss: snapshots
                .best_global
                .map(|snapshot| snapshot.objective),
            best_candidate_id: snapshots
                .best_global
                .map(|snapshot| snapshot.candidate_id.clone()),
            best_global_loss: snapshots.best_global.map(|snapshot| snapshot.objective),
            exact_closed_form_buy_count: snapshots
                .exact_counts
                .map(|counts| counts.closed_form_buy),
            exact_closed_form_sell_count: snapshots
                .exact_counts
                .map(|counts| counts.closed_form_sell),
            exact_tilted_mixed_count: snapshots.exact_counts.map(|counts| counts.tilted_mixed),
            candidate_evaluation_timing_diagnostic,
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::PopulationRefinement,
                state.completed_finalization_count(),
                state.candidate_count(),
            )
        },
    );
}

fn population_control_progress_snapshots(
    state: &PopulationControlState,
) -> PopulationControlProgressSnapshots<'_> {
    let latest_score = state.latest_score_snapshot();
    let best_score = state.best_score_snapshot();
    let best_global = state.best_global_snapshot().or(best_score);
    let exact_counts = latest_score.and_then(|snapshot| snapshot.exact_likelihood_evaluator_counts);

    PopulationControlProgressSnapshots {
        latest_score,
        best_global,
        exact_counts,
    }
}

fn population_control_aggregate_progress_percent(
    state: &PopulationControlState,
    total_progress_units: usize,
) -> usize {
    population_control_started_and_completed_progress_units(
        state.scheduled_search_evaluation_count(),
        state.completed_search_evaluation_count(),
        state.scheduled_finalization_count(),
        state.completed_finalization_count(),
    )
    .saturating_mul(100)
    .checked_div(total_progress_units.max(1))
    .unwrap_or(0)
    .min(99)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::stages::joint_refinement::execution::test_joint_candidate_outcome;

    use super::super::state::population_control_state_with_progress_snapshots_for_test;
    use super::{
        emit_population_control_finished, emit_population_control_search_completed_progress,
    };

    #[test]
    fn search_progress_reports_latest_trial_and_best_global_loss_separately() {
        let (sender, receiver) = mpsc::channel();
        let state = population_control_state_with_progress_snapshots_for_test(12.0, 5.0, 3.0);

        emit_population_control_search_completed_progress(Some(&sender), &state, 10, 20, None);

        let progress = receiver
            .recv()
            .expect("progress event should be emitted for completed population search");

        assert_eq!(
            progress.current_item_label.as_deref(),
            Some("population refinement")
        );
        assert_eq!(
            progress.current_candidate_id.as_deref(),
            Some("latest_candidate")
        );
        assert_eq!(progress.current_global_loss, Some(12.0));
        assert_eq!(progress.current_candidate_best_global_loss, Some(3.0));
        assert_eq!(
            progress.best_candidate_id.as_deref(),
            Some("best_global_candidate")
        );
        assert_eq!(progress.best_global_loss, Some(3.0));
    }

    #[test]
    fn finished_progress_keeps_search_and_final_validation_losses_separate() {
        let (sender, receiver) = mpsc::channel();
        let mut search_best = test_joint_candidate_outcome("search_best", 10.0);
        search_best.local_evaluation.breakdown.global_loss = 4.0;
        search_best.final_evaluation.breakdown.global_loss = 12.0;

        let mut final_best = test_joint_candidate_outcome("final_best", 8.0);
        final_best.local_evaluation.breakdown.global_loss = 6.0;
        final_best.final_evaluation.breakdown.global_loss = 8.0;

        emit_population_control_finished(Some(&sender), &[search_best, final_best]);

        let progress = receiver
            .recv()
            .expect("progress event should be emitted for completed population search");

        assert_eq!(progress.current_candidate_best_global_loss, Some(4.0));
        assert_eq!(progress.best_global_loss, Some(8.0));
        assert_eq!(progress.best_candidate_id.as_deref(), Some("final_best"));
    }
}
