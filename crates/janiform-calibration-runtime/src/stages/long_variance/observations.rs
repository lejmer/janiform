use janiform_foundations::time::{NANOS_PER_HOUR, floor_to_interval_utc_nanos};

use super::source::overlaps_tick_intervals;
use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;

/// Return energy over its measured interval. Sampling noise is separate from latent variance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VarianceObservation {
    pub(crate) start: i64,
    pub(crate) end: i64,
    pub(crate) energy: f64,
    pub(crate) sampling_count: usize,
}

#[derive(Debug, Clone, Copy)]
struct ReturnInterval {
    start: i64,
    end: i64,
    value: f64,
}

/// Sum five-minute return squares within complete UTC hours.
pub(crate) fn hourly_realized_variance(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<VarianceObservation> {
    complete_hour_energy(intervals.iter().map(|interval| ReturnInterval {
        start: interval.interval_start_utc_nanos,
        end: interval.interval_end_utc_nanos,
        value: interval.log_return,
    }))
}

/// Tick observations take precedence. Coarse returns retain their measured durations.
pub(crate) fn observed_realized_variance(
    tick_intervals: &[JointRefinementAggregatedIntervalObservation],
    coarse_records: Option<&[MarketDataNormalizedCoarseDataRecord]>,
) -> Vec<VarianceObservation> {
    let mut observations = hourly_realized_variance(tick_intervals);

    let Some(records) = coarse_records else {
        return observations;
    };

    // Borrow records while sorting. Market-data rows remain owned by the shared input.
    let mut ordered = records.iter().collect::<Vec<_>>();

    ordered.sort_by_key(|record| record.interval_start_utc_nanos);

    let mut tick_position = 0;
    let mut previous_end = None;

    for record in ordered {
        let start = record.interval_start_utc_nanos;
        let end = record.interval_end_utc_nanos;

        let Some(value) = record.close_to_close_log_return else {
            continue;
        };

        if !end.checked_sub(start).is_some_and(|duration| duration > 0)
            || !value.is_finite()
            || previous_end.is_some_and(|previous| start < previous)
            || overlaps_tick_intervals(tick_intervals, &mut tick_position, start, end)
        {
            continue;
        }

        let energy = value * value;
        if !energy.is_finite() {
            continue;
        }

        observations.push(VarianceObservation {
            start,
            end,
            energy,
            sampling_count: 1,
        });

        previous_end = Some(end);
    }

    observations.sort_by_key(|observation| observation.start);

    observations
}

fn complete_hour_energy(returns: impl Iterator<Item = ReturnInterval>) -> Vec<VarianceObservation> {
    let mut observations = Vec::new();
    let mut hour = None;
    let mut next = 0;
    let mut energy = 0.0;
    let mut count = 0;
    let mut valid = false;

    for interval in returns {
        let start = floor_to_interval_utc_nanos(interval.start, NANOS_PER_HOUR);

        if hour != Some(start) {
            if let Some(previous) = hour
                && valid
                && next == previous.saturating_add(NANOS_PER_HOUR)
            {
                observations.push(VarianceObservation {
                    start: previous,
                    end: next,
                    energy,
                    sampling_count: count,
                });
            }

            hour = Some(start);
            next = start;
            energy = 0.0;
            count = 0;
            valid = true;
        }

        valid &= interval.start == next
            && interval.end > interval.start
            && interval.end <= start.saturating_add(NANOS_PER_HOUR)
            && interval.value.is_finite();

        next = interval.end;

        energy += interval.value * interval.value;
        count += 1;

        valid &= energy.is_finite();
    }

    if let Some(start) = hour
        && valid
        && next == start.saturating_add(NANOS_PER_HOUR)
    {
        observations.push(VarianceObservation {
            start,
            end: next,
            energy,
            sampling_count: count,
        });
    }

    observations
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::{NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR};

    use super::{
        ReturnInterval, VarianceObservation, complete_hour_energy, observed_realized_variance,
    };

    #[test]
    fn realized_variance_retains_offsetting_moves_and_rejects_partial_hours() {
        let returns = (0..24)
            .map(|position| ReturnInterval {
                start: position * NANOS_PER_FIVE_MINUTES,
                end: (position + 1) * NANOS_PER_FIVE_MINUTES,
                value: if position % 2 == 0 {
                    0.5
                } else {
                    -0.5
                },
            })
            .collect::<Vec<_>>();

        assert_eq!(
            complete_hour_energy(returns.iter().copied()),
            vec![
                VarianceObservation {
                    start: 0,
                    end: NANOS_PER_HOUR,
                    energy: 3.0,
                    sampling_count: 12
                },
                VarianceObservation {
                    start: NANOS_PER_HOUR,
                    end: 2 * NANOS_PER_HOUR,
                    energy: 3.0,
                    sampling_count: 12
                },
            ]
        );

        assert_eq!(
            complete_hour_energy(returns.iter().skip(1).copied()),
            vec![VarianceObservation {
                start: NANOS_PER_HOUR,
                end: 2 * NANOS_PER_HOUR,
                energy: 3.0,
                sampling_count: 12
            },]
        );
    }

    #[test]
    fn realized_variance_rejects_overlapping_and_nonfinite_intervals() {
        let full = ReturnInterval {
            start: 0,
            end: NANOS_PER_HOUR,
            value: 0.5,
        };

        assert!(complete_hour_energy([full, full].into_iter()).is_empty());

        assert!(
            complete_hour_energy(
                [ReturnInterval {
                    value: f64::NAN,
                    ..full
                }]
                .into_iter()
            )
            .is_empty()
        );

        assert_eq!(
            complete_hour_energy([full].into_iter()),
            vec![VarianceObservation {
                start: 0,
                end: NANOS_PER_HOUR,
                energy: 0.25,
                sampling_count: 1
            },]
        );
    }

    #[test]
    fn coarse_observations_retain_native_durations_and_scale() {
        use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;

        let records = [7, 15, 30, 60, 1440]
            .into_iter()
            .scan(0, |start, minutes| {
                let end = *start + minutes * NANOS_PER_FIVE_MINUTES / 5;
                let record = MarketDataNormalizedCoarseDataRecord {
                    interval_start_utc_nanos: *start,
                    interval_end_utc_nanos: end,
                    observation_timestamp_utc_nanos: end,
                    close_price: 1.0,
                    open_price: None,
                    high_price: None,
                    low_price: None,
                    volume: None,
                    close_to_close_log_return: Some(0.2),
                    open_to_close_log_return: None,
                    close_to_open_gap_log_return: None,
                    high_low_log_range: None,
                    source_row_number: 1,
                };
                *start = end;
                Some(record)
            })
            .collect::<Vec<_>>();

        let expected = records
            .iter()
            .map(|record| VarianceObservation {
                start: record.interval_start_utc_nanos,
                end: record.interval_end_utc_nanos,
                energy: 0.2 * 0.2,
                sampling_count: 1,
            })
            .collect::<Vec<_>>();

        assert_eq!(observed_realized_variance(&[], Some(&records)), expected);
    }
}
