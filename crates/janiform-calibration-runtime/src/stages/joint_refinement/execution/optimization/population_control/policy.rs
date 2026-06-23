use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::numeric::ceil_integer_square_root;

use super::super::super::JointRefinementContext;
use super::super::workflow::{
    JointRefinementBudgetSchedule, restart_candidate_total_evaluation_budget,
};

// A generation keeps one running wave and one queued wave per worker. This keeps
// CPU workers fed while leaving room for generation-level distribution updates.
const POPULATION_CONTROL_WORKER_PIPELINE_WAVE_COUNT: usize = 2;

pub(super) fn population_control_search_budget(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
) -> usize {
    let fixed_restart_budget = budget_schedule.restart_count.max(1).saturating_mul(
        restart_candidate_total_evaluation_budget(context, solver_settings, budget_schedule),
    );

    ((fixed_restart_budget as f64)
        * budget_schedule
            .calibration_effort
            .restart_pool_total_budget_scale)
        .ceil()
        .max(1.0) as usize
}

pub(super) fn population_control_generation_width(
    restart_count: usize,
    worker_capacity: usize,
    remaining_search_budget: usize,
) -> usize {
    let worker_pipeline_width = worker_capacity
        .max(1)
        .saturating_mul(POPULATION_CONTROL_WORKER_PIPELINE_WAVE_COUNT);

    restart_count
        .max(worker_pipeline_width)
        .max(1)
        .min(remaining_search_budget.max(1))
}

pub(super) fn population_control_elite_count(generation_width: usize) -> usize {
    ceil_integer_square_root(generation_width.max(1)).min(generation_width.max(1))
}

pub(super) fn population_control_total_progress_units(
    search_budget: usize,
    finalization_count: usize,
) -> usize {
    search_budget
        .saturating_add(finalization_count)
        .max(1)
        .saturating_mul(2)
}

pub(super) fn population_control_started_and_completed_progress_units(
    scheduled_search_count: usize,
    completed_search_count: usize,
    scheduled_finalization_count: usize,
    completed_finalization_count: usize,
) -> usize {
    scheduled_search_count
        .saturating_add(completed_search_count)
        .saturating_add(scheduled_finalization_count)
        .saturating_add(completed_finalization_count)
}

#[cfg(test)]
mod tests {
    use super::{
        population_control_elite_count, population_control_generation_width,
        population_control_started_and_completed_progress_units,
        population_control_total_progress_units,
    };

    #[test]
    fn generation_width_fills_workers_and_respects_remaining_budget() {
        assert_eq!(population_control_generation_width(3, 4, 100), 8);
        assert_eq!(population_control_generation_width(12, 4, 100), 12);
        assert_eq!(population_control_generation_width(12, 4, 5), 5);
    }

    #[test]
    fn elite_count_is_square_root_generation_width() {
        assert_eq!(population_control_elite_count(1), 1);
        assert_eq!(population_control_elite_count(8), 3);
        assert_eq!(population_control_elite_count(16), 4);
    }

    #[test]
    fn progress_units_advance_for_scheduled_and_completed_work() {
        assert_eq!(population_control_total_progress_units(10, 4), 28);
        assert_eq!(
            population_control_started_and_completed_progress_units(3, 2, 1, 0),
            6
        );
    }
}
