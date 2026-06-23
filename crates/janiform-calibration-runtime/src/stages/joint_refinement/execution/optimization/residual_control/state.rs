use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use crate::stages::adaptive_direct_search::objective_improves;

use super::super::super::{JointRefinementContext, ordered_parameter_bits};
use super::super::residual_profile::ResidualTailProfile;
use super::super::trust_region_policy::{
    refined_trust_region_radius, trust_region_radius_after_prediction,
    trust_region_radius_is_at_tolerance,
};
use super::super::workflow::JOINT_REFINEMENT_RESTART_FAILED_SEARCH_REFINEMENT_DIVISOR;
use super::model::{control_probe_candidate_vectors, residual_repair_candidate_vectors};
use super::{
    RESIDUAL_CONTROL_SCHEDULE_CATEGORY_CONTROL_PROBE, RESIDUAL_CONTROL_SCHEDULE_CATEGORY_INITIAL,
    RESIDUAL_CONTROL_SCHEDULE_CATEGORY_REPAIR, RESIDUAL_CONTROL_STOP_REASON_NO_SCHEDULABLE_WORK,
    RESIDUAL_CONTROL_STOP_REASON_POOL_BUDGET, ResidualControlJob, ResidualControlPrediction,
    ResidualControlRecord, ResidualControlResult, ResidualControlState, RestartObjectiveEvaluation,
};

pub(super) fn next_residual_control_search_job(
    states: &mut [ResidualControlState],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    next_restart_position: &mut usize,
    max_in_flight_per_restart: usize,
) -> Option<ResidualControlJob> {
    if states.is_empty() {
        return None;
    }

    for _ in 0..states.len() {
        let restart_position = *next_restart_position % states.len();
        *next_restart_position = restart_position.saturating_add(1) % states.len();

        if states[restart_position].in_flight_count >= max_in_flight_per_restart {
            continue;
        }

        loop {
            refresh_residual_control_state(
                &mut states[restart_position],
                context.lower_bounds.as_slice(),
                context.upper_bounds.as_slice(),
                solver_settings,
            );

            if let Some(job) = next_residual_control_job(states, restart_position, context) {
                return Some(job);
            }

            let search_round_count = states[restart_position].completed_search_rounds;
            let next_control_probe_position = states[restart_position].next_control_probe_position;
            let next_repair_position = states[restart_position].next_repair_position;

            refresh_residual_control_state(
                &mut states[restart_position],
                context.lower_bounds.as_slice(),
                context.upper_bounds.as_slice(),
                solver_settings,
            );

            let state = &states[restart_position];
            if state.completed
                || state.in_flight_count > 0
                || (state.completed_search_rounds == search_round_count
                    && state.next_control_probe_position == next_control_probe_position
                    && state.next_repair_position == next_repair_position)
            {
                break;
            }
        }
    }

    None
}

fn next_residual_control_job(
    states: &mut [ResidualControlState],
    restart_position: usize,
    context: &JointRefinementContext,
) -> Option<ResidualControlJob> {
    let state = states.get_mut(restart_position)?;

    if state.completed {
        return None;
    }

    if state.evaluation_count == 0 && state.in_flight_count > 0 {
        return None;
    }

    if !residual_control_state_has_remaining_budget(state) {
        state.completed = true;
        state.stop_reason = RESIDUAL_CONTROL_STOP_REASON_POOL_BUDGET.to_string();
        return None;
    }

    if state.evaluation_count == 0 {
        // The worker job owns a candidate snapshot so Rayon never borrows mutable restart state.
        return schedule_residual_control_job(
            state,
            restart_position,
            state.accepted_vector.clone(),
            RESIDUAL_CONTROL_SCHEDULE_CATEGORY_INITIAL,
            None,
            "initial".to_string(),
            ResidualControlPrediction::default(),
        );
    }

    if !residual_control_probe_phase_completed(state) {
        return schedule_next_control_probe_job(state, restart_position, context);
    }

    if !residual_control_can_prepare_repair_candidates(state) {
        return None;
    }

    schedule_next_residual_repair_job(state, restart_position, context)
}

fn schedule_next_control_probe_job(
    state: &mut ResidualControlState,
    restart_position: usize,
    context: &JointRefinementContext,
) -> Option<ResidualControlJob> {
    if !state.control_probes_prepared {
        state.control_probe_vectors = control_probe_candidate_vectors(
            state,
            context.lower_bounds.as_slice(),
            context.upper_bounds.as_slice(),
            context,
        );

        state.control_probes_prepared = true;
        state.next_control_probe_position = 0;
    }

    while let Some(candidate) = state
        .control_probe_vectors
        .get(state.next_control_probe_position)
        .cloned()
    {
        let probe_position = state.next_control_probe_position;

        state.next_control_probe_position = state.next_control_probe_position.saturating_add(1);

        let Some(job) = schedule_residual_control_job(
            state,
            restart_position,
            candidate.parameter_vector,
            RESIDUAL_CONTROL_SCHEDULE_CATEGORY_CONTROL_PROBE,
            Some(probe_position),
            candidate.label,
            candidate.prediction,
        ) else {
            continue;
        };

        return Some(job);
    }

    None
}

fn schedule_next_residual_repair_job(
    state: &mut ResidualControlState,
    restart_position: usize,
    context: &JointRefinementContext,
) -> Option<ResidualControlJob> {
    if !state.repairs_prepared {
        state.repair_vectors = residual_repair_candidate_vectors(
            state,
            context.lower_bounds.as_slice(),
            context.upper_bounds.as_slice(),
        );

        state.repairs_prepared = true;
        state.next_repair_position = 0;
    }

    while let Some(candidate) = state
        .repair_vectors
        .get(state.next_repair_position)
        .cloned()
    {
        let repair_position = state.next_repair_position;

        state.next_repair_position = state.next_repair_position.saturating_add(1);

        let Some(job) = schedule_residual_control_job(
            state,
            restart_position,
            candidate.parameter_vector,
            RESIDUAL_CONTROL_SCHEDULE_CATEGORY_REPAIR,
            Some(repair_position),
            candidate.label,
            candidate.prediction,
        ) else {
            continue;
        };

        return Some(job);
    }

    None
}

fn schedule_residual_control_job(
    state: &mut ResidualControlState,
    restart_position: usize,
    parameter_vector: Vec<f64>,
    schedule_category: &'static str,
    search_direction_position: Option<usize>,
    search_direction_label: String,
    prediction: ResidualControlPrediction,
) -> Option<ResidualControlJob> {
    if residual_control_parameter_vector_was_scheduled(state, parameter_vector.as_slice()) {
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

    Some(ResidualControlJob {
        restart_position,
        parameter_vector,
        schedule_category,
        candidate_evaluation_count,
        search_direction_position,
        search_direction_label,
        prediction,
    })
}

fn residual_control_parameter_vector_was_scheduled(
    state: &ResidualControlState,
    parameter_vector: &[f64],
) -> bool {
    let parameter_bits = ordered_parameter_bits(parameter_vector);

    state
        .scheduled_parameter_bits
        .iter()
        .any(|scheduled_bits| scheduled_bits == &parameter_bits)
}

pub(super) fn residual_control_state_has_remaining_budget(state: &ResidualControlState) -> bool {
    state.evaluation_count.saturating_add(state.in_flight_count) < state.max_evaluation_count
}

fn residual_control_probe_phase_completed(state: &ResidualControlState) -> bool {
    state.control_probes_prepared
        && state.next_control_probe_position >= state.control_probe_vectors.len()
}

fn residual_control_can_prepare_repair_candidates(state: &ResidualControlState) -> bool {
    state.repairs_prepared || state.in_flight_count == 0
}

fn refresh_residual_control_state(
    state: &mut ResidualControlState,
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

    if residual_control_round_has_pending_candidates(state) {
        return;
    }

    if residual_control_round_improved(state, solver_settings) {
        update_residual_control_radius_after_round_improvement(state, lower_bounds, upper_bounds);

        if let Some(round_best_vector) = state.round_best_vector.take() {
            state.accepted_vector = round_best_vector;
        }

        state.accepted_objective = state.round_best_objective;
        state.accepted_profile = state.round_best_profile.take();

        if let Some(round_best_residuals) = state.round_best_residuals.take() {
            state.accepted_residuals = Some(round_best_residuals);
        }

        state.stop_reason = "residual_control_improved".to_string();
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
        state.stop_reason = "residual_control_refined".to_string();
    }

    state.round_best_vector = None;
    state.round_best_objective = None;
    state.round_best_profile = None;
    state.round_best_residuals = None;
    state.round_best_predicted_profile = None;
    state.control_probe_vectors.clear();
    state.next_control_probe_position = 0;
    state.control_probes_prepared = false;
    state.repair_vectors.clear();
    state.next_repair_position = 0;
    state.repairs_prepared = false;
    state.completed_search_rounds = state.completed_search_rounds.saturating_add(1);
}

fn residual_control_round_has_pending_candidates(state: &ResidualControlState) -> bool {
    !state.control_probes_prepared
        || state.next_control_probe_position < state.control_probe_vectors.len()
        || !state.repairs_prepared
        || state.next_repair_position < state.repair_vectors.len()
}

fn residual_control_round_improved(
    state: &ResidualControlState,
    solver_settings: &CalibrationOptimizerSettings,
) -> bool {
    if let (Some(round_best_profile), Some(accepted_profile)) = (
        state.round_best_profile.as_ref(),
        state.accepted_profile.as_ref(),
    ) {
        return round_best_profile.dominates(accepted_profile, solver_settings);
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

fn update_residual_control_radius_after_round_improvement(
    state: &mut ResidualControlState,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) {
    let Some(accepted_profile) = state.accepted_profile.as_ref() else {
        return;
    };

    let Some(round_best_profile) = state.round_best_profile.as_ref() else {
        return;
    };

    let Some(predicted_profile) = state.round_best_predicted_profile.as_ref() else {
        return;
    };

    let Some(updated_radius) = trust_region_radius_after_prediction(
        state.trust_region_radius,
        accepted_profile.total_loss(),
        predicted_profile.total_loss(),
        round_best_profile.total_loss(),
        lower_bounds,
        upper_bounds,
    ) else {
        return;
    };

    state.trust_region_radius = updated_radius;
}

pub(super) fn record_residual_control_result(
    state: &mut ResidualControlState,
    result: ResidualControlResult,
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

    let profile = ResidualTailProfile::from_residuals(residuals.as_slice());

    state.exact_likelihood_evaluator_counts = exact_likelihood_evaluator_counts;

    // Probe history, accepted state, and best-candidate roles keep independent
    // snapshots because each role can survive a different search-round boundary.
    state.records.push(ResidualControlRecord {
        parameter_vector: result.parameter_vector.clone(),
        residuals: residuals.clone(),
    });

    if result.schedule_category == RESIDUAL_CONTROL_SCHEDULE_CATEGORY_INITIAL {
        state.accepted_objective = Some(objective);
        state.accepted_profile = profile.clone();
        state.accepted_residuals = Some(residuals.clone());
        state.best_global_objective = Some(objective);
        state.best_profile_objective = Some(objective);
        state.best_profile = profile.clone();
    } else if candidate_improves_residual_control_round(
        profile.as_ref(),
        objective,
        state,
        solver_settings,
    ) && objective.is_finite()
    {
        state.round_best_vector = Some(result.parameter_vector.clone());
        state.round_best_objective = Some(objective);
        state.round_best_profile = profile.clone();
        state.round_best_residuals = Some(residuals.clone());
        state.round_best_predicted_profile = result.prediction.profile;
    }

    if objective.is_finite()
        && state
            .best_global_objective
            .map(|best_objective| objective < best_objective)
            .unwrap_or(true)
    {
        state.best_global_objective = Some(objective);
        state.best_global_vector = result.parameter_vector.clone();
    }

    if let Some(profile) = profile
        && candidate_profile_improves_incumbent(
            &profile,
            state.best_profile.as_ref(),
            solver_settings,
        )
    {
        state.best_profile = Some(profile);
        state.best_profile_objective = Some(objective);
        state.best_profile_vector = result.parameter_vector;
    }

    if state.evaluation_count >= state.max_evaluation_count {
        state.completed = true;
        state.stop_reason = RESIDUAL_CONTROL_STOP_REASON_POOL_BUDGET.to_string();
    }
}

fn candidate_improves_residual_control_round(
    candidate_profile: Option<&ResidualTailProfile>,
    candidate_objective: f64,
    state: &ResidualControlState,
    solver_settings: &CalibrationOptimizerSettings,
) -> bool {
    if let Some(candidate_profile) = candidate_profile {
        if let Some(round_best_profile) = state.round_best_profile.as_ref() {
            return candidate_profile.dominates(round_best_profile, solver_settings);
        }

        if candidate_profile_improves_incumbent(
            candidate_profile,
            state.accepted_profile.as_ref(),
            solver_settings,
        ) {
            return true;
        }
    }

    if state.round_best_profile.is_some() {
        return false;
    }

    objective_improves_incumbent(
        candidate_objective,
        state.round_best_objective.or(state.accepted_objective),
        solver_settings,
    )
}

fn candidate_profile_improves_incumbent(
    candidate_profile: &ResidualTailProfile,
    incumbent_profile: Option<&ResidualTailProfile>,
    solver_settings: &CalibrationOptimizerSettings,
) -> bool {
    match incumbent_profile {
        Some(incumbent_profile) => candidate_profile.dominates(incumbent_profile, solver_settings),
        None => true,
    }
}

fn objective_improves_incumbent(
    candidate_objective: f64,
    incumbent_objective: Option<f64>,
    solver_settings: &CalibrationOptimizerSettings,
) -> bool {
    match incumbent_objective {
        Some(incumbent_objective) => objective_improves(
            candidate_objective,
            incumbent_objective,
            solver_settings.absolute_tolerance,
            solver_settings.relative_tolerance,
        ),
        None => candidate_objective.is_finite(),
    }
}

pub(super) fn mark_pending_residual_control_states_completed(states: &mut [ResidualControlState]) {
    for state in states {
        if state.in_flight_count > 0 || state.completed {
            continue;
        }

        state.completed = true;
        state.stop_reason = RESIDUAL_CONTROL_STOP_REASON_POOL_BUDGET.to_string();
    }
}

pub(super) fn mark_unscheduled_residual_control_states_completed(
    states: &mut [ResidualControlState],
) {
    for state in states {
        if state.completed || state.in_flight_count > 0 {
            continue;
        }

        state.completed = true;
        state.stop_reason = RESIDUAL_CONTROL_STOP_REASON_NO_SCHEDULABLE_WORK.to_string();
    }
}
