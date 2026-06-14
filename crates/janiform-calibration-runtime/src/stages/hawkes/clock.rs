use super::contracts::HAWKES_MAX_CONTINUOUS_OPEN_GAP_SECONDS;
use janiform_foundations::time::{SECONDS_PER_HOUR_F64, elapsed_utc_nanos_to_hours};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct HawkesAdjacentOpenGap {
    pub(super) raw_gap_seconds: f64,
}

pub(super) fn hawkes_adjacent_open_gap(
    previous_timestamp_utc_nanos: i64,
    timestamp_utc_nanos: i64,
) -> Option<HawkesAdjacentOpenGap> {
    let raw_gap_seconds =
        (elapsed_utc_nanos_to_hours(previous_timestamp_utc_nanos, timestamp_utc_nanos)
            * SECONDS_PER_HOUR_F64)
            .max(0.0);

    if raw_gap_seconds > HAWKES_MAX_CONTINUOUS_OPEN_GAP_SECONDS {
        return None;
    }

    Some(HawkesAdjacentOpenGap { raw_gap_seconds })
}

#[cfg(test)]
mod tests {
    use super::hawkes_adjacent_open_gap;
    use janiform_foundations::time::{
        NANOS_PER_HOUR, NANOS_PER_SECOND, SECONDS_PER_HOUR_F64, elapsed_utc_nanos_to_hours,
    };

    #[test]
    fn adjacent_open_gap_skips_non_adjacent_session_gap() {
        let two_hour_gap_nanos = 2 * NANOS_PER_HOUR;

        let gap = hawkes_adjacent_open_gap(0, two_hour_gap_nanos);

        assert_eq!(gap, None);
    }

    #[test]
    fn adjacent_open_gap_keeps_raw_gap_for_adjacent_events() {
        let one_second_gap_nanos = NANOS_PER_SECOND;

        let expected_raw_gap_seconds =
            elapsed_utc_nanos_to_hours(0, one_second_gap_nanos) * SECONDS_PER_HOUR_F64;

        let gap = hawkes_adjacent_open_gap(0, one_second_gap_nanos)
            .expect("one-second same-session gap should be adjacent");

        assert_eq!(
            gap,
            super::HawkesAdjacentOpenGap {
                raw_gap_seconds: expected_raw_gap_seconds,
            }
        );
    }
}
