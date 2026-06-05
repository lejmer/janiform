use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::stages::coarse_data::coarse_data_interval_observations;
use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;

pub(crate) const LONG_VARIANCE_SLOW_HORIZON_SOURCE_SELECTION_POLICY: &str =
    "coarse_preferred_overlap_with_tick_gap_fill_slow_horizon_intervals.v1";

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
        .map(coarse_data_interval_observations)
        .unwrap_or_default();

    if coarse_intervals.is_empty() {
        return LongVarianceSlowHorizonIntervalSelection {
            intervals: tick_hourly_intervals.to_vec(),
            tick_interval_count: tick_hourly_intervals.len(),
            coarse_interval_count: 0,
            omitted_overlapping_tick_interval_count: 0,
        };
    }

    coarse_intervals.sort_by(|left, right| {
        left.interval_start_utc_nanos
            .cmp(&right.interval_start_utc_nanos)
            .then(
                left.interval_end_utc_nanos
                    .cmp(&right.interval_end_utc_nanos),
            )
    });

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

    selected_intervals.sort_by(|left, right| {
        left.interval_start_utc_nanos
            .cmp(&right.interval_start_utc_nanos)
            .then(
                left.interval_end_utc_nanos
                    .cmp(&right.interval_end_utc_nanos),
            )
    });

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

#[cfg(test)]
mod tests {
    use super::select_long_variance_slow_horizon_intervals;
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
            close_to_close_log_return: Some(0.1),
            open_to_close_log_return: None,
            close_to_open_gap_log_return: None,
            high_low_log_range: None,
            source_row_number: 1,
        }
    }

    #[test]
    fn slow_horizon_source_selection_uses_coarse_overlap_and_tick_gaps() {
        let tick_intervals = vec![tick_interval(10, 20), tick_interval(20, 30)];
        let coarse_records = vec![
            coarse_record(0, 10),
            coarse_record(10, 20),
            coarse_record(30, 40),
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
            vec![(0, 10, 0), (10, 20, 0), (20, 30, 10), (30, 40, 0)]
        );
        assert_eq!(selection.omitted_overlapping_tick_interval_count, 1);
    }
}
