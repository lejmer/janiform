use std::sync::mpsc::Sender;

use crate::stages::joint_refinement::contracts::JointRefinementStageProgressUpdate;

use super::progress::{
    emit_population_control_finalization_completed_progress,
    emit_population_control_finalization_scheduled_progress,
};
use super::types::{
    PopulationControlFinalizationJob, PopulationControlFinalizationResult,
    PopulationControlWorkerPoolState,
};

pub(super) fn next_population_control_finalization_job(
    pool_state: &mut PopulationControlWorkerPoolState<'_>,
    search_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Option<PopulationControlFinalizationJob> {
    let finalization_job = next_population_control_finalization_candidate(pool_state)?;

    schedule_population_control_finalization_job(
        pool_state,
        finalization_job,
        search_budget,
        total_progress_units,
        progress_sender,
    )
}

pub(super) fn record_population_control_finalization_result(
    pool_state: &mut PopulationControlWorkerPoolState<'_>,
    result: PopulationControlFinalizationResult,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) {
    let restart_position = result.restart_position;

    let Some(outcome_slot) = pool_state.restart_outcomes.get_mut(restart_position) else {
        return;
    };

    if outcome_slot.is_some() {
        return;
    }

    *outcome_slot = Some(result.outcome);

    pool_state.state.record_finalization_completed();

    if let Some(outcome) = pool_state
        .restart_outcomes
        .get(restart_position)
        .and_then(Option::as_ref)
    {
        emit_population_control_finalization_completed_progress(
            progress_sender,
            pool_state.state,
            outcome,
            total_progress_units,
        );
    }
}

fn schedule_population_control_finalization_job(
    pool_state: &mut PopulationControlWorkerPoolState<'_>,
    finalization_job: PopulationControlFinalizationJob,
    search_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Option<PopulationControlFinalizationJob> {
    pool_state.state.record_finalization_scheduled();

    emit_population_control_finalization_scheduled_progress(
        progress_sender,
        pool_state.state,
        search_budget,
        total_progress_units,
    );

    Some(finalization_job)
}

fn next_population_control_finalization_candidate(
    pool_state: &mut PopulationControlWorkerPoolState<'_>,
) -> Option<PopulationControlFinalizationJob> {
    if pool_state.state.restart_count() == 0 {
        return None;
    }

    for _ in 0..pool_state.state.restart_count() {
        let restart_position =
            pool_state.next_finalization_restart_position % pool_state.state.restart_count();

        pool_state.next_finalization_restart_position = pool_state
            .next_finalization_restart_position
            .saturating_add(1)
            % pool_state.state.restart_count();

        if !population_control_finalization_is_schedulable(pool_state, restart_position) {
            continue;
        }

        let Some(finalization_job) = pool_state
            .state
            .finalization_job_for_restart(restart_position)
        else {
            continue;
        };

        if let Some(finalization_scheduled) =
            pool_state.finalization_scheduled.get_mut(restart_position)
        {
            *finalization_scheduled = true;
        }

        return Some(finalization_job);
    }

    None
}

fn population_control_finalization_is_schedulable(
    pool_state: &PopulationControlWorkerPoolState<'_>,
    restart_position: usize,
) -> bool {
    if pool_state
        .restart_outcomes
        .get(restart_position)
        .is_none_or(Option::is_some)
    {
        return false;
    }

    if pool_state
        .finalization_scheduled
        .get(restart_position)
        .copied()
        .unwrap_or(true)
    {
        return false;
    }

    if pool_state
        .pending_search_jobs
        .iter()
        .any(|candidate_spec| candidate_spec.restart_position == restart_position)
    {
        return false;
    }

    if !pool_state.state.restart_search_idle(restart_position) {
        return false;
    }

    pool_state.state.search_closed()
        || pool_state
            .state
            .restart_search_budget_exhausted(restart_position)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::super::POPULATION_CONTROL_SCHEDULE_CATEGORY_SAMPLE;
    use super::super::state::population_control_state_with_progress_snapshots_for_test;
    use super::super::types::{PopulationControlCandidateSpec, PopulationControlWorkerPoolState};
    use super::next_population_control_finalization_job;

    fn test_pool_state<'a>(
        state: &'a mut super::super::state::PopulationControlState,
        evaluation_timing_diagnostics: &'a mut Vec<
            janiform_calibration_artifacts::JointRefinementStageRestartEvaluationTimingDiagnostic,
        >,
    ) -> PopulationControlWorkerPoolState<'a> {
        let restart_count = state.restart_count();

        PopulationControlWorkerPoolState {
            state,
            worker_capacity: 2,
            pending_search_jobs: VecDeque::new(),
            restart_outcomes: std::iter::repeat_with(|| None)
                .take(restart_count)
                .collect(),
            next_finalization_restart_position: 0,
            finalization_scheduled: vec![false; restart_count],
            evaluation_timing_diagnostics,
        }
    }

    #[test]
    fn exhausted_idle_restart_schedules_finalization_before_search_closes() {
        let mut state = population_control_state_with_progress_snapshots_for_test(3.0, 2.0, 1.0);
        let mut evaluation_timing_diagnostics = Vec::new();
        let mut pool_state = test_pool_state(&mut state, &mut evaluation_timing_diagnostics);

        let finalization_job =
            next_population_control_finalization_job(&mut pool_state, 12, 30, None)
                .expect("exhausted idle restart should schedule finalization");

        assert_eq!(finalization_job.restart_position, 0);
        assert_eq!(pool_state.finalization_scheduled, vec![true, false, false]);
        assert_eq!(pool_state.state.scheduled_finalization_count(), 1);
    }

    #[test]
    fn pending_search_job_blocks_same_restart_finalization() {
        let mut state = population_control_state_with_progress_snapshots_for_test(3.0, 2.0, 1.0);
        let mut evaluation_timing_diagnostics = Vec::new();
        let mut pool_state = test_pool_state(&mut state, &mut evaluation_timing_diagnostics);

        pool_state
            .pending_search_jobs
            .push_back(PopulationControlCandidateSpec {
                restart_position: 0,
                restart_id: "latest_restart".to_string(),
                parameter_vector: vec![1.0],
                schedule_category: POPULATION_CONTROL_SCHEDULE_CATEGORY_SAMPLE,
                search_direction_position: Some(0),
                search_direction_label: "population_candidate".to_string(),
            });

        let finalization_job =
            next_population_control_finalization_job(&mut pool_state, 12, 30, None)
                .expect("next idle exhausted restart should still schedule finalization");

        assert_eq!(finalization_job.restart_position, 1);
        assert_eq!(pool_state.finalization_scheduled, vec![false, true, false]);
    }
}
