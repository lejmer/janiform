use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::stages::return_components::{
    ReturnComponentsObservedOpenSessionReturn,
    ReturnComponentsStageContinuousReturnCalibrationSample,
    ReturnComponentsStageSignalSourceWindow,
    build_continuous_return_calibration_sample_from_components,
};
#[cfg(test)]
use janiform_engine_simulation::{SeededFinePathRecord, SeededFinePathTransitionKind};
use janiform_foundations::numeric::{
    arithmetic_mean_or_zero, compute_finite_quantile_or_zero, pearson_correlation, rolling_sum,
    standardized_path_distance,
};
use janiform_foundations::time::{ceil_to_interval_utc_nanos, floor_to_interval_utc_nanos};
use janiform_market_time::calendar::{ExchangeCalendarRegistry, MarketState};

pub(crate) const JOINT_REFINEMENT_ROLLING_ACTIVITY_WINDOW_INTERVAL_COUNT: usize = 96;
const JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR: f64 = 1.0e-8;
const JOINT_REFINEMENT_RETURN_SCALE_QUANTILE_Q90: f64 = 0.90;
const JOINT_REFINEMENT_BUSINESS_TIME_BUCKET_COUNT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementBusinessTimeBucketThresholds {
    pub lower_quartile_cutoff: f64,
    pub median_cutoff: f64,
    pub upper_quartile_cutoff: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementRollingVarianceActivityMetric {
    pub correlation: f64,
    pub standardized_path_distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementReturnScaleSummary {
    pub mean_absolute_return: f64,
    pub realized_variance: f64,
    pub absolute_return_quantile_q90: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementBusinessTimeBucketReturnScaleSummaries {
    pub bucket_summaries:
        [Option<JointRefinementReturnScaleSummary>; JOINT_REFINEMENT_BUSINESS_TIME_BUCKET_COUNT],
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementAggregatedIntervalObservation {
    pub interval_start_utc_nanos: i64,
    pub interval_end_utc_nanos: i64,
    pub log_return: f64,
    pub tick_count: usize,
    pub sell_share_proxy: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct JointRefinementIntervalObservationCursor {
    next_index: usize,
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

    let mut cursor = JointRefinementIntervalObservationCursor::default();

    for observation in observed_open_session_returns {
        if observation.return_timestamp_utc_nanos < source_window.window_start_utc_nanos
            || observation.return_timestamp_utc_nanos >= source_window.window_end_utc_nanos
        {
            continue;
        }

        accumulate_return_into_intervals_with_cursor(
            observation.return_timestamp_utc_nanos,
            observation.log_return,
            1,
            observation.sell_share_proxy,
            intervals.as_mut_slice(),
            &mut cursor,
        );
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

#[cfg(test)]
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

#[cfg(test)]
pub(crate) fn aggregate_simulated_path_to_interval_template(
    fine_path: &[SeededFinePathRecord],
    template: &[JointRefinementAggregatedIntervalObservation],
    interval_duration_nanos: i64,
) -> Vec<JointRefinementAggregatedIntervalObservation> {
    let mut intervals = template.to_vec();
    if intervals.is_empty() {
        return intervals;
    }

    for record in fine_path {
        accumulate_fine_path_record_into_intervals(
            record,
            intervals.as_mut_slice(),
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
    let returns: Vec<f64> = intervals
        .iter()
        .map(|interval| interval.log_return)
        .collect();

    return_scale_summary_from_returns(returns.as_slice())
}

pub(crate) fn return_scale_summary_from_returns(
    returns: &[f64],
) -> JointRefinementReturnScaleSummary {
    let absolute_returns: Vec<f64> = returns
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(f64::abs)
        .collect();
    let realized_variance = returns
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(|value| value * value)
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

pub(crate) fn business_time_bucket_thresholds(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Option<JointRefinementBusinessTimeBucketThresholds> {
    let business_time_scales: Vec<f64> = intervals
        .iter()
        .map(|interval| interval.tick_count as f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .collect();
    if business_time_scales.is_empty() {
        return None;
    }

    Some(JointRefinementBusinessTimeBucketThresholds {
        lower_quartile_cutoff: compute_finite_quantile_or_zero(
            business_time_scales.as_slice(),
            0.25,
        ),
        median_cutoff: compute_finite_quantile_or_zero(business_time_scales.as_slice(), 0.50),
        upper_quartile_cutoff: compute_finite_quantile_or_zero(
            business_time_scales.as_slice(),
            0.75,
        ),
    })
}

pub(crate) fn return_scale_summaries_by_business_time_bucket(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    thresholds: JointRefinementBusinessTimeBucketThresholds,
) -> JointRefinementBusinessTimeBucketReturnScaleSummaries {
    let mut bucket_absolute_returns: [Vec<f64>; JOINT_REFINEMENT_BUSINESS_TIME_BUCKET_COUNT] =
        std::array::from_fn(|_| Vec::new());
    let mut bucket_realized_variance = [0.0; JOINT_REFINEMENT_BUSINESS_TIME_BUCKET_COUNT];

    for interval in intervals {
        if !interval.log_return.is_finite() {
            continue;
        }

        let bucket_index = business_time_bucket_index(interval.tick_count as f64, thresholds);
        let absolute_return = interval.log_return.abs();

        bucket_absolute_returns[bucket_index].push(absolute_return);
        bucket_realized_variance[bucket_index] += interval.log_return * interval.log_return;
    }

    JointRefinementBusinessTimeBucketReturnScaleSummaries {
        bucket_summaries: std::array::from_fn(|bucket_index| {
            let absolute_returns = bucket_absolute_returns[bucket_index].as_slice();
            if absolute_returns.is_empty() {
                return None;
            }

            Some(JointRefinementReturnScaleSummary {
                mean_absolute_return: arithmetic_mean_or_zero(absolute_returns),
                realized_variance: bucket_realized_variance[bucket_index],
                absolute_return_quantile_q90: compute_finite_quantile_or_zero(
                    absolute_returns,
                    JOINT_REFINEMENT_RETURN_SCALE_QUANTILE_Q90,
                ),
            })
        }),
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

pub(crate) fn mean_absolute_return_by_utc_hour(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<f64> {
    mean_absolute_return_profile_by_utc_hour(
        intervals
            .iter()
            .map(|interval| (interval.interval_start_utc_nanos, interval.log_return)),
    )
}

pub(crate) fn mean_absolute_observed_return_by_utc_hour(
    observed_returns: &[ReturnComponentsObservedOpenSessionReturn],
) -> Vec<f64> {
    mean_absolute_return_profile_by_utc_hour(observed_returns.iter().map(|observation| {
        (
            observation.return_timestamp_utc_nanos,
            observation.log_return,
        )
    }))
}

#[cfg(test)]
pub(crate) fn mean_absolute_trade_return_by_utc_hour(
    fine_path: &[SeededFinePathRecord],
) -> Vec<f64> {
    mean_absolute_return_profile_by_utc_hour(fine_path.iter().filter_map(|record| {
        matches!(
            record.transition_kind,
            SeededFinePathTransitionKind::BuyTrade | SeededFinePathTransitionKind::SellTrade
        )
        .then_some((
            record.timestamp_utc_nanos,
            record.return_components.total_return,
        ))
    }))
}

#[cfg(test)]
pub(crate) fn sampled_trade_returns(
    fine_path: &[SeededFinePathRecord],
    target_count: usize,
) -> Vec<f64> {
    let finite_trade_returns: Vec<f64> = fine_path
        .iter()
        .filter_map(|record| {
            matches!(
                record.transition_kind,
                SeededFinePathTransitionKind::BuyTrade | SeededFinePathTransitionKind::SellTrade
            )
            .then_some(record.return_components.total_return)
        })
        .filter(|value| value.is_finite())
        .collect();

    sampled_return_values(finite_trade_returns.as_slice(), target_count)
}

pub(crate) fn sampled_return_values(returns: &[f64], target_count: usize) -> Vec<f64> {
    let finite_returns: Vec<f64> = returns
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();

    sample_values_by_equal_stride(finite_returns.as_slice(), target_count)
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

fn sample_values_by_equal_stride(values: &[f64], target_count: usize) -> Vec<f64> {
    if values.is_empty() || target_count == 0 {
        return Vec::new();
    }
    if values.len() <= target_count {
        return values.to_vec();
    }
    if target_count == 1 {
        return vec![values[values.len() / 2]];
    }

    (0..target_count)
        .filter_map(|sample_index| {
            let source_index = sample_index.saturating_mul(values.len() - 1) / (target_count - 1);
            values.get(source_index).copied()
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

fn business_time_bucket_index(
    business_time_scale: f64,
    thresholds: JointRefinementBusinessTimeBucketThresholds,
) -> usize {
    if !business_time_scale.is_finite() || business_time_scale <= thresholds.lower_quartile_cutoff {
        0
    } else if business_time_scale <= thresholds.median_cutoff {
        1
    } else if business_time_scale <= thresholds.upper_quartile_cutoff {
        2
    } else {
        3
    }
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

    let aligned_start = floor_to_interval_utc_nanos(
        source_window.window_start_utc_nanos,
        interval_duration_nanos,
    );
    let aligned_end =
        ceil_to_interval_utc_nanos(source_window.window_end_utc_nanos, interval_duration_nanos);

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

pub(crate) fn finalize_sell_share_proxies(
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

#[cfg(test)]
pub(crate) fn accumulate_fine_path_record_into_intervals(
    record: &SeededFinePathRecord,
    intervals: &mut [JointRefinementAggregatedIntervalObservation],
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

    let Some(first_interval_start_utc_nanos) = intervals
        .first()
        .map(|interval| interval.interval_start_utc_nanos)
    else {
        return;
    };

    if record.timestamp_utc_nanos < first_interval_start_utc_nanos
        || record.timestamp_utc_nanos >= last_interval_end_utc_nanos
    {
        return;
    }

    let (tick_increment, sell_share_increment) = match record.transition_kind {
        SeededFinePathTransitionKind::BuyTrade => (1, 0.0),
        SeededFinePathTransitionKind::SellTrade => (1, 1.0),
        _ => (0, 0.0),
    };

    accumulate_return_into_intervals_with_cursor(
        record.timestamp_utc_nanos,
        record.return_components.total_return,
        tick_increment,
        sell_share_increment,
        intervals,
        &mut JointRefinementIntervalObservationCursor::default(),
    );
}

pub(crate) fn accumulate_return_into_intervals_with_cursor(
    timestamp_utc_nanos: i64,
    log_return: f64,
    tick_increment: usize,
    sell_share_increment: f64,
    intervals: &mut [JointRefinementAggregatedIntervalObservation],
    cursor: &mut JointRefinementIntervalObservationCursor,
) {
    let Some(interval_index) =
        interval_index_for_timestamp_from_cursor(intervals, timestamp_utc_nanos, cursor)
    else {
        return;
    };

    let Some(interval) = intervals.get_mut(interval_index) else {
        return;
    };

    interval.log_return += log_return;
    interval.tick_count += tick_increment;
    interval.sell_share_proxy += sell_share_increment;
}

fn interval_index_for_timestamp(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    timestamp_utc_nanos: i64,
) -> Option<usize> {
    let candidate_index = intervals
        .partition_point(|interval| interval.interval_end_utc_nanos <= timestamp_utc_nanos);

    let candidate_interval = intervals.get(candidate_index)?;

    (candidate_interval.interval_start_utc_nanos <= timestamp_utc_nanos
        && timestamp_utc_nanos < candidate_interval.interval_end_utc_nanos)
        .then_some(candidate_index)
}

fn interval_index_for_timestamp_from_cursor(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    timestamp_utc_nanos: i64,
    cursor: &mut JointRefinementIntervalObservationCursor,
) -> Option<usize> {
    if intervals.is_empty() {
        return None;
    }

    let first_interval_start_utc_nanos = intervals.first()?.interval_start_utc_nanos;
    let last_interval_end_utc_nanos = intervals.last()?.interval_end_utc_nanos;
    if timestamp_utc_nanos < first_interval_start_utc_nanos
        || timestamp_utc_nanos >= last_interval_end_utc_nanos
    {
        return None;
    }

    if cursor.next_index >= intervals.len() {
        cursor.next_index = intervals.len().saturating_sub(1);
    }

    if let Some(current_interval) = intervals.get(cursor.next_index) {
        if current_interval.interval_start_utc_nanos <= timestamp_utc_nanos
            && timestamp_utc_nanos < current_interval.interval_end_utc_nanos
        {
            return Some(cursor.next_index);
        }

        if timestamp_utc_nanos >= current_interval.interval_end_utc_nanos {
            while cursor.next_index < intervals.len()
                && intervals[cursor.next_index].interval_end_utc_nanos <= timestamp_utc_nanos
            {
                cursor.next_index += 1;
            }

            if let Some(advanced_interval) = intervals.get(cursor.next_index)
                && advanced_interval.interval_start_utc_nanos <= timestamp_utc_nanos
                && timestamp_utc_nanos < advanced_interval.interval_end_utc_nanos
            {
                return Some(cursor.next_index);
            }
        }
    }

    let interval_index = interval_index_for_timestamp(intervals, timestamp_utc_nanos)?;
    cursor.next_index = interval_index;
    Some(interval_index)
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

fn mean_absolute_return_profile_by_utc_hour<I>(timestamped_returns: I) -> Vec<f64>
where
    I: IntoIterator<Item = (i64, f64)>,
{
    let mut absolute_return_sum_by_hour = [0.0; 24];
    let mut observation_count_by_hour = [0usize; 24];

    for (timestamp_utc_nanos, log_return) in timestamped_returns {
        if !log_return.is_finite() {
            continue;
        }

        let hour_index = DateTime::<Utc>::from_timestamp_nanos(timestamp_utc_nanos).hour() as usize;

        absolute_return_sum_by_hour[hour_index] += log_return.abs();
        observation_count_by_hour[hour_index] += 1;
    }

    mean_absolute_return_profile_from_hourly_totals(
        absolute_return_sum_by_hour,
        observation_count_by_hour,
    )
}

pub(crate) fn mean_absolute_return_profile_from_hourly_totals(
    absolute_return_sum_by_hour: [f64; 24],
    observation_count_by_hour: [usize; 24],
) -> Vec<f64> {
    (0..24)
        .map(|hour_index| {
            if observation_count_by_hour[hour_index] == 0 {
                0.0
            } else {
                absolute_return_sum_by_hour[hour_index]
                    / observation_count_by_hour[hour_index] as f64
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        JOINT_REFINEMENT_ROLLING_ACTIVITY_WINDOW_INTERVAL_COUNT,
        JointRefinementAggregatedIntervalObservation, JointRefinementIntervalObservationCursor,
        accumulate_return_into_intervals_with_cursor,
        aggregate_simulated_path_to_interval_template,
        aggregate_simulated_path_to_interval_templates, business_time_bucket_thresholds,
        interval_index_for_timestamp, log_ratio_distance, mean_absolute_return_by_utc_hour,
        normalized_profile_distance, return_scale_summaries_by_business_time_bucket,
        return_scale_summary, return_scale_summary_from_returns, rolling_variance_activity_metric,
        sampled_return_values, sampled_trade_returns, scale_normalized_returns,
        zeroed_interval_observation_template,
    };
    use janiform_affine_oracle::sufficient_statistics::SufficientStatisticsReturnComponents;
    use janiform_engine_simulation::{
        SeededFinePathRecord, SeededFinePathTransitionKind, SeededTickEventSide,
        SeededTickSimulationNewsEventMarketState,
    };
    use janiform_foundations::numeric::{relative_rmse, standardized_path_distance};

    fn test_fine_path_record(
        transition_index: u64,
        timestamp_utc_nanos: i64,
        transition_kind: SeededFinePathTransitionKind,
        side: Option<SeededTickEventSide>,
        total_return: f64,
        cumulative_core_log_return: f64,
    ) -> SeededFinePathRecord {
        SeededFinePathRecord {
            transition_index,
            timestamp_utc_nanos,
            transition_kind,
            side,
            news_arrival_component: None,
            market_state: SeededTickSimulationNewsEventMarketState::Open,
            return_components: SufficientStatisticsReturnComponents {
                buy_side_return: total_return.max(0.0),
                sell_side_return: total_return.min(0.0),
                trading_time_return: total_return,
                open_jump_return: 0.0,
                closed_gap_return: 0.0,
                total_return,
            },
            core_total_return_increment: total_return,
            observed_log_price: cumulative_core_log_return,
            anchor_value: 0.0,
            pressure_state: 0.0,
            deviation_from_anchor: cumulative_core_log_return,
            cumulative_core_log_return,
            state_snapshot: None,
        }
    }

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
            test_fine_path_record(
                0,
                120,
                SeededFinePathTransitionKind::BuyTrade,
                Some(SeededTickEventSide::Buy),
                0.01,
                0.01,
            ),
            test_fine_path_record(
                1,
                420,
                SeededFinePathTransitionKind::SellTrade,
                Some(SeededTickEventSide::Sell),
                -0.02,
                -0.01,
            ),
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

        let fine_path = vec![test_fine_path_record(
            0,
            120,
            SeededFinePathTransitionKind::BuyTrade,
            Some(SeededTickEventSide::Buy),
            0.01,
            0.01,
        )];

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
    fn sampled_trade_returns_preserve_endpoints_and_requested_count() {
        let fine_path: Vec<SeededFinePathRecord> = (0..6)
            .map(|position| {
                test_fine_path_record(
                    position,
                    position as i64,
                    SeededFinePathTransitionKind::BuyTrade,
                    Some(SeededTickEventSide::Buy),
                    position as f64,
                    position as f64,
                )
            })
            .collect();

        let sampled_returns = sampled_trade_returns(fine_path.as_slice(), 3);

        assert_eq!(sampled_returns, vec![0.0, 2.0, 5.0]);
    }

    #[test]
    fn sampled_return_values_drop_non_finite_values_before_stride_sampling() {
        let sampled_returns =
            sampled_return_values(&[0.0, f64::NAN, 1.0, f64::INFINITY, 2.0, 3.0], 3);

        assert_eq!(sampled_returns, vec![0.0, 1.0, 3.0]);
    }

    #[test]
    fn return_scale_summary_from_returns_matches_interval_variant() {
        let intervals = vec![
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: 1,
                log_return: 0.01,
                tick_count: 1,
                sell_share_proxy: 0.5,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 1,
                interval_end_utc_nanos: 2,
                log_return: -0.03,
                tick_count: 1,
                sell_share_proxy: 0.5,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 2,
                interval_end_utc_nanos: 3,
                log_return: 0.02,
                tick_count: 1,
                sell_share_proxy: 0.5,
            },
        ];

        let returns = vec![0.01, -0.03, 0.02];

        assert_eq!(
            return_scale_summary(intervals.as_slice()),
            return_scale_summary_from_returns(returns.as_slice())
        );
    }

    #[test]
    fn sparse_open_session_templates_keep_records_after_closed_gap() {
        let template = zeroed_interval_observation_template(&[
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: 300,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 900,
                interval_end_utc_nanos: 1_200,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
        ]);

        let fine_path = vec![
            test_fine_path_record(
                0,
                120,
                SeededFinePathTransitionKind::BuyTrade,
                Some(SeededTickEventSide::Buy),
                0.01,
                0.01,
            ),
            test_fine_path_record(
                1,
                960,
                SeededFinePathTransitionKind::SellTrade,
                Some(SeededTickEventSide::Sell),
                -0.02,
                -0.01,
            ),
        ];

        let aggregated = aggregate_simulated_path_to_interval_template(
            fine_path.as_slice(),
            template.as_slice(),
            300,
        );

        assert_eq!(aggregated[0].tick_count, 1);
        assert_eq!(aggregated[0].log_return, 0.01);
        assert_eq!(aggregated[1].tick_count, 1);
        assert_eq!(aggregated[1].log_return, -0.02);
        assert_eq!(aggregated[1].sell_share_proxy, 1.0);
    }

    #[test]
    fn interval_lookup_handles_sparse_templates() {
        let template = [
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: 300,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 900,
                interval_end_utc_nanos: 1_200,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
        ];

        assert_eq!(
            interval_index_for_timestamp(template.as_slice(), 120),
            Some(0)
        );
        assert_eq!(interval_index_for_timestamp(template.as_slice(), 600), None);
        assert_eq!(
            interval_index_for_timestamp(template.as_slice(), 960),
            Some(1)
        );
    }

    #[test]
    fn cursor_accumulation_matches_binary_search_accumulation_for_monotonic_records() {
        let mut cursor_intervals = zeroed_interval_observation_template(&[
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: 10,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 10,
                interval_end_utc_nanos: 20,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 20,
                interval_end_utc_nanos: 30,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
        ]);
        let mut baseline_intervals = cursor_intervals.clone();
        let mut cursor = JointRefinementIntervalObservationCursor::default();

        for (timestamp_utc_nanos, log_return, tick_increment, sell_share_increment) in [
            (1_i64, 0.5, 1_usize, 0.0),
            (8_i64, -0.2, 1_usize, 1.0),
            (11_i64, 0.3, 0_usize, 0.0),
            (19_i64, 0.7, 1_usize, 1.0),
            (21_i64, -0.4, 1_usize, 0.0),
        ] {
            accumulate_return_into_intervals_with_cursor(
                timestamp_utc_nanos,
                log_return,
                tick_increment,
                sell_share_increment,
                cursor_intervals.as_mut_slice(),
                &mut cursor,
            );

            let Some(interval_index) =
                interval_index_for_timestamp(baseline_intervals.as_slice(), timestamp_utc_nanos)
            else {
                continue;
            };
            let interval = &mut baseline_intervals[interval_index];
            interval.log_return += log_return;
            interval.tick_count += tick_increment;
            interval.sell_share_proxy += sell_share_increment;
        }

        assert_eq!(cursor_intervals, baseline_intervals);
    }

    #[test]
    fn cursor_accumulation_falls_back_for_out_of_order_records() {
        let mut intervals = zeroed_interval_observation_template(&[
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: 10,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 10,
                interval_end_utc_nanos: 20,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 20,
                interval_end_utc_nanos: 30,
                log_return: 0.0,
                tick_count: 0,
                sell_share_proxy: 0.0,
            },
        ]);
        let mut cursor = JointRefinementIntervalObservationCursor::default();

        accumulate_return_into_intervals_with_cursor(
            22,
            0.9,
            1,
            1.0,
            intervals.as_mut_slice(),
            &mut cursor,
        );
        accumulate_return_into_intervals_with_cursor(
            3,
            -0.4,
            1,
            0.0,
            intervals.as_mut_slice(),
            &mut cursor,
        );

        assert_eq!(intervals[0].log_return, -0.4);
        assert_eq!(intervals[0].tick_count, 1);
        assert_eq!(intervals[2].log_return, 0.9);
        assert_eq!(intervals[2].tick_count, 1);
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

    #[test]
    fn business_time_bucket_return_scale_summaries_group_by_tick_count_regime() {
        let intervals: Vec<JointRefinementAggregatedIntervalObservation> = (0..8)
            .map(|index| JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: index,
                interval_end_utc_nanos: index + 1,
                log_return: 0.001 * (index as f64 + 1.0),
                tick_count: (index + 1) as usize,
                sell_share_proxy: 0.5,
            })
            .collect();

        let thresholds = business_time_bucket_thresholds(intervals.as_slice())
            .expect("bucket thresholds should exist");

        let summaries =
            return_scale_summaries_by_business_time_bucket(intervals.as_slice(), thresholds);

        assert_eq!(summaries.bucket_summaries.len(), 4);
        assert!(summaries.bucket_summaries.iter().all(Option::is_some));
        assert!(
            summaries.bucket_summaries[3]
                .expect("upper bucket summary should exist")
                .mean_absolute_return
                > summaries.bucket_summaries[0]
                    .expect("lower bucket summary should exist")
                    .mean_absolute_return
        );
    }

    #[test]
    fn mean_absolute_return_by_utc_hour_aggregates_per_hour_level() {
        let hourly_profile = mean_absolute_return_by_utc_hour(&[
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: 1,
                log_return: 0.10,
                tick_count: 1,
                sell_share_proxy: 0.5,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 30 * 60 * 1_000_000_000,
                interval_end_utc_nanos: 30 * 60 * 1_000_000_000 + 1,
                log_return: -0.30,
                tick_count: 1,
                sell_share_proxy: 0.5,
            },
            JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 60 * 60 * 1_000_000_000,
                interval_end_utc_nanos: 60 * 60 * 1_000_000_000 + 1,
                log_return: 0.20,
                tick_count: 1,
                sell_share_proxy: 0.5,
            },
        ]);

        assert_eq!(hourly_profile.len(), 24);
        assert_eq!(hourly_profile[0], 0.20);
        assert_eq!(hourly_profile[1], 0.20);
        assert_eq!(hourly_profile[2], 0.0);
    }
}
