//! Exchange calendar domain model and artifact validation.
//!
//! The model is designed for immutable construction from versioned artifacts.
//! It covers timezone context, exchange metadata, and regular session windows
//! that can be consumed by downstream calendar rule resolution.
//!
//! # Examples
//!
//! A single regular session per day:
//! ```rust
//! use janiform_core::calendar::{
//!     CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarDefinition,
//!     RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
//! };
//!
//! let artifact = ExchangeCalendarArtifact {
//!     schema_version: "calendar.v1".to_string(),
//!     exchange_code: "XNYS".to_string(),
//!     exchange_name: "New York Stock Exchange".to_string(),
//!     timezone_name: Some("America/New_York".to_string()),
//!     rule_metadata: CalendarRuleMetadataArtifact {
//!         holiday_calendar_code: "US_EQUITIES".to_string(),
//!         early_close_rule_set: Some("US_EQUITIES_EARLY_CLOSE".to_string()),
//!     },
//!     regular_sessions: vec![RegularSessionArtifact {
//!         weekday: TradingWeekday::Monday,
//!         windows: vec![SessionWindowArtifact {
//!             label: "regular".to_string(),
//!             open_time: "09:30".to_string(),
//!             close_time: "16:00".to_string(),
//!         }],
//!     }],
//! };
//!
//! let definition = ExchangeCalendarDefinition::from_artifact(artifact)?;
//! assert_eq!(definition.timezone_name(), "America/New_York");
//! # Ok::<(), janiform_core::calendar::ExchangeCalendarValidationError>(())
//! ```
//!
//! A split session on a single weekday:
//! ```rust
//! use janiform_core::calendar::{
//!     CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarDefinition,
//!     RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
//! };
//!
//! let artifact = ExchangeCalendarArtifact {
//!     schema_version: "calendar.v1".to_string(),
//!     exchange_code: "XTKS".to_string(),
//!     exchange_name: "Tokyo Stock Exchange".to_string(),
//!     timezone_name: Some("Asia/Tokyo".to_string()),
//!     rule_metadata: CalendarRuleMetadataArtifact {
//!         holiday_calendar_code: "JP_EQUITIES".to_string(),
//!         early_close_rule_set: None,
//!     },
//!     regular_sessions: vec![RegularSessionArtifact {
//!         weekday: TradingWeekday::Monday,
//!         windows: vec![
//!             SessionWindowArtifact {
//!                 label: "morning".to_string(),
//!                 open_time: "09:00".to_string(),
//!                 close_time: "11:30".to_string(),
//!             },
//!             SessionWindowArtifact {
//!                 label: "afternoon".to_string(),
//!                 open_time: "12:30".to_string(),
//!                 close_time: "15:00".to_string(),
//!             },
//!         ],
//!     }],
//! };
//!
//! let definition = ExchangeCalendarDefinition::from_artifact(artifact)?;
//! assert_eq!(definition.regular_sessions().len(), 1);
//! # Ok::<(), janiform_core::calendar::ExchangeCalendarValidationError>(())
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Weekday used by regular exchange sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TradingWeekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Display for TradingWeekday {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Monday => "Monday",
            Self::Tuesday => "Tuesday",
            Self::Wednesday => "Wednesday",
            Self::Thursday => "Thursday",
            Self::Friday => "Friday",
            Self::Saturday => "Saturday",
            Self::Sunday => "Sunday",
        };
        formatter.write_str(value)
    }
}

/// Artifact structure loaded from versioned calendar configuration data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeCalendarArtifact {
    pub schema_version: String,
    pub exchange_code: String,
    pub exchange_name: String,
    pub timezone_name: Option<String>,
    pub rule_metadata: CalendarRuleMetadataArtifact,
    pub regular_sessions: Vec<RegularSessionArtifact>,
}

/// Artifact metadata for holiday and early close rule resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarRuleMetadataArtifact {
    pub holiday_calendar_code: String,
    pub early_close_rule_set: Option<String>,
}

/// Regular session windows for a weekday in artifact form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegularSessionArtifact {
    pub weekday: TradingWeekday,
    pub windows: Vec<SessionWindowArtifact>,
}

/// Session window in artifact form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWindowArtifact {
    pub label: String,
    pub open_time: String,
    pub close_time: String,
}

/// Immutable exchange calendar definition used by core calendar services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeCalendarDefinition {
    schema_version: String,
    exchange_metadata: ExchangeMetadata,
    timezone_name: String,
    rule_metadata: CalendarRuleMetadata,
    regular_sessions: Box<[RegularSessionDefinition]>,
}

impl ExchangeCalendarDefinition {
    /// Build an immutable calendar definition from a versioned artifact.
    pub fn from_artifact(
        artifact: ExchangeCalendarArtifact,
    ) -> Result<Self, ExchangeCalendarValidationError> {
        let schema_version = require_non_empty(artifact.schema_version, "schema_version")?;
        let exchange_code = require_non_empty(artifact.exchange_code, "exchange_code")?;
        let exchange_name = require_non_empty(artifact.exchange_name, "exchange_name")?;
        let timezone_name = match artifact.timezone_name {
            Some(timezone_name) => require_non_empty(timezone_name, "timezone_name")?,
            None => {
                return Err(ExchangeCalendarValidationError::MissingField {
                    field: "timezone_name",
                });
            }
        };
        let holiday_calendar_code = require_non_empty(
            artifact.rule_metadata.holiday_calendar_code,
            "rule_metadata.holiday_calendar_code",
        )?;
        let early_close_rule_set = normalize_optional_string(artifact.rule_metadata.early_close_rule_set);

        if artifact.regular_sessions.is_empty() {
            return Err(ExchangeCalendarValidationError::MissingField {
                field: "regular_sessions",
            });
        }

        let mut grouped_windows: BTreeMap<TradingWeekday, Vec<SessionWindowDefinition>> = BTreeMap::new();
        for weekday_sessions in artifact.regular_sessions {
            if weekday_sessions.windows.is_empty() {
                return Err(ExchangeCalendarValidationError::MissingSessionWindows {
                    weekday: weekday_sessions.weekday,
                });
            }

            let weekday = weekday_sessions.weekday;
            for window in weekday_sessions.windows {
                let label = require_non_empty(window.label, "regular_sessions.windows.label")?;
                let open_time = SessionTime::parse(&window.open_time).map_err(|parse_reason| {
                    ExchangeCalendarValidationError::InvalidClockTime {
                        field: "regular_sessions.windows.open_time",
                        value: window.open_time.clone(),
                        reason: parse_reason,
                    }
                })?;
                let close_time = SessionTime::parse(&window.close_time).map_err(|parse_reason| {
                    ExchangeCalendarValidationError::InvalidClockTime {
                        field: "regular_sessions.windows.close_time",
                        value: window.close_time.clone(),
                        reason: parse_reason,
                    }
                })?;
                if open_time >= close_time {
                    return Err(ExchangeCalendarValidationError::InvalidSessionWindow {
                        weekday,
                        label,
                        open_time: window.open_time,
                        close_time: window.close_time,
                        reason: "open_time must be earlier than close_time",
                    });
                }

                grouped_windows
                    .entry(weekday)
                    .or_default()
                    .push(SessionWindowDefinition {
                        label,
                        open_time,
                        close_time,
                    });
            }
        }

        for (weekday, weekday_windows) in &mut grouped_windows {
            weekday_windows.sort_by(|left_window, right_window| {
                left_window
                    .open_time
                    .cmp(&right_window.open_time)
                    .then(left_window.close_time.cmp(&right_window.close_time))
                    .then(left_window.label.cmp(&right_window.label))
            });
            for window_pair in weekday_windows.windows(2) {
                let first_window = &window_pair[0];
                let second_window = &window_pair[1];
                if first_window.close_time > second_window.open_time {
                    return Err(ExchangeCalendarValidationError::OverlappingRegularSessions {
                        weekday: *weekday,
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
                    });
                }
            }
        }

        let regular_sessions = grouped_windows
            .into_iter()
            .map(|(weekday, windows)| RegularSessionDefinition {
                weekday,
                windows: windows.into_boxed_slice(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Ok(Self {
            schema_version,
            exchange_metadata: ExchangeMetadata {
                exchange_code,
                exchange_name,
            },
            timezone_name,
            rule_metadata: CalendarRuleMetadata {
                holiday_calendar_code,
                early_close_rule_set,
            },
            regular_sessions,
        })
    }

    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    pub fn exchange_metadata(&self) -> &ExchangeMetadata {
        &self.exchange_metadata
    }

    pub fn timezone_name(&self) -> &str {
        &self.timezone_name
    }

    pub fn rule_metadata(&self) -> &CalendarRuleMetadata {
        &self.rule_metadata
    }

    pub fn regular_sessions(&self) -> &[RegularSessionDefinition] {
        &self.regular_sessions
    }
}

/// Immutable metadata for an exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeMetadata {
    exchange_code: String,
    exchange_name: String,
}

impl ExchangeMetadata {
    pub fn exchange_code(&self) -> &str {
        &self.exchange_code
    }

    pub fn exchange_name(&self) -> &str {
        &self.exchange_name
    }
}

/// Immutable metadata required to resolve non-regular calendar rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarRuleMetadata {
    holiday_calendar_code: String,
    early_close_rule_set: Option<String>,
}

impl CalendarRuleMetadata {
    pub fn holiday_calendar_code(&self) -> &str {
        &self.holiday_calendar_code
    }

    pub fn early_close_rule_set(&self) -> Option<&str> {
        self.early_close_rule_set.as_deref()
    }
}

/// Immutable regular sessions for one weekday.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegularSessionDefinition {
    weekday: TradingWeekday,
    windows: Box<[SessionWindowDefinition]>,
}

impl RegularSessionDefinition {
    pub fn weekday(&self) -> TradingWeekday {
        self.weekday
    }

    pub fn windows(&self) -> &[SessionWindowDefinition] {
        &self.windows
    }
}

/// Immutable session window represented as a half-open interval [open, close).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWindowDefinition {
    label: String,
    open_time: SessionTime,
    close_time: SessionTime,
}

impl SessionWindowDefinition {
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn open_time(&self) -> SessionTime {
        self.open_time
    }

    pub fn close_time(&self) -> SessionTime {
        self.close_time
    }
}

/// Clock time represented in local exchange time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionTime {
    hour: u8,
    minute: u8,
}

impl SessionTime {
    fn parse(raw: &str) -> Result<Self, &'static str> {
        let mut parts = raw.split(':');
        let hour_segment = parts
            .next()
            .ok_or("clock time must be in HH:MM format with two digits per component")?;
        let minute_segment = parts
            .next()
            .ok_or("clock time must be in HH:MM format with two digits per component")?;
        if parts.next().is_some() {
            return Err("clock time must be in HH:MM format with two digits per component");
        }
        if hour_segment.len() != 2 || minute_segment.len() != 2 {
            return Err("clock time must be in HH:MM format with two digits per component");
        }

        let hour = hour_segment
            .parse::<u8>()
            .map_err(|_| "hour must be a numeric value")?;
        let minute = minute_segment
            .parse::<u8>()
            .map_err(|_| "minute must be a numeric value")?;
        if hour > 23 {
            return Err("hour must be between 00 and 23");
        }
        if minute > 59 {
            return Err("minute must be between 00 and 59");
        }

        Ok(Self { hour, minute })
    }
}

impl Display for SessionTime {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:02}:{:02}", self.hour, self.minute)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeCalendarValidationError {
    MissingField {
        field: &'static str,
    },
    MissingSessionWindows {
        weekday: TradingWeekday,
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
        }
    }
}

impl Error for ExchangeCalendarValidationError {}

fn require_non_empty(
    value: String,
    field: &'static str,
) -> Result<String, ExchangeCalendarValidationError> {
    let trimmed_value = value.trim();
    if trimmed_value.is_empty() {
        return Err(ExchangeCalendarValidationError::MissingField { field });
    }
    Ok(trimmed_value.to_string())
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|string_value| string_value.trim().to_string())
        .and_then(|trimmed_value| (!trimmed_value.is_empty()).then_some(trimmed_value))
}

#[cfg(test)]
mod tests {
    use super::{
        CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarDefinition,
        ExchangeCalendarValidationError, RegularSessionArtifact, SessionWindowArtifact,
        TradingWeekday,
    };

    #[test]
    fn missing_timezone_fails_validation() {
        let mut artifact = valid_exchange_artifact();
        artifact.timezone_name = None;

        let result = ExchangeCalendarDefinition::from_artifact(artifact);

        assert_eq!(
            result,
            Err(ExchangeCalendarValidationError::MissingField {
                field: "timezone_name",
            })
        );
    }

    #[test]
    fn invalid_session_window_fails_validation() {
        let artifact = ExchangeCalendarArtifact {
            schema_version: "calendar.v1".to_string(),
            exchange_code: "XNYS".to_string(),
            exchange_name: "New York Stock Exchange".to_string(),
            timezone_name: Some("America/New_York".to_string()),
            rule_metadata: CalendarRuleMetadataArtifact {
                holiday_calendar_code: "US_EQUITIES".to_string(),
                early_close_rule_set: Some("US_EQUITIES_EARLY_CLOSE".to_string()),
            },
            regular_sessions: vec![RegularSessionArtifact {
                weekday: TradingWeekday::Monday,
                windows: vec![SessionWindowArtifact {
                    label: "regular".to_string(),
                    open_time: "16:00".to_string(),
                    close_time: "09:30".to_string(),
                }],
            }],
        };

        let result = ExchangeCalendarDefinition::from_artifact(artifact);

        match result {
            Err(ExchangeCalendarValidationError::InvalidSessionWindow {
                weekday,
                label,
                open_time,
                close_time,
                reason,
            }) => {
                assert_eq!(weekday, TradingWeekday::Monday);
                assert_eq!(label, "regular");
                assert_eq!(open_time, "16:00");
                assert_eq!(close_time, "09:30");
                assert_eq!(reason, "open_time must be earlier than close_time");
            }
            _ => panic!("expected InvalidSessionWindow"),
        }
    }

    #[test]
    fn overlapping_regular_sessions_fail_validation() {
        let artifact = ExchangeCalendarArtifact {
            schema_version: "calendar.v1".to_string(),
            exchange_code: "XTKS".to_string(),
            exchange_name: "Tokyo Stock Exchange".to_string(),
            timezone_name: Some("Asia/Tokyo".to_string()),
            rule_metadata: CalendarRuleMetadataArtifact {
                holiday_calendar_code: "JP_EQUITIES".to_string(),
                early_close_rule_set: None,
            },
            regular_sessions: vec![RegularSessionArtifact {
                weekday: TradingWeekday::Monday,
                windows: vec![
                    SessionWindowArtifact {
                        label: "morning".to_string(),
                        open_time: "09:00".to_string(),
                        close_time: "11:30".to_string(),
                    },
                    SessionWindowArtifact {
                        label: "overlap".to_string(),
                        open_time: "11:00".to_string(),
                        close_time: "15:00".to_string(),
                    },
                ],
            }],
        };

        let result = ExchangeCalendarDefinition::from_artifact(artifact);

        match result {
            Err(ExchangeCalendarValidationError::OverlappingRegularSessions {
                weekday,
                first_label,
                second_label,
                first_window,
                second_window,
            }) => {
                assert_eq!(weekday, TradingWeekday::Monday);
                assert_eq!(first_label, "morning");
                assert_eq!(second_label, "overlap");
                assert_eq!(first_window, "09:00-11:30");
                assert_eq!(second_window, "11:00-15:00");
            }
            _ => panic!("expected OverlappingRegularSessions"),
        }
    }

    #[test]
    fn valid_definition_with_multiple_weekdays_builds_successfully() {
        let artifact = valid_exchange_artifact();

        let definition = ExchangeCalendarDefinition::from_artifact(artifact)
            .expect("valid artifact should build an exchange calendar definition");

        assert_eq!(definition.schema_version(), "calendar.v1");
        assert_eq!(definition.exchange_metadata().exchange_code(), "XNYS");
        assert_eq!(
            definition.exchange_metadata().exchange_name(),
            "New York Stock Exchange"
        );
        assert_eq!(definition.timezone_name(), "America/New_York");
        assert_eq!(
            definition.rule_metadata().holiday_calendar_code(),
            "US_EQUITIES"
        );
        assert_eq!(
            definition.rule_metadata().early_close_rule_set(),
            Some("US_EQUITIES_EARLY_CLOSE")
        );
        assert_eq!(definition.regular_sessions().len(), 3);
        assert_eq!(
            definition.regular_sessions()[0].weekday(),
            TradingWeekday::Monday
        );
        assert_eq!(
            definition.regular_sessions()[1].weekday(),
            TradingWeekday::Tuesday
        );
        assert_eq!(
            definition.regular_sessions()[2].weekday(),
            TradingWeekday::Wednesday
        );
    }

    fn valid_exchange_artifact() -> ExchangeCalendarArtifact {
        ExchangeCalendarArtifact {
            schema_version: "calendar.v1".to_string(),
            exchange_code: "XNYS".to_string(),
            exchange_name: "New York Stock Exchange".to_string(),
            timezone_name: Some("America/New_York".to_string()),
            rule_metadata: CalendarRuleMetadataArtifact {
                holiday_calendar_code: "US_EQUITIES".to_string(),
                early_close_rule_set: Some("US_EQUITIES_EARLY_CLOSE".to_string()),
            },
            regular_sessions: vec![
                RegularSessionArtifact {
                    weekday: TradingWeekday::Monday,
                    windows: vec![SessionWindowArtifact {
                        label: "regular".to_string(),
                        open_time: "09:30".to_string(),
                        close_time: "16:00".to_string(),
                    }],
                },
                RegularSessionArtifact {
                    weekday: TradingWeekday::Tuesday,
                    windows: vec![SessionWindowArtifact {
                        label: "regular".to_string(),
                        open_time: "09:30".to_string(),
                        close_time: "16:00".to_string(),
                    }],
                },
                RegularSessionArtifact {
                    weekday: TradingWeekday::Wednesday,
                    windows: vec![
                        SessionWindowArtifact {
                            label: "pre_lunch".to_string(),
                            open_time: "09:30".to_string(),
                            close_time: "11:30".to_string(),
                        },
                        SessionWindowArtifact {
                            label: "post_lunch".to_string(),
                            open_time: "12:30".to_string(),
                            close_time: "16:00".to_string(),
                        },
                    ],
                },
            ],
        }
    }
}
