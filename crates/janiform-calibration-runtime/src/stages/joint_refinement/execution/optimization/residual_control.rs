use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageRestartEvaluationTimingDiagnostic,
};
use janiform_work_scheduler::{DynamicWorkPoolInput, run_dynamic_work_pool};

use crate::stages::joint_refinement::contracts::JointRefinementStageProgressUpdate;
use crate::stages::return_components::ContinuousLikelihoodEvaluatorCounts;

use super::super::{
    JointRefinementContext, JointRefinementRestartOutcome, elapsed_millis,
    joint_refinement_parallel_worker_count,
};
use super::workflow::{
    JointRefinementBudgetSchedule, JointRefinementRestartOptimizationOutput,
    RestartObjectiveEvaluation, RestartObjectiveResidual, evaluate_finalized_restart_candidate,
    evaluate_prechecked_full_parameter_vector_objective,
    joint_refinement_has_fatal_execution_failure, restart_candidate_total_evaluation_budget,
    restart_cycle_evaluation_budget,
};

mod finalization;
mod model;
mod progress;
mod state;

use super::residual_profile::ResidualTailProfile;
use finalization::{
    next_residual_control_finalization_job, select_residual_control_finalization_outcome,
};
use model::initial_residual_control_trust_region_radius;
use progress::{
    ResidualControlSearchProgressInput, emit_residual_control_finalization_progress,
    emit_residual_control_finished, emit_residual_control_search_progress,
    residual_control_evaluation_timing_diagnostic,
};
use state::{
    mark_pending_residual_control_states_completed,
    mark_unscheduled_residual_control_states_completed, next_residual_control_search_job,
    record_residual_control_result, residual_control_state_has_remaining_budget,
};

const RESIDUAL_CONTROL_SCHEDULE_CATEGORY_INITIAL: &str = "initial";
const RESIDUAL_CONTROL_SCHEDULE_CATEGORY_CONTROL_PROBE: &str = "control_probe";
const RESIDUAL_CONTROL_SCHEDULE_CATEGORY_REPAIR: &str = "residual_control_repair";
const RESIDUAL_CONTROL_STOP_REASON_POOL_BUDGET: &str = "residual_control_pool_budget";
const RESIDUAL_CONTROL_STOP_REASON_NO_SCHEDULABLE_WORK: &str =
    "residual_control_no_schedulable_work";

/// Unit mean-diagonal damping uses the average Jacobian curvature as the LM penalty scale.
const RESIDUAL_CONTROL_LEVENBERG_MARQUARDT_DAMPING_MEAN_DIAGONAL_MULTIPLIER: f64 = 1.0;

#[derive(Debug, Clone)]
struct ResidualControlState {
    restart_id: String,
    seed_vector: Vec<f64>,
    accepted_vector: Vec<f64>,
    accepted_objective: Option<f64>,
    accepted_profile: Option<ResidualTailProfile>,
    accepted_residuals: Option<Vec<RestartObjectiveResidual>>,
    best_global_vector: Vec<f64>,
    best_global_objective: Option<f64>,
    best_profile_vector: Vec<f64>,
    best_profile_objective: Option<f64>,
    best_profile: Option<ResidualTailProfile>,
    round_best_vector: Option<Vec<f64>>,
    round_best_objective: Option<f64>,
    round_best_profile: Option<ResidualTailProfile>,
    round_best_residuals: Option<Vec<RestartObjectiveResidual>>,
    round_best_predicted_profile: Option<ResidualTailProfile>,
    records: Vec<ResidualControlRecord>,
    scheduled_parameter_bits: Vec<Vec<u64>>,
    control_probe_vectors: Vec<ResidualControlCandidate>,
    next_control_probe_position: usize,
    control_probes_prepared: bool,
    repair_vectors: Vec<ResidualControlCandidate>,
    next_repair_position: usize,
    repairs_prepared: bool,
    trust_region_radius: f64,
    completed_search_rounds: usize,
    evaluation_count: usize,
    max_evaluation_count: usize,
    in_flight_count: usize,
    stop_reason: String,
    completed: bool,
    exact_likelihood_evaluator_counts: Option<ContinuousLikelihoodEvaluatorCounts>,
}

#[derive(Debug, Clone)]
struct ResidualControlRecord {
    parameter_vector: Vec<f64>,
    residuals: Vec<RestartObjectiveResidual>,
}

#[derive(Debug, Clone, Default)]
struct ResidualControlPrediction {
    profile: Option<ResidualTailProfile>,
    residual_objective: Option<f64>,
}

#[derive(Debug, Clone)]
struct ResidualControlCandidate {
    parameter_vector: Vec<f64>,
    label: String,
    prediction: ResidualControlPrediction,
}

#[derive(Debug, Clone)]
struct ResidualControlJob {
    restart_position: usize,
    parameter_vector: Vec<f64>,
    schedule_category: &'static str,
    candidate_evaluation_count: usize,
    search_direction_position: Option<usize>,
    search_direction_label: String,
    prediction: ResidualControlPrediction,
}

#[derive(Debug, Clone)]
struct ResidualControlResult {
    restart_position: usize,
    parameter_vector: Vec<f64>,
    schedule_category: &'static str,
    candidate_evaluation_count: usize,
    search_direction_position: Option<usize>,
    search_direction_label: String,
    prediction: ResidualControlPrediction,
    elapsed_millis: u64,
    evaluation: Option<RestartObjectiveEvaluation>,
}

#[derive(Debug)]
struct ResidualControlFinalizationJob {
    restart_position: usize,
    candidate_position: usize,
    restart_context: Arc<ResidualControlFinalizationRestartContext>,
    candidate_vector: Vec<f64>,
}

#[derive(Debug)]
struct ResidualControlFinalizationRestartContext {
    restart_id: String,
    seed_vector: Vec<f64>,
    evaluation_count: usize,
    stop_reason: String,
}

#[derive(Debug)]
struct ResidualControlFinalizationResult {
    restart_position: usize,
    candidate_position: usize,
    outcome: JointRefinementRestartOutcome,
}

enum ResidualControlPoolJob {
    Search(ResidualControlJob),
    Finalization(Box<ResidualControlFinalizationJob>),
}

enum ResidualControlPoolResult {
    Search(Box<ResidualControlResult>),
    Finalization(Box<ResidualControlFinalizationResult>),
}

struct ResidualControlWorkerPoolState<'a> {
    states: &'a mut [ResidualControlState],
    worker_capacity: usize,
    restart_outcomes: Vec<Option<JointRefinementRestartOutcome>>,
    finalization_candidate_outcomes: Vec<Vec<Option<JointRefinementRestartOutcome>>>,
    pending_finalization_jobs: VecDeque<ResidualControlFinalizationJob>,
    finalization_scheduled: Vec<bool>,
    next_restart_position: usize,
    next_finalization_restart_position: usize,
    scheduled_search_evaluation_count: usize,
    completed_search_evaluation_count: usize,
    completed_finalization_count: usize,
    evaluation_timing_diagnostics:
        &'a mut Vec<JointRefinementStageRestartEvaluationTimingDiagnostic>,
}

pub(super) fn optimize_restart_seeds_residual_control(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> JointRefinementRestartOptimizationOutput {
    let mut states =
        build_residual_control_states(restart_seeds, context, solver_settings, budget_schedule);

    let search_pool_budget =
        residual_control_total_evaluation_budget(context, solver_settings, budget_schedule);

    let total_progress_units = search_pool_budget.saturating_add(states.len()).max(1);

    let mut evaluation_timing_diagnostics = Vec::with_capacity(search_pool_budget);

    let restart_outcomes = run_residual_control_worker_pool(ResidualControlWorkerPoolInput {
        states: states.as_mut_slice(),
        context,
        solver_settings,
        search_pool_budget,
        total_progress_units,
        progress_sender,
        evaluation_timing_diagnostics: &mut evaluation_timing_diagnostics,
    });

    emit_residual_control_finished(progress_sender, restart_outcomes.as_slice());

    JointRefinementRestartOptimizationOutput {
        restart_outcomes,
        evaluation_timing_diagnostics,
    }
}

struct ResidualControlWorkerPoolInput<'a> {
    states: &'a mut [ResidualControlState],
    context: &'a JointRefinementContext,
    solver_settings: &'a CalibrationOptimizerSettings,
    search_pool_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    evaluation_timing_diagnostics:
        &'a mut Vec<JointRefinementStageRestartEvaluationTimingDiagnostic>,
}

fn build_residual_control_states(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
) -> Vec<ResidualControlState> {
    let cycle_budget = restart_cycle_evaluation_budget(context, solver_settings).max(1);

    let max_candidate_budget = ((cycle_budget as f64)
        * budget_schedule
            .calibration_effort
            .restart_pool_max_candidate_budget_scale)
        .ceil()
        .max(1.0) as usize;

    restart_seeds
        .iter()
        .map(|(restart_id, seed_vector)| ResidualControlState {
            // Search roles own vector snapshots because accepted, best global, and best
            // residual-profile candidates can diverge before finalization.
            restart_id: restart_id.clone(),
            seed_vector: seed_vector.clone(),
            accepted_vector: seed_vector.clone(),
            accepted_objective: None,
            accepted_profile: None,
            accepted_residuals: None,
            best_global_vector: seed_vector.clone(),
            best_global_objective: None,
            best_profile_vector: seed_vector.clone(),
            best_profile_objective: None,
            best_profile: None,
            round_best_vector: None,
            round_best_objective: None,
            round_best_profile: None,
            round_best_residuals: None,
            round_best_predicted_profile: None,
            records: Vec::new(),
            scheduled_parameter_bits: Vec::new(),
            control_probe_vectors: Vec::new(),
            next_control_probe_position: 0,
            control_probes_prepared: false,
            repair_vectors: Vec::new(),
            next_repair_position: 0,
            repairs_prepared: false,
            trust_region_radius: initial_residual_control_trust_region_radius(
                context.lower_bounds.as_slice(),
                context.upper_bounds.as_slice(),
            ),
            completed_search_rounds: 0,
            evaluation_count: 0,
            max_evaluation_count: max_candidate_budget,
            in_flight_count: 0,
            stop_reason: RESIDUAL_CONTROL_STOP_REASON_POOL_BUDGET.to_string(),
            completed: false,
            exact_likelihood_evaluator_counts: None,
        })
        .collect()
}

fn residual_control_total_evaluation_budget(
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

fn run_residual_control_worker_pool(
    input: ResidualControlWorkerPoolInput<'_>,
) -> Vec<JointRefinementRestartOutcome> {
    let worker_capacity = joint_refinement_parallel_worker_count(input.context).max(1);
    let restart_count = input.states.len();

    let mut pool_state = ResidualControlWorkerPoolState {
        states: input.states,
        worker_capacity,
        restart_outcomes: std::iter::repeat_with(|| None)
            .take(restart_count)
            .collect(),
        finalization_candidate_outcomes: std::iter::repeat_with(Vec::new)
            .take(restart_count)
            .collect(),
        pending_finalization_jobs: VecDeque::new(),
        finalization_scheduled: vec![false; restart_count],
        next_restart_position: 0,
        next_finalization_restart_position: 0,
        scheduled_search_evaluation_count: 0,
        completed_search_evaluation_count: 0,
        completed_finalization_count: 0,
        evaluation_timing_diagnostics: input.evaluation_timing_diagnostics,
    };

    run_dynamic_work_pool(
        input.context.stage_thread_pool.as_deref(),
        DynamicWorkPoolInput::new(
            &mut pool_state,
            input.context.cancellation_token.shared_handle(),
            worker_capacity,
            |pool_state: &mut ResidualControlWorkerPoolState<'_>, _status| {
                next_residual_control_pool_job(
                    pool_state,
                    input.context,
                    input.solver_settings,
                    input.search_pool_budget,
                    input.total_progress_units,
                    input.progress_sender,
                )
            },
            |pool_job, _cancellation_token| {
                run_residual_control_pool_job(pool_job, input.context, input.solver_settings)
            },
            |pool_state: &mut ResidualControlWorkerPoolState<'_>, result, _status| {
                record_residual_control_pool_result(
                    pool_state,
                    result,
                    input.solver_settings,
                    input.search_pool_budget,
                    input.total_progress_units,
                    input.progress_sender,
                    input.context,
                );
            },
            |pool_state: &ResidualControlWorkerPoolState<'_>, _status| {
                residual_control_pool_completed(pool_state)
            },
        ),
    );

    pool_state.restart_outcomes.into_iter().flatten().collect()
}

fn next_residual_control_pool_job(
    pool_state: &mut ResidualControlWorkerPoolState<'_>,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    search_pool_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Option<ResidualControlPoolJob> {
    if residual_control_search_scheduling_complete(pool_state, search_pool_budget, context) {
        mark_pending_residual_control_states_completed(pool_state.states);
    } else {
        let max_in_flight_per_restart = residual_control_max_in_flight_per_restart(
            pool_state.states,
            pool_state.worker_capacity,
        );

        let search_job = next_residual_control_search_job(
            pool_state.states,
            context,
            solver_settings,
            &mut pool_state.next_restart_position,
            max_in_flight_per_restart,
        );

        if let Some(search_job) = search_job {
            pool_state.scheduled_search_evaluation_count = pool_state
                .scheduled_search_evaluation_count
                .saturating_add(1);

            return Some(ResidualControlPoolJob::Search(search_job));
        }

        if residual_control_search_in_flight_count(pool_state.states) == 0 {
            mark_unscheduled_residual_control_states_completed(pool_state.states);
        }
    }

    next_residual_control_finalization_job(pool_state, total_progress_units, progress_sender)
        .map(Box::new)
        .map(ResidualControlPoolJob::Finalization)
}

fn residual_control_search_scheduling_complete(
    pool_state: &ResidualControlWorkerPoolState<'_>,
    search_pool_budget: usize,
    context: &JointRefinementContext,
) -> bool {
    pool_state.scheduled_search_evaluation_count >= search_pool_budget
        || joint_refinement_has_fatal_execution_failure(context)
}

fn residual_control_search_in_flight_count(states: &[ResidualControlState]) -> usize {
    states
        .iter()
        .map(|state| state.in_flight_count)
        .sum::<usize>()
}

fn residual_control_max_in_flight_per_restart(
    states: &[ResidualControlState],
    worker_capacity: usize,
) -> usize {
    let active_restart_count = states
        .iter()
        .filter(|state| !state.completed && residual_control_state_has_remaining_budget(state))
        .count()
        .max(1);

    worker_capacity
        .saturating_add(active_restart_count - 1)
        .checked_div(active_restart_count)
        .unwrap_or(1)
        .max(1)
}

fn run_residual_control_pool_job(
    pool_job: ResidualControlPoolJob,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> ResidualControlPoolResult {
    match pool_job {
        ResidualControlPoolJob::Search(search_job) => ResidualControlPoolResult::Search(Box::new(
            run_residual_control_job(search_job, context, solver_settings),
        )),
        ResidualControlPoolJob::Finalization(finalization_job) => {
            ResidualControlPoolResult::Finalization(Box::new(
                run_residual_control_finalization_job(*finalization_job, context, solver_settings),
            ))
        }
    }
}

fn run_residual_control_job(
    job: ResidualControlJob,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> ResidualControlResult {
    let evaluation_started_at = std::time::Instant::now();

    let evaluation = evaluate_prechecked_full_parameter_vector_objective(
        job.parameter_vector.as_slice(),
        context,
        solver_settings,
    );

    let elapsed_millis = elapsed_millis(evaluation_started_at);

    ResidualControlResult {
        restart_position: job.restart_position,
        parameter_vector: job.parameter_vector,
        schedule_category: job.schedule_category,
        candidate_evaluation_count: job.candidate_evaluation_count,
        search_direction_position: job.search_direction_position,
        search_direction_label: job.search_direction_label,
        prediction: job.prediction,
        elapsed_millis,
        evaluation,
    }
}

fn run_residual_control_finalization_job(
    job: ResidualControlFinalizationJob,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> ResidualControlFinalizationResult {
    ResidualControlFinalizationResult {
        restart_position: job.restart_position,
        candidate_position: job.candidate_position,
        outcome: evaluate_finalized_restart_candidate(
            job.restart_context.restart_id.as_str(),
            job.restart_context.seed_vector.as_slice(),
            job.candidate_vector.as_slice(),
            context,
            solver_settings,
            job.restart_context.evaluation_count,
            job.restart_context.stop_reason.as_str(),
        ),
    }
}

fn record_residual_control_pool_result(
    pool_state: &mut ResidualControlWorkerPoolState<'_>,
    result: ResidualControlPoolResult,
    solver_settings: &CalibrationOptimizerSettings,
    search_pool_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    context: &JointRefinementContext,
) {
    match result {
        ResidualControlPoolResult::Search(search_result) => {
            record_residual_control_search_result(
                pool_state,
                *search_result,
                solver_settings,
                search_pool_budget,
                total_progress_units,
                progress_sender,
                context,
            );
        }
        ResidualControlPoolResult::Finalization(finalization_result) => {
            record_residual_control_finalization_result(
                pool_state,
                *finalization_result,
                total_progress_units,
                progress_sender,
                context,
                solver_settings,
            );
        }
    }
}

fn record_residual_control_search_result(
    pool_state: &mut ResidualControlWorkerPoolState<'_>,
    result: ResidualControlResult,
    solver_settings: &CalibrationOptimizerSettings,
    search_pool_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    context: &JointRefinementContext,
) {
    let restart_position = result.restart_position;

    let timing_diagnostic = residual_control_evaluation_timing_diagnostic(
        &pool_state.states[restart_position],
        &result,
    );

    // Progress updates and the final diagnostic artifact both own this timing record.
    pool_state
        .evaluation_timing_diagnostics
        .push(timing_diagnostic.clone());

    record_residual_control_result(
        &mut pool_state.states[restart_position],
        result,
        solver_settings,
    );

    pool_state.completed_search_evaluation_count = pool_state
        .completed_search_evaluation_count
        .saturating_add(1);

    if residual_control_search_scheduling_complete(pool_state, search_pool_budget, context) {
        mark_pending_residual_control_states_completed(pool_state.states);
    }

    emit_residual_control_search_progress(ResidualControlSearchProgressInput {
        progress_sender,
        states: pool_state.states,
        restart_outcomes: pool_state.restart_outcomes.as_slice(),
        current_restart_position: restart_position,
        completed_search_evaluation_count: pool_state.completed_search_evaluation_count,
        completed_finalization_count: pool_state.completed_finalization_count,
        total_progress_units,
        restart_evaluation_timing_diagnostic: Some(timing_diagnostic),
    });
}

fn record_residual_control_finalization_result(
    pool_state: &mut ResidualControlWorkerPoolState<'_>,
    result: ResidualControlFinalizationResult,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) {
    let restart_position = result.restart_position;

    let Some(candidate_outcome_slots) = pool_state
        .finalization_candidate_outcomes
        .get_mut(restart_position)
    else {
        return;
    };

    let Some(candidate_outcome_slot) = candidate_outcome_slots.get_mut(result.candidate_position)
    else {
        return;
    };

    if candidate_outcome_slot.is_some() {
        return;
    }

    *candidate_outcome_slot = Some(result.outcome);

    if candidate_outcome_slots.iter().any(Option::is_none) {
        return;
    }

    let candidate_outcomes = candidate_outcome_slots
        .iter_mut()
        .filter_map(Option::take)
        .collect::<Vec<_>>();

    let selected_outcome = select_residual_control_finalization_outcome(
        &pool_state.states[restart_position],
        candidate_outcomes,
        context,
        solver_settings,
    );

    if let Some(outcome_slot) = pool_state.restart_outcomes.get_mut(restart_position) {
        if outcome_slot.is_some() {
            return;
        }

        pool_state.completed_finalization_count =
            pool_state.completed_finalization_count.saturating_add(1);

        *outcome_slot = Some(selected_outcome);
    }

    if let Some(outcome) = pool_state
        .restart_outcomes
        .get(restart_position)
        .and_then(Option::as_ref)
    {
        emit_residual_control_finalization_progress(
            progress_sender,
            outcome,
            pool_state.restart_outcomes.as_slice(),
            pool_state.completed_search_evaluation_count,
            pool_state.completed_finalization_count,
            pool_state.states.len(),
            total_progress_units,
        );
    }
}

fn residual_control_pool_completed(pool_state: &ResidualControlWorkerPoolState<'_>) -> bool {
    pool_state.restart_outcomes.iter().all(Option::is_some)
}
