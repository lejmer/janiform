//! Shared UTC nanosecond time quantization helpers.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Duration;

use chrono::ParseError;
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Deserializer};

/// Exact nanoseconds per microsecond.
pub const NANOS_PER_MICROSECOND: i64 = 1_000;
/// Exact nanoseconds per millisecond.
pub const NANOS_PER_MILLISECOND: i64 = 1_000_000;
/// Exact nanoseconds per second.
pub const NANOS_PER_SECOND: i64 = 1_000_000_000;

/// Exact nanoseconds per minute.
pub const NANOS_PER_MINUTE: i64 = 60 * NANOS_PER_SECOND;
/// Exact nanoseconds per five-minute interval.
pub const NANOS_PER_FIVE_MINUTES: i64 = 5 * NANOS_PER_MINUTE;
/// Exact nanoseconds per fifteen-minute interval.
pub const NANOS_PER_FIFTEEN_MINUTES: i64 = 15 * NANOS_PER_MINUTE;
/// Exact nanoseconds per hour.
pub const NANOS_PER_HOUR: i64 = 60 * NANOS_PER_MINUTE;
/// Floating-point view of the exact nanoseconds per hour for unit conversion only.
pub const NANOS_PER_HOUR_F64: f64 = NANOS_PER_HOUR as f64;
/// Exact reciprocal conversion from nanoseconds to hours derived from `NANOS_PER_HOUR_F64`.
pub const HOURS_PER_NANOSECOND: f64 = 1.0 / NANOS_PER_HOUR_F64;

/// Exact nanoseconds per day.
pub const NANOS_PER_DAY: i64 = 24 * NANOS_PER_HOUR;

/// Error returned when a duration spec cannot be represented as UTC nanoseconds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DurationSpecParseError {
    /// The spec does not match the accepted `<number><unit>` shape.
    InvalidShape,
    /// The numeric value is invalid.
    InvalidNumericValue,
    /// The unit is not supported.
    UnsupportedUnit { unit: String },
    /// The computed duration is not an integer number of nanoseconds.
    FractionalNanoseconds,
    /// The computed duration is zero or negative.
    NonPositive,
    /// The computed duration does not fit in `i64` nanoseconds.
    OutOfRange,
}

impl Display for DurationSpecParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidShape => {
                formatter.write_str("must be a duration string like `1h` or `15min`")
            }
            Self::InvalidNumericValue => {
                formatter.write_str("contains an invalid numeric interval value")
            }
            Self::UnsupportedUnit { unit } => {
                write!(formatter, "uses unsupported interval unit `{unit}`")
            }
            Self::FractionalNanoseconds => {
                formatter.write_str("must map exactly to an integer UTC nanosecond duration")
            }
            Self::NonPositive => formatter.write_str("must be strictly positive"),
            Self::OutOfRange => formatter.write_str("does not fit in signed 64-bit nanoseconds"),
        }
    }
}

impl Error for DurationSpecParseError {}

/// Convert a fixed-width UTC nanosecond duration to hours.
pub fn duration_nanos_to_hours(duration_nanos: i64) -> f64 {
    duration_nanos as f64 * HOURS_PER_NANOSECOND
}

/// Parse an RFC 3339 timestamp string and normalize it to UTC.
pub fn parse_utc_datetime(value: &str) -> Result<DateTime<Utc>, ParseError> {
    DateTime::parse_from_rfc3339(value).map(|timestamp| timestamp.with_timezone(&Utc))
}

/// Return elapsed wall-clock duration between optional RFC 3339 UTC timestamps.
pub fn elapsed_between_utc_timestamps(start: Option<&str>, end: Option<&str>) -> Option<Duration> {
    let start = parse_utc_datetime(start?).ok()?;
    let end = parse_utc_datetime(end?).ok()?;
    let duration = end.signed_duration_since(start).to_std().ok()?;

    Some(duration)
}

/// Format UTC nanoseconds since the Unix epoch as an RFC 3339 timestamp when representable.
pub fn format_utc_nanos(timestamp_nanos: i64) -> String {
    let seconds = timestamp_nanos.div_euclid(NANOS_PER_SECOND);
    let nanos = timestamp_nanos.rem_euclid(NANOS_PER_SECOND) as u32;

    DateTime::from_timestamp(seconds, nanos)
        .map(|timestamp| timestamp.to_rfc3339())
        .unwrap_or_else(|| timestamp_nanos.to_string())
}

/// Parse a pandas-style duration spec into an exact UTC nanosecond duration.
///
/// Accepted units are nanoseconds, microseconds, milliseconds, seconds,
/// minutes, hours, and days, including common abbreviations such as `15min`,
/// `1h`, and `1D`.
pub fn parse_duration_spec_nanos(value: &str) -> Result<i64, DurationSpecParseError> {
    let trimmed = value.trim();
    let (raw_number, raw_unit) = split_duration_spec(trimmed)?;

    let unit_nanos = duration_unit_nanos(raw_unit)?;
    let (mantissa, scale) = parse_decimal_number(raw_number)?;
    let scaled_nanos = mantissa
        .checked_mul(unit_nanos as u128)
        .ok_or(DurationSpecParseError::OutOfRange)?;

    if scaled_nanos % scale != 0 {
        return Err(DurationSpecParseError::FractionalNanoseconds);
    }

    let nanos = scaled_nanos / scale;
    if nanos == 0 {
        return Err(DurationSpecParseError::NonPositive);
    }

    i64::try_from(nanos).map_err(|_| DurationSpecParseError::OutOfRange)
}

/// Deserialize an RFC 3339 timestamp string as a UTC `DateTime`.
pub fn deserialize_utc_datetime<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    parse_utc_datetime(value.as_str()).map_err(serde::de::Error::custom)
}

fn split_duration_spec(value: &str) -> Result<(&str, &str), DurationSpecParseError> {
    if value.is_empty() {
        return Err(DurationSpecParseError::InvalidShape);
    }

    let mut unit_start = value.len();
    for (position, character) in value.char_indices() {
        if character.is_ascii_alphabetic() {
            unit_start = position;
            break;
        }

        if !character.is_ascii_digit() && character != '.' && !character.is_ascii_whitespace() {
            return Err(DurationSpecParseError::InvalidShape);
        }
    }

    if unit_start == 0 || unit_start == value.len() {
        return Err(DurationSpecParseError::InvalidShape);
    }

    let number = value[..unit_start].trim();
    let unit = value[unit_start..].trim();
    if number.is_empty()
        || unit.is_empty()
        || !unit
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return Err(DurationSpecParseError::InvalidShape);
    }

    Ok((number, unit))
}

fn parse_decimal_number(value: &str) -> Result<(u128, u128), DurationSpecParseError> {
    let mut mantissa = 0_u128;
    let mut scale = 1_u128;
    let mut saw_digit = false;
    let mut saw_decimal_point = false;

    for character in value.chars() {
        match character {
            '0'..='9' => {
                saw_digit = true;
                mantissa = mantissa
                    .checked_mul(10)
                    .and_then(|current| current.checked_add(u128::from(character as u8 - b'0')))
                    .ok_or(DurationSpecParseError::OutOfRange)?;

                if saw_decimal_point {
                    scale = scale
                        .checked_mul(10)
                        .ok_or(DurationSpecParseError::OutOfRange)?;
                }
            }
            '.' if !saw_decimal_point => {
                saw_decimal_point = true;
            }
            _ => return Err(DurationSpecParseError::InvalidNumericValue),
        }
    }

    if !saw_digit {
        return Err(DurationSpecParseError::InvalidNumericValue);
    }

    Ok((mantissa, scale))
}

fn duration_unit_nanos(unit: &str) -> Result<i64, DurationSpecParseError> {
    let normalized = unit.to_ascii_lowercase();
    let nanos = match normalized.as_str() {
        "ns" | "nanosecond" | "nanoseconds" => 1,
        "us" | "microsecond" | "microseconds" => NANOS_PER_MICROSECOND,
        "ms" | "millisecond" | "milliseconds" => NANOS_PER_MILLISECOND,
        "s" | "sec" | "secs" | "second" | "seconds" => NANOS_PER_SECOND,
        "m" | "min" | "mins" | "minute" | "minutes" => NANOS_PER_MINUTE,
        "h" | "hr" | "hrs" | "hour" | "hours" => NANOS_PER_HOUR,
        "d" | "day" | "days" => NANOS_PER_DAY,
        _ => return Err(DurationSpecParseError::UnsupportedUnit { unit: normalized }),
    };

    Ok(nanos)
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

/// Checked floor of a UTC nanosecond timestamp to the containing interval boundary.
///
/// The interval duration must be strictly positive.
pub fn checked_floor_to_interval_utc_nanos(
    timestamp_utc_nanos: i64,
    interval_duration_nanos: i64,
) -> Option<i64> {
    timestamp_utc_nanos
        .div_euclid(interval_duration_nanos)
        .checked_mul(interval_duration_nanos)
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

/// Checked ceil of a UTC nanosecond timestamp to the next interval boundary.
///
/// The interval duration must be strictly positive.
pub fn checked_ceil_to_interval_utc_nanos(
    timestamp_utc_nanos: i64,
    interval_duration_nanos: i64,
) -> Option<i64> {
    let floor = checked_floor_to_interval_utc_nanos(timestamp_utc_nanos, interval_duration_nanos)?;
    if floor == timestamp_utc_nanos {
        return Some(floor);
    }

    floor.checked_add(interval_duration_nanos)
}

/// Floor a UTC nanosecond timestamp to the containing hour boundary.
pub fn floor_to_hour_utc_nanos(timestamp_utc_nanos: i64) -> i64 {
    floor_to_interval_utc_nanos(timestamp_utc_nanos, NANOS_PER_HOUR)
}

/// Checked floor of a UTC nanosecond timestamp to the containing hour boundary.
pub fn checked_floor_to_hour_utc_nanos(timestamp_utc_nanos: i64) -> Option<i64> {
    checked_floor_to_interval_utc_nanos(timestamp_utc_nanos, NANOS_PER_HOUR)
}

/// Ceil a UTC nanosecond timestamp to the next hour boundary.
pub fn ceil_to_hour_utc_nanos(timestamp_utc_nanos: i64) -> i64 {
    ceil_to_interval_utc_nanos(timestamp_utc_nanos, NANOS_PER_HOUR)
}

/// Checked ceil of a UTC nanosecond timestamp to the next hour boundary.
pub fn checked_ceil_to_hour_utc_nanos(timestamp_utc_nanos: i64) -> Option<i64> {
    checked_ceil_to_interval_utc_nanos(timestamp_utc_nanos, NANOS_PER_HOUR)
}

#[cfg(test)]
mod tests {
    use super::{
        NANOS_PER_HOUR, ceil_to_hour_utc_nanos, ceil_to_interval_utc_nanos,
        checked_ceil_to_hour_utc_nanos, checked_floor_to_hour_utc_nanos, duration_nanos_to_hours,
        elapsed_between_utc_timestamps, floor_to_hour_utc_nanos, floor_to_interval_utc_nanos,
        format_utc_nanos, parse_duration_spec_nanos, parse_utc_datetime, utc_nanos,
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
            floor_to_hour_utc_nanos(NANOS_PER_HOUR + 123),
            NANOS_PER_HOUR
        );
        assert_eq!(floor_to_hour_utc_nanos(-123), -NANOS_PER_HOUR);
    }

    #[test]
    fn ceil_to_hour_utc_nanos_rounds_up() {
        assert_eq!(
            ceil_to_hour_utc_nanos(NANOS_PER_HOUR + 123),
            2 * NANOS_PER_HOUR
        );
        assert_eq!(ceil_to_hour_utc_nanos(NANOS_PER_HOUR), NANOS_PER_HOUR);
    }

    #[test]
    fn checked_hour_rounding_reports_overflow() {
        assert_eq!(
            checked_floor_to_hour_utc_nanos(NANOS_PER_HOUR + 123),
            Some(NANOS_PER_HOUR)
        );
        assert_eq!(
            checked_ceil_to_hour_utc_nanos(NANOS_PER_HOUR + 123),
            Some(2 * NANOS_PER_HOUR)
        );
        assert_eq!(checked_ceil_to_hour_utc_nanos(i64::MAX), None);
    }

    #[test]
    fn duration_nanos_to_hours_matches_hour_boundary_constant() {
        assert_eq!(duration_nanos_to_hours(NANOS_PER_HOUR), 1.0);
        assert_eq!(duration_nanos_to_hours(NANOS_PER_HOUR / 4), 0.25);
    }

    #[test]
    fn parse_utc_datetime_normalizes_rfc3339_offset() {
        let parsed = parse_utc_datetime("2026-04-29T12:00:00+02:00")
            .expect("RFC 3339 timestamp should parse");

        assert_eq!(parsed.to_rfc3339(), "2026-04-29T10:00:00+00:00");
    }

    #[test]
    fn elapsed_between_utc_timestamps_reports_positive_duration() {
        let elapsed = elapsed_between_utc_timestamps(
            Some("2026-04-29T10:00:00Z"),
            Some("2026-04-29T10:05:00Z"),
        );

        assert_eq!(elapsed.map(|duration| duration.as_secs()), Some(300));
    }

    #[test]
    fn format_utc_nanos_renders_rfc3339_when_representable() {
        assert_eq!(format_utc_nanos(0), "1970-01-01T00:00:00+00:00");
        assert_eq!(
            format_utc_nanos(1_000_000_001),
            "1970-01-01T00:00:01.000000001+00:00"
        );
    }

    #[test]
    fn parse_duration_spec_nanos_accepts_supported_units() {
        assert_eq!(parse_duration_spec_nanos("15min"), Ok(900_000_000_000));
        assert_eq!(parse_duration_spec_nanos("1h"), Ok(NANOS_PER_HOUR));
        assert_eq!(parse_duration_spec_nanos("1D"), Ok(24 * NANOS_PER_HOUR));
        assert_eq!(parse_duration_spec_nanos("0.5s"), Ok(500_000_000));
    }

    #[test]
    fn parse_duration_spec_nanos_rejects_fractional_nanoseconds() {
        assert!(parse_duration_spec_nanos("0.1ns").is_err());
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
