use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::stages::coarse_data::coarse_data_hourly_return_interval_observations;
use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;

pub(crate) const LONG_VARIANCE_INTERVAL_SOURCE_SELECTION_POLICY: &str = concat!(
    "tick_hourly_return_and_activity_with_coarse_hourly_return_gap_fill_",
    "from_records_at_or_below_one_hour.v1",
);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LongVarianceObservedIntervalSelection {
    pub(crate) return_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) activity_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) return_interval_tick_count: usize,
    pub(crate) return_interval_coarse_candidate_count: usize,
    pub(crate) return_interval_coarse_gap_fill_count: usize,
    pub(crate) return_interval_tick_coarse_overlap_count: usize,
}

pub(crate) fn select_long_variance_observed_intervals(
    tick_hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
    coarse_data_records: Option<&[MarketDataNormalizedCoarseDataRecord]>,
) -> LongVarianceObservedIntervalSelection {
    let mut tick_intervals = tick_hourly_intervals.to_vec();

    sort_intervals_by_time(tick_intervals.as_mut_slice());

    let mut coarse_intervals = coarse_data_records
        .map(coarse_data_hourly_return_interval_observations)
        .unwrap_or_default();

    if coarse_intervals.is_empty() {
        // Return and activity consumers own separate streams because later aggregation uses
        // different horizon policies for each stream.
        let activity_intervals = tick_intervals.clone();

        return LongVarianceObservedIntervalSelection {
            return_intervals: tick_intervals,
            activity_intervals,
            return_interval_tick_count: tick_hourly_intervals.len(),
            return_interval_coarse_candidate_count: 0,
            return_interval_coarse_gap_fill_count: 0,
            return_interval_tick_coarse_overlap_count: 0,
        };
    }

    sort_intervals_by_time(coarse_intervals.as_mut_slice());

    let mut selected_intervals = Vec::with_capacity(
        coarse_intervals
            .len()
            .saturating_add(tick_hourly_intervals.len()),
    );

    selected_intervals.extend(tick_intervals.iter().copied());

    let mut return_interval_tick_coarse_overlap_count = 0usize;
    let mut tick_search_position = 0usize;

    for coarse_interval in coarse_intervals.iter().copied() {
        while tick_intervals
            .get(tick_search_position)
            .is_some_and(|tick_interval| {
                tick_interval.interval_end_utc_nanos <= coarse_interval.interval_start_utc_nanos
            })
        {
            tick_search_position = tick_search_position.saturating_add(1);
        }

        let coarse_overlaps_tick = tick_intervals
            .iter()
            .skip(tick_search_position)
            .take_while(|tick_interval| {
                tick_interval.interval_start_utc_nanos < coarse_interval.interval_end_utc_nanos
            })
            .any(|tick_interval| intervals_overlap(coarse_interval, *tick_interval));

        if coarse_overlaps_tick {
            return_interval_tick_coarse_overlap_count =
                return_interval_tick_coarse_overlap_count.saturating_add(1);
        } else {
            selected_intervals.push(coarse_interval);
        }
    }

    sort_intervals_by_time(selected_intervals.as_mut_slice());

    let return_interval_coarse_gap_fill_count = selected_intervals
        .len()
        .saturating_sub(tick_hourly_intervals.len());

    LongVarianceObservedIntervalSelection {
        return_intervals: selected_intervals,
        activity_intervals: tick_intervals,
        return_interval_tick_count: tick_hourly_intervals.len(),
        return_interval_coarse_candidate_count: coarse_intervals.len(),
        return_interval_coarse_gap_fill_count,
        return_interval_tick_coarse_overlap_count,
    }
}

fn intervals_overlap(
    left: JointRefinementAggregatedIntervalObservation,
    right: JointRefinementAggregatedIntervalObservation,
) -> bool {
    left.interval_start_utc_nanos < right.interval_end_utc_nanos
        && right.interval_start_utc_nanos < left.interval_end_utc_nanos
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
    use super::select_long_variance_observed_intervals;
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
    fn observed_interval_selection_uses_tick_returns_before_coarse_gap_fill() {
        let tick_intervals = vec![
            tick_interval(NANOS_PER_HOUR, 2 * NANOS_PER_HOUR),
            tick_interval(2 * NANOS_PER_HOUR, 3 * NANOS_PER_HOUR),
        ];

        let coarse_records = vec![
            coarse_record(0, NANOS_PER_HOUR),
            coarse_record(NANOS_PER_HOUR, 2 * NANOS_PER_HOUR),
            coarse_record(3 * NANOS_PER_HOUR, 4 * NANOS_PER_HOUR),
        ];

        let selection = select_long_variance_observed_intervals(
            tick_intervals.as_slice(),
            Some(coarse_records.as_slice()),
        );

        let intervals = selection
            .return_intervals
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
                (NANOS_PER_HOUR, 2 * NANOS_PER_HOUR, 10),
                (2 * NANOS_PER_HOUR, 3 * NANOS_PER_HOUR, 10),
                (3 * NANOS_PER_HOUR, 4 * NANOS_PER_HOUR, 0),
            ]
        );
        assert_eq!(selection.activity_intervals, tick_intervals);
        assert_eq!(selection.return_interval_tick_count, 2);
        assert_eq!(selection.return_interval_coarse_candidate_count, 3);
        assert_eq!(selection.return_interval_coarse_gap_fill_count, 2);
        assert_eq!(selection.return_interval_tick_coarse_overlap_count, 1);
    }

    #[test]
    fn observed_interval_selection_keeps_tick_return_intervals_when_tick_fully_covers_coarse_window()
     {
        let tick_intervals = vec![
            tick_interval(0, NANOS_PER_HOUR),
            tick_interval(NANOS_PER_HOUR, 2 * NANOS_PER_HOUR),
        ];

        let coarse_records = vec![
            coarse_record(0, NANOS_PER_HOUR),
            coarse_record(NANOS_PER_HOUR, 2 * NANOS_PER_HOUR),
        ];

        let selection = select_long_variance_observed_intervals(
            tick_intervals.as_slice(),
            Some(coarse_records.as_slice()),
        );

        assert_eq!(selection.return_intervals, tick_intervals);
        assert_eq!(selection.activity_intervals, selection.return_intervals);
        assert_eq!(selection.return_interval_tick_count, 2);
        assert_eq!(selection.return_interval_coarse_candidate_count, 2);
        assert_eq!(selection.return_interval_coarse_gap_fill_count, 0);
        assert_eq!(selection.return_interval_tick_coarse_overlap_count, 2);
    }

    #[test]
    fn observed_interval_selection_aggregates_subhour_coarse_returns_to_hourly_units() {
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
            select_long_variance_observed_intervals(&[], Some(coarse_records.as_slice()));

        assert_eq!(selection.return_interval_coarse_candidate_count, 1);
        assert_eq!(selection.return_interval_coarse_gap_fill_count, 1);
        assert!(selection.activity_intervals.is_empty());
        assert_eq!(
            selection.return_intervals,
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
    fn observed_interval_selection_excludes_coarse_records_above_hourly_units() {
        let coarse_records = vec![coarse_record(0, 2 * NANOS_PER_HOUR)];

        let selection =
            select_long_variance_observed_intervals(&[], Some(coarse_records.as_slice()));

        assert_eq!(selection.return_interval_coarse_candidate_count, 0);
        assert!(selection.return_intervals.is_empty());
        assert!(selection.activity_intervals.is_empty());
    }

    #[test]
    fn observed_interval_selection_uses_tick_only_without_coarse_records() {
        let tick_intervals = vec![
            tick_interval(0, NANOS_PER_HOUR),
            tick_interval(NANOS_PER_HOUR, 2 * NANOS_PER_HOUR),
        ];

        let selection = select_long_variance_observed_intervals(tick_intervals.as_slice(), None);

        assert_eq!(selection.return_intervals, tick_intervals);
        assert_eq!(selection.activity_intervals, selection.return_intervals);
        assert_eq!(selection.return_interval_tick_count, 2);
        assert_eq!(selection.return_interval_coarse_candidate_count, 0);
        assert_eq!(selection.return_interval_coarse_gap_fill_count, 0);
        assert_eq!(selection.return_interval_tick_coarse_overlap_count, 0);
    }
}
