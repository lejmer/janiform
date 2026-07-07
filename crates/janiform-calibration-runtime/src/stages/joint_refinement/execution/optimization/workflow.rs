use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CalibrationEffortOptions, CalibrationOptimizerSettings,
    JointRefinementStageCandidateEvaluationTimingDiagnostic, JointRefinementStageFittedParameters,
    STANDARD_CALIBRATION_EFFORT,
};
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameters, spectral_radius_alpha_over_beta,
};

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use crate::stages::return_components::ContinuousLikelihoodEvaluatorCounts;

use super::super::super::objective::{evaluate_joint_candidate, invalid_candidate_evaluation};
use super::super::super::setup::parameter_vector_to_fitted_parameters;
use super::super::selection::admissibility_threshold_from_sample_count;
use super::super::{
    JOINT_REFINEMENT_PARAMETER_FAMILY_COUNT, JointRefinementCandidateEvaluation,
    JointRefinementCandidateOutcome, JointRefinementContext, JointRefinementFidelityTier,
    JointRefinementResumeStore, option_pricing_execution_failure_message,
    simulation_infrastructure_failure_message,
};
use super::parameter_space::movable_parameter_count;

// The population-control budget scales with one baseline point plus two
// positive-spanning reference rounds and four exploratory candidates per round.
const POPULATION_CONTROL_REFERENCE_ROUND_COUNT: usize = 2;
const POPULATION_CONTROL_REFERENCE_EXPLORATORY_CANDIDATE_COUNT: usize = 4;

#[derive(Debug, Clone, Copy)]
pub(crate) struct JointRefinementBudgetSchedule {
    pub(crate) candidate_count: usize,
    pub(crate) search_depth_multiplier: usize,
    pub(crate) calibration_effort: CalibrationEffortOptions,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CandidateObjectiveResidual {
    pub(super) family_id: String,
    pub(super) residual_id: String,
    pub(super) objective_residual: f64,
}

#[derive(Debug, Clone)]
pub(super) struct CandidateObjectiveEvaluation {
    pub(super) objective: f64,
    pub(super) admissibility_warning_count: usize,
    pub(super) max_standardized_residual: Option<f64>,
    pub(super) exact_likelihood_evaluator_counts: Option<ContinuousLikelihoodEvaluatorCounts>,
    pub(super) residuals: Vec<CandidateObjectiveResidual>,
}

struct RefreshedFullVectorCandidate {
    parameter_vector: Vec<f64>,
    local_evaluation: JointRefinementCandidateEvaluation,
}

pub(crate) struct JointRefinementPopulationRefinementOutput {
    pub(crate) candidate_outcomes: Vec<JointRefinementCandidateOutcome>,
    pub(crate) evaluation_timing_diagnostics:
        Vec<JointRefinementStageCandidateEvaluationTimingDiagnostic>,
}

pub(crate) fn optimize_candidate_seeds(
    candidate_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_candidate_store: Option<&mut JointRefinementResumeStore>,
) -> JointRefinementPopulationRefinementOutput {
    debug_assert!(budget_schedule.candidate_count > 0);

    // Resume passes only pending candidate seeds while retaining the original search depth.
    let total_units = candidate_seeds.len();

    let planned_work_item_labels =
        super::population_control::population_control_planned_work_item_labels();

    let total_evaluation_budget =
        population_control_seed_evaluation_budget(context, solver_settings, budget_schedule);

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
                JointRefinementStageProgressPhase::PopulationRefinement,
                0,
                total_units,
            )
        },
    );

    let mut output = super::population_control::optimize_candidate_seeds_population_control(
        candidate_seeds,
        context,
        solver_settings,
        budget_schedule,
        progress_sender,
    );

    if let Some(resume_store) = completed_candidate_store {
        for outcome in &output.candidate_outcomes {
            resume_store.record_candidate_outcome(outcome);
        }
    }

    output.candidate_outcomes.sort_by_key(|outcome| {
        candidate_seeds
            .iter()
            .position(|(candidate_id, _)| *candidate_id == outcome.candidate_id)
            .unwrap_or(usize::MAX)
    });

    output
}

pub(crate) fn joint_refinement_budget_schedule(
    solver_settings: &CalibrationOptimizerSettings,
    candidate_count: usize,
    calibration_effort: Option<CalibrationEffortOptions>,
) -> JointRefinementBudgetSchedule {
    let base_search_depth_multiplier =
        joint_refinement_base_search_depth_multiplier(solver_settings);

    let calibration_effort = calibration_effort.unwrap_or(STANDARD_CALIBRATION_EFFORT);
    let candidate_count = candidate_count.max(1);

    let search_depth_multiplier = base_search_depth_multiplier
        .saturating_mul(base_search_depth_multiplier)
        .checked_div(candidate_count)
        .unwrap_or(1)
        .clamp(1, base_search_depth_multiplier);

    JointRefinementBudgetSchedule {
        candidate_count,
        search_depth_multiplier,
        calibration_effort,
    }
}

pub(crate) fn desired_joint_refinement_candidate_budget(
    candidate_budget_override: Option<usize>,
    calibration_effort: Option<CalibrationEffortOptions>,
) -> usize {
    candidate_budget_override
        .filter(|candidate_budget| *candidate_budget > 0)
        .unwrap_or_else(|| {
            calibration_effort
                .unwrap_or(STANDARD_CALIBRATION_EFFORT)
                .candidate_budget
                .max(1)
        })
}

fn joint_refinement_base_search_depth_multiplier(
    _solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    JOINT_REFINEMENT_PARAMETER_FAMILY_COUNT
}

fn precheck_candidate_seed(
    seed_vector: &[f64],
    context: &JointRefinementContext,
) -> Result<(), String> {
    precheck_candidate_seed_vector_contract(
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

fn precheck_candidate_seed_vector_contract(
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

    if context.runtime_interval_seasonality_factors.is_empty() {
        return Err("runtime_interval_seasonality_factors_empty".to_string());
    }

    for (interval_index, factor) in context
        .runtime_interval_seasonality_factors
        .iter()
        .enumerate()
    {
        if !factor.is_finite() || *factor < 0.0 {
            return Err(format!(
                "runtime_interval_seasonality_factor_invalid interval_index={interval_index} value={factor}"
            ));
        }

        let interval_activity_scale = modeled_open_event_rate_per_hour * *factor;
        if !interval_activity_scale.is_finite() || interval_activity_scale < 0.0 {
            return Err(format!(
                "runtime_interval_activity_scale_invalid interval_index={interval_index} value={interval_activity_scale}"
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

    let surprise_open_baseline_intensity = news_process.arrivals.surprise.open_baseline_intensity;

    let surprise_closed_baseline_intensity =
        news_process.arrivals.surprise.closed_baseline_intensity;

    if !surprise_open_baseline_intensity.is_finite() || surprise_open_baseline_intensity < 0.0 {
        return Err(format!(
            "surprise_open_baseline_intensity_invalid value={surprise_open_baseline_intensity}"
        ));
    }

    if !surprise_closed_baseline_intensity.is_finite() || surprise_closed_baseline_intensity < 0.0 {
        return Err(format!(
            "surprise_closed_baseline_intensity_invalid value={surprise_closed_baseline_intensity}"
        ));
    }

    crate::surprise_profile::validate_surprise_news_weekly_profile(
        &fitted_parameters
            .return_components_fitted_parameters
            .surprise_deterministic_intensity_profile,
    )
    .map_err(|source| format!("surprise_weekly_profile_invalid {source}"))?;

    let surprise_weekly_profile_multipliers = fitted_parameters
        .return_components_fitted_parameters
        .surprise_deterministic_intensity_profile
        .weekly_utc_profile_multipliers
        .as_slice();
    if surprise_weekly_profile_multipliers.is_empty() {
        return Err("surprise_weekly_multipliers_empty".to_string());
    }

    for (bucket_index, multiplier) in surprise_weekly_profile_multipliers.iter().enumerate() {
        if !multiplier.is_finite() || *multiplier < 0.0 {
            return Err(format!(
                "surprise_weekly_multiplier_invalid bucket_index={bucket_index} value={multiplier}"
            ));
        }

        let surprise_open_intensity = surprise_open_baseline_intensity * *multiplier;
        if !surprise_open_intensity.is_finite() || surprise_open_intensity < 0.0 {
            return Err(format!(
                "surprise_open_intensity_scale_invalid bucket_index={bucket_index} value={surprise_open_intensity}"
            ));
        }

        let surprise_closed_intensity = surprise_closed_baseline_intensity * *multiplier;
        if !surprise_closed_intensity.is_finite() || surprise_closed_intensity < 0.0 {
            return Err(format!(
                "surprise_closed_intensity_scale_invalid bucket_index={bucket_index} value={surprise_closed_intensity}"
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

fn build_precheck_rejected_candidate_outcome(
    candidate_id: &str,
    current_vector: Vec<f64>,
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    failure_reason: &str,
) -> JointRefinementCandidateOutcome {
    let invalid_reason = format!("candidate_seed_precheck_failed:{failure_reason}");

    let local_evaluation = invalid_candidate_evaluation(
        context,
        &context.baseline_hawkes_fitted_parameters,
        &context.baseline_return_components_fitted_parameters,
        solver_settings.constraint_penalty_weight,
        invalid_reason.as_str(),
    );

    JointRefinementCandidateOutcome {
        candidate_id: candidate_id.to_string(),
        parameter_vector: current_vector,
        final_evaluation: local_evaluation.clone(),
        local_evaluation,
        local_evaluation_count: 0,
        stop_reason: "candidate_seed_precheck_failed".to_string(),
    }
}

fn refreshed_full_vector_candidate(
    previous_vector: &[f64],
    candidate_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> RefreshedFullVectorCandidate {
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
        return RefreshedFullVectorCandidate {
            parameter_vector: previous_vector.to_vec(),
            local_evaluation: previous_evaluation,
        };
    }

    if previous_evaluation.invalid_reason.is_some() {
        return RefreshedFullVectorCandidate {
            parameter_vector: candidate_vector.to_vec(),
            local_evaluation: candidate_evaluation,
        };
    }

    let candidate_global_loss = candidate_evaluation.breakdown.global_loss;
    if !candidate_global_loss.is_finite() {
        return RefreshedFullVectorCandidate {
            parameter_vector: previous_vector.to_vec(),
            local_evaluation: previous_evaluation,
        };
    }

    let previous_global_loss = previous_evaluation.breakdown.global_loss;
    if previous_global_loss.is_finite() && candidate_global_loss > previous_global_loss {
        return RefreshedFullVectorCandidate {
            parameter_vector: previous_vector.to_vec(),
            local_evaluation: previous_evaluation,
        };
    }

    RefreshedFullVectorCandidate {
        parameter_vector: candidate_vector.to_vec(),
        local_evaluation: candidate_evaluation,
    }
}

pub(super) fn evaluate_finalized_candidate(
    candidate_id: &str,
    seed_vector: &[f64],
    candidate_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    evaluation_count: usize,
    stop_reason: &str,
) -> JointRefinementCandidateOutcome {
    if let Err(precheck_failure) = precheck_candidate_seed(candidate_vector, context) {
        return build_precheck_rejected_candidate_outcome(
            candidate_id,
            candidate_vector.to_vec(),
            context,
            solver_settings,
            precheck_failure.as_str(),
        );
    }

    let refreshed_candidate =
        refreshed_full_vector_candidate(seed_vector, candidate_vector, context, solver_settings);
    let refreshed_vector = refreshed_candidate.parameter_vector;
    let local_evaluation = refreshed_candidate.local_evaluation;

    if local_evaluation.invalid_reason.is_some()
        || joint_refinement_has_fatal_execution_failure(context)
    {
        return JointRefinementCandidateOutcome {
            candidate_id: candidate_id.to_string(),
            parameter_vector: refreshed_vector,
            // No final pass is valid after a local failure, so the final slot mirrors local.
            final_evaluation: local_evaluation.clone(),
            local_evaluation,
            local_evaluation_count: evaluation_count,
            stop_reason: stop_reason.to_string(),
        };
    }

    let final_evaluation = evaluate_joint_candidate(
        refreshed_vector.as_slice(),
        context,
        JointRefinementFidelityTier::Final,
        solver_settings,
    );

    JointRefinementCandidateOutcome {
        candidate_id: candidate_id.to_string(),
        parameter_vector: refreshed_vector,
        local_evaluation,
        final_evaluation,
        local_evaluation_count: evaluation_count,
        stop_reason: stop_reason.to_string(),
    }
}

pub(super) fn evaluate_prechecked_full_parameter_vector_objective(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> Option<CandidateObjectiveEvaluation> {
    if precheck_candidate_seed(parameter_vector, context).is_err() {
        return None;
    }

    evaluate_full_parameter_vector_objective(parameter_vector, context, solver_settings)
}

fn evaluate_full_parameter_vector_objective(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> Option<CandidateObjectiveEvaluation> {
    let candidate_evaluation = evaluate_joint_candidate(
        parameter_vector,
        context,
        full_vector_search_fidelity_tier(),
        solver_settings,
    );

    candidate_evaluation
        .invalid_reason
        .is_none()
        .then_some(CandidateObjectiveEvaluation {
            objective: candidate_evaluation.breakdown.global_loss,
            admissibility_warning_count: candidate_evaluation
                .breakdown
                .admissibility
                .warnings
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
            residuals: candidate_objective_residuals(&candidate_evaluation.breakdown),
        })
}

fn candidate_objective_residuals(
    breakdown: &janiform_calibration_artifacts::JointRefinementStageObjectiveBreakdown,
) -> Vec<CandidateObjectiveResidual> {
    breakdown
        .family_losses
        .iter()
        .flat_map(|family| {
            if family.unidentified
                || family.rank == 0
                || family.loss < 0.0
                || family.whitened_coordinates.is_empty()
            {
                return Vec::new();
            }

            let rank_scale = (family.rank as f64).sqrt();
            let coordinate_ids = breakdown
                .term_diagnostics
                .iter()
                .filter(|term| term.family_id == family.family_id)
                .map(|term| term.term_id.as_str())
                .collect::<Vec<_>>();

            family
                .whitened_coordinates
                .iter()
                .enumerate()
                .filter_map(|(coordinate_position, whitened_coordinate)| {
                    let objective_residual = whitened_coordinate / rank_scale;

                    objective_residual
                        .is_finite()
                        .then(|| CandidateObjectiveResidual {
                            family_id: family.family_id.clone(),
                            residual_id: coordinate_ids
                                .get(coordinate_position)
                                .map(|coordinate_id| (*coordinate_id).to_string())
                                .unwrap_or_else(|| {
                                    format!(
                                        "{}.whitened_coordinate_{coordinate_position:02}",
                                        family.family_id
                                    )
                                }),
                            objective_residual,
                        })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn full_vector_search_fidelity_tier() -> JointRefinementFidelityTier {
    JointRefinementFidelityTier::CoarseLocal
}

fn full_vector_refresh_fidelity_tier() -> JointRefinementFidelityTier {
    JointRefinementFidelityTier::Local
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

pub(super) fn population_control_seed_evaluation_budget(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    budget_schedule: JointRefinementBudgetSchedule,
) -> usize {
    budget_schedule
        .search_depth_multiplier
        .max(1)
        .saturating_mul(
            population_control_reference_evaluation_budget(context, solver_settings).max(1),
        )
}

pub(super) fn population_control_reference_evaluation_budget(
    context: &JointRefinementContext,
    _solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    population_control_candidate_reference_evaluation_budget(
        context.lower_bounds.as_slice(),
        context.upper_bounds.as_slice(),
    )
}

fn population_control_candidate_reference_evaluation_budget(
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> usize {
    let active_parameter_count = movable_parameter_count(lower_bounds, upper_bounds);

    let positive_spanning_candidate_count = active_parameter_count
        .saturating_mul(2)
        .saturating_add(usize::from(active_parameter_count > 1).saturating_mul(2));

    let reference_round_candidate_count = positive_spanning_candidate_count
        .saturating_add(POPULATION_CONTROL_REFERENCE_EXPLORATORY_CANDIDATE_COUNT)
        .max(1);

    1usize.saturating_add(
        reference_round_candidate_count
            .max(1)
            .saturating_mul(POPULATION_CONTROL_REFERENCE_ROUND_COUNT),
    )
}

pub(super) fn emit_joint_refinement_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_update: JointRefinementStageProgressUpdate,
) {
    if let Some(progress_sender) = progress_sender {
        let _ = progress_sender.send(progress_update);
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::{
        CalibrationLossAdmissibility, CalibrationLossFamilyDiagnostic,
        CalibrationLossTermDiagnostic, CalibrationLossTermKind,
        JointRefinementStageObjectiveBreakdown,
    };

    use super::{
        candidate_objective_residuals, full_vector_refresh_fidelity_tier,
        full_vector_search_fidelity_tier, population_control_candidate_reference_evaluation_budget,
        precheck_candidate_seed_vector_contract, precheck_hawkes_structural_domain,
        precheck_nig_return_composition_domain, runtime_branching_workload_is_infeasible,
        summary_simulation_event_capacity_is_infeasible,
    };
    use crate::stages::joint_refinement::execution::{
        JointRefinementFidelityTier, test_joint_fitted_parameters,
    };

    #[test]
    fn candidate_objective_residuals_use_family_loss_geometry() {
        let breakdown = JointRefinementStageObjectiveBreakdown {
            global_loss: 6.5,
            family_losses: vec![
                CalibrationLossFamilyDiagnostic {
                    family_id: "return_shape".to_string(),
                    coordinate_count: 2,
                    rank: 2,
                    whitened_coordinates: vec![3.0, 0.0],
                    loss: 9.0,
                    max_standardized_residual: 4.0,
                    worst_term_id: Some("return_shape.q01".to_string()),
                    unidentified: false,
                },
                CalibrationLossFamilyDiagnostic {
                    family_id: "activity_shape".to_string(),
                    coordinate_count: 2,
                    rank: 2,
                    whitened_coordinates: vec![0.0, 2.0],
                    loss: 4.0,
                    max_standardized_residual: 3.0,
                    worst_term_id: Some("activity_shape.hour_12".to_string()),
                    unidentified: false,
                },
                CalibrationLossFamilyDiagnostic {
                    family_id: "unidentified".to_string(),
                    coordinate_count: 2,
                    rank: 0,
                    whitened_coordinates: Vec::new(),
                    loss: 25.0,
                    max_standardized_residual: 5.0,
                    worst_term_id: None,
                    unidentified: true,
                },
            ],
            term_diagnostics: vec![
                CalibrationLossTermDiagnostic {
                    term_id: "return_shape.q01".to_string(),
                    family_id: "return_shape".to_string(),
                    term_kind: CalibrationLossTermKind::DiagnosticCoordinate,
                    observed_value: Some(0.0),
                    model_value: Some(1.0),
                    observed_standard_error: None,
                    model_standard_error: None,
                    uncertainty_scale: Some(1.0),
                    standardized_residual: Some(100.0),
                    covariance_diagonal: Some(1.0),
                    loss: 1_666.6666666666667,
                },
                CalibrationLossTermDiagnostic {
                    term_id: "return_shape.q99".to_string(),
                    family_id: "return_shape".to_string(),
                    term_kind: CalibrationLossTermKind::DiagnosticCoordinate,
                    observed_value: Some(0.0),
                    model_value: Some(1.0),
                    observed_standard_error: None,
                    model_standard_error: None,
                    uncertainty_scale: Some(1.0),
                    standardized_residual: Some(0.0),
                    covariance_diagonal: Some(1.0),
                    loss: 0.0,
                },
                CalibrationLossTermDiagnostic {
                    term_id: "activity_shape.hour_00".to_string(),
                    family_id: "activity_shape".to_string(),
                    term_kind: CalibrationLossTermKind::DiagnosticCoordinate,
                    observed_value: Some(0.0),
                    model_value: Some(1.0),
                    observed_standard_error: None,
                    model_standard_error: None,
                    uncertainty_scale: Some(1.0),
                    standardized_residual: Some(0.0),
                    covariance_diagonal: Some(1.0),
                    loss: 0.0,
                },
                CalibrationLossTermDiagnostic {
                    term_id: "activity_shape.hour_12".to_string(),
                    family_id: "activity_shape".to_string(),
                    term_kind: CalibrationLossTermKind::DiagnosticCoordinate,
                    observed_value: Some(0.0),
                    model_value: Some(1.0),
                    observed_standard_error: None,
                    model_standard_error: None,
                    uncertainty_scale: Some(1.0),
                    standardized_residual: Some(3.0),
                    covariance_diagonal: Some(1.0),
                    loss: 9.0,
                },
            ],
            domain_margin_diagnostics: Vec::new(),
            admissibility: CalibrationLossAdmissibility {
                admissible: true,
                max_standardized_residual: 100.0,
                excess_loss: 0.0,
                warnings: Vec::new(),
            },
        };

        assert_eq!(
            candidate_objective_residuals(&breakdown),
            vec![
                super::CandidateObjectiveResidual {
                    family_id: "return_shape".to_string(),
                    residual_id: "return_shape.q01".to_string(),
                    objective_residual: 3.0 / 2.0_f64.sqrt(),
                },
                super::CandidateObjectiveResidual {
                    family_id: "return_shape".to_string(),
                    residual_id: "return_shape.q99".to_string(),
                    objective_residual: 0.0,
                },
                super::CandidateObjectiveResidual {
                    family_id: "activity_shape".to_string(),
                    residual_id: "activity_shape.hour_00".to_string(),
                    objective_residual: 0.0,
                },
                super::CandidateObjectiveResidual {
                    family_id: "activity_shape".to_string(),
                    residual_id: "activity_shape.hour_12".to_string(),
                    objective_residual: 2.0 / 2.0_f64.sqrt(),
                },
            ],
        );
    }

    #[test]
    fn population_control_reference_budget_scales_with_parameter_dimension() {
        let four_dimensional_lower_bounds = vec![-1.0, -1.0, -1.0, -1.0];
        let four_dimensional_upper_bounds = vec![1.0, 1.0, 1.0, 1.0];

        let four_dimensional_budget = population_control_candidate_reference_evaluation_budget(
            four_dimensional_lower_bounds.as_slice(),
            four_dimensional_upper_bounds.as_slice(),
        );

        let one_movable_dimension_lower_bounds = vec![-1.0, 0.0, 0.0];
        let one_movable_dimension_upper_bounds = vec![1.0, 0.0, 0.0];

        let one_dimensional_budget = population_control_candidate_reference_evaluation_budget(
            one_movable_dimension_lower_bounds.as_slice(),
            one_movable_dimension_upper_bounds.as_slice(),
        );

        assert_eq!(four_dimensional_budget, 29);
        assert_eq!(one_dimensional_budget, 13);
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
    fn candidate_full_vector_objectives_use_coarse_search_fidelity() {
        assert_eq!(
            full_vector_search_fidelity_tier(),
            JointRefinementFidelityTier::CoarseLocal
        );
    }

    #[test]
    fn candidate_full_vector_refresh_uses_seeded_local_fidelity() {
        assert_eq!(
            full_vector_refresh_fidelity_tier(),
            JointRefinementFidelityTier::Local
        );
    }

    #[test]
    fn candidate_seed_vector_precheck_rejects_non_finite_parameters() {
        let error =
            precheck_candidate_seed_vector_contract(&[0.0, f64::NAN], &[-1.0, -1.0], &[1.0, 1.0])
                .expect_err("non-finite seed parameter should be rejected");

        assert!(error.contains("non_finite_parameter_or_bound"));
    }

    #[test]
    fn candidate_seed_vector_precheck_rejects_out_of_bounds_parameters() {
        let error =
            precheck_candidate_seed_vector_contract(&[0.0, 2.0], &[-1.0, -1.0], &[1.0, 1.0])
                .expect_err("out-of-bounds seed parameter should be rejected");

        assert!(error.contains("parameter_out_of_bounds"));
    }

    #[test]
    fn candidate_seed_model_domain_prechecks_accept_valid_parameters() {
        let fitted_parameters = test_joint_fitted_parameters();

        precheck_hawkes_structural_domain(&fitted_parameters)
            .expect("valid Hawkes parameters should pass precheck");
        precheck_nig_return_composition_domain(&fitted_parameters)
            .expect("valid NIG return-composition domain should pass precheck");
    }

    #[test]
    fn candidate_seed_hawkes_precheck_rejects_structural_domain_violations() {
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
    fn candidate_seed_nig_precheck_rejects_return_composition_domain_violations() {
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
