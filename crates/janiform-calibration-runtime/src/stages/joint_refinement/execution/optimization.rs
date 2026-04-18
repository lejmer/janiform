use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use crate::stages::optimization::{PowellOptimizerSettings, optimize_bounded_powell};

use super::super::objective::{
    evaluate_joint_candidate, evaluate_joint_candidate_nig_block_search_objective,
};
use super::{
    JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT, JointRefinementContext, JointRefinementFidelityTier,
    JointRefinementRestartOutcome, option_pricing_execution_failure_message,
    simulation_infrastructure_failure_message,
};
pub(crate) const JOINT_REFINEMENT_DEFAULT_RESTART_COUNT: usize = 15;

#[derive(Debug, Clone, Copy)]
pub(crate) struct JointRefinementBudgetSchedule {
    pub(crate) restart_count: usize,
    pub(crate) outer_cycle_count: usize,
}

#[derive(Debug, Default)]
struct RestartProgressState {
    completed_units: usize,
    cumulative_local_evaluation_count: usize,
    valid_completed_units: usize,
}

#[derive(Debug, Default)]
struct RestartDisplayBestState {
    best_restart_id: Option<String>,
    best_weighted_objective: Option<f64>,
}

#[derive(Debug, Default)]
struct RestartProgressCounters {
    completed_units: AtomicUsize,
    active_restart_count: AtomicUsize,
    aggregate_progress_percent_sum: AtomicUsize,
    display_best_state: Mutex<RestartDisplayBestState>,
}

struct RestartCandidateProgress<'a> {
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    restart_id: &'a str,
    total_units: usize,
    counters: &'a RestartProgressCounters,
    total_outer_cycles: usize,
    total_evaluation_budget: usize,
    current_evaluation_count: usize,
    last_evaluation_progress_percent: Option<usize>,
    current_restart_best_weighted_objective: Option<f64>,
    finished: bool,
}

pub(crate) fn optimize_restart_seeds(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_restart_store: Option<&mut super::JointRefinementResumeStore>,
) -> Vec<JointRefinementRestartOutcome> {
    debug_assert_eq!(restart_seeds.len().max(1), budget_schedule.restart_count);

    let total_units = restart_seeds.len();

    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(0),
            valid_completed_units: Some(0),
            cumulative_local_evaluation_count: Some(0),
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::RestartOptimization,
                0,
                total_units,
            )
        },
    );

    if !joint_refinement_has_parallel_worker_capacity(context) || total_units <= 1 {
        return optimize_restart_seeds_sequential(
            restart_seeds,
            context,
            solver_settings,
            budget_schedule,
            progress_sender,
            completed_restart_store,
        );
    }

    optimize_restart_seeds_parallel(
        restart_seeds,
        context,
        solver_settings,
        budget_schedule,
        progress_sender,
        completed_restart_store,
    )
}

fn optimize_restart_seeds_sequential(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut completed_restart_store: Option<&mut super::JointRefinementResumeStore>,
) -> Vec<JointRefinementRestartOutcome> {
    let total_units = restart_seeds.len();

    let mut progress_state = RestartProgressState::default();
    let progress_counters = RestartProgressCounters::default();
    let mut ordered_outcomes = Vec::with_capacity(total_units);

    for (restart_id, seed_vector) in restart_seeds {
        if joint_refinement_has_fatal_execution_failure(context) {
            break;
        }

        let restart_outcome = optimize_single_restart(
            restart_id.as_str(),
            seed_vector.as_slice(),
            context,
            solver_settings,
            budget_schedule,
            total_units,
            &progress_counters,
            progress_sender,
        );

        progress_state.record(&restart_outcome);
        progress_counters
            .completed_units
            .store(progress_state.completed_units, Ordering::Relaxed);
        progress_counters.record_display_best(
            restart_outcome.restart_id.as_str(),
            restart_outcome
                .final_evaluation
                .breakdown
                .weighted_objective,
        );

        emit_restart_progress(
            progress_sender,
            total_units,
            &progress_state,
            &restart_outcome,
            &progress_counters,
            progress_counters
                .active_restart_count
                .load(Ordering::Relaxed),
            restart_candidate_total_evaluation_budget(context, solver_settings, budget_schedule),
            progress_counters
                .aggregate_progress_percent_sum
                .load(Ordering::Relaxed)
                .checked_div(total_units.max(1))
                .unwrap_or(0)
                .min(100),
        );

        if let Some(resume_store) = completed_restart_store.as_deref_mut() {
            resume_store.record_restart_outcome(&restart_outcome);
        }

        ordered_outcomes.push(restart_outcome);
    }

    ordered_outcomes
}

fn optimize_restart_seeds_parallel(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut completed_restart_store: Option<&mut super::JointRefinementResumeStore>,
) -> Vec<JointRefinementRestartOutcome> {
    let total_units = restart_seeds.len();

    with_joint_refinement_thread_pool(context, || {
        let (result_sender, result_receiver) =
            mpsc::channel::<(usize, JointRefinementRestartOutcome)>();
        let result_receiver = Arc::new(Mutex::new(result_receiver));
        let progress_counters = Arc::new(RestartProgressCounters::default());

        rayon::scope(|scope| {
            for (restart_position, (restart_id, seed_vector)) in restart_seeds.iter().enumerate() {
                let result_sender_for_restart = result_sender.clone();
                let progress_counters_for_restart = Arc::clone(&progress_counters);
                scope.spawn(move |_| {
                    let outcome = optimize_single_restart(
                        restart_id.as_str(),
                        seed_vector.as_slice(),
                        context,
                        solver_settings,
                        budget_schedule,
                        total_units,
                        progress_counters_for_restart.as_ref(),
                        progress_sender,
                    );
                    let _ = result_sender_for_restart.send((restart_position, outcome));
                });
            }

            drop(result_sender);

            let mut progress_state = RestartProgressState::default();
            let mut ordered_outcomes = vec![None; total_units];

            while progress_state.completed_units < total_units {
                let Ok(receiver_guard) = result_receiver.lock() else {
                    break;
                };
                let Ok((restart_position, restart_outcome)) = receiver_guard.recv() else {
                    break;
                };
                drop(receiver_guard);

                progress_state.record(&restart_outcome);
                progress_counters
                    .completed_units
                    .store(progress_state.completed_units, Ordering::Relaxed);
                progress_counters.record_display_best(
                    restart_outcome.restart_id.as_str(),
                    restart_outcome
                        .final_evaluation
                        .breakdown
                        .weighted_objective,
                );
                emit_restart_progress(
                    progress_sender,
                    total_units,
                    &progress_state,
                    &restart_outcome,
                    progress_counters.as_ref(),
                    progress_counters
                        .active_restart_count
                        .load(Ordering::Relaxed),
                    restart_candidate_total_evaluation_budget(
                        context,
                        solver_settings,
                        budget_schedule,
                    ),
                    progress_counters
                        .aggregate_progress_percent_sum
                        .load(Ordering::Relaxed)
                        .checked_div(total_units.max(1))
                        .unwrap_or(0)
                        .min(100),
                );
                if let Some(resume_store) = completed_restart_store.as_deref_mut() {
                    resume_store.record_restart_outcome(&restart_outcome);
                }
                ordered_outcomes[restart_position] = Some(restart_outcome);
            }

            ordered_outcomes
                .into_iter()
                .flatten()
                .collect::<Vec<JointRefinementRestartOutcome>>()
        })
    })
}

pub(crate) fn joint_refinement_budget_schedule(
    solver_settings: &CalibrationOptimizerSettings,
    restart_count: usize,
) -> JointRefinementBudgetSchedule {
    let base_outer_cycle_count = joint_refinement_base_outer_cycle_count(solver_settings);

    let restart_count = restart_count.max(1);
    let outer_cycle_count = base_outer_cycle_count
        .saturating_mul(base_outer_cycle_count)
        .checked_div(restart_count)
        .unwrap_or(1)
        .clamp(1, base_outer_cycle_count);

    JointRefinementBudgetSchedule {
        restart_count,
        outer_cycle_count,
    }
}

pub(crate) fn desired_joint_refinement_restart_count(
    restart_count_override: Option<usize>,
) -> usize {
    restart_count_override
        .filter(|restart_count| *restart_count > 0)
        .unwrap_or(JOINT_REFINEMENT_DEFAULT_RESTART_COUNT)
}

pub(crate) fn joint_refinement_restart_block_powell_settings(
    solver_settings: &CalibrationOptimizerSettings,
    active_block_dimension: usize,
) -> PowellOptimizerSettings {
    let line_search_evaluation_cap = active_block_dimension.saturating_add(1).max(1);
    PowellOptimizerSettings {
        absolute_tolerance: solver_settings.absolute_tolerance,
        relative_tolerance: solver_settings.relative_tolerance,
        min_step_norm: solver_settings.absolute_tolerance,
        max_cycle_count: 1,
        line_search_evaluation_cap,
        evaluation_budget: active_block_dimension
            .max(1)
            .saturating_mul(line_search_evaluation_cap),
    }
}

fn joint_refinement_base_outer_cycle_count(
    _solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT
}

fn optimize_single_restart(
    restart_id: &str,
    seed_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    total_units: usize,
    progress_counters: &RestartProgressCounters,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> JointRefinementRestartOutcome {
    let mut restart_progress = RestartCandidateProgress::new(
        progress_sender,
        restart_id,
        total_units,
        progress_counters,
        budget_schedule,
        context,
        solver_settings,
    );
    restart_progress.emit_started();

    let mut current_vector = seed_vector.to_vec();
    let mut evaluation_count = 0usize;
    let mut stop_reason = "tolerance";

    for outer_cycle_index in 0..budget_schedule.outer_cycle_count {
        if joint_refinement_has_fatal_execution_failure(context) {
            restart_progress.emit_finished();
            return build_short_circuited_restart_outcome(
                restart_id,
                current_vector,
                context,
                solver_settings,
                evaluation_count,
                "fatal_execution_failure",
            );
        }

        let cycle_result = optimize_restart_cycle(
            current_vector.as_slice(),
            context,
            solver_settings,
            outer_cycle_index + 1,
            &mut restart_progress,
        );

        current_vector = cycle_result.updated_vector;
        evaluation_count += cycle_result.evaluation_count;

        if let Some(invalid_step_stop_reason) = cycle_result.invalid_step_stop_reason {
            restart_progress.emit_finished();
            return build_short_circuited_restart_outcome(
                restart_id,
                current_vector,
                context,
                solver_settings,
                evaluation_count,
                invalid_step_stop_reason,
            );
        }

        stop_reason = select_restart_stop_reason(
            cycle_result.hawkes_stop_reason,
            cycle_result.nig_stop_reason,
            cycle_result.coupling_stop_reason,
        );
        if stop_reason != "tolerance" {
            break;
        }
    }

    let local_evaluation = evaluate_joint_candidate(
        current_vector.as_slice(),
        context,
        JointRefinementFidelityTier::Local,
        solver_settings,
    );
    if local_evaluation.invalid_reason.is_some() {
            restart_progress.emit_finished();
            return JointRefinementRestartOutcome {
                restart_id: restart_id.to_string(),
                parameter_vector: current_vector,
                final_evaluation: local_evaluation.clone(),
                local_evaluation,
                local_evaluation_count: evaluation_count,
                stop_reason: stop_reason.to_string(),
            };
        }

    if joint_refinement_has_fatal_execution_failure(context) {
        restart_progress.emit_finished();
        return JointRefinementRestartOutcome {
            restart_id: restart_id.to_string(),
            parameter_vector: current_vector,
            final_evaluation: local_evaluation.clone(),
            local_evaluation,
            local_evaluation_count: evaluation_count,
            stop_reason: "fatal_execution_failure".to_string(),
        };
    }

    let final_evaluation = evaluate_joint_candidate(
        current_vector.as_slice(),
        context,
        JointRefinementFidelityTier::Final,
        solver_settings,
    );

    restart_progress.emit_finished();

    JointRefinementRestartOutcome {
        restart_id: restart_id.to_string(),
        parameter_vector: current_vector,
        local_evaluation,
        final_evaluation,
        local_evaluation_count: evaluation_count,
        stop_reason: stop_reason.to_string(),
    }
}

struct RestartCycleResult {
    updated_vector: Vec<f64>,
    hawkes_stop_reason: &'static str,
    nig_stop_reason: &'static str,
    coupling_stop_reason: &'static str,
    invalid_step_stop_reason: Option<&'static str>,
    evaluation_count: usize,
}

fn optimize_restart_cycle(
    current_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    outer_cycle: usize,
    restart_progress: &mut RestartCandidateProgress<'_>,
) -> RestartCycleResult {
    let mut updated_vector = current_vector.to_vec();
    let mut evaluation_count = 0usize;

    let (updated_nig_vector, nig_stop_reason, nig_evaluations) = optimize_parameter_block(
        updated_vector.as_slice(),
        context.active_parameter_layout.nig_block.as_slice(),
        context,
        solver_settings,
        "nig",
        outer_cycle,
        restart_progress,
    );

    updated_vector = updated_nig_vector;
    evaluation_count += nig_evaluations;

    if nig_stop_reason == "invalid_step" {
        return RestartCycleResult {
            updated_vector,
            hawkes_stop_reason: "tolerance",
            nig_stop_reason,
            coupling_stop_reason: "tolerance",
            invalid_step_stop_reason: Some(nig_stop_reason),
            evaluation_count,
        };
    }

    let (updated_hawkes_vector, hawkes_stop_reason, hawkes_evaluations) = optimize_parameter_block(
        updated_vector.as_slice(),
        context.active_parameter_layout.hawkes_block.as_slice(),
        context,
        solver_settings,
        "hawkes",
        outer_cycle,
        restart_progress,
    );

    updated_vector = updated_hawkes_vector;
    evaluation_count += hawkes_evaluations;

    if hawkes_stop_reason == "invalid_step" {
        return RestartCycleResult {
            updated_vector,
            hawkes_stop_reason,
            nig_stop_reason,
            coupling_stop_reason: "tolerance",
            invalid_step_stop_reason: Some(hawkes_stop_reason),
            evaluation_count,
        };
    }

    let (updated_coupling_vector, coupling_stop_reason, coupling_evaluations) =
        optimize_parameter_block(
            updated_vector.as_slice(),
            context.active_parameter_layout.coupling_block.as_slice(),
            context,
            solver_settings,
            "coupling",
            outer_cycle,
            restart_progress,
        );

    updated_vector = updated_coupling_vector;
    evaluation_count += coupling_evaluations;

    RestartCycleResult {
        updated_vector,
        hawkes_stop_reason,
        nig_stop_reason,
        coupling_stop_reason,
        invalid_step_stop_reason: (coupling_stop_reason == "invalid_step")
            .then_some(coupling_stop_reason),
        evaluation_count,
    }
}

fn build_short_circuited_restart_outcome(
    restart_id: &str,
    current_vector: Vec<f64>,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    evaluation_count: usize,
    stop_reason: &'static str,
) -> JointRefinementRestartOutcome {
    let local_evaluation = evaluate_joint_candidate(
        current_vector.as_slice(),
        context,
        JointRefinementFidelityTier::Local,
        solver_settings,
    );

    JointRefinementRestartOutcome {
        restart_id: restart_id.to_string(),
        parameter_vector: current_vector,
        final_evaluation: local_evaluation.clone(),
        local_evaluation,
        local_evaluation_count: evaluation_count,
        stop_reason: stop_reason.to_string(),
    }
}

fn select_restart_stop_reason(
    hawkes_stop_reason: &'static str,
    nig_stop_reason: &'static str,
    coupling_stop_reason: &'static str,
) -> &'static str {
    [coupling_stop_reason, nig_stop_reason, hawkes_stop_reason]
        .into_iter()
        .find(|stop_reason| *stop_reason != "tolerance")
        .unwrap_or("tolerance")
}

fn optimize_parameter_block(
    current_vector: &[f64],
    block_indices: &[usize],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    parameter_block_label: &'static str,
    outer_cycle: usize,
    restart_progress: &mut RestartCandidateProgress<'_>,
) -> (Vec<f64>, &'static str, usize) {
    if block_indices.is_empty() {
        return (current_vector.to_vec(), "fixed_block", 0);
    }
    restart_progress.emit_block_started(parameter_block_label, outer_cycle);

    let initial_point: Vec<f64> = block_indices
        .iter()
        .map(|index| current_vector[*index])
        .collect();
    let lower_bounds: Vec<f64> = block_indices
        .iter()
        .map(|index| context.lower_bounds[*index])
        .collect();
    let upper_bounds: Vec<f64> = block_indices
        .iter()
        .map(|index| context.upper_bounds[*index])
        .collect();

    let mut evaluate_block = |block_vector: &[f64]| -> Option<f64> {
        if joint_refinement_has_fatal_execution_failure(context) {
            return None;
        }

        let full_vector = substitute_block(current_vector, block_indices, block_vector);
        let block_objective = match parameter_block_label {
            "nig" => {
                evaluate_joint_candidate_nig_block_search_objective(full_vector.as_slice(), context)
            }
            _ => {
                let candidate_evaluation = evaluate_joint_candidate(
                    full_vector.as_slice(),
                    context,
                    JointRefinementFidelityTier::Local,
                    solver_settings,
                );
                candidate_evaluation
                    .invalid_reason
                    .is_none()
                    .then_some(candidate_evaluation.breakdown.weighted_objective)
            }
        };
        restart_progress.record_evaluation(parameter_block_label, outer_cycle, block_objective);

        block_objective
    };

    let optimizer_result = optimize_bounded_powell(
        initial_point.as_slice(),
        lower_bounds.as_slice(),
        upper_bounds.as_slice(),
        joint_refinement_restart_block_powell_settings(solver_settings, block_indices.len()),
        &mut evaluate_block,
    );

    match optimizer_result {
        Some(result) => (
            substitute_block(current_vector, block_indices, result.best_point.as_slice()),
            result.stop_reason,
            result.evaluation_count,
        ),
        None => (current_vector.to_vec(), "invalid_step", 0),
    }
}

fn substitute_block(
    current_vector: &[f64],
    block_indices: &[usize],
    block_vector: &[f64],
) -> Vec<f64> {
    let mut updated = current_vector.to_vec();
    for (block_position, parameter_index) in block_indices.iter().enumerate() {
        updated[*parameter_index] = block_vector[block_position];
    }

    updated
}

fn with_joint_refinement_thread_pool<T, F>(context: &JointRefinementContext, operation: F) -> T
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    match context.stage_thread_pool.as_ref() {
        Some(thread_pool) => thread_pool.install(operation),
        None => operation(),
    }
}

fn joint_refinement_has_parallel_worker_capacity(context: &JointRefinementContext) -> bool {
    context
        .stage_thread_pool
        .as_ref()
        .is_some_and(|thread_pool| thread_pool.current_num_threads() > 1)
}

fn joint_refinement_has_fatal_execution_failure(context: &JointRefinementContext) -> bool {
    simulation_infrastructure_failure_message(context).is_some()
        || option_pricing_execution_failure_message(context).is_some()
}

fn restart_candidate_total_evaluation_budget(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
) -> usize {
    budget_schedule
        .outer_cycle_count
        .max(1)
        .saturating_mul(restart_cycle_evaluation_budget(context, solver_settings).max(1))
}

fn restart_cycle_evaluation_budget(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    [
        context.active_parameter_layout.nig_block.as_slice(),
        context.active_parameter_layout.hawkes_block.as_slice(),
        context.active_parameter_layout.coupling_block.as_slice(),
    ]
    .into_iter()
    .filter(|block_indices| !block_indices.is_empty())
    .map(|block_indices| {
        joint_refinement_restart_block_powell_settings(solver_settings, block_indices.len())
            .evaluation_budget
    })
    .sum::<usize>()
}

fn emit_joint_refinement_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_update: JointRefinementStageProgressUpdate,
) {
    if let Some(progress_sender) = progress_sender {
        let _ = progress_sender.send(progress_update);
    }
}

impl<'a> RestartCandidateProgress<'a> {
    fn new(
        progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
        restart_id: &'a str,
        total_units: usize,
        counters: &'a RestartProgressCounters,
        budget_schedule: JointRefinementBudgetSchedule,
        context: &JointRefinementContext,
        solver_settings: &CalibrationOptimizerSettings,
    ) -> Self {
        Self {
            progress_sender,
            restart_id,
            total_units,
            counters,
            total_outer_cycles: budget_schedule.outer_cycle_count.max(1),
            total_evaluation_budget: restart_candidate_total_evaluation_budget(
                context,
                solver_settings,
                budget_schedule,
            ),
            current_evaluation_count: 0,
            last_evaluation_progress_percent: None,
            current_restart_best_weighted_objective: None,
            finished: false,
        }
    }

    fn emit_started(&mut self) {
        let active_restart_count = self
            .counters
            .active_restart_count
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        self.emit_progress(None, None, None, Some(active_restart_count), None);
    }

    fn record_evaluation(
        &mut self,
        parameter_block_label: &'static str,
        outer_cycle: usize,
        weighted_objective: Option<f64>,
    ) {
        self.current_evaluation_count = self.current_evaluation_count.saturating_add(1);
        if let Some(weighted_objective) = weighted_objective {
            self.current_restart_best_weighted_objective = Some(
                self.current_restart_best_weighted_objective
                    .map(|current_best| current_best.min(weighted_objective))
                    .unwrap_or(weighted_objective),
            );
            self.counters
                .record_display_best(self.restart_id, weighted_objective);
        }

        let progress_percent = self.running_evaluation_progress_percent();
        if self.last_evaluation_progress_percent == Some(progress_percent) {
            return;
        }

        self.record_progress_percent(progress_percent);
        self.last_evaluation_progress_percent = Some(progress_percent);
        self.emit_progress(
            Some(parameter_block_label),
            Some(outer_cycle),
            Some(progress_percent),
            Some(self.counters.active_restart_count.load(Ordering::Relaxed)),
            weighted_objective,
        );
    }

    fn emit_block_started(&self, parameter_block_label: &'static str, outer_cycle: usize) {
        self.emit_progress(
            Some(parameter_block_label),
            Some(outer_cycle),
            None,
            Some(self.counters.active_restart_count.load(Ordering::Relaxed)),
            None,
        );
    }

    fn emit_finished(&mut self) {
        if self.finished {
            return;
        }

        self.record_progress_percent(100);
        self.last_evaluation_progress_percent = Some(100);
        self.finished = true;
        self.counters
            .active_restart_count
            .fetch_sub(1, Ordering::Relaxed);
    }

    fn record_progress_percent(&self, progress_percent: usize) {
        let previous_progress_percent = self.last_evaluation_progress_percent.unwrap_or(0);
        if progress_percent <= previous_progress_percent {
            return;
        }

        self.counters.aggregate_progress_percent_sum.fetch_add(
            progress_percent - previous_progress_percent,
            Ordering::Relaxed,
        );
    }

    fn running_evaluation_progress_percent(&self) -> usize {
        self.current_evaluation_count
            .saturating_mul(100)
            .checked_div(self.total_evaluation_budget.max(1))
            .unwrap_or(0)
            .min(99)
    }

    fn aggregate_progress_percent(&self) -> usize {
        self.counters
            .aggregate_progress_percent_sum
            .load(Ordering::Relaxed)
            .checked_div(self.total_units.max(1))
            .unwrap_or(0)
            .min(100)
    }

    fn emit_progress(
        &self,
        parameter_block_label: Option<&str>,
        outer_cycle: Option<usize>,
        evaluation_progress_percent: Option<usize>,
        active_restart_count: Option<usize>,
        current_weighted_objective: Option<f64>,
    ) {
        let (best_restart_id, best_weighted_objective) = self.counters.display_best_snapshot();
        emit_joint_refinement_progress(
            self.progress_sender,
            JointRefinementStageProgressUpdate {
                aggregate_progress_percent: Some(self.aggregate_progress_percent()),
                current_item_label: Some(self.restart_id.to_string()),
                current_restart_id: Some(self.restart_id.to_string()),
                active_restart_count,
                current_parameter_block: parameter_block_label.map(str::to_string),
                current_outer_cycle: outer_cycle,
                total_outer_cycles: Some(self.total_outer_cycles),
                current_evaluation_count: Some(self.current_evaluation_count),
                total_evaluation_budget: Some(self.total_evaluation_budget),
                evaluation_progress_percent,
                current_weighted_objective,
                current_restart_best_weighted_objective: self
                    .current_restart_best_weighted_objective,
                best_restart_id,
                best_weighted_objective,
                ..JointRefinementStageProgressUpdate::new(
                    JointRefinementStageProgressPhase::RestartOptimization,
                    self.counters.completed_units.load(Ordering::Relaxed),
                    self.total_units,
                )
            },
        );
    }
}

impl RestartProgressState {
    fn record(&mut self, restart_outcome: &JointRefinementRestartOutcome) {
        self.completed_units += 1;
        self.cumulative_local_evaluation_count += restart_outcome.local_evaluation_count;

        if restart_outcome.local_evaluation.invalid_reason.is_none()
            && restart_outcome.final_evaluation.invalid_reason.is_none()
        {
            self.valid_completed_units += 1;
        }
    }
}

fn emit_restart_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    total_units: usize,
    progress_state: &RestartProgressState,
    restart_outcome: &JointRefinementRestartOutcome,
    progress_counters: &RestartProgressCounters,
    active_restart_count: usize,
    total_evaluation_budget: usize,
    aggregate_progress_percent: usize,
) {
    let completed_units = progress_state.completed_units;
    let (best_restart_id, best_weighted_objective) = progress_counters.display_best_snapshot();
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(aggregate_progress_percent),
            current_item_label: Some(restart_outcome.restart_id.clone()),
            current_restart_id: Some(restart_outcome.restart_id.clone()),
            active_restart_count: Some(active_restart_count),
            current_evaluation_count: Some(restart_outcome.local_evaluation_count),
            total_evaluation_budget: Some(total_evaluation_budget),
            evaluation_progress_percent: Some(100),
            valid_completed_units: Some(progress_state.valid_completed_units),
            cumulative_local_evaluation_count: Some(
                progress_state.cumulative_local_evaluation_count,
            ),
            current_weighted_objective: Some(
                restart_outcome
                    .final_evaluation
                    .breakdown
                    .weighted_objective,
            ),
            best_restart_id,
            best_weighted_objective,
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::RestartOptimization,
                completed_units,
                total_units,
            )
        },
    );
}

impl RestartProgressCounters {
    fn record_display_best(&self, restart_id: &str, weighted_objective: f64) {
        let Ok(mut display_best_state) = self.display_best_state.lock() else {
            return;
        };

        match display_best_state.best_weighted_objective {
            Some(current_best) if weighted_objective >= current_best => {}
            _ => {
                display_best_state.best_weighted_objective = Some(weighted_objective);
                display_best_state.best_restart_id = Some(restart_id.to_string());
            }
        }
    }

    fn display_best_snapshot(&self) -> (Option<String>, Option<f64>) {
        let Ok(display_best_state) = self.display_best_state.lock() else {
            return (None, None);
        };

        (
            display_best_state.best_restart_id.clone(),
            display_best_state.best_weighted_objective,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::{RestartCandidateProgress, RestartProgressCounters};

    #[test]
    fn running_restart_evaluation_progress_is_capped_below_completion() {
        let (progress_sender, _progress_receiver) = mpsc::channel();
        let progress_counters = RestartProgressCounters::default();
        let mut restart_progress = RestartCandidateProgress {
            progress_sender: Some(&progress_sender),
            restart_id: "halton_09",
            total_units: 15,
            counters: &progress_counters,
            total_outer_cycles: 3,
            total_evaluation_budget: 10,
            current_evaluation_count: 10,
            last_evaluation_progress_percent: None,
            current_restart_best_weighted_objective: None,
            finished: false,
        };

        assert_eq!(restart_progress.running_evaluation_progress_percent(), 99);

        restart_progress.current_evaluation_count = 5;

        assert_eq!(restart_progress.running_evaluation_progress_percent(), 50);
    }

    #[test]
    fn running_restart_progress_emits_live_global_best_objective() {
        let (progress_sender, progress_receiver) = mpsc::channel();
        let progress_counters = RestartProgressCounters::default();
        let mut baseline_progress = RestartCandidateProgress {
            progress_sender: Some(&progress_sender),
            restart_id: "baseline",
            total_units: 2,
            counters: &progress_counters,
            total_outer_cycles: 1,
            total_evaluation_budget: 10,
            current_evaluation_count: 0,
            last_evaluation_progress_percent: None,
            current_restart_best_weighted_objective: None,
            finished: false,
        };
        let mut halton_progress = RestartCandidateProgress {
            progress_sender: Some(&progress_sender),
            restart_id: "halton_01",
            total_units: 2,
            counters: &progress_counters,
            total_outer_cycles: 1,
            total_evaluation_budget: 10,
            current_evaluation_count: 0,
            last_evaluation_progress_percent: None,
            current_restart_best_weighted_objective: None,
            finished: false,
        };

        baseline_progress.record_evaluation("nig", 1, Some(10.0));
        progress_receiver.recv().unwrap();

        halton_progress.record_evaluation("nig", 1, Some(4.0));
        let progress_update = progress_receiver.recv().unwrap();

        assert_eq!(progress_update.best_restart_id.as_deref(), Some("halton_01"));
        assert_eq!(progress_update.best_weighted_objective, Some(4.0));
    }
}
