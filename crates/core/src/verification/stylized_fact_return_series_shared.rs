//! Shared return-series preprocessing and autocorrelation helpers.

use super::time_buckets::{TimestampAnalysisWindow, interval_position_for_timestamp};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TimestampedReturnObservation {
    pub timestamp_utc_nanos: i64,
    pub return_increment: f64,
}

pub(crate) trait ReturnObservationLike: Copy {
    fn timestamp_utc_nanos(self) -> i64;
    fn return_increment(self) -> f64;
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BucketedAbsReturnSeries {
    pub abs_return_by_interval: Vec<f64>,
    pub covered_interval_count: usize,
}

pub(crate) fn find_non_finite_return_observation<O: ReturnObservationLike>(
    return_observations: &[O],
) -> Option<(usize, f64)> {
    for (observation_position, return_observation) in
        return_observations.iter().copied().enumerate()
    {
        let return_increment = return_observation.return_increment();
        if !return_increment.is_finite() {
            return Some((observation_position, return_increment));
        }
    }

    None
}

pub(crate) fn filter_sort_returns_in_window<O: ReturnObservationLike>(
    return_observations: &[O],
    analysis_window: TimestampAnalysisWindow,
) -> Vec<TimestampedReturnObservation> {
    let mut return_observations_in_window: Vec<TimestampedReturnObservation> = return_observations
        .iter()
        .copied()
        .filter(|return_observation| {
            analysis_window.contains_timestamp(return_observation.timestamp_utc_nanos())
        })
        .map(|return_observation| TimestampedReturnObservation {
            timestamp_utc_nanos: return_observation.timestamp_utc_nanos(),
            return_increment: return_observation.return_increment(),
        })
        .collect();
    return_observations_in_window
        .sort_by_key(|return_observation| return_observation.timestamp_utc_nanos);

    return_observations_in_window
}

pub(crate) fn bucket_abs_returns_by_interval(
    return_observations_in_window: &[TimestampedReturnObservation],
    analysis_window: TimestampAnalysisWindow,
    horizon_duration_nanos: i64,
    interval_count: usize,
) -> BucketedAbsReturnSeries {
    let mut interval_return_sum: Vec<f64> = vec![0.0; interval_count];
    let mut return_interval_has_observation: Vec<bool> = vec![false; interval_count];

    for return_observation in return_observations_in_window {
        let Some(interval_position) = interval_position_for_timestamp(
            return_observation.timestamp_utc_nanos,
            analysis_window,
            horizon_duration_nanos,
            interval_count,
        ) else {
            continue;
        };
        interval_return_sum[interval_position] += return_observation.return_increment;
        return_interval_has_observation[interval_position] = true;
    }

    let covered_interval_count = return_interval_has_observation
        .iter()
        .filter(|covered| **covered)
        .count();
    let abs_return_by_interval: Vec<f64> = interval_return_sum
        .iter()
        .map(|return_sum| return_sum.abs())
        .collect();

    BucketedAbsReturnSeries {
        abs_return_by_interval,
        covered_interval_count,
    }
}

pub(crate) fn compute_lag_autocorrelation(
    series: &[f64],
    lag_interval_count: usize,
) -> Option<f64> {
    if lag_interval_count == 0 || series.len() <= lag_interval_count {
        return None;
    }
    if series.iter().any(|value| !value.is_finite()) {
        return None;
    }

    let sample_mean = series.iter().sum::<f64>() / series.len() as f64;
    let variance_numerator = series
        .iter()
        .map(|value| {
            let centered = *value - sample_mean;
            centered * centered
        })
        .sum::<f64>();
    if !variance_numerator.is_finite() || variance_numerator <= 0.0 {
        return Some(0.0);
    }

    let covariance_numerator = series
        .iter()
        .skip(lag_interval_count)
        .zip(series.iter())
        .map(|(current_value, lagged_value)| {
            (*current_value - sample_mean) * (*lagged_value - sample_mean)
        })
        .sum::<f64>();
    let autocorrelation = covariance_numerator / variance_numerator;

    autocorrelation.is_finite().then_some(autocorrelation)
}
