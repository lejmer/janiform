use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use crate::stages::adaptive_direct_search::objective_improves;
use crate::stages::joint_refinement::contracts::JointRefinementStageProgressUpdate;
use crate::stages::return_components::ContinuousLikelihoodEvaluatorCounts;

use super::super::{JointRefinementContext, elapsed_millis, ordered_parameter_bits};
use super::trust_region_policy::{
    refined_trust_region_radius, residual_squared_objective, trust_region_radius_after_prediction,
    trust_region_radius_is_at_tolerance,
};
use super::workflow::{
    JOINT_REFINEMENT_RESTART_FAILED_SEARCH_REFINEMENT_DIVISOR, JointRefinementBudgetSchedule,
    JointRefinementRestartOptimizationOutput, RestartObjectiveEvaluation, RestartObjectiveResidual,
    evaluate_prechecked_full_parameter_vector_objective,
    joint_refinement_restart_full_vector_initial_mesh_divisor,
    restart_candidate_total_evaluation_budget, restart_cycle_evaluation_budget,
};

use model_probe::trust_region_model_probe_vectors;
use progress::emit_batched_restart_pool_finished;
use racing::racing_restart_positions;
use repair::restart_repair_loss;
use scheduler::{RestartProbeWorkerPoolInput, run_restart_probe_worker_pool};
use trust_region::trust_region_candidate_vectors;

mod finalization;
mod model_probe;
mod progress;
mod racing;
mod repair;
mod scheduler;
mod trust_region;

const RESTART_PROBE_SCHEDULE_CATEGORY_INITIAL: &str = "initial";
const RESTART_PROBE_SCHEDULE_CATEGORY_MODEL_PROBE: &str = "model_probe";
const RESTART_PROBE_SCHEDULE_CATEGORY_TRUST_REGION_PROPOSAL: &str = "trust_region_proposal";
const RESTART_STOP_REASON_POOL_BUDGET: &str = "restart_pool_budget";
const RESTART_STOP_REASON_NO_SCHEDULABLE_WORK: &str = "restart_pool_no_schedulable_work";

#[derive(Debug, Clone)]
struct RestartProbeState {
    restart_id: String,
    seed_vector: Vec<f64>,
    accepted_vector: Vec<f64>,
    best_vector: Vec<f64>,
    initial_objective: Option<f64>,
    accepted_objective: Option<f64>,
    best_objective: Option<f64>,
    initial_repair_loss: Option<f64>,
    accepted_repair_loss: Option<f64>,
    best_repair_loss: Option<f64>,
    round_best_vector: Option<Vec<f64>>,
    round_best_objective: Option<f64>,
    round_best_repair_loss: Option<f64>,
    round_best_residuals: Option<Vec<RestartObjectiveResidual>>,
    round_best_predicted_residual_objective: Option<f64>,
    round_best_predicted_repair_loss: Option<f64>,
    accepted_residuals: Option<Vec<RestartObjectiveResidual>>,
    evaluated_records: Vec<RestartProbeRecord>,
    scheduled_parameter_bits: Vec<Vec<u64>>,
    model_probe_vectors: Vec<trust_region::TrustRegionCandidate>,
    next_model_probe_position: usize,
    model_probes_prepared: bool,
    trust_region_proposal_vectors: Vec<trust_region::TrustRegionCandidate>,
    next_trust_region_proposal_position: usize,
    trust_region_proposals_prepared: bool,
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
struct RestartProbeRecord {
    parameter_vector: Vec<f64>,
    residuals: Vec<RestartObjectiveResidual>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RestartProbePrediction {
    pub(super) residual_objective: Option<f64>,
    pub(super) repair_loss: Option<f64>,
}

#[derive(Debug, Clone)]
struct RestartProbeJob {
    restart_position: usize,
    parameter_vector: Vec<f64>,
    schedule_category: &'static str,
    candidate_evaluation_count: usize,
    search_direction_position: Option<usize>,
    search_direction_label: String,
    prediction: RestartProbePrediction,
}

#[derive(Debug, Clone)]
struct RestartProbeResult {
    restart_position: usize,
    parameter_vector: Vec<f64>,
    schedule_category: &'static str,
    candidate_evaluation_count: usize,
    search_direction_position: Option<usize>,
    search_direction_label: String,
    prediction: RestartProbePrediction,
    elapsed_millis: u64,
    evaluation: Option<RestartObjectiveEvaluation>,
}

pub(super) fn optimize_restart_seeds_trust_region_pool(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> JointRefinementRestartOptimizationOutput {
    let mut states =
        build_restart_probe_states(restart_seeds, context, solver_settings, budget_schedule);

    let search_pool_budget =
        restart_pool_total_evaluation_budget(context, solver_settings, budget_schedule);

    let total_progress_units = search_pool_budget.saturating_add(states.len()).max(1);

    let mut evaluation_timing_diagnostics = Vec::with_capacity(search_pool_budget);

    let restart_outcomes = run_restart_probe_worker_pool(RestartProbeWorkerPoolInput {
        states: states.as_mut_slice(),
        context,
        solver_settings,
        search_pool_budget,
        total_progress_units,
        progress_sender,
        evaluation_timing_diagnostics: &mut evaluation_timing_diagnostics,
    });

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
                initial_objective: None,
                accepted_objective: None,
                best_objective: None,
                initial_repair_loss: None,
                accepted_repair_loss: None,
                best_repair_loss: None,
                round_best_vector: None,
                round_best_objective: None,
                round_best_repair_loss: None,
                round_best_residuals: None,
                round_best_predicted_residual_objective: None,
                round_best_predicted_repair_loss: None,
                accepted_residuals: None,
                evaluated_records: Vec::new(),
                scheduled_parameter_bits: Vec::new(),
                model_probe_vectors: Vec::new(),
                next_model_probe_position: 0,
                model_probes_prepared: false,
                trust_region_proposal_vectors: Vec::new(),
                next_trust_region_proposal_position: 0,
                trust_region_proposals_prepared: false,
                trust_region_radius: initial_restart_trust_region_radius(
                    context.lower_bounds.as_slice(),
                    context.upper_bounds.as_slice(),
                ),
                completed_search_rounds: 0,
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

fn run_restart_probe_job(
    probe_job: RestartProbeJob,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> RestartProbeResult {
    let evaluation_started_at = std::time::Instant::now();

    let evaluation = evaluate_prechecked_full_parameter_vector_objective(
        probe_job.parameter_vector.as_slice(),
        context,
        solver_settings,
    );

    let evaluation_elapsed_millis = elapsed_millis(evaluation_started_at);

    RestartProbeResult {
        restart_position: probe_job.restart_position,
        parameter_vector: probe_job.parameter_vector,
        schedule_category: probe_job.schedule_category,
        candidate_evaluation_count: probe_job.candidate_evaluation_count,
        search_direction_position: probe_job.search_direction_position,
        search_direction_label: probe_job.search_direction_label,
        prediction: probe_job.prediction,
        elapsed_millis: evaluation_elapsed_millis,
        evaluation,
    }
}

fn next_restart_probe_job_racing(
    states: &mut [RestartProbeState],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    next_restart_position: &mut usize,
    max_in_flight_per_restart: usize,
) -> Option<RestartProbeJob> {
    if states.is_empty() {
        return None;
    }

    let restart_positions =
        racing_restart_positions(states, *next_restart_position, max_in_flight_per_restart);

    for restart_position in restart_positions {
        *next_restart_position = restart_position.saturating_add(1) % states.len();

        loop {
            refresh_restart_probe_state(
                &mut states[restart_position],
                context.lower_bounds.as_slice(),
                context.upper_bounds.as_slice(),
                solver_settings,
            );

            if let Some(probe_job) = next_restart_probe_job(states, restart_position, context) {
                return Some(probe_job);
            }

            let state_completed_search_rounds = states[restart_position].completed_search_rounds;

            let state_next_model_probe_position =
                states[restart_position].next_model_probe_position;

            let state_next_trust_region_proposal_position =
                states[restart_position].next_trust_region_proposal_position;

            refresh_restart_probe_state(
                &mut states[restart_position],
                context.lower_bounds.as_slice(),
                context.upper_bounds.as_slice(),
                solver_settings,
            );

            let state = &states[restart_position];

            if state.completed
                || state.in_flight_count > 0
                || (state.completed_search_rounds == state_completed_search_rounds
                    && state.next_model_probe_position == state_next_model_probe_position
                    && state.next_trust_region_proposal_position
                        == state_next_trust_region_proposal_position)
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
        // The worker job owns a candidate snapshot so Rayon never borrows mutable restart state.
        return schedule_restart_probe_job(
            state,
            restart_position,
            state.accepted_vector.clone(),
            RESTART_PROBE_SCHEDULE_CATEGORY_INITIAL,
            None,
            "initial".to_string(),
            RestartProbePrediction::default(),
        );
    }

    if !restart_probe_model_probe_phase_completed(state) {
        return schedule_next_model_probe_job(state, restart_position, context);
    }

    if !restart_probe_can_prepare_trust_region_proposals(state) {
        return None;
    }

    schedule_next_trust_region_proposal_job(state, restart_position, context)
}

fn schedule_next_trust_region_proposal_job(
    state: &mut RestartProbeState,
    restart_position: usize,
    context: &JointRefinementContext,
) -> Option<RestartProbeJob> {
    if !state.trust_region_proposals_prepared {
        state.trust_region_proposal_vectors = trust_region_candidate_vectors(
            state,
            state.evaluated_records.as_slice(),
            context.lower_bounds.as_slice(),
            context.upper_bounds.as_slice(),
        );

        state.trust_region_proposals_prepared = true;
        state.next_trust_region_proposal_position = 0;
    }

    while let Some(candidate) = state
        .trust_region_proposal_vectors
        .get(state.next_trust_region_proposal_position)
        .cloned()
    {
        let proposal_position = state.next_trust_region_proposal_position;

        state.next_trust_region_proposal_position =
            state.next_trust_region_proposal_position.saturating_add(1);

        let Some(probe_job) = schedule_restart_probe_job(
            state,
            restart_position,
            candidate.parameter_vector,
            RESTART_PROBE_SCHEDULE_CATEGORY_TRUST_REGION_PROPOSAL,
            Some(proposal_position),
            format!("trust_region.{}", candidate.label),
            candidate.prediction,
        ) else {
            continue;
        };

        return Some(probe_job);
    }

    None
}

fn schedule_next_model_probe_job(
    state: &mut RestartProbeState,
    restart_position: usize,
    context: &JointRefinementContext,
) -> Option<RestartProbeJob> {
    if !state.model_probes_prepared {
        state.model_probe_vectors = trust_region_model_probe_vectors(
            state,
            context.lower_bounds.as_slice(),
            context.upper_bounds.as_slice(),
            &context.active_parameter_layout,
        );

        state.model_probes_prepared = true;
        state.next_model_probe_position = 0;
    }

    while let Some(candidate) = state
        .model_probe_vectors
        .get(state.next_model_probe_position)
        .cloned()
    {
        let model_probe_position = state.next_model_probe_position;

        state.next_model_probe_position = state.next_model_probe_position.saturating_add(1);

        let Some(probe_job) = schedule_restart_probe_job(
            state,
            restart_position,
            candidate.parameter_vector,
            RESTART_PROBE_SCHEDULE_CATEGORY_MODEL_PROBE,
            Some(model_probe_position),
            candidate.label,
            candidate.prediction,
        ) else {
            continue;
        };

        return Some(probe_job);
    }

    None
}

fn schedule_restart_probe_job(
    state: &mut RestartProbeState,
    restart_position: usize,
    parameter_vector: Vec<f64>,
    schedule_category: &'static str,
    search_direction_position: Option<usize>,
    search_direction_label: String,
    prediction: RestartProbePrediction,
) -> Option<RestartProbeJob> {
    if restart_parameter_vector_was_scheduled(state, parameter_vector.as_slice()) {
        return None;
    }

    let candidate_evaluation_count = state
        .evaluation_count
        .saturating_add(state.in_flight_count)
        .saturating_add(1);

    state
        .scheduled_parameter_bits
        .push(ordered_parameter_bits(parameter_vector.as_slice()));

    state.in_flight_count = state.in_flight_count.saturating_add(1);

    Some(RestartProbeJob {
        restart_position,
        parameter_vector,
        schedule_category,
        candidate_evaluation_count,
        search_direction_position,
        search_direction_label,
        prediction,
    })
}

fn restart_parameter_vector_was_scheduled(
    state: &RestartProbeState,
    parameter_vector: &[f64],
) -> bool {
    let parameter_bits = ordered_parameter_bits(parameter_vector);

    state
        .scheduled_parameter_bits
        .iter()
        .any(|scheduled_bits| scheduled_bits == &parameter_bits)
}

fn restart_probe_state_has_remaining_budget(state: &RestartProbeState) -> bool {
    state.evaluation_count.saturating_add(state.in_flight_count) < state.max_evaluation_count
}

fn restart_probe_model_probe_phase_completed(state: &RestartProbeState) -> bool {
    state.model_probes_prepared
        && state.next_model_probe_position >= state.model_probe_vectors.len()
}

fn restart_probe_can_prepare_trust_region_proposals(state: &RestartProbeState) -> bool {
    state.trust_region_proposals_prepared || state.in_flight_count == 0
}

fn refresh_restart_probe_state(
    state: &mut RestartProbeState,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    solver_settings: &CalibrationOptimizerSettings,
) {
    if state.completed || state.evaluation_count == 0 || state.in_flight_count > 0 {
        return;
    }

    if trust_region_radius_is_at_tolerance(
        state.trust_region_radius,
        state.accepted_vector.as_slice(),
        lower_bounds,
        upper_bounds,
        solver_settings.absolute_tolerance,
        solver_settings.relative_tolerance,
    ) {
        state.completed = true;
        state.stop_reason = "tolerance".to_string();

        return;
    }

    if restart_probe_round_has_pending_candidates(state) {
        return;
    }

    let round_improved = restart_probe_round_improved(state, solver_settings);

    if round_improved {
        update_trust_region_radius_after_round_improvement(state, lower_bounds, upper_bounds);

        if let Some(round_best_vector) = state.round_best_vector.take() {
            state.accepted_vector = round_best_vector;
        }

        state.accepted_objective = state.round_best_objective;
        state.accepted_repair_loss = state.round_best_repair_loss;

        if let Some(round_best_residuals) = state.round_best_residuals.take() {
            state.accepted_residuals = Some(round_best_residuals);
        }

        state.stop_reason = "trust_region_improved".to_string();
    } else {
        let Some(refined_radius) = refined_trust_region_radius(
            state.trust_region_radius,
            JOINT_REFINEMENT_RESTART_FAILED_SEARCH_REFINEMENT_DIVISOR,
        ) else {
            state.completed = true;
            state.stop_reason = "invalid_step".to_string();

            return;
        };

        state.trust_region_radius = refined_radius;
        state.stop_reason = "trust_region_refined".to_string();
    }

    state.round_best_objective = None;
    state.round_best_repair_loss = None;
    state.round_best_residuals = None;
    state.round_best_predicted_residual_objective = None;
    state.round_best_predicted_repair_loss = None;
    state.model_probe_vectors.clear();
    state.next_model_probe_position = 0;
    state.model_probes_prepared = false;
    state.trust_region_proposal_vectors.clear();
    state.next_trust_region_proposal_position = 0;
    state.trust_region_proposals_prepared = false;
    state.completed_search_rounds = state.completed_search_rounds.saturating_add(1);

    if trust_region_radius_is_at_tolerance(
        state.trust_region_radius,
        state.accepted_vector.as_slice(),
        lower_bounds,
        upper_bounds,
        solver_settings.absolute_tolerance,
        solver_settings.relative_tolerance,
    ) {
        state.completed = true;
        state.stop_reason = "tolerance".to_string();
    }
}

fn restart_probe_round_has_pending_candidates(state: &RestartProbeState) -> bool {
    !state.model_probes_prepared
        || state.next_model_probe_position < state.model_probe_vectors.len()
        || !state.trust_region_proposals_prepared
        || state.next_trust_region_proposal_position < state.trust_region_proposal_vectors.len()
}

fn restart_probe_round_improved(
    state: &RestartProbeState,
    solver_settings: &CalibrationOptimizerSettings,
) -> bool {
    if let (Some(round_best_repair), Some(accepted_repair)) =
        (state.round_best_repair_loss, state.accepted_repair_loss)
        && objective_improves(
            round_best_repair,
            accepted_repair,
            solver_settings.absolute_tolerance,
            solver_settings.relative_tolerance,
        )
    {
        return true;
    }

    match (state.round_best_objective, state.accepted_objective) {
        (Some(round_best), Some(accepted)) => objective_improves(
            round_best,
            accepted,
            solver_settings.absolute_tolerance,
            solver_settings.relative_tolerance,
        ),
        (Some(_), None) => true,
        _ => false,
    }
}

fn update_trust_region_radius_after_round_improvement(
    state: &mut RestartProbeState,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) {
    let Some(accepted_residuals) = state.accepted_residuals.as_ref() else {
        return;
    };

    let Some(round_best_residuals) = state.round_best_residuals.as_ref() else {
        return;
    };

    if let (Some(accepted_repair_loss), Some(round_best_repair_loss), Some(predicted_repair_loss)) = (
        state.accepted_repair_loss,
        state.round_best_repair_loss,
        state.round_best_predicted_repair_loss,
    ) {
        if let Some(updated_radius) = trust_region_radius_after_prediction(
            state.trust_region_radius,
            accepted_repair_loss,
            predicted_repair_loss,
            round_best_repair_loss,
            lower_bounds,
            upper_bounds,
        ) {
            state.trust_region_radius = updated_radius;
        }

        return;
    }

    let Some(predicted_residual_objective) = state.round_best_predicted_residual_objective else {
        return;
    };

    let accepted_residual_objective = residual_squared_objective(accepted_residuals.as_slice());
    let round_best_residual_objective = residual_squared_objective(round_best_residuals.as_slice());

    if let Some(updated_radius) = trust_region_radius_after_prediction(
        state.trust_region_radius,
        accepted_residual_objective,
        predicted_residual_objective,
        round_best_residual_objective,
        lower_bounds,
        upper_bounds,
    ) {
        state.trust_region_radius = updated_radius;
    }
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

    let RestartObjectiveEvaluation {
        objective,
        exact_likelihood_evaluator_counts,
        residuals,
        ..
    } = evaluation;

    let repair_loss = restart_repair_loss(residuals.as_slice());

    state.exact_likelihood_evaluator_counts = exact_likelihood_evaluator_counts;

    // Probe history, round winner, and all-time best state keep independent
    // snapshots because each role can survive a different search-round boundary.
    state.evaluated_records.push(RestartProbeRecord {
        parameter_vector: result.parameter_vector.clone(),
        residuals: residuals.clone(),
    });

    if result.schedule_category == RESTART_PROBE_SCHEDULE_CATEGORY_INITIAL {
        state.initial_objective = Some(objective);
        state.accepted_objective = Some(objective);
        state.best_objective = Some(objective);
        state.initial_repair_loss = repair_loss;
        state.accepted_repair_loss = repair_loss;
        state.best_repair_loss = repair_loss;
        state.accepted_residuals = Some(residuals);
    } else {
        if result.schedule_category == RESTART_PROBE_SCHEDULE_CATEGORY_MODEL_PROBE {
            state.trust_region_proposals_prepared = false;
        }

        // Round winner storage is independent from all-time best storage because this
        // same evaluated candidate may need to update both roles below.
        let round_improved =
            candidate_improves_round_search(objective, repair_loss, state, solver_settings)
                && objective.is_finite();

        if round_improved {
            state.round_best_objective = Some(objective);
            state.round_best_repair_loss = repair_loss;
            state.round_best_vector = Some(result.parameter_vector.clone());
            state.round_best_residuals = Some(residuals);
            state.round_best_predicted_residual_objective = result.prediction.residual_objective;
            state.round_best_predicted_repair_loss = result.prediction.repair_loss;
        }
    }

    if repair_loss.is_some_and(|candidate_repair_loss| {
        candidate_improves_loss(
            candidate_repair_loss,
            state.best_repair_loss,
            solver_settings,
        )
    }) {
        state.best_repair_loss = repair_loss;
    }

    if objective.is_finite()
        && state
            .best_objective
            .map(|best_objective| objective < best_objective)
            .unwrap_or(true)
    {
        state.best_objective = Some(objective);
        state.best_vector = result.parameter_vector;
    }

    if state.evaluation_count >= state.max_evaluation_count {
        state.completed = true;
        state.stop_reason = RESTART_STOP_REASON_POOL_BUDGET.to_string();
    }
}

fn candidate_improves_round_search(
    candidate_objective: f64,
    candidate_repair_loss: Option<f64>,
    state: &RestartProbeState,
    solver_settings: &CalibrationOptimizerSettings,
) -> bool {
    if let Some(candidate_repair_loss) = candidate_repair_loss {
        if state.round_best_repair_loss.is_some() {
            return candidate_improves_loss(
                candidate_repair_loss,
                state.round_best_repair_loss,
                solver_settings,
            );
        }

        if candidate_improves_loss(
            candidate_repair_loss,
            state.accepted_repair_loss,
            solver_settings,
        ) {
            return true;
        }
    }

    if state.round_best_repair_loss.is_some() {
        return false;
    }

    objective_improves_incumbent(
        candidate_objective,
        state.round_best_objective.or(state.accepted_objective),
        solver_settings,
    )
}

fn candidate_improves_loss(
    candidate_loss: f64,
    incumbent_loss: Option<f64>,
    solver_settings: &CalibrationOptimizerSettings,
) -> bool {
    match incumbent_loss {
        Some(incumbent) => objective_improves(
            candidate_loss,
            incumbent,
            solver_settings.absolute_tolerance,
            solver_settings.relative_tolerance,
        ),
        None => candidate_loss.is_finite(),
    }
}

fn initial_restart_trust_region_radius(lower_bounds: &[f64], upper_bounds: &[f64]) -> f64 {
    let active_parameter_count =
        super::parameter_space::movable_parameter_count(lower_bounds, upper_bounds).max(1);

    (joint_refinement_restart_full_vector_initial_mesh_divisor(active_parameter_count) as f64)
        .recip()
}

fn objective_improves_incumbent(
    candidate_objective: f64,
    incumbent_objective: Option<f64>,
    solver_settings: &CalibrationOptimizerSettings,
) -> bool {
    match incumbent_objective {
        Some(incumbent) => objective_improves(
            candidate_objective,
            incumbent,
            solver_settings.absolute_tolerance,
            solver_settings.relative_tolerance,
        ),
        None => candidate_objective.is_finite(),
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
fn restart_probe_state_for_test(
    accepted_vector: Vec<f64>,
    trust_region_radius: f64,
) -> RestartProbeState {
    RestartProbeState {
        restart_id: "restart_0".to_string(),
        seed_vector: accepted_vector.clone(),
        accepted_vector: accepted_vector.clone(),
        best_vector: accepted_vector,
        initial_objective: Some(8.0),
        accepted_objective: Some(8.0),
        best_objective: Some(8.0),
        initial_repair_loss: Some(8.0),
        accepted_repair_loss: Some(8.0),
        best_repair_loss: Some(8.0),
        round_best_vector: None,
        round_best_objective: None,
        round_best_repair_loss: None,
        round_best_residuals: None,
        round_best_predicted_residual_objective: None,
        round_best_predicted_repair_loss: None,
        accepted_residuals: None,
        evaluated_records: Vec::new(),
        scheduled_parameter_bits: Vec::new(),
        model_probe_vectors: Vec::new(),
        next_model_probe_position: 0,
        model_probes_prepared: false,
        trust_region_proposal_vectors: Vec::new(),
        next_trust_region_proposal_position: 0,
        trust_region_proposals_prepared: false,
        trust_region_radius,
        completed_search_rounds: 1,
        evaluation_count: 3,
        max_evaluation_count: 16,
        in_flight_count: 0,
        stop_reason: "trust_region_refined".to_string(),
        completed: false,
        exact_likelihood_evaluator_counts: None,
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;

    use super::{
        RESTART_PROBE_SCHEDULE_CATEGORY_INITIAL, RESTART_PROBE_SCHEDULE_CATEGORY_MODEL_PROBE,
        RESTART_PROBE_SCHEDULE_CATEGORY_TRUST_REGION_PROPOSAL, RestartObjectiveEvaluation,
        RestartObjectiveResidual, RestartProbePrediction, RestartProbeResult, RestartProbeState,
        record_restart_probe_result, refresh_restart_probe_state,
        restart_probe_can_prepare_trust_region_proposals, schedule_restart_probe_job,
    };

    fn test_solver_settings() -> CalibrationOptimizerSettings {
        CalibrationOptimizerSettings::try_new("restart-probe-test", 16, 2, 0.1, 0.01, 10.0)
            .expect("test optimizer settings should be valid")
    }

    fn running_search_state() -> RestartProbeState {
        RestartProbeState {
            restart_id: "restart_0".to_string(),
            seed_vector: vec![0.0],
            accepted_vector: vec![0.0],
            best_vector: vec![0.0],
            initial_objective: Some(10.0),
            accepted_objective: Some(10.0),
            best_objective: Some(10.0),
            initial_repair_loss: Some(10.0),
            accepted_repair_loss: Some(10.0),
            best_repair_loss: Some(10.0),
            round_best_vector: None,
            round_best_objective: None,
            round_best_repair_loss: None,
            round_best_residuals: None,
            round_best_predicted_residual_objective: None,
            round_best_predicted_repair_loss: None,
            accepted_residuals: None,
            evaluated_records: Vec::new(),
            scheduled_parameter_bits: Vec::new(),
            model_probe_vectors: Vec::new(),
            next_model_probe_position: 1,
            model_probes_prepared: true,
            trust_region_proposal_vectors: Vec::new(),
            next_trust_region_proposal_position: 0,
            trust_region_proposals_prepared: true,
            trust_region_radius: 1.0,
            completed_search_rounds: 0,
            evaluation_count: 1,
            max_evaluation_count: 16,
            in_flight_count: 1,
            stop_reason: "running".to_string(),
            completed: false,
            exact_likelihood_evaluator_counts: None,
        }
    }

    fn model_probe_result(objective: f64) -> RestartProbeResult {
        model_probe_result_with_repair_loss(objective, objective)
    }

    fn model_probe_result_with_repair_loss(objective: f64, repair_loss: f64) -> RestartProbeResult {
        RestartProbeResult {
            restart_position: 0,
            parameter_vector: vec![1.0],
            schedule_category: RESTART_PROBE_SCHEDULE_CATEGORY_MODEL_PROBE,
            candidate_evaluation_count: 2,
            search_direction_position: Some(0),
            search_direction_label: "model_probe.all_movable.positive".to_string(),
            prediction: RestartProbePrediction::default(),
            elapsed_millis: 1,
            evaluation: Some(RestartObjectiveEvaluation {
                objective,
                admissibility_warning_count: 0,
                max_standardized_residual: Some(0.0),
                exact_likelihood_evaluator_counts: None,
                residuals: vec![RestartObjectiveResidual {
                    family_id: "test".to_string(),
                    residual_id: "test.residual".to_string(),
                    objective_residual: repair_loss.sqrt(),
                }],
            }),
        }
    }

    fn trust_region_proposal_result(objective: f64) -> RestartProbeResult {
        RestartProbeResult {
            restart_position: 0,
            parameter_vector: vec![1.0],
            schedule_category: RESTART_PROBE_SCHEDULE_CATEGORY_TRUST_REGION_PROPOSAL,
            candidate_evaluation_count: 2,
            search_direction_position: Some(1),
            search_direction_label: "trust_region.gauss_newton".to_string(),
            prediction: RestartProbePrediction {
                residual_objective: Some(8.0),
                repair_loss: Some(8.0),
            },
            elapsed_millis: 1,
            evaluation: Some(RestartObjectiveEvaluation {
                objective,
                admissibility_warning_count: 0,
                max_standardized_residual: Some(0.0),
                exact_likelihood_evaluator_counts: None,
                residuals: vec![RestartObjectiveResidual {
                    family_id: "test".to_string(),
                    residual_id: "test.residual".to_string(),
                    objective_residual: objective.sqrt(),
                }],
            }),
        }
    }

    #[test]
    fn model_probe_candidate_requires_tolerance_improvement_for_acceptance() {
        let solver_settings = test_solver_settings();

        let mut state = running_search_state();

        record_restart_probe_result(&mut state, model_probe_result(9.95), &solver_settings);

        state.trust_region_proposals_prepared = true;

        refresh_restart_probe_state(&mut state, &[0.0], &[1.0], &solver_settings);

        assert_eq!(state.accepted_vector, vec![0.0]);
        assert_eq!(state.accepted_objective, Some(10.0));
        assert_eq!(state.best_vector, vec![1.0]);
        assert_eq!(state.best_objective, Some(9.95));
        assert_eq!(state.round_best_objective, None);
        assert_eq!(state.stop_reason, "trust_region_refined");
    }

    #[test]
    fn model_probe_candidate_with_tolerance_improvement_is_accepted() {
        let solver_settings = test_solver_settings();

        let mut state = running_search_state();

        record_restart_probe_result(&mut state, model_probe_result(9.8), &solver_settings);

        state.trust_region_proposals_prepared = true;

        refresh_restart_probe_state(&mut state, &[0.0], &[1.0], &solver_settings);

        assert_eq!(state.accepted_vector, vec![1.0]);
        assert_eq!(state.accepted_objective, Some(9.8));
        assert_eq!(state.best_vector, vec![1.0]);
        assert_eq!(state.best_objective, Some(9.8));
        assert_eq!(state.round_best_objective, None);
        assert_eq!(state.stop_reason, "trust_region_improved");
    }

    #[test]
    fn repair_improving_candidate_can_become_search_incumbent_without_global_best() {
        let solver_settings = test_solver_settings();

        let mut state = running_search_state();

        record_restart_probe_result(
            &mut state,
            model_probe_result_with_repair_loss(10.05, 9.0),
            &solver_settings,
        );

        state.trust_region_proposals_prepared = true;

        refresh_restart_probe_state(&mut state, &[0.0], &[1.0], &solver_settings);

        assert_eq!(state.accepted_vector, vec![1.0]);
        assert_eq!(state.accepted_objective, Some(10.05));
        assert_eq!(state.accepted_repair_loss, Some(9.0));
        assert_eq!(state.best_vector, vec![0.0]);
        assert_eq!(state.best_objective, Some(10.0));
        assert_eq!(state.best_repair_loss, Some(9.0));
    }

    #[test]
    fn trust_region_proposal_with_tolerance_improvement_is_accepted_after_round_refresh() {
        let solver_settings = test_solver_settings();

        let mut state = running_search_state();

        record_restart_probe_result(
            &mut state,
            trust_region_proposal_result(9.8),
            &solver_settings,
        );

        assert_eq!(state.accepted_vector, vec![0.0]);
        assert_eq!(state.round_best_vector, Some(vec![1.0]));

        refresh_restart_probe_state(&mut state, &[0.0], &[1.0], &solver_settings);

        assert_eq!(state.accepted_vector, vec![1.0]);
        assert_eq!(state.accepted_objective, Some(9.8));
        assert_eq!(state.best_vector, vec![1.0]);
        assert_eq!(state.best_objective, Some(9.8));
        assert_eq!(state.next_model_probe_position, 0);
        assert_eq!(state.stop_reason, "trust_region_improved");
    }

    #[test]
    fn duplicate_probe_vectors_are_not_scheduled_again() {
        let mut state = running_search_state();

        state.in_flight_count = 0;
        state.evaluation_count = 0;
        state.scheduled_parameter_bits = vec![vec![0.0f64.to_bits()]];

        let probe_job = schedule_restart_probe_job(
            &mut state,
            0,
            vec![0.0],
            RESTART_PROBE_SCHEDULE_CATEGORY_INITIAL,
            None,
            "initial".to_string(),
            RestartProbePrediction::default(),
        );

        assert!(probe_job.is_none());
        assert_eq!(state.in_flight_count, 0);
        assert_eq!(state.scheduled_parameter_bits, vec![vec![0.0f64.to_bits()]]);
    }

    #[test]
    fn trust_region_preparation_waits_for_model_probe_results() {
        let mut state = running_search_state();

        state.trust_region_proposals_prepared = false;
        state.in_flight_count = 1;

        assert!(!restart_probe_can_prepare_trust_region_proposals(&state));

        state.in_flight_count = 0;

        assert!(restart_probe_can_prepare_trust_region_proposals(&state));
    }

    #[test]
    fn first_probe_schedules_initial_vector_once() {
        let mut state = running_search_state();

        state.in_flight_count = 0;
        state.evaluation_count = 0;

        let probe_job = schedule_restart_probe_job(
            &mut state,
            0,
            vec![0.0],
            RESTART_PROBE_SCHEDULE_CATEGORY_INITIAL,
            None,
            "initial".to_string(),
            RestartProbePrediction::default(),
        )
        .expect("first probe should schedule the initial vector");

        assert_eq!(
            probe_job.schedule_category,
            RESTART_PROBE_SCHEDULE_CATEGORY_INITIAL
        );
        assert_eq!(probe_job.parameter_vector, vec![0.0]);
        assert_eq!(state.in_flight_count, 1);
        assert_eq!(state.scheduled_parameter_bits, vec![vec![0.0f64.to_bits()]]);
    }
}
