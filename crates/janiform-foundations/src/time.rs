//! Shared UTC nanosecond time quantization helpers.

use chrono::{TimeZone, Utc};

/// Exact five-minute UTC duration expressed in nanoseconds.
pub const FIVE_MINUTES_NANOS: i64 = 5 * 60 * 1_000_000_000;
/// Exact fifteen-minute UTC duration expressed in nanoseconds.
pub const FIFTEEN_MINUTES_NANOS: i64 = 15 * 60 * 1_000_000_000;
/// Exact one-hour UTC duration expressed in nanoseconds.
pub const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;
/// Floating-point view of the exact one-hour nanosecond duration for unit conversion only.
pub const ONE_HOUR_NANOS_F64: f64 = ONE_HOUR_NANOS as f64;
/// Exact reciprocal conversion from nanoseconds to hours derived from `ONE_HOUR_NANOS_F64`.
pub const HOURS_PER_NANOSECOND: f64 = 1.0 / ONE_HOUR_NANOS_F64;

/// Convert a fixed-width UTC nanosecond duration to hours.
pub fn duration_nanos_to_hours(duration_nanos: i64) -> f64 {
    duration_nanos as f64 * HOURS_PER_NANOSECOND
}

/// Convert a UTC calendar timestamp to UTC nanoseconds since the Unix epoch.
///
/// Panics when the timestamp is invalid or cannot be represented in `i64`
/// nanoseconds.
pub fn utc_nanos(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .expect("UTC timestamp should be valid")
        .timestamp_nanos_opt()
        .expect("UTC timestamp should fit in i64 nanoseconds")
}

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
        duration_nanos_to_hours, floor_to_hour_utc_nanos, floor_to_interval_utc_nanos, utc_nanos,
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

    #[test]
    fn duration_nanos_to_hours_matches_hour_boundary_constant() {
        assert_eq!(duration_nanos_to_hours(ONE_HOUR_NANOS), 1.0);
        assert_eq!(duration_nanos_to_hours(ONE_HOUR_NANOS / 4), 0.25);
    }

    #[test]
    fn utc_nanos_returns_epoch_for_unix_epoch() {
        assert_eq!(utc_nanos(1970, 1, 1, 0, 0, 0), 0);
    }

    #[test]
    #[should_panic(expected = "UTC timestamp should be valid")]
    fn utc_nanos_panics_for_invalid_calendar_timestamp() {
        utc_nanos(2026, 2, 30, 12, 0, 0);
    }
}
