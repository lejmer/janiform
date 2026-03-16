use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::calendar::types::TradingWeekday;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeCalendarValidationError {
    MissingField {
        field: &'static str,
    },
    MissingSessionWindows {
        weekday: TradingWeekday,
    },
    MissingEarlyCloseSessionWindows {
        date: String,
    },
    InvalidDate {
        field: &'static str,
        value: String,
        reason: &'static str,
    },
    InvalidClockTime {
        field: &'static str,
        value: String,
        reason: &'static str,
    },
    InvalidSessionWindow {
        weekday: TradingWeekday,
        label: String,
        open_time: String,
        close_time: String,
        reason: &'static str,
    },
    OverlappingRegularSessions {
        weekday: TradingWeekday,
        first_label: String,
        second_label: String,
        first_window: String,
        second_window: String,
    },
    InvalidEarlyCloseSessionWindow {
        date: String,
        label: String,
        open_time: String,
        close_time: String,
        reason: &'static str,
    },
    OverlappingEarlyCloseSessions {
        date: String,
        first_label: String,
        second_label: String,
        first_window: String,
        second_window: String,
    },
    DuplicateHolidayDate {
        date: String,
    },
    DuplicateEarlyCloseDate {
        date: String,
    },
}

impl Display for ExchangeCalendarValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField { field } => write!(
                formatter,
                "missing required field `{field}` in calendar artifact"
            ),
            Self::MissingSessionWindows { weekday } => write!(
                formatter,
                "regular session entry for {weekday} must include at least one session window"
            ),
            Self::MissingEarlyCloseSessionWindows { date } => write!(
                formatter,
                "early-close session entry for {date} must include at least one session window"
            ),
            Self::InvalidDate {
                field,
                value,
                reason,
            } => write!(formatter, "invalid date `{value}` in `{field}`: {reason}"),
            Self::InvalidClockTime {
                field,
                value,
                reason,
            } => write!(
                formatter,
                "invalid clock time `{value}` in `{field}`: {reason}"
            ),
            Self::InvalidSessionWindow {
                weekday,
                label,
                open_time,
                close_time,
                reason,
            } => write!(
                formatter,
                "invalid session window `{label}` for {weekday} using {open_time}-{close_time}: {reason}"
            ),
            Self::OverlappingRegularSessions {
                weekday,
                first_label,
                second_label,
                first_window,
                second_window,
            } => write!(
                formatter,
                "overlapping regular session windows for {weekday}: `{first_label}` ({first_window}) overlaps `{second_label}` ({second_window})"
            ),
            Self::InvalidEarlyCloseSessionWindow {
                date,
                label,
                open_time,
                close_time,
                reason,
            } => write!(
                formatter,
                "invalid early-close session window `{label}` for {date} using {open_time}-{close_time}: {reason}"
            ),
            Self::OverlappingEarlyCloseSessions {
                date,
                first_label,
                second_label,
                first_window,
                second_window,
            } => write!(
                formatter,
                "overlapping early-close session windows for {date}: `{first_label}` ({first_window}) overlaps `{second_label}` ({second_window})"
            ),
            Self::DuplicateHolidayDate { date } => {
                write!(
                    formatter,
                    "duplicate holiday date `{date}` in calendar artifact"
                )
            }
            Self::DuplicateEarlyCloseDate { date } => write!(
                formatter,
                "duplicate early-close date `{date}` in calendar artifact"
            ),
        }
    }
}

impl Error for ExchangeCalendarValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeCalendarRegistryLookupError {
    MissingExchangeId { exchange_code: String },
}

impl Display for ExchangeCalendarRegistryLookupError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingExchangeId { exchange_code } => {
                write!(
                    formatter,
                    "missing exchange id `{exchange_code}` in registry"
                )
            }
        }
    }
}

impl Error for ExchangeCalendarRegistryLookupError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeCalendarResolveError {
    MissingExchangeId {
        exchange_code: String,
    },
    InvalidTimezoneName {
        exchange_code: String,
        timezone_name: String,
    },
}

impl ExchangeCalendarResolveError {
    pub(crate) fn from_lookup_error(source: ExchangeCalendarRegistryLookupError) -> Self {
        match source {
            ExchangeCalendarRegistryLookupError::MissingExchangeId { exchange_code } => {
                Self::MissingExchangeId { exchange_code }
            }
        }
    }
}

impl Display for ExchangeCalendarResolveError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingExchangeId { exchange_code } => {
                write!(
                    formatter,
                    "missing exchange id `{exchange_code}` in registry"
                )
            }
            Self::InvalidTimezoneName {
                exchange_code,
                timezone_name,
            } => {
                write!(
                    formatter,
                    "invalid timezone `{timezone_name}` configured for exchange `{exchange_code}`"
                )
            }
        }
    }
}

impl Error for ExchangeCalendarResolveError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimestampEventPreprocessingError {
    InvalidSparseHourEventThreshold {
        sparse_hour_event_threshold: usize,
    },
    TimestampOutOfRange {
        source_record_position: u64,
    },
    CalendarResolve {
        source: ExchangeCalendarResolveError,
    },
}

impl TimestampEventPreprocessingError {
    pub(crate) fn from_calendar_resolve_error(source: ExchangeCalendarResolveError) -> Self {
        Self::CalendarResolve { source }
    }
}

impl Display for TimestampEventPreprocessingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSparseHourEventThreshold {
                sparse_hour_event_threshold,
            } => write!(
                formatter,
                "sparse_hour_event_threshold must be at least 1, received {sparse_hour_event_threshold}"
            ),
            Self::TimestampOutOfRange {
                source_record_position,
            } => write!(
                formatter,
                "event record at position {source_record_position} has a timestamp outside the supported nanosecond range"
            ),
            Self::CalendarResolve { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for TimestampEventPreprocessingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CalendarResolve { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeCalendarRegistryLoadError {
    MalformedArtifact {
        reason: String,
    },
    UnknownField {
        field: String,
    },
    MissingField {
        field: &'static str,
    },
    MalformedDate {
        field: &'static str,
        value: String,
        reason: &'static str,
    },
    UnsupportedSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    DuplicateExchangeId {
        exchange_code: String,
    },
    InvalidExchangeArtifact {
        exchange_code: Option<String>,
        source: ExchangeCalendarValidationError,
    },
}

impl ExchangeCalendarRegistryLoadError {
    pub(crate) fn from_validation_error(source: ExchangeCalendarValidationError) -> Self {
        match source {
            ExchangeCalendarValidationError::MissingField { field } => Self::MissingField { field },
            _ => Self::InvalidExchangeArtifact {
                exchange_code: None,
                source,
            },
        }
    }

    pub(crate) fn from_deserialization_error(source: serde_json::Error) -> Self {
        let message = source.to_string();
        if let Some(field_name) = crate::calendar::validation::parse_unknown_field_name(&message) {
            return Self::UnknownField { field: field_name };
        }
        Self::MalformedArtifact { reason: message }
    }
}

impl Display for ExchangeCalendarRegistryLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedArtifact { reason } => {
                write!(formatter, "malformed calendar registry artifact: {reason}")
            }
            Self::UnknownField { field } => {
                write!(
                    formatter,
                    "unknown field `{field}` in calendar registry artifact"
                )
            }
            Self::MissingField { field } => {
                write!(
                    formatter,
                    "missing required field `{field}` in calendar registry artifact"
                )
            }
            Self::MalformedDate {
                field,
                value,
                reason,
            } => {
                write!(formatter, "malformed date `{value}` in `{field}`: {reason}")
            }
            Self::UnsupportedSchemaVersion { expected, actual } => {
                write!(
                    formatter,
                    "unsupported registry schema version `{actual}`, expected `{expected}`"
                )
            }
            Self::DuplicateExchangeId { exchange_code } => {
                write!(
                    formatter,
                    "duplicate exchange id `{exchange_code}` in calendar registry artifact"
                )
            }
            Self::InvalidExchangeArtifact {
                exchange_code,
                source,
            } => {
                if let Some(exchange_code) = exchange_code {
                    write!(
                        formatter,
                        "invalid calendar artifact for exchange `{exchange_code}`: {source}"
                    )
                } else {
                    write!(formatter, "invalid calendar artifact: {source}")
                }
            }
        }
    }
}

impl Error for ExchangeCalendarRegistryLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidExchangeArtifact { source, .. } => Some(source),
            _ => None,
        }
    }
}
