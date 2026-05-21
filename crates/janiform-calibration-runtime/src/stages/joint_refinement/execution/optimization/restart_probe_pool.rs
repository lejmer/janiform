use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageRestartEvaluationTimingDiagnostic,
};

use crate::stages::adaptive_direct_search::{
    adaptive_direct_search_poll_directions, initial_mesh_radii, mesh_is_at_tolerance,
    mesh_poll_candidate, objective_improves, refine_mesh_radii,
};
use crate::stages::joint_refinement::contracts::JointRefinementStageProgressUpdate;
use crate::stages::return_components::ContinuousLikelihoodEvaluatorCounts;

use super::super::selection::joint_parameter_name_by_index;
use super::super::{
    JointRefinementActiveParameterLayout, JointRefinementContext, JointRefinementRestartOutcome,
    elapsed_millis, joint_refinement_parallel_worker_count,
};
use super::workflow::{
    JOINT_REFINEMENT_DIRECT_SEARCH_FAILED_POLL_REFINEMENT_DIVISOR, JointRefinementBudgetSchedule,
    JointRefinementRestartOptimizationOutput, ParameterBlockObjectiveEvaluation,
    evaluate_prechecked_full_parameter_vector_objective,
    joint_refinement_has_fatal_execution_failure,
    joint_refinement_restart_full_vector_direct_search_settings,
    restart_candidate_total_evaluation_budget, restart_cycle_evaluation_budget,
};

use finalization::finalize_restart_probe_outcomes;
use progress::{
    emit_batched_restart_pool_finished, emit_batched_restart_progress,
    restart_probe_evaluation_timing_diagnostic,
};

mod finalization;
mod progress;

const RESTART_PROBE_SCHEDULE_CATEGORY_INITIAL: &str = "initial";
const RESTART_PROBE_SCHEDULE_CATEGORY_POLL: &str = "poll";
const RESTART_STOP_REASON_POOL_BUDGET: &str = "restart_pool_budget";
const RESTART_STOP_REASON_NO_SCHEDULABLE_WORK: &str = "restart_pool_no_schedulable_work";
const RESTART_WORK_ITEM_STATUS_COMPLETED: &str = "completed";
const RESTART_WORK_ITEM_STATUS_QUEUED: &str = "queued";
const RESTART_WORK_ITEM_STATUS_REJECTED: &str = "rejected";
const RESTART_WORK_ITEM_STATUS_RUNNING: &str = "running";
const RESTART_WORK_ITEM_STATUS_WAITING: &str = "waiting";

#[derive(Debug, Clone)]
struct RestartProbeState {
    restart_id: String,
    seed_vector: Vec<f64>,
    accepted_vector: Vec<f64>,
    best_vector: Vec<f64>,
    accepted_objective: Option<f64>,
    best_objective: Option<f64>,
    poll_best_vector: Option<Vec<f64>>,
    poll_best_objective: Option<f64>,
    mesh_radii: Vec<f64>,
    next_poll_direction_position: usize,
    completed_poll_rounds: usize,
    evaluation_count: usize,
    max_evaluation_count: usize,
    in_flight_count: usize,
    stop_reason: String,
    completed: bool,
    exact_likelihood_evaluator_counts: Option<ContinuousLikelihoodEvaluatorCounts>,
}

#[derive(Debug, Clone)]
struct RestartProbeJob {
    restart_position: usize,
    parameter_vector: Vec<f64>,
    schedule_category: &'static str,
    candidate_evaluation_count: usize,
    poll_direction_position: Option<usize>,
    poll_direction_label: String,
}

#[derive(Debug, Clone)]
struct RestartProbeResult {
    restart_position: usize,
    parameter_vector: Vec<f64>,
    schedule_category: &'static str,
    candidate_evaluation_count: usize,
    poll_direction_position: Option<usize>,
    poll_direction_label: String,
    elapsed_millis: u64,
    evaluation: Option<ParameterBlockObjectiveEvaluation>,
}

struct RestartProbeFinalizationResult {
    restart_position: usize,
    outcome: JointRefinementRestartOutcome,
}

struct RestartProbeWorkerFillInput<'a, 'scope> {
    scope: &'a rayon::Scope<'scope>,
    result_sender: &'a std::sync::mpsc::Sender<RestartProbeResult>,
    states: &'a mut [RestartProbeState],
    poll_directions: &'a [Vec<f64>],
    context: &'scope JointRefinementContext,
    solver_settings: &'scope CalibrationOptimizerSettings,
    search_pool_budget: usize,
    worker_capacity: usize,
}

pub(super) fn optimize_restart_seeds_batched_poll_pool(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> JointRefinementRestartOptimizationOutput {
    let mut states =
        build_restart_probe_states(restart_seeds, context, solver_settings, budget_schedule);

    let poll_directions =
        adaptive_direct_search_poll_directions(context.active_parameter_layout.vector_length());

    let search_pool_budget =
        restart_pool_total_evaluation_budget(context, solver_settings, budget_schedule);

    let total_progress_units = search_pool_budget.saturating_add(states.len()).max(1);

    let mut evaluation_timing_diagnostics = Vec::with_capacity(search_pool_budget);

    let completed_search_evaluation_count = run_restart_probe_worker_pool(
        states.as_mut_slice(),
        poll_directions.as_slice(),
        context,
        solver_settings,
        search_pool_budget,
        total_progress_units,
        progress_sender,
        &mut evaluation_timing_diagnostics,
    );

    let restart_outcomes = finalize_restart_probe_outcomes(
        states,
        context,
        solver_settings,
        completed_search_evaluation_count,
        total_progress_units,
        progress_sender,
    );

    emit_batched_restart_pool_finished(progress_sender, restart_outcomes.as_slice());

    JointRefinementRestartOptimizationOutput {
        restart_outcomes,
        evaluation_timing_diagnostics,
    }
}

fn build_restart_probe_states(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
) -> Vec<RestartProbeState> {
    let cycle_budget = restart_cycle_evaluation_budget(context, solver_settings).max(1);

    let max_candidate_budget = ((cycle_budget as f64)
        * budget_schedule
            .calibration_effort
            .restart_pool_max_candidate_budget_scale)
        .ceil()
        .max(1.0) as usize;

    restart_seeds
        .iter()
        .map(|(restart_id, seed_vector)| {
            // Seed, accepted, and best vectors are independent state because the pool mutates
            // the accepted incumbent without losing the restart seed or best feasible probe.
            RestartProbeState {
                restart_id: restart_id.clone(),
                seed_vector: seed_vector.clone(),
                accepted_vector: seed_vector.clone(),
                best_vector: seed_vector.clone(),
                accepted_objective: None,
                best_objective: None,
                poll_best_vector: None,
                poll_best_objective: None,
                mesh_radii: initial_mesh_radii(
                    context.lower_bounds.as_slice(),
                    context.upper_bounds.as_slice(),
                    joint_refinement_restart_full_vector_direct_search_settings(
                        solver_settings,
                        seed_vector.len(),
                    )
                    .initial_mesh_divisor,
                    solver_settings.absolute_tolerance,
                ),
                next_poll_direction_position: 0,
                completed_poll_rounds: 0,
                evaluation_count: 0,
                max_evaluation_count: max_candidate_budget,
                in_flight_count: 0,
                stop_reason: RESTART_STOP_REASON_POOL_BUDGET.to_string(),
                completed: false,
                exact_likelihood_evaluator_counts: None,
            }
        })
        .collect()
}

fn restart_pool_total_evaluation_budget(
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

fn run_restart_probe_worker_pool(
    states: &mut [RestartProbeState],
    poll_directions: &[Vec<f64>],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    search_pool_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    evaluation_timing_diagnostics: &mut Vec<JointRefinementStageRestartEvaluationTimingDiagnostic>,
) -> usize {
    let worker_capacity = joint_refinement_parallel_worker_count(context).max(1);

    match context.stage_thread_pool.as_ref() {
        Some(thread_pool) => thread_pool.scope(|scope| {
            run_restart_probe_worker_scope(
                scope,
                states,
                poll_directions,
                context,
                solver_settings,
                search_pool_budget,
                total_progress_units,
                progress_sender,
                evaluation_timing_diagnostics,
                worker_capacity,
            )
        }),
        None => rayon::scope(|scope| {
            run_restart_probe_worker_scope(
                scope,
                states,
                poll_directions,
                context,
                solver_settings,
                search_pool_budget,
                total_progress_units,
                progress_sender,
                evaluation_timing_diagnostics,
                worker_capacity,
            )
        }),
    }
}

fn run_restart_probe_worker_scope<'scope>(
    scope: &rayon::Scope<'scope>,
    states: &mut [RestartProbeState],
    poll_directions: &[Vec<f64>],
    context: &'scope JointRefinementContext,
    solver_settings: &'scope CalibrationOptimizerSettings,
    search_pool_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    evaluation_timing_diagnostics: &mut Vec<JointRefinementStageRestartEvaluationTimingDiagnostic>,
    worker_capacity: usize,
) -> usize {
    let (result_sender, result_receiver) = std::sync::mpsc::channel::<RestartProbeResult>();

    let mut scheduled_evaluation_count = 0usize;
    let mut completed_evaluation_count = 0usize;
    let mut in_flight_count = 0usize;
    let mut next_restart_position = 0usize;

    fill_restart_probe_worker_slots(
        RestartProbeWorkerFillInput {
            scope,
            result_sender: &result_sender,
            states,
            poll_directions,
            context,
            solver_settings,
            search_pool_budget,
            worker_capacity,
        },
        &mut scheduled_evaluation_count,
        &mut in_flight_count,
        &mut next_restart_position,
    );

    while in_flight_count > 0 {
        let Ok(result) = result_receiver.recv() else {
            break;
        };

        in_flight_count = in_flight_count.saturating_sub(1);
        completed_evaluation_count = completed_evaluation_count.saturating_add(1);

        let restart_position = result.restart_position;

        let timing_diagnostic =
            restart_probe_evaluation_timing_diagnostic(&states[restart_position], &result);

        // Progress updates and the final diagnostic artifact both own this timing record.
        evaluation_timing_diagnostics.push(timing_diagnostic.clone());

        record_restart_probe_result(&mut states[restart_position], result, solver_settings);

        emit_batched_restart_progress(
            progress_sender,
            states,
            restart_position,
            completed_evaluation_count,
            total_progress_units,
            Some(timing_diagnostic),
        );

        if scheduled_evaluation_count >= search_pool_budget
            || joint_refinement_has_fatal_execution_failure(context)
        {
            continue;
        }

        fill_restart_probe_worker_slots(
            RestartProbeWorkerFillInput {
                scope,
                result_sender: &result_sender,
                states,
                poll_directions,
                context,
                solver_settings,
                search_pool_budget,
                worker_capacity,
            },
            &mut scheduled_evaluation_count,
            &mut in_flight_count,
            &mut next_restart_position,
        );
    }

    if scheduled_evaluation_count >= search_pool_budget
        || joint_refinement_has_fatal_execution_failure(context)
    {
        mark_pending_restart_probe_states_completed(states);
    } else {
        mark_unscheduled_restart_probe_states_completed(states);
    }

    completed_evaluation_count
}

fn fill_restart_probe_worker_slots(
    input: RestartProbeWorkerFillInput<'_, '_>,
    scheduled_evaluation_count: &mut usize,
    in_flight_count: &mut usize,
    next_restart_position: &mut usize,
) {
    while *in_flight_count < input.worker_capacity
        && *scheduled_evaluation_count < input.search_pool_budget
        && !joint_refinement_has_fatal_execution_failure(input.context)
    {
        let Some(probe_job) = next_restart_probe_job_round_robin(
            input.states,
            input.poll_directions,
            input.context,
            input.solver_settings,
            next_restart_position,
        ) else {
            break;
        };

        *scheduled_evaluation_count = (*scheduled_evaluation_count).saturating_add(1);
        *in_flight_count = (*in_flight_count).saturating_add(1);

        // Each worker gets its own channel handle so completed probes can be streamed back.
        spawn_restart_probe_job(
            input.scope,
            input.result_sender.clone(),
            probe_job,
            input.context,
            input.solver_settings,
        );
    }
}

fn spawn_restart_probe_job<'scope>(
    scope: &rayon::Scope<'scope>,
    result_sender: std::sync::mpsc::Sender<RestartProbeResult>,
    probe_job: RestartProbeJob,
    context: &'scope JointRefinementContext,
    solver_settings: &'scope CalibrationOptimizerSettings,
) {
    scope.spawn(move |_| {
        let evaluation_started_at = std::time::Instant::now();
        let evaluation = evaluate_prechecked_full_parameter_vector_objective(
            probe_job.parameter_vector.as_slice(),
            context,
            solver_settings,
        );
        let evaluation_elapsed_millis = elapsed_millis(evaluation_started_at);

        let _ = result_sender.send(RestartProbeResult {
            restart_position: probe_job.restart_position,
            parameter_vector: probe_job.parameter_vector,
            schedule_category: probe_job.schedule_category,
            candidate_evaluation_count: probe_job.candidate_evaluation_count,
            poll_direction_position: probe_job.poll_direction_position,
            poll_direction_label: probe_job.poll_direction_label,
            elapsed_millis: evaluation_elapsed_millis,
            evaluation,
        });
    });
}

fn next_restart_probe_job_round_robin(
    states: &mut [RestartProbeState],
    poll_directions: &[Vec<f64>],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    next_restart_position: &mut usize,
) -> Option<RestartProbeJob> {
    if states.is_empty() {
        return None;
    }

    for _ in 0..states.len() {
        let restart_position = *next_restart_position % states.len();
        *next_restart_position = (*next_restart_position).saturating_add(1) % states.len();

        refresh_restart_probe_state(
            &mut states[restart_position],
            poll_directions.len(),
            solver_settings,
        );

        loop {
            if let Some(probe_job) =
                next_restart_probe_job(states, restart_position, poll_directions, context)
            {
                return Some(probe_job);
            }

            let state_completed_poll_rounds = states[restart_position].completed_poll_rounds;
            let state_next_poll_direction_position =
                states[restart_position].next_poll_direction_position;

            refresh_restart_probe_state(
                &mut states[restart_position],
                poll_directions.len(),
                solver_settings,
            );

            let state = &states[restart_position];
            if state.completed
                || state.in_flight_count > 0
                || (state.completed_poll_rounds == state_completed_poll_rounds
                    && state.next_poll_direction_position == state_next_poll_direction_position)
            {
                break;
            }
        }
    }

    None
}

fn next_restart_probe_job(
    states: &mut [RestartProbeState],
    restart_position: usize,
    poll_directions: &[Vec<f64>],
    context: &JointRefinementContext,
) -> Option<RestartProbeJob> {
    let state = states.get_mut(restart_position)?;

    if state.completed {
        return None;
    }

    if state.evaluation_count == 0 && state.in_flight_count > 0 {
        return None;
    }

    if !restart_probe_state_has_remaining_budget(state) {
        state.completed = true;
        state.stop_reason = RESTART_STOP_REASON_POOL_BUDGET.to_string();
        return None;
    }

    if state.evaluation_count == 0 {
        let candidate_evaluation_count = state
            .evaluation_count
            .saturating_add(state.in_flight_count)
            .saturating_add(1);

        state.in_flight_count = state.in_flight_count.saturating_add(1);

        // The worker job owns a candidate snapshot so Rayon never borrows mutable restart state.
        return Some(RestartProbeJob {
            restart_position,
            parameter_vector: state.accepted_vector.clone(),
            schedule_category: RESTART_PROBE_SCHEDULE_CATEGORY_INITIAL,
            candidate_evaluation_count,
            poll_direction_position: None,
            poll_direction_label: "initial".to_string(),
        });
    }

    while let Some(poll_direction) = poll_directions.get(state.next_poll_direction_position) {
        let poll_direction_position = state.next_poll_direction_position;

        let candidate_vector = mesh_poll_candidate(
            state.accepted_vector.as_slice(),
            poll_direction.as_slice(),
            state.mesh_radii.as_slice(),
            context.lower_bounds.as_slice(),
            context.upper_bounds.as_slice(),
        );

        state.next_poll_direction_position = state.next_poll_direction_position.saturating_add(1);

        if candidate_vector == state.accepted_vector {
            continue;
        }

        let candidate_evaluation_count = state
            .evaluation_count
            .saturating_add(state.in_flight_count)
            .saturating_add(1);

        state.in_flight_count = state.in_flight_count.saturating_add(1);

        return Some(RestartProbeJob {
            restart_position,
            parameter_vector: candidate_vector,
            schedule_category: RESTART_PROBE_SCHEDULE_CATEGORY_POLL,
            candidate_evaluation_count,
            poll_direction_position: Some(poll_direction_position),
            poll_direction_label: restart_poll_direction_label(
                poll_direction_position,
                state.accepted_vector.len(),
                &context.active_parameter_layout,
            ),
        });
    }

    None
}

fn restart_probe_state_has_remaining_budget(state: &RestartProbeState) -> bool {
    state.evaluation_count.saturating_add(state.in_flight_count) < state.max_evaluation_count
}

fn refresh_restart_probe_state(
    state: &mut RestartProbeState,
    poll_direction_count: usize,
    solver_settings: &CalibrationOptimizerSettings,
) {
    if state.completed || state.evaluation_count == 0 || state.in_flight_count > 0 {
        return;
    }

    if mesh_is_at_tolerance(
        state.mesh_radii.as_slice(),
        state.accepted_vector.as_slice(),
        solver_settings.absolute_tolerance,
        solver_settings.relative_tolerance,
    ) {
        state.completed = true;
        state.stop_reason = "tolerance".to_string();
        return;
    }

    if state.next_poll_direction_position < poll_direction_count {
        return;
    }

    let poll_improved = match (state.poll_best_objective, state.accepted_objective) {
        (Some(poll_best), Some(accepted)) => objective_improves(
            poll_best,
            accepted,
            solver_settings.absolute_tolerance,
            solver_settings.relative_tolerance,
        ),
        (Some(_), None) => true,
        _ => false,
    };

    if poll_improved {
        if let Some(poll_best_vector) = state.poll_best_vector.take() {
            state.accepted_vector = poll_best_vector;
        }

        state.accepted_objective = state.poll_best_objective;
        state.stop_reason = "poll_improved".to_string();
    } else {
        if !refine_mesh_radii(
            state.mesh_radii.as_mut_slice(),
            JOINT_REFINEMENT_DIRECT_SEARCH_FAILED_POLL_REFINEMENT_DIVISOR,
            solver_settings.absolute_tolerance,
        ) {
            state.completed = true;
            state.stop_reason = "invalid_step".to_string();

            return;
        }

        state.stop_reason = "mesh_refined".to_string();
    }

    state.poll_best_objective = None;
    state.next_poll_direction_position = 0;
    state.completed_poll_rounds = state.completed_poll_rounds.saturating_add(1);

    if mesh_is_at_tolerance(
        state.mesh_radii.as_slice(),
        state.accepted_vector.as_slice(),
        solver_settings.absolute_tolerance,
        solver_settings.relative_tolerance,
    ) {
        state.completed = true;
        state.stop_reason = "tolerance".to_string();
    }
}

fn restart_poll_direction_label(
    poll_direction_position: usize,
    active_parameter_dimension: usize,
    active_parameter_layout: &JointRefinementActiveParameterLayout,
) -> String {
    let coordinate_direction_count = active_parameter_dimension.saturating_mul(2);
    if poll_direction_position < coordinate_direction_count {
        let parameter_index = poll_direction_position / 2;

        let direction_label = if poll_direction_position % 2 == 0 {
            "positive"
        } else {
            "negative"
        };

        let parameter_label =
            joint_parameter_name_by_index(parameter_index, active_parameter_layout)
                .unwrap_or_else(|| format!("parameter_{parameter_index}"));

        return format!("{direction_label}.{parameter_label}");
    }

    format!("direction_{poll_direction_position}")
}

fn record_restart_probe_result(
    state: &mut RestartProbeState,
    result: RestartProbeResult,
    solver_settings: &CalibrationOptimizerSettings,
) {
    state.in_flight_count = state.in_flight_count.saturating_sub(1);
    state.evaluation_count = state.evaluation_count.saturating_add(1);

    let Some(evaluation) = result.evaluation else {
        state.stop_reason = "invalid_probe".to_string();

        if state.accepted_objective.is_none() {
            state.completed = true;
        }

        return;
    };

    state.exact_likelihood_evaluator_counts = evaluation.exact_likelihood_evaluator_counts;

    if result.poll_direction_position.is_none() {
        state.accepted_objective = Some(evaluation.objective);
    } else {
        // Poll winner storage is independent from all-time best storage because this
        // same evaluated candidate may need to update both roles below.
        match state.poll_best_objective.or(state.accepted_objective) {
            Some(poll_incumbent_objective)
                if objective_improves(
                    evaluation.objective,
                    poll_incumbent_objective,
                    solver_settings.absolute_tolerance,
                    solver_settings.relative_tolerance,
                ) =>
            {
                state.poll_best_objective = Some(evaluation.objective);
                state.poll_best_vector = Some(result.parameter_vector.clone());
            }
            None if evaluation.objective.is_finite() => {
                state.poll_best_objective = Some(evaluation.objective);
                state.poll_best_vector = Some(result.parameter_vector.clone());
            }
            _ => {}
        }
    }

    if evaluation.objective.is_finite()
        && state
            .best_objective
            .map(|best_objective| evaluation.objective < best_objective)
            .unwrap_or(true)
    {
        state.best_objective = Some(evaluation.objective);
        state.best_vector = result.parameter_vector;
    }

    if state.evaluation_count >= state.max_evaluation_count {
        state.completed = true;
        state.stop_reason = RESTART_STOP_REASON_POOL_BUDGET.to_string();
    }
}

fn mark_pending_restart_probe_states_completed(states: &mut [RestartProbeState]) {
    for state in states {
        if state.in_flight_count > 0 || state.completed {
            continue;
        }

        state.completed = true;
        state.stop_reason = RESTART_STOP_REASON_POOL_BUDGET.to_string();
    }
}

fn mark_unscheduled_restart_probe_states_completed(states: &mut [RestartProbeState]) {
    for state in states {
        if state.completed || state.in_flight_count > 0 {
            continue;
        }

        state.completed = true;
        state.stop_reason = RESTART_STOP_REASON_NO_SCHEDULABLE_WORK.to_string();
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;

    use super::{
        ParameterBlockObjectiveEvaluation, RESTART_PROBE_SCHEDULE_CATEGORY_POLL,
        RestartProbeResult, RestartProbeState, record_restart_probe_result,
        refresh_restart_probe_state,
    };

    fn test_solver_settings() -> CalibrationOptimizerSettings {
        CalibrationOptimizerSettings::try_new("restart-probe-test", 16, 2, 0.1, 0.01, 10.0)
            .expect("test optimizer settings should be valid")
    }

    fn running_poll_state() -> RestartProbeState {
        RestartProbeState {
            restart_id: "restart_0".to_string(),
            seed_vector: vec![0.0],
            accepted_vector: vec![0.0],
            best_vector: vec![0.0],
            accepted_objective: Some(10.0),
            best_objective: Some(10.0),
            poll_best_vector: None,
            poll_best_objective: None,
            mesh_radii: vec![1.0],
            next_poll_direction_position: 1,
            completed_poll_rounds: 0,
            evaluation_count: 1,
            max_evaluation_count: 16,
            in_flight_count: 1,
            stop_reason: "running".to_string(),
            completed: false,
            exact_likelihood_evaluator_counts: None,
        }
    }

    fn poll_result(objective: f64) -> RestartProbeResult {
        RestartProbeResult {
            restart_position: 0,
            parameter_vector: vec![1.0],
            schedule_category: RESTART_PROBE_SCHEDULE_CATEGORY_POLL,
            candidate_evaluation_count: 2,
            poll_direction_position: Some(0),
            poll_direction_label: "positive.parameter_0".to_string(),
            elapsed_millis: 1,
            evaluation: Some(ParameterBlockObjectiveEvaluation {
                objective,
                admissibility_blocker_count: 0,
                max_standardized_residual: Some(0.0),
                exact_likelihood_evaluator_counts: None,
            }),
        }
    }

    #[test]
    fn poll_candidate_requires_tolerance_improvement_for_acceptance() {
        let solver_settings = test_solver_settings();

        let mut state = running_poll_state();

        record_restart_probe_result(&mut state, poll_result(9.95), &solver_settings);
        refresh_restart_probe_state(&mut state, 1, &solver_settings);

        assert_eq!(state.accepted_vector, vec![0.0]);
        assert_eq!(state.accepted_objective, Some(10.0));
        assert_eq!(state.best_vector, vec![1.0]);
        assert_eq!(state.best_objective, Some(9.95));
        assert_eq!(state.poll_best_objective, None);
        assert_eq!(state.stop_reason, "mesh_refined");
    }

    #[test]
    fn poll_candidate_with_tolerance_improvement_is_accepted() {
        let solver_settings = test_solver_settings();

        let mut state = running_poll_state();

        record_restart_probe_result(&mut state, poll_result(9.8), &solver_settings);
        refresh_restart_probe_state(&mut state, 1, &solver_settings);

        assert_eq!(state.accepted_vector, vec![1.0]);
        assert_eq!(state.accepted_objective, Some(9.8));
        assert_eq!(state.best_vector, vec![1.0]);
        assert_eq!(state.best_objective, Some(9.8));
        assert_eq!(state.poll_best_objective, None);
        assert_eq!(state.stop_reason, "poll_improved");
    }
}
