use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_calibration_artifacts::JointRefinementStageFittedParameters;
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameters, spectral_radius_alpha_over_beta,
};

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use crate::stages::optimization::{PowellOptimizerSettings, optimize_bounded_powell};

use super::super::objective::{
    compressed_final_objective, dynamics_block_search_objective, evaluate_joint_candidate,
    evaluate_joint_candidate_nig_block_search_objective, invalid_candidate_evaluation,
};
use super::super::setup::parameter_vector_to_fitted_parameters;
use super::selection::admissibility_threshold_from_sample_count;
use super::{
    JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT,
    JOINT_REFINEMENT_THRESHOLD_RELATIVE_ADMISSIBILITY_SCORE, JointRefinementContext,
    JointRefinementFidelityTier, JointRefinementRestartOutcome,
    joint_raw_metrics_are_dynamics_feasible, joint_refinement_admissibility_thresholds,
    joint_refinement_has_parallel_worker_capacity, option_pricing_execution_failure_message,
    simulation_infrastructure_failure_message, with_joint_refinement_thread_pool,
};
pub(crate) const JOINT_REFINEMENT_DEFAULT_RESTART_CANDIDATE_BUDGET: usize = 15;

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
    best_weighted_objective_by_restart_id: BTreeMap<String, f64>,
}

#[derive(Debug, Default)]
struct RestartProgressCounters {
    completed_units: AtomicUsize,
    active_restart_count: AtomicUsize,
    aggregate_progress_percent_sum: AtomicUsize,
    cumulative_local_evaluation_count: AtomicUsize,
    valid_completed_units: AtomicUsize,
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
    debug_assert!(budget_schedule.restart_count > 0);
    // Resume passes only pending restart seeds while retaining the original search depth.
    let total_units = restart_seeds.len();
    let planned_work_item_labels = restart_seeds
        .iter()
        .map(|(restart_id, _)| restart_id.clone())
        .collect::<Vec<_>>();
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
        progress_counters.record_completed_progress_state(&progress_state);

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
            context,
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
                progress_counters.record_completed_progress_state(&progress_state);

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
                    context,
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

pub(crate) fn desired_joint_refinement_restart_candidate_budget(
    restart_candidate_budget_override: Option<usize>,
) -> usize {
    restart_candidate_budget_override
        .filter(|restart_candidate_budget| *restart_candidate_budget > 0)
        .unwrap_or(JOINT_REFINEMENT_DEFAULT_RESTART_CANDIDATE_BUDGET)
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

    let admissibility_thresholds = joint_refinement_admissibility_thresholds(context);

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

        stop_reason = select_restart_stop_reason(
            cycle_result.hawkes_stop_reason,
            cycle_result.nig_stop_reason,
            cycle_result.coupling_stop_reason,
        );
        if stop_reason != "tolerance" && stop_reason != "dynamics_infeasible" {
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

    if !joint_raw_metrics_are_dynamics_feasible(
        &local_evaluation.raw_admissibility_metrics,
        admissibility_thresholds,
    ) {
        restart_progress.emit_finished();

        return JointRefinementRestartOutcome {
            restart_id: restart_id.to_string(),
            parameter_vector: current_vector,
            final_evaluation: local_evaluation.clone(),
            local_evaluation,
            local_evaluation_count: evaluation_count,
            stop_reason: if stop_reason == "tolerance" {
                "local_dynamics_infeasible".to_string()
            } else {
                stop_reason.to_string()
            },
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

fn precheck_restart_seed(
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
    let nig_configuration = fitted_parameters
        .pricing_block_fitted_parameters
        .as_ref()
        .map(|pricing_block| pricing_block.nig_shared_plus_shift_configuration)
        .unwrap_or(
            fitted_parameters
                .return_components_fitted_parameters
                .nig_shared_plus_shift_configuration,
        );

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

fn build_precheck_rejected_restart_outcome(
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

    if nig_stop_reason == "invalid_step"
        || nig_stop_reason == "threshold_admissible_best_already_available"
    {
        return RestartCycleResult {
            updated_vector,
            hawkes_stop_reason: "tolerance",
            nig_stop_reason,
            coupling_stop_reason: "tolerance",
            invalid_step_stop_reason: (nig_stop_reason == "invalid_step")
                .then_some(nig_stop_reason),
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
            nig_stop_reason: "tolerance",
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

    let dynamics_evaluation = evaluate_joint_candidate(
        updated_vector.as_slice(),
        context,
        JointRefinementFidelityTier::Local,
        solver_settings,
    );

    if dynamics_evaluation.invalid_reason.is_some() {
        return RestartCycleResult {
            updated_vector,
            hawkes_stop_reason,
            nig_stop_reason: "tolerance",
            coupling_stop_reason,
            invalid_step_stop_reason: Some("invalid_step"),
            evaluation_count,
        };
    }

    let admissibility_thresholds = joint_refinement_admissibility_thresholds(context);
    if !joint_raw_metrics_are_dynamics_feasible(
        &dynamics_evaluation.raw_admissibility_metrics,
        admissibility_thresholds,
    ) {
        return RestartCycleResult {
            updated_vector,
            hawkes_stop_reason,
            nig_stop_reason: "dynamics_infeasible",
            coupling_stop_reason,
            invalid_step_stop_reason: None,
            evaluation_count,
        };
    }

    RestartCycleResult {
        updated_vector,
        hawkes_stop_reason,
        nig_stop_reason,
        coupling_stop_reason,
        invalid_step_stop_reason: (coupling_stop_reason == "invalid_step")
            .then_some(coupling_stop_reason)
            .or((nig_stop_reason == "invalid_step").then_some(nig_stop_reason)),
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
        .find(|stop_reason| *stop_reason != "tolerance" && *stop_reason != "dynamics_infeasible")
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

    if restart_seed_summary_workload_is_infeasible(current_vector, context) {
        return (
            current_vector.to_vec(),
            "seed_summary_workload_infeasible",
            0,
        );
    }

    let Some(initial_block_objective) = evaluate_parameter_block_objective(
        current_vector,
        block_indices,
        initial_point.as_slice(),
        context,
        solver_settings,
        parameter_block_label,
    ) else {
        restart_progress.record_evaluation(parameter_block_label, outer_cycle, None);
        return (current_vector.to_vec(), "invalid_step", 1);
    };

    restart_progress.record_evaluation(
        parameter_block_label,
        outer_cycle,
        Some(initial_block_objective),
    );

    if restart_block_initial_objective_is_noncompetitive(
        initial_block_objective,
        restart_progress.counters,
    ) {
        return (
            current_vector.to_vec(),
            "threshold_admissible_best_already_available",
            1,
        );
    }

    let mut pending_initial_block_objective = Some(initial_block_objective);
    let mut evaluate_block = |block_vector: &[f64]| -> Option<f64> {
        if joint_refinement_has_fatal_execution_failure(context) {
            return None;
        }

        if let Some(initial_block_objective) = pending_initial_block_objective.take() {
            return Some(initial_block_objective);
        }

        let full_vector = substitute_block(current_vector, block_indices, block_vector);
        if restart_seed_summary_workload_is_infeasible(full_vector.as_slice(), context) {
            restart_progress.record_evaluation(parameter_block_label, outer_cycle, None);
            return None;
        }

        let block_objective = evaluate_parameter_block_objective(
            current_vector,
            block_indices,
            block_vector,
            context,
            solver_settings,
            parameter_block_label,
        );
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

fn evaluate_parameter_block_objective(
    current_vector: &[f64],
    block_indices: &[usize],
    block_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    parameter_block_label: &'static str,
) -> Option<f64> {
    let full_vector = substitute_block(current_vector, block_indices, block_vector);

    match parameter_block_label {
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

            candidate_evaluation.invalid_reason.is_none().then_some(
                dynamics_block_search_objective(
                    &candidate_evaluation.raw_admissibility_metrics,
                    joint_refinement_admissibility_thresholds(context),
                ),
            )
        }
    }
}

fn restart_block_initial_objective_is_noncompetitive(
    initial_block_objective: f64,
    progress_counters: &RestartProgressCounters,
) -> bool {
    let (_, Some(best_weighted_objective)) = progress_counters.display_best_snapshot() else {
        return false;
    };

    best_weighted_objective <= JOINT_REFINEMENT_THRESHOLD_RELATIVE_ADMISSIBILITY_SCORE
        && initial_block_objective > JOINT_REFINEMENT_THRESHOLD_RELATIVE_ADMISSIBILITY_SCORE
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

    let runtime_effective_branching_ratio = context.max_runtime_hourly_seasonality_factor
        * spectral_radius_alpha_over_beta(
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
        restart_block_progress_evaluation_budget(solver_settings, block_indices.len())
    })
    .sum::<usize>()
}

fn restart_block_progress_evaluation_budget(
    solver_settings: &CalibrationOptimizerSettings,
    active_block_dimension: usize,
) -> usize {
    if active_block_dimension == 0 {
        return 0;
    }

    let block_settings =
        joint_refinement_restart_block_powell_settings(solver_settings, active_block_dimension);

    debug_assert_eq!(block_settings.max_cycle_count, 1);

    // The restart progress counter increments on every objective callback. Under the
    // current one-cycle Powell configuration, the solver can consume the full block
    // budget across the coordinate directions before it checks the budget guard. Each
    // directional line search also probes the incumbent point once outside the
    // optimizer's internal `evaluation_count`, so the display budget must include
    // those bookkeeping callbacks to keep the terminal progress panel consistent.
    1usize.saturating_add(
        active_block_dimension
            .saturating_mul(block_settings.line_search_evaluation_cap.saturating_add(1)),
    )
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
        completed_outer_cycles: usize,
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
            Some(completed_outer_cycles),
            Some(progress_percent),
            Some(self.counters.active_restart_count.load(Ordering::Relaxed)),
            weighted_objective,
        );
    }

    fn emit_block_started(
        &self,
        parameter_block_label: &'static str,
        completed_outer_cycles: usize,
    ) {
        self.emit_progress(
            Some(parameter_block_label),
            Some(completed_outer_cycles),
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
        completed_outer_cycles: Option<usize>,
        evaluation_progress_percent: Option<usize>,
        active_restart_count: Option<usize>,
        current_weighted_objective: Option<f64>,
    ) {
        let (best_restart_id, best_weighted_objective) = self.counters.display_best_snapshot();
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
                current_parameter_block: parameter_block_label.map(str::to_string),
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
    context: &JointRefinementContext,
    active_restart_count: usize,
    total_evaluation_budget: usize,
    aggregate_progress_percent: usize,
) {
    let completed_units = progress_state.completed_units;
    let (best_restart_id, best_weighted_objective) = progress_counters.display_best_snapshot();
    let final_display_objective = compressed_final_objective(
        &restart_outcome.final_evaluation.raw_admissibility_metrics,
        joint_refinement_admissibility_thresholds(context),
    );

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
            current_weighted_objective: Some(final_display_objective),
            current_restart_best_weighted_objective: progress_counters
                .restart_best_weighted_objective(restart_outcome.restart_id.as_str()),
            current_candidate_assessment: None,
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

    fn record_display_best(&self, restart_id: &str, weighted_objective: f64) {
        let Ok(mut display_best_state) = self.display_best_state.lock() else {
            return;
        };

        match display_best_state
            .best_weighted_objective_by_restart_id
            .get(restart_id)
            .copied()
        {
            Some(current_best) if weighted_objective >= current_best => {}
            _ => {
                display_best_state
                    .best_weighted_objective_by_restart_id
                    .insert(restart_id.to_string(), weighted_objective);
            }
        }

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

    fn restart_best_weighted_objective(&self, restart_id: &str) -> Option<f64> {
        let Ok(display_best_state) = self.display_best_state.lock() else {
            return None;
        };

        display_best_state
            .best_weighted_objective_by_restart_id
            .get(restart_id)
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    use janiform_calibration_artifacts::{
        CalibrationOptimizerSettings, HawkesStageFittedParameters,
        JointRefinementStageFittedParameters, ReturnComponentsStageFittedParameters,
        SurpriseNewsDeterministicIntensityProfile,
    };
    use janiform_model_kernels::hawkes::{
        HawkesKernelLoadings, HawkesKernelParameters, HawkesStructuredAlphaParameterization,
        HawkesStructuredRhoParameterization,
    };
    use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;
    use janiform_model_kernels::news::{
        CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
        GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
        NewsComponentMarkScaleConfiguration, NewsProcessConfiguration,
        SharedNewsMarkLawConfiguration, SurpriseNewsArrivalConfiguration,
    };
    use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

    use super::{
        RestartCandidateProgress, RestartProgressCounters, RestartProgressState,
        precheck_hawkes_structural_domain, precheck_nig_return_composition_domain,
        precheck_restart_seed_vector_contract, restart_block_initial_objective_is_noncompetitive,
        restart_block_progress_evaluation_budget, runtime_branching_workload_is_infeasible,
        select_restart_stop_reason, summary_simulation_event_capacity_is_infeasible,
    };

    fn test_joint_fitted_parameters() -> JointRefinementStageFittedParameters {
        let hawkes_fitted_parameters = HawkesStageFittedParameters {
            kernel_parameters: HawkesKernelParameters::try_new(
                1.0,
                2.0,
                HawkesKernelLoadings {
                    plus_plus: 0.2,
                    plus_minus: 0.1,
                    minus_plus: 0.1,
                    minus_minus: 0.2,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.05,
                    plus_minus: 0.02,
                    minus_plus: 0.02,
                    minus_minus: 0.05,
                },
            )
            .expect("test Hawkes parameters should validate"),
            structured_alpha: HawkesStructuredAlphaParameterization::new(0.2, 0.1, 0.0, 0.0),
            structured_rho: HawkesStructuredRhoParameterization::new(0.05, 0.02, 0.0, 0.0),
            self_dominance_policy: None,
            stability_numeric_tolerance: 0.0,
        };

        let return_components_fitted_parameters = ReturnComponentsStageFittedParameters {
            nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration::try_new(
                2.0, -0.2, 0.5, 0.0, -0.1,
            )
            .expect("test NIG configuration should validate"),
            news_process_configuration: NewsProcessConfiguration::try_new(
                NewsArrivalConfiguration {
                    known: KnownNewsArrivalConfiguration {
                        scheduled_event_timestamps_utc_nanos: Vec::new(),
                    },
                    surprise: SurpriseNewsArrivalConfiguration {
                        baseline_intensity: 0.0,
                    },
                },
                SharedNewsMarkLawConfiguration {
                    gamma_severity_marks: GammaSeverityMarkConfiguration {
                        shape_k: 1.5,
                        scale_theta: 0.8,
                    },
                    double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
                        probability_positive: 0.5,
                        eta_plus: 1.2,
                        eta_minus: 1.2,
                    },
                    coupled_gap_marks: CoupledGapMarkConfiguration { sigma_g: 0.2 },
                },
                NewsComponentMarkScaleConfiguration {
                    activity_coupling_scale: 0.0,
                    return_jump_scale: 0.0,
                },
                NewsComponentMarkScaleConfiguration {
                    activity_coupling_scale: 0.0,
                    return_jump_scale: 0.0,
                },
            )
            .expect("test news process configuration should validate"),
            surprise_deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile {
                weekly_hour_utc_multipliers: vec![1.0; 24 * 7],
                profile_resolution_hours: 1,
                fit_policy: "test".to_string(),
                normalization_policy: "test".to_string(),
            },
            mean_reversion_fitted_parameters:
                MeanReversionFittedParameters::zero_kappa_flat_anchor_mean_reversion(),
        };

        JointRefinementStageFittedParameters {
            hawkes_fitted_parameters,
            return_components_fitted_parameters,
            pricing_block_fitted_parameters: None,
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

        assert_eq!(
            progress_update.best_restart_id.as_deref(),
            Some("halton_01")
        );
        assert_eq!(progress_update.best_weighted_objective, Some(4.0));
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
            current_restart_best_weighted_objective: None,
            finished: false,
        };

        restart_progress.record_evaluation("nig", 1, Some(3.0));

        let progress_update = progress_receiver.recv().unwrap();

        assert_eq!(progress_update.valid_completed_units, Some(1));
        assert_eq!(progress_update.cumulative_local_evaluation_count, Some(41));
    }

    #[test]
    fn restart_progress_counters_track_best_objective_by_restart() {
        let progress_counters = RestartProgressCounters::default();

        progress_counters.record_display_best("baseline", 10.0);
        progress_counters.record_display_best("halton_01", 4.0);
        progress_counters.record_display_best("baseline", 8.0);
        progress_counters.record_display_best("baseline", 12.0);

        assert_eq!(
            progress_counters.restart_best_weighted_objective("baseline"),
            Some(8.0)
        );
        assert_eq!(
            progress_counters.restart_best_weighted_objective("halton_01"),
            Some(4.0)
        );
    }

    #[test]
    fn restart_block_progress_budget_accounts_for_powell_callback_count() {
        let solver_settings =
            CalibrationOptimizerSettings::try_new("joint-test", 256, 4, 1.0e-6, 1.0e-6, 10.0)
                .expect("solver settings should validate");

        assert_eq!(
            restart_block_progress_evaluation_budget(&solver_settings, 4),
            25
        );
        assert_eq!(
            restart_block_progress_evaluation_budget(&solver_settings, 1),
            4
        );
        assert_eq!(
            restart_block_progress_evaluation_budget(&solver_settings, 0),
            0
        );
    }

    #[test]
    fn restart_stop_reason_ignores_deferred_dynamics_feasibility() {
        assert_eq!(
            select_restart_stop_reason("tolerance", "dynamics_infeasible", "tolerance"),
            "tolerance"
        );
    }

    #[test]
    fn restart_block_initial_objective_is_noncompetitive_after_threshold_best_exists() {
        let progress_counters = RestartProgressCounters::default();
        progress_counters.record_display_best("activity_aligned", 0.8);

        assert!(restart_block_initial_objective_is_noncompetitive(
            1.2,
            &progress_counters,
        ));
        assert!(!restart_block_initial_objective_is_noncompetitive(
            0.9,
            &progress_counters,
        ));
    }

    #[test]
    fn restart_block_initial_objective_keeps_search_before_threshold_best_exists() {
        let progress_counters = RestartProgressCounters::default();
        progress_counters.record_display_best("baseline", 1.2);

        assert!(!restart_block_initial_objective_is_noncompetitive(
            1.3,
            &progress_counters,
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
