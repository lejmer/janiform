//! Exchange calendar domain model, resolver semantics, and artifact validation.
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
//!     holiday_dates: vec![],
//!     early_close_sessions: vec![],
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
//!     holiday_dates: vec![],
//!     early_close_sessions: vec![],
//! };
//!
//! let definition = ExchangeCalendarDefinition::from_artifact(artifact)?;
//! assert_eq!(definition.regular_sessions().len(), 1);
//! # Ok::<(), janiform_core::calendar::ExchangeCalendarValidationError>(())
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use chrono::{DateTime, Datelike, Timelike, Utc, Weekday as ChronoWeekday};
use chrono_tz::Tz;
use serde::Deserialize;

const CALENDAR_REGISTRY_SCHEMA_VERSION: &str = "calendar.registry.v1";
pub const TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION: &str = "seasonality.timestamp_event.v1";
pub const PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION: &str =
    "seasonality.timestamp_event.preprocessed.v1";
pub const MARKET_STATE_RESOLUTION_POLICY: &str = "resolve_market_state_at_utc.v1";
pub const SESSION_WINDOW_BOUNDARY_POLICY: &str = "open_inclusive_close_exclusive.v1";
pub use crate::seasonality::{
    DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION, DayOfWeekSeasonalityDiagnostics,
    DayOfWeekSeasonalityEstimateOutput, DayOfWeekSeasonalityEstimationError,
    DayOfWeekSeasonalityEstimatorSettings, DayOfYearSeasonalityDiagnostics,
    DayOfYearSeasonalityEstimateOutput, DayOfYearSeasonalityEstimationError,
    DayOfYearSeasonalityEstimatorSettings, DeterministicSeasonalityCompositionDiagnostics,
    DeterministicSeasonalityCompositionError, DeterministicSeasonalityCompositionOutput,
    INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION, IntradaySeasonalityDiagnostics,
    IntradaySeasonalityEstimateOutput, IntradaySeasonalityEstimationError,
    IntradaySeasonalityEstimatorSettings, IntradaySmoothingKernel,
    SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION,
    SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION, ScheduledEventOverlayDefinition,
    ScheduledEventOverlayProvenanceMetadata, SeasonalityDiagnosticsBatchReportOutput,
    SeasonalityDiagnosticsReportError, SeasonalityDiagnosticsReportInput,
    SeasonalityDiagnosticsReportOutput, compose_deterministic_seasonality_signal,
    estimate_day_of_week_seasonality, estimate_day_of_year_seasonality,
    estimate_intraday_seasonality, generate_seasonality_diagnostics_batch_report,
    generate_seasonality_diagnostics_report,
};

/// Weekday used by regular exchange sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
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

impl TradingWeekday {
    fn from_chrono_weekday(weekday: ChronoWeekday) -> Self {
        match weekday {
            ChronoWeekday::Mon => Self::Monday,
            ChronoWeekday::Tue => Self::Tuesday,
            ChronoWeekday::Wed => Self::Wednesday,
            ChronoWeekday::Thu => Self::Thursday,
            ChronoWeekday::Fri => Self::Friday,
            ChronoWeekday::Sat => Self::Saturday,
            ChronoWeekday::Sun => Self::Sunday,
        }
    }
}

/// Local exchange date used for holiday and early-close schedule keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct CalendarDate {
    year: i32,
    month: u8,
    day: u8,
}

impl CalendarDate {
    fn parse(raw: &str) -> Result<Self, &'static str> {
        let (year, month, day) = parse_iso_date_parts(raw)?;
        Ok(Self { year, month, day })
    }

    fn from_local_timestamp(timestamp_local: &DateTime<Tz>) -> Self {
        Self {
            year: timestamp_local.year(),
            month: timestamp_local.month() as u8,
            day: timestamp_local.day() as u8,
        }
    }
}

impl Display for CalendarDate {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

/// Canonical market state returned by calendar resolution for v0.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketState {
    Open,
    Closed,
}

impl Display for MarketState {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Open => "open",
            Self::Closed => "closed",
        };
        formatter.write_str(value)
    }
}

/// Session filter applied during timestamp event preprocessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampEventSessionFilter {
    All,
    OpenOnly,
    ClosedOnly,
}

impl TimestampEventSessionFilter {
    fn includes_market_state(self, market_state: MarketState) -> bool {
        match self {
            Self::All => true,
            Self::OpenOnly => market_state == MarketState::Open,
            Self::ClosedOnly => market_state == MarketState::Closed,
        }
    }
}

/// Canonical input record for seasonality timestamp preprocessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimestampEventInputRecord {
    pub source_record_position: u64,
    pub timestamp_utc: DateTime<Utc>,
}

/// Diagnostics emitted by timestamp preprocessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampEventPreprocessingDiagnostics {
    pub input_record_count: usize,
    pub validated_record_count: usize,
    pub accepted_record_count: usize,
    pub filtered_record_count: usize,
    pub first_event_timestamp_utc_nanos: Option<i64>,
    pub last_event_timestamp_utc_nanos: Option<i64>,
    pub hour_bucket_count: usize,
    pub missing_hour_bucket_count: usize,
    pub sparse_hour_bucket_count: usize,
}

/// Compact, deterministic preprocessing output for seasonality workflows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampEventPreprocessingOutput {
    pub schema_version: &'static str,
    pub input_schema_version: &'static str,
    pub exchange_code: String,
    pub exchange_timezone_name: String,
    pub source_record_positions: Vec<u64>,
    pub timestamp_utc_nanos: Vec<i64>,
    pub exchange_local_minute_of_day: Vec<u16>,
    pub exchange_local_day_of_week: Vec<u8>,
    pub exchange_local_day_of_year: Vec<u16>,
    pub market_state_codes: Vec<u8>,
    pub missing_hour_bucket_utc_nanos: Vec<i64>,
    pub sparse_hour_bucket_utc_nanos: Vec<i64>,
    pub diagnostics: TimestampEventPreprocessingDiagnostics,
}

/// Artifact structure loaded from versioned calendar configuration data.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeCalendarArtifact {
    pub schema_version: String,
    pub exchange_code: String,
    pub exchange_name: String,
    pub timezone_name: Option<String>,
    pub rule_metadata: CalendarRuleMetadataArtifact,
    pub regular_sessions: Vec<RegularSessionArtifact>,
    #[serde(default)]
    pub holiday_dates: Vec<String>,
    #[serde(default)]
    pub early_close_sessions: Vec<EarlyCloseSessionArtifact>,
}

/// Artifact metadata for holiday and early close rule resolution.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarRuleMetadataArtifact {
    pub holiday_calendar_code: String,
    pub early_close_rule_set: Option<String>,
}

/// Regular session windows for a weekday in artifact form.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegularSessionArtifact {
    pub weekday: TradingWeekday,
    pub windows: Vec<SessionWindowArtifact>,
}

/// Session window in artifact form.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionWindowArtifact {
    pub label: String,
    pub open_time: String,
    pub close_time: String,
}

/// Truncated session windows for one explicit early-close date.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EarlyCloseSessionArtifact {
    pub date: String,
    pub windows: Vec<SessionWindowArtifact>,
}

/// Immutable exchange calendar definition used by core calendar services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeCalendarDefinition {
    schema_version: String,
    exchange_metadata: ExchangeMetadata,
    timezone_name: String,
    rule_metadata: CalendarRuleMetadata,
    regular_sessions: Box<[RegularSessionDefinition]>,
    holiday_dates: BTreeSet<CalendarDate>,
    early_close_sessions_by_date: BTreeMap<CalendarDate, Box<[SessionWindowDefinition]>>,
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
        let early_close_rule_set =
            normalize_optional_string(artifact.rule_metadata.early_close_rule_set);

        if artifact.regular_sessions.is_empty() {
            return Err(ExchangeCalendarValidationError::MissingField {
                field: "regular_sessions",
            });
        }

        let mut grouped_windows: BTreeMap<TradingWeekday, Vec<SessionWindowDefinition>> =
            BTreeMap::new();

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
                let close_time =
                    SessionTime::parse(&window.close_time).map_err(|parse_reason| {
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
                    return Err(
                        ExchangeCalendarValidationError::OverlappingRegularSessions {
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
                        },
                    );
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
        let holiday_dates = build_holiday_dates(artifact.holiday_dates)?;
        let early_close_sessions_by_date =
            build_early_close_sessions(artifact.early_close_sessions)?;

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
            holiday_dates,
            early_close_sessions_by_date,
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

    fn resolve_market_state_for_local_timestamp(
        &self,
        local_date: CalendarDate,
        weekday: TradingWeekday,
        local_clock: SessionTime,
    ) -> MarketState {
        // Rule precedence is deterministic and centralized
        // 1. Holidays always resolve closed
        // 2. Early-close windows override regular sessions for the same date
        // 3. Regular sessions apply for all remaining dates
        if self.holiday_dates.contains(&local_date) {
            return MarketState::Closed;
        }

        if let Some(early_close_windows) = self.early_close_sessions_by_date.get(&local_date) {
            return resolve_market_state_for_session_windows(early_close_windows, local_clock);
        }

        self.resolve_regular_market_state_for_local_clock(weekday, local_clock)
    }

    fn resolve_regular_market_state_for_local_clock(
        &self,
        weekday: TradingWeekday,
        local_clock: SessionTime,
    ) -> MarketState {
        let Some(regular_session) = self
            .regular_sessions
            .iter()
            .find(|definition| definition.weekday == weekday)
        else {
            return MarketState::Closed;
        };

        resolve_market_state_for_session_windows(&regular_session.windows, local_clock)
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

    fn contains_local_clock(&self, local_clock: SessionTime) -> bool {
        // Boundary policy is [open, close): exact open resolves open and exact close resolves closed.
        local_clock >= self.open_time && local_clock < self.close_time
    }
}

fn resolve_market_state_for_session_windows(
    windows: &[SessionWindowDefinition],
    local_clock: SessionTime,
) -> MarketState {
    if windows
        .iter()
        .any(|window| window.contains_local_clock(local_clock))
    {
        return MarketState::Open;
    }

    MarketState::Closed
}

/// Clock time represented in local exchange time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionTime {
    hour: u8,
    minute: u8,
}

impl SessionTime {
    fn from_hour_minute(hour: u8, minute: u8) -> Self {
        Self { hour, minute }
    }

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

/// Versioned artifact containing exchange calendar entries for registry construction.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeCalendarRegistryArtifact {
    pub schema_version: String,
    pub artifact_date: String,
    pub exchanges: Vec<ExchangeCalendarArtifact>,
}

/// Immutable in-memory calendar registry keyed by exchange code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeCalendarRegistry {
    definitions_by_exchange: BTreeMap<String, ExchangeCalendarDefinition>,
}

impl ExchangeCalendarRegistry {
    /// Build a registry from a parsed registry artifact.
    pub fn from_artifact(
        artifact: ExchangeCalendarRegistryArtifact,
    ) -> Result<Self, ExchangeCalendarRegistryLoadError> {
        let schema_version = require_non_empty(artifact.schema_version, "schema_version")
            .map_err(ExchangeCalendarRegistryLoadError::from_validation_error)?;
        if schema_version != CALENDAR_REGISTRY_SCHEMA_VERSION {
            return Err(
                ExchangeCalendarRegistryLoadError::UnsupportedSchemaVersion {
                    expected: CALENDAR_REGISTRY_SCHEMA_VERSION,
                    actual: schema_version,
                },
            );
        }

        if artifact.exchanges.is_empty() {
            return Err(ExchangeCalendarRegistryLoadError::MissingField { field: "exchanges" });
        }

        validate_iso_date(&artifact.artifact_date).map_err(|reason| {
            ExchangeCalendarRegistryLoadError::MalformedDate {
                field: "artifact_date",
                value: artifact.artifact_date.clone(),
                reason,
            }
        })?;

        let mut definitions_by_exchange: BTreeMap<String, ExchangeCalendarDefinition> =
            BTreeMap::new();

        for exchange_artifact in artifact.exchanges {
            let exchange_code = exchange_artifact.exchange_code.trim().to_string();
            let definition =
                ExchangeCalendarDefinition::from_artifact(exchange_artifact).map_err(|source| {
                    ExchangeCalendarRegistryLoadError::InvalidExchangeArtifact {
                        exchange_code: (!exchange_code.is_empty()).then_some(exchange_code.clone()),
                        source,
                    }
                })?;
            if definitions_by_exchange
                .insert(exchange_code.clone(), definition)
                .is_some()
            {
                return Err(ExchangeCalendarRegistryLoadError::DuplicateExchangeId {
                    exchange_code,
                });
            }
        }

        Ok(Self {
            definitions_by_exchange,
        })
    }

    /// Lookup a loaded exchange by identifier.
    pub fn get(&self, exchange_code: &str) -> Option<&ExchangeCalendarDefinition> {
        self.definitions_by_exchange.get(exchange_code)
    }

    /// Lookup a loaded exchange by identifier with a structured error when missing.
    pub fn get_required(
        &self,
        exchange_code: &str,
    ) -> Result<&ExchangeCalendarDefinition, ExchangeCalendarRegistryLookupError> {
        self.get(exchange_code).ok_or_else(|| {
            ExchangeCalendarRegistryLookupError::MissingExchangeId {
                exchange_code: exchange_code.to_string(),
            }
        })
    }

    /// Resolve market state for a UTC timestamp and exchange identifier.
    ///
    /// Boundary policy:
    /// - Session windows are interpreted as half-open intervals `[open, close)`.
    /// - Exact session open timestamp resolves to `MarketState::Open`.
    /// - Exact session close timestamp resolves to `MarketState::Closed`.
    /// Rule precedence:
    /// - Holidays resolve `MarketState::Closed` for the full local exchange date.
    /// - Early-close windows override regular sessions when configured for a local date.
    /// - Regular sessions apply when neither holiday nor early-close rule matches.
    pub fn resolve_market_state_at_utc(
        &self,
        exchange_code: &str,
        timestamp_utc: DateTime<Utc>,
    ) -> Result<MarketState, ExchangeCalendarResolveError> {
        let (definition, timezone) = self.required_definition_and_timezone(exchange_code)?;

        Ok(resolve_market_state_for_definition_at_utc(
            definition,
            timezone,
            timestamp_utc,
        ))
    }

    /// Preprocess timestamp events for deterministic seasonality workflows.
    pub fn preprocess_timestamp_events(
        &self,
        exchange_code: &str,
        input_records: Vec<TimestampEventInputRecord>,
        session_filter: TimestampEventSessionFilter,
        sparse_hour_event_threshold: usize,
    ) -> Result<TimestampEventPreprocessingOutput, TimestampEventPreprocessingError> {
        if sparse_hour_event_threshold < 1 {
            return Err(
                TimestampEventPreprocessingError::InvalidSparseHourEventThreshold {
                    sparse_hour_event_threshold,
                },
            );
        }

        let (definition, timezone) = self
            .required_definition_and_timezone(exchange_code)
            .map_err(TimestampEventPreprocessingError::from_calendar_resolve_error)?;
        let exchange_timezone_name = definition.timezone_name().to_string();

        let input_record_count = input_records.len();
        let validated_record_count = input_record_count;
        let mut accepted_rows: Vec<AcceptedPreprocessedTimestampEventRow> = Vec::new();

        for input_record in input_records {
            let timestamp_utc_nanos = input_record
                .timestamp_utc
                .timestamp_nanos_opt()
                .ok_or_else(|| TimestampEventPreprocessingError::TimestampOutOfRange {
                    source_record_position: input_record.source_record_position,
                })?;
            let timestamp_local = input_record.timestamp_utc.with_timezone(&timezone);
            let market_state =
                resolve_market_state_for_definition_at_local(definition, timestamp_local);

            if !session_filter.includes_market_state(market_state) {
                continue;
            }

            accepted_rows.push(AcceptedPreprocessedTimestampEventRow {
                source_record_position: input_record.source_record_position,
                timestamp_utc_nanos,
                exchange_local_minute_of_day: (timestamp_local.hour() * 60
                    + timestamp_local.minute())
                    as u16,
                exchange_local_day_of_week: timestamp_local.weekday().number_from_monday() as u8,
                exchange_local_day_of_year: timestamp_local.ordinal() as u16,
                market_state_code: market_state_to_code(market_state),
            });
        }

        accepted_rows.sort_by(|left_row, right_row| {
            left_row
                .timestamp_utc_nanos
                .cmp(&right_row.timestamp_utc_nanos)
                .then(
                    left_row
                        .source_record_position
                        .cmp(&right_row.source_record_position),
                )
        });

        let accepted_record_count = accepted_rows.len();
        let filtered_record_count = validated_record_count.saturating_sub(accepted_record_count);

        let mut source_record_positions: Vec<u64> = Vec::with_capacity(accepted_record_count);
        let mut timestamp_utc_nanos: Vec<i64> = Vec::with_capacity(accepted_record_count);
        let mut exchange_local_minute_of_day: Vec<u16> = Vec::with_capacity(accepted_record_count);
        let mut exchange_local_day_of_week: Vec<u8> = Vec::with_capacity(accepted_record_count);
        let mut exchange_local_day_of_year: Vec<u16> = Vec::with_capacity(accepted_record_count);
        let mut market_state_codes: Vec<u8> = Vec::with_capacity(accepted_record_count);

        for accepted_row in accepted_rows {
            source_record_positions.push(accepted_row.source_record_position);
            timestamp_utc_nanos.push(accepted_row.timestamp_utc_nanos);
            exchange_local_minute_of_day.push(accepted_row.exchange_local_minute_of_day);
            exchange_local_day_of_week.push(accepted_row.exchange_local_day_of_week);
            exchange_local_day_of_year.push(accepted_row.exchange_local_day_of_year);
            market_state_codes.push(accepted_row.market_state_code);
        }

        let (
            missing_hour_bucket_utc_nanos,
            sparse_hour_bucket_utc_nanos,
            first_event_timestamp_utc_nanos,
            last_event_timestamp_utc_nanos,
            hour_bucket_count,
        ) = build_hour_bucket_diagnostics(&timestamp_utc_nanos, sparse_hour_event_threshold);

        let diagnostics = TimestampEventPreprocessingDiagnostics {
            input_record_count,
            validated_record_count,
            accepted_record_count,
            filtered_record_count,
            first_event_timestamp_utc_nanos,
            last_event_timestamp_utc_nanos,
            hour_bucket_count,
            missing_hour_bucket_count: missing_hour_bucket_utc_nanos.len(),
            sparse_hour_bucket_count: sparse_hour_bucket_utc_nanos.len(),
        };

        Ok(TimestampEventPreprocessingOutput {
            schema_version: PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION,
            input_schema_version: TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION,
            exchange_code: exchange_code.to_string(),
            exchange_timezone_name,
            source_record_positions,
            timestamp_utc_nanos,
            exchange_local_minute_of_day,
            exchange_local_day_of_week,
            exchange_local_day_of_year,
            market_state_codes,
            missing_hour_bucket_utc_nanos,
            sparse_hour_bucket_utc_nanos,
            diagnostics,
        })
    }

    fn required_definition_and_timezone(
        &self,
        exchange_code: &str,
    ) -> Result<(&ExchangeCalendarDefinition, Tz), ExchangeCalendarResolveError> {
        let definition = self
            .get_required(exchange_code)
            .map_err(ExchangeCalendarResolveError::from_lookup_error)?;
        let timezone_name = definition.timezone_name();
        let timezone = Tz::from_str(timezone_name).map_err(|_| {
            ExchangeCalendarResolveError::InvalidTimezoneName {
                exchange_code: exchange_code.to_string(),
                timezone_name: timezone_name.to_string(),
            }
        })?;

        Ok((definition, timezone))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AcceptedPreprocessedTimestampEventRow {
    source_record_position: u64,
    timestamp_utc_nanos: i64,
    exchange_local_minute_of_day: u16,
    exchange_local_day_of_week: u8,
    exchange_local_day_of_year: u16,
    market_state_code: u8,
}

const OPEN_MARKET_STATE_CODE: u8 = 1;
const CLOSED_MARKET_STATE_CODE: u8 = 0;
const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;

fn market_state_to_code(market_state: MarketState) -> u8 {
    match market_state {
        MarketState::Open => OPEN_MARKET_STATE_CODE,
        MarketState::Closed => CLOSED_MARKET_STATE_CODE,
    }
}

fn resolve_market_state_for_definition_at_utc(
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    timestamp_utc: DateTime<Utc>,
) -> MarketState {
    let timestamp_local = timestamp_utc.with_timezone(&timezone);
    resolve_market_state_for_definition_at_local(definition, timestamp_local)
}

fn resolve_market_state_for_definition_at_local(
    definition: &ExchangeCalendarDefinition,
    timestamp_local: DateTime<Tz>,
) -> MarketState {
    let local_date = CalendarDate::from_local_timestamp(&timestamp_local);
    let local_weekday = TradingWeekday::from_chrono_weekday(timestamp_local.weekday());
    let local_clock =
        SessionTime::from_hour_minute(timestamp_local.hour() as u8, timestamp_local.minute() as u8);

    definition.resolve_market_state_for_local_timestamp(local_date, local_weekday, local_clock)
}

fn floor_to_hour_utc_nanos(timestamp_utc_nanos: i64) -> i64 {
    timestamp_utc_nanos.div_euclid(ONE_HOUR_NANOS) * ONE_HOUR_NANOS
}

fn build_hour_bucket_diagnostics(
    timestamp_utc_nanos: &[i64],
    sparse_hour_event_threshold: usize,
) -> (Vec<i64>, Vec<i64>, Option<i64>, Option<i64>, usize) {
    if timestamp_utc_nanos.is_empty() {
        return (Vec::new(), Vec::new(), None, None, 0);
    }

    let first_event_timestamp_utc_nanos = timestamp_utc_nanos.first().copied();
    let last_event_timestamp_utc_nanos = timestamp_utc_nanos.last().copied();

    let mut observed_bucket_counts: BTreeMap<i64, usize> = BTreeMap::new();
    for timestamp_utc_nanos in timestamp_utc_nanos {
        let hour_bucket_utc_nanos = floor_to_hour_utc_nanos(*timestamp_utc_nanos);
        *observed_bucket_counts
            .entry(hour_bucket_utc_nanos)
            .or_insert(0) += 1;
    }

    let (first_hour_bucket_utc_nanos, last_hour_bucket_utc_nanos) = match (
        first_event_timestamp_utc_nanos,
        last_event_timestamp_utc_nanos,
    ) {
        (Some(first_timestamp_utc_nanos), Some(last_timestamp_utc_nanos)) => (
            floor_to_hour_utc_nanos(first_timestamp_utc_nanos),
            floor_to_hour_utc_nanos(last_timestamp_utc_nanos),
        ),
        _ => {
            return (Vec::new(), Vec::new(), None, None, 0);
        }
    };

    let mut missing_hour_bucket_utc_nanos: Vec<i64> = Vec::new();
    let mut sparse_hour_bucket_utc_nanos: Vec<i64> = Vec::new();
    let mut hour_bucket_count: usize = 0;
    let mut current_bucket_utc_nanos = first_hour_bucket_utc_nanos;

    while current_bucket_utc_nanos <= last_hour_bucket_utc_nanos {
        hour_bucket_count += 1;

        let observed_count = observed_bucket_counts
            .get(&current_bucket_utc_nanos)
            .copied()
            .unwrap_or(0);

        if observed_count == 0 {
            missing_hour_bucket_utc_nanos.push(current_bucket_utc_nanos);
        } else if observed_count < sparse_hour_event_threshold {
            sparse_hour_bucket_utc_nanos.push(current_bucket_utc_nanos);
        }

        current_bucket_utc_nanos += ONE_HOUR_NANOS;
    }

    (
        missing_hour_bucket_utc_nanos,
        sparse_hour_bucket_utc_nanos,
        first_event_timestamp_utc_nanos,
        last_event_timestamp_utc_nanos,
        hour_bucket_count,
    )
}

/// Loader for parsing exchange calendar registry artifacts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ExchangeCalendarRegistryLoader;

impl ExchangeCalendarRegistryLoader {
    /// Parse and load a registry from JSON artifact content.
    pub fn load_from_json(
        artifact_json: &str,
    ) -> Result<ExchangeCalendarRegistry, ExchangeCalendarRegistryLoadError> {
        let artifact = serde_json::from_str::<ExchangeCalendarRegistryArtifact>(artifact_json)
            .map_err(ExchangeCalendarRegistryLoadError::from_deserialization_error)?;
        ExchangeCalendarRegistry::from_artifact(artifact)
    }
}

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
    fn from_lookup_error(source: ExchangeCalendarRegistryLookupError) -> Self {
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
    fn from_calendar_resolve_error(source: ExchangeCalendarResolveError) -> Self {
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
    fn from_validation_error(source: ExchangeCalendarValidationError) -> Self {
        match source {
            ExchangeCalendarValidationError::MissingField { field } => Self::MissingField { field },
            _ => Self::InvalidExchangeArtifact {
                exchange_code: None,
                source,
            },
        }
    }

    fn from_deserialization_error(source: serde_json::Error) -> Self {
        let message = source.to_string();
        if let Some(field_name) = parse_unknown_field_name(&message) {
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

fn build_holiday_dates(
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

fn build_early_close_sessions(
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

fn parse_unknown_field_name(error_message: &str) -> Option<String> {
    let unknown_field_prefix = "unknown field `";
    let after_prefix = error_message.split_once(unknown_field_prefix)?.1;
    let field_name = after_prefix.split('`').next()?;

    Some(field_name.to_string())
}

fn validate_iso_date(raw: &str) -> Result<(), &'static str> {
    parse_iso_date_parts(raw).map(|_| ())
}

fn parse_iso_date_parts(raw: &str) -> Result<(i32, u8, u8), &'static str> {
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

#[cfg(test)]
mod tests {
    use super::{
        CalendarRuleMetadataArtifact, EarlyCloseSessionArtifact, ExchangeCalendarArtifact,
        ExchangeCalendarDefinition, ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact,
        ExchangeCalendarRegistryLoadError, ExchangeCalendarRegistryLoader,
        ExchangeCalendarRegistryLookupError, ExchangeCalendarResolveError,
        ExchangeCalendarValidationError, MarketState, RegularSessionArtifact,
        SessionWindowArtifact, TimestampEventInputRecord, TimestampEventPreprocessingError,
        TimestampEventSessionFilter, TradingWeekday,
    };
    use chrono::{DateTime, TimeZone, Utc};

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
            holiday_dates: vec![],
            early_close_sessions: vec![],
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
            holiday_dates: vec![],
            early_close_sessions: vec![],
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

    #[test]
    fn loader_ingests_registry_with_multiple_exchanges() {
        let artifact_json = r#"
        {
            "schema_version": "calendar.registry.v1",
            "artifact_date": "2026-01-31",
            "exchanges": [
                {
                    "schema_version": "calendar.v1",
                    "exchange_code": "XNYS",
                    "exchange_name": "New York Stock Exchange",
                    "timezone_name": "America/New_York",
                    "rule_metadata": {
                        "holiday_calendar_code": "US_EQUITIES",
                        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
                    },
                    "regular_sessions": [
                        {
                            "weekday": "Monday",
                            "windows": [
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ]
                },
                {
                    "schema_version": "calendar.v1",
                    "exchange_code": "XTKS",
                    "exchange_name": "Tokyo Stock Exchange",
                    "timezone_name": "Asia/Tokyo",
                    "rule_metadata": {
                        "holiday_calendar_code": "JP_EQUITIES",
                        "early_close_rule_set": null
                    },
                    "regular_sessions": [
                        {
                            "weekday": "Monday",
                            "windows": [
                                {
                                    "label": "morning",
                                    "open_time": "09:00",
                                    "close_time": "11:30"
                                },
                                {
                                    "label": "afternoon",
                                    "open_time": "12:30",
                                    "close_time": "15:00"
                                }
                            ]
                        }
                    ]
                }
            ]
        }
        "#;

        let registry = ExchangeCalendarRegistryLoader::load_from_json(artifact_json)
            .expect("valid registry artifact should load");
        let xnys = registry
            .get_required("XNYS")
            .expect("XNYS must be present in registry");
        let xtks = registry
            .get_required("XTKS")
            .expect("XTKS must be present in registry");

        assert_eq!(xnys.timezone_name(), "America/New_York");
        assert_eq!(xtks.timezone_name(), "Asia/Tokyo");
    }

    #[test]
    fn loader_ingests_holiday_and_early_close_schedule_fields() {
        let artifact_json = r#"
        {
            "schema_version": "calendar.registry.v1",
            "artifact_date": "2026-01-31",
            "exchanges": [
                {
                    "schema_version": "calendar.v1",
                    "exchange_code": "XNYS",
                    "exchange_name": "New York Stock Exchange",
                    "timezone_name": "America/New_York",
                    "rule_metadata": {
                        "holiday_calendar_code": "US_EQUITIES",
                        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
                    },
                    "regular_sessions": [
                        {
                            "weekday": "Tuesday",
                            "windows": [
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        },
                        {
                            "weekday": "Wednesday",
                            "windows": [
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ],
                    "holiday_dates": ["2026-02-03"],
                    "early_close_sessions": [
                        {
                            "date": "2026-02-04",
                            "windows": [
                                {
                                    "label": "early_close",
                                    "open_time": "09:30",
                                    "close_time": "13:00"
                                }
                            ]
                        }
                    ]
                }
            ]
        }
        "#;

        let registry = ExchangeCalendarRegistryLoader::load_from_json(artifact_json)
            .expect("registry with holiday and early-close fields should load");
        let holiday_timestamp = timestamp_utc(2026, 2, 3, 15, 0, 0);
        let early_close_boundary = timestamp_utc(2026, 2, 4, 18, 0, 0);

        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", holiday_timestamp),
            Ok(MarketState::Closed)
        );
        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", early_close_boundary),
            Ok(MarketState::Closed)
        );
    }

    #[test]
    fn loader_rejects_unknown_fields() {
        let artifact_json = r#"
        {
            "schema_version": "calendar.registry.v1",
            "artifact_date": "2026-01-31",
            "unexpected_field": true,
            "exchanges": []
        }
        "#;

        let result = ExchangeCalendarRegistryLoader::load_from_json(artifact_json);
        assert_eq!(
            result,
            Err(ExchangeCalendarRegistryLoadError::UnknownField {
                field: "unexpected_field".to_string(),
            })
        );
    }

    #[test]
    fn loader_rejects_malformed_dates() {
        let artifact_json = r#"
        {
            "schema_version": "calendar.registry.v1",
            "artifact_date": "2026-13-01",
            "exchanges": [
                {
                    "schema_version": "calendar.v1",
                    "exchange_code": "XNYS",
                    "exchange_name": "New York Stock Exchange",
                    "timezone_name": "America/New_York",
                    "rule_metadata": {
                        "holiday_calendar_code": "US_EQUITIES",
                        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
                    },
                    "regular_sessions": [
                        {
                            "weekday": "Monday",
                            "windows": [
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ]
                }
            ]
        }
        "#;

        let result = ExchangeCalendarRegistryLoader::load_from_json(artifact_json);
        match result {
            Err(ExchangeCalendarRegistryLoadError::MalformedDate {
                field,
                value,
                reason,
            }) => {
                assert_eq!(field, "artifact_date");
                assert_eq!(value, "2026-13-01");
                assert_eq!(reason, "month must be between 01 and 12");
            }
            _ => panic!("expected malformed date error"),
        }
    }

    #[test]
    fn loader_rejects_duplicate_exchange_ids() {
        let artifact_json = r#"
        {
            "schema_version": "calendar.registry.v1",
            "artifact_date": "2026-01-31",
            "exchanges": [
                {
                    "schema_version": "calendar.v1",
                    "exchange_code": "XNYS",
                    "exchange_name": "New York Stock Exchange",
                    "timezone_name": "America/New_York",
                    "rule_metadata": {
                        "holiday_calendar_code": "US_EQUITIES",
                        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
                    },
                    "regular_sessions": [
                        {
                            "weekday": "Monday",
                            "windows": [
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ]
                },
                {
                    "schema_version": "calendar.v1",
                    "exchange_code": "XNYS",
                    "exchange_name": "New York Stock Exchange Copy",
                    "timezone_name": "America/New_York",
                    "rule_metadata": {
                        "holiday_calendar_code": "US_EQUITIES",
                        "early_close_rule_set": null
                    },
                    "regular_sessions": [
                        {
                            "weekday": "Tuesday",
                            "windows": [
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ]
                }
            ]
        }
        "#;

        let result = ExchangeCalendarRegistryLoader::load_from_json(artifact_json);
        assert_eq!(
            result,
            Err(ExchangeCalendarRegistryLoadError::DuplicateExchangeId {
                exchange_code: "XNYS".to_string(),
            })
        );
    }

    #[test]
    fn registry_lookup_returns_structured_error_for_missing_exchange() {
        let artifact_json = r#"
        {
            "schema_version": "calendar.registry.v1",
            "artifact_date": "2026-01-31",
            "exchanges": [
                {
                    "schema_version": "calendar.v1",
                    "exchange_code": "XNYS",
                    "exchange_name": "New York Stock Exchange",
                    "timezone_name": "America/New_York",
                    "rule_metadata": {
                        "holiday_calendar_code": "US_EQUITIES",
                        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
                    },
                    "regular_sessions": [
                        {
                            "weekday": "Monday",
                            "windows": [
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ]
                }
            ]
        }
        "#;

        let registry = ExchangeCalendarRegistryLoader::load_from_json(artifact_json)
            .expect("valid artifact should load");

        assert_eq!(registry.get("MISSING"), None);
        assert_eq!(
            registry.get_required("MISSING"),
            Err(ExchangeCalendarRegistryLookupError::MissingExchangeId {
                exchange_code: "MISSING".to_string(),
            })
        );
    }

    #[test]
    fn resolver_returns_expected_boundaries_for_regular_session() {
        let registry = resolver_test_registry();

        let pre_open = timestamp_utc(2026, 2, 2, 14, 29, 59);
        let at_open = timestamp_utc(2026, 2, 2, 14, 30, 0);
        let in_session = timestamp_utc(2026, 2, 2, 20, 59, 59);
        let at_close = timestamp_utc(2026, 2, 2, 21, 0, 0);
        let post_close = timestamp_utc(2026, 2, 2, 21, 0, 1);

        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", pre_open),
            Ok(MarketState::Closed)
        );
        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", at_open),
            Ok(MarketState::Open)
        );
        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", in_session),
            Ok(MarketState::Open)
        );
        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", at_close),
            Ok(MarketState::Closed)
        );
        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", post_close),
            Ok(MarketState::Closed)
        );
    }

    #[test]
    fn resolver_returns_closed_on_weekend_timestamp() {
        let registry = resolver_test_registry();
        let weekend_timestamp = timestamp_utc(2026, 2, 1, 3, 0, 0);

        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", weekend_timestamp),
            Ok(MarketState::Closed)
        );
    }

    #[test]
    fn resolver_returns_closed_for_holiday_during_regular_weekday_session() {
        let mut artifact = xnys_full_week_artifact();
        artifact.holiday_dates = vec!["2026-02-03".to_string()];

        let registry = single_exchange_registry(artifact);
        let holiday_timestamp = timestamp_utc(2026, 2, 3, 15, 0, 0);

        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", holiday_timestamp),
            Ok(MarketState::Closed)
        );
    }

    #[test]
    fn resolver_uses_truncated_early_close_window_for_market_state_transitions() {
        let mut artifact = xnys_full_week_artifact();
        artifact.early_close_sessions = vec![EarlyCloseSessionArtifact {
            date: "2026-02-04".to_string(),
            windows: vec![SessionWindowArtifact {
                label: "early_close".to_string(),
                open_time: "09:30".to_string(),
                close_time: "13:00".to_string(),
            }],
        }];

        let registry = single_exchange_registry(artifact);
        let before_early_close = timestamp_utc(2026, 2, 4, 17, 59, 59);
        let at_early_close = timestamp_utc(2026, 2, 4, 18, 0, 0);
        let after_early_close = timestamp_utc(2026, 2, 4, 18, 0, 1);

        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", before_early_close),
            Ok(MarketState::Open)
        );
        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", at_early_close),
            Ok(MarketState::Closed)
        );
        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", after_early_close),
            Ok(MarketState::Closed)
        );
    }

    #[test]
    fn resolver_applies_holiday_precedence_over_early_close_schedule() {
        let mut artifact = xnys_full_week_artifact();
        artifact.holiday_dates = vec!["2026-02-05".to_string()];
        artifact.early_close_sessions = vec![EarlyCloseSessionArtifact {
            date: "2026-02-05".to_string(),
            windows: vec![SessionWindowArtifact {
                label: "early_close".to_string(),
                open_time: "09:30".to_string(),
                close_time: "13:00".to_string(),
            }],
        }];

        let registry = single_exchange_registry(artifact);
        let timestamp_inside_early_close_window = timestamp_utc(2026, 2, 5, 16, 0, 0);

        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", timestamp_inside_early_close_window),
            Ok(MarketState::Closed)
        );
    }

    #[test]
    fn resolver_returns_structured_error_for_missing_exchange_id() {
        let registry = resolver_test_registry();
        let timestamp = timestamp_utc(2026, 2, 2, 14, 30, 0);

        assert_eq!(
            registry.resolve_market_state_at_utc("MISSING", timestamp),
            Err(ExchangeCalendarResolveError::MissingExchangeId {
                exchange_code: "MISSING".to_string(),
            })
        );
    }

    #[test]
    fn resolver_uses_exchange_timezone_and_cross_day_local_date() {
        let registry = resolver_test_registry();
        let same_utc_timestamp = timestamp_utc(2026, 2, 2, 0, 30, 0);

        assert_eq!(
            registry.resolve_market_state_at_utc("XNYS", same_utc_timestamp),
            Ok(MarketState::Closed)
        );
        assert_eq!(
            registry.resolve_market_state_at_utc("XTKS", same_utc_timestamp),
            Ok(MarketState::Open)
        );
    }

    #[test]
    fn resolver_handles_cross_day_transition_for_ahead_timezone_exchange() {
        let registry = resolver_test_registry();

        let before_utc_midnight = timestamp_utc(2026, 2, 2, 23, 59, 0);
        let after_utc_midnight = timestamp_utc(2026, 2, 3, 0, 30, 0);

        assert_eq!(
            registry.resolve_market_state_at_utc("XTKS", before_utc_midnight),
            Ok(MarketState::Closed)
        );
        assert_eq!(
            registry.resolve_market_state_at_utc("XTKS", after_utc_midnight),
            Ok(MarketState::Open)
        );
    }

    #[test]
    fn resolver_returns_structured_error_for_invalid_timezone_name() {
        let artifact = ExchangeCalendarRegistryArtifact {
            schema_version: "calendar.registry.v1".to_string(),
            artifact_date: "2026-01-31".to_string(),
            exchanges: vec![ExchangeCalendarArtifact {
                schema_version: "calendar.v1".to_string(),
                exchange_code: "XBAD".to_string(),
                exchange_name: "Broken Exchange".to_string(),
                timezone_name: Some("Not/A_Real_Timezone".to_string()),
                rule_metadata: CalendarRuleMetadataArtifact {
                    holiday_calendar_code: "BROKEN".to_string(),
                    early_close_rule_set: None,
                },
                regular_sessions: full_week_regular_sessions("09:30", "16:00"),
                holiday_dates: vec![],
                early_close_sessions: vec![],
            }],
        };

        let registry = ExchangeCalendarRegistry::from_artifact(artifact)
            .expect("registry artifact should validate before timezone resolution");
        let timestamp = timestamp_utc(2026, 2, 2, 14, 30, 0);

        assert_eq!(
            registry.resolve_market_state_at_utc("XBAD", timestamp),
            Err(ExchangeCalendarResolveError::InvalidTimezoneName {
                exchange_code: "XBAD".to_string(),
                timezone_name: "Not/A_Real_Timezone".to_string(),
            })
        );
    }

    #[test]
    fn preprocessing_applies_session_filter_and_local_context() {
        let registry = resolver_test_registry();

        let input_records = vec![
            timestamp_input_record(0, 2026, 2, 2, 14, 30, 0),
            timestamp_input_record(1, 2026, 2, 2, 3, 0, 0),
        ];

        let output = registry
            .preprocess_timestamp_events(
                "XNYS",
                input_records,
                TimestampEventSessionFilter::OpenOnly,
                1,
            )
            .expect("preprocessing should succeed");

        assert_eq!(output.exchange_timezone_name, "America/New_York");
        assert_eq!(output.source_record_positions, vec![0]);
        assert_eq!(
            output.timestamp_utc_nanos,
            vec![
                timestamp_utc(2026, 2, 2, 14, 30, 0)
                    .timestamp_nanos_opt()
                    .expect("fixture timestamp should be in nanos range")
            ]
        );
        assert_eq!(output.exchange_local_minute_of_day, vec![570]);
        assert_eq!(output.exchange_local_day_of_week, vec![1]);
        assert_eq!(output.market_state_codes, vec![1]);
        assert_eq!(output.diagnostics.accepted_record_count, 1);
        assert_eq!(output.diagnostics.filtered_record_count, 1);
    }

    #[test]
    fn preprocessing_is_deterministic_and_emits_sparse_hour_diagnostics() {
        let registry = resolver_test_registry();

        let input_records = vec![
            timestamp_input_record(0, 2026, 2, 2, 16, 30, 0),
            timestamp_input_record(1, 2026, 2, 2, 14, 30, 0),
            timestamp_input_record(2, 2026, 2, 2, 16, 31, 0),
        ];

        let first_output = registry
            .preprocess_timestamp_events(
                "XNYS",
                input_records.clone(),
                TimestampEventSessionFilter::All,
                2,
            )
            .expect("preprocessing should succeed");
        let second_output = registry
            .preprocess_timestamp_events("XNYS", input_records, TimestampEventSessionFilter::All, 2)
            .expect("preprocessing should succeed");

        assert_eq!(first_output, second_output);
        assert_eq!(first_output.source_record_positions, vec![1, 0, 2]);
        assert_eq!(
            first_output.missing_hour_bucket_utc_nanos,
            vec![
                timestamp_utc(2026, 2, 2, 15, 0, 0)
                    .timestamp_nanos_opt()
                    .expect("fixture timestamp should be in nanos range")
            ]
        );
        assert_eq!(
            first_output.sparse_hour_bucket_utc_nanos,
            vec![
                timestamp_utc(2026, 2, 2, 14, 0, 0)
                    .timestamp_nanos_opt()
                    .expect("fixture timestamp should be in nanos range")
            ]
        );
    }

    #[test]
    fn preprocessing_rejects_invalid_sparse_hour_threshold() {
        let registry = resolver_test_registry();

        let result = registry.preprocess_timestamp_events(
            "XNYS",
            vec![timestamp_input_record(0, 2026, 2, 2, 14, 30, 0)],
            TimestampEventSessionFilter::All,
            0,
        );

        assert_eq!(
            result,
            Err(
                TimestampEventPreprocessingError::InvalidSparseHourEventThreshold {
                    sparse_hour_event_threshold: 0,
                },
            ),
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
            holiday_dates: vec![],
            early_close_sessions: vec![],
        }
    }

    fn resolver_test_registry() -> ExchangeCalendarRegistry {
        let artifact = ExchangeCalendarRegistryArtifact {
            schema_version: "calendar.registry.v1".to_string(),
            artifact_date: "2026-01-31".to_string(),
            exchanges: vec![xnys_full_week_artifact(), xtks_full_week_artifact()],
        };

        ExchangeCalendarRegistry::from_artifact(artifact)
            .expect("resolver test registry artifact should be valid")
    }

    fn single_exchange_registry(
        exchange_artifact: ExchangeCalendarArtifact,
    ) -> ExchangeCalendarRegistry {
        let artifact = ExchangeCalendarRegistryArtifact {
            schema_version: "calendar.registry.v1".to_string(),
            artifact_date: "2026-01-31".to_string(),
            exchanges: vec![exchange_artifact],
        };

        ExchangeCalendarRegistry::from_artifact(artifact)
            .expect("single-exchange registry artifact should be valid")
    }

    fn xnys_full_week_artifact() -> ExchangeCalendarArtifact {
        ExchangeCalendarArtifact {
            schema_version: "calendar.v1".to_string(),
            exchange_code: "XNYS".to_string(),
            exchange_name: "New York Stock Exchange".to_string(),
            timezone_name: Some("America/New_York".to_string()),
            rule_metadata: CalendarRuleMetadataArtifact {
                holiday_calendar_code: "US_EQUITIES".to_string(),
                early_close_rule_set: Some("US_EQUITIES_EARLY_CLOSE".to_string()),
            },
            regular_sessions: full_week_regular_sessions("09:30", "16:00"),
            holiday_dates: vec![],
            early_close_sessions: vec![],
        }
    }

    fn xtks_full_week_artifact() -> ExchangeCalendarArtifact {
        ExchangeCalendarArtifact {
            schema_version: "calendar.v1".to_string(),
            exchange_code: "XTKS".to_string(),
            exchange_name: "Tokyo Stock Exchange".to_string(),
            timezone_name: Some("Asia/Tokyo".to_string()),
            rule_metadata: CalendarRuleMetadataArtifact {
                holiday_calendar_code: "JP_EQUITIES".to_string(),
                early_close_rule_set: None,
            },
            regular_sessions: full_week_regular_sessions("09:30", "15:00"),
            holiday_dates: vec![],
            early_close_sessions: vec![],
        }
    }

    fn full_week_regular_sessions(
        open_time: &str,
        close_time: &str,
    ) -> Vec<RegularSessionArtifact> {
        [
            TradingWeekday::Monday,
            TradingWeekday::Tuesday,
            TradingWeekday::Wednesday,
            TradingWeekday::Thursday,
            TradingWeekday::Friday,
        ]
        .into_iter()
        .map(|weekday| RegularSessionArtifact {
            weekday,
            windows: vec![SessionWindowArtifact {
                label: "regular".to_string(),
                open_time: open_time.to_string(),
                close_time: close_time.to_string(),
            }],
        })
        .collect()
    }

    fn timestamp_utc(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .expect("timestamp fixture must resolve to a unique UTC datetime")
    }

    fn timestamp_input_record(
        source_record_position: u64,
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> TimestampEventInputRecord {
        TimestampEventInputRecord {
            source_record_position,
            timestamp_utc: timestamp_utc(year, month, day, hour, minute, second),
        }
    }
}
