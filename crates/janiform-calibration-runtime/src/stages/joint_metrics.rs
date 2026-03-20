use chrono::{DateTime, Utc};

use crate::calendar::{ExchangeCalendarRegistry, MarketState};
use crate::stages::return_components::{
    ReturnComponentsObservedOpenSessionReturn,
    ReturnComponentsStageContinuousReturnCalibrationSample,
    ReturnComponentsStageSignalSourceWindow,
    build_continuous_return_calibration_sample_from_components,
};
use janiform_engine_simulation::{SeededFinePathRecord, SeededFinePathTransitionKind};
use janiform_foundations::numeric::{
    arithmetic_mean_or_zero, compute_finite_quantile_or_zero, pearson_correlation,
    standardized_path_distance,
};

pub(crate) const JOINT_REFINEMENT_ROLLING_ACTIVITY_WINDOW_INTERVAL_COUNT: usize = 96;
const JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR: f64 = 1.0e-8;
const JOINT_REFINEMENT_RETURN_SCALE_QUANTILE_Q90: f64 = 0.90;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct JointRefinementRollingVarianceActivityMetric {
    pub(crate) correlation: f64,
    pub(crate) standardized_path_distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct JointRefinementReturnScaleSummary {
    pub(crate) mean_absolute_return: f64,
    pub(crate) realized_variance: f64,
    pub(crate) absolute_return_quantile_q90: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct JointRefinementAggregatedIntervalObservation {
    pub(crate) interval_start_utc_nanos: i64,
    pub(crate) interval_end_utc_nanos: i64,
    pub(crate) log_return: f64,
    pub(crate) tick_count: usize,
    pub(crate) sell_share_proxy: f64,
}

pub(crate) fn build_observed_open_session_interval_observations(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    source_window: ReturnComponentsStageSignalSourceWindow,
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    interval_duration_nanos: i64,
) -> Vec<JointRefinementAggregatedIntervalObservation> {
    let mut intervals = build_open_session_interval_template(
        calendar_registry,
        exchange_code,
        source_window,
        interval_duration_nanos,
    );
    if intervals.is_empty() {
        return intervals;
    }

    let aligned_window_start = intervals
        .first()
        .map(|interval| interval.interval_start_utc_nanos)
        .unwrap_or(source_window.window_start_utc_nanos);

    for observation in observed_open_session_returns {
        if observation.return_timestamp_utc_nanos < source_window.window_start_utc_nanos
            || observation.return_timestamp_utc_nanos >= source_window.window_end_utc_nanos
        {
            continue;
        }
        let interval_index = ((observation.return_timestamp_utc_nanos - aligned_window_start)
            .div_euclid(interval_duration_nanos)) as usize;
        let Some(interval) = intervals.get_mut(interval_index) else {
            continue;
        };
        interval.log_return += observation.log_return;
        interval.tick_count += 1;
        interval.sell_share_proxy += observation.sell_share_proxy;
    }

    finalize_sell_share_proxies(&mut intervals, 0.5);
    intervals
}

pub(crate) fn zeroed_interval_observation_template(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<JointRefinementAggregatedIntervalObservation> {
    intervals
        .iter()
        .map(|interval| JointRefinementAggregatedIntervalObservation {
            interval_start_utc_nanos: interval.interval_start_utc_nanos,
            interval_end_utc_nanos: interval.interval_end_utc_nanos,
            log_return: 0.0,
            tick_count: 0,
            sell_share_proxy: 0.0,
        })
        .collect()
}

pub(crate) fn aggregate_simulated_path_to_interval_templates(
    fine_path: &[SeededFinePathRecord],
    five_minute_template: &[JointRefinementAggregatedIntervalObservation],
    fifteen_minute_template: &[JointRefinementAggregatedIntervalObservation],
    five_minute_interval_duration_nanos: i64,
    fifteen_minute_interval_duration_nanos: i64,
) -> (
    Vec<JointRefinementAggregatedIntervalObservation>,
    Vec<JointRefinementAggregatedIntervalObservation>,
) {
    (
        aggregate_simulated_path_to_interval_template(
            fine_path,
            five_minute_template,
            five_minute_interval_duration_nanos,
        ),
        aggregate_simulated_path_to_interval_template(
            fine_path,
            fifteen_minute_template,
            fifteen_minute_interval_duration_nanos,
        ),
    )
}

pub(crate) fn aggregate_simulated_path_to_interval_template(
    fine_path: &[SeededFinePathRecord],
    template: &[JointRefinementAggregatedIntervalObservation],
    interval_duration_nanos: i64,
) -> Vec<JointRefinementAggregatedIntervalObservation> {
    let mut intervals = template.to_vec();
    if intervals.is_empty() {
        return intervals;
    }

    let aligned_start = intervals
        .first()
        .map(|interval| interval.interval_start_utc_nanos)
        .unwrap_or(0);
    for record in fine_path {
        accumulate_fine_path_record_into_intervals(
            record,
            intervals.as_mut_slice(),
            aligned_start,
            interval_duration_nanos,
        );
    }

    finalize_sell_share_proxies(&mut intervals, 0.5);
    intervals
}

pub(crate) fn build_continuous_calibration_sample_from_aggregated_intervals(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    modeled_open_event_rate_per_hour: f64,
    observed_open_event_rate_per_hour: f64,
    fallback_sell_share_proxy: f64,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    let observed_rate_floor = observed_open_event_rate_per_hour.max(1.0e-8);
    let scale_ratio = modeled_open_event_rate_per_hour.max(1.0e-8) / observed_rate_floor;
    let observed_returns = intervals
        .iter()
        .map(|interval| interval.log_return)
        .collect();
    let business_time_scales = intervals
        .iter()
        .map(|interval| ((interval.tick_count as f64).max(1.0e-8)) * scale_ratio)
        .collect();
    let sell_share_proxies = intervals
        .iter()
        .map(|interval| {
            if interval.tick_count == 0 {
                fallback_sell_share_proxy
            } else {
                interval.sell_share_proxy
            }
        })
        .collect();

    build_continuous_return_calibration_sample_from_components(
        observed_returns,
        business_time_scales,
        sell_share_proxies,
        fallback_sell_share_proxy,
    )
}

pub(crate) fn rolling_variance_activity_metric(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> JointRefinementRollingVarianceActivityMetric {
    if intervals.is_empty() {
        return JointRefinementRollingVarianceActivityMetric {
            correlation: f64::NAN,
            standardized_path_distance: f64::INFINITY,
        };
    }

    let tick_counts: Vec<f64> = intervals
        .iter()
        .map(|interval| interval.tick_count as f64)
        .collect();
    let squared_returns: Vec<f64> = intervals
        .iter()
        .map(|interval| interval.log_return * interval.log_return)
        .collect();
    let rolling_activity = rolling_sum(
        tick_counts.as_slice(),
        JOINT_REFINEMENT_ROLLING_ACTIVITY_WINDOW_INTERVAL_COUNT,
    );
    let rolling_realized_variance = rolling_sum(
        squared_returns.as_slice(),
        JOINT_REFINEMENT_ROLLING_ACTIVITY_WINDOW_INTERVAL_COUNT,
    );

    let correlation = pearson_correlation(
        rolling_activity.as_slice(),
        rolling_realized_variance.as_slice(),
    );
    let standardized_path_distance = standardized_path_distance(
        rolling_activity.as_slice(),
        rolling_realized_variance.as_slice(),
    );

    JointRefinementRollingVarianceActivityMetric {
        correlation,
        standardized_path_distance,
    }
}

pub(crate) fn return_scale_summary(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> JointRefinementReturnScaleSummary {
    let absolute_returns: Vec<f64> = intervals
        .iter()
        .map(|interval| interval.log_return.abs())
        .collect();
    let realized_variance = intervals
        .iter()
        .map(|interval| interval.log_return * interval.log_return)
        .sum::<f64>();

    JointRefinementReturnScaleSummary {
        mean_absolute_return: arithmetic_mean_or_zero(absolute_returns.as_slice()),
        realized_variance,
        absolute_return_quantile_q90: compute_finite_quantile_or_zero(
            absolute_returns.as_slice(),
            JOINT_REFINEMENT_RETURN_SCALE_QUANTILE_Q90,
        ),
    }
}

pub(crate) fn tick_count_series(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<f64> {
    intervals
        .iter()
        .map(|interval| interval.tick_count as f64)
        .collect()
}

pub(crate) fn scale_normalized_returns(returns: &[f64]) -> Vec<f64> {
    let absolute_returns: Vec<f64> = returns
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(f64::abs)
        .collect();
    let scale = arithmetic_mean_or_zero(absolute_returns.as_slice())
        .max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR);

    returns
        .iter()
        .map(|value| {
            if value.is_finite() {
                *value / scale
            } else {
                0.0
            }
        })
        .collect()
}

pub(crate) fn normalized_profile_distance(left: &[f64], right: &[f64]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return f64::INFINITY;
    }

    let left_sum = left
        .iter()
        .sum::<f64>()
        .abs()
        .max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR);
    let right_sum = right
        .iter()
        .sum::<f64>()
        .abs()
        .max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR);

    left.iter()
        .zip(right.iter())
        .map(|(left_value, right_value)| {
            ((*left_value / left_sum) - (*right_value / right_sum)).abs()
        })
        .sum::<f64>()
        / left.len() as f64
}

pub(crate) fn relative_rmse(left: &[f64], right: &[f64], floor: f64) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return f64::INFINITY;
    }

    let squared_error_sum = left
        .iter()
        .zip(right.iter())
        .map(|(left_value, right_value)| {
            let error = left_value - right_value;
            error * error
        })
        .sum::<f64>();
    let rmse = (squared_error_sum / left.len() as f64).sqrt();
    let scale = arithmetic_mean_or_zero(right).abs().max(floor);

    rmse / scale
}

pub(crate) fn log_ratio_distance(candidate_value: f64, observed_value: f64, floor: f64) -> f64 {
    let safe_floor = floor.abs().max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR);
    let candidate = candidate_value.abs().max(safe_floor);
    let observed = observed_value.abs().max(safe_floor);

    (candidate / observed).ln().abs()
}

pub(crate) fn zero_aware_normalized_bucket_value(
    candidate_value: f64,
    baseline_value: f64,
    floor: f64,
) -> f64 {
    let denominator = baseline_value.abs().max(floor);
    candidate_value / denominator
}

fn build_open_session_interval_template(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    source_window: ReturnComponentsStageSignalSourceWindow,
    interval_duration_nanos: i64,
) -> Vec<JointRefinementAggregatedIntervalObservation> {
    if interval_duration_nanos <= 0 {
        return Vec::new();
    }

    let aligned_start = source_window
        .window_start_utc_nanos
        .div_euclid(interval_duration_nanos)
        * interval_duration_nanos;
    let aligned_end = ceil_to_interval(source_window.window_end_utc_nanos, interval_duration_nanos);
    let mut intervals = Vec::new();
    let mut interval_start_utc_nanos = aligned_start;
    while interval_start_utc_nanos < aligned_end {
        let interval_end_utc_nanos =
            interval_start_utc_nanos.saturating_add(interval_duration_nanos);
        let midpoint_utc_nanos =
            interval_start_utc_nanos.saturating_add(interval_duration_nanos / 2);
        if resolve_market_state(calendar_registry, exchange_code, midpoint_utc_nanos)
            == Some(MarketState::Open)
        {
            intervals.push(JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos,
                interval_end_utc_nanos,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            });
        }
        interval_start_utc_nanos = interval_end_utc_nanos;
    }
    intervals
}

fn finalize_sell_share_proxies(
    intervals: &mut [JointRefinementAggregatedIntervalObservation],
    fallback_sell_share_proxy: f64,
) {
    for interval in intervals {
        interval.sell_share_proxy = if interval.tick_count == 0 {
            fallback_sell_share_proxy
        } else {
            (interval.sell_share_proxy / interval.tick_count as f64).clamp(0.0, 1.0)
        };
    }
}

fn accumulate_fine_path_record_into_intervals(
    record: &SeededFinePathRecord,
    intervals: &mut [JointRefinementAggregatedIntervalObservation],
    aligned_window_start: i64,
    interval_duration_nanos: i64,
) {
    if intervals.is_empty() || interval_duration_nanos <= 0 {
        return;
    }

    let Some(last_interval_end_utc_nanos) = intervals
        .last()
        .map(|interval| interval.interval_end_utc_nanos)
    else {
        return;
    };
    if record.timestamp_utc_nanos < aligned_window_start
        || record.timestamp_utc_nanos >= last_interval_end_utc_nanos
    {
        return;
    }

    let interval_index = ((record.timestamp_utc_nanos - aligned_window_start)
        .div_euclid(interval_duration_nanos)) as usize;
    let Some(interval) = intervals.get_mut(interval_index) else {
        return;
    };

    interval.log_return += record.return_components.total_return;

    if matches!(
        record.transition_kind,
        SeededFinePathTransitionKind::BuyTrade | SeededFinePathTransitionKind::SellTrade
    ) {
        interval.tick_count += 1;
        interval.sell_share_proxy += match record.transition_kind {
            SeededFinePathTransitionKind::BuyTrade => 0.0,
            SeededFinePathTransitionKind::SellTrade => 1.0,
            _ => 0.5,
        };
    }
}

fn resolve_market_state(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    timestamp_utc_nanos: i64,
) -> Option<MarketState> {
    let timestamp_utc = DateTime::<Utc>::from_timestamp_nanos(timestamp_utc_nanos);
    calendar_registry
        .resolve_market_state_at_utc(exchange_code, timestamp_utc)
        .ok()
}

fn ceil_to_interval(timestamp_utc_nanos: i64, interval_duration_nanos: i64) -> i64 {
    let remainder = timestamp_utc_nanos.rem_euclid(interval_duration_nanos);
    if remainder == 0 {
        timestamp_utc_nanos
    } else {
        timestamp_utc_nanos.saturating_add(interval_duration_nanos - remainder)
    }
}

fn rolling_sum(values: &[f64], window: usize) -> Vec<f64> {
    let mut output = Vec::with_capacity(values.len());
    let mut running_sum = 0.0;

    for (position, value) in values.iter().enumerate() {
        running_sum += *value;
        if position >= window {
            running_sum -= values[position - window];
        }
        output.push(running_sum);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::{
        JOINT_REFINEMENT_ROLLING_ACTIVITY_WINDOW_INTERVAL_COUNT,
        JointRefinementAggregatedIntervalObservation,
        aggregate_simulated_path_to_interval_template,
        aggregate_simulated_path_to_interval_templates, log_ratio_distance,
        normalized_profile_distance, relative_rmse, return_scale_summary,
        rolling_variance_activity_metric, scale_normalized_returns,
        zeroed_interval_observation_template,
    };
    use janiform_affine_oracle::sufficient_statistics::SufficientStatisticsRiskReturnComponents;
    use janiform_engine_simulation::{
        SeededFinePathRecord, SeededFinePathTransitionKind, SeededTickEventSide,
        SeededTickSimulationNewsEventMarketState,
    };
    use janiform_foundations::numeric::standardized_path_distance;

    #[test]
    fn rolling_variance_activity_metric_matches_reference_series_definition() {
        let interval_count = JOINT_REFINEMENT_ROLLING_ACTIVITY_WINDOW_INTERVAL_COUNT + 4;
        let intervals: Vec<JointRefinementAggregatedIntervalObservation> = (0..interval_count)
            .map(|position| JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: position as i64,
                interval_end_utc_nanos: position as i64 + 1,
                log_return: position as f64 * 0.01,
                tick_count: position + 1,
                sell_share_proxy: 0.5,
            })
            .collect();

        let metric = rolling_variance_activity_metric(intervals.as_slice());

        assert!(metric.correlation.is_finite());
        assert!(metric.standardized_path_distance.is_finite());
        assert!(metric.correlation > 0.0);
    }

    #[test]
    fn single_path_multi_horizon_aggregation_matches_expected_template_fill() {
        let five_minute_template = zeroed_interval_observation_template(&[
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: 300,
                log_return: 1.0,
                tick_count: 1,
                sell_share_proxy: 1.0,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 300,
                interval_end_utc_nanos: 600,
                log_return: 1.0,
                tick_count: 1,
                sell_share_proxy: 1.0,
            },
        ]);

        let fifteen_minute_template =
            zeroed_interval_observation_template(&[JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: 900,
                log_return: 1.0,
                tick_count: 1,
                sell_share_proxy: 1.0,
            }]);

        let fine_path = vec![
            SeededFinePathRecord {
                transition_index: 0,
                timestamp_utc_nanos: 120,
                transition_kind: SeededFinePathTransitionKind::BuyTrade,
                side: Some(SeededTickEventSide::Buy),
                news_arrival_component: None,
                market_state: SeededTickSimulationNewsEventMarketState::Open,
                return_components: SufficientStatisticsRiskReturnComponents {
                    buy_side_return: 0.01,
                    sell_side_return: 0.0,
                    trading_time_return: 0.01,
                    open_jump_return: 0.0,
                    closed_gap_return: 0.0,
                    total_return: 0.01,
                },
                cumulative_log_return: 0.01,
                state_snapshot: None,
            },
            SeededFinePathRecord {
                transition_index: 1,
                timestamp_utc_nanos: 420,
                transition_kind: SeededFinePathTransitionKind::SellTrade,
                side: Some(SeededTickEventSide::Sell),
                news_arrival_component: None,
                market_state: SeededTickSimulationNewsEventMarketState::Open,
                return_components: SufficientStatisticsRiskReturnComponents {
                    buy_side_return: 0.0,
                    sell_side_return: -0.02,
                    trading_time_return: -0.02,
                    open_jump_return: 0.0,
                    closed_gap_return: 0.0,
                    total_return: -0.02,
                },
                cumulative_log_return: -0.01,
                state_snapshot: None,
            },
        ];

        let (five_minute_intervals, fifteen_minute_intervals) =
            aggregate_simulated_path_to_interval_templates(
                fine_path.as_slice(),
                five_minute_template.as_slice(),
                fifteen_minute_template.as_slice(),
                300,
                900,
            );

        assert_eq!(five_minute_intervals.len(), 2);
        assert_eq!(five_minute_intervals[0].tick_count, 1);
        assert_eq!(five_minute_intervals[1].tick_count, 1);
        assert_eq!(five_minute_intervals[0].log_return, 0.01);
        assert_eq!(five_minute_intervals[1].log_return, -0.02);
        assert_eq!(five_minute_intervals[0].sell_share_proxy, 0.0);
        assert_eq!(five_minute_intervals[1].sell_share_proxy, 1.0);

        assert_eq!(fifteen_minute_intervals.len(), 1);
        assert_eq!(fifteen_minute_intervals[0].tick_count, 2);
        assert_eq!(fifteen_minute_intervals[0].log_return, -0.01);
        assert_eq!(fifteen_minute_intervals[0].sell_share_proxy, 0.5);
    }

    #[test]
    fn single_template_aggregation_matches_multi_horizon_helper() {
        let template = zeroed_interval_observation_template(&[
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: 300,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 300,
                interval_end_utc_nanos: 600,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
        ]);
        let fine_path = vec![SeededFinePathRecord {
            transition_index: 0,
            timestamp_utc_nanos: 120,
            transition_kind: SeededFinePathTransitionKind::BuyTrade,
            side: Some(SeededTickEventSide::Buy),
            news_arrival_component: None,
            market_state: SeededTickSimulationNewsEventMarketState::Open,
            return_components: SufficientStatisticsRiskReturnComponents {
                buy_side_return: 0.01,
                sell_side_return: 0.0,
                trading_time_return: 0.01,
                open_jump_return: 0.0,
                closed_gap_return: 0.0,
                total_return: 0.01,
            },
            cumulative_log_return: 0.01,
            state_snapshot: None,
        }];

        let aggregated = aggregate_simulated_path_to_interval_template(
            fine_path.as_slice(),
            template.as_slice(),
            300,
        );

        assert_eq!(aggregated[0].tick_count, 1);
        assert_eq!(aggregated[0].log_return, 0.01);
        assert_eq!(aggregated[1].tick_count, 0);
    }

    #[test]
    fn scale_and_activity_helpers_are_level_and_shape_aware() {
        let normalized_returns = scale_normalized_returns(&[1.0, -2.0, 1.0]);
        assert_eq!(normalized_returns.len(), 3);
        assert!(normalized_returns[1].abs() > normalized_returns[0].abs());

        let profile_distance = normalized_profile_distance(&[2.0, 4.0], &[1.0, 2.0]);
        assert_eq!(profile_distance, 0.0);

        let path_distance = standardized_path_distance(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]);
        assert_eq!(path_distance, 0.0);

        let rmse = relative_rmse(&[2.0, 4.0], &[1.0, 3.0], 1.0e-8);
        assert!(rmse.is_finite());
        assert!(rmse > 0.0);

        let ratio_distance = log_ratio_distance(4.0, 2.0, 1.0e-8);
        assert!(ratio_distance > 0.0);

        let scale_summary = return_scale_summary(&[
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: 1,
                log_return: 0.1,
                tick_count: 1,
                sell_share_proxy: 0.5,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 1,
                interval_end_utc_nanos: 2,
                log_return: -0.2,
                tick_count: 1,
                sell_share_proxy: 0.5,
            },
        ]);
        assert!(scale_summary.mean_absolute_return > 0.0);
        assert!(scale_summary.realized_variance > 0.0);
        assert!(scale_summary.absolute_return_quantile_q90 >= 0.1);
    }
}
