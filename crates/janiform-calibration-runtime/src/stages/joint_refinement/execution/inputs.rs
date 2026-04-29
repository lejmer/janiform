use chrono::{DateTime, Utc};
use janiform_foundations::time::{
    NANOS_PER_FIFTEEN_MINUTES, NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR, ceil_to_hour_utc_nanos,
    floor_to_hour_utc_nanos,
};

use crate::stages::joint_metrics::{
    JointRefinementBusinessTimeBucketThresholds, build_observed_open_session_interval_observations,
    business_time_bucket_thresholds, mean_absolute_observed_return_by_utc_hour,
    mean_absolute_return_by_utc_hour, return_scale_summaries_by_business_time_bucket,
    return_scale_summary, return_scale_summary_from_returns, rolling_variance_activity_metric,
    sampled_return_values, scale_normalized_returns, tick_count_series,
    zeroed_interval_observation_template,
};
use crate::stages::return_components::{
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageJointRefinementObservedSignalPreparation,
    continuous_return_calibration_sample_observed_returns,
};
use janiform_calibration_artifacts::ReturnComponentsStageFittedParameters;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::super::setup::{
    build_surprise_multipliers_for_interval_grid, compute_cumulative_log_price_span,
};
use super::super::summary_simulation::JointRefinementSummarySimulationSessionContext;
use super::{JointRefinementObservedData, build_exchange_calendar_registry_artifact};

pub(crate) fn build_observed_data(
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
        NANOS_PER_FIVE_MINUTES,
    );
    let observed_15m_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        NANOS_PER_FIFTEEN_MINUTES,
    );
    let observed_hourly_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        NANOS_PER_HOUR,
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
    .div_euclid(NANOS_PER_HOUR)) as usize;

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

pub(crate) fn build_summary_simulation_session_context(
    calendar_registry: &ExchangeCalendarRegistry,
    context: &JointRefinementObservedData,
    exchange_code: &str,
    known_scheduled_event_timestamps_utc_nanos: &[i64],
    runtime_hourly_seasonality_factors: &[f64],
) -> Result<JointRefinementSummarySimulationSessionContext, String> {
    let calendar_registry_artifact = build_exchange_calendar_registry_artifact(
        calendar_registry,
        exchange_code,
        context.hourly_simulation_start_utc,
    )
    .ok_or_else(|| "failed to build exchange calendar registry artifact".to_string())?;

    Ok(JointRefinementSummarySimulationSessionContext {
        calendar_registry_artifact,
        exchange_code: exchange_code.to_string(),
        known_scheduled_event_timestamps_utc_nanos: known_scheduled_event_timestamps_utc_nanos
            .to_vec(),
        runtime_hourly_seasonality_factors: runtime_hourly_seasonality_factors.to_vec(),
        hourly_simulation_start_utc_nanos: context
            .hourly_simulation_start_utc
            .timestamp_nanos_opt()
            .ok_or_else(|| {
                "joint refinement hourly simulation start timestamp is out of range".to_string()
            })?,
        hourly_simulation_interval_count: context.hourly_simulation_interval_count,
        surprise_weekly_hour_utc_multipliers: context.surprise_weekly_hour_utc_multipliers.clone(),
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

fn build_surprise_multipliers_by_hour_index(
    hourly_simulation_start_utc: DateTime<Utc>,
    hourly_simulation_interval_count: usize,
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
) -> Vec<(u64, f64)> {
    build_surprise_multipliers_for_interval_grid(
        hourly_simulation_start_utc,
        hourly_simulation_interval_count,
        NANOS_PER_HOUR,
        return_components_fitted_parameters
            .surprise_deterministic_intensity_profile
            .weekly_hour_utc_multipliers
            .as_slice(),
    )
}
