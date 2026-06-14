use serde::{Deserialize, Serialize};

use janiform_foundations::numeric::{
    adjusted_shape_jackknife_standard_errors, arithmetic_mean_or_zero,
    jackknife_standard_error_from_leave_one_out_values, pearson_correlation,
    sample_adjusted_excess_kurtosis, sample_adjusted_skewness, sample_standard_deviation,
    sample_variance,
};
use janiform_foundations::time::{
    NANOS_PER_DAY, NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR, NANOS_PER_WEEK,
    floor_to_interval_utc_nanos,
};

use crate::stages::joint_metrics::{
    JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY, JointRefinementAggregatedIntervalObservation,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LongVarianceHorizonDefinition {
    pub(crate) label: &'static str,
    pub(crate) hours: f64,
}

pub(crate) const LONG_VARIANCE_HORIZON_DEFINITIONS: [LongVarianceHorizonDefinition; 4] = [
    LongVarianceHorizonDefinition {
        label: "5min",
        hours: NANOS_PER_FIVE_MINUTES as f64 / NANOS_PER_HOUR as f64,
    },
    LongVarianceHorizonDefinition {
        label: "1h",
        hours: 1.0,
    },
    LongVarianceHorizonDefinition {
        label: "1d",
        hours: NANOS_PER_DAY as f64 / NANOS_PER_HOUR as f64,
    },
    LongVarianceHorizonDefinition {
        label: "1w",
        hours: NANOS_PER_WEEK as f64 / NANOS_PER_HOUR as f64,
    },
];
pub(crate) const LONG_VARIANCE_HORIZON_MOMENT_METRIC_COUNT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct LongVarianceHorizonMomentSummary {
    pub(crate) sample_count: usize,
    pub(crate) realized_variance: Option<f64>,
    pub(crate) variance_of_variance: Option<f64>,
    pub(crate) return_skewness: Option<f64>,
    pub(crate) return_excess_kurtosis: Option<f64>,
    pub(crate) activity_variance_correlation: Option<f64>,
    pub(crate) signed_activity_future_variance_relation: Option<f64>,
    pub(crate) standard_errors: LongVarianceHorizonMomentStandardErrors,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LongVarianceHorizonSummaryComputation {
    pub(crate) horizon_position: usize,
    pub(crate) intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) block_duration_nanos: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct LongVarianceHorizonMomentStandardErrors {
    pub(crate) realized_variance: Option<f64>,
    pub(crate) variance_of_variance: Option<f64>,
    pub(crate) return_skewness: Option<f64>,
    pub(crate) return_excess_kurtosis: Option<f64>,
    pub(crate) activity_variance_correlation: Option<f64>,
    pub(crate) signed_activity_future_variance_relation: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LongVarianceIntervalSideEventCounts {
    pub(crate) buy_count: f64,
    pub(crate) sell_count: f64,
}

pub(crate) fn long_variance_interval_side_event_counts(
    interval: JointRefinementAggregatedIntervalObservation,
) -> LongVarianceIntervalSideEventCounts {
    let sell_share_proxy = if interval.sell_share_proxy.is_finite() {
        interval.sell_share_proxy.clamp(0.0, 1.0)
    } else {
        JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY
    };

    let tick_count = interval.tick_count as f64;

    LongVarianceIntervalSideEventCounts {
        buy_count: tick_count * (1.0 - sell_share_proxy),
        sell_count: tick_count * sell_share_proxy,
    }
}

pub(crate) fn finite_long_variance_log_returns(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<f64> {
    intervals
        .iter()
        .filter_map(|interval| {
            interval
                .log_return
                .is_finite()
                .then_some(interval.log_return)
        })
        .collect()
}

pub(crate) fn finite_long_variance_squared_returns(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<f64> {
    finite_long_variance_log_returns(intervals)
        .into_iter()
        .map(|log_return| log_return * log_return)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct IntervalAggregationAccumulator {
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    log_return: f64,
    tick_count: usize,
    weighted_sell_share_sum: f64,
}

impl IntervalAggregationAccumulator {
    fn new(
        interval_start_utc_nanos: i64,
        interval_duration_nanos: i64,
        interval: JointRefinementAggregatedIntervalObservation,
    ) -> Self {
        let mut accumulator = Self {
            interval_start_utc_nanos,
            interval_end_utc_nanos: interval_start_utc_nanos
                .saturating_add(interval_duration_nanos),
            log_return: 0.0,
            tick_count: 0,
            weighted_sell_share_sum: 0.0,
        };

        accumulator.accumulate(interval);
        accumulator
    }

    fn accumulate(&mut self, interval: JointRefinementAggregatedIntervalObservation) {
        self.log_return += interval.log_return;
        self.tick_count = self.tick_count.saturating_add(interval.tick_count);
        self.weighted_sell_share_sum += interval.sell_share_proxy * interval.tick_count as f64;
    }

    fn finish(self) -> JointRefinementAggregatedIntervalObservation {
        JointRefinementAggregatedIntervalObservation {
            interval_start_utc_nanos: self.interval_start_utc_nanos,
            interval_end_utc_nanos: self.interval_end_utc_nanos,
            log_return: self.log_return,
            tick_count: self.tick_count,
            sell_share_proxy: if self.tick_count == 0 {
                JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY
            } else {
                self.weighted_sell_share_sum / self.tick_count as f64
            },
        }
    }
}

pub(crate) fn aggregate_intervals_to_fixed_duration(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    interval_duration_nanos: i64,
) -> Vec<JointRefinementAggregatedIntervalObservation> {
    if intervals.is_empty() || interval_duration_nanos <= 0 {
        return Vec::new();
    }

    let mut aggregated_intervals = Vec::new();
    let mut current_accumulator: Option<IntervalAggregationAccumulator> = None;

    for interval in intervals.iter().copied() {
        let bucket_start =
            floor_to_interval_utc_nanos(interval.interval_start_utc_nanos, interval_duration_nanos);

        match current_accumulator.as_mut() {
            Some(accumulator) if accumulator.interval_start_utc_nanos == bucket_start => {
                accumulator.accumulate(interval);
            }
            Some(_) => {
                if let Some(accumulator) = current_accumulator.take() {
                    aggregated_intervals.push(accumulator.finish());
                }

                current_accumulator = Some(IntervalAggregationAccumulator::new(
                    bucket_start,
                    interval_duration_nanos,
                    interval,
                ));
            }
            None => {
                current_accumulator = Some(IntervalAggregationAccumulator::new(
                    bucket_start,
                    interval_duration_nanos,
                    interval,
                ));
            }
        }
    }

    if let Some(accumulator) = current_accumulator {
        aggregated_intervals.push(accumulator.finish());
    }

    aggregated_intervals
}

pub(crate) fn long_variance_horizon_moment_summary(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> LongVarianceHorizonMomentSummary {
    long_variance_horizon_moment_summary_without_standard_errors(intervals)
}

fn long_variance_horizon_moment_summary_without_standard_errors(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> LongVarianceHorizonMomentSummary {
    let finite_intervals = intervals
        .iter()
        .copied()
        .filter(|interval| interval.log_return.is_finite())
        .collect::<Vec<_>>();

    let returns = finite_intervals
        .iter()
        .map(|interval| interval.log_return)
        .collect::<Vec<_>>();

    let squared_returns = finite_long_variance_squared_returns(finite_intervals.as_slice());

    let activities = finite_intervals
        .iter()
        .map(|interval| interval.tick_count as f64)
        .collect::<Vec<_>>();

    let signed_activities = finite_intervals
        .iter()
        .map(|interval| {
            (interval.sell_share_proxy - JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY)
                * interval.tick_count as f64
        })
        .collect::<Vec<_>>();

    LongVarianceHorizonMomentSummary {
        sample_count: returns.len(),
        realized_variance: non_empty_value(&squared_returns, arithmetic_mean_or_zero),
        variance_of_variance: sample_variance(squared_returns.as_slice()),
        return_skewness: sample_adjusted_skewness(returns.as_slice()),
        return_excess_kurtosis: sample_adjusted_excess_kurtosis(returns.as_slice()),
        activity_variance_correlation: finite_correlation(
            activities.as_slice(),
            squared_returns.as_slice(),
        ),
        signed_activity_future_variance_relation: signed_activity_future_variance_relation(
            signed_activities.as_slice(),
            squared_returns.as_slice(),
        ),
        standard_errors: LongVarianceHorizonMomentStandardErrors::default(),
    }
}

pub(crate) fn long_variance_horizon_summaries(
    five_minute_intervals: &[JointRefinementAggregatedIntervalObservation],
    hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<LongVarianceHorizonMomentSummary> {
    let computations =
        long_variance_horizon_summary_computations(five_minute_intervals, hourly_intervals);

    ordered_long_variance_horizon_summaries_from_outputs(
        computations
            .into_iter()
            .map(compute_long_variance_horizon_summary)
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn long_variance_horizon_summary_computations(
    five_minute_intervals: &[JointRefinementAggregatedIntervalObservation],
    hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<LongVarianceHorizonSummaryComputation> {
    let daily_intervals = aggregate_intervals_to_fixed_duration(hourly_intervals, NANOS_PER_DAY);
    let weekly_intervals = aggregate_intervals_to_fixed_duration(hourly_intervals, NANOS_PER_WEEK);

    vec![
        LongVarianceHorizonSummaryComputation {
            horizon_position: 0,
            intervals: five_minute_intervals.to_vec(),
            block_duration_nanos: Some(NANOS_PER_HOUR),
        },
        LongVarianceHorizonSummaryComputation {
            horizon_position: 1,
            intervals: hourly_intervals.to_vec(),
            block_duration_nanos: Some(NANOS_PER_DAY),
        },
        LongVarianceHorizonSummaryComputation {
            horizon_position: 2,
            intervals: daily_intervals,
            block_duration_nanos: Some(NANOS_PER_WEEK),
        },
        LongVarianceHorizonSummaryComputation {
            horizon_position: 3,
            intervals: weekly_intervals,
            block_duration_nanos: None,
        },
    ]
}

pub(crate) fn compute_long_variance_horizon_summary(
    computation: LongVarianceHorizonSummaryComputation,
) -> (usize, LongVarianceHorizonMomentSummary) {
    let summary = match computation.block_duration_nanos {
        Some(block_duration_nanos) => {
            long_variance_horizon_moment_summary_with_block_standard_errors(
                computation.intervals.as_slice(),
                block_duration_nanos,
            )
        }
        None => long_variance_horizon_moment_summary_with_jackknife_standard_errors(
            computation.intervals.as_slice(),
        ),
    };

    (computation.horizon_position, summary)
}

fn long_variance_horizon_moment_summary_with_jackknife_standard_errors(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> LongVarianceHorizonMomentSummary {
    let mut summary = long_variance_horizon_moment_summary(intervals);

    summary.standard_errors = leave_one_out_horizon_standard_errors(intervals);

    summary
}

pub(crate) fn ordered_long_variance_horizon_summaries_from_outputs(
    mut outputs: Vec<(usize, LongVarianceHorizonMomentSummary)>,
) -> Vec<LongVarianceHorizonMomentSummary> {
    outputs.sort_by_key(|(horizon_position, _)| *horizon_position);
    outputs.into_iter().map(|(_, summary)| summary).collect()
}

fn long_variance_horizon_moment_summary_with_block_standard_errors(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    block_duration_nanos: i64,
) -> LongVarianceHorizonMomentSummary {
    let mut summary = long_variance_horizon_moment_summary(intervals);

    let block_standard_errors =
        long_variance_horizon_block_standard_errors(intervals, block_duration_nanos);

    let jackknife_standard_errors = long_variance_horizon_jackknife_standard_errors(intervals);

    summary.standard_errors =
        combine_horizon_standard_errors(block_standard_errors, jackknife_standard_errors);

    summary
}

fn combine_horizon_standard_errors(
    block_standard_errors: LongVarianceHorizonMomentStandardErrors,
    jackknife_standard_errors: LongVarianceHorizonMomentStandardErrors,
) -> LongVarianceHorizonMomentStandardErrors {
    LongVarianceHorizonMomentStandardErrors {
        realized_variance: block_standard_errors.realized_variance,
        variance_of_variance: block_standard_errors.variance_of_variance,
        return_skewness: maximum_standard_error(
            block_standard_errors.return_skewness,
            jackknife_standard_errors.return_skewness,
        ),
        return_excess_kurtosis: maximum_standard_error(
            block_standard_errors.return_excess_kurtosis,
            jackknife_standard_errors.return_excess_kurtosis,
        ),
        activity_variance_correlation: block_standard_errors.activity_variance_correlation,
        signed_activity_future_variance_relation: block_standard_errors
            .signed_activity_future_variance_relation,
    }
}

fn maximum_standard_error(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn long_variance_horizon_block_standard_errors(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    block_duration_nanos: i64,
) -> LongVarianceHorizonMomentStandardErrors {
    let block_summaries = horizon_block_summaries(intervals, block_duration_nanos);

    LongVarianceHorizonMomentStandardErrors {
        realized_variance: block_standard_error(block_summaries.as_slice(), |summary| {
            summary.realized_variance
        }),
        variance_of_variance: block_standard_error(block_summaries.as_slice(), |summary| {
            summary.variance_of_variance
        }),
        return_skewness: block_standard_error(block_summaries.as_slice(), |summary| {
            summary.return_skewness
        }),
        return_excess_kurtosis: block_standard_error(block_summaries.as_slice(), |summary| {
            summary.return_excess_kurtosis
        }),
        activity_variance_correlation: block_standard_error(
            block_summaries.as_slice(),
            |summary| summary.activity_variance_correlation,
        ),
        signed_activity_future_variance_relation: block_standard_error(
            block_summaries.as_slice(),
            |summary| summary.signed_activity_future_variance_relation,
        ),
    }
}

fn horizon_block_summaries(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    block_duration_nanos: i64,
) -> Vec<LongVarianceHorizonMomentSummary> {
    intervals_by_block_duration(intervals, block_duration_nanos)
        .into_iter()
        .map(|block_intervals| {
            long_variance_horizon_moment_summary_without_standard_errors(block_intervals.as_slice())
        })
        .collect()
}

fn intervals_by_block_duration(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    block_duration_nanos: i64,
) -> Vec<Vec<JointRefinementAggregatedIntervalObservation>> {
    if intervals.is_empty() || block_duration_nanos <= 0 {
        return Vec::new();
    }

    let mut blocks = Vec::new();
    let mut current_block_start: Option<i64> = None;
    let mut current_block = Vec::new();

    for interval in intervals.iter().copied() {
        let block_start =
            floor_to_interval_utc_nanos(interval.interval_start_utc_nanos, block_duration_nanos);

        if current_block_start == Some(block_start) {
            current_block.push(interval);
            continue;
        }

        if !current_block.is_empty() {
            blocks.push(current_block);
            current_block = Vec::new();
        }

        current_block_start = Some(block_start);

        current_block.push(interval);
    }

    if !current_block.is_empty() {
        blocks.push(current_block);
    }

    blocks
}

fn long_variance_horizon_jackknife_standard_errors(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> LongVarianceHorizonMomentStandardErrors {
    let returns = finite_long_variance_log_returns(intervals);
    let shape_standard_errors = adjusted_shape_jackknife_standard_errors(returns.as_slice());

    LongVarianceHorizonMomentStandardErrors {
        realized_variance: None,
        variance_of_variance: None,
        return_skewness: shape_standard_errors.skewness,
        return_excess_kurtosis: shape_standard_errors.excess_kurtosis,
        activity_variance_correlation: None,
        signed_activity_future_variance_relation: None,
    }
}

fn leave_one_out_horizon_standard_errors(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> LongVarianceHorizonMomentStandardErrors {
    LongVarianceHorizonMomentStandardErrors {
        realized_variance: leave_one_out_horizon_standard_error(intervals, |summary| {
            summary.realized_variance
        }),
        variance_of_variance: leave_one_out_horizon_standard_error(intervals, |summary| {
            summary.variance_of_variance
        }),
        return_skewness: leave_one_out_horizon_standard_error(intervals, |summary| {
            summary.return_skewness
        }),
        return_excess_kurtosis: leave_one_out_horizon_standard_error(intervals, |summary| {
            summary.return_excess_kurtosis
        }),
        activity_variance_correlation: leave_one_out_horizon_standard_error(intervals, |summary| {
            summary.activity_variance_correlation
        }),
        signed_activity_future_variance_relation: leave_one_out_horizon_standard_error(
            intervals,
            |summary| summary.signed_activity_future_variance_relation,
        ),
    }
}

fn leave_one_out_horizon_standard_error(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    extract_metric: fn(LongVarianceHorizonMomentSummary) -> Option<f64>,
) -> Option<f64> {
    if intervals.len() < 3 {
        return None;
    }

    let mut leave_one_out_values = Vec::with_capacity(intervals.len());

    for omitted_position in 0..intervals.len() {
        let leave_one_out_intervals = intervals
            .iter()
            .enumerate()
            .filter_map(|(interval_position, interval)| {
                (interval_position != omitted_position).then_some(*interval)
            })
            .collect::<Vec<_>>();

        let summary = long_variance_horizon_moment_summary_without_standard_errors(
            leave_one_out_intervals.as_slice(),
        );
        if let Some(value) = extract_metric(summary)
            && value.is_finite()
        {
            leave_one_out_values.push(value);
        }
    }

    jackknife_standard_error_from_leave_one_out_values(leave_one_out_values.as_slice())
}

fn block_standard_error(
    block_summaries: &[LongVarianceHorizonMomentSummary],
    extract_metric: fn(LongVarianceHorizonMomentSummary) -> Option<f64>,
) -> Option<f64> {
    let values = block_summaries
        .iter()
        .copied()
        .filter_map(extract_metric)
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();

    if values.len() < 2 {
        return None;
    }

    let standard_error =
        sample_standard_deviation(values.as_slice()) / (values.len() as f64).sqrt();

    (standard_error.is_finite() && standard_error > 0.0).then_some(standard_error)
}

fn non_empty_value(values: &[f64], compute_value: fn(&[f64]) -> f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    let value = compute_value(values);
    value.is_finite().then_some(value)
}

fn finite_correlation(left: &[f64], right: &[f64]) -> Option<f64> {
    let correlation = pearson_correlation(left, right);
    correlation.is_finite().then_some(correlation)
}

fn signed_activity_future_variance_relation(
    signed_activities: &[f64],
    squared_returns: &[f64],
) -> Option<f64> {
    if signed_activities.len() != squared_returns.len() || signed_activities.len() < 2 {
        return None;
    }

    finite_correlation(
        &signed_activities[..signed_activities.len() - 1],
        &squared_returns[1..],
    )
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_intervals_to_fixed_duration, long_variance_horizon_moment_summary,
        long_variance_horizon_summaries, long_variance_interval_side_event_counts,
    };
    use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
    use janiform_foundations::time::{
        NANOS_PER_DAY, NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR, NANOS_PER_WEEK, SECONDS_PER_HOUR_F64,
    };

    fn interval(
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
        log_return: f64,
        tick_count: usize,
        sell_share_proxy: f64,
    ) -> JointRefinementAggregatedIntervalObservation {
        JointRefinementAggregatedIntervalObservation {
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            log_return,
            tick_count,
            sell_share_proxy,
        }
    }

    #[test]
    fn aggregate_intervals_to_fixed_duration_preserves_return_count_and_sell_share() {
        let intervals = [
            interval(0, 5, 0.125, 2, 0.25),
            interval(5, 10, -0.0625, 6, 0.75),
            interval(10, 15, 0.04, 4, 0.50),
        ];

        let aggregated = aggregate_intervals_to_fixed_duration(intervals.as_slice(), 10);

        assert_eq!(
            aggregated,
            vec![
                interval(0, 10, 0.0625, 8, 0.625),
                interval(10, 20, 0.04, 4, 0.50),
            ],
        );
    }

    #[test]
    fn aggregate_intervals_to_fixed_duration_preserves_scale_across_second_minute_and_hour_units() {
        let second_nanos = NANOS_PER_HOUR / SECONDS_PER_HOUR_F64 as i64;
        let minute_nanos = NANOS_PER_HOUR / 60;

        let second_intervals = (0_i64..3_600)
            .map(|second_position| {
                interval(
                    second_position * second_nanos,
                    (second_position + 1) * second_nanos,
                    1.0,
                    1,
                    0.50,
                )
            })
            .collect::<Vec<_>>();

        let minute_intervals = (0_i64..60)
            .map(|minute_position| {
                interval(
                    minute_position * minute_nanos,
                    (minute_position + 1) * minute_nanos,
                    60.0,
                    60,
                    0.50,
                )
            })
            .collect::<Vec<_>>();

        let hourly_intervals = [interval(0, NANOS_PER_HOUR, 3_600.0, 3_600, 0.50)];

        let aggregated_seconds =
            aggregate_intervals_to_fixed_duration(second_intervals.as_slice(), NANOS_PER_HOUR);

        let aggregated_minutes =
            aggregate_intervals_to_fixed_duration(minute_intervals.as_slice(), NANOS_PER_HOUR);

        assert_eq!(aggregated_seconds, hourly_intervals);
        assert_eq!(aggregated_minutes, hourly_intervals);
    }

    #[test]
    fn aggregate_intervals_to_fixed_duration_preserves_scale_across_hour_and_day_units() {
        let hourly_intervals = (0_i64..24)
            .map(|hour_position| {
                interval(
                    hour_position * NANOS_PER_HOUR,
                    (hour_position + 1) * NANOS_PER_HOUR,
                    1.0,
                    10,
                    0.50,
                )
            })
            .collect::<Vec<_>>();

        let daily_intervals =
            aggregate_intervals_to_fixed_duration(hourly_intervals.as_slice(), NANOS_PER_DAY);

        assert_eq!(
            daily_intervals,
            vec![interval(0, NANOS_PER_DAY, 24.0, 240, 0.50)]
        );
    }

    #[test]
    fn long_variance_horizon_summary_keeps_variance_and_activity_shape() {
        let intervals = [
            interval(0, 5, 0.10, 2, 0.20),
            interval(5, 10, -0.05, 5, 0.80),
            interval(10, 15, 0.02, 3, 0.60),
            interval(15, 20, -0.08, 8, 0.75),
        ];

        let summary = long_variance_horizon_moment_summary(intervals.as_slice());

        assert_eq!(summary.sample_count, 4);
        assert!(summary.realized_variance.is_some());
        assert!(summary.variance_of_variance.is_some());
        assert!(summary.activity_variance_correlation.is_some());
        assert!(summary.signed_activity_future_variance_relation.is_some());
    }

    #[test]
    fn long_variance_horizon_summaries_return_specified_horizon_count() {
        let five_minute_intervals = [
            interval(0, 5, 0.10, 2, 0.20),
            interval(5, 10, -0.05, 5, 0.80),
        ];

        let hourly_intervals = [
            interval(0, 60, 0.12, 12, 0.45),
            interval(60, 120, -0.07, 10, 0.55),
        ];

        let summaries = long_variance_horizon_summaries(
            five_minute_intervals.as_slice(),
            hourly_intervals.as_slice(),
        );

        assert_eq!(summaries.len(), 4);
        assert_eq!(summaries[0].sample_count, 2);
        assert_eq!(summaries[1].sample_count, 2);
    }

    #[test]
    fn long_variance_horizon_summaries_attach_block_standard_errors() {
        let five_minute_intervals = (0..24)
            .map(|interval_position| {
                interval(
                    interval_position * NANOS_PER_FIVE_MINUTES,
                    (interval_position + 1) * NANOS_PER_FIVE_MINUTES,
                    if interval_position < 12 { 0.01 } else { -0.03 },
                    10 + interval_position as usize,
                    0.50,
                )
            })
            .collect::<Vec<_>>();

        let hourly_intervals = (0..48)
            .map(|interval_position| {
                interval(
                    interval_position * NANOS_PER_HOUR,
                    (interval_position + 1) * NANOS_PER_HOUR,
                    if interval_position < 24 { 0.02 } else { -0.04 },
                    100 + interval_position as usize,
                    0.50,
                )
            })
            .collect::<Vec<_>>();

        let summaries = long_variance_horizon_summaries(
            five_minute_intervals.as_slice(),
            hourly_intervals.as_slice(),
        );

        assert!(summaries[0].standard_errors.realized_variance.is_some());
        assert!(summaries[1].standard_errors.realized_variance.is_some());
        assert!(summaries[3].standard_errors.realized_variance.is_none());
    }

    #[test]
    fn weekly_long_variance_horizon_uses_jackknife_standard_errors() {
        let five_minute_intervals = [
            interval(0, 5, 0.10, 2, 0.20),
            interval(5, 10, -0.05, 5, 0.80),
        ];

        let hourly_intervals = [
            interval(0, NANOS_PER_HOUR, 0.01, 10, 0.50),
            interval(
                NANOS_PER_WEEK,
                NANOS_PER_WEEK + NANOS_PER_HOUR,
                0.03,
                20,
                0.50,
            ),
            interval(
                2 * NANOS_PER_WEEK,
                2 * NANOS_PER_WEEK + NANOS_PER_HOUR,
                -0.06,
                30,
                0.50,
            ),
        ];

        let summaries = long_variance_horizon_summaries(
            five_minute_intervals.as_slice(),
            hourly_intervals.as_slice(),
        );

        assert_eq!(summaries[3].sample_count, 3);
        assert!(summaries[3].standard_errors.realized_variance.is_some());
    }

    #[test]
    fn interval_side_event_counts_uses_share_bounds() {
        let counts =
            long_variance_interval_side_event_counts(interval(0, NANOS_PER_WEEK, 0.0, 10, 1.5));

        assert_eq!(counts.buy_count, 0.0);
        assert_eq!(counts.sell_count, 10.0);
    }
}
