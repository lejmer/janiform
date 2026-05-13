#[cfg(test)]
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::time::Instant;

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use janiform_model_kernels::hawkes::{HawkesKernelLoadings, HawkesKernelParameters};
use janiform_model_kernels::news::SurpriseNewsArrivalConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};

use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals;
use crate::stages::return_components::{
    NigShapeSearchPoint, ReturnComponentsStageCandidateDiagnostic,
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
    ReturnComponentsStageContinuousReturnCalibrationSample, ReturnComponentsStageFittedPayload,
    build_nig_moment_seed_configurations, nig_configuration_from_search_point,
    nig_search_bounds_for_calibration_sample, nig_search_point_from_configuration,
    rescale_continuous_return_calibration_sample_business_time_scales,
};

use super::contracts::{
    JointRefinementStageHawkesInput, JointRefinementStageObjectiveTermDiagnostic,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use super::execution::{
    JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND,
    JOINT_REFINEMENT_THRESHOLD_RELATIVE_ADMISSIBILITY_SCORE, JointRefinementActiveParameterLayout,
    JointRefinementContext, JointRefinementSeedSummary, JointRefinementSeedSummaryCacheKey,
    build_halton_restart_id, clamp_in_place, deduplicate_restart_seed_candidates,
    halton_parameter_vector, joint_refinement_has_parallel_worker_capacity,
    precheck_seed_summary_workload, with_joint_refinement_thread_pool,
};
use super::objective::evaluate_joint_fitted_candidate_restart_admission_score;
use super::worker_runtime::JointRefinementSeedSummaryAcquisitionError;
use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageFittedParameters,
    JointRefinementStagePricingBlockFittedParameters, ReturnComponentsStageFittedParameters,
};

const RETURN_SCALE_REALIZED_VARIANCE_15M_METRIC_ID: &str = "joint.return_scale";
const RETURN_SCALE_MEAN_ABSOLUTE_RETURN_15M_METRIC_ID: &str =
    "joint.return_scale.mean_absolute_return_15m";
const RETURN_SCALE_TICK_REALIZED_VARIANCE_METRIC_ID: &str =
    "joint.return_scale.tick_realized_variance";
const RETURN_SCALE_TICK_MEAN_ABSOLUTE_RETURN_METRIC_ID: &str =
    "joint.return_scale.tick_mean_absolute_return";
const RETURN_SCALE_ALIGNMENT_METRICS: [ReturnScaleAlignmentMetric; 4] = [
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_REALIZED_VARIANCE_15M_METRIC_ID,
        seed_label: "realized_variance_15m",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Quadratic,
    },
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_MEAN_ABSOLUTE_RETURN_15M_METRIC_ID,
        seed_label: "mean_absolute_return_15m",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Linear,
    },
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_TICK_REALIZED_VARIANCE_METRIC_ID,
        seed_label: "tick_realized_variance",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Quadratic,
    },
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_TICK_MEAN_ABSOLUTE_RETURN_METRIC_ID,
        seed_label: "tick_mean_absolute_return",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Linear,
    },
];

#[derive(Debug, Clone, Copy)]
struct ReturnScaleAlignmentMetric {
    metric_id: &'static str,
    seed_label: &'static str,
    scale_mapping: ReturnScaleAlignmentScaleMapping,
}

#[derive(Debug, Clone, Copy)]
enum ReturnScaleAlignmentScaleMapping {
    Linear,
    Quadratic,
}

#[derive(Debug, Clone, Copy)]
struct ReturnScaleAlignmentLogDeltaShift {
    seed_label: &'static str,
    log_delta_shift: f64,
}

pub(super) fn parameter_vector_to_fitted_parameters(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Option<JointRefinementStageFittedParameters> {
    let option_pricing_only = context.option_pricing_refinement_enabled;

    let mut structured_alpha = context.baseline_hawkes_fitted_parameters.structured_alpha;

    let mut news_process_configuration = context
        .baseline_return_components_fitted_parameters
        .news_process_configuration
        .clone();

    let hawkes_fitted_parameters = if option_pricing_only {
        context.baseline_hawkes_fitted_parameters.clone()
    } else {
        let lambda_scale =
            parameter_vector[context.active_parameter_layout.log_lambda_scale_index].exp();

        let beta_scale =
            parameter_vector[context.active_parameter_layout.log_beta_scale_index].exp();

        let self_excitation_scale = parameter_vector[context
            .active_parameter_layout
            .log_self_excitation_scale_index]
            .exp();

        let cross_excitation_scale = parameter_vector[context
            .active_parameter_layout
            .log_cross_excitation_scale_index]
            .exp();

        structured_alpha.a_self *= self_excitation_scale;
        structured_alpha.delta_self *= self_excitation_scale;
        structured_alpha.a_cross *= cross_excitation_scale;
        structured_alpha.delta_cross *= cross_excitation_scale;

        let alpha_loadings =
            HawkesKernelLoadings::try_from_structured_alpha(structured_alpha).ok()?;

        let rho_loadings = HawkesKernelLoadings::try_from_structured_rho(
            context.baseline_hawkes_fitted_parameters.structured_rho,
            context
                .baseline_hawkes_fitted_parameters
                .self_dominance_policy,
        )
        .ok()?;

        let lambda_bar = context
            .baseline_hawkes_fitted_parameters
            .kernel_parameters
            .lambda_bar
            * lambda_scale;

        let beta = context
            .baseline_hawkes_fitted_parameters
            .kernel_parameters
            .beta
            * beta_scale;

        let kernel_parameters =
            HawkesKernelParameters::try_new(lambda_bar, beta, alpha_loadings, rho_loadings).ok()?;

        janiform_calibration_artifacts::HawkesStageFittedParameters {
            kernel_parameters,
            structured_alpha,
            structured_rho: context.baseline_hawkes_fitted_parameters.structured_rho,
            self_dominance_policy: context
                .baseline_hawkes_fitted_parameters
                .self_dominance_policy,
            stability_numeric_tolerance: context
                .baseline_hawkes_fitted_parameters
                .stability_numeric_tolerance,
        }
    };

    let modeled_open_event_rate_per_hour =
        crate::stages::hawkes::stationary_open_event_rate_per_hour(&hawkes_fitted_parameters)?;

    let aggregated_15m_sample = build_continuous_calibration_sample_from_aggregated_intervals(
        context.observed_data.observed_15m_intervals.as_slice(),
        modeled_open_event_rate_per_hour,
        context.observed_data.observed_open_event_rate_per_hour,
        context.observed_data.fallback_sell_share_proxy,
    );

    let nig_search_point = NigShapeSearchPoint {
        log_alpha: parameter_vector[context.active_parameter_layout.nig_block[0]],
        buy_skew_coordinate: parameter_vector[context.active_parameter_layout.nig_block[1]],
        sell_relative_coordinate: parameter_vector[context.active_parameter_layout.nig_block[2]],
        log_delta_offset: parameter_vector[context.active_parameter_layout.nig_block[3]],
    };

    let nig_shared_plus_shift_configuration =
        nig_configuration_from_search_point(&aggregated_15m_sample, nig_search_point)?;

    let (return_components_fitted_parameters, pricing_block_fitted_parameters) =
        if option_pricing_only {
            (
                context.baseline_return_components_fitted_parameters.clone(),
                Some(JointRefinementStagePricingBlockFittedParameters {
                    nig_shared_plus_shift_configuration,
                }),
            )
        } else {
            if let Some(index) = context.active_parameter_layout.known_activity_scale_index {
                news_process_configuration
                    .known_component_mark_scales
                    .activity_coupling_scale *= parameter_vector[index].exp();
            }

            if let Some(index) = context
                .active_parameter_layout
                .surprise_activity_scale_index
            {
                news_process_configuration
                    .surprise_component_mark_scales
                    .activity_coupling_scale *= parameter_vector[index].exp();
            }

            if let Some(index) = context
                .active_parameter_layout
                .surprise_baseline_scale_index
            {
                news_process_configuration.arrivals.surprise = SurpriseNewsArrivalConfiguration {
                    baseline_intensity: news_process_configuration
                        .arrivals
                        .surprise
                        .baseline_intensity
                        * parameter_vector[index].exp(),
                };
            }

            let return_components_fitted_parameters = ReturnComponentsStageFittedParameters {
                nig_shared_plus_shift_configuration,
                news_process_configuration,
                surprise_deterministic_intensity_profile: context
                    .baseline_return_components_fitted_parameters
                    .surprise_deterministic_intensity_profile
                    .clone(),
                return_mark_hour_of_week_scale_profile: context
                    .baseline_return_components_fitted_parameters
                    .return_mark_hour_of_week_scale_profile
                    .clone(),
                tick_grid_observation_model: context
                    .baseline_return_components_fitted_parameters
                    .tick_grid_observation_model
                    .clone(),
                mean_reversion_fitted_parameters: context
                    .baseline_mean_reversion_fitted_parameters
                    .clone(),
            };

            return_components_fitted_parameters
                .news_process_configuration
                .validate()
                .ok()?;

            (return_components_fitted_parameters, None)
        };

    Some(JointRefinementStageFittedParameters {
        hawkes_fitted_parameters,
        return_components_fitted_parameters,
        pricing_block_fitted_parameters,
    })
}

pub(super) fn build_baseline_parameter_vector(
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    _hawkes_fitted_parameters: &janiform_calibration_artifacts::HawkesStageFittedParameters,
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> Vec<f64> {
    let mut parameter_vector = vec![0.0; active_parameter_layout.vector_length()];
    parameter_vector[active_parameter_layout.log_lambda_scale_index] = 0.0;
    parameter_vector[active_parameter_layout.log_beta_scale_index] = 0.0;
    parameter_vector[active_parameter_layout.log_self_excitation_scale_index] = 0.0;
    parameter_vector[active_parameter_layout.log_cross_excitation_scale_index] = 0.0;

    if let Some(search_point) = nig_search_point_from_configuration(
        baseline_15m_calibration_sample,
        return_components_fitted_parameters.nig_shared_plus_shift_configuration,
    ) {
        parameter_vector[active_parameter_layout.nig_block[0]] = search_point.log_alpha;
        parameter_vector[active_parameter_layout.nig_block[1]] = search_point.buy_skew_coordinate;
        parameter_vector[active_parameter_layout.nig_block[2]] =
            search_point.sell_relative_coordinate;
        parameter_vector[active_parameter_layout.nig_block[3]] = search_point.log_delta_offset;
    }

    if let Some(index) = active_parameter_layout.known_activity_scale_index {
        parameter_vector[index] = 0.0;
    }

    if let Some(index) = active_parameter_layout.surprise_activity_scale_index {
        parameter_vector[index] = 0.0;
    }

    if let Some(index) = active_parameter_layout.surprise_baseline_scale_index {
        parameter_vector[index] = 0.0;
    }

    parameter_vector
}

pub(super) fn build_active_parameter_layout(
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
) -> JointRefinementActiveParameterLayout {
    let mut next_index = 0usize;

    let log_lambda_scale_index = next_index;
    next_index += 1;

    let log_beta_scale_index = next_index;
    next_index += 1;

    let log_self_excitation_scale_index = next_index;
    next_index += 1;

    let log_cross_excitation_scale_index = next_index;
    next_index += 1;

    let hawkes_block = vec![
        log_lambda_scale_index,
        log_beta_scale_index,
        log_self_excitation_scale_index,
        log_cross_excitation_scale_index,
    ];

    let nig_block = vec![next_index, next_index + 1, next_index + 2, next_index + 3];
    next_index += 4;

    let known_activity_scale_index = (return_components_fitted_parameters
        .news_process_configuration
        .known_component_mark_scales
        .activity_coupling_scale
        > 0.0)
        .then_some(next_index);
    if known_activity_scale_index.is_some() {
        next_index += 1;
    }

    let surprise_activity_scale_index = (return_components_fitted_parameters
        .news_process_configuration
        .surprise_component_mark_scales
        .activity_coupling_scale
        > 0.0)
        .then_some(next_index);
    if surprise_activity_scale_index.is_some() {
        next_index += 1;
    }

    let surprise_baseline_scale_index = (return_components_fitted_parameters
        .news_process_configuration
        .arrivals
        .surprise
        .baseline_intensity
        > 0.0)
        .then_some(next_index);

    let mut coupling_block = Vec::new();
    if let Some(index) = known_activity_scale_index {
        coupling_block.push(index);
    }

    if let Some(index) = surprise_activity_scale_index {
        coupling_block.push(index);
    }

    if let Some(index) = surprise_baseline_scale_index {
        coupling_block.push(index);
    }

    JointRefinementActiveParameterLayout {
        hawkes_block,
        nig_block,
        coupling_block,
        log_lambda_scale_index,
        log_beta_scale_index,
        log_self_excitation_scale_index,
        log_cross_excitation_scale_index,
        known_activity_scale_index,
        surprise_activity_scale_index,
        surprise_baseline_scale_index,
    }
}

impl JointRefinementActiveParameterLayout {
    pub(super) fn vector_length(&self) -> usize {
        self.coupling_block
            .iter()
            .copied()
            .chain(self.nig_block.iter().copied())
            .chain(self.hawkes_block.iter().copied())
            .max()
            .map(|index| index + 1)
            .unwrap_or(0)
    }
}

pub(super) fn build_parameter_bounds(
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    baseline_parameter_vector: &[f64],
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> (Vec<f64>, Vec<f64>) {
    let mut lower_bounds = vec![0.0; baseline_parameter_vector.len()];
    let mut upper_bounds = vec![0.0; baseline_parameter_vector.len()];

    for index in &active_parameter_layout.hawkes_block {
        lower_bounds[*index] = -JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND;
        upper_bounds[*index] = JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND;
    }

    let (nig_lower_bounds, nig_upper_bounds) =
        nig_search_bounds_for_calibration_sample(baseline_15m_calibration_sample, 128);
    for (coordinate_position, parameter_index) in
        active_parameter_layout.nig_block.iter().enumerate()
    {
        lower_bounds[*parameter_index] = nig_lower_bounds[coordinate_position];
        upper_bounds[*parameter_index] = nig_upper_bounds[coordinate_position];
    }

    for index in &active_parameter_layout.coupling_block {
        lower_bounds[*index] = -JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND;
        upper_bounds[*index] = JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND;
    }

    (lower_bounds, upper_bounds)
}

pub(super) fn build_restart_seeds(input: RestartSeedsBuildInput<'_>) -> Vec<(String, Vec<f64>)> {
    let RestartSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
        joint_context,
        solver_settings,
        restart_candidate_budget,
        progress_sender,
    } = input;

    emit_restart_seed_generation_progress(
        progress_sender,
        0,
        2,
        Some("core_restart_seed_generation"),
    );

    let mut restart_candidates = build_core_restart_seed_candidates(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
    );

    emit_restart_seed_generation_progress(
        progress_sender,
        1,
        2,
        Some("tick_moment_seed_generation"),
    );

    restart_candidates.extend(joint_tick_moment_restart_seeds(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        activity_log_shift,
        joint_context,
        solver_settings,
    ));

    emit_restart_seed_generation_progress(
        progress_sender,
        2,
        2,
        Some("restart_seed_candidates_built"),
    );

    let restart_seed_score = |_: &str, restart_seed: &[f64]| {
        screened_exploratory_restart_seed_score(restart_seed, joint_context)
    };

    finalize_restart_seed_candidates(RestartSeedCandidateFinalizationInput {
        restart_candidates,
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        restart_candidate_budget,
        restart_seed_score: &restart_seed_score,
        joint_context: Some(joint_context),
        progress_sender,
    })
}

pub(super) struct RestartSeedsBuildInput<'a> {
    pub(super) baseline_parameter_vector: &'a [f64],
    pub(super) active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    pub(super) lower_bounds: &'a [f64],
    pub(super) upper_bounds: &'a [f64],
    pub(super) baseline_15m_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(super) return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    pub(super) baseline_final_objective_terms: &'a [JointRefinementStageObjectiveTermDiagnostic],
    pub(super) activity_log_shift: f64,
    pub(super) joint_context: &'a JointRefinementContext,
    pub(super) solver_settings: &'a CalibrationOptimizerSettings,
    pub(super) restart_candidate_budget: usize,
    pub(super) progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
}

#[cfg(test)]
pub(super) fn build_restart_seeds_with_halton_acceptance(
    input: RestartSeedsWithHaltonAcceptanceBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let RestartSeedsWithHaltonAcceptanceBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
        restart_candidate_budget,
        halton_seed_is_accepted,
    } = input;

    let halton_seed_is_accepted = Mutex::new(halton_seed_is_accepted);

    let restart_candidates = build_core_restart_seed_candidates(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
    );

    let restart_seed_score = |restart_id: &str, restart_seed: &[f64]| {
        if !restart_id.starts_with("halton_") {
            return Some(0.0);
        }

        let Ok(mut halton_seed_is_accepted) = halton_seed_is_accepted.lock() else {
            return None;
        };

        halton_seed_is_accepted(restart_seed).then_some(0.0)
    };

    finalize_restart_seed_candidates(RestartSeedCandidateFinalizationInput {
        restart_candidates,
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        restart_candidate_budget,
        restart_seed_score: &restart_seed_score,
        joint_context: None,
        progress_sender: None,
    })
}

#[cfg(test)]
type HaltonSeedAcceptancePredicate<'a> = Box<dyn FnMut(&[f64]) -> bool + Send + 'a>;

#[cfg(test)]
pub(super) struct RestartSeedsWithHaltonAcceptanceBuildInput<'a> {
    pub(super) baseline_parameter_vector: &'a [f64],
    pub(super) active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    pub(super) lower_bounds: &'a [f64],
    pub(super) upper_bounds: &'a [f64],
    pub(super) baseline_15m_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(super) return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    pub(super) baseline_final_objective_terms: &'a [JointRefinementStageObjectiveTermDiagnostic],
    pub(super) activity_log_shift: f64,
    pub(super) restart_candidate_budget: usize,
    pub(super) halton_seed_is_accepted: HaltonSeedAcceptancePredicate<'a>,
}

fn build_core_restart_seed_candidates(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    return_component_basin_diagnostics: &[ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    baseline_final_objective_terms: &[JointRefinementStageObjectiveTermDiagnostic],
    activity_log_shift: f64,
) -> Vec<(String, Vec<f64>)> {
    let mut baseline = baseline_parameter_vector.to_vec();

    clamp_in_place(baseline.as_mut_slice(), lower_bounds, upper_bounds);

    let mut activity_aligned = baseline_parameter_vector.to_vec();

    apply_activity_alignment(
        activity_aligned.as_mut_slice(),
        active_parameter_layout,
        activity_log_shift,
    );

    clamp_in_place(activity_aligned.as_mut_slice(), lower_bounds, upper_bounds);

    let mut midpoint = lower_bounds
        .iter()
        .zip(upper_bounds.iter())
        .map(|(lower_bound, upper_bound)| 0.5 * (lower_bound + upper_bound))
        .collect::<Vec<_>>();

    clamp_in_place(midpoint.as_mut_slice(), lower_bounds, upper_bounds);

    let mut restart_candidates = vec![("baseline".to_string(), baseline)];

    restart_candidates.push(("activity_aligned".to_string(), activity_aligned));

    restart_candidates.extend(joint_return_scale_alignment_restart_seeds(
        JointReturnScaleAlignmentSeedsBuildInput {
            baseline_parameter_vector,
            active_parameter_layout,
            lower_bounds,
            upper_bounds,
            baseline_final_objective_terms,
            activity_log_shift,
        },
    ));

    restart_candidates.push(("midpoint".to_string(), midpoint));

    restart_candidates.extend(joint_nig_basin_restart_seeds(
        JointNigRestartSeedsBuildInput {
            baseline_parameter_vector,
            active_parameter_layout,
            lower_bounds,
            upper_bounds,
            baseline_15m_calibration_sample,
            return_component_basin_diagnostics,
            activity_log_shift,
            maximum_seed_count: 4,
        },
    ));

    restart_candidates.extend(joint_nig_alpha_delta_restart_seeds(
        JointNigRestartSeedsBuildInput {
            baseline_parameter_vector,
            active_parameter_layout,
            lower_bounds,
            upper_bounds,
            baseline_15m_calibration_sample,
            return_component_basin_diagnostics,
            activity_log_shift,
            maximum_seed_count: 2,
        },
    ));

    restart_candidates
}

struct JointReturnScaleAlignmentSeedsBuildInput<'a> {
    baseline_parameter_vector: &'a [f64],
    active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    baseline_final_objective_terms: &'a [JointRefinementStageObjectiveTermDiagnostic],
    activity_log_shift: f64,
}

fn joint_return_scale_alignment_restart_seeds(
    input: JointReturnScaleAlignmentSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let JointReturnScaleAlignmentSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_final_objective_terms,
        activity_log_shift,
    } = input;

    return_scale_alignment_log_delta_shifts(baseline_final_objective_terms)
        .into_iter()
        .flat_map(|alignment| {
            let mut baseline_scale_aligned = baseline_parameter_vector.to_vec();
            apply_nig_log_delta_offset_shift(
                baseline_scale_aligned.as_mut_slice(),
                active_parameter_layout,
                alignment.log_delta_shift,
            );
            clamp_in_place(
                baseline_scale_aligned.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            let mut activity_scale_aligned = baseline_parameter_vector.to_vec();

            apply_nig_log_delta_offset_shift(
                activity_scale_aligned.as_mut_slice(),
                active_parameter_layout,
                alignment.log_delta_shift,
            );

            apply_activity_alignment(
                activity_scale_aligned.as_mut_slice(),
                active_parameter_layout,
                activity_log_shift,
            );

            clamp_in_place(
                activity_scale_aligned.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            vec![
                (
                    format!("return_scale_{}", alignment.seed_label),
                    baseline_scale_aligned,
                ),
                (
                    format!("return_scale_{}_activity_aligned", alignment.seed_label),
                    activity_scale_aligned,
                ),
            ]
        })
        .collect()
}

fn return_scale_alignment_log_delta_shifts(
    objective_terms: &[JointRefinementStageObjectiveTermDiagnostic],
) -> Vec<ReturnScaleAlignmentLogDeltaShift> {
    RETURN_SCALE_ALIGNMENT_METRICS
        .iter()
        .filter_map(|alignment_metric| {
            objective_terms
                .iter()
                .find(|term| term.metric_id == alignment_metric.metric_id)
                .and_then(|term| return_scale_alignment_ratio(term, alignment_metric.scale_mapping))
                .map(|scale_ratio| ReturnScaleAlignmentLogDeltaShift {
                    seed_label: alignment_metric.seed_label,
                    log_delta_shift: scale_ratio.ln(),
                })
        })
        .collect()
}

fn return_scale_alignment_ratio(
    objective_term: &JointRefinementStageObjectiveTermDiagnostic,
    scale_mapping: ReturnScaleAlignmentScaleMapping,
) -> Option<f64> {
    if !objective_term.observed_value.is_finite()
        || !objective_term.simulated_value.is_finite()
        || objective_term.observed_value <= 0.0
        || objective_term.simulated_value <= 0.0
    {
        return None;
    }

    let observed_to_simulated_ratio =
        objective_term.observed_value / objective_term.simulated_value;
    if !observed_to_simulated_ratio.is_finite() || observed_to_simulated_ratio <= 0.0 {
        return None;
    }

    let scale_ratio = match scale_mapping {
        ReturnScaleAlignmentScaleMapping::Linear => observed_to_simulated_ratio,
        ReturnScaleAlignmentScaleMapping::Quadratic => observed_to_simulated_ratio.sqrt(),
    };

    scale_ratio.is_finite().then_some(scale_ratio)
}

fn finalize_restart_seed_candidates(
    input: RestartSeedCandidateFinalizationInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let RestartSeedCandidateFinalizationInput {
        restart_candidates,
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        restart_candidate_budget,
        restart_seed_score,
        joint_context,
        progress_sender,
    } = input;

    let target_restart_count = restart_candidate_budget.max(1);

    let deduplicated_candidates = deduplicate_restart_seed_candidates(restart_candidates);

    let total_screening_units = deduplicated_candidates
        .len()
        .saturating_add(target_restart_count);

    emit_restart_candidate_screening_progress(
        progress_sender,
        0,
        total_screening_units,
        Some("candidate_frontier"),
    );

    let completed_screening_units = Mutex::new(0usize);

    let mut screened_restart_candidates = screen_restart_candidates(
        deduplicated_candidates.as_slice(),
        &restart_seed_score,
        joint_context,
        progress_sender,
        &completed_screening_units,
        total_screening_units,
    );

    let frontier_score_ceiling =
        exploratory_restart_frontier_score_ceiling(screened_restart_candidates.as_slice());

    if screened_restart_candidates.len() >= target_restart_count {
        emit_restart_candidate_screening_progress(
            progress_sender,
            total_screening_units,
            total_screening_units,
            Some("restart_candidates_screened"),
        );

        return screened_restart_candidates
            .into_iter()
            .map(ScreenedRestartCandidate::into_seed_candidate)
            .take(target_restart_count)
            .collect();
    }

    let mut screened_halton_candidates =
        screen_halton_restart_candidates(HaltonRestartCandidateScreeningInput {
            baseline_parameter_vector,
            active_parameter_layout,
            lower_bounds,
            upper_bounds,
            target_restart_count,
            frontier_score_ceiling,
            halton_seed_score: &restart_seed_score,
            joint_context,
            progress_sender,
            completed_screening_units: &completed_screening_units,
            total_screening_units,
        });

    screened_halton_candidates.sort_by(|left, right| {
        left.score
            .total_cmp(&right.score)
            .then_with(|| left.restart_id.cmp(&right.restart_id))
    });

    screened_restart_candidates.extend(screened_halton_candidates);

    let selected_restart_candidates = screened_restart_candidates
        .into_iter()
        .map(ScreenedRestartCandidate::into_seed_candidate)
        .collect::<Vec<_>>();

    deduplicate_restart_seed_candidates(selected_restart_candidates)
        .into_iter()
        .take(target_restart_count)
        .collect()
}

struct RestartSeedCandidateFinalizationInput<'a> {
    restart_candidates: Vec<(String, Vec<f64>)>,
    baseline_parameter_vector: &'a [f64],
    active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    restart_candidate_budget: usize,
    restart_seed_score: &'a (dyn Fn(&str, &[f64]) -> Option<f64> + Sync),
    joint_context: Option<&'a JointRefinementContext>,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
}

#[derive(Debug)]
struct ScreenedRestartCandidate {
    restart_id: String,
    parameter_vector: Vec<f64>,
    score: f64,
}

impl ScreenedRestartCandidate {
    fn try_new(restart_id: &str, parameter_vector: &[f64], score: Option<f64>) -> Option<Self> {
        score.map(|score| Self {
            restart_id: restart_id.to_string(),
            parameter_vector: parameter_vector.to_vec(),
            score,
        })
    }

    fn into_seed_candidate(self) -> (String, Vec<f64>) {
        (self.restart_id, self.parameter_vector)
    }
}

fn exploratory_restart_frontier_score_ceiling(
    screened_candidates: &[ScreenedRestartCandidate],
) -> f64 {
    screened_candidates
        .iter()
        .map(|screened_candidate| screened_candidate.score)
        .fold(None, |best_score: Option<f64>, score| {
            Some(best_score.map_or(score, |best_score| best_score.min(score)))
        })
        .map_or(
            JOINT_REFINEMENT_THRESHOLD_RELATIVE_ADMISSIBILITY_SCORE,
            |best_score| best_score.max(JOINT_REFINEMENT_THRESHOLD_RELATIVE_ADMISSIBILITY_SCORE),
        )
}

fn screen_restart_candidates<F>(
    restart_candidates: &[(String, Vec<f64>)],
    seed_score: &F,
    joint_context: Option<&JointRefinementContext>,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_screening_units: &Mutex<usize>,
    total_screening_units: usize,
) -> Vec<ScreenedRestartCandidate>
where
    F: Fn(&str, &[f64]) -> Option<f64> + Sync + ?Sized,
{
    let thread_pool = joint_context.and_then(|joint_context| {
        joint_context
            .stage_thread_pool
            .as_ref()
            .map(|thread_pool| thread_pool.as_ref())
    });

    screen_restart_candidates_with_thread_pool(
        restart_candidates,
        seed_score,
        thread_pool,
        progress_sender,
        completed_screening_units,
        total_screening_units,
    )
}

fn screen_restart_candidates_with_thread_pool<F>(
    restart_candidates: &[(String, Vec<f64>)],
    seed_score: &F,
    thread_pool: Option<&rayon::ThreadPool>,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_screening_units: &Mutex<usize>,
    total_screening_units: usize,
) -> Vec<ScreenedRestartCandidate>
where
    F: Fn(&str, &[f64]) -> Option<f64> + Sync + ?Sized,
{
    let screen_candidate = |(restart_id, seed): &(String, Vec<f64>)| {
        let score = seed_score(restart_id.as_str(), seed.as_slice());

        emit_completed_restart_candidate_screening_unit(
            progress_sender,
            completed_screening_units,
            total_screening_units,
            restart_id.as_str(),
        );

        ScreenedRestartCandidate::try_new(restart_id.as_str(), seed.as_slice(), score)
    };

    if let Some(thread_pool) = thread_pool
        && thread_pool.current_num_threads() > 1
        && restart_candidates.len() > 1
    {
        return thread_pool.install(|| {
            restart_candidates
                .par_iter()
                .filter_map(screen_candidate)
                .collect()
        });
    }

    restart_candidates
        .iter()
        .filter_map(screen_candidate)
        .collect()
}

#[cfg(test)]
pub(super) fn restart_candidate_screening_max_parallelism_for_test(
    cpu_count: usize,
    candidate_count: usize,
) -> usize {
    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cpu_count.max(1))
        .build()
        .expect("test thread pool should build");

    let active_count = Arc::new(AtomicUsize::new(0));
    let max_active_count = Arc::new(AtomicUsize::new(0));

    let restart_candidates = (0..candidate_count)
        .map(|candidate_index| {
            (
                format!("candidate_{candidate_index}"),
                vec![candidate_index as f64],
            )
        })
        .collect::<Vec<_>>();

    let completed_screening_units = Mutex::new(0usize);

    let score_candidate = |_: &str, _: &[f64]| {
        let current_active_count = active_count.fetch_add(1, Ordering::SeqCst) + 1;
        max_active_count.fetch_max(current_active_count, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(25));
        active_count.fetch_sub(1, Ordering::SeqCst);
        Some(0.0)
    };

    let _ = screen_restart_candidates_with_thread_pool(
        restart_candidates.as_slice(),
        &score_candidate,
        Some(&thread_pool),
        None,
        &completed_screening_units,
        candidate_count,
    );

    max_active_count.load(Ordering::SeqCst)
}

fn screen_halton_restart_candidates<F>(
    input: HaltonRestartCandidateScreeningInput<'_, F>,
) -> Vec<ScreenedRestartCandidate>
where
    F: Fn(&str, &[f64]) -> Option<f64> + Sync + ?Sized,
{
    let HaltonRestartCandidateScreeningInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        target_restart_count,
        frontier_score_ceiling,
        halton_seed_score,
        joint_context,
        progress_sender,
        completed_screening_units,
        total_screening_units,
    } = input;

    let screen_halton_position = |halton_position: usize| {
        let mut halton_seed =
            halton_parameter_vector(halton_position as u64, lower_bounds, upper_bounds);

        if halton_seed.len() >= baseline_parameter_vector.len() {
            halton_seed[active_parameter_layout.log_lambda_scale_index] =
                halton_seed[active_parameter_layout.log_lambda_scale_index].clamp(
                    lower_bounds[active_parameter_layout.log_lambda_scale_index],
                    upper_bounds[active_parameter_layout.log_lambda_scale_index],
                );
        }

        clamp_in_place(halton_seed.as_mut_slice(), lower_bounds, upper_bounds);

        let restart_id = build_halton_restart_id(halton_position);

        let score = halton_seed_score(restart_id.as_str(), halton_seed.as_slice());

        emit_completed_restart_candidate_screening_unit(
            progress_sender,
            completed_screening_units,
            total_screening_units,
            restart_id.as_str(),
        );

        score
            .filter(|score| *score <= frontier_score_ceiling)
            .and_then(|score| {
                ScreenedRestartCandidate::try_new(
                    restart_id.as_str(),
                    halton_seed.as_slice(),
                    Some(score),
                )
            })
    };

    if let Some(joint_context) = joint_context
        && joint_refinement_has_parallel_worker_capacity(joint_context)
        && target_restart_count > 1
    {
        return with_joint_refinement_thread_pool(joint_context, || {
            (1..=target_restart_count)
                .into_par_iter()
                .filter_map(screen_halton_position)
                .collect()
        });
    }

    (1..=target_restart_count)
        .filter_map(screen_halton_position)
        .collect()
}

struct HaltonRestartCandidateScreeningInput<'a, F: ?Sized> {
    baseline_parameter_vector: &'a [f64],
    active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    target_restart_count: usize,
    frontier_score_ceiling: f64,
    halton_seed_score: &'a F,
    joint_context: Option<&'a JointRefinementContext>,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    completed_screening_units: &'a Mutex<usize>,
    total_screening_units: usize,
}

fn emit_completed_restart_candidate_screening_unit(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_screening_units: &Mutex<usize>,
    total_screening_units: usize,
    current_item_label: &str,
) {
    let Ok(mut completed_units_guard) = completed_screening_units.lock() else {
        return;
    };

    *completed_units_guard = completed_units_guard.saturating_add(1);

    emit_restart_candidate_screening_progress(
        progress_sender,
        *completed_units_guard,
        total_screening_units,
        Some(current_item_label),
    );
}

fn emit_restart_candidate_screening_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(JointRefinementStageProgressUpdate {
        current_item_label: current_item_label.map(str::to_string),
        ..JointRefinementStageProgressUpdate::new(
            JointRefinementStageProgressPhase::RestartCandidateScreening,
            completed_units,
            total_units,
        )
    });
}

fn emit_restart_seed_generation_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(JointRefinementStageProgressUpdate {
        current_item_label: current_item_label.map(str::to_string),
        ..JointRefinementStageProgressUpdate::new(
            JointRefinementStageProgressPhase::RestartSeedGeneration,
            completed_units,
            total_units,
        )
    });
}

fn screened_exploratory_restart_seed_score(
    halton_seed: &[f64],
    joint_context: &JointRefinementContext,
) -> Option<f64> {
    let fitted_parameters = parameter_vector_to_fitted_parameters(halton_seed, joint_context)?;

    precheck_seed_summary_workload(&fitted_parameters, joint_context).ok()?;

    evaluate_joint_fitted_candidate_restart_admission_score(&fitted_parameters, joint_context).ok()
}

fn joint_tick_moment_restart_seeds(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    activity_log_shift: f64,
    joint_context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<(String, Vec<f64>)> {
    let mut activity_aligned_parameter_vector = baseline_parameter_vector.to_vec();

    apply_activity_alignment(
        activity_aligned_parameter_vector.as_mut_slice(),
        active_parameter_layout,
        activity_log_shift,
    );

    clamp_in_place(
        activity_aligned_parameter_vector.as_mut_slice(),
        lower_bounds,
        upper_bounds,
    );

    let Some(activity_aligned_fitted_parameters) = parameter_vector_to_fitted_parameters(
        activity_aligned_parameter_vector.as_slice(),
        joint_context,
    ) else {
        return Vec::new();
    };

    let Some(modeled_open_event_rate_per_hour) =
        crate::stages::hawkes::stationary_open_event_rate_per_hour(
            &activity_aligned_fitted_parameters.hawkes_fitted_parameters,
        )
    else {
        return Vec::new();
    };

    if !joint_context
        .baseline_modeled_open_event_rate_per_hour
        .is_finite()
        || joint_context.baseline_modeled_open_event_rate_per_hour <= 0.0
    {
        return Vec::new();
    }

    let aggregated_15m_sample = build_continuous_calibration_sample_from_aggregated_intervals(
        joint_context
            .observed_data
            .observed_15m_intervals
            .as_slice(),
        modeled_open_event_rate_per_hour,
        joint_context
            .observed_data
            .observed_open_event_rate_per_hour,
        joint_context.observed_data.fallback_sell_share_proxy,
    );

    let tick_sample_business_time_scaled =
        rescale_continuous_return_calibration_sample_business_time_scales(
            &joint_context
                .observed_data
                .observed_tick_continuous_return_calibration_sample,
            modeled_open_event_rate_per_hour
                / joint_context.baseline_modeled_open_event_rate_per_hour,
        );

    build_nig_moment_seed_configurations(&tick_sample_business_time_scaled, solver_settings)
        .into_iter()
        .enumerate()
        .filter_map(|(configuration_index, configuration)| {
            let projected_search_point =
                nig_search_point_from_configuration(&aggregated_15m_sample, configuration)?;
            let mut restart_seed = activity_aligned_parameter_vector.clone();
            apply_nig_search_point(
                restart_seed.as_mut_slice(),
                active_parameter_layout,
                projected_search_point,
            );
            clamp_in_place(restart_seed.as_mut_slice(), lower_bounds, upper_bounds);

            Some((
                format!(
                    "tick_moment_{:02}_activity_aligned",
                    configuration_index + 1
                ),
                restart_seed,
            ))
        })
        .collect()
}

struct JointNigRestartSeedsBuildInput<'a> {
    baseline_parameter_vector: &'a [f64],
    active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    baseline_15m_calibration_sample: &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    activity_log_shift: f64,
    maximum_seed_count: usize,
}

fn joint_nig_basin_restart_seeds(
    input: JointNigRestartSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let JointNigRestartSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        activity_log_shift,
        maximum_seed_count,
    } = input;

    return_component_basin_diagnostics
        .iter()
        .take(maximum_seed_count.div_ceil(2))
        .filter_map(|diagnostic| {
            project_basin_diagnostic_into_joint_nig_coordinates(
                baseline_15m_calibration_sample,
                diagnostic,
            )
            .map(|search_point| (diagnostic, search_point))
        })
        .flat_map(|(diagnostic, projected_search_point)| {
            let mut baseline_nig_seed = baseline_parameter_vector.to_vec();
            apply_nig_search_point(
                baseline_nig_seed.as_mut_slice(),
                active_parameter_layout,
                projected_search_point,
            );
            clamp_in_place(baseline_nig_seed.as_mut_slice(), lower_bounds, upper_bounds);

            let mut activity_aligned_nig_seed = baseline_nig_seed.clone();
            apply_activity_alignment(
                activity_aligned_nig_seed.as_mut_slice(),
                active_parameter_layout,
                activity_log_shift,
            );
            clamp_in_place(
                activity_aligned_nig_seed.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            vec![
                (
                    format!("nig_basin_{}", diagnostic.family_label),
                    baseline_nig_seed,
                ),
                (
                    format!("nig_basin_{}_activity_aligned", diagnostic.family_label),
                    activity_aligned_nig_seed,
                ),
            ]
        })
        .collect()
}

fn joint_nig_alpha_delta_restart_seeds(
    input: JointNigRestartSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let JointNigRestartSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        activity_log_shift,
        maximum_seed_count,
    } = input;

    return_component_basin_diagnostics
        .iter()
        .take(maximum_seed_count.div_ceil(2))
        .filter_map(|diagnostic| {
            project_basin_diagnostic_into_joint_nig_coordinates(
                baseline_15m_calibration_sample,
                diagnostic,
            )
            .map(|search_point| (diagnostic, search_point))
        })
        .flat_map(|(diagnostic, projected_search_point)| {
            let mut alpha_delta_seed = baseline_parameter_vector.to_vec();

            apply_nig_alpha_delta_search_point(
                alpha_delta_seed.as_mut_slice(),
                active_parameter_layout,
                projected_search_point,
            );

            clamp_in_place(alpha_delta_seed.as_mut_slice(), lower_bounds, upper_bounds);

            let mut activity_aligned_alpha_delta_seed = alpha_delta_seed.clone();

            apply_activity_alignment(
                activity_aligned_alpha_delta_seed.as_mut_slice(),
                active_parameter_layout,
                activity_log_shift,
            );

            clamp_in_place(
                activity_aligned_alpha_delta_seed.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            vec![
                (
                    format!("nig_alpha_delta_{}", diagnostic.family_label),
                    alpha_delta_seed,
                ),
                (
                    format!(
                        "nig_alpha_delta_{}_activity_aligned",
                        diagnostic.family_label
                    ),
                    activity_aligned_alpha_delta_seed,
                ),
            ]
        })
        .collect()
}

fn apply_activity_alignment(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    activity_log_shift: f64,
) {
    parameter_vector[active_parameter_layout.log_lambda_scale_index] = activity_log_shift;

    if let Some(index) = active_parameter_layout.known_activity_scale_index {
        parameter_vector[index] = activity_log_shift;
    }

    if let Some(index) = active_parameter_layout.surprise_activity_scale_index {
        parameter_vector[index] = activity_log_shift;
    }

    if let Some(index) = active_parameter_layout.surprise_baseline_scale_index {
        parameter_vector[index] = activity_log_shift;
    }
}

pub(super) fn selected_return_components_candidate(
    return_components_payload: &ReturnComponentsStageFittedPayload,
) -> Option<&ReturnComponentsStageCandidateDiagnostic> {
    return_components_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == return_components_payload.selected_candidate_id)
}

pub(super) fn project_basin_diagnostic_into_joint_nig_coordinates(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    diagnostic: &ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
) -> Option<NigShapeSearchPoint> {
    if !nig_basin_coordinates_are_finite(diagnostic) {
        return None;
    }

    let configuration = NigSharedPlusShiftConfiguration::try_new(
        diagnostic.final_alpha,
        diagnostic.final_beta_0,
        diagnostic.final_delta,
        0.0,
        diagnostic.final_delta_beta,
    )
    .ok()?;

    nig_search_point_from_configuration(calibration_sample, configuration)
}

fn nig_basin_coordinates_are_finite(
    diagnostic: &ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
) -> bool {
    [
        diagnostic.final_log_alpha,
        diagnostic.final_buy_skew_coordinate,
        diagnostic.final_sell_relative_coordinate,
        diagnostic.final_log_delta_offset,
    ]
    .into_iter()
    .all(f64::is_finite)
}

fn apply_nig_search_point(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    search_point: NigShapeSearchPoint,
) {
    parameter_vector[active_parameter_layout.nig_block[0]] = search_point.log_alpha;
    parameter_vector[active_parameter_layout.nig_block[1]] = search_point.buy_skew_coordinate;
    parameter_vector[active_parameter_layout.nig_block[2]] = search_point.sell_relative_coordinate;
    parameter_vector[active_parameter_layout.nig_block[3]] = search_point.log_delta_offset;
}

fn apply_nig_alpha_delta_search_point(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    search_point: NigShapeSearchPoint,
) {
    parameter_vector[active_parameter_layout.nig_block[0]] = search_point.log_alpha;
    parameter_vector[active_parameter_layout.nig_block[3]] = search_point.log_delta_offset;
}

fn apply_nig_log_delta_offset_shift(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    log_delta_offset_shift: f64,
) {
    parameter_vector[active_parameter_layout.nig_block[3]] += log_delta_offset_shift;
}

pub(super) fn observed_activity_log_shift(
    observed_open_event_rate_per_hour: f64,
    modeled_open_event_rate_per_hour: f64,
) -> f64 {
    let observed_rate = observed_open_event_rate_per_hour.max(1.0e-8);
    let modeled_rate = modeled_open_event_rate_per_hour.max(1.0e-8);

    (observed_rate / modeled_rate).ln()
}

pub(super) fn joint_refinement_final_simulation_seed_count(
    solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    match solver_settings.max_iterations {
        0..=32 => 1,
        33..=96 => 2,
        _ => 3,
    }
}

pub(super) fn compute_cumulative_log_price_span(
    intervals: &[crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation],
) -> f64 {
    let mut cumulative_value: f64 = 0.0;
    let mut minimum_cumulative_value: f64 = 0.0;
    let mut maximum_cumulative_value: f64 = 0.0;

    for interval in intervals {
        cumulative_value += interval.log_return;
        minimum_cumulative_value = minimum_cumulative_value.min(cumulative_value);
        maximum_cumulative_value = maximum_cumulative_value.max(cumulative_value);
    }

    maximum_cumulative_value - minimum_cumulative_value
}

pub(super) fn cached_seed_summaries_for_candidate_seeds(
    input: CandidateSeedSummariesCacheInput<'_>,
) -> Result<Vec<(u64, JointRefinementSeedSummary)>, String> {
    let CandidateSeedSummariesCacheInput {
        parameter_bits,
        fitted_parameters,
        context,
        simulation_seeds,
        progress_sender,
        progress_phase,
        completed_units_before_seed_summaries,
        total_units,
    } = input;

    let cache_key = parameter_bits.map(|bits| JointRefinementSeedSummaryCacheKey {
        parameter_bits: bits.to_vec(),
        simulation_seed: 0,
    });

    let mut resolved_seed_summaries = Vec::with_capacity(simulation_seeds.len());
    let mut missing_seeds = Vec::new();

    for simulation_seed in simulation_seeds {
        let per_seed_cache_key =
            cache_key
                .as_ref()
                .map(|candidate_cache_key| JointRefinementSeedSummaryCacheKey {
                    parameter_bits: candidate_cache_key.parameter_bits.clone(),
                    simulation_seed: *simulation_seed,
                });

        if let Some(seed_cache_key) = per_seed_cache_key.as_ref()
            && let Ok(cache_guard) = context.seed_summary_cache.lock()
            && let Some(cached_summary) = cache_guard.get(seed_cache_key)
        {
            context.profiling_state.record_seed_summary_cache_hit();
            match cached_summary.clone() {
                Ok(summary) => {
                    resolved_seed_summaries.push((*simulation_seed, summary));
                    continue;
                }
                Err(error_message) => return Err(error_message),
            }
        }

        missing_seeds.push(*simulation_seed);
    }

    context
        .profiling_state
        .record_seed_summary_cache_miss(missing_seeds.len());

    let completed_units = completed_units_before_seed_summaries + resolved_seed_summaries.len();

    emit_seed_summary_progress(
        progress_sender,
        progress_phase,
        completed_units,
        total_units,
        None,
    );

    if !missing_seeds.is_empty() {
        let missing_seed_summaries = build_seed_summaries_for_candidate_seeds(
            context,
            fitted_parameters,
            missing_seeds.as_slice(),
            progress_sender,
            progress_phase,
            completed_units,
            total_units,
        )?;

        if let Some(candidate_cache_key) = cache_key
            && let Ok(mut cache_guard) = context.seed_summary_cache.lock()
        {
            for (simulation_seed, seed_summary) in &missing_seed_summaries {
                cache_guard.insert(
                    JointRefinementSeedSummaryCacheKey {
                        parameter_bits: candidate_cache_key.parameter_bits.clone(),
                        simulation_seed: *simulation_seed,
                    },
                    Ok(seed_summary.clone()),
                );
            }
        }

        resolved_seed_summaries.extend(missing_seed_summaries);
    }

    resolved_seed_summaries.sort_by_key(|(simulation_seed, _)| {
        simulation_seeds
            .iter()
            .position(|candidate_seed| candidate_seed == simulation_seed)
            .unwrap_or(simulation_seeds.len())
    });

    Ok(resolved_seed_summaries)
}

pub(super) struct CandidateSeedSummariesCacheInput<'a> {
    pub(super) parameter_bits: Option<&'a [u64]>,
    pub(super) fitted_parameters: &'a JointRefinementStageFittedParameters,
    pub(super) context: &'a JointRefinementContext,
    pub(super) simulation_seeds: &'a [u64],
    pub(super) progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(super) progress_phase: Option<JointRefinementStageProgressPhase>,
    pub(super) completed_units_before_seed_summaries: usize,
    pub(super) total_units: usize,
}

pub(crate) fn build_surprise_multipliers_for_interval_grid(
    simulation_start_utc: DateTime<Utc>,
    interval_count: usize,
    interval_duration_nanos: i64,
    weekly_hour_utc_multipliers: &[f64],
) -> Vec<(u64, f64)> {
    (0..interval_count)
        .map(|interval_index| {
            let interval_start = simulation_start_utc
                .checked_add_signed(Duration::nanoseconds(
                    interval_duration_nanos.saturating_mul(interval_index as i64),
                ))
                .unwrap_or(simulation_start_utc);

            let weekday_index = interval_start.weekday().num_days_from_monday() as usize;
            let hour_index = interval_start.hour() as usize;
            let bucket_index = weekday_index * 24 + hour_index;
            (
                interval_index as u64,
                *weekly_hour_utc_multipliers
                    .get(bucket_index)
                    .unwrap_or(&1.0),
            )
        })
        .collect()
}

fn build_seed_summaries_for_candidate_seeds(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seeds: &[u64],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units_before_missing: usize,
    total_units: usize,
) -> Result<Vec<(u64, JointRefinementSeedSummary)>, String> {
    if joint_refinement_has_parallel_worker_capacity(context) && simulation_seeds.len() > 1 {
        return build_seed_summaries_for_candidate_seeds_parallel(
            context,
            fitted_parameters,
            simulation_seeds,
            progress_sender,
            progress_phase,
            completed_units_before_missing,
            total_units,
        );
    }

    let mut completed_units = completed_units_before_missing;
    let mut seed_summaries = Vec::with_capacity(simulation_seeds.len());

    for simulation_seed in simulation_seeds {
        let seed_summary =
            build_seed_summary_for_candidate_seed(context, fitted_parameters, *simulation_seed)?;

        completed_units += 1;

        emit_seed_summary_progress(
            progress_sender,
            progress_phase,
            completed_units,
            total_units,
            Some(*simulation_seed),
        );

        seed_summaries.push((*simulation_seed, seed_summary));
    }

    Ok(seed_summaries)
}

fn build_seed_summaries_for_candidate_seeds_parallel(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seeds: &[u64],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units_before_missing: usize,
    total_units: usize,
) -> Result<Vec<(u64, JointRefinementSeedSummary)>, String> {
    let completed_units = Mutex::new(completed_units_before_missing);

    let seed_summary_results = with_joint_refinement_thread_pool(context, || {
        simulation_seeds
            .par_iter()
            .map(|simulation_seed| {
                let seed_summary = build_seed_summary_for_candidate_seed(
                    context,
                    fitted_parameters,
                    *simulation_seed,
                )?;
                if let Ok(mut completed_units_guard) = completed_units.lock() {
                    *completed_units_guard = completed_units_guard.saturating_add(1);
                    emit_seed_summary_progress(
                        progress_sender,
                        progress_phase,
                        *completed_units_guard,
                        total_units,
                        Some(*simulation_seed),
                    );
                }
                Ok((*simulation_seed, seed_summary))
            })
            .collect::<Vec<_>>()
    });

    seed_summary_results.into_iter().collect()
}

fn build_seed_summary_for_candidate_seed(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
) -> Result<JointRefinementSeedSummary, String> {
    let seed_summary_started_at = Instant::now();

    let seed_summary_result = context
        .summary_simulation_scheduler
        .run_seed_summary_simulation(fitted_parameters, simulation_seed);

    context
        .profiling_state
        .record_seed_summary_simulation_duration(
            seed_summary_started_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        );

    seed_summary_result.map_err(|error| match error {
        JointRefinementSeedSummaryAcquisitionError::SimulationFailure { error_message } => {
            error_message
        }
        JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure { error_message } => {
            if let Ok(mut failure_guard) = context.simulation_infrastructure_failure.lock()
                && failure_guard.is_none()
            {
                *failure_guard = Some(error_message.clone());
            }
            error_message
        }
    })
}

fn emit_seed_summary_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units: usize,
    total_units: usize,
    simulation_seed: Option<u64>,
) {
    let Some(progress_phase) = progress_phase else {
        return;
    };

    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(JointRefinementStageProgressUpdate {
        current_item_label: simulation_seed.map(format_seed_summary_progress_item),
        ..JointRefinementStageProgressUpdate::new(progress_phase, completed_units, total_units)
    });
}

fn format_seed_summary_progress_item(simulation_seed: u64) -> String {
    format!("simulation_seed {simulation_seed}")
}

pub(super) fn selected_hawkes_observed_open_event_rate_per_hour(
    hawkes_payload: &HawkesStageFittedPayload,
) -> Option<f64> {
    hawkes_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == hawkes_payload.selected_candidate_id)
        .map(|candidate| candidate.implied_metrics.observed_open_event_rate_per_hour)
        .or_else(|| {
            crate::stages::hawkes::stationary_open_event_rate_per_hour(
                &hawkes_payload.fitted_parameters,
            )
        })
}

pub(super) fn stage_input_to_activity_input(
    hawkes_input: &JointRefinementStageHawkesInput,
) -> crate::stages::return_components::ReturnComponentsStageActivityInput {
    match hawkes_input {
        JointRefinementStageHawkesInput::InMemoryOutput { payload } => {
            crate::stages::return_components::ReturnComponentsStageActivityInput::InMemoryOutput {
                payload: payload.clone(),
            }
        }
        JointRefinementStageHawkesInput::PersistedArtifactReference {
            artifact_reference,
            payload,
        } => crate::stages::return_components::ReturnComponentsStageActivityInput::PersistedArtifactReference {
            artifact_reference: artifact_reference.clone(),
            payload: payload.clone(),
        },
    }
}
