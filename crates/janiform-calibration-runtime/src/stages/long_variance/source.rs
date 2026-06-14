use janiform_foundations::time::NANOS_PER_HOUR;

use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::stages::coarse_data::coarse_data_interval_observations;
use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
use crate::stages::long_variance::metrics::aggregate_intervals_to_fixed_duration;

pub(crate) const LONG_VARIANCE_SLOW_HORIZON_SOURCE_SELECTION_POLICY: &str = concat!(
    "coarse_records_at_or_below_one_hour_aggregate_to_",
    "one_hour_log_return_variance_units_with_tick_gap_fill.v1",
);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LongVarianceSlowHorizonIntervalSelection {
    pub(crate) intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) tick_interval_count: usize,
    pub(crate) coarse_interval_count: usize,
    pub(crate) omitted_overlapping_tick_interval_count: usize,
}

pub(crate) fn select_long_variance_slow_horizon_intervals(
    tick_hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
    coarse_data_records: Option<&[MarketDataNormalizedCoarseDataRecord]>,
) -> LongVarianceSlowHorizonIntervalSelection {
    let mut coarse_intervals = coarse_data_records
        .map(coarse_data_hourly_interval_observations)
        .unwrap_or_default();

    if coarse_intervals.is_empty() {
        return LongVarianceSlowHorizonIntervalSelection {
            intervals: tick_hourly_intervals.to_vec(),
            tick_interval_count: tick_hourly_intervals.len(),
            coarse_interval_count: 0,
            omitted_overlapping_tick_interval_count: 0,
        };
    }

    sort_intervals_by_time(coarse_intervals.as_mut_slice());

    let mut selected_intervals = Vec::with_capacity(
        coarse_intervals
            .len()
            .saturating_add(tick_hourly_intervals.len()),
    );
    selected_intervals.extend(coarse_intervals.iter().copied());
    let mut omitted_overlapping_tick_interval_count = 0usize;
    let mut coarse_search_position = 0usize;

    for tick_interval in tick_hourly_intervals {
        while coarse_intervals
            .get(coarse_search_position)
            .is_some_and(|coarse_interval| {
                coarse_interval.interval_end_utc_nanos <= tick_interval.interval_start_utc_nanos
            })
        {
            coarse_search_position = coarse_search_position.saturating_add(1);
        }

        let tick_overlaps_coarse = coarse_intervals
            .iter()
            .skip(coarse_search_position)
            .take_while(|coarse_interval| {
                coarse_interval.interval_start_utc_nanos < tick_interval.interval_end_utc_nanos
            })
            .any(|coarse_interval| intervals_overlap(*tick_interval, *coarse_interval));

        if tick_overlaps_coarse {
            omitted_overlapping_tick_interval_count =
                omitted_overlapping_tick_interval_count.saturating_add(1);
        } else {
            selected_intervals.push(*tick_interval);
        }
    }

    sort_intervals_by_time(selected_intervals.as_mut_slice());

    LongVarianceSlowHorizonIntervalSelection {
        intervals: selected_intervals,
        tick_interval_count: tick_hourly_intervals.len(),
        coarse_interval_count: coarse_intervals.len(),
        omitted_overlapping_tick_interval_count,
    }
}

fn intervals_overlap(
    left: JointRefinementAggregatedIntervalObservation,
    right: JointRefinementAggregatedIntervalObservation,
) -> bool {
    left.interval_start_utc_nanos < right.interval_end_utc_nanos
        && right.interval_start_utc_nanos < left.interval_end_utc_nanos
}

fn coarse_data_hourly_interval_observations(
    records: &[MarketDataNormalizedCoarseDataRecord],
) -> Vec<JointRefinementAggregatedIntervalObservation> {
    let mut intervals = coarse_data_interval_observations(records)
        .into_iter()
        .filter(|interval| {
            let Some(interval_duration_nanos) = interval
                .interval_end_utc_nanos
                .checked_sub(interval.interval_start_utc_nanos)
            else {
                return false;
            };

            interval_duration_nanos > 0 && interval_duration_nanos <= NANOS_PER_HOUR
        })
        .collect::<Vec<_>>();

    sort_intervals_by_time(intervals.as_mut_slice());
    aggregate_intervals_to_fixed_duration(intervals.as_slice(), NANOS_PER_HOUR)
}

fn sort_intervals_by_time(intervals: &mut [JointRefinementAggregatedIntervalObservation]) {
    intervals.sort_by(|left, right| {
        left.interval_start_utc_nanos
            .cmp(&right.interval_start_utc_nanos)
            .then(
                left.interval_end_utc_nanos
                    .cmp(&right.interval_end_utc_nanos),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::select_long_variance_slow_horizon_intervals;
    use janiform_foundations::time::NANOS_PER_HOUR;

    use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
    use crate::stages::joint_metrics::{
        JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY, JointRefinementAggregatedIntervalObservation,
    };

    fn tick_interval(
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    ) -> JointRefinementAggregatedIntervalObservation {
        JointRefinementAggregatedIntervalObservation {
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            log_return: 0.1,
            tick_count: 10,
            sell_share_proxy: JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY,
        }
    }

    fn coarse_record(
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    ) -> MarketDataNormalizedCoarseDataRecord {
        MarketDataNormalizedCoarseDataRecord {
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            observation_timestamp_utc_nanos: interval_end_utc_nanos,
            close_price: 100.0_f64.exp(),
            open_price: Some(100.0),
            high_price: Some(101.0),
            low_price: Some(99.0),
            volume: Some(0.0),
            close_to_close_log_return: Some(0.5),
            open_to_close_log_return: None,
            close_to_open_gap_log_return: None,
            high_low_log_range: None,
            source_row_number: 1,
        }
    }

    #[test]
    fn slow_horizon_source_selection_uses_coarse_overlap_and_tick_gaps() {
        let tick_intervals = vec![
            tick_interval(NANOS_PER_HOUR, 2 * NANOS_PER_HOUR),
            tick_interval(2 * NANOS_PER_HOUR, 3 * NANOS_PER_HOUR),
        ];

        let coarse_records = vec![
            coarse_record(0, NANOS_PER_HOUR),
            coarse_record(NANOS_PER_HOUR, 2 * NANOS_PER_HOUR),
            coarse_record(3 * NANOS_PER_HOUR, 4 * NANOS_PER_HOUR),
        ];

        let selection = select_long_variance_slow_horizon_intervals(
            tick_intervals.as_slice(),
            Some(coarse_records.as_slice()),
        );

        let intervals = selection
            .intervals
            .iter()
            .map(|interval| {
                (
                    interval.interval_start_utc_nanos,
                    interval.interval_end_utc_nanos,
                    interval.tick_count,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            intervals,
            vec![
                (0, NANOS_PER_HOUR, 0),
                (NANOS_PER_HOUR, 2 * NANOS_PER_HOUR, 0),
                (2 * NANOS_PER_HOUR, 3 * NANOS_PER_HOUR, 10),
                (3 * NANOS_PER_HOUR, 4 * NANOS_PER_HOUR, 0),
            ]
        );
        assert_eq!(selection.omitted_overlapping_tick_interval_count, 1);
    }

    #[test]
    fn slow_horizon_source_selection_aggregates_subhour_coarse_returns_to_hourly_units() {
        let minute_nanos = NANOS_PER_HOUR / 60;

        let coarse_records = (0_i64..60)
            .map(|minute_position| {
                coarse_record(
                    minute_position * minute_nanos,
                    (minute_position + 1) * minute_nanos,
                )
            })
            .collect::<Vec<_>>();

        let selection =
            select_long_variance_slow_horizon_intervals(&[], Some(coarse_records.as_slice()));

        assert_eq!(selection.coarse_interval_count, 1);
        assert_eq!(
            selection.intervals,
            vec![JointRefinementAggregatedIntervalObservation {
                interval_start_utc_nanos: 0,
                interval_end_utc_nanos: NANOS_PER_HOUR,
                log_return: 30.0,
                tick_count: 0,
                sell_share_proxy: JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY,
            }]
        );
    }

    #[test]
    fn slow_horizon_source_selection_excludes_coarse_records_above_hourly_units() {
        let coarse_records = vec![coarse_record(0, 2 * NANOS_PER_HOUR)];

        let selection =
            select_long_variance_slow_horizon_intervals(&[], Some(coarse_records.as_slice()));

        assert_eq!(selection.coarse_interval_count, 0);
        assert!(selection.intervals.is_empty());
    }
}
