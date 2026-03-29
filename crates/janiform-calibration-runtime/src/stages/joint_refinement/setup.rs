use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use janiform_engine_simulation::{
    MeanReversionBoundaryState, MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
    SeededTickEventStateSnapshotCapture, SeededTickSimulationModelInputs,
    SeededTickSimulationRunConfig, SeededTickSimulationRunOptions,
    run_seeded_tick_event_simulation,
};
use janiform_foundations::time::{
    FIFTEEN_MINUTES_NANOS, FIVE_MINUTES_NANOS, ONE_HOUR_NANOS, ceil_to_hour_utc_nanos,
    floor_to_hour_utc_nanos,
};
use janiform_market_time::hourly_factors::{
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::news::SurpriseNewsArrivalConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use crate::adapters::load_hourly_factor_contract_for_calibration;
use crate::calendar::ExchangeCalendarRegistry;
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::joint_metrics::{
    JointRefinementBusinessTimeBucketThresholds,
    build_continuous_calibration_sample_from_aggregated_intervals,
    build_observed_open_session_interval_observations, business_time_bucket_thresholds,
    mean_absolute_observed_return_by_utc_hour, mean_absolute_return_by_utc_hour,
    mean_absolute_trade_return_by_utc_hour, return_scale_summaries_by_business_time_bucket,
    return_scale_summary, return_scale_summary_from_returns, rolling_variance_activity_metric,
    sampled_return_values, sampled_trade_returns, scale_normalized_returns, tick_count_series,
    zeroed_interval_observation_template,
};
use crate::stages::joint_refinement::contracts::JointRefinementStageHawkesInput;
use crate::stages::return_components::{
    NigShapeSearchPoint, ReturnComponentsObservedOpenSessionReturn,
    ReturnComponentsStageCandidateDiagnostic, ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
    ReturnComponentsStageContinuousReturnCalibrationSample, ReturnComponentsStageFittedPayload,
    ReturnComponentsStageJointRefinementObservedSignalPreparation,
    continuous_return_calibration_sample_observed_returns, nig_configuration_from_search_point,
    nig_search_bounds_for_calibration_sample, nig_search_point_from_configuration,
};

use super::execution::{
    JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND, JointRefinementActiveParameterLayout,
    JointRefinementContext, JointRefinementObservedData, JointRefinementSeedSummary,
    JointRefinementSeedSummaryCacheKey, build_halton_restart_id, clamp_in_place,
    deduplicate_restart_seed_candidates, halton_parameter_vector,
};
use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageFittedParameters,
    ReturnComponentsStageFittedParameters,
};

pub(super) fn parameter_vector_to_fitted_parameters(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Option<JointRefinementStageFittedParameters> {
    let mut structured_alpha = context.baseline_hawkes_fitted_parameters.structured_alpha;

    let mut news_process_configuration = context
        .baseline_return_components_fitted_parameters
        .news_process_configuration
        .clone();

    let lambda_scale =
        parameter_vector[context.active_parameter_layout.log_lambda_scale_index].exp();
    let beta_scale = parameter_vector[context.active_parameter_layout.log_beta_scale_index].exp();
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

    let alpha_loadings = HawkesKernelLoadings::try_from_structured_alpha(structured_alpha).ok()?;
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
    let hawkes_fitted_parameters = janiform_calibration_artifacts::HawkesStageFittedParameters {
        kernel_parameters,
        structured_alpha,
        structured_rho: context.baseline_hawkes_fitted_parameters.structured_rho,
        self_dominance_policy: context
            .baseline_hawkes_fitted_parameters
            .self_dominance_policy,
        stability_numeric_tolerance: context
            .baseline_hawkes_fitted_parameters
            .stability_numeric_tolerance,
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
        mean_reversion_fitted_parameters:
            MeanReversionFittedParameters::endogenous_core_expectation_one_interval(0.0)
                .expect("zero-kappa endogenous mean reversion should validate"),
    };

    return_components_fitted_parameters
        .news_process_configuration
        .validate()
        .ok()?;

    Some(JointRefinementStageFittedParameters {
        hawkes_fitted_parameters,
        return_components_fitted_parameters,
    })
}

pub(super) fn build_observed_data(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
    observed_signal_preparation: &ReturnComponentsStageJointRefinementObservedSignalPreparation,
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    observed_open_event_rate_per_hour: f64,
) -> JointRefinementObservedData {
    let source_window = candidate_generation_input.known_news_calendar.source_window;

    let observed_5m_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        FIVE_MINUTES_NANOS,
    );
    let observed_15m_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        FIFTEEN_MINUTES_NANOS,
    );
    let observed_hourly_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        ONE_HOUR_NANOS,
    );

    let observed_15m_rv_activity_metric =
        rolling_variance_activity_metric(observed_15m_intervals.as_slice());
    let observed_log_price_span =
        compute_cumulative_log_price_span(observed_15m_intervals.as_slice());

    let observed_5m_log_returns = observed_5m_intervals
        .iter()
        .map(|interval| interval.log_return)
        .collect::<Vec<_>>();
    let observed_15m_log_returns = observed_15m_intervals
        .iter()
        .map(|interval| interval.log_return)
        .collect::<Vec<_>>();

    let observed_tick_sample_count = continuous_return_calibration_sample_observed_returns(
        &observed_signal_preparation.continuous_return_calibration_sample,
    )
    .len();

    let observed_tick_returns = sampled_observed_tick_returns(
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        observed_tick_sample_count,
    );

    let observed_tick_shape_returns = scale_normalized_returns(observed_tick_returns.as_slice());
    let observed_5m_shape_returns = scale_normalized_returns(observed_5m_log_returns.as_slice());
    let observed_15m_shape_returns = scale_normalized_returns(observed_15m_log_returns.as_slice());

    let observed_tick_return_scale_summary =
        return_scale_summary_from_returns(observed_tick_returns.as_slice());
    let observed_5m_return_scale_summary = return_scale_summary(observed_5m_intervals.as_slice());
    let observed_15m_return_scale_summary = return_scale_summary(observed_15m_intervals.as_slice());

    let observed_5m_business_time_bucket_thresholds = business_time_bucket_thresholds(
        observed_5m_intervals.as_slice(),
    )
    .unwrap_or(JointRefinementBusinessTimeBucketThresholds {
        lower_quartile_cutoff: 0.0,
        median_cutoff: 0.0,
        upper_quartile_cutoff: 0.0,
    });
    let observed_15m_business_time_bucket_thresholds = business_time_bucket_thresholds(
        observed_15m_intervals.as_slice(),
    )
    .unwrap_or(JointRefinementBusinessTimeBucketThresholds {
        lower_quartile_cutoff: 0.0,
        median_cutoff: 0.0,
        upper_quartile_cutoff: 0.0,
    });

    let observed_5m_business_time_return_scale_summaries =
        return_scale_summaries_by_business_time_bucket(
            observed_5m_intervals.as_slice(),
            observed_5m_business_time_bucket_thresholds,
        );
    let observed_15m_business_time_return_scale_summaries =
        return_scale_summaries_by_business_time_bucket(
            observed_15m_intervals.as_slice(),
            observed_15m_business_time_bucket_thresholds,
        );

    let observed_5m_intraday_mean_absolute_returns =
        mean_absolute_return_by_utc_hour(observed_5m_intervals.as_slice());
    let observed_15m_intraday_mean_absolute_returns =
        mean_absolute_return_by_utc_hour(observed_15m_intervals.as_slice());
    let observed_tick_intraday_mean_absolute_returns = mean_absolute_observed_return_by_utc_hour(
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
    );

    let observed_hourly_tick_counts = tick_count_series(observed_hourly_intervals.as_slice());

    let hourly_simulation_start_utc = DateTime::<Utc>::from_timestamp_nanos(
        floor_to_hour_utc_nanos(source_window.window_start_utc_nanos),
    );
    let hourly_simulation_end_utc = DateTime::<Utc>::from_timestamp_nanos(ceil_to_hour_utc_nanos(
        source_window.window_end_utc_nanos,
    ));

    let hourly_simulation_interval_count = ((hourly_simulation_end_utc
        .timestamp_nanos_opt()
        .unwrap_or(source_window.window_end_utc_nanos)
        - hourly_simulation_start_utc
            .timestamp_nanos_opt()
            .unwrap_or(source_window.window_start_utc_nanos))
    .div_euclid(ONE_HOUR_NANOS)) as usize;

    let surprise_multiplier_by_hour_index = build_surprise_multipliers_by_hour_index(
        hourly_simulation_start_utc,
        hourly_simulation_interval_count,
        return_components_fitted_parameters,
    );

    JointRefinementObservedData {
        observed_open_event_rate_per_hour,
        fallback_sell_share_proxy: candidate_generation_input.open_session_sell_share_proxy,
        observed_tick_continuous_return_calibration_sample: observed_signal_preparation
            .continuous_return_calibration_sample
            .clone(),
        observed_5m_intervals: observed_5m_intervals.clone(),
        observed_15m_intervals: observed_15m_intervals.clone(),
        observed_tick_intraday_mean_absolute_returns,
        observed_tick_shape_returns,
        observed_5m_shape_returns,
        observed_15m_shape_returns,
        observed_tick_return_scale_summary,
        observed_5m_return_scale_summary,
        observed_15m_return_scale_summary,
        observed_5m_business_time_bucket_thresholds,
        observed_15m_business_time_bucket_thresholds,
        observed_5m_business_time_return_scale_summaries,
        observed_15m_business_time_return_scale_summaries,
        observed_5m_intraday_mean_absolute_returns,
        observed_15m_intraday_mean_absolute_returns,
        observed_hourly_tick_counts,
        observed_15m_rv_activity_metric,
        observed_log_price_span,
        hourly_simulation_start_utc,
        hourly_simulation_end_utc,
        hourly_simulation_interval_count,
        surprise_weekly_hour_utc_multipliers: surprise_multiplier_by_hour_index
            .into_iter()
            .map(|(_, multiplier)| multiplier)
            .collect(),
        simulated_5m_interval_template: zeroed_interval_observation_template(
            observed_5m_intervals.as_slice(),
        ),
        simulated_15m_interval_template: zeroed_interval_observation_template(
            observed_15m_intervals.as_slice(),
        ),
        simulated_hourly_interval_template: zeroed_interval_observation_template(
            observed_hourly_intervals.as_slice(),
        ),
    }
}

pub(super) fn sampled_observed_tick_returns(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    target_count: usize,
) -> Vec<f64> {
    let raw_observed_tick_returns = observed_open_session_returns
        .iter()
        .map(|observation| observation.log_return)
        .collect::<Vec<_>>();

    sampled_return_values(raw_observed_tick_returns.as_slice(), target_count)
}

pub(super) fn build_baseline_parameter_vector(
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    hawkes_fitted_parameters: &janiform_calibration_artifacts::HawkesStageFittedParameters,
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
        let _ = hawkes_fitted_parameters;
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
    restart_count: usize,
) -> Vec<(String, Vec<f64>)> {
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
    ));
    restart_candidates.extend(joint_nig_alpha_delta_restart_seeds(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        activity_log_shift,
    ));

    let target_restart_count = restart_count.max(1);
    let mut deduplicated_candidates = deduplicate_restart_seed_candidates(restart_candidates);
    let mut halton_index = 1u64;
    let mut halton_position = 1usize;
    while deduplicated_candidates.len() < target_restart_count {
        let mut halton_seed = halton_parameter_vector(halton_index, lower_bounds, upper_bounds);
        if halton_seed.len() >= baseline_parameter_vector.len() {
            halton_seed[active_parameter_layout.log_lambda_scale_index] =
                halton_seed[active_parameter_layout.log_lambda_scale_index].clamp(
                    lower_bounds[active_parameter_layout.log_lambda_scale_index],
                    upper_bounds[active_parameter_layout.log_lambda_scale_index],
                );
        }

        clamp_in_place(halton_seed.as_mut_slice(), lower_bounds, upper_bounds);

        deduplicated_candidates = deduplicate_restart_seed_candidates({
            let mut candidates = deduplicated_candidates;
            candidates.push((build_halton_restart_id(halton_position), halton_seed));
            candidates
        });

        halton_index += 1;
        halton_position += 1;
    }

    deduplicated_candidates
        .into_iter()
        .take(target_restart_count)
        .collect()
}

fn joint_nig_basin_restart_seeds(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    return_component_basin_diagnostics: &[ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    activity_log_shift: f64,
) -> Vec<(String, Vec<f64>)> {
    return_component_basin_diagnostics
        .iter()
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
) -> Vec<(String, Vec<f64>)> {
    return_component_basin_diagnostics
        .iter()
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

pub(super) fn cached_seed_summary_for_candidate_seed(
    parameter_bits: Option<&[u64]>,
    fitted_parameters: &JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
    simulation_seed: u64,
) -> Option<JointRefinementSeedSummary> {
    let cache_key = parameter_bits.map(|bits| JointRefinementSeedSummaryCacheKey {
        parameter_bits: bits.to_vec(),
        simulation_seed,
    });
    if let Some(seed_cache_key) = cache_key.as_ref()
        && let Ok(cache_guard) = context.seed_summary_cache.lock()
        && let Some(cached_summary) = cache_guard.get(seed_cache_key)
    {
        return cached_summary.clone();
    }

    let seed_summary = build_seed_summary_for_candidate_seed(
        &context.calendar_registry,
        context,
        fitted_parameters,
        simulation_seed,
    );
    if let Some(seed_cache_key) = cache_key
        && let Ok(mut cache_guard) = context.seed_summary_cache.lock()
    {
        cache_guard.insert(seed_cache_key, seed_summary.clone());
    }

    seed_summary
}

pub(super) fn build_surprise_multipliers_by_hour_index(
    hourly_simulation_start_utc: DateTime<Utc>,
    hourly_simulation_interval_count: usize,
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
) -> Vec<(u64, f64)> {
    build_surprise_multipliers_for_interval_grid(
        hourly_simulation_start_utc,
        hourly_simulation_interval_count,
        ONE_HOUR_NANOS,
        return_components_fitted_parameters
            .surprise_deterministic_intensity_profile
            .weekly_hour_utc_multipliers
            .as_slice(),
    )
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

pub(super) fn build_seed_summary_for_candidate_seed(
    calendar_registry: &ExchangeCalendarRegistry,
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
) -> Option<JointRefinementSeedSummary> {
    let surprise_deterministic_time_multiplier_by_interval_index =
        build_surprise_multipliers_for_interval_grid(
            context.observed_data.hourly_simulation_start_utc,
            context.observed_data.hourly_simulation_interval_count,
            ONE_HOUR_NANOS,
            context
                .observed_data
                .surprise_weekly_hour_utc_multipliers
                .as_slice(),
        );

    let hourly_factor_contract = load_hourly_factor_contract_for_calibration(
        calendar_registry,
        HourlyFactorProviderRequest {
            exchange_code: context.exchange_code.as_str(),
            range_start_utc: context.observed_data.hourly_simulation_start_utc,
            range_end_utc: context.observed_data.hourly_simulation_end_utc,
            interval_duration_nanos: ONE_HOUR_NANOS,
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: context.runtime_hourly_seasonality_factors.as_slice(),
                materialization_method:
                    "runtime_compact_seasonality_parameters_mean_renormalized.v1",
            }),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Materialized {
                known_scheduled_event_timestamps_utc_nanos: fitted_parameters
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .known
                    .scheduled_event_timestamps_utc_nanos
                    .as_slice(),
                surprise_baseline_intensity: fitted_parameters
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .surprise
                    .baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index:
                    surprise_deterministic_time_multiplier_by_interval_index.as_slice(),
            }),
        },
    )
    .ok()?;

    let simulation_result = run_seeded_tick_event_simulation(
        SeededTickSimulationRunConfig::try_new(
            simulation_seed,
            context.observed_data.hourly_simulation_start_utc,
            context.observed_data.hourly_simulation_interval_count,
            SeededTickSimulationModelInputs {
                kernel_parameters: fitted_parameters.hawkes_fitted_parameters.kernel_parameters,
                nig_shared_plus_shift_configuration: fitted_parameters
                    .return_components_fitted_parameters
                    .nig_shared_plus_shift_configuration,
                news_process_configuration: fitted_parameters
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .clone(),
                initial_excitation_state: HawkesExcitationState {
                    y_plus_plus: 0.0,
                    y_plus_minus: 0.0,
                    y_minus_plus: 0.0,
                    y_minus_minus: 0.0,
                },
                mean_reversion_fitted_parameters:
                    MeanReversionFittedParameters::endogenous_core_expectation_one_interval(0.0)
                        .expect("zero-kappa endogenous mean reversion should validate"),
                mean_reversion_runtime_interval_anchors:
                    MeanReversionRuntimeIntervalAnchors::try_new(
                        hourly_factor_contract.metadata.interval_alignment_policy,
                        0.0,
                        vec![0.0; context.observed_data.hourly_simulation_interval_count],
                        vec![
                            hourly_factor_contract.interval_duration_nanos as f64
                                / ONE_HOUR_NANOS as f64;
                            context.observed_data.hourly_simulation_interval_count
                        ],
                    )
                    .ok()?,
                initial_mean_reversion_boundary_state: MeanReversionBoundaryState::try_new(
                    0.0, 0.0, 0.0,
                )
                .ok()?,
                hourly_factor_contract,
                calendar_registry: calendar_registry.clone(),
            },
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: false,
                debug_trace_options: Default::default(),
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .ok()?,
    )
    .ok()?;

    let (simulated_5m_intervals, simulated_15m_intervals) =
        crate::stages::joint_metrics::aggregate_simulated_path_to_interval_templates(
            simulation_result.fine_path.as_slice(),
            context
                .observed_data
                .simulated_5m_interval_template
                .as_slice(),
            context
                .observed_data
                .simulated_15m_interval_template
                .as_slice(),
            FIVE_MINUTES_NANOS,
            FIFTEEN_MINUTES_NANOS,
        );

    let simulated_hourly_intervals =
        crate::stages::joint_metrics::aggregate_simulated_path_to_interval_template(
            simulation_result.fine_path.as_slice(),
            context
                .observed_data
                .simulated_hourly_interval_template
                .as_slice(),
            ONE_HOUR_NANOS,
        );

    let simulated_5m_log_returns = simulated_5m_intervals
        .iter()
        .map(|interval| interval.log_return)
        .collect::<Vec<_>>();
    let simulated_15m_log_returns = simulated_15m_intervals
        .iter()
        .map(|interval| interval.log_return)
        .collect::<Vec<_>>();
    let simulated_tick_returns = sampled_trade_returns(
        simulation_result.fine_path.as_slice(),
        context.observed_data.observed_tick_shape_returns.len(),
    );

    let simulated_tick_shape_returns = scale_normalized_returns(simulated_tick_returns.as_slice());
    let simulated_5m_shape_returns = scale_normalized_returns(simulated_5m_log_returns.as_slice());
    let simulated_15m_shape_returns =
        scale_normalized_returns(simulated_15m_log_returns.as_slice());

    let simulated_tick_return_scale_summary =
        return_scale_summary_from_returns(simulated_tick_returns.as_slice());
    let simulated_5m_return_scale_summary = return_scale_summary(simulated_5m_intervals.as_slice());
    let simulated_15m_return_scale_summary =
        return_scale_summary(simulated_15m_intervals.as_slice());

    let simulated_5m_business_time_return_scale_summaries =
        return_scale_summaries_by_business_time_bucket(
            simulated_5m_intervals.as_slice(),
            context
                .observed_data
                .observed_5m_business_time_bucket_thresholds,
        );
    let simulated_15m_business_time_return_scale_summaries =
        return_scale_summaries_by_business_time_bucket(
            simulated_15m_intervals.as_slice(),
            context
                .observed_data
                .observed_15m_business_time_bucket_thresholds,
        );

    let simulated_5m_intraday_mean_absolute_returns =
        mean_absolute_return_by_utc_hour(simulated_5m_intervals.as_slice());
    let simulated_15m_intraday_mean_absolute_returns =
        mean_absolute_return_by_utc_hour(simulated_15m_intervals.as_slice());
    let simulated_tick_intraday_mean_absolute_returns =
        mean_absolute_trade_return_by_utc_hour(simulation_result.fine_path.as_slice());
    let simulated_hourly_tick_counts = tick_count_series(simulated_hourly_intervals.as_slice());
    let simulated_15m_rv_activity_metric =
        rolling_variance_activity_metric(simulated_15m_intervals.as_slice());

    Some(JointRefinementSeedSummary {
        simulated_5m_intervals,
        simulated_15m_intervals,
        simulated_tick_shape_returns,
        simulated_5m_shape_returns,
        simulated_15m_shape_returns,
        simulated_tick_return_scale_summary,
        simulated_5m_return_scale_summary,
        simulated_15m_return_scale_summary,
        simulated_5m_business_time_return_scale_summaries,
        simulated_15m_business_time_return_scale_summaries,
        simulated_tick_intraday_mean_absolute_returns,
        simulated_5m_intraday_mean_absolute_returns,
        simulated_15m_intraday_mean_absolute_returns,
        simulated_hourly_tick_counts,
        simulated_15m_rv_activity_metric,
    })
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
