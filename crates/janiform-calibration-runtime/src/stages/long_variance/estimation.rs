use janiform_foundations::linear_algebra::non_negative_least_squares_2;
use janiform_foundations::numeric::{
    arithmetic_mean_or_zero, pearson_correlation, sample_variance,
};
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::long_variance::LongVarianceParameters;

use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
use crate::stages::long_variance::metrics::{
    finite_long_variance_squared_returns, long_variance_interval_side_event_counts,
};
use crate::stages::long_variance::policy::{
    LONG_VARIANCE_STANDARD_CIR_VARIANCE_PROXY_BOUNDARY, long_variance_event_feedback_drift_budget,
    long_variance_mean_reversion_rate_calibration_upper_bound,
};

pub(crate) fn estimate_long_variance_parameters(
    baseline_parameters: LongVarianceParameters,
    five_minute_intervals: &[JointRefinementAggregatedIntervalObservation],
    hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> LongVarianceParameters {
    let persistence_decay_rate = hourly_squared_return_persistence_decay_rate(hourly_intervals)
        .unwrap_or(baseline_parameters.mean_reversion_rate);

    let mean_reversion_rate =
        persistence_decay_rate.min(long_variance_mean_reversion_rate_calibration_upper_bound());

    let event_rates = observed_side_event_rates_per_hour(five_minute_intervals);

    let event_loadings = project_event_loadings_to_stationary_budget(
        side_event_loading_estimate(five_minute_intervals),
        event_rates,
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

fn project_event_loadings_to_stationary_budget(
    event_loadings: SideEventLoadingEstimate,
    event_rates: SideEventRateEstimate,
    mean_reversion_rate: f64,
) -> SideEventLoadingEstimate {
    let event_loading_drift = event_loadings.buy_event_loading
        * event_rates.buy_event_rate_per_hour
        + event_loadings.sell_event_loading * event_rates.sell_event_rate_per_hour;

    let Some(event_feedback_drift_budget) =
        long_variance_event_feedback_drift_budget(mean_reversion_rate)
    else {
        return SideEventLoadingEstimate {
            buy_event_loading: 0.0,
            sell_event_loading: 0.0,
        };
    };

    if !event_loading_drift.is_finite() || event_loading_drift <= event_feedback_drift_budget {
        return event_loadings;
    }

    let scale = event_feedback_drift_budget / event_loading_drift;

    SideEventLoadingEstimate {
        buy_event_loading: event_loadings.buy_event_loading * scale,
        sell_event_loading: event_loadings.sell_event_loading * scale,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SideEventLoadingEstimate {
    buy_event_loading: f64,
    sell_event_loading: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SideEventRateEstimate {
    buy_event_rate_per_hour: f64,
    sell_event_rate_per_hour: f64,
}

fn hourly_squared_return_persistence_decay_rate(
    hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Option<f64> {
    let squared_returns = finite_long_variance_squared_returns(hourly_intervals);
    if squared_returns.len() < 2 {
        return None;
    }

    let persistence_correlation = pearson_correlation(
        &squared_returns[..squared_returns.len() - 1],
        &squared_returns[1..],
    );
    if !persistence_correlation.is_finite()
        || persistence_correlation <= 0.0
        || persistence_correlation >= 1.0
    {
        return None;
    }

    let average_interval_hours = average_interval_duration_hours(hourly_intervals)?;
    let decay_rate = -persistence_correlation.ln() / average_interval_hours;

    (decay_rate.is_finite() && decay_rate > 0.0).then_some(decay_rate)
}

fn side_event_loading_estimate(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> SideEventLoadingEstimate {
    let Some(mean_squared_return) = mean_squared_return(intervals) else {
        return SideEventLoadingEstimate {
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
        return SideEventLoadingEstimate {
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
        return SideEventLoadingEstimate {
            buy_event_loading: 0.0,
            sell_event_loading: 0.0,
        };
    };

    SideEventLoadingEstimate {
        buy_event_loading: coefficients[0],
        sell_event_loading: coefficients[1],
    }
}

fn observed_side_event_rates_per_hour(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> SideEventRateEstimate {
    let mut total_buy_count = 0.0;
    let mut total_sell_count = 0.0;
    let mut total_hours = 0.0;

    for interval in intervals.iter().copied() {
        let counts = long_variance_interval_side_event_counts(interval);

        let interval_hours = duration_nanos_to_hours(
            interval.interval_end_utc_nanos - interval.interval_start_utc_nanos,
        );
        if !interval_hours.is_finite() || interval_hours <= 0.0 {
            continue;
        }

        total_buy_count += counts.buy_count;
        total_sell_count += counts.sell_count;

        total_hours += interval_hours;
    }

    if total_hours <= 0.0 {
        return SideEventRateEstimate {
            buy_event_rate_per_hour: 0.0,
            sell_event_rate_per_hour: 0.0,
        };
    }

    SideEventRateEstimate {
        buy_event_rate_per_hour: total_buy_count / total_hours,
        sell_event_rate_per_hour: total_sell_count / total_hours,
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

fn average_interval_duration_hours(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Option<f64> {
    let interval_hours = intervals
        .iter()
        .filter_map(|interval| {
            let duration_hours = duration_nanos_to_hours(
                interval.interval_end_utc_nanos - interval.interval_start_utc_nanos,
            );

            (duration_hours.is_finite() && duration_hours > 0.0).then_some(duration_hours)
        })
        .collect::<Vec<_>>();
    if interval_hours.is_empty() {
        return None;
    }

    Some(arithmetic_mean_or_zero(interval_hours.as_slice()))
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::{NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR};
    use janiform_model_kernels::long_variance::LongVarianceParameters;

    use super::estimate_long_variance_parameters;
    use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
    use crate::stages::long_variance::policy::long_variance_vol_of_vol_upper_bound_for_mean_reversion_rate;

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

        let estimate = estimate_long_variance_parameters(
            LongVarianceParameters::neutral(),
            five_minute_intervals.as_slice(),
            hourly_intervals.as_slice(),
        );

        let raw_event_loadings =
            super::side_event_loading_estimate(five_minute_intervals.as_slice());

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
