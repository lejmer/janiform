use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CalibrationEffortOptions, CalibrationOptimizerSettings, JointRefinementStageFittedParameters,
    JointRefinementStageRestartEvaluationTimingDiagnostic, STANDARD_CALIBRATION_EFFORT,
};
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameters, spectral_radius_alpha_over_beta,
};

use crate::stages::adaptive_direct_search::{
    AdaptiveDirectSearchSettings, optimize_bounded_adaptive_direct_search,
};
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use crate::stages::return_components::ContinuousLikelihoodEvaluatorCounts;

use super::super::super::objective::{evaluate_joint_candidate, invalid_candidate_evaluation};
use super::super::super::setup::parameter_vector_to_fitted_parameters;
use super::super::selection::admissibility_threshold_from_sample_count;
use super::super::{
    JOINT_REFINEMENT_PARAMETER_FAMILY_COUNT, JointRefinementContext, JointRefinementFidelityTier,
    JointRefinementRestartOutcome, JointRefinementResumeStore,
    joint_refinement_has_parallel_worker_capacity, option_pricing_execution_failure_message,
    planned_restart_work_item_labels, simulation_infrastructure_failure_message,
};

// Joint restart direct search uses binary mesh refinement after a failed complete
// poll, then budgets one incumbent evaluation plus two complete poll rounds.
pub(super) const JOINT_REFINEMENT_DIRECT_SEARCH_FAILED_POLL_REFINEMENT_DIVISOR: usize = 2;
const JOINT_REFINEMENT_DIRECT_SEARCH_COMPLETE_POLL_ROUND_COUNT: usize =
    JOINT_REFINEMENT_DIRECT_SEARCH_FAILED_POLL_REFINEMENT_DIVISOR;
#[derive(Debug, Clone, Copy)]
pub(crate) struct JointRefinementBudgetSchedule {
    pub(crate) restart_count: usize,
    pub(crate) outer_cycle_count: usize,
    pub(crate) calibration_effort: CalibrationEffortOptions,
}

#[derive(Debug, Default)]
struct RestartProgressState {
    completed_units: usize,
    cumulative_local_evaluation_count: usize,
    valid_completed_units: usize,
}

#[derive(Debug, Default)]
struct RestartCompletedBestState {
    best_restart_id: Option<String>,
    best_global_loss: Option<f64>,
    best_global_loss_by_restart_id: BTreeMap<String, f64>,
}

#[derive(Debug, Default)]
struct RestartProgressCounters {
    completed_units: AtomicUsize,
    active_restart_count: AtomicUsize,
    aggregate_progress_percent_sum: AtomicUsize,
    cumulative_local_evaluation_count: AtomicUsize,
    valid_completed_units: AtomicUsize,
    completed_best_state: Mutex<RestartCompletedBestState>,
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
    current_restart_best_global_loss: Option<f64>,
    finished: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RestartObjectiveEvaluation {
    pub(super) objective: f64,
    pub(super) admissibility_blocker_count: usize,
    pub(super) max_standardized_residual: Option<f64>,
    pub(super) exact_likelihood_evaluator_counts: Option<ContinuousLikelihoodEvaluatorCounts>,
}

pub(crate) struct JointRefinementRestartOptimizationOutput {
    pub(crate) restart_outcomes: Vec<JointRefinementRestartOutcome>,
    pub(crate) evaluation_timing_diagnostics:
        Vec<JointRefinementStageRestartEvaluationTimingDiagnostic>,
}

pub(crate) fn optimize_restart_seeds(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut completed_restart_store: Option<&mut JointRefinementResumeStore>,
) -> JointRefinementRestartOptimizationOutput {
    debug_assert!(budget_schedule.restart_count > 0);

    // Resume passes only pending restart seeds while retaining the original search depth.
    let total_units = restart_seeds.len();

    let planned_work_item_labels = planned_restart_work_item_labels(
        restart_seeds
            .iter()
            .map(|(restart_id, _)| restart_id.as_str()),
    );

    let total_evaluation_budget =
        restart_candidate_total_evaluation_budget(context, solver_settings, budget_schedule);

    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(0),
            planned_work_item_labels,
            current_evaluation_count: Some(0),
            total_evaluation_budget: Some(total_evaluation_budget),
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
        optimize_restart_seeds_sequential(
            restart_seeds,
            context,
            solver_settings,
            budget_schedule,
            progress_sender,
            completed_restart_store.as_deref_mut(),
        )
    } else {
        optimize_restart_seeds_parallel(
            restart_seeds,
            context,
            solver_settings,
            budget_schedule,
            progress_sender,
            completed_restart_store.as_deref_mut(),
        )
    }
}

fn optimize_restart_seeds_sequential(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut completed_restart_store: Option<&mut JointRefinementResumeStore>,
) -> JointRefinementRestartOptimizationOutput {
    let total_units = restart_seeds.len();

    let mut progress_state = RestartProgressState::default();
    let progress_counters = RestartProgressCounters::default();

    let mut ordered_outcomes = Vec::with_capacity(total_units);

    for (restart_id, seed_vector) in restart_seeds {
        if joint_refinement_has_fatal_execution_failure(context) {
            break;
        }

        let restart_outcome = optimize_single_restart(OptimizeSingleRestartInput {
            restart_id: restart_id.as_str(),
            seed_vector: seed_vector.as_slice(),
            context,
            solver_settings,
            budget_schedule,
            total_units,
            progress_counters: &progress_counters,
            progress_sender,
        });

        progress_state.record(&restart_outcome);

        progress_counters
            .completed_units
            .store(progress_state.completed_units, Ordering::Relaxed);
        progress_counters.record_completed_progress_state(&progress_state);

        progress_counters.record_completed_best(
            restart_outcome.restart_id.as_str(),
            restart_outcome.final_evaluation.breakdown.global_loss,
        );

        emit_restart_progress(RestartProgressEmissionInput {
            progress_sender,
            total_units,
            progress_state: &progress_state,
            restart_outcome: &restart_outcome,
            progress_counters: &progress_counters,
            active_restart_count: progress_counters
                .active_restart_count
                .load(Ordering::Relaxed),
            total_evaluation_budget: restart_candidate_total_evaluation_budget(
                context,
                solver_settings,
                budget_schedule,
            ),
            aggregate_progress_percent: progress_counters
                .aggregate_progress_percent_sum
                .load(Ordering::Relaxed)
                .checked_div(total_units.max(1))
                .unwrap_or(0)
                .min(100),
        });

        if let Some(resume_store) = completed_restart_store.as_deref_mut() {
            resume_store.record_restart_outcome(&restart_outcome);
        }

        ordered_outcomes.push(restart_outcome);
    }

    JointRefinementRestartOptimizationOutput {
        restart_outcomes: ordered_outcomes,
        evaluation_timing_diagnostics: Vec::new(),
    }
}

fn optimize_restart_seeds_parallel(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    mut completed_restart_store: Option<&mut JointRefinementResumeStore>,
) -> JointRefinementRestartOptimizationOutput {
    let mut output = super::restart_probe_pool::optimize_restart_seeds_batched_poll_pool(
        restart_seeds,
        context,
        solver_settings,
        budget_schedule,
        progress_sender,
    );

    if let Some(resume_store) = completed_restart_store.as_deref_mut() {
        for outcome in &output.restart_outcomes {
            resume_store.record_restart_outcome(outcome);
        }
    }

    output.restart_outcomes.sort_by_key(|outcome| {
        restart_seeds
            .iter()
            .position(|(restart_id, _)| *restart_id == outcome.restart_id)
            .unwrap_or(usize::MAX)
    });

    output
}

pub(crate) fn joint_refinement_budget_schedule(
    solver_settings: &CalibrationOptimizerSettings,
    restart_count: usize,
    calibration_effort: Option<CalibrationEffortOptions>,
) -> JointRefinementBudgetSchedule {
    let base_outer_cycle_count = joint_refinement_base_outer_cycle_count(solver_settings);
    let calibration_effort = calibration_effort.unwrap_or(STANDARD_CALIBRATION_EFFORT);

    let restart_count = restart_count.max(1);
    let outer_cycle_count = base_outer_cycle_count
        .saturating_mul(base_outer_cycle_count)
        .checked_div(restart_count)
        .unwrap_or(1)
        .clamp(1, base_outer_cycle_count);

    JointRefinementBudgetSchedule {
        restart_count,
        outer_cycle_count,
        calibration_effort,
    }
}

pub(crate) fn desired_joint_refinement_restart_candidate_budget(
    restart_candidate_budget_override: Option<usize>,
    calibration_effort: Option<CalibrationEffortOptions>,
) -> usize {
    restart_candidate_budget_override
        .filter(|restart_candidate_budget| *restart_candidate_budget > 0)
        .unwrap_or_else(|| {
            calibration_effort
                .unwrap_or(STANDARD_CALIBRATION_EFFORT)
                .restart_candidate_budget
                .max(1)
        })
}

pub(crate) fn joint_refinement_restart_full_vector_direct_search_settings(
    solver_settings: &CalibrationOptimizerSettings,
    active_parameter_dimension: usize,
) -> AdaptiveDirectSearchSettings {
    let poll_direction_count = active_parameter_dimension.saturating_mul(2).max(1);

    AdaptiveDirectSearchSettings {
        absolute_tolerance: solver_settings.absolute_tolerance,
        relative_tolerance: solver_settings.relative_tolerance,
        initial_mesh_divisor: joint_refinement_restart_full_vector_initial_mesh_divisor(
            active_parameter_dimension,
        ),
        failed_poll_mesh_refinement_divisor:
            JOINT_REFINEMENT_DIRECT_SEARCH_FAILED_POLL_REFINEMENT_DIVISOR,
        evaluation_budget: 1usize.saturating_add(
            poll_direction_count
                .max(1)
                .saturating_mul(JOINT_REFINEMENT_DIRECT_SEARCH_COMPLETE_POLL_ROUND_COUNT),
        ),
    }
}

pub(crate) fn joint_refinement_restart_full_vector_initial_mesh_divisor(
    active_parameter_dimension: usize,
) -> usize {
    active_parameter_dimension.saturating_add(1).max(1)
}

fn joint_refinement_base_outer_cycle_count(
    _solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    JOINT_REFINEMENT_PARAMETER_FAMILY_COUNT
}

struct OptimizeSingleRestartInput<'a> {
    restart_id: &'a str,
    seed_vector: &'a [f64],
    context: &'a JointRefinementContext,
    solver_settings: &'a CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    total_units: usize,
    progress_counters: &'a RestartProgressCounters,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
}

fn optimize_single_restart(input: OptimizeSingleRestartInput<'_>) -> JointRefinementRestartOutcome {
    let OptimizeSingleRestartInput {
        restart_id,
        seed_vector,
        context,
        solver_settings,
        budget_schedule,
        total_units,
        progress_counters,
        progress_sender,
    } = input;

    if let Err(precheck_failure) = precheck_restart_seed(seed_vector, context) {
        return build_precheck_rejected_restart_outcome(
            restart_id,
            seed_vector.to_vec(),
            context,
            solver_settings,
            precheck_failure.as_str(),
        );
    }

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
            outer_cycle_index,
            &mut restart_progress,
        );

        current_vector = cycle_result.updated_vector;
        evaluation_count += cycle_result.evaluation_count;

        if let Some(invalid_step_stop_reason) = cycle_result.invalid_step_stop_reason {
            restart_progress.emit_finished();

            if invalid_step_stop_reason == "seed_summary_workload_infeasible" {
                return build_precheck_rejected_restart_outcome(
                    restart_id,
                    current_vector,
                    context,
                    solver_settings,
                    invalid_step_stop_reason,
                );
            }

            return build_short_circuited_restart_outcome(
                restart_id,
                current_vector,
                context,
                solver_settings,
                evaluation_count,
                invalid_step_stop_reason,
            );
        }

        stop_reason = select_restart_stop_reason(cycle_result.global_stop_reason);

        if !restart_stop_reason_allows_next_outer_cycle(stop_reason) {
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

pub(super) fn precheck_restart_seed(
    seed_vector: &[f64],
    context: &JointRefinementContext,
) -> Result<(), String> {
    precheck_restart_seed_vector_contract(
        seed_vector,
        context.lower_bounds.as_slice(),
        context.upper_bounds.as_slice(),
    )?;

    let fitted_parameters = parameter_vector_to_fitted_parameters(seed_vector, context)
        .ok_or_else(|| "model_construction_failed".to_string())?;

    precheck_hawkes_structural_domain(&fitted_parameters)?;
    precheck_nig_return_composition_domain(&fitted_parameters)?;
    precheck_deterministic_intensity_scales(&fitted_parameters, context)?;
    precheck_seed_summary_workload(&fitted_parameters, context)?;

    Ok(())
}

fn precheck_restart_seed_vector_contract(
    seed_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Result<(), String> {
    if seed_vector.len() != lower_bounds.len() || seed_vector.len() != upper_bounds.len() {
        return Err(format!(
            "parameter_vector_length_mismatch vector={} lower_bounds={} upper_bounds={}",
            seed_vector.len(),
            lower_bounds.len(),
            upper_bounds.len()
        ));
    }

    for (parameter_index, (value, (lower_bound, upper_bound))) in seed_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .enumerate()
    {
        if !value.is_finite() || !lower_bound.is_finite() || !upper_bound.is_finite() {
            return Err(format!(
                "non_finite_parameter_or_bound parameter_index={parameter_index}"
            ));
        }

        if lower_bound > upper_bound {
            return Err(format!(
                "invalid_parameter_bounds parameter_index={parameter_index}"
            ));
        }

        if value < lower_bound || value > upper_bound {
            return Err(format!(
                "parameter_out_of_bounds parameter_index={parameter_index} value={value} lower_bound={lower_bound} upper_bound={upper_bound}"
            ));
        }
    }

    Ok(())
}

fn precheck_hawkes_structural_domain(
    fitted_parameters: &JointRefinementStageFittedParameters,
) -> Result<(), String> {
    let hawkes = &fitted_parameters.hawkes_fitted_parameters;

    let report = HawkesKernelParameters::validate_structured_domain_with_stability_tolerance(
        hawkes.kernel_parameters.lambda_bar,
        hawkes.kernel_parameters.beta,
        hawkes.structured_alpha,
        hawkes.structured_rho,
        hawkes.self_dominance_policy,
        hawkes.stability_numeric_tolerance,
    );

    if report.is_valid() {
        return Ok(());
    }

    let Some(violation) = report.first_violation() else {
        return Err("hawkes_structural_domain_validation_failed".to_string());
    };

    Err(format!(
        "hawkes_structural_domain_validation_failed field={} constraint={} value={}",
        violation.field, violation.constraint, violation.value
    ))
}

fn precheck_nig_return_composition_domain(
    fitted_parameters: &JointRefinementStageFittedParameters,
) -> Result<(), String> {
    let nig_configuration = fitted_parameters.pricing_nig_shared_plus_shift_configuration();

    let driver_parameters = nig_configuration
        .derive_driver_parameters()
        .map_err(|source| format!("nig_driver_parameter_domain_failed error={source}"))?;

    let domain = driver_parameters.derive_return_composition_log_moment_domain();

    if !domain.lower_theta_exclusive.is_finite()
        || !domain.upper_theta_exclusive.is_finite()
        || domain.lower_theta_exclusive >= 0.0
        || domain.upper_theta_exclusive <= 0.0
        || domain.lower_theta_exclusive >= domain.upper_theta_exclusive
    {
        return Err(format!(
            "nig_return_composition_domain_invalid lower_theta_exclusive={} upper_theta_exclusive={}",
            domain.lower_theta_exclusive, domain.upper_theta_exclusive
        ));
    }

    driver_parameters
        .evaluate_return_composition_log_moment(1.0, 1.0, 0.0)
        .map_err(|source| format!("nig_return_composition_log_moment_failed error={source}"))?;

    driver_parameters
        .differentiate_return_composition_log_moment(1.0, 1.0, 0.0)
        .map_err(|source| {
            format!("nig_return_composition_log_moment_derivative_failed error={source}")
        })?;

    Ok(())
}

fn precheck_deterministic_intensity_scales(
    fitted_parameters: &JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
) -> Result<(), String> {
    let modeled_open_event_rate_per_hour =
        crate::stages::hawkes::stationary_open_event_rate_per_hour(
            &fitted_parameters.hawkes_fitted_parameters,
        )
        .ok_or_else(|| "stationary_open_event_rate_unavailable".to_string())?;
    if !modeled_open_event_rate_per_hour.is_finite() || modeled_open_event_rate_per_hour < 0.0 {
        return Err(format!(
            "stationary_open_event_rate_invalid value={modeled_open_event_rate_per_hour}"
        ));
    }

    if context.runtime_hourly_seasonality_factors.is_empty() {
        return Err("runtime_hourly_seasonality_factors_empty".to_string());
    }

    for (hour_index, factor) in context
        .runtime_hourly_seasonality_factors
        .iter()
        .enumerate()
    {
        if !factor.is_finite() || *factor < 0.0 {
            return Err(format!(
                "runtime_hourly_seasonality_factor_invalid hour_index={hour_index} value={factor}"
            ));
        }

        let hourly_activity_scale = modeled_open_event_rate_per_hour * *factor;
        if !hourly_activity_scale.is_finite() || hourly_activity_scale < 0.0 {
            return Err(format!(
                "runtime_hourly_activity_scale_invalid hour_index={hour_index} value={hourly_activity_scale}"
            ));
        }
    }

    for (interval_index, interval) in context
        .observed_data
        .observed_15m_intervals
        .iter()
        .enumerate()
    {
        if interval.interval_end_utc_nanos < interval.interval_start_utc_nanos {
            return Err(format!(
                "observed_interval_duration_invalid interval_index={interval_index}"
            ));
        }

        let duration_hours = duration_nanos_to_hours(
            interval.interval_end_utc_nanos - interval.interval_start_utc_nanos,
        );
        if !duration_hours.is_finite() || duration_hours < 0.0 {
            return Err(format!(
                "observed_interval_duration_invalid interval_index={interval_index} value={duration_hours}"
            ));
        }

        let integrated_activity_scale = modeled_open_event_rate_per_hour * duration_hours;
        if !integrated_activity_scale.is_finite() || integrated_activity_scale < 0.0 {
            return Err(format!(
                "observed_interval_integrated_activity_scale_invalid interval_index={interval_index} value={integrated_activity_scale}"
            ));
        }
    }

    let news_process = &fitted_parameters
        .return_components_fitted_parameters
        .news_process_configuration;

    let surprise_baseline_intensity = news_process.arrivals.surprise.baseline_intensity;
    if !surprise_baseline_intensity.is_finite() || surprise_baseline_intensity < 0.0 {
        return Err(format!(
            "surprise_baseline_intensity_invalid value={surprise_baseline_intensity}"
        ));
    }

    let surprise_weekly_hour_utc_multipliers = fitted_parameters
        .return_components_fitted_parameters
        .surprise_deterministic_intensity_profile
        .weekly_hour_utc_multipliers
        .as_slice();
    if surprise_weekly_hour_utc_multipliers.is_empty() {
        return Err("surprise_weekly_multipliers_empty".to_string());
    }

    for (bucket_index, multiplier) in surprise_weekly_hour_utc_multipliers.iter().enumerate() {
        if !multiplier.is_finite() || *multiplier < 0.0 {
            return Err(format!(
                "surprise_weekly_multiplier_invalid bucket_index={bucket_index} value={multiplier}"
            ));
        }

        let surprise_intensity = surprise_baseline_intensity * *multiplier;
        if !surprise_intensity.is_finite() || surprise_intensity < 0.0 {
            return Err(format!(
                "surprise_intensity_scale_invalid bucket_index={bucket_index} value={surprise_intensity}"
            ));
        }
    }

    for (field_name, scale) in [
        (
            "known_component_mark_scales.activity_coupling_scale",
            news_process
                .known_component_mark_scales
                .activity_coupling_scale,
        ),
        (
            "surprise_component_mark_scales.activity_coupling_scale",
            news_process
                .surprise_component_mark_scales
                .activity_coupling_scale,
        ),
    ] {
        if !scale.is_finite() || scale < 0.0 {
            return Err(format!(
                "activity_coupling_scale_invalid field={field_name} value={scale}"
            ));
        }
    }

    Ok(())
}

pub(crate) fn precheck_seed_summary_workload(
    fitted_parameters: &JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
) -> Result<(), String> {
    if fitted_parameters_runtime_branching_workload_is_infeasible(fitted_parameters, context) {
        return Err("seed_summary_workload_infeasible".to_string());
    }

    let workload_estimate = context
        .summary_simulation_scheduler
        .estimate_seed_summary_workload(fitted_parameters, context.local_simulation_seed)
        .map_err(|source| format!("seed_summary_workload_estimate_failed error={source:?}"))?;
    if summary_simulation_event_capacity_is_infeasible(
        workload_estimate.estimated_event_capacity,
        context.observed_data.observed_hourly_tick_counts.as_slice(),
    ) {
        return Err(format!(
            "seed_summary_workload_infeasible estimated_event_capacity={} observed_hourly_tick_count_sum={}",
            workload_estimate.estimated_event_capacity,
            context
                .observed_data
                .observed_hourly_tick_counts
                .iter()
                .sum::<f64>()
        ));
    }

    Ok(())
}

pub(super) fn build_precheck_rejected_restart_outcome(
    restart_id: &str,
    current_vector: Vec<f64>,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    failure_reason: &str,
) -> JointRefinementRestartOutcome {
    let invalid_reason = format!("restart_seed_precheck_failed:{failure_reason}");

    let local_evaluation = invalid_candidate_evaluation(
        context,
        &context.baseline_hawkes_fitted_parameters,
        &context.baseline_return_components_fitted_parameters,
        solver_settings.constraint_penalty_weight,
        invalid_reason.as_str(),
    );

    JointRefinementRestartOutcome {
        restart_id: restart_id.to_string(),
        parameter_vector: current_vector,
        final_evaluation: local_evaluation.clone(),
        local_evaluation,
        local_evaluation_count: 0,
        stop_reason: "restart_seed_precheck_failed".to_string(),
    }
}

struct RestartCycleResult {
    updated_vector: Vec<f64>,
    global_stop_reason: &'static str,
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
    if restart_seed_summary_workload_is_infeasible(current_vector, context) {
        return RestartCycleResult {
            updated_vector: current_vector.to_vec(),
            global_stop_reason: "seed_summary_workload_infeasible",
            invalid_step_stop_reason: Some("seed_summary_workload_infeasible"),
            evaluation_count: 0,
        };
    }

    let Some(initial_objective) = evaluate_prechecked_full_parameter_vector_objective(
        current_vector,
        context,
        solver_settings,
    ) else {
        restart_progress.record_evaluation(outer_cycle, None);

        return RestartCycleResult {
            updated_vector: current_vector.to_vec(),
            global_stop_reason: "invalid_step",
            invalid_step_stop_reason: Some("invalid_step"),
            evaluation_count: 1,
        };
    };

    restart_progress.record_evaluation(outer_cycle, Some(initial_objective));

    let mut pending_initial_objective = Some(initial_objective);
    let mut evaluate_full_vector = |parameter_vector: &[f64]| -> Option<f64> {
        if joint_refinement_has_fatal_execution_failure(context) {
            return None;
        }

        if let Some(initial_objective) = pending_initial_objective.take() {
            return Some(initial_objective.objective);
        }

        if restart_seed_summary_workload_is_infeasible(parameter_vector, context) {
            restart_progress.record_evaluation(outer_cycle, None);
            return None;
        }

        let objective = evaluate_prechecked_full_parameter_vector_objective(
            parameter_vector,
            context,
            solver_settings,
        );

        restart_progress.record_evaluation(outer_cycle, objective);

        objective.map(|evaluation| evaluation.objective)
    };

    let optimizer_result = optimize_bounded_adaptive_direct_search(
        current_vector,
        context.lower_bounds.as_slice(),
        context.upper_bounds.as_slice(),
        joint_refinement_restart_full_vector_direct_search_settings(
            solver_settings,
            current_vector.len(),
        ),
        &mut evaluate_full_vector,
    );

    let Some(optimizer_result) = optimizer_result else {
        return RestartCycleResult {
            updated_vector: current_vector.to_vec(),
            global_stop_reason: "invalid_step",
            invalid_step_stop_reason: Some("invalid_step"),
            evaluation_count: 0,
        };
    };

    let refreshed_vector = refreshed_full_vector_candidate(
        current_vector,
        optimizer_result.best_point.as_slice(),
        context,
        solver_settings,
    );

    RestartCycleResult {
        updated_vector: refreshed_vector,
        global_stop_reason: optimizer_result.stop_reason,
        invalid_step_stop_reason: None,
        evaluation_count: optimizer_result.evaluation_count,
    }
}

pub(super) fn refreshed_full_vector_candidate(
    previous_vector: &[f64],
    candidate_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<f64> {
    let previous_evaluation = evaluate_joint_candidate(
        previous_vector,
        context,
        full_vector_refresh_fidelity_tier(),
        solver_settings,
    );

    let candidate_evaluation = evaluate_joint_candidate(
        candidate_vector,
        context,
        full_vector_refresh_fidelity_tier(),
        solver_settings,
    );

    if candidate_evaluation.invalid_reason.is_some() {
        return previous_vector.to_vec();
    }

    if previous_evaluation.invalid_reason.is_some() {
        return candidate_vector.to_vec();
    }

    let candidate_global_loss = candidate_evaluation.breakdown.global_loss;
    if !candidate_global_loss.is_finite() {
        return previous_vector.to_vec();
    }

    let previous_global_loss = previous_evaluation.breakdown.global_loss;
    if previous_global_loss.is_finite() && candidate_global_loss > previous_global_loss {
        return previous_vector.to_vec();
    }

    candidate_vector.to_vec()
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

fn select_restart_stop_reason(global_stop_reason: &'static str) -> &'static str {
    global_stop_reason
}

fn restart_stop_reason_allows_next_outer_cycle(stop_reason: &str) -> bool {
    matches!(stop_reason, "tolerance" | "budget" | "dynamics_infeasible")
}

pub(super) fn evaluate_prechecked_full_parameter_vector_objective(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> Option<RestartObjectiveEvaluation> {
    if precheck_restart_seed(parameter_vector, context).is_err() {
        return None;
    }

    evaluate_full_parameter_vector_objective(parameter_vector, context, solver_settings)
}

fn evaluate_full_parameter_vector_objective(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> Option<RestartObjectiveEvaluation> {
    let candidate_evaluation = evaluate_joint_candidate(
        parameter_vector,
        context,
        full_vector_search_fidelity_tier(),
        solver_settings,
    );

    candidate_evaluation
        .invalid_reason
        .is_none()
        .then_some(RestartObjectiveEvaluation {
            objective: candidate_evaluation.breakdown.global_loss,
            admissibility_blocker_count: candidate_evaluation
                .breakdown
                .admissibility
                .blockers
                .len(),
            max_standardized_residual: Some(
                candidate_evaluation
                    .breakdown
                    .admissibility
                    .max_standardized_residual,
            ),
            exact_likelihood_evaluator_counts: Some(
                candidate_evaluation.exact_likelihood_evaluator_counts,
            ),
        })
}

fn full_vector_search_fidelity_tier() -> JointRefinementFidelityTier {
    JointRefinementFidelityTier::CoarseLocal
}

fn full_vector_refresh_fidelity_tier() -> JointRefinementFidelityTier {
    JointRefinementFidelityTier::Local
}

fn restart_seed_summary_workload_is_infeasible(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> bool {
    let Some(fitted_parameters) = parameter_vector_to_fitted_parameters(parameter_vector, context)
    else {
        return true;
    };

    precheck_seed_summary_workload(&fitted_parameters, context).is_err()
}

fn fitted_parameters_runtime_branching_workload_is_infeasible(
    fitted_parameters: &JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
) -> bool {
    let Ok(derived_alpha) = HawkesKernelLoadings::try_from_structured_alpha(
        fitted_parameters.hawkes_fitted_parameters.structured_alpha,
    ) else {
        return true;
    };

    let runtime_effective_branching_ratio = spectral_radius_alpha_over_beta(
        derived_alpha,
        fitted_parameters
            .hawkes_fitted_parameters
            .kernel_parameters
            .beta,
    );

    runtime_branching_workload_is_infeasible(
        runtime_effective_branching_ratio,
        context.observed_data.observed_hourly_tick_counts.len(),
    )
}

fn runtime_branching_workload_is_infeasible(
    runtime_effective_branching_ratio: f64,
    hourly_sample_count: usize,
) -> bool {
    if !runtime_effective_branching_ratio.is_finite() || runtime_effective_branching_ratio >= 1.0 {
        return true;
    }

    let minimum_branching_margin = (hourly_sample_count.max(1) as f64).recip().sqrt();
    1.0 - runtime_effective_branching_ratio < minimum_branching_margin
}

fn summary_simulation_event_capacity_is_infeasible(
    estimated_event_capacity: usize,
    observed_hourly_tick_counts: &[f64],
) -> bool {
    let observed_tick_count_sum = observed_hourly_tick_counts.iter().sum::<f64>();
    if !observed_tick_count_sum.is_finite() || observed_tick_count_sum < 0.0 {
        return true;
    }

    let admissible_event_capacity = observed_tick_count_sum
        * admissibility_threshold_from_sample_count(observed_hourly_tick_counts.len());

    estimated_event_capacity as f64 > admissible_event_capacity
}

pub(super) fn joint_refinement_has_fatal_execution_failure(
    context: &JointRefinementContext,
) -> bool {
    simulation_infrastructure_failure_message(context).is_some()
        || option_pricing_execution_failure_message(context).is_some()
}

pub(super) fn restart_candidate_total_evaluation_budget(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
) -> usize {
    budget_schedule
        .outer_cycle_count
        .max(1)
        .saturating_mul(restart_cycle_evaluation_budget(context, solver_settings).max(1))
}

pub(super) fn restart_cycle_evaluation_budget(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    joint_refinement_restart_full_vector_direct_search_settings(
        solver_settings,
        context.active_parameter_layout.vector_length(),
    )
    .evaluation_budget
}

pub(super) fn emit_joint_refinement_progress(
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
            current_restart_best_global_loss: None,
            finished: false,
        }
    }

    fn emit_started(&mut self) {
        let active_restart_count = self
            .counters
            .active_restart_count
            .fetch_add(1, Ordering::Relaxed)
            + 1;

        self.emit_progress(None, None, Some(active_restart_count), None);
    }

    fn record_evaluation(
        &mut self,
        completed_outer_cycles: usize,
        evaluation: Option<RestartObjectiveEvaluation>,
    ) {
        self.current_evaluation_count = self.current_evaluation_count.saturating_add(1);

        if let Some(global_loss) = evaluation.map(|evaluation| evaluation.objective) {
            self.current_restart_best_global_loss = Some(
                self.current_restart_best_global_loss
                    .map(|current_best| current_best.min(global_loss))
                    .unwrap_or(global_loss),
            );
        }

        let progress_percent = self.running_evaluation_progress_percent();
        if self.last_evaluation_progress_percent == Some(progress_percent) {
            return;
        }

        self.record_progress_percent(progress_percent);

        self.last_evaluation_progress_percent = Some(progress_percent);

        self.emit_progress(
            Some(completed_outer_cycles),
            Some(progress_percent),
            Some(self.counters.active_restart_count.load(Ordering::Relaxed)),
            evaluation,
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
        completed_outer_cycles: Option<usize>,
        evaluation_progress_percent: Option<usize>,
        active_restart_count: Option<usize>,
        evaluation: Option<RestartObjectiveEvaluation>,
    ) {
        let current_global_loss = evaluation.map(|evaluation| evaluation.objective);

        let exact_likelihood_evaluator_counts =
            evaluation.and_then(|evaluation| evaluation.exact_likelihood_evaluator_counts);

        let (best_restart_id, best_global_loss) = self.counters.completed_best_snapshot();

        let completed_progress_state = self.counters.completed_progress_state();

        let completed_progress_totals = (completed_progress_state.completed_units > 0).then_some((
            completed_progress_state.valid_completed_units,
            completed_progress_state.cumulative_local_evaluation_count,
        ));

        emit_joint_refinement_progress(
            self.progress_sender,
            JointRefinementStageProgressUpdate {
                aggregate_progress_percent: Some(self.aggregate_progress_percent()),
                current_item_label: Some(self.restart_id.to_string()),
                current_restart_id: Some(self.restart_id.to_string()),
                active_restart_count,
                completed_outer_cycles,
                total_outer_cycles: Some(self.total_outer_cycles),
                current_evaluation_count: Some(self.current_evaluation_count),
                total_evaluation_budget: Some(self.total_evaluation_budget),
                evaluation_progress_percent,
                valid_completed_units: completed_progress_totals
                    .map(|(valid_completed_units, _)| valid_completed_units),
                cumulative_local_evaluation_count: completed_progress_totals.map(
                    |(_, cumulative_local_evaluation_count)| cumulative_local_evaluation_count,
                ),
                current_global_loss,
                current_restart_best_global_loss: self.current_restart_best_global_loss,
                best_restart_id,
                best_global_loss,
                exact_closed_form_buy_count: exact_likelihood_evaluator_counts
                    .map(|counts| counts.closed_form_buy),
                exact_closed_form_sell_count: exact_likelihood_evaluator_counts
                    .map(|counts| counts.closed_form_sell),
                exact_tilted_mixed_count: exact_likelihood_evaluator_counts
                    .map(|counts| counts.tilted_mixed),
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

struct RestartProgressEmissionInput<'a> {
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    total_units: usize,
    progress_state: &'a RestartProgressState,
    restart_outcome: &'a JointRefinementRestartOutcome,
    progress_counters: &'a RestartProgressCounters,
    active_restart_count: usize,
    total_evaluation_budget: usize,
    aggregate_progress_percent: usize,
}

fn emit_restart_progress(input: RestartProgressEmissionInput<'_>) {
    let RestartProgressEmissionInput {
        progress_sender,
        total_units,
        progress_state,
        restart_outcome,
        progress_counters,
        active_restart_count,
        total_evaluation_budget,
        aggregate_progress_percent,
    } = input;

    let completed_units = progress_state.completed_units;

    let (best_restart_id, best_global_loss) = progress_counters.completed_best_snapshot();

    let final_display_objective = restart_outcome.final_evaluation.breakdown.global_loss;

    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(aggregate_progress_percent),
            current_item_label: Some(restart_outcome.restart_id.clone()),
            current_restart_id: Some(restart_outcome.restart_id.clone()),
            active_restart_count: Some(active_restart_count),
            // Completion rows use the callback-counted progress budget. The local
            // evaluation count remains available through cumulative diagnostics.
            current_evaluation_count: Some(total_evaluation_budget),
            total_evaluation_budget: Some(total_evaluation_budget),
            evaluation_progress_percent: Some(100),
            valid_completed_units: Some(progress_state.valid_completed_units),
            cumulative_local_evaluation_count: Some(
                progress_state.cumulative_local_evaluation_count,
            ),
            current_global_loss: Some(final_display_objective),
            current_restart_best_global_loss: None,
            current_candidate_assessment: None,
            best_restart_id,
            best_global_loss,
            exact_closed_form_buy_count: Some(
                restart_outcome
                    .final_evaluation
                    .exact_likelihood_evaluator_counts
                    .closed_form_buy,
            ),
            exact_closed_form_sell_count: Some(
                restart_outcome
                    .final_evaluation
                    .exact_likelihood_evaluator_counts
                    .closed_form_sell,
            ),
            exact_tilted_mixed_count: Some(
                restart_outcome
                    .final_evaluation
                    .exact_likelihood_evaluator_counts
                    .tilted_mixed,
            ),
            ..JointRefinementStageProgressUpdate::new(
                JointRefinementStageProgressPhase::RestartOptimization,
                completed_units,
                total_units,
            )
        },
    );
}

impl RestartProgressCounters {
    fn record_completed_progress_state(&self, progress_state: &RestartProgressState) {
        self.valid_completed_units
            .store(progress_state.valid_completed_units, Ordering::Relaxed);
        self.cumulative_local_evaluation_count.store(
            progress_state.cumulative_local_evaluation_count,
            Ordering::Relaxed,
        );
    }

    fn completed_progress_state(&self) -> RestartProgressState {
        RestartProgressState {
            completed_units: self.completed_units.load(Ordering::Relaxed),
            cumulative_local_evaluation_count: self
                .cumulative_local_evaluation_count
                .load(Ordering::Relaxed),
            valid_completed_units: self.valid_completed_units.load(Ordering::Relaxed),
        }
    }

    fn record_completed_best(&self, restart_id: &str, global_loss: f64) {
        let Ok(mut completed_best_state) = self.completed_best_state.lock() else {
            return;
        };

        match completed_best_state
            .best_global_loss_by_restart_id
            .get(restart_id)
            .copied()
        {
            Some(current_best) if global_loss >= current_best => {}
            _ => {
                completed_best_state
                    .best_global_loss_by_restart_id
                    .insert(restart_id.to_string(), global_loss);
            }
        }

        match completed_best_state.best_global_loss {
            Some(current_best) if global_loss >= current_best => {}
            _ => {
                completed_best_state.best_global_loss = Some(global_loss);
                completed_best_state.best_restart_id = Some(restart_id.to_string());
            }
        }
    }

    fn completed_best_snapshot(&self) -> (Option<String>, Option<f64>) {
        let Ok(completed_best_state) = self.completed_best_state.lock() else {
            return (None, None);
        };

        (
            completed_best_state.best_restart_id.clone(),
            completed_best_state.best_global_loss,
        )
    }

    #[cfg(test)]
    fn restart_completed_best_global_loss(&self, restart_id: &str) -> Option<f64> {
        let Ok(completed_best_state) = self.completed_best_state.lock() else {
            return None;
        };

        completed_best_state
            .best_global_loss_by_restart_id
            .get(restart_id)
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    use janiform_calibration_artifacts::CalibrationOptimizerSettings;

    use super::{
        RestartCandidateProgress, RestartObjectiveEvaluation, RestartProgressCounters,
        RestartProgressState, full_vector_refresh_fidelity_tier, full_vector_search_fidelity_tier,
        joint_refinement_restart_full_vector_direct_search_settings,
        precheck_hawkes_structural_domain, precheck_nig_return_composition_domain,
        precheck_restart_seed_vector_contract, restart_stop_reason_allows_next_outer_cycle,
        runtime_branching_workload_is_infeasible, select_restart_stop_reason,
        summary_simulation_event_capacity_is_infeasible,
    };
    use crate::stages::joint_refinement::execution::{
        JointRefinementFidelityTier, test_joint_fitted_parameters,
    };

    fn restart_objective_for_test(objective: f64) -> RestartObjectiveEvaluation {
        RestartObjectiveEvaluation {
            objective,
            admissibility_blocker_count: 0,
            max_standardized_residual: None,
            exact_likelihood_evaluator_counts: None,
        }
    }

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
            current_restart_best_global_loss: None,
            finished: false,
        };

        assert_eq!(restart_progress.running_evaluation_progress_percent(), 99);

        restart_progress.current_evaluation_count = 5;

        assert_eq!(restart_progress.running_evaluation_progress_percent(), 50);
    }

    #[test]
    fn running_restart_progress_emits_current_restart_accepted_merit() {
        let (progress_sender, progress_receiver) = mpsc::channel();
        let progress_counters = RestartProgressCounters::default();

        let mut halton_progress = RestartCandidateProgress {
            progress_sender: Some(&progress_sender),
            restart_id: "halton_01",
            total_units: 2,
            counters: &progress_counters,
            total_outer_cycles: 1,
            total_evaluation_budget: 10,
            current_evaluation_count: 0,
            last_evaluation_progress_percent: None,
            current_restart_best_global_loss: None,
            finished: false,
        };

        halton_progress.record_evaluation(1, Some(restart_objective_for_test(10.0)));
        progress_receiver.recv().unwrap();

        halton_progress.record_evaluation(1, Some(restart_objective_for_test(4.0)));

        let progress_update = progress_receiver.recv().unwrap();

        assert_eq!(progress_update.best_restart_id, None);
        assert_eq!(progress_update.best_global_loss, None);
        assert_eq!(progress_update.current_restart_best_global_loss, Some(4.0));
        assert_eq!(progress_update.valid_completed_units, None);
    }

    #[test]
    fn running_restart_progress_preserves_completed_candidate_totals() {
        let (progress_sender, progress_receiver) = mpsc::channel();
        let progress_counters = RestartProgressCounters::default();
        progress_counters
            .completed_units
            .store(2, Ordering::Relaxed);
        progress_counters.record_completed_progress_state(&RestartProgressState {
            completed_units: 2,
            cumulative_local_evaluation_count: 41,
            valid_completed_units: 1,
        });

        let mut restart_progress = RestartCandidateProgress {
            progress_sender: Some(&progress_sender),
            restart_id: "halton_03",
            total_units: 5,
            counters: &progress_counters,
            total_outer_cycles: 1,
            total_evaluation_budget: 10,
            current_evaluation_count: 0,
            last_evaluation_progress_percent: None,
            current_restart_best_global_loss: None,
            finished: false,
        };

        restart_progress.record_evaluation(1, Some(restart_objective_for_test(3.0)));

        let progress_update = progress_receiver.recv().unwrap();

        assert_eq!(progress_update.valid_completed_units, Some(1));
        assert_eq!(progress_update.cumulative_local_evaluation_count, Some(41));
    }

    #[test]
    fn restart_progress_counters_track_completed_best_merit_by_restart() {
        let progress_counters = RestartProgressCounters::default();

        progress_counters.record_completed_best("baseline", 10.0);
        progress_counters.record_completed_best("halton_01", 4.0);
        progress_counters.record_completed_best("baseline", 8.0);
        progress_counters.record_completed_best("baseline", 12.0);

        assert_eq!(
            progress_counters.restart_completed_best_global_loss("baseline"),
            Some(8.0)
        );
        assert_eq!(
            progress_counters.restart_completed_best_global_loss("halton_01"),
            Some(4.0)
        );
    }

    #[test]
    fn restart_full_vector_progress_budget_matches_optimizer_budget() {
        let solver_settings =
            CalibrationOptimizerSettings::try_new("joint-test", 256, 4, 1.0e-6, 1.0e-6, 10.0)
                .expect("solver settings should validate");

        let four_dimensional_settings =
            joint_refinement_restart_full_vector_direct_search_settings(&solver_settings, 4);
        let one_dimensional_settings =
            joint_refinement_restart_full_vector_direct_search_settings(&solver_settings, 1);

        assert_eq!(four_dimensional_settings.initial_mesh_divisor, 5);
        assert_eq!(four_dimensional_settings.evaluation_budget, 17);
        assert_eq!(one_dimensional_settings.evaluation_budget, 5);
    }

    #[test]
    fn restart_stop_reason_preserves_full_vector_optimizer_reason() {
        assert_eq!(select_restart_stop_reason("budget"), "budget");
        assert_eq!(select_restart_stop_reason("tolerance"), "tolerance");
    }

    #[test]
    fn restart_outer_cycle_continues_after_retryable_stop_reasons() {
        assert!(restart_stop_reason_allows_next_outer_cycle("budget"));
        assert!(restart_stop_reason_allows_next_outer_cycle("tolerance"));
        assert!(restart_stop_reason_allows_next_outer_cycle(
            "dynamics_infeasible"
        ));
        assert!(!restart_stop_reason_allows_next_outer_cycle("invalid_step"));
        assert!(!restart_stop_reason_allows_next_outer_cycle(
            "seed_summary_workload_infeasible"
        ));
    }

    #[test]
    fn runtime_branching_workload_guard_uses_hourly_sample_resolution() {
        assert!(runtime_branching_workload_is_infeasible(0.95, 100));
        assert!(!runtime_branching_workload_is_infeasible(0.89, 100));
        assert!(runtime_branching_workload_is_infeasible(1.0, 100));
    }

    #[test]
    fn summary_simulation_event_capacity_guard_uses_observed_activity_resolution() {
        let observed_hourly_tick_counts = [100.0; 100];

        assert!(!summary_simulation_event_capacity_is_infeasible(
            10_900,
            &observed_hourly_tick_counts,
        ));
        assert!(summary_simulation_event_capacity_is_infeasible(
            11_001,
            &observed_hourly_tick_counts,
        ));
    }

    #[test]
    fn restart_full_vector_objectives_use_coarse_search_fidelity() {
        assert_eq!(
            full_vector_search_fidelity_tier(),
            JointRefinementFidelityTier::CoarseLocal
        );
    }

    #[test]
    fn restart_full_vector_refresh_uses_seeded_local_fidelity() {
        assert_eq!(
            full_vector_refresh_fidelity_tier(),
            JointRefinementFidelityTier::Local
        );
    }

    #[test]
    fn restart_seed_vector_precheck_rejects_non_finite_parameters() {
        let error =
            precheck_restart_seed_vector_contract(&[0.0, f64::NAN], &[-1.0, -1.0], &[1.0, 1.0])
                .expect_err("non-finite seed parameter should be rejected");

        assert!(error.contains("non_finite_parameter_or_bound"));
    }

    #[test]
    fn restart_seed_vector_precheck_rejects_out_of_bounds_parameters() {
        let error = precheck_restart_seed_vector_contract(&[0.0, 2.0], &[-1.0, -1.0], &[1.0, 1.0])
            .expect_err("out-of-bounds seed parameter should be rejected");

        assert!(error.contains("parameter_out_of_bounds"));
    }

    #[test]
    fn restart_seed_model_domain_prechecks_accept_valid_parameters() {
        let fitted_parameters = test_joint_fitted_parameters();

        precheck_hawkes_structural_domain(&fitted_parameters)
            .expect("valid Hawkes parameters should pass precheck");
        precheck_nig_return_composition_domain(&fitted_parameters)
            .expect("valid NIG return-composition domain should pass precheck");
    }

    #[test]
    fn restart_seed_hawkes_precheck_rejects_structural_domain_violations() {
        let mut fitted_parameters = test_joint_fitted_parameters();

        fitted_parameters
            .hawkes_fitted_parameters
            .structured_alpha
            .delta_self = -0.1;

        let error = precheck_hawkes_structural_domain(&fitted_parameters)
            .expect_err("invalid structured Hawkes parameters should be rejected");

        assert!(error.contains("hawkes_structural_domain_validation_failed"));
    }

    #[test]
    fn restart_seed_nig_precheck_rejects_return_composition_domain_violations() {
        let mut fitted_parameters = test_joint_fitted_parameters();

        fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration
            .delta_beta = 0.1;

        let error = precheck_nig_return_composition_domain(&fitted_parameters)
            .expect_err("invalid NIG return-composition domain should be rejected");

        assert!(error.contains("nig_driver_parameter_domain_failed"));
    }
}
