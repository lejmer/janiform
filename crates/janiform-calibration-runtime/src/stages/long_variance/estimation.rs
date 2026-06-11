use janiform_foundations::linear_algebra::non_negative_least_squares_2;
use janiform_foundations::numeric::{
    arithmetic_mean_or_zero, pearson_correlation, sample_variance,
};
use janiform_foundations::time::{NANOS_PER_HOUR, duration_nanos_to_hours};
use janiform_model_kernels::long_variance::LongVarianceParameters;

use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
use crate::stages::long_variance::metrics::{
    finite_long_variance_squared_returns, long_variance_interval_side_event_counts,
};
use crate::stages::long_variance::policy::{
    LONG_VARIANCE_STANDARD_CIR_VARIANCE_PROXY_BOUNDARY, LongVarianceSideEventLoadings,
    LongVarianceStationaryEventRates, long_variance_mean_reversion_rate_calibration_upper_bound,
    project_long_variance_event_loadings_to_stationary_budget,
};

const UTC_HOUR_BUCKET_COUNT: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq)]
struct DeseasonalizedSquaredReturnObservation {
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    deseasonalized_squared_return: f64,
}

pub(crate) fn estimate_long_variance_parameters(
    baseline_parameters: LongVarianceParameters,
    stationary_event_rates: LongVarianceStationaryEventRates,
    five_minute_intervals: &[JointRefinementAggregatedIntervalObservation],
    hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> LongVarianceParameters {
    let persistence_decay_rate = hourly_squared_return_persistence_decay_rate(hourly_intervals)
        .unwrap_or(baseline_parameters.mean_reversion_rate);

    let mean_reversion_rate =
        persistence_decay_rate.min(long_variance_mean_reversion_rate_calibration_upper_bound());

    let event_loadings = project_long_variance_event_loadings_to_stationary_budget(
        side_event_loading_estimate(five_minute_intervals),
        stationary_event_rates,
        mean_reversion_rate,
    );

    let vol_of_vol = long_variance_vol_of_vol_estimate(mean_reversion_rate, hourly_intervals)
        .unwrap_or(baseline_parameters.vol_of_vol);

    LongVarianceParameters::try_new(
        mean_reversion_rate,
        vol_of_vol,
        event_loadings.buy_event_loading,
        event_loadings.sell_event_loading,
    )
    .unwrap_or(baseline_parameters)
}

fn hourly_squared_return_persistence_decay_rate(
    hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Option<f64> {
    let observations = deseasonalized_hourly_squared_return_observations(hourly_intervals);
    if observations.len() < 2 {
        return None;
    }

    let mut lagged_squared_returns = Vec::new();
    let mut current_squared_returns = Vec::new();
    let mut lag_hours = Vec::new();

    for observation_pair in observations.windows(2) {
        let previous = observation_pair[0];
        let current = observation_pair[1];

        if previous.interval_end_utc_nanos != current.interval_start_utc_nanos {
            continue;
        }

        let current_lag_hours = duration_nanos_to_hours(
            current.interval_start_utc_nanos - previous.interval_start_utc_nanos,
        );
        if !current_lag_hours.is_finite() || current_lag_hours <= 0.0 {
            continue;
        }

        lagged_squared_returns.push(previous.deseasonalized_squared_return);
        current_squared_returns.push(current.deseasonalized_squared_return);
        lag_hours.push(current_lag_hours);
    }

    if lagged_squared_returns.len() < 2 {
        return None;
    }

    let persistence_correlation = pearson_correlation(
        lagged_squared_returns.as_slice(),
        current_squared_returns.as_slice(),
    );
    if !persistence_correlation.is_finite()
        || persistence_correlation <= 0.0
        || persistence_correlation >= 1.0
    {
        return None;
    }

    let average_interval_hours = arithmetic_mean_or_zero(lag_hours.as_slice());
    if !average_interval_hours.is_finite() || average_interval_hours <= 0.0 {
        return None;
    }

    let decay_rate = -persistence_correlation.ln() / average_interval_hours;

    (decay_rate.is_finite() && decay_rate > 0.0).then_some(decay_rate)
}

fn deseasonalized_hourly_squared_return_observations(
    hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<DeseasonalizedSquaredReturnObservation> {
    let squared_return_observations = hourly_intervals
        .iter()
        .filter(|interval| interval.log_return.is_finite())
        .filter_map(|interval| {
            let squared_return = interval.log_return * interval.log_return;
            if !squared_return.is_finite() {
                return None;
            }

            let hour_bucket = utc_hour_bucket(interval.interval_start_utc_nanos);

            Some((*interval, hour_bucket, squared_return))
        })
        .collect::<Vec<_>>();

    let mut squared_return_sum_by_hour = [0.0; UTC_HOUR_BUCKET_COUNT];
    let mut squared_return_count_by_hour = [0usize; UTC_HOUR_BUCKET_COUNT];

    for (_, hour_bucket, squared_return) in squared_return_observations.iter().copied() {
        squared_return_sum_by_hour[hour_bucket] += squared_return;
        squared_return_count_by_hour[hour_bucket] =
            squared_return_count_by_hour[hour_bucket].saturating_add(1);
    }

    squared_return_observations
        .into_iter()
        .filter_map(|(interval, hour_bucket, squared_return)| {
            let bucket_count = squared_return_count_by_hour[hour_bucket];
            if bucket_count == 0 {
                return None;
            }

            let bucket_mean = squared_return_sum_by_hour[hour_bucket] / bucket_count as f64;
            if !bucket_mean.is_finite() || bucket_mean <= 0.0 {
                return None;
            }

            let deseasonalized_squared_return = squared_return / bucket_mean;

            (deseasonalized_squared_return.is_finite() && deseasonalized_squared_return >= 0.0)
                .then_some(DeseasonalizedSquaredReturnObservation {
                    interval_start_utc_nanos: interval.interval_start_utc_nanos,
                    interval_end_utc_nanos: interval.interval_end_utc_nanos,
                    deseasonalized_squared_return,
                })
        })
        .collect()
}

fn utc_hour_bucket(timestamp_utc_nanos: i64) -> usize {
    timestamp_utc_nanos
        .div_euclid(NANOS_PER_HOUR)
        .rem_euclid(UTC_HOUR_BUCKET_COUNT as i64) as usize
}

fn side_event_loading_estimate(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> LongVarianceSideEventLoadings {
    let Some(mean_squared_return) = mean_squared_return(intervals) else {
        return LongVarianceSideEventLoadings {
            buy_event_loading: 0.0,
            sell_event_loading: 0.0,
        };
    };

    let side_counts = intervals
        .iter()
        .copied()
        .map(long_variance_interval_side_event_counts)
        .collect::<Vec<_>>();
    if side_counts.is_empty() {
        return LongVarianceSideEventLoadings {
            buy_event_loading: 0.0,
            sell_event_loading: 0.0,
        };
    }

    let mean_buy_count = side_counts
        .iter()
        .map(|counts| counts.buy_count)
        .sum::<f64>()
        / side_counts.len() as f64;

    let mean_sell_count = side_counts
        .iter()
        .map(|counts| counts.sell_count)
        .sum::<f64>()
        / side_counts.len() as f64;

    let mut rows = Vec::new();
    let mut response = Vec::new();

    for (interval, counts) in intervals.iter().zip(side_counts.iter()) {
        if !interval.log_return.is_finite() {
            continue;
        }

        rows.push([
            counts.buy_count - mean_buy_count,
            counts.sell_count - mean_sell_count,
        ]);
        response.push(interval.log_return * interval.log_return / mean_squared_return - 1.0);
    }

    let Some(coefficients) = non_negative_least_squares_2(rows.as_slice(), response.as_slice())
    else {
        return LongVarianceSideEventLoadings {
            buy_event_loading: 0.0,
            sell_event_loading: 0.0,
        };
    };

    LongVarianceSideEventLoadings {
        buy_event_loading: coefficients[0],
        sell_event_loading: coefficients[1],
    }
}

fn long_variance_vol_of_vol_estimate(
    mean_reversion_rate: f64,
    hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Option<f64> {
    if !mean_reversion_rate.is_finite() || mean_reversion_rate <= 0.0 {
        return None;
    }

    let mean_squared_return = mean_squared_return(hourly_intervals)?;

    let normalized_squared_returns = finite_long_variance_squared_returns(hourly_intervals)
        .into_iter()
        .map(|squared_return| squared_return / mean_squared_return)
        .collect::<Vec<_>>();

    let variance_proxy = sample_variance(normalized_squared_returns.as_slice())?;
    if !variance_proxy.is_finite() || variance_proxy < 0.0 {
        return None;
    }

    let projected_variance_proxy =
        variance_proxy.min(LONG_VARIANCE_STANDARD_CIR_VARIANCE_PROXY_BOUNDARY);

    Some((2.0 * mean_reversion_rate * projected_variance_proxy).sqrt())
}

fn mean_squared_return(intervals: &[JointRefinementAggregatedIntervalObservation]) -> Option<f64> {
    let squared_returns = finite_long_variance_squared_returns(intervals);
    if squared_returns.is_empty() {
        return None;
    }

    let mean_squared_return = arithmetic_mean_or_zero(squared_returns.as_slice());
    (mean_squared_return.is_finite() && mean_squared_return > 0.0).then_some(mean_squared_return)
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::{NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR};
    use janiform_model_kernels::long_variance::LongVarianceParameters;

    use super::{estimate_long_variance_parameters, hourly_squared_return_persistence_decay_rate};
    use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
    use crate::stages::long_variance::policy::{
        LongVarianceStationaryEventRates, long_variance_event_feedback_drift_budget,
        long_variance_vol_of_vol_upper_bound_for_mean_reversion_rate,
    };

    fn default_stationary_event_rates() -> LongVarianceStationaryEventRates {
        LongVarianceStationaryEventRates {
            buy_event_rate_per_hour: 1.0,
            sell_event_rate_per_hour: 1.0,
        }
    }

    fn interval(
        interval_start_utc_nanos: i64,
        interval_duration_nanos: i64,
        log_return: f64,
        tick_count: usize,
        sell_share_proxy: f64,
    ) -> JointRefinementAggregatedIntervalObservation {
        JointRefinementAggregatedIntervalObservation {
            interval_start_utc_nanos,
            interval_end_utc_nanos: interval_start_utc_nanos + interval_duration_nanos,
            log_return,
            tick_count,
            sell_share_proxy,
        }
    }

    #[test]
    fn hourly_persistence_skips_non_adjacent_session_pairs() {
        let hourly_intervals = [
            interval(0, NANOS_PER_HOUR, 0.01, 100, 0.50),
            interval(24 * NANOS_PER_HOUR, NANOS_PER_HOUR, 0.02, 100, 0.50),
            interval(48 * NANOS_PER_HOUR, NANOS_PER_HOUR, 0.04, 100, 0.50),
        ];

        assert_eq!(
            hourly_squared_return_persistence_decay_rate(hourly_intervals.as_slice()),
            None
        );
    }

    #[test]
    fn hourly_persistence_deseasonalizes_utc_hour_squared_returns() {
        let hourly_intervals = (0..3)
            .flat_map(|day_position| {
                [0.01, 0.02, 0.03, 0.04].into_iter().enumerate().map(
                    move |(hour_position, log_return)| {
                        let interval_start = day_position * 24 * NANOS_PER_HOUR
                            + hour_position as i64 * NANOS_PER_HOUR;

                        interval(interval_start, NANOS_PER_HOUR, log_return, 100, 0.50)
                    },
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            hourly_squared_return_persistence_decay_rate(hourly_intervals.as_slice()),
            None
        );
    }

    #[test]
    fn estimator_keeps_neutral_event_loadings_for_flat_activity_relation() {
        let five_minute_intervals = [
            interval(0, NANOS_PER_FIVE_MINUTES, 0.01, 10, 0.50),
            interval(
                NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                -0.01,
                10,
                0.50,
            ),
        ];

        let hourly_intervals = [
            interval(0, NANOS_PER_HOUR, 0.02, 100, 0.50),
            interval(NANOS_PER_HOUR, NANOS_PER_HOUR, -0.02, 100, 0.50),
        ];

        let estimate = estimate_long_variance_parameters(
            LongVarianceParameters::neutral(),
            default_stationary_event_rates(),
            five_minute_intervals.as_slice(),
            hourly_intervals.as_slice(),
        );

        assert_eq!(estimate.buy_event_loading, 0.0);
        assert_eq!(estimate.sell_event_loading, 0.0);
    }

    #[test]
    fn estimator_moves_buy_loading_when_buy_activity_tracks_squared_returns() {
        let five_minute_intervals = [
            interval(0, NANOS_PER_FIVE_MINUTES, 0.01, 10, 0.10),
            interval(
                NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.03,
                30,
                0.10,
            ),
            interval(
                2 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.01,
                10,
                0.10,
            ),
            interval(
                3 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.03,
                30,
                0.10,
            ),
        ];

        let hourly_intervals = [
            interval(0, NANOS_PER_HOUR, 0.01, 100, 0.50),
            interval(NANOS_PER_HOUR, NANOS_PER_HOUR, 0.02, 100, 0.50),
            interval(2 * NANOS_PER_HOUR, NANOS_PER_HOUR, 0.03, 100, 0.50),
        ];

        let estimate = estimate_long_variance_parameters(
            LongVarianceParameters::neutral(),
            default_stationary_event_rates(),
            five_minute_intervals.as_slice(),
            hourly_intervals.as_slice(),
        );

        assert!(estimate.buy_event_loading > 0.0);
        assert_eq!(estimate.sell_event_loading, 0.0);
    }

    #[test]
    fn estimator_projects_event_loadings_into_stationary_budget() {
        let five_minute_intervals = [
            interval(0, NANOS_PER_FIVE_MINUTES, 0.01, 10, 0.10),
            interval(
                NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.10,
                200,
                0.10,
            ),
            interval(
                2 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.01,
                10,
                0.10,
            ),
            interval(
                3 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.10,
                200,
                0.10,
            ),
        ];

        let hourly_intervals = [
            interval(0, NANOS_PER_HOUR, 0.02, 100, 0.50),
            interval(NANOS_PER_HOUR, NANOS_PER_HOUR, -0.02, 100, 0.50),
        ];

        let raw_event_loadings =
            super::side_event_loading_estimate(five_minute_intervals.as_slice());

        let event_feedback_budget = long_variance_event_feedback_drift_budget(
            LongVarianceParameters::neutral().mean_reversion_rate,
        )
        .expect("neutral long variance mean reversion should have a feedback budget");

        let stationary_event_rates = LongVarianceStationaryEventRates {
            buy_event_rate_per_hour: 2.0 * event_feedback_budget
                / raw_event_loadings.buy_event_loading,
            sell_event_rate_per_hour: 1.0,
        };

        let estimate = estimate_long_variance_parameters(
            LongVarianceParameters::neutral(),
            stationary_event_rates,
            five_minute_intervals.as_slice(),
            hourly_intervals.as_slice(),
        );

        assert!(estimate.buy_event_loading < raw_event_loadings.buy_event_loading);
        assert_eq!(
            estimate.sell_event_loading,
            raw_event_loadings.sell_event_loading
        );
        assert_eq!(
            estimate.mean_reversion_rate,
            LongVarianceParameters::neutral().mean_reversion_rate
        );
    }

    #[test]
    fn estimator_projects_vol_of_vol_into_restart_domain() {
        let five_minute_intervals = [
            interval(0, NANOS_PER_FIVE_MINUTES, 0.01, 10, 0.50),
            interval(
                NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                -0.01,
                10,
                0.50,
            ),
        ];

        let hourly_intervals = [
            interval(0, NANOS_PER_HOUR, 0.001, 100, 0.50),
            interval(NANOS_PER_HOUR, NANOS_PER_HOUR, 0.001, 100, 0.50),
            interval(2 * NANOS_PER_HOUR, NANOS_PER_HOUR, 1.0, 100, 0.50),
        ];

        let estimate = estimate_long_variance_parameters(
            LongVarianceParameters::neutral(),
            default_stationary_event_rates(),
            five_minute_intervals.as_slice(),
            hourly_intervals.as_slice(),
        );

        let upper_bound = long_variance_vol_of_vol_upper_bound_for_mean_reversion_rate(
            estimate.mean_reversion_rate,
        )
        .expect("positive mean reversion should have a vol-of-vol bound");

        assert!(estimate.vol_of_vol <= upper_bound);
    }
}
