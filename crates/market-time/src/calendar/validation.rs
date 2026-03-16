use std::collections::{BTreeMap, BTreeSet};

use crate::calendar::errors::ExchangeCalendarValidationError;
use crate::calendar::types::{
    CalendarDate, EarlyCloseSessionArtifact, SessionTime, SessionWindowDefinition,
};

pub(crate) fn build_holiday_dates(
    holiday_date_values: Vec<String>,
) -> Result<BTreeSet<CalendarDate>, ExchangeCalendarValidationError> {
    let mut holiday_dates: BTreeSet<CalendarDate> = BTreeSet::new();

    for holiday_date_value in holiday_date_values {
        let raw_date = require_non_empty(holiday_date_value, "holiday_dates")?;
        let date = CalendarDate::parse(&raw_date).map_err(|reason| {
            ExchangeCalendarValidationError::InvalidDate {
                field: "holiday_dates",
                value: raw_date.clone(),
                reason,
            }
        })?;

        if !holiday_dates.insert(date) {
            return Err(ExchangeCalendarValidationError::DuplicateHolidayDate {
                date: date.to_string(),
            });
        }
    }

    Ok(holiday_dates)
}

pub(crate) fn build_early_close_sessions(
    early_close_sessions: Vec<EarlyCloseSessionArtifact>,
) -> Result<BTreeMap<CalendarDate, Box<[SessionWindowDefinition]>>, ExchangeCalendarValidationError>
{
    let mut sessions_by_date: BTreeMap<CalendarDate, Box<[SessionWindowDefinition]>> =
        BTreeMap::new();

    for early_close_session in early_close_sessions {
        let raw_date = require_non_empty(early_close_session.date, "early_close_sessions.date")?;
        let date = CalendarDate::parse(&raw_date).map_err(|reason| {
            ExchangeCalendarValidationError::InvalidDate {
                field: "early_close_sessions.date",
                value: raw_date.clone(),
                reason,
            }
        })?;

        if early_close_session.windows.is_empty() {
            return Err(
                ExchangeCalendarValidationError::MissingEarlyCloseSessionWindows {
                    date: date.to_string(),
                },
            );
        }

        let mut windows = Vec::with_capacity(early_close_session.windows.len());

        for window in early_close_session.windows {
            let label = require_non_empty(window.label, "early_close_sessions.windows.label")?;

            let open_time = SessionTime::parse(&window.open_time).map_err(|parse_reason| {
                ExchangeCalendarValidationError::InvalidClockTime {
                    field: "early_close_sessions.windows.open_time",
                    value: window.open_time.clone(),
                    reason: parse_reason,
                }
            })?;
            let close_time = SessionTime::parse(&window.close_time).map_err(|parse_reason| {
                ExchangeCalendarValidationError::InvalidClockTime {
                    field: "early_close_sessions.windows.close_time",
                    value: window.close_time.clone(),
                    reason: parse_reason,
                }
            })?;

            if open_time >= close_time {
                return Err(
                    ExchangeCalendarValidationError::InvalidEarlyCloseSessionWindow {
                        date: date.to_string(),
                        label,
                        open_time: window.open_time,
                        close_time: window.close_time,
                        reason: "open_time must be earlier than close_time",
                    },
                );
            }

            windows.push(SessionWindowDefinition {
                label,
                open_time,
                close_time,
            });
        }

        windows.sort_by(|left_window, right_window| {
            left_window
                .open_time
                .cmp(&right_window.open_time)
                .then(left_window.close_time.cmp(&right_window.close_time))
                .then(left_window.label.cmp(&right_window.label))
        });

        for window_pair in windows.windows(2) {
            let first_window = &window_pair[0];
            let second_window = &window_pair[1];

            if first_window.close_time > second_window.open_time {
                return Err(
                    ExchangeCalendarValidationError::OverlappingEarlyCloseSessions {
                        date: date.to_string(),
                        first_label: first_window.label.clone(),
                        second_label: second_window.label.clone(),
                        first_window: format!(
                            "{}-{}",
                            first_window.open_time, first_window.close_time
                        ),
                        second_window: format!(
                            "{}-{}",
                            second_window.open_time, second_window.close_time
                        ),
                    },
                );
            }
        }

        if sessions_by_date
            .insert(date, windows.into_boxed_slice())
            .is_some()
        {
            return Err(ExchangeCalendarValidationError::DuplicateEarlyCloseDate {
                date: raw_date,
            });
        }
    }

    Ok(sessions_by_date)
}

pub(crate) fn require_non_empty(
    value: String,
    field: &'static str,
) -> Result<String, ExchangeCalendarValidationError> {
    let trimmed_value = value.trim();

    if trimmed_value.is_empty() {
        return Err(ExchangeCalendarValidationError::MissingField { field });
    }

    Ok(trimmed_value.to_string())
}

pub(crate) fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|string_value| string_value.trim().to_string())
        .and_then(|trimmed_value| (!trimmed_value.is_empty()).then_some(trimmed_value))
}

pub(crate) fn parse_unknown_field_name(error_message: &str) -> Option<String> {
    let unknown_field_prefix = "unknown field `";
    let after_prefix = error_message.split_once(unknown_field_prefix)?.1;
    let field_name = after_prefix.split('`').next()?;

    Some(field_name.to_string())
}

pub(crate) fn validate_iso_date(raw: &str) -> Result<(), &'static str> {
    parse_iso_date_parts(raw).map(|_| ())
}

pub(crate) fn parse_iso_date_parts(raw: &str) -> Result<(i32, u8, u8), &'static str> {
    if raw.len() != 10 {
        return Err("date must use YYYY-MM-DD format");
    }

    let bytes = raw.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return Err("date must use YYYY-MM-DD format");
    }

    if !bytes[0..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
    {
        return Err("date must contain numeric year, month, and day values");
    }

    let year = raw[0..4]
        .parse::<i32>()
        .map_err(|_| "year must be numeric")?;
    let month = raw[5..7]
        .parse::<u8>()
        .map_err(|_| "month must be numeric")?;
    let day = raw[8..10]
        .parse::<u8>()
        .map_err(|_| "day must be numeric")?;

    if month == 0 || month > 12 {
        return Err("month must be between 01 and 12");
    }

    let maximum_day = max_day_of_month(year, month);
    if day == 0 || day > maximum_day {
        return Err("day is outside valid range for month");
    }

    Ok((year, month, day))
}

fn max_day_of_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap_year = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if leap_year { 29 } else { 28 }
        }
        _ => 31,
    }
}
