use janiform_foundations::linear_algebra::{
    non_negative_least_squares_2, rank_aware_one_column_least_squares,
};
use janiform_foundations::numeric::arithmetic_mean_or_zero;
use janiform_foundations::time::duration_nanos_to_hours;

use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
use crate::stages::long_variance::metrics::{
    long_variance_interval_side_event_counts, mean_long_variance_squared_return,
};
use crate::stages::long_variance::policy::LongVarianceSideEventLoadings;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LongVarianceEventLoadingEstimation {
    pub(crate) event_loadings: LongVarianceSideEventLoadings,
    pub(crate) candidate_pair_count: usize,
    pub(crate) fit_pair_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LongVarianceReturnEnergyLoadingEstimation {
    pub(crate) stationary_return_energy_rate_per_hour: f64,
    pub(crate) return_energy_loading: f64,
    pub(crate) candidate_pair_count: usize,
    pub(crate) fit_pair_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct KernelCompressedIntervalSignal {
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    value: f64,
}

pub(crate) fn side_event_loading_estimate(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    mean_reversion_rate: f64,
) -> LongVarianceEventLoadingEstimation {
    let candidate_pair_count = intervals.len().saturating_sub(1);
    if !mean_reversion_rate.is_finite() || mean_reversion_rate <= 0.0 {
        return empty_event_loading_estimation(candidate_pair_count);
    }

    let Some(mean_squared_return) = mean_long_variance_squared_return(intervals) else {
        return empty_event_loading_estimation(candidate_pair_count);
    };

    let side_signals = side_count_signals(intervals);
    if side_signals.is_empty() {
        return empty_event_loading_estimation(candidate_pair_count);
    }

    let compressed_buy_counts = kernel_compressed_signals(
        side_signals
            .iter()
            .map(|signals| signals.buy_signal)
            .collect::<Vec<_>>()
            .as_slice(),
        mean_reversion_rate,
    );

    let compressed_sell_counts = kernel_compressed_signals(
        side_signals
            .iter()
            .map(|signals| signals.sell_signal)
            .collect::<Vec<_>>()
            .as_slice(),
        mean_reversion_rate,
    );

    let mut rows = Vec::new();
    let mut response = Vec::new();

    for (pair_index, interval_pair) in intervals.windows(2).enumerate() {
        let current_interval = interval_pair[0];
        let next_interval = interval_pair[1];

        if current_interval.interval_end_utc_nanos != next_interval.interval_start_utc_nanos
            || !next_interval.log_return.is_finite()
        {
            continue;
        }

        let Some(buy_signal) = compressed_buy_counts.get(pair_index) else {
            continue;
        };

        let Some(sell_signal) = compressed_sell_counts.get(pair_index) else {
            continue;
        };

        let next_squared_return = next_interval.log_return * next_interval.log_return;
        if !next_squared_return.is_finite() {
            continue;
        }

        rows.push([buy_signal.value, sell_signal.value]);
        response.push(next_squared_return / mean_squared_return - 1.0);
    }

    if response.is_empty() {
        return empty_event_loading_estimation(candidate_pair_count);
    }

    let Some(coefficients) = non_negative_least_squares_2(rows.as_slice(), response.as_slice())
    else {
        return empty_event_loading_estimation(candidate_pair_count);
    };

    LongVarianceEventLoadingEstimation {
        event_loadings: LongVarianceSideEventLoadings {
            buy_event_loading: coefficients[0],
            sell_event_loading: coefficients[1],
        },
        candidate_pair_count,
        fit_pair_count: response.len(),
    }
}

pub(crate) fn return_energy_loading_estimate(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    mean_reversion_rate: f64,
) -> LongVarianceReturnEnergyLoadingEstimation {
    let candidate_pair_count = intervals.len().saturating_sub(1);

    let Some(stationary_return_energy_rate_per_hour) =
        stationary_return_energy_rate_per_hour(intervals)
    else {
        return empty_return_energy_loading_estimation(candidate_pair_count, 0.0);
    };

    if !mean_reversion_rate.is_finite() || mean_reversion_rate <= 0.0 {
        return empty_return_energy_loading_estimation(
            candidate_pair_count,
            stationary_return_energy_rate_per_hour,
        );
    }

    let energy_signals = return_energy_signals(intervals, stationary_return_energy_rate_per_hour);

    let compressed_energy_residuals =
        kernel_compressed_signals(energy_signals.as_slice(), mean_reversion_rate);

    let mut current_energy_residuals = Vec::new();
    let mut next_energy_responses = Vec::new();

    for (pair_index, interval_pair) in intervals.windows(2).enumerate() {
        let current_interval = interval_pair[0];
        let next_interval = interval_pair[1];
        if current_interval.interval_end_utc_nanos != next_interval.interval_start_utc_nanos {
            continue;
        }

        let Some(compressed_energy_residual) = compressed_energy_residuals.get(pair_index) else {
            continue;
        };

        let next_expected_energy =
            expected_interval_return_energy(next_interval, stationary_return_energy_rate_per_hour);
        if !next_expected_energy.is_finite()
            || next_expected_energy <= 0.0
            || !next_interval.log_return.is_finite()
        {
            continue;
        }

        let next_energy = next_interval.log_return * next_interval.log_return;
        if !next_energy.is_finite() {
            continue;
        }

        current_energy_residuals.push(compressed_energy_residual.value);
        next_energy_responses.push(next_energy / next_expected_energy - 1.0);
    }

    if next_energy_responses.is_empty() {
        return empty_return_energy_loading_estimation(
            candidate_pair_count,
            stationary_return_energy_rate_per_hour,
        );
    }

    let Some(solution) = rank_aware_one_column_least_squares(
        current_energy_residuals.as_slice(),
        next_energy_responses.as_slice(),
    ) else {
        return empty_return_energy_loading_estimation(
            candidate_pair_count,
            stationary_return_energy_rate_per_hour,
        );
    };

    let return_energy_loading = solution.first_coefficient.max(0.0);

    LongVarianceReturnEnergyLoadingEstimation {
        stationary_return_energy_rate_per_hour,
        return_energy_loading,
        candidate_pair_count,
        fit_pair_count: next_energy_responses.len(),
    }
}

pub(crate) fn event_loading_fit_coverage_ratio(
    fit_pair_count: usize,
    candidate_pair_count: usize,
) -> f64 {
    if candidate_pair_count == 0 {
        return 0.0;
    }

    fit_pair_count as f64 / candidate_pair_count as f64
}

fn side_count_signals(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<SideCountSignals> {
    let side_counts = intervals
        .iter()
        .map(|interval| {
            let counts = long_variance_interval_side_event_counts(*interval);

            (*interval, counts.buy_count, counts.sell_count)
        })
        .collect::<Vec<_>>();

    if side_counts.is_empty() {
        return Vec::new();
    }

    let mean_buy_count = arithmetic_mean_or_zero(
        side_counts
            .iter()
            .map(|(_, buy_count, _)| *buy_count)
            .collect::<Vec<_>>()
            .as_slice(),
    );

    let mean_sell_count = arithmetic_mean_or_zero(
        side_counts
            .iter()
            .map(|(_, _, sell_count)| *sell_count)
            .collect::<Vec<_>>()
            .as_slice(),
    );

    side_counts
        .into_iter()
        .map(|(interval, buy_count, sell_count)| SideCountSignals {
            buy_signal: KernelCompressedIntervalSignal {
                interval_start_utc_nanos: interval.interval_start_utc_nanos,
                interval_end_utc_nanos: interval.interval_end_utc_nanos,
                value: buy_count - mean_buy_count,
            },
            sell_signal: KernelCompressedIntervalSignal {
                interval_start_utc_nanos: interval.interval_start_utc_nanos,
                interval_end_utc_nanos: interval.interval_end_utc_nanos,
                value: sell_count - mean_sell_count,
            },
        })
        .collect()
}

fn return_energy_signals(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    stationary_return_energy_rate_per_hour: f64,
) -> Vec<KernelCompressedIntervalSignal> {
    intervals
        .iter()
        .filter_map(|interval| {
            if !interval.log_return.is_finite() {
                return None;
            }

            let expected_energy =
                expected_interval_return_energy(*interval, stationary_return_energy_rate_per_hour);
            if !expected_energy.is_finite() || expected_energy < 0.0 {
                return None;
            }

            let return_energy = interval.log_return * interval.log_return;
            if !return_energy.is_finite() {
                return None;
            }

            Some(KernelCompressedIntervalSignal {
                interval_start_utc_nanos: interval.interval_start_utc_nanos,
                interval_end_utc_nanos: interval.interval_end_utc_nanos,
                value: return_energy - expected_energy,
            })
        })
        .collect()
}

fn kernel_compressed_signals(
    signals: &[KernelCompressedIntervalSignal],
    mean_reversion_rate: f64,
) -> Vec<KernelCompressedIntervalSignal> {
    let mut compressed_signals = Vec::with_capacity(signals.len());
    let mut current_state = 0.0;
    let mut previous_interval_end_utc_nanos = None;

    for signal in signals {
        if let Some(previous_interval_end_utc_nanos) = previous_interval_end_utc_nanos {
            let gap_hours = duration_nanos_to_hours(
                signal.interval_start_utc_nanos - previous_interval_end_utc_nanos,
            );
            current_state = decay_kernel_state(current_state, mean_reversion_rate, gap_hours);
        }

        let interval_hours = duration_nanos_to_hours(
            signal.interval_end_utc_nanos - signal.interval_start_utc_nanos,
        );

        current_state =
            decay_kernel_state(current_state, mean_reversion_rate, interval_hours) + signal.value;

        compressed_signals.push(KernelCompressedIntervalSignal {
            interval_start_utc_nanos: signal.interval_start_utc_nanos,
            interval_end_utc_nanos: signal.interval_end_utc_nanos,
            value: current_state,
        });

        previous_interval_end_utc_nanos = Some(signal.interval_end_utc_nanos);
    }

    compressed_signals
}

fn decay_kernel_state(state: f64, mean_reversion_rate: f64, elapsed_hours: f64) -> f64 {
    if !elapsed_hours.is_finite() || elapsed_hours <= 0.0 {
        return state;
    }

    state * (-mean_reversion_rate * elapsed_hours).exp()
}

fn empty_event_loading_estimation(
    candidate_pair_count: usize,
) -> LongVarianceEventLoadingEstimation {
    LongVarianceEventLoadingEstimation {
        event_loadings: LongVarianceSideEventLoadings {
            buy_event_loading: 0.0,
            sell_event_loading: 0.0,
        },
        candidate_pair_count,
        fit_pair_count: 0,
    }
}

fn empty_return_energy_loading_estimation(
    candidate_pair_count: usize,
    stationary_return_energy_rate_per_hour: f64,
) -> LongVarianceReturnEnergyLoadingEstimation {
    LongVarianceReturnEnergyLoadingEstimation {
        stationary_return_energy_rate_per_hour,
        return_energy_loading: 0.0,
        candidate_pair_count,
        fit_pair_count: 0,
    }
}

fn stationary_return_energy_rate_per_hour(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Option<f64> {
    let mut total_return_energy = 0.0;
    let mut total_interval_hours = 0.0;

    for interval in intervals {
        if !interval.log_return.is_finite() {
            continue;
        }

        let interval_hours = duration_nanos_to_hours(
            interval.interval_end_utc_nanos - interval.interval_start_utc_nanos,
        );
        if !interval_hours.is_finite() || interval_hours <= 0.0 {
            continue;
        }

        let return_energy = interval.log_return * interval.log_return;
        if !return_energy.is_finite() {
            continue;
        }

        total_return_energy += return_energy;
        total_interval_hours += interval_hours;
    }

    if total_interval_hours <= 0.0 {
        return None;
    }

    let rate = total_return_energy / total_interval_hours;

    (rate.is_finite() && rate >= 0.0).then_some(rate)
}

fn expected_interval_return_energy(
    interval: JointRefinementAggregatedIntervalObservation,
    stationary_return_energy_rate_per_hour: f64,
) -> f64 {
    let interval_hours = duration_nanos_to_hours(
        interval.interval_end_utc_nanos - interval.interval_start_utc_nanos,
    );

    stationary_return_energy_rate_per_hour * interval_hours
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SideCountSignals {
    buy_signal: KernelCompressedIntervalSignal,
    sell_signal: KernelCompressedIntervalSignal,
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::NANOS_PER_HOUR;

    use super::{kernel_compressed_signals, return_energy_loading_estimate};
    use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
    use crate::stages::long_variance::feedback::KernelCompressedIntervalSignal;

    fn interval(
        interval_start_utc_nanos: i64,
        log_return: f64,
    ) -> JointRefinementAggregatedIntervalObservation {
        JointRefinementAggregatedIntervalObservation {
            interval_start_utc_nanos,
            interval_end_utc_nanos: interval_start_utc_nanos + NANOS_PER_HOUR,
            log_return,
            tick_count: 10,
            sell_share_proxy: 0.50,
        }
    }

    #[test]
    fn kernel_compression_decays_previous_interval_signal() {
        let signals = [
            KernelCompressedIntervalSignal {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: NANOS_PER_HOUR,
                value: 2.0,
            },
            KernelCompressedIntervalSignal {
                interval_start_utc_nanos: NANOS_PER_HOUR,
                interval_end_utc_nanos: 2 * NANOS_PER_HOUR,
                value: 3.0,
            },
        ];

        let compressed = kernel_compressed_signals(signals.as_slice(), 1.0);

        assert_eq!(compressed[0].value, 2.0);
        assert_eq!(compressed[1].value, 2.0 * (-1.0_f64).exp() + 3.0);
    }

    #[test]
    fn return_energy_loading_uses_compressed_persistent_energy_signal() {
        let intervals = [
            interval(0, 0.01),
            interval(NANOS_PER_HOUR, 0.03),
            interval(2 * NANOS_PER_HOUR, 0.01),
            interval(3 * NANOS_PER_HOUR, 0.03),
        ];

        let estimation = return_energy_loading_estimate(intervals.as_slice(), 0.25);

        assert_eq!(estimation.candidate_pair_count, 3);
        assert_eq!(estimation.fit_pair_count, 3);
        assert!(estimation.stationary_return_energy_rate_per_hour > 0.0);
        assert!(estimation.return_energy_loading >= 0.0);
    }
}
