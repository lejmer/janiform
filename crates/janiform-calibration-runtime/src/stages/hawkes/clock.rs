use super::contracts::{
    HAWKES_MAX_DESEASONALIZED_GAP_SECONDS, HAWKES_MINIMUM_POSITIVE_SEASONALITY_FACTOR,
};
use janiform_foundations::time::{SECONDS_PER_HOUR, elapsed_utc_nanos_to_hours};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct HawkesDeseasonalizedAdjacentOpenGap {
    pub(super) raw_gap_seconds: f64,
    pub(super) transformed_gap_seconds: f64,
    pub(super) transformed_gap_hours: f64,
}

pub(super) fn hawkes_deseasonalized_adjacent_open_gap(
    previous_timestamp_utc_nanos: i64,
    previous_factor: f64,
    timestamp_utc_nanos: i64,
    factor: f64,
) -> Option<HawkesDeseasonalizedAdjacentOpenGap> {
    let raw_gap_seconds =
        (elapsed_utc_nanos_to_hours(previous_timestamp_utc_nanos, timestamp_utc_nanos)
            * SECONDS_PER_HOUR)
            .max(0.0);

    if raw_gap_seconds > HAWKES_MAX_DESEASONALIZED_GAP_SECONDS {
        return None;
    }

    let average_factor =
        ((previous_factor + factor) / 2.0).max(HAWKES_MINIMUM_POSITIVE_SEASONALITY_FACTOR);

    let transformed_gap_seconds = raw_gap_seconds * average_factor;

    Some(HawkesDeseasonalizedAdjacentOpenGap {
        raw_gap_seconds,
        transformed_gap_seconds,
        transformed_gap_hours: transformed_gap_seconds / SECONDS_PER_HOUR,
    })
}

#[cfg(test)]
mod tests {
    use super::hawkes_deseasonalized_adjacent_open_gap;
    use janiform_foundations::time::{
        NANOS_PER_HOUR, NANOS_PER_SECOND, SECONDS_PER_HOUR, elapsed_utc_nanos_to_hours,
    };

    #[test]
    fn deseasonalized_open_gap_skips_non_adjacent_session_gap() {
        let two_hour_gap_nanos = 2 * NANOS_PER_HOUR;

        let gap = hawkes_deseasonalized_adjacent_open_gap(0, 1.0, two_hour_gap_nanos, 1.0);

        assert_eq!(gap, None);
    }

    #[test]
    fn deseasonalized_open_gap_uses_average_factor_for_adjacent_events() {
        let one_second_gap_nanos = NANOS_PER_SECOND;
        let expected_average_factor = (1.0 + 3.0) / 2.0;
        let expected_raw_gap_seconds =
            elapsed_utc_nanos_to_hours(0, one_second_gap_nanos) * SECONDS_PER_HOUR;
        let expected_transformed_gap_seconds = expected_raw_gap_seconds * expected_average_factor;

        let gap = hawkes_deseasonalized_adjacent_open_gap(0, 1.0, one_second_gap_nanos, 3.0)
            .expect("one-second same-session gap should be adjacent");

        assert_eq!(
            gap,
            super::HawkesDeseasonalizedAdjacentOpenGap {
                raw_gap_seconds: expected_raw_gap_seconds,
                transformed_gap_seconds: expected_transformed_gap_seconds,
                transformed_gap_hours: expected_transformed_gap_seconds / SECONDS_PER_HOUR,
            }
        );
    }
}
