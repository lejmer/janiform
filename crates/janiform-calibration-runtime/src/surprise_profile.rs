use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Datelike, Duration, NaiveDateTime, NaiveTime, Timelike, Utc};
use janiform_calibration_artifacts::SurpriseNewsDeterministicIntensityProfile;

const HOURS_PER_WEEK: usize = 7 * 24;

/// Errors raised while validating or materializing deterministic surprise-news weekly profiles.
#[derive(Debug, Clone, PartialEq)]
pub enum SurpriseNewsWeeklyProfileError {
    /// The profile resolution does not evenly partition the 168-hour UTC week.
    InvalidProfileResolutionHours { profile_resolution_hours: usize },
    /// The multiplier vector length does not match the declared profile resolution.
    MultiplierCountMismatch {
        expected_bucket_count: usize,
        actual_bucket_count: usize,
        profile_resolution_hours: usize,
    },
    /// A profile multiplier is not a finite non-negative value.
    InvalidMultiplier { bucket_index: usize, value: f64 },
    /// The requested runtime interval duration is not strictly positive.
    InvalidIntervalDurationNanos { interval_duration_nanos: i64 },
    /// A requested interval range timestamp cannot be represented at nanosecond resolution.
    TimestampOutOfNanosecondRange { field: &'static str },
    /// The requested interval range does not contain a whole number of fixed-width intervals.
    RangeDurationNotDivisibleByInterval {
        range_start_utc_nanos: i64,
        range_end_utc_nanos: i64,
        interval_duration_nanos: i64,
        remainder_nanos: i64,
    },
    /// Interval offset arithmetic overflowed while building a fixed-width interval grid.
    IntervalOffsetOverflow {
        interval_index: usize,
        interval_duration_nanos: i64,
    },
    /// Interval timestamp arithmetic produced an out-of-range UTC timestamp.
    IntervalTimestampOutOfRange { interval_index: usize },
}

impl Display for SurpriseNewsWeeklyProfileError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProfileResolutionHours {
                profile_resolution_hours,
            } => write!(
                formatter,
                "surprise-news weekly profile resolution must be a positive divisor of {HOURS_PER_WEEK} hours, found {profile_resolution_hours}"
            ),
            Self::MultiplierCountMismatch {
                expected_bucket_count,
                actual_bucket_count,
                profile_resolution_hours,
            } => write!(
                formatter,
                "surprise-news weekly profile must contain {expected_bucket_count} multipliers for profile_resolution_hours={profile_resolution_hours}, found {actual_bucket_count}"
            ),
            Self::InvalidMultiplier {
                bucket_index,
                value,
            } => write!(
                formatter,
                "surprise-news weekly profile multiplier {bucket_index} must be finite and non-negative, found {value}"
            ),
            Self::InvalidIntervalDurationNanos {
                interval_duration_nanos,
            } => write!(
                formatter,
                "surprise-news weekly profile materialization requires a positive interval duration, found {interval_duration_nanos}"
            ),
            Self::TimestampOutOfNanosecondRange { field } => write!(
                formatter,
                "surprise-news weekly profile timestamp field `{field}` is outside the supported nanosecond range"
            ),
            Self::RangeDurationNotDivisibleByInterval {
                range_start_utc_nanos,
                range_end_utc_nanos,
                interval_duration_nanos,
                remainder_nanos,
            } => write!(
                formatter,
                "surprise-news weekly profile range [{range_start_utc_nanos}, {range_end_utc_nanos}) is not divisible by interval_duration_nanos={interval_duration_nanos}, remainder_nanos={remainder_nanos}"
            ),
            Self::IntervalOffsetOverflow {
                interval_index,
                interval_duration_nanos,
            } => write!(
                formatter,
                "surprise-news weekly profile interval offset overflowed for interval_index={interval_index} interval_duration_nanos={interval_duration_nanos}"
            ),
            Self::IntervalTimestampOutOfRange { interval_index } => write!(
                formatter,
                "surprise-news weekly profile interval timestamp is out of range for interval_index={interval_index}"
            ),
        }
    }
}

impl Error for SurpriseNewsWeeklyProfileError {}

/// Validate a deterministic surprise-news weekly profile and return its bucket width in hours.
pub fn validate_surprise_news_weekly_profile(
    surprise_profile: &SurpriseNewsDeterministicIntensityProfile,
) -> Result<usize, SurpriseNewsWeeklyProfileError> {
    let profile_resolution_hours = surprise_profile.profile_resolution_hours;
    if profile_resolution_hours == 0 || !HOURS_PER_WEEK.is_multiple_of(profile_resolution_hours) {
        return Err(
            SurpriseNewsWeeklyProfileError::InvalidProfileResolutionHours {
                profile_resolution_hours,
            },
        );
    }

    let expected_bucket_count = HOURS_PER_WEEK / profile_resolution_hours;

    let actual_bucket_count = surprise_profile.weekly_utc_profile_multipliers.len();
    if actual_bucket_count != expected_bucket_count {
        return Err(SurpriseNewsWeeklyProfileError::MultiplierCountMismatch {
            expected_bucket_count,
            actual_bucket_count,
            profile_resolution_hours,
        });
    }

    for (bucket_index, multiplier) in surprise_profile
        .weekly_utc_profile_multipliers
        .iter()
        .copied()
        .enumerate()
    {
        if !multiplier.is_finite() || multiplier < 0.0 {
            return Err(SurpriseNewsWeeklyProfileError::InvalidMultiplier {
                bucket_index,
                value: multiplier,
            });
        }
    }

    Ok(profile_resolution_hours)
}

/// Average the deterministic surprise-news weekly profile over one UTC interval.
pub fn average_surprise_news_weekly_profile_multiplier_over_interval(
    interval_start_utc: DateTime<Utc>,
    interval_end_utc: DateTime<Utc>,
    surprise_profile: &SurpriseNewsDeterministicIntensityProfile,
) -> Result<f64, SurpriseNewsWeeklyProfileError> {
    let profile_resolution_hours = validate_surprise_news_weekly_profile(surprise_profile)?;

    average_surprise_news_weekly_profile_multiplier_over_interval_with_resolution(
        interval_start_utc,
        interval_end_utc,
        surprise_profile,
        profile_resolution_hours,
    )
}

fn average_surprise_news_weekly_profile_multiplier_over_interval_with_resolution(
    interval_start_utc: DateTime<Utc>,
    interval_end_utc: DateTime<Utc>,
    surprise_profile: &SurpriseNewsDeterministicIntensityProfile,
    profile_resolution_hours: usize,
) -> Result<f64, SurpriseNewsWeeklyProfileError> {
    if interval_end_utc <= interval_start_utc {
        return Ok(1.0);
    }

    let mut weighted_multiplier_sum = 0.0;
    let mut weighted_duration_seconds = 0.0;
    let mut current = interval_start_utc;

    while current < interval_end_utc {
        let segment_end = next_weekly_profile_bucket_boundary(current, profile_resolution_hours)
            .min(interval_end_utc);

        let segment_duration_seconds = (segment_end - current)
            .to_std()
            .map_err(
                |_| SurpriseNewsWeeklyProfileError::IntervalTimestampOutOfRange {
                    interval_index: 0,
                },
            )?
            .as_secs_f64();

        weighted_multiplier_sum += surprise_profile.weekly_utc_profile_multipliers
            [weekly_profile_bucket_index(current, profile_resolution_hours)]
            * segment_duration_seconds;

        weighted_duration_seconds += segment_duration_seconds;

        current = segment_end;
    }

    if weighted_duration_seconds <= 0.0 {
        return Ok(1.0);
    }

    Ok(weighted_multiplier_sum / weighted_duration_seconds)
}

/// Materialize deterministic surprise-news multipliers for a fixed interval grid.
///
/// The returned entries include every interval index, not only intervals where the
/// multiplier differs from one.
pub fn materialize_surprise_news_weekly_profile_multipliers_for_interval_grid(
    range_start_utc: DateTime<Utc>,
    interval_count: usize,
    interval_duration_nanos: i64,
    surprise_profile: &SurpriseNewsDeterministicIntensityProfile,
) -> Result<Vec<(u64, f64)>, SurpriseNewsWeeklyProfileError> {
    if interval_duration_nanos <= 0 {
        return Err(
            SurpriseNewsWeeklyProfileError::InvalidIntervalDurationNanos {
                interval_duration_nanos,
            },
        );
    }

    let profile_resolution_hours = validate_surprise_news_weekly_profile(surprise_profile)?;

    let mut multiplier_by_interval = Vec::with_capacity(interval_count);

    for interval_index in 0..interval_count {
        let interval_index_i64 = i64::try_from(interval_index).map_err(|_| {
            SurpriseNewsWeeklyProfileError::IntervalOffsetOverflow {
                interval_index,
                interval_duration_nanos,
            }
        })?;

        let offset_nanos = interval_duration_nanos
            .checked_mul(interval_index_i64)
            .ok_or(SurpriseNewsWeeklyProfileError::IntervalOffsetOverflow {
                interval_index,
                interval_duration_nanos,
            })?;

        let interval_start_utc = range_start_utc
            .checked_add_signed(Duration::nanoseconds(offset_nanos))
            .ok_or(
                SurpriseNewsWeeklyProfileError::IntervalTimestampOutOfRange { interval_index },
            )?;

        let interval_end_utc = interval_start_utc
            .checked_add_signed(Duration::nanoseconds(interval_duration_nanos))
            .ok_or(
                SurpriseNewsWeeklyProfileError::IntervalTimestampOutOfRange { interval_index },
            )?;

        let interval_index_u64 = u64::try_from(interval_index).map_err(|_| {
            SurpriseNewsWeeklyProfileError::IntervalOffsetOverflow {
                interval_index,
                interval_duration_nanos,
            }
        })?;

        let multiplier =
            average_surprise_news_weekly_profile_multiplier_over_interval_with_resolution(
                interval_start_utc,
                interval_end_utc,
                surprise_profile,
                profile_resolution_hours,
            )?;

        multiplier_by_interval.push((interval_index_u64, multiplier));
    }

    Ok(multiplier_by_interval)
}

/// Materialize deterministic surprise-news multipliers for a divisible UTC interval range.
///
/// The requested range must contain an integer number of intervals with the given
/// interval duration.
pub fn materialize_surprise_news_weekly_profile_multipliers_for_interval_range(
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
    surprise_profile: &SurpriseNewsDeterministicIntensityProfile,
) -> Result<Vec<(u64, f64)>, SurpriseNewsWeeklyProfileError> {
    if interval_duration_nanos <= 0 {
        return Err(
            SurpriseNewsWeeklyProfileError::InvalidIntervalDurationNanos {
                interval_duration_nanos,
            },
        );
    }

    if range_end_utc <= range_start_utc {
        return Ok(Vec::new());
    }

    let range_start_utc_nanos = range_start_utc.timestamp_nanos_opt().ok_or(
        SurpriseNewsWeeklyProfileError::TimestampOutOfNanosecondRange {
            field: "range_start_utc",
        },
    )?;

    let range_end_utc_nanos = range_end_utc.timestamp_nanos_opt().ok_or(
        SurpriseNewsWeeklyProfileError::TimestampOutOfNanosecondRange {
            field: "range_end_utc",
        },
    )?;

    let range_duration_nanos = range_end_utc_nanos - range_start_utc_nanos;

    let remainder_nanos = range_duration_nanos.rem_euclid(interval_duration_nanos);
    if remainder_nanos != 0 {
        return Err(
            SurpriseNewsWeeklyProfileError::RangeDurationNotDivisibleByInterval {
                range_start_utc_nanos,
                range_end_utc_nanos,
                interval_duration_nanos,
                remainder_nanos,
            },
        );
    }

    let interval_count =
        usize::try_from(range_duration_nanos / interval_duration_nanos).map_err(|_| {
            SurpriseNewsWeeklyProfileError::IntervalOffsetOverflow {
                interval_index: usize::MAX,
                interval_duration_nanos,
            }
        })?;

    materialize_surprise_news_weekly_profile_multipliers_for_interval_grid(
        range_start_utc,
        interval_count,
        interval_duration_nanos,
        surprise_profile,
    )
}

pub(crate) fn weekly_profile_bucket_index(
    timestamp_utc: DateTime<Utc>,
    profile_resolution_hours: usize,
) -> usize {
    let hour_of_week = timestamp_utc.weekday().num_days_from_monday() as usize * 24
        + timestamp_utc.hour() as usize;

    hour_of_week / profile_resolution_hours
}

pub(crate) fn next_weekly_profile_bucket_boundary(
    timestamp_utc: DateTime<Utc>,
    profile_resolution_hours: usize,
) -> DateTime<Utc> {
    let hour_of_week = timestamp_utc.weekday().num_days_from_monday() as usize * 24
        + timestamp_utc.hour() as usize;

    let next_bucket_hour_of_week =
        (hour_of_week / profile_resolution_hours).saturating_add(1) * profile_resolution_hours;

    let week_start_date = timestamp_utc.date_naive()
        - Duration::days(timestamp_utc.weekday().num_days_from_monday() as i64);

    let week_start = NaiveDateTime::new(week_start_date, NaiveTime::MIN).and_utc();

    week_start + Duration::hours(next_bucket_hour_of_week as i64)
}
