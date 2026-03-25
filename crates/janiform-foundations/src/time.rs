//! Shared UTC nanosecond time quantization helpers.

pub const FIVE_MINUTES_NANOS: i64 = 5 * 60 * 1_000_000_000;
pub const FIFTEEN_MINUTES_NANOS: i64 = 15 * 60 * 1_000_000_000;
pub const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;

/// Floor a UTC nanosecond timestamp to the containing interval boundary.
///
/// The interval duration must be strictly positive.
pub fn floor_to_interval_utc_nanos(timestamp_utc_nanos: i64, interval_duration_nanos: i64) -> i64 {
    timestamp_utc_nanos.div_euclid(interval_duration_nanos) * interval_duration_nanos
}

/// Ceil a UTC nanosecond timestamp to the next interval boundary.
///
/// The interval duration must be strictly positive.
pub fn ceil_to_interval_utc_nanos(timestamp_utc_nanos: i64, interval_duration_nanos: i64) -> i64 {
    let remainder = timestamp_utc_nanos.rem_euclid(interval_duration_nanos);
    if remainder == 0 {
        timestamp_utc_nanos
    } else {
        timestamp_utc_nanos.saturating_add(interval_duration_nanos - remainder)
    }
}

/// Floor a UTC nanosecond timestamp to the containing hour boundary.
pub fn floor_to_hour_utc_nanos(timestamp_utc_nanos: i64) -> i64 {
    floor_to_interval_utc_nanos(timestamp_utc_nanos, ONE_HOUR_NANOS)
}

/// Ceil a UTC nanosecond timestamp to the next hour boundary.
pub fn ceil_to_hour_utc_nanos(timestamp_utc_nanos: i64) -> i64 {
    ceil_to_interval_utc_nanos(timestamp_utc_nanos, ONE_HOUR_NANOS)
}

#[cfg(test)]
mod tests {
    use super::{
        ONE_HOUR_NANOS, ceil_to_hour_utc_nanos, ceil_to_interval_utc_nanos,
        floor_to_hour_utc_nanos, floor_to_interval_utc_nanos,
    };

    #[test]
    fn interval_rounding_respects_custom_boundaries() {
        assert_eq!(floor_to_interval_utc_nanos(95, 10), 90);
        assert_eq!(ceil_to_interval_utc_nanos(95, 10), 100);
        assert_eq!(ceil_to_interval_utc_nanos(100, 10), 100);
    }

    #[test]
    fn floor_to_hour_utc_nanos_rounds_down() {
        assert_eq!(
            floor_to_hour_utc_nanos(ONE_HOUR_NANOS + 123),
            ONE_HOUR_NANOS
        );
        assert_eq!(floor_to_hour_utc_nanos(-123), -ONE_HOUR_NANOS);
    }

    #[test]
    fn ceil_to_hour_utc_nanos_rounds_up() {
        assert_eq!(
            ceil_to_hour_utc_nanos(ONE_HOUR_NANOS + 123),
            2 * ONE_HOUR_NANOS
        );
        assert_eq!(ceil_to_hour_utc_nanos(ONE_HOUR_NANOS), ONE_HOUR_NANOS);
    }
}
