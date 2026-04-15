use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::numeric::integer_square_root;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use crate::stages::optimization::{PowellOptimizerSettings, optimize_bounded_powell};

use super::super::objective::evaluate_joint_candidate;
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
    pub(crate) powell_settings: PowellOptimizerSettings,
}

#[derive(Debug, Default)]
struct RestartProgressState {
    completed_units: usize,
    cumulative_local_evaluation_count: usize,
    valid_completed_units: usize,
    best_weighted_objective: Option<f64>,
}

pub(crate) fn optimize_restart_seeds(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Vec<JointRefinementRestartOutcome> {
    debug_assert_eq!(restart_seeds.len().max(1), budget_schedule.restart_count);

    let total_units = restart_seeds.len();

    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            phase: JointRefinementStageProgressPhase::RestartOptimization,
            completed_units: 0,
            total_units,
            current_item_label: None,
            valid_completed_units: Some(0),
            cumulative_local_evaluation_count: Some(0),
            current_weighted_objective: None,
            best_weighted_objective: None,
            current_coarse_consistency_fit: None,
            best_coarse_consistency_fit: None,
        },
    );

    if !joint_refinement_has_parallel_worker_capacity(context) || total_units <= 1 {
        return optimize_restart_seeds_sequential(
            restart_seeds,
            context,
            solver_settings,
            budget_schedule,
            progress_sender,
        );
    }

    optimize_restart_seeds_parallel(
        restart_seeds,
        context,
        solver_settings,
        budget_schedule,
        progress_sender,
    )
}

fn optimize_restart_seeds_sequential(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Vec<JointRefinementRestartOutcome> {
    let total_units = restart_seeds.len();

    let mut progress_state = RestartProgressState::default();
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
        );

        progress_state.record(&restart_outcome);

        emit_restart_progress(
            progress_sender,
            total_units,
            &progress_state,
            &restart_outcome,
        );

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
) -> Vec<JointRefinementRestartOutcome> {
    let total_units = restart_seeds.len();

    with_joint_refinement_thread_pool(context, || {
        let (result_sender, result_receiver) =
            mpsc::channel::<(usize, JointRefinementRestartOutcome)>();
        let result_receiver = Arc::new(Mutex::new(result_receiver));

        rayon::scope(|scope| {
            for (restart_position, (restart_id, seed_vector)) in restart_seeds.iter().enumerate() {
                let result_sender_for_restart = result_sender.clone();
                scope.spawn(move |_| {
                    let outcome = optimize_single_restart(
                        restart_id.as_str(),
                        seed_vector.as_slice(),
                        context,
                        solver_settings,
                        budget_schedule,
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
                emit_restart_progress(
                    progress_sender,
                    total_units,
                    &progress_state,
                    &restart_outcome,
                );
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
    let (base_outer_cycle_count, base_powell_settings) =
        joint_refinement_base_schedule_components(solver_settings);

    JointRefinementBudgetSchedule {
        restart_count: restart_count.max(1),
        outer_cycle_count: base_outer_cycle_count,
        powell_settings: base_powell_settings,
    }
}

pub(crate) fn desired_joint_refinement_restart_count(
    restart_count_override: Option<usize>,
) -> usize {
    restart_count_override
        .filter(|restart_count| *restart_count > 0)
        .unwrap_or(JOINT_REFINEMENT_DEFAULT_RESTART_COUNT)
}

pub(crate) fn joint_refinement_base_schedule_components(
    solver_settings: &CalibrationOptimizerSettings,
) -> (usize, PowellOptimizerSettings) {
    base_schedule_components(solver_settings)
}

fn base_schedule_components(
    solver_settings: &CalibrationOptimizerSettings,
) -> (usize, PowellOptimizerSettings) {
    let outer_cycle_count = JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT;
    let evaluation_budget = solver_settings.max_iterations.max(outer_cycle_count);
    let line_search_evaluation_cap = integer_square_root(evaluation_budget).max(1);

    let max_cycle_count = evaluation_budget
        .div_ceil(line_search_evaluation_cap.max(1))
        .max(1);

    let powell_settings = PowellOptimizerSettings {
        absolute_tolerance: solver_settings.absolute_tolerance,
        relative_tolerance: solver_settings.relative_tolerance,
        min_step_norm: solver_settings.absolute_tolerance,
        max_cycle_count,
        line_search_evaluation_cap,
        evaluation_budget,
    };

    (outer_cycle_count, powell_settings)
}

fn optimize_single_restart(
    restart_id: &str,
    seed_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
) -> JointRefinementRestartOutcome {
    let mut current_vector = seed_vector.to_vec();
    let mut evaluation_count = 0usize;
    let mut stop_reason = "tolerance";

    for _ in 0..budget_schedule.outer_cycle_count {
        if joint_refinement_has_fatal_execution_failure(context) {
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
            budget_schedule.powell_settings,
        );

        current_vector = cycle_result.updated_vector;
        evaluation_count += cycle_result.evaluation_count;

        if let Some(invalid_step_stop_reason) = cycle_result.invalid_step_stop_reason {
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
        return JointRefinementRestartOutcome {
            restart_id: restart_id.to_string(),
            parameter_vector: current_vector,
            final_evaluation: local_evaluation.clone(),
            local_evaluation,
            local_evaluation_count: evaluation_count,
            stop_reason,
        };
    }

    if joint_refinement_has_fatal_execution_failure(context) {
        return JointRefinementRestartOutcome {
            restart_id: restart_id.to_string(),
            parameter_vector: current_vector,
            final_evaluation: local_evaluation.clone(),
            local_evaluation,
            local_evaluation_count: evaluation_count,
            stop_reason: "fatal_execution_failure",
        };
    }

    let final_evaluation = evaluate_joint_candidate(
        current_vector.as_slice(),
        context,
        JointRefinementFidelityTier::Final,
        solver_settings,
    );

    JointRefinementRestartOutcome {
        restart_id: restart_id.to_string(),
        parameter_vector: current_vector,
        local_evaluation,
        final_evaluation,
        local_evaluation_count: evaluation_count,
        stop_reason,
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
    optimizer_settings: PowellOptimizerSettings,
) -> RestartCycleResult {
    let mut updated_vector = current_vector.to_vec();
    let mut evaluation_count = 0usize;

    let (updated_nig_vector, nig_stop_reason, nig_evaluations) = optimize_parameter_block(
        updated_vector.as_slice(),
        context.active_parameter_layout.nig_block.as_slice(),
        context,
        optimizer_settings,
        solver_settings,
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
        optimizer_settings,
        solver_settings,
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
            optimizer_settings,
            solver_settings,
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
        stop_reason,
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
    optimizer_settings: PowellOptimizerSettings,
    solver_settings: &CalibrationOptimizerSettings,
) -> (Vec<f64>, &'static str, usize) {
    if block_indices.is_empty() {
        return (current_vector.to_vec(), "fixed_block", 0);
    }

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
        let candidate_evaluation = evaluate_joint_candidate(
            full_vector.as_slice(),
            context,
            JointRefinementFidelityTier::Local,
            solver_settings,
        );
        if candidate_evaluation.invalid_reason.is_some() {
            return None;
        }

        Some(candidate_evaluation.breakdown.weighted_objective)
    };

    let optimizer_result = optimize_bounded_powell(
        initial_point.as_slice(),
        lower_bounds.as_slice(),
        upper_bounds.as_slice(),
        optimizer_settings,
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

fn emit_joint_refinement_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_update: JointRefinementStageProgressUpdate,
) {
    if let Some(progress_sender) = progress_sender {
        let _ = progress_sender.send(progress_update);
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

            let restart_weighted_objective = restart_outcome
                .final_evaluation
                .breakdown
                .weighted_objective;

            self.best_weighted_objective = Some(match self.best_weighted_objective {
                Some(current_best) => current_best.min(restart_weighted_objective),
                None => restart_weighted_objective,
            });
        }
    }
}

fn emit_restart_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    total_units: usize,
    progress_state: &RestartProgressState,
    restart_outcome: &JointRefinementRestartOutcome,
) {
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            phase: JointRefinementStageProgressPhase::RestartOptimization,
            completed_units: progress_state.completed_units,
            total_units,
            current_item_label: Some(restart_outcome.restart_id.clone()),
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
            best_weighted_objective: progress_state.best_weighted_objective,
            current_coarse_consistency_fit: None,
            best_coarse_consistency_fit: None,
        },
    );
}
