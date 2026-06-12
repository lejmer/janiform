use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Datelike, Days, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use janiform_foundations::time::{MINUTES_PER_DAY, MINUTES_PER_HOUR, NANOS_PER_SECOND};

use super::coordinates::{annual_phase, intraday_phase, weekly_phase};
use crate::calendar::{CalendarDate, ExchangeCalendarDefinition, SessionTime, TradingWeekday};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BusinessTimeClockExposureRow {
    pub interval_start_utc_nanos: i64,
    pub interval_end_utc_nanos: i64,
    pub exposure_seconds: f64,
    pub event_count: usize,
    pub intraday_phase: f64,
    pub weekly_phase: f64,
    pub annual_phase: f64,
}

pub struct BusinessTimeClockExposureBuildInput<'a> {
    pub calendar_definition: &'a ExchangeCalendarDefinition,
    pub timezone: Tz,
    pub range_start_utc_nanos: i64,
    pub range_end_utc_nanos: i64,
    pub event_counts_by_minute_start_utc_nanos: &'a BTreeMap<i64, usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BusinessTimeClockExposureError {
    NonIncreasingRange,
    LocalDateOutOfRange {
        local_date: NaiveDate,
    },
    LocalBoundaryOutOfRange {
        local_date: NaiveDate,
        minute_of_day: usize,
    },
    AmbiguousLocalBoundary {
        local_date: NaiveDate,
        minute_of_day: usize,
        timezone: Tz,
    },
    NonexistentLocalBoundary {
        local_date: NaiveDate,
        minute_of_day: usize,
        timezone: Tz,
    },
    TimestampOutOfRange {
        local_date: NaiveDate,
        minute_of_day: usize,
    },
    DateIterationOverflow,
    BoundaryDateOverflow,
    ExposureOverflow,
    InvalidCoordinate {
        local_date: NaiveDate,
        minute_of_day: usize,
    },
}

impl Display for BusinessTimeClockExposureError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonIncreasingRange => {
                formatter.write_str("business-time clock exposure range must be increasing")
            }
            Self::LocalDateOutOfRange { local_date } => write!(
                formatter,
                "business-time clock cannot represent local calendar date {local_date}"
            ),
            Self::LocalBoundaryOutOfRange {
                local_date,
                minute_of_day,
            } => write!(
                formatter,
                "business-time clock received invalid local boundary {local_date} minute_of_day={minute_of_day}"
            ),
            Self::AmbiguousLocalBoundary {
                local_date,
                minute_of_day,
                timezone,
            } => write!(
                formatter,
                "business-time clock encountered ambiguous local boundary {local_date} minute_of_day={minute_of_day} in timezone {timezone}"
            ),
            Self::NonexistentLocalBoundary {
                local_date,
                minute_of_day,
                timezone,
            } => write!(
                formatter,
                "business-time clock encountered nonexistent local boundary {local_date} minute_of_day={minute_of_day} in timezone {timezone}"
            ),
            Self::TimestampOutOfRange {
                local_date,
                minute_of_day,
            } => write!(
                formatter,
                "business-time clock local boundary {local_date} minute_of_day={minute_of_day} is outside the UTC nanosecond range"
            ),
            Self::DateIterationOverflow => {
                formatter.write_str("business-time clock overflowed local date iteration")
            }
            Self::BoundaryDateOverflow => {
                formatter.write_str("business-time clock overflowed local boundary date")
            }
            Self::ExposureOverflow => {
                formatter.write_str("business-time clock overflowed exposure accumulation")
            }
            Self::InvalidCoordinate {
                local_date,
                minute_of_day,
            } => write!(
                formatter,
                "business-time clock could not compute recurring coordinates for {local_date} minute_of_day={minute_of_day}"
            ),
        }
    }
}

impl Error for BusinessTimeClockExposureError {}

pub fn build_business_time_clock_exposure_rows(
    input: BusinessTimeClockExposureBuildInput<'_>,
) -> Result<Vec<BusinessTimeClockExposureRow>, BusinessTimeClockExposureError> {
    if input.range_end_utc_nanos <= input.range_start_utc_nanos {
        return Err(BusinessTimeClockExposureError::NonIncreasingRange);
    }

    let interval_start_utc = DateTime::<Utc>::from_timestamp_nanos(input.range_start_utc_nanos);

    let interval_end_exclusive_utc =
        DateTime::<Utc>::from_timestamp_nanos(input.range_end_utc_nanos - 1);

    let mut local_date = interval_start_utc
        .with_timezone(&input.timezone)
        .date_naive();

    let last_local_date = interval_end_exclusive_utc
        .with_timezone(&input.timezone)
        .date_naive();

    let mut rows = Vec::new();

    while local_date <= last_local_date {
        let calendar_date = calendar_date_from_naive_date(local_date)?;
        let local_weekday = TradingWeekday::from_chrono_weekday(local_date.weekday());

        for session_window in input
            .calendar_definition
            .session_windows_for_local_date(calendar_date, local_weekday)
        {
            add_session_window_rows(
                &mut rows,
                SessionWindowRowsInput {
                    timezone: input.timezone,
                    local_date,
                    session_open_time: session_window.open_time(),
                    session_close_time: session_window.close_time(),
                    range_start_utc_nanos: input.range_start_utc_nanos,
                    range_end_utc_nanos: input.range_end_utc_nanos,
                    event_counts_by_minute_start_utc_nanos: input
                        .event_counts_by_minute_start_utc_nanos,
                },
            )?;
        }

        local_date = local_date
            .checked_add_days(Days::new(1))
            .ok_or(BusinessTimeClockExposureError::DateIterationOverflow)?;
    }

    Ok(rows)
}

struct SessionWindowRowsInput<'a> {
    timezone: Tz,
    local_date: NaiveDate,
    session_open_time: SessionTime,
    session_close_time: SessionTime,
    range_start_utc_nanos: i64,
    range_end_utc_nanos: i64,
    event_counts_by_minute_start_utc_nanos: &'a BTreeMap<i64, usize>,
}

fn add_session_window_rows(
    rows: &mut Vec<BusinessTimeClockExposureRow>,
    input: SessionWindowRowsInput<'_>,
) -> Result<(), BusinessTimeClockExposureError> {
    let session_open_minute = session_time_minute_of_day(input.session_open_time);
    let session_close_minute = session_time_minute_of_day(input.session_close_time);

    for minute_of_day in session_open_minute..session_close_minute {
        let segment_start_utc_nanos =
            local_minute_boundary_utc_nanos(input.timezone, input.local_date, minute_of_day)?;

        let segment_end_utc_nanos =
            local_minute_boundary_utc_nanos(input.timezone, input.local_date, minute_of_day + 1)?;

        let overlap_start_utc_nanos = input.range_start_utc_nanos.max(segment_start_utc_nanos);
        let overlap_end_utc_nanos = input.range_end_utc_nanos.min(segment_end_utc_nanos);

        if overlap_start_utc_nanos >= overlap_end_utc_nanos {
            continue;
        }

        let exposure_nanos = overlap_end_utc_nanos
            .checked_sub(overlap_start_utc_nanos)
            .ok_or(BusinessTimeClockExposureError::ExposureOverflow)?;

        let exposure_seconds = exposure_nanos as f64 / NANOS_PER_SECOND as f64;
        if !exposure_seconds.is_finite() || exposure_seconds <= 0.0 {
            return Err(BusinessTimeClockExposureError::ExposureOverflow);
        }

        let intraday_phase = intraday_phase(minute_of_day).ok_or(
            BusinessTimeClockExposureError::InvalidCoordinate {
                local_date: input.local_date,
                minute_of_day,
            },
        )?;

        let weekly_phase = weekly_phase(input.local_date).ok_or(
            BusinessTimeClockExposureError::InvalidCoordinate {
                local_date: input.local_date,
                minute_of_day,
            },
        )?;

        let annual_phase = annual_phase(input.local_date, minute_of_day).ok_or(
            BusinessTimeClockExposureError::InvalidCoordinate {
                local_date: input.local_date,
                minute_of_day,
            },
        )?;

        rows.push(BusinessTimeClockExposureRow {
            interval_start_utc_nanos: overlap_start_utc_nanos,
            interval_end_utc_nanos: overlap_end_utc_nanos,
            exposure_seconds,
            event_count: input
                .event_counts_by_minute_start_utc_nanos
                .get(&segment_start_utc_nanos)
                .copied()
                .unwrap_or(0),
            intraday_phase,
            weekly_phase,
            annual_phase,
        });
    }

    Ok(())
}

fn calendar_date_from_naive_date(
    local_date: NaiveDate,
) -> Result<CalendarDate, BusinessTimeClockExposureError> {
    Ok(CalendarDate {
        year: local_date.year(),
        month: u8::try_from(local_date.month())
            .map_err(|_| BusinessTimeClockExposureError::LocalDateOutOfRange { local_date })?,
        day: u8::try_from(local_date.day())
            .map_err(|_| BusinessTimeClockExposureError::LocalDateOutOfRange { local_date })?,
    })
}

fn session_time_minute_of_day(session_time: SessionTime) -> usize {
    usize::from(session_time.hour()) * MINUTES_PER_HOUR + usize::from(session_time.minute())
}

pub(super) fn local_minute_boundary_utc_nanos(
    timezone: Tz,
    local_date: NaiveDate,
    minute_of_day: usize,
) -> Result<i64, BusinessTimeClockExposureError> {
    if minute_of_day > MINUTES_PER_DAY {
        return Err(BusinessTimeClockExposureError::LocalBoundaryOutOfRange {
            local_date,
            minute_of_day,
        });
    }

    let day_offset = minute_of_day / MINUTES_PER_DAY;
    let minute_within_day = minute_of_day % MINUTES_PER_DAY;

    let boundary_date = local_date
        .checked_add_days(Days::new(
            u64::try_from(day_offset)
                .map_err(|_| BusinessTimeClockExposureError::BoundaryDateOverflow)?,
        ))
        .ok_or(BusinessTimeClockExposureError::BoundaryDateOverflow)?;

    let hour = u32::try_from(minute_within_day / MINUTES_PER_HOUR).map_err(|_| {
        BusinessTimeClockExposureError::LocalBoundaryOutOfRange {
            local_date,
            minute_of_day,
        }
    })?;

    let minute = u32::try_from(minute_within_day % MINUTES_PER_HOUR).map_err(|_| {
        BusinessTimeClockExposureError::LocalBoundaryOutOfRange {
            local_date,
            minute_of_day,
        }
    })?;

    match timezone.with_ymd_and_hms(
        boundary_date.year(),
        boundary_date.month(),
        boundary_date.day(),
        hour,
        minute,
        0,
    ) {
        LocalResult::Single(timestamp) => timestamp
            .with_timezone(&Utc)
            .timestamp_nanos_opt()
            .ok_or(BusinessTimeClockExposureError::TimestampOutOfRange {
                local_date,
                minute_of_day,
            }),
        LocalResult::Ambiguous(_, _) => {
            Err(BusinessTimeClockExposureError::AmbiguousLocalBoundary {
                local_date,
                minute_of_day,
                timezone,
            })
        }
        LocalResult::None => Err(BusinessTimeClockExposureError::NonexistentLocalBoundary {
            local_date,
            minute_of_day,
            timezone,
        }),
    }
}
