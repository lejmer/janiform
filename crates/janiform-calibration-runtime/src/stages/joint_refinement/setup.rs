use std::sync::mpsc::Sender;
use std::time::Instant;

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use janiform_model_kernels::hawkes::{HawkesKernelLoadings, HawkesKernelParameters};
use janiform_model_kernels::news::SurpriseNewsArrivalConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals;
use crate::stages::return_components::{
    NigShapeSearchPoint, ReturnComponentsStageCandidateDiagnostic,
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
    ReturnComponentsStageContinuousReturnCalibrationSample, ReturnComponentsStageFittedPayload,
    nig_configuration_from_search_point, nig_search_bounds_for_calibration_sample,
    nig_search_point_from_configuration,
};

use super::contracts::{
    JointRefinementStageHawkesInput, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};
use super::execution::{
    JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND, JointRefinementActiveParameterLayout,
    JointRefinementContext, JointRefinementSeedSummary, JointRefinementSeedSummaryCacheKey,
    build_halton_restart_id, clamp_in_place, deduplicate_restart_seed_candidates,
    halton_parameter_vector,
};
use super::worker_runtime::JointRefinementSeedSummaryAcquisitionError;
use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageFittedParameters,
    JointRefinementStagePricingBlockFittedParameters, ReturnComponentsStageFittedParameters,
};

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

pub(super) fn build_restart_seeds(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    return_component_basin_diagnostics: &[ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    activity_log_shift: f64,
    halton_validation_context: &JointRefinementContext,
    restart_count: usize,
) -> Vec<(String, Vec<f64>)> {
    build_restart_seeds_with_halton_acceptance(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        activity_log_shift,
        restart_count,
        |halton_seed| {
            parameter_vector_to_fitted_parameters(halton_seed, halton_validation_context).is_some()
        },
    )
}

pub(super) fn build_restart_seeds_with_halton_acceptance<F>(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    return_component_basin_diagnostics: &[ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    activity_log_shift: f64,
    restart_count: usize,
    mut halton_seed_is_accepted: F,
) -> Vec<(String, Vec<f64>)>
where
    F: FnMut(&[f64]) -> bool,
{
    let mut baseline = baseline_parameter_vector.to_vec();
    clamp_in_place(baseline.as_mut_slice(), lower_bounds, upper_bounds);

    let mut activity_aligned = baseline_parameter_vector.to_vec();
    activity_aligned[active_parameter_layout.log_lambda_scale_index] = activity_log_shift;
    clamp_in_place(activity_aligned.as_mut_slice(), lower_bounds, upper_bounds);

    let mut midpoint = lower_bounds
        .iter()
        .zip(upper_bounds.iter())
        .map(|(lower_bound, upper_bound)| 0.5 * (lower_bound + upper_bound))
        .collect::<Vec<_>>();
    clamp_in_place(midpoint.as_mut_slice(), lower_bounds, upper_bounds);

    let mut restart_candidates = vec![
        ("baseline".to_string(), baseline),
        ("activity_aligned".to_string(), activity_aligned),
        ("midpoint".to_string(), midpoint),
    ];
    restart_candidates.extend(joint_nig_basin_restart_seeds(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        activity_log_shift,
        4,
    ));
    restart_candidates.extend(joint_nig_alpha_delta_restart_seeds(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        activity_log_shift,
        2,
    ));

    let target_restart_count = restart_count.max(1);
    let mut deduplicated_candidates = deduplicate_restart_seed_candidates(restart_candidates)
        .into_iter()
        .take(target_restart_count)
        .collect::<Vec<_>>();
    let mut halton_index = 1u64;
    let mut halton_position = 1usize;
    let halton_attempt_cap = 8 * target_restart_count;

    while deduplicated_candidates.len() < target_restart_count
        && halton_position <= halton_attempt_cap
    {
        let mut halton_seed = halton_parameter_vector(halton_index, lower_bounds, upper_bounds);
        if halton_seed.len() >= baseline_parameter_vector.len() {
            halton_seed[active_parameter_layout.log_lambda_scale_index] =
                halton_seed[active_parameter_layout.log_lambda_scale_index].clamp(
                    lower_bounds[active_parameter_layout.log_lambda_scale_index],
                    upper_bounds[active_parameter_layout.log_lambda_scale_index],
                );
        }

        clamp_in_place(halton_seed.as_mut_slice(), lower_bounds, upper_bounds);

        if halton_seed_is_accepted(halton_seed.as_slice()) {
            deduplicated_candidates = deduplicate_restart_seed_candidates({
                let mut candidates = deduplicated_candidates;
                candidates.push((build_halton_restart_id(halton_position), halton_seed));
                candidates
            });
        }

        halton_index += 1;
        halton_position += 1;
    }

    deduplicated_candidates
}

fn joint_nig_basin_restart_seeds(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    return_component_basin_diagnostics: &[ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    activity_log_shift: f64,
    maximum_seed_count: usize,
) -> Vec<(String, Vec<f64>)> {
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
            activity_aligned_nig_seed[active_parameter_layout.log_lambda_scale_index] =
                activity_log_shift;
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
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    return_component_basin_diagnostics: &[ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    activity_log_shift: f64,
    maximum_seed_count: usize,
) -> Vec<(String, Vec<f64>)> {
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
            activity_aligned_alpha_delta_seed[active_parameter_layout.log_lambda_scale_index] =
                activity_log_shift;
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
    parameter_bits: Option<&[u64]>,
    fitted_parameters: &JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
    simulation_seeds: &[u64],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units_before_seed_summaries: usize,
    total_units: usize,
) -> Result<Vec<(u64, JointRefinementSeedSummary)>, String> {
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
            completed_units_before_seed_summaries,
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
    let mut completed_units = completed_units_before_missing;
    let mut seed_summaries = Vec::with_capacity(simulation_seeds.len());

    for simulation_seed in simulation_seeds {
        let seed_summary_started_at = Instant::now();
        let seed_summary_result = context
            .summary_simulation_scheduler
            .run_seed_summary_simulation(fitted_parameters, *simulation_seed);
        context
            .profiling_state
            .record_seed_summary_simulation_duration(
                seed_summary_started_at
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64,
            );
        let seed_summary = seed_summary_result.map_err(|error| match error {
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
        })?;

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
