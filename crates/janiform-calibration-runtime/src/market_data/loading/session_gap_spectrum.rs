use chrono::{DateTime, Datelike, Utc};

use crate::market_data::preprocessing::MarketDataTranslationTickRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObservedGap {
    duration_nanos: i64,
    midpoint_weekday_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObservedGapSpectrum {
    weekday_support: [usize; 7],
    inactivity_gap_floor_nanos: Option<i64>,
    inactivity_gap_count: usize,
    inactivity_gap_midpoint_weekday_support: [usize; 7],
}

impl ObservedGapSpectrum {
    pub(super) fn from_tick_records(
        tick_records: &[MarketDataTranslationTickRecord],
    ) -> ObservedGapSpectrum {
        let mut weekday_support = [0_usize; 7];
        let mut observed_gaps = Vec::with_capacity(tick_records.len().saturating_sub(1));

        for tick_record in tick_records {
            let weekday_index =
                DateTime::<Utc>::from_timestamp_nanos(tick_record.timestamp_utc_nanos)
                    .weekday()
                    .num_days_from_monday() as usize;

            weekday_support[weekday_index] += 1;
        }

        for tick_pair in tick_records.windows(2) {
            let duration_nanos =
                tick_pair[1].timestamp_utc_nanos - tick_pair[0].timestamp_utc_nanos;
            if duration_nanos <= 0 {
                continue;
            }

            let midpoint_utc_nanos = tick_pair[0].timestamp_utc_nanos + (duration_nanos / 2);

            let midpoint_weekday_index = DateTime::<Utc>::from_timestamp_nanos(midpoint_utc_nanos)
                .weekday()
                .num_days_from_monday() as usize;

            observed_gaps.push(ObservedGap {
                duration_nanos,
                midpoint_weekday_index,
            });
        }

        let inactivity_gap_floor_nanos = derive_inactivity_gap_floor_nanos(&mut observed_gaps);

        let mut inactivity_gap_midpoint_weekday_support = [0_usize; 7];
        let mut inactivity_gap_count = 0_usize;

        if let Some(gap_floor_nanos) = inactivity_gap_floor_nanos {
            for observed_gap in &observed_gaps {
                if observed_gap.duration_nanos <= gap_floor_nanos {
                    continue;
                }

                inactivity_gap_count += 1;
                inactivity_gap_midpoint_weekday_support[observed_gap.midpoint_weekday_index] += 1;
            }
        }

        ObservedGapSpectrum {
            weekday_support,
            inactivity_gap_floor_nanos,
            inactivity_gap_count,
            inactivity_gap_midpoint_weekday_support,
        }
    }

    pub(super) fn weekday_support(&self) -> &[usize; 7] {
        &self.weekday_support
    }

    pub(super) fn inactivity_gap_midpoint_weekday_support(&self) -> &[usize; 7] {
        &self.inactivity_gap_midpoint_weekday_support
    }

    pub(super) fn is_inactivity_gap(&self, gap_nanos: i64) -> bool {
        self.inactivity_gap_floor_nanos
            .is_some_and(|gap_floor_nanos| gap_nanos > gap_floor_nanos)
    }
}

fn derive_inactivity_gap_floor_nanos(observed_gaps: &mut [ObservedGap]) -> Option<i64> {
    if observed_gaps.len() < 2 {
        return None;
    }

    observed_gaps.sort_unstable_by_key(|observed_gap| observed_gap.duration_nanos);

    let mut strongest_lower_duration_nanos = None::<i64>;
    let mut strongest_upper_duration_nanos = None::<i64>;

    for adjacent_pair in observed_gaps.windows(2) {
        let lower_duration_nanos = adjacent_pair[0].duration_nanos;
        let upper_duration_nanos = adjacent_pair[1].duration_nanos;

        if lower_duration_nanos == upper_duration_nanos {
            continue;
        }

        match (
            strongest_lower_duration_nanos,
            strongest_upper_duration_nanos,
        ) {
            (Some(current_lower_duration_nanos), Some(current_upper_duration_nanos))
                if !gap_ratio_is_stronger(
                    lower_duration_nanos,
                    upper_duration_nanos,
                    current_lower_duration_nanos,
                    current_upper_duration_nanos,
                ) => {}
            _ => {
                strongest_lower_duration_nanos = Some(lower_duration_nanos);
                strongest_upper_duration_nanos = Some(upper_duration_nanos);
            }
        }
    }

    strongest_lower_duration_nanos
}

fn gap_ratio_is_stronger(
    candidate_lower_duration_nanos: i64,
    candidate_upper_duration_nanos: i64,
    current_lower_duration_nanos: i64,
    current_upper_duration_nanos: i64,
) -> bool {
    let candidate_ratio_cross_product =
        i128::from(candidate_upper_duration_nanos) * i128::from(current_lower_duration_nanos);
    let current_ratio_cross_product =
        i128::from(current_upper_duration_nanos) * i128::from(candidate_lower_duration_nanos);

    if candidate_ratio_cross_product != current_ratio_cross_product {
        return candidate_ratio_cross_product > current_ratio_cross_product;
    }

    let candidate_gap_width_nanos = candidate_upper_duration_nanos - candidate_lower_duration_nanos;
    let current_gap_width_nanos = current_upper_duration_nanos - current_lower_duration_nanos;

    if candidate_gap_width_nanos != current_gap_width_nanos {
        return candidate_gap_width_nanos > current_gap_width_nanos;
    }

    candidate_upper_duration_nanos > current_upper_duration_nanos
}

#[cfg(test)]
mod tests {
    use super::ObservedGapSpectrum;
    use crate::market_data::loading::test_support::tick;

    #[test]
    fn derives_inactivity_gap_floor_from_strongest_gap_ratio() {
        let spectrum = ObservedGapSpectrum::from_tick_records(&[
            tick("2026-04-06T09:00:00Z"),
            tick("2026-04-06T09:05:00Z"),
            tick("2026-04-06T09:10:00Z"),
            tick("2026-04-07T09:00:00Z"),
            tick("2026-04-07T09:05:00Z"),
            tick("2026-04-08T09:00:00Z"),
        ]);

        assert!(spectrum.is_inactivity_gap(hours(23) + minutes(50)));
        assert_eq!(
            *spectrum.inactivity_gap_midpoint_weekday_support(),
            [1, 1, 0, 0, 0, 0, 0]
        );
        assert!(!spectrum.is_inactivity_gap(minutes(5)));
    }

    #[test]
    fn reports_no_inactivity_gaps_when_gap_spectrum_has_no_separation() {
        let spectrum = ObservedGapSpectrum::from_tick_records(&[
            tick("2026-04-06T09:00:00Z"),
            tick("2026-04-06T09:05:00Z"),
            tick("2026-04-06T09:10:00Z"),
            tick("2026-04-06T09:15:00Z"),
        ]);

        assert!(!spectrum.is_inactivity_gap(minutes(10)));
        assert_eq!(
            *spectrum.inactivity_gap_midpoint_weekday_support(),
            [0, 0, 0, 0, 0, 0, 0]
        );
        assert!(!spectrum.is_inactivity_gap(minutes(5)));
    }

    #[test]
    fn retains_weekday_support_from_observed_ticks() {
        let spectrum = ObservedGapSpectrum::from_tick_records(&[
            tick("2026-04-10T23:00:00Z"),
            tick("2026-04-13T00:30:00Z"),
            tick("2026-04-13T00:35:00Z"),
        ]);

        assert_eq!(*spectrum.weekday_support(), [2, 0, 0, 0, 1, 0, 0]);
        assert_eq!(
            *spectrum.inactivity_gap_midpoint_weekday_support(),
            [0, 0, 0, 0, 0, 1, 0]
        );
    }
    fn minutes(value: i64) -> i64 {
        value * 60 * 1_000_000_000
    }

    fn hours(value: i64) -> i64 {
        value * 60 * minutes(1)
    }
}
