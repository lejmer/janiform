use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use super::super::super::JointRefinementContext;
use super::POPULATION_CONTROL_STOP_REASON_NO_SCHEDULABLE_WORK;
use super::policy::population_control_generation_width;
use super::types::{PopulationControlSearchJob, PopulationControlWorkerPoolState};

pub(super) fn next_population_control_search_job(
    pool_state: &mut PopulationControlWorkerPoolState<'_>,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    search_budget: usize,
) -> Option<PopulationControlSearchJob> {
    if let Some(search_job) = next_schedulable_population_search_job(pool_state) {
        return Some(search_job);
    }

    if pool_state.state.search_closed() {
        return None;
    }

    let remaining_search_budget =
        search_budget.saturating_sub(pool_state.state.scheduled_search_evaluation_count());
    if remaining_search_budget == 0 {
        return None;
    }

    let generation_width = population_control_generation_width(
        pool_state.state.restart_count(),
        pool_state.worker_capacity,
        remaining_search_budget,
    );

    let candidates = pool_state.state.prepare_population_candidates(
        context,
        solver_settings,
        generation_width,
        remaining_search_budget,
    );

    if candidates.is_empty() {
        pool_state
            .state
            .mark_search_closed(POPULATION_CONTROL_STOP_REASON_NO_SCHEDULABLE_WORK);
        return None;
    }

    pool_state.pending_search_jobs.extend(candidates);

    next_schedulable_population_search_job(pool_state)
}

pub(super) fn population_control_pool_completed(
    pool_state: &PopulationControlWorkerPoolState<'_>,
) -> bool {
    pool_state.restart_outcomes.iter().all(Option::is_some)
}

fn next_schedulable_population_search_job(
    pool_state: &mut PopulationControlWorkerPoolState<'_>,
) -> Option<PopulationControlSearchJob> {
    while let Some(candidate_spec) = pool_state.pending_search_jobs.pop_front() {
        if let Some(search_job) = pool_state.state.schedule_search_job(candidate_spec) {
            return Some(search_job);
        }
    }

    None
}
