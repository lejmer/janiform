use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageRestartEvaluationTimingDiagnostic,
};
use janiform_work_scheduler::{DynamicWorkPoolInput, run_dynamic_work_pool};

use crate::stages::joint_refinement::contracts::JointRefinementStageProgressUpdate;

use super::super::super::{
    JointRefinementContext, JointRefinementRestartOutcome, joint_refinement_parallel_worker_count,
};
use super::super::workflow::joint_refinement_has_fatal_execution_failure;
use super::finalization::{
    RestartProbeFinalizationJob, RestartProbeFinalizationResult, run_restart_probe_finalization_job,
};
use super::progress::{
    emit_batched_restart_progress, emit_restart_finalization_progress,
    emit_restart_finalization_started, restart_probe_evaluation_timing_diagnostic,
};
use super::{
    RestartProbeJob, RestartProbeResult, RestartProbeState,
    mark_pending_restart_probe_states_completed, mark_unscheduled_restart_probe_states_completed,
    next_restart_probe_job_round_robin, record_restart_probe_result, run_restart_probe_job,
};

enum RestartProbePoolJob {
    Search(RestartProbeJob),
    Finalization(Box<RestartProbeFinalizationJob>),
}

enum RestartProbePoolResult {
    Search(RestartProbeResult),
    Finalization(Box<RestartProbeFinalizationResult>),
}

struct RestartProbeWorkerPoolState<'a> {
    states: &'a mut [RestartProbeState],
    restart_outcomes: Vec<Option<JointRefinementRestartOutcome>>,
    finalization_scheduled: Vec<bool>,
    next_restart_position: usize,
    next_finalization_restart_position: usize,
    scheduled_search_evaluation_count: usize,
    completed_search_evaluation_count: usize,
    completed_finalization_count: usize,
    evaluation_timing_diagnostics:
        &'a mut Vec<JointRefinementStageRestartEvaluationTimingDiagnostic>,
}

pub(super) struct RestartProbeWorkerPoolInput<'a> {
    pub(super) states: &'a mut [RestartProbeState],
    pub(super) poll_directions: &'a [Vec<f64>],
    pub(super) context: &'a JointRefinementContext,
    pub(super) solver_settings: &'a CalibrationOptimizerSettings,
    pub(super) search_pool_budget: usize,
    pub(super) total_progress_units: usize,
    pub(super) progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(super) evaluation_timing_diagnostics:
        &'a mut Vec<JointRefinementStageRestartEvaluationTimingDiagnostic>,
}

pub(super) fn run_restart_probe_worker_pool(
    input: RestartProbeWorkerPoolInput<'_>,
) -> Vec<JointRefinementRestartOutcome> {
    let worker_capacity = joint_refinement_parallel_worker_count(input.context).max(1);

    run_restart_probe_worker_pool_with_scheduler(input, worker_capacity)
}

fn run_restart_probe_worker_pool_with_scheduler(
    input: RestartProbeWorkerPoolInput<'_>,
    worker_capacity: usize,
) -> Vec<JointRefinementRestartOutcome> {
    let restart_count = input.states.len();

    let mut pool_state = RestartProbeWorkerPoolState {
        states: input.states,
        restart_outcomes: std::iter::repeat_with(|| None)
            .take(restart_count)
            .collect(),
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
            |pool_state: &mut RestartProbeWorkerPoolState<'_>, _status| {
                next_restart_probe_pool_job(
                    pool_state,
                    input.poll_directions,
                    input.context,
                    input.solver_settings,
                    input.search_pool_budget,
                    input.total_progress_units,
                    input.progress_sender,
                )
            },
            |pool_job, _cancellation_token| {
                run_restart_probe_pool_job(pool_job, input.context, input.solver_settings)
            },
            |pool_state: &mut RestartProbeWorkerPoolState<'_>, result, _status| {
                record_restart_probe_pool_result(
                    pool_state,
                    result,
                    input.solver_settings,
                    input.search_pool_budget,
                    input.total_progress_units,
                    input.progress_sender,
                    input.context,
                );
            },
            |pool_state: &RestartProbeWorkerPoolState<'_>, _status| {
                restart_probe_pool_completed(pool_state)
            },
        ),
    );

    pool_state.restart_outcomes.into_iter().flatten().collect()
}

fn next_restart_probe_pool_job(
    pool_state: &mut RestartProbeWorkerPoolState<'_>,
    poll_directions: &[Vec<f64>],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    search_pool_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Option<RestartProbePoolJob> {
    if restart_probe_search_scheduling_complete(pool_state, search_pool_budget, context) {
        mark_pending_restart_probe_states_completed(pool_state.states);
    } else {
        let search_job = next_restart_probe_job_round_robin(
            pool_state.states,
            poll_directions,
            context,
            solver_settings,
            &mut pool_state.next_restart_position,
        );

        if let Some(search_job) = search_job {
            pool_state.scheduled_search_evaluation_count = pool_state
                .scheduled_search_evaluation_count
                .saturating_add(1);

            return Some(RestartProbePoolJob::Search(search_job));
        }

        if restart_probe_search_in_flight_count(pool_state.states) == 0 {
            mark_unscheduled_restart_probe_states_completed(pool_state.states);
        }
    }

    next_restart_probe_finalization_job(pool_state, total_progress_units, progress_sender)
        .map(Box::new)
        .map(RestartProbePoolJob::Finalization)
}

fn restart_probe_search_scheduling_complete(
    pool_state: &RestartProbeWorkerPoolState<'_>,
    search_pool_budget: usize,
    context: &JointRefinementContext,
) -> bool {
    pool_state.scheduled_search_evaluation_count >= search_pool_budget
        || joint_refinement_has_fatal_execution_failure(context)
}

fn restart_probe_search_in_flight_count(states: &[RestartProbeState]) -> usize {
    states
        .iter()
        .map(|state| state.in_flight_count)
        .sum::<usize>()
}

fn next_restart_probe_finalization_job(
    pool_state: &mut RestartProbeWorkerPoolState<'_>,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Option<RestartProbeFinalizationJob> {
    if pool_state.states.is_empty() {
        return None;
    }

    for _ in 0..pool_state.states.len() {
        let restart_position =
            pool_state.next_finalization_restart_position % pool_state.states.len();

        pool_state.next_finalization_restart_position = pool_state
            .next_finalization_restart_position
            .saturating_add(1)
            % pool_state.states.len();

        if !restart_probe_finalization_is_schedulable(pool_state, restart_position) {
            continue;
        }

        let Some(state) = pool_state.states.get(restart_position) else {
            continue;
        };

        emit_restart_finalization_started(
            progress_sender,
            state,
            pool_state.states.len(),
            pool_state.completed_search_evaluation_count,
            pool_state.completed_finalization_count,
            total_progress_units,
        );

        if let Some(finalization_slot) = pool_state.finalization_scheduled.get_mut(restart_position)
        {
            *finalization_slot = true;
        }

        // Finalization workers own a completed restart snapshot so the scheduler can keep
        // mutating unrelated restart state while final loss runs.
        return Some(RestartProbeFinalizationJob {
            restart_position,
            state: state.clone(),
        });
    }

    None
}

fn restart_probe_finalization_is_schedulable(
    pool_state: &RestartProbeWorkerPoolState<'_>,
    restart_position: usize,
) -> bool {
    let Some(state) = pool_state.states.get(restart_position) else {
        return false;
    };

    if !state.completed || state.in_flight_count > 0 {
        return false;
    }

    if pool_state
        .finalization_scheduled
        .get(restart_position)
        .copied()
        .unwrap_or(false)
    {
        return false;
    }

    pool_state
        .restart_outcomes
        .get(restart_position)
        .is_some_and(Option::is_none)
}

fn restart_probe_pool_completed(pool_state: &RestartProbeWorkerPoolState<'_>) -> bool {
    pool_state.restart_outcomes.iter().all(Option::is_some)
}

fn run_restart_probe_pool_job(
    pool_job: RestartProbePoolJob,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> RestartProbePoolResult {
    match pool_job {
        RestartProbePoolJob::Search(probe_job) => RestartProbePoolResult::Search(
            run_restart_probe_job(probe_job, context, solver_settings),
        ),
        RestartProbePoolJob::Finalization(finalization_job) => {
            RestartProbePoolResult::Finalization(Box::new(run_restart_probe_finalization_job(
                *finalization_job,
                context,
                solver_settings,
            )))
        }
    }
}

fn record_restart_probe_pool_result(
    pool_state: &mut RestartProbeWorkerPoolState<'_>,
    result: RestartProbePoolResult,
    solver_settings: &CalibrationOptimizerSettings,
    search_pool_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    context: &JointRefinementContext,
) {
    match result {
        RestartProbePoolResult::Search(search_result) => {
            record_restart_probe_search_result(
                pool_state,
                search_result,
                solver_settings,
                search_pool_budget,
                total_progress_units,
                progress_sender,
                context,
            );
        }
        RestartProbePoolResult::Finalization(finalization_result) => {
            record_restart_probe_finalization_result(
                pool_state,
                *finalization_result,
                total_progress_units,
                progress_sender,
            );
        }
    }
}

fn record_restart_probe_search_result(
    pool_state: &mut RestartProbeWorkerPoolState<'_>,
    result: RestartProbeResult,
    solver_settings: &CalibrationOptimizerSettings,
    search_pool_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    context: &JointRefinementContext,
) {
    let restart_position = result.restart_position;

    let timing_diagnostic =
        restart_probe_evaluation_timing_diagnostic(&pool_state.states[restart_position], &result);

    // Progress updates and the final diagnostic artifact both own this timing record.
    pool_state
        .evaluation_timing_diagnostics
        .push(timing_diagnostic.clone());

    record_restart_probe_result(
        &mut pool_state.states[restart_position],
        result,
        solver_settings,
    );

    pool_state.completed_search_evaluation_count = pool_state
        .completed_search_evaluation_count
        .saturating_add(1);

    if restart_probe_search_scheduling_complete(pool_state, search_pool_budget, context) {
        mark_pending_restart_probe_states_completed(pool_state.states);
    }

    emit_batched_restart_progress(
        progress_sender,
        pool_state.states,
        restart_position,
        pool_state.completed_search_evaluation_count,
        total_progress_units,
        Some(timing_diagnostic),
    );
}

fn record_restart_probe_finalization_result(
    pool_state: &mut RestartProbeWorkerPoolState<'_>,
    result: RestartProbeFinalizationResult,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) {
    let restart_position = result.restart_position;
    if let Some(outcome_slot) = pool_state.restart_outcomes.get_mut(restart_position) {
        if outcome_slot.is_none() {
            pool_state.completed_finalization_count =
                pool_state.completed_finalization_count.saturating_add(1);
        }

        *outcome_slot = Some(result.outcome);
    }

    if let Some(outcome) = pool_state
        .restart_outcomes
        .get(restart_position)
        .and_then(Option::as_ref)
    {
        emit_restart_finalization_progress(
            progress_sender,
            outcome,
            pool_state.restart_outcomes.as_slice(),
            pool_state.completed_finalization_count,
            pool_state.states.len(),
            pool_state.completed_search_evaluation_count,
            total_progress_units,
        );
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::JointRefinementStageRestartEvaluationTimingDiagnostic;

    use super::super::{
        RESTART_STOP_REASON_POOL_BUDGET, RestartProbeState,
        mark_pending_restart_probe_states_completed,
    };
    use super::{
        RestartProbeWorkerPoolState, next_restart_probe_finalization_job,
        restart_probe_finalization_is_schedulable,
    };

    fn completed_search_state(restart_id: &str) -> RestartProbeState {
        RestartProbeState {
            restart_id: restart_id.to_string(),
            seed_vector: vec![0.0],
            accepted_vector: vec![0.0],
            best_vector: vec![0.0],
            accepted_objective: Some(10.0),
            best_objective: Some(10.0),
            poll_best_vector: None,
            poll_best_objective: None,
            mesh_radii: vec![1.0],
            next_poll_direction_position: 0,
            completed_poll_rounds: 0,
            evaluation_count: 4,
            max_evaluation_count: 4,
            in_flight_count: 0,
            stop_reason: RESTART_STOP_REASON_POOL_BUDGET.to_string(),
            completed: true,
            exact_likelihood_evaluator_counts: None,
        }
    }

    fn pending_search_state(restart_id: &str) -> RestartProbeState {
        RestartProbeState {
            restart_id: restart_id.to_string(),
            seed_vector: vec![0.0],
            accepted_vector: vec![0.0],
            best_vector: vec![0.0],
            accepted_objective: Some(10.0),
            best_objective: Some(10.0),
            poll_best_vector: None,
            poll_best_objective: None,
            mesh_radii: vec![1.0],
            next_poll_direction_position: 0,
            completed_poll_rounds: 0,
            evaluation_count: 3,
            max_evaluation_count: 8,
            in_flight_count: 0,
            stop_reason: "running".to_string(),
            completed: false,
            exact_likelihood_evaluator_counts: None,
        }
    }

    fn in_flight_search_state(restart_id: &str) -> RestartProbeState {
        let mut state = pending_search_state(restart_id);
        state.in_flight_count = 1;

        state
    }

    fn test_pool_state<'a>(
        states: &'a mut [RestartProbeState],
        evaluation_timing_diagnostics: &'a mut Vec<
            JointRefinementStageRestartEvaluationTimingDiagnostic,
        >,
    ) -> RestartProbeWorkerPoolState<'a> {
        let restart_count = states.len();

        RestartProbeWorkerPoolState {
            states,
            restart_outcomes: std::iter::repeat_with(|| None)
                .take(restart_count)
                .collect(),
            finalization_scheduled: vec![false; restart_count],
            next_restart_position: 0,
            next_finalization_restart_position: 0,
            scheduled_search_evaluation_count: 0,
            completed_search_evaluation_count: 0,
            completed_finalization_count: 0,
            evaluation_timing_diagnostics,
        }
    }

    #[test]
    fn completed_restart_schedules_finalization_while_search_is_in_flight() {
        let mut states = vec![
            completed_search_state("finished"),
            in_flight_search_state("running"),
        ];

        let mut evaluation_timing_diagnostics = Vec::new();

        let mut pool_state =
            test_pool_state(states.as_mut_slice(), &mut evaluation_timing_diagnostics);

        pool_state.completed_search_evaluation_count = 7;

        let finalization_job = next_restart_probe_finalization_job(&mut pool_state, 12, None)
            .expect("completed restart should schedule finalization");

        assert_eq!(finalization_job.restart_position, 0);
        assert_eq!(finalization_job.state.restart_id, "finished");
        assert_eq!(pool_state.finalization_scheduled, vec![true, false]);
    }

    #[test]
    fn finalization_scheduler_does_not_schedule_restart_twice() {
        let mut states = vec![completed_search_state("finished")];
        let mut evaluation_timing_diagnostics = Vec::new();

        let mut pool_state =
            test_pool_state(states.as_mut_slice(), &mut evaluation_timing_diagnostics);

        assert!(next_restart_probe_finalization_job(&mut pool_state, 5, None).is_some());
        assert!(next_restart_probe_finalization_job(&mut pool_state, 5, None).is_none());
    }

    #[test]
    fn finalization_requires_completed_idle_unscheduled_restart() {
        let mut states = vec![
            pending_search_state("pending"),
            in_flight_search_state("running"),
            completed_search_state("finished"),
        ];

        let mut evaluation_timing_diagnostics = Vec::new();

        let mut pool_state =
            test_pool_state(states.as_mut_slice(), &mut evaluation_timing_diagnostics);

        assert!(!restart_probe_finalization_is_schedulable(&pool_state, 0));
        assert!(!restart_probe_finalization_is_schedulable(&pool_state, 1));
        assert!(restart_probe_finalization_is_schedulable(&pool_state, 2));

        pool_state.finalization_scheduled[2] = true;

        assert!(!restart_probe_finalization_is_schedulable(&pool_state, 2));
    }

    #[test]
    fn exhausted_pool_budget_completes_idle_states_before_in_flight_states() {
        let mut states = vec![
            pending_search_state("idle"),
            in_flight_search_state("running"),
        ];

        mark_pending_restart_probe_states_completed(states.as_mut_slice());

        assert!(states[0].completed);
        assert_eq!(states[0].stop_reason, RESTART_STOP_REASON_POOL_BUDGET);
        assert!(!states[1].completed);
    }
}
