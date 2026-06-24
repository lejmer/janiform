use std::collections::VecDeque;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_work_scheduler::{DynamicWorkPoolInput, run_dynamic_work_pool};

use crate::stages::joint_refinement::contracts::JointRefinementStageProgressUpdate;

use super::super::{
    JointRefinementCandidateOutcome, JointRefinementContext, elapsed_millis,
    joint_refinement_parallel_worker_count,
};
use super::evaluation_timing::{
    CandidateFinalizationTimingDiagnosticInput, candidate_finalization_timing_diagnostic,
};
use super::workflow::{
    JointRefinementBudgetSchedule, JointRefinementPopulationRefinementOutput,
    evaluate_finalized_candidate, evaluate_prechecked_full_parameter_vector_objective,
    joint_refinement_has_fatal_execution_failure,
};

mod archive;
mod finalization;
mod policy;
mod progress;
mod sampling;
mod scheduling;
mod scoring;
mod state;
mod types;

use finalization::{
    next_population_control_finalization_job, record_population_control_finalization_result,
};
use policy::population_control_search_budget;
use progress::{
    emit_population_control_finished, emit_population_control_search_completed_progress,
    emit_population_control_search_scheduled_progress,
    population_control_evaluation_timing_diagnostic,
};
use scheduling::{next_population_control_search_job, population_control_pool_completed};
use state::{build_population_control_state, record_population_control_search_result};
use types::{
    PopulationControlFinalizationJob, PopulationControlFinalizationResult,
    PopulationControlPoolJob, PopulationControlPoolResult, PopulationControlSearchJob,
    PopulationControlSearchResult, PopulationControlWorkerPoolInput,
    PopulationControlWorkerPoolState,
};

const POPULATION_CONTROL_STOP_REASON_POOL_BUDGET: &str = "population_control_pool_budget";
const POPULATION_CONTROL_STOP_REASON_NO_SCHEDULABLE_WORK: &str =
    "population_control_no_schedulable_work";
const POPULATION_CONTROL_PROGRESS_LABEL: &str = "population refinement";
const POPULATION_CONTROL_SCHEDULE_CATEGORY_FINALIZATION: &str = "population_finalization";
const POPULATION_CONTROL_SCHEDULE_CATEGORY_SEED: &str = "population_seed";
const POPULATION_CONTROL_SCHEDULE_CATEGORY_SAMPLE: &str = "population_sample";

#[derive(Debug, Clone)]
struct PopulationControlCandidateRecord {
    candidate_id: String,
    parameter_vector: Vec<f64>,
    candidate_evaluation_count: usize,
    score: scoring::PopulationControlScore,
}

pub(super) fn optimize_candidate_seeds_population_control(
    candidate_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> JointRefinementPopulationRefinementOutput {
    let search_budget = population_control_search_budget(context, solver_settings, budget_schedule);

    let mut state =
        build_population_control_state(candidate_seeds, context, solver_settings, budget_schedule);

    let total_progress_units =
        policy::population_control_total_progress_units(search_budget, candidate_seeds.len());

    let mut evaluation_timing_diagnostics = Vec::with_capacity(search_budget);

    let candidate_outcomes = run_population_control_worker_pool(PopulationControlWorkerPoolInput {
        state: &mut state,
        context,
        solver_settings,
        search_budget,
        total_progress_units,
        progress_sender,
        evaluation_timing_diagnostics: &mut evaluation_timing_diagnostics,
    });

    emit_population_control_finished(progress_sender, candidate_outcomes.as_slice());

    JointRefinementPopulationRefinementOutput {
        candidate_outcomes,
        evaluation_timing_diagnostics,
    }
}

pub(super) fn population_control_planned_work_item_labels() -> Vec<String> {
    vec![POPULATION_CONTROL_PROGRESS_LABEL.to_string()]
}

fn run_population_control_worker_pool(
    input: PopulationControlWorkerPoolInput<'_>,
) -> Vec<JointRefinementCandidateOutcome> {
    let worker_capacity = joint_refinement_parallel_worker_count(input.context).max(1);
    let candidate_count = input.state.candidate_count();

    let mut pool_state = PopulationControlWorkerPoolState {
        state: input.state,
        worker_capacity,
        pending_search_jobs: VecDeque::new(),
        candidate_outcomes: std::iter::repeat_with(|| None)
            .take(candidate_count)
            .collect(),
        next_finalization_candidate_position: 0,
        finalization_scheduled: vec![false; candidate_count],
        evaluation_timing_diagnostics: input.evaluation_timing_diagnostics,
    };

    run_dynamic_work_pool(
        input.context.stage_thread_pool.as_deref(),
        DynamicWorkPoolInput::new(
            &mut pool_state,
            input.context.cancellation_token.shared_handle(),
            worker_capacity,
            |pool_state: &mut PopulationControlWorkerPoolState<'_>, _status| {
                next_population_control_pool_job(
                    pool_state,
                    input.context,
                    input.solver_settings,
                    input.search_budget,
                    input.total_progress_units,
                    input.progress_sender,
                )
            },
            |pool_job, _cancellation_token| {
                run_population_control_pool_job(pool_job, input.context, input.solver_settings)
            },
            |pool_state: &mut PopulationControlWorkerPoolState<'_>, result, _status| {
                record_population_control_pool_result(
                    pool_state,
                    result,
                    input.solver_settings,
                    input.search_budget,
                    input.total_progress_units,
                    input.progress_sender,
                    input.context,
                );
            },
            |pool_state: &PopulationControlWorkerPoolState<'_>, _status| {
                population_control_pool_completed(pool_state)
            },
        ),
    );

    pool_state
        .candidate_outcomes
        .into_iter()
        .flatten()
        .collect()
}

fn next_population_control_pool_job(
    pool_state: &mut PopulationControlWorkerPoolState<'_>,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    search_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Option<PopulationControlPoolJob> {
    if population_control_search_scheduling_allowed(pool_state, search_budget, context) {
        let search_job =
            next_population_control_search_job(pool_state, context, solver_settings, search_budget);

        if let Some(search_job) = search_job {
            emit_population_control_search_scheduled_progress(
                progress_sender,
                pool_state.state,
                &search_job,
                search_budget,
                total_progress_units,
            );

            return Some(PopulationControlPoolJob::Search(search_job));
        }
    }

    next_population_control_finalization_job(
        pool_state,
        search_budget,
        total_progress_units,
        progress_sender,
    )
    .map(PopulationControlPoolJob::Finalization)
}

fn population_control_search_scheduling_allowed(
    pool_state: &PopulationControlWorkerPoolState<'_>,
    search_budget: usize,
    context: &JointRefinementContext,
) -> bool {
    pool_state.state.scheduled_search_evaluation_count() < search_budget
        && !joint_refinement_has_fatal_execution_failure(context)
}

fn run_population_control_pool_job(
    pool_job: PopulationControlPoolJob,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> PopulationControlPoolResult {
    match pool_job {
        PopulationControlPoolJob::Search(search_job) => {
            PopulationControlPoolResult::Search(Box::new(run_population_control_search_job(
                search_job,
                context,
                solver_settings,
            )))
        }
        PopulationControlPoolJob::Finalization(finalization_job) => {
            PopulationControlPoolResult::Finalization(Box::new(
                run_population_control_finalization_job(finalization_job, context, solver_settings),
            ))
        }
    }
}

fn run_population_control_search_job(
    job: PopulationControlSearchJob,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> PopulationControlSearchResult {
    let evaluation_started_at = std::time::Instant::now();

    let evaluation = evaluate_prechecked_full_parameter_vector_objective(
        job.parameter_vector.as_slice(),
        context,
        solver_settings,
    );

    PopulationControlSearchResult {
        candidate_position: job.candidate_position,
        candidate_id: job.candidate_id,
        parameter_vector: job.parameter_vector,
        candidate_evaluation_count: job.candidate_evaluation_count,
        schedule_category: job.schedule_category,
        search_direction_position: job.search_direction_position,
        search_direction_label: job.search_direction_label,
        elapsed_millis: elapsed_millis(evaluation_started_at),
        evaluation,
    }
}

fn run_population_control_finalization_job(
    job: PopulationControlFinalizationJob,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> PopulationControlFinalizationResult {
    let evaluation_started_at = std::time::Instant::now();
    let outcome = evaluate_finalized_candidate(
        job.candidate_id.as_str(),
        job.seed_vector.as_slice(),
        job.candidate_vector.as_slice(),
        context,
        solver_settings,
        job.evaluation_count,
        job.stop_reason.as_str(),
    );

    PopulationControlFinalizationResult {
        candidate_position: job.candidate_position,
        elapsed_millis: elapsed_millis(evaluation_started_at),
        outcome,
    }
}

fn record_population_control_pool_result(
    pool_state: &mut PopulationControlWorkerPoolState<'_>,
    result: PopulationControlPoolResult,
    solver_settings: &CalibrationOptimizerSettings,
    search_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    context: &JointRefinementContext,
) {
    match result {
        PopulationControlPoolResult::Search(search_result) => {
            record_population_control_search_pool_result(
                pool_state,
                *search_result,
                solver_settings,
                search_budget,
                total_progress_units,
                progress_sender,
                context,
            );
        }
        PopulationControlPoolResult::Finalization(finalization_result) => {
            let timing_diagnostic = candidate_finalization_timing_diagnostic(
                CandidateFinalizationTimingDiagnosticInput {
                    schedule_category: POPULATION_CONTROL_SCHEDULE_CATEGORY_FINALIZATION,
                    elapsed_millis: finalization_result.elapsed_millis,
                    outcome: &finalization_result.outcome,
                },
            );
            pool_state
                .evaluation_timing_diagnostics
                .push(timing_diagnostic);

            record_population_control_finalization_result(
                pool_state,
                *finalization_result,
                total_progress_units,
                progress_sender,
            );
        }
    }
}

fn record_population_control_search_pool_result(
    pool_state: &mut PopulationControlWorkerPoolState<'_>,
    result: PopulationControlSearchResult,
    solver_settings: &CalibrationOptimizerSettings,
    search_budget: usize,
    total_progress_units: usize,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    context: &JointRefinementContext,
) {
    let timing_diagnostic = population_control_evaluation_timing_diagnostic(&result);

    // Search diagnostics are stored and emitted, so the progress event receives
    // an owned copy while the retained diagnostics vector keeps its record.
    pool_state
        .evaluation_timing_diagnostics
        .push(timing_diagnostic.clone());

    record_population_control_search_result(pool_state.state, result, solver_settings);

    if pool_state.state.scheduled_search_evaluation_count() >= search_budget
        || joint_refinement_has_fatal_execution_failure(context)
    {
        pool_state
            .state
            .mark_search_closed(POPULATION_CONTROL_STOP_REASON_POOL_BUDGET);
    }

    emit_population_control_search_completed_progress(
        progress_sender,
        pool_state.state,
        search_budget,
        total_progress_units,
        Some(timing_diagnostic),
    );
}
