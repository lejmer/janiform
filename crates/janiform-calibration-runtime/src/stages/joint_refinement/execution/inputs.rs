use chrono::{DateTime, Utc};
use janiform_foundations::time::{
    NANOS_PER_DAY, NANOS_PER_FIFTEEN_MINUTES, NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR,
    NANOS_PER_WEEK, checked_ceil_to_hour_utc_nanos, checked_floor_to_hour_utc_nanos,
    checked_interval_count_between_utc_nanos,
};

use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::market_data::preparation::CalibrationTickObjectiveDataUsagePolicy;
use crate::stages::joint_metrics::{
    JointRefinementBusinessTimeBucketThresholds, build_observed_open_session_interval_observations,
    business_time_bucket_thresholds, interval_log_returns,
    mean_absolute_observed_return_by_utc_hour, mean_absolute_return_by_utc_hour,
    return_scale_summaries_by_business_time_bucket, return_scale_summary,
    return_scale_summary_from_returns, rolling_variance_activity_metric, sampled_return_values,
    scale_normalized_returns, tick_count_series, zeroed_interval_observation_template,
};
use crate::stages::long_variance::covariance::{supports_persistence_fit, variance_covariances};
use crate::stages::long_variance::metrics::{
    LongVarianceHorizonInputIntervals, aggregate_intervals_to_fixed_duration,
    long_variance_horizon_summaries,
};
use crate::stages::long_variance::observations::hourly_realized_variance;
use crate::stages::long_variance::source::select_long_variance_observed_intervals;
use crate::stages::return_components::{
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageContinuousReturnCalibrationSample,
    ReturnComponentsStageJointRefinementObservedSignalPreparation,
    build_continuous_return_optimizer_context, build_weighted_continuous_return_optimizer_context,
    rescale_continuous_return_calibration_sample_business_time_scales,
};
use crate::stages::tick_objective_data::{
    retained_open_session_returns_for_tick_objectives, source_window_for_retained_tick_objectives,
};
use janiform_calibration_artifacts::ReturnComponentsStageFittedParameters;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::super::summary_simulation::JointRefinementSummarySimulationSessionContext;
use super::{JointRefinementObservedData, build_exchange_calendar_registry_artifact};

// The joint return-shape objective checks q01/q99, so one extreme-tail rank
// quantum is 1 / 0.01 observations per temporal block.
const RETURN_SHAPE_EXTREME_TAIL_RANK_DENOMINATOR: usize = 100;

pub(crate) struct JointRefinementObservedDataBuildInput<'a> {
    pub(crate) long_variance_fit:
        Option<&'a janiform_calibration_artifacts::VariancePersistenceFit>,
    pub(crate) calendar_registry: &'a ExchangeCalendarRegistry,
    pub(crate) exchange_code: &'a str,
    pub(crate) return_components_fitted_parameters: &'a ReturnComponentsStageFittedParameters,
    pub(crate) observed_signal_preparation:
        &'a ReturnComponentsStageJointRefinementObservedSignalPreparation,
    pub(crate) candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    pub(crate) coarse_data_records: Option<&'a [MarketDataNormalizedCoarseDataRecord]>,
    pub(crate) tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy,
    pub(crate) observed_open_event_rate_per_hour: f64,
}

pub(crate) fn build_observed_data(
    input: JointRefinementObservedDataBuildInput<'_>,
) -> Result<JointRefinementObservedData, String> {
    let JointRefinementObservedDataBuildInput {
        long_variance_fit,
        calendar_registry,
        exchange_code,
        return_components_fitted_parameters,
        observed_signal_preparation,
        candidate_generation_input,
        coarse_data_records,
        tick_objective_data_usage_policy,
        observed_open_event_rate_per_hour,
    } = input;

    let observed_returns = retained_open_session_returns_for_tick_objectives(
        candidate_generation_input,
        tick_objective_data_usage_policy,
    );

    let source_window = source_window_for_retained_tick_objectives(
        candidate_generation_input,
        observed_returns.as_slice(),
    );

    let observed_5m_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        observed_returns.as_slice(),
        NANOS_PER_FIVE_MINUTES,
    );

    let observed_15m_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        observed_returns.as_slice(),
        NANOS_PER_FIFTEEN_MINUTES,
    );

    let tick_hourly_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        observed_returns.as_slice(),
        NANOS_PER_HOUR,
    );

    let observed_1d_intervals =
        aggregate_intervals_to_fixed_duration(tick_hourly_intervals.as_slice(), NANOS_PER_DAY);

    let observed_1w_intervals =
        aggregate_intervals_to_fixed_duration(tick_hourly_intervals.as_slice(), NANOS_PER_WEEK);

    let observed_long_variance_intervals = select_long_variance_observed_intervals(
        tick_hourly_intervals.as_slice(),
        coarse_data_records,
    );

    // Only active factors need simulated covariance targets. The decay profile is
    // carried from the long-variance stage and does not require another history scan.
    let mut observed_variance_covariances = if return_components_fitted_parameters
        .long_variance_parameters
        .is_neutral()
    {
        Vec::new()
    } else {
        variance_covariances(&hourly_realized_variance(&observed_5m_intervals), true)
    };

    if !supports_persistence_fit(&observed_variance_covariances) {
        observed_variance_covariances.clear();
    }

    // Scalar evidence is copied so the observed-data context owns its lifetime.
    let variance_persistence_fit = long_variance_fit.cloned();

    let observed_long_variance_horizon_summaries =
        long_variance_horizon_summaries(LongVarianceHorizonInputIntervals {
            five_minute_return_intervals: observed_5m_intervals.as_slice(),
            five_minute_activity_intervals: observed_5m_intervals.as_slice(),
            hourly_return_intervals: observed_long_variance_intervals.return_intervals.as_slice(),
            hourly_activity_intervals: observed_long_variance_intervals
                .activity_intervals
                .as_slice(),
        });

    let hourly_simulation_start_utc_nanos =
        checked_floor_to_hour_utc_nanos(source_window.window_start_utc_nanos).ok_or_else(|| {
            "joint refinement source window start cannot be rounded down to an hourly boundary"
                .to_string()
        })?;

    let hourly_simulation_end_utc_nanos =
        checked_ceil_to_hour_utc_nanos(source_window.window_end_utc_nanos).ok_or_else(|| {
            "joint refinement source window end cannot be rounded up to an hourly boundary"
                .to_string()
        })?;

    let hourly_simulation_interval_count = checked_interval_count_between_utc_nanos(
        hourly_simulation_start_utc_nanos,
        hourly_simulation_end_utc_nanos,
        NANOS_PER_HOUR,
    )
    .ok_or_else(|| {
        "joint refinement source window does not define a valid hourly simulation grid".to_string()
    })?;

    let hourly_simulation_start_utc =
        DateTime::<Utc>::from_timestamp_nanos(hourly_simulation_start_utc_nanos);

    let hourly_simulation_end_utc =
        DateTime::<Utc>::from_timestamp_nanos(hourly_simulation_end_utc_nanos);

    let observed_15m_rv_activity_metric =
        rolling_variance_activity_metric(observed_15m_intervals.as_slice());

    let observed_5m_log_returns = interval_log_returns(observed_5m_intervals.as_slice());
    let observed_15m_log_returns = interval_log_returns(observed_15m_intervals.as_slice());
    let observed_1h_log_returns = interval_log_returns(tick_hourly_intervals.as_slice());
    let observed_1d_log_returns = interval_log_returns(observed_1d_intervals.as_slice());
    let observed_1w_log_returns = interval_log_returns(observed_1w_intervals.as_slice());

    let observed_tick_available_sample_count = observed_returns.len();

    let observed_tick_sample_count = tick_shape_sample_target_count(
        observed_tick_available_sample_count,
        hourly_simulation_interval_count,
    );

    let observed_tick_returns =
        sampled_observed_tick_returns(observed_returns.as_slice(), observed_tick_sample_count);

    let observed_tick_shape_returns = scale_normalized_returns(observed_tick_returns.as_slice());
    let observed_5m_shape_returns = scale_normalized_returns(observed_5m_log_returns.as_slice());
    let observed_15m_shape_returns = scale_normalized_returns(observed_15m_log_returns.as_slice());
    let observed_1h_shape_returns = scale_normalized_returns(observed_1h_log_returns.as_slice());
    let observed_1d_shape_returns = scale_normalized_returns(observed_1d_log_returns.as_slice());
    let observed_1w_shape_returns = scale_normalized_returns(observed_1w_log_returns.as_slice());

    let observed_tick_return_scale_summary =
        return_scale_summary_from_returns(observed_tick_returns.as_slice());

    let observed_1h_return_scale_summary = return_scale_summary(tick_hourly_intervals.as_slice());
    let observed_1d_return_scale_summary = return_scale_summary(observed_1d_intervals.as_slice());

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

    let observed_tick_intraday_mean_absolute_returns =
        mean_absolute_observed_return_by_utc_hour(observed_returns.as_slice());

    let observed_hourly_tick_counts = tick_count_series(tick_hourly_intervals.as_slice());

    let observed_tick_continuous_return_calibration_sample = observed_signal_preparation
        .continuous_return_calibration_sample
        .clone();

    let observed_tick_continuous_return_optimizer_context =
        build_continuous_return_optimizer_context(
            &observed_tick_continuous_return_calibration_sample,
        );

    let weighted_tick_continuous_return_optimizer_context =
        build_weighted_continuous_return_optimizer_context(
            &observed_tick_continuous_return_calibration_sample,
        );

    let simulated_hourly_interval_template =
        zeroed_interval_observation_template(tick_hourly_intervals.as_slice());

    Ok(JointRefinementObservedData {
        observed_open_event_rate_per_hour,
        fallback_sell_share_proxy: candidate_generation_input.open_session_sell_share_proxy,
        observed_tick_continuous_return_calibration_sample,
        observed_tick_continuous_return_optimizer_context,
        weighted_tick_continuous_return_optimizer_context,
        observed_5m_intervals: observed_5m_intervals.clone(),
        observed_15m_intervals: observed_15m_intervals.clone(),
        observed_1h_intervals: tick_hourly_intervals,
        observed_1d_intervals,
        observed_tick_intraday_mean_absolute_returns,
        observed_tick_shape_returns,
        observed_5m_shape_returns,
        observed_15m_shape_returns,
        observed_1h_shape_returns,
        observed_1d_shape_returns,
        observed_1w_shape_returns,
        observed_tick_return_scale_summary,
        observed_1h_return_scale_summary,
        observed_1d_return_scale_summary,
        observed_5m_business_time_bucket_thresholds,
        observed_15m_business_time_bucket_thresholds,
        observed_5m_business_time_return_scale_summaries,
        observed_15m_business_time_return_scale_summaries,
        observed_5m_intraday_mean_absolute_returns,
        observed_15m_intraday_mean_absolute_returns,
        observed_hourly_tick_counts,
        observed_15m_rv_activity_metric,
        observed_variance_covariances,
        variance_persistence_fit,
        observed_long_variance_horizon_summaries,
        hourly_simulation_start_utc,
        hourly_simulation_end_utc,
        hourly_simulation_interval_count,
        surprise_deterministic_intensity_profile: return_components_fitted_parameters
            .surprise_deterministic_intensity_profile
            .clone(),
        simulated_5m_interval_template: zeroed_interval_observation_template(
            observed_5m_intervals.as_slice(),
        ),
        simulated_15m_interval_template: zeroed_interval_observation_template(
            observed_15m_intervals.as_slice(),
        ),
        simulated_hourly_interval_template,
    })
}

pub(crate) fn sampled_observed_tick_returns(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    target_count: usize,
) -> Vec<f64> {
    let raw_observed_tick_returns = observed_open_session_returns
        .iter()
        .map(|observation| observation.log_return)
        .collect::<Vec<_>>();

    sampled_return_values(raw_observed_tick_returns.as_slice(), target_count)
}

pub(crate) fn tick_shape_sample_target_count(
    available_tick_return_count: usize,
    temporal_block_count: usize,
) -> usize {
    available_tick_return_count.min(
        temporal_block_count
            .max(1)
            .saturating_mul(RETURN_SHAPE_EXTREME_TAIL_RANK_DENOMINATOR),
    )
}

pub(crate) fn rescaled_observed_tick_calibration_sample_for_modeled_rate(
    observed_data: &JointRefinementObservedData,
    baseline_modeled_open_event_rate_per_hour: f64,
    target_modeled_open_event_rate_per_hour: f64,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    rescale_continuous_return_calibration_sample_business_time_scales(
        &observed_data.observed_tick_continuous_return_calibration_sample,
        target_modeled_open_event_rate_per_hour / baseline_modeled_open_event_rate_per_hour,
    )
}

pub(crate) fn build_summary_simulation_session_context(
    calendar_registry: &ExchangeCalendarRegistry,
    context: &JointRefinementObservedData,
    exchange_code: &str,
    known_scheduled_event_timestamps_utc_nanos: &[i64],
    runtime_interval_seasonality_factors: &[f64],
) -> Result<JointRefinementSummarySimulationSessionContext, String> {
    let calendar_registry_artifact = build_exchange_calendar_registry_artifact(
        calendar_registry,
        exchange_code,
        context.hourly_simulation_start_utc,
    )
    .ok_or_else(|| "failed to build exchange calendar registry artifact".to_string())?;

    Ok(JointRefinementSummarySimulationSessionContext {
        variance_persistence_enabled: !context.observed_variance_covariances.is_empty(),
        calendar_registry_artifact,
        exchange_code: exchange_code.to_string(),
        known_scheduled_event_timestamps_utc_nanos: known_scheduled_event_timestamps_utc_nanos
            .to_vec(),
        runtime_interval_seasonality_factors: runtime_interval_seasonality_factors.to_vec(),
        hourly_simulation_start_utc_nanos: context
            .hourly_simulation_start_utc
            .timestamp_nanos_opt()
            .ok_or_else(|| {
                "joint refinement hourly simulation start timestamp is out of range".to_string()
            })?,
        hourly_simulation_interval_count: context.hourly_simulation_interval_count,
        surprise_deterministic_intensity_profile: context
            .surprise_deterministic_intensity_profile
            .clone(),
        simulated_5m_interval_template: context.simulated_5m_interval_template.clone(),
        simulated_15m_interval_template: context.simulated_15m_interval_template.clone(),
        simulated_hourly_interval_template: context.simulated_hourly_interval_template.clone(),
        observed_5m_business_time_bucket_thresholds: context
            .observed_5m_business_time_bucket_thresholds,
        observed_15m_business_time_bucket_thresholds: context
            .observed_15m_business_time_bucket_thresholds,
        observed_tick_shape_target_count: context.observed_tick_shape_returns.len(),
    })
}
