use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Datelike, Utc, Weekday as ChronoWeekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

/// Weekday used by regular exchange sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
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
    pub(crate) fn from_chrono_weekday(weekday: ChronoWeekday) -> Self {
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
pub(crate) struct CalendarDate {
    pub(crate) year: i32,
    pub(crate) month: u8,
    pub(crate) day: u8,
}

impl CalendarDate {
    pub(crate) fn parse(raw: &str) -> Result<Self, &'static str> {
        let (year, month, day) = crate::calendar::validation::parse_iso_date_parts(raw)?;
        Ok(Self { year, month, day })
    }

    pub(crate) fn from_local_timestamp(timestamp_local: &DateTime<Tz>) -> Self {
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
    pub(crate) fn includes_market_state(self, market_state: MarketState) -> bool {
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarRuleMetadataArtifact {
    pub holiday_calendar_code: String,
    pub early_close_rule_set: Option<String>,
}

/// Regular session windows for a weekday in artifact form.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegularSessionArtifact {
    pub weekday: TradingWeekday,
    pub windows: Vec<SessionWindowArtifact>,
}

/// Session window in artifact form.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionWindowArtifact {
    pub label: String,
    pub open_time: String,
    pub close_time: String,
}

/// Truncated session windows for one explicit early-close date.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EarlyCloseSessionArtifact {
    pub date: String,
    pub windows: Vec<SessionWindowArtifact>,
}

/// Immutable exchange calendar definition used by core calendar services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeCalendarDefinition {
    pub(crate) schema_version: String,
    pub(crate) exchange_metadata: ExchangeMetadata,
    pub(crate) timezone_name: String,
    pub(crate) rule_metadata: CalendarRuleMetadata,
    pub(crate) regular_sessions: Box<[RegularSessionDefinition]>,
    pub(crate) holiday_dates: BTreeSet<CalendarDate>,
    pub(crate) early_close_sessions_by_date: BTreeMap<CalendarDate, Box<[SessionWindowDefinition]>>,
}

/// Immutable metadata for an exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeMetadata {
    pub(crate) exchange_code: String,
    pub(crate) exchange_name: String,
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
    pub(crate) holiday_calendar_code: String,
    pub(crate) early_close_rule_set: Option<String>,
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
    pub(crate) weekday: TradingWeekday,
    pub(crate) windows: Box<[SessionWindowDefinition]>,
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
    pub(crate) label: String,
    pub(crate) open_time: SessionTime,
    pub(crate) close_time: SessionTime,
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

    pub(crate) fn contains_local_clock(&self, local_clock: SessionTime) -> bool {
        // Boundary policy is [open, close): exact open resolves open and exact close resolves closed.
        local_clock >= self.open_time && local_clock < self.close_time
    }
}

/// Clock time represented in local exchange time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionTime {
    hour: u8,
    minute: u8,
}

impl SessionTime {
    pub(crate) fn from_hour_minute(hour: u8, minute: u8) -> Self {
        Self { hour, minute }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self, &'static str> {
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

/// Versioned artifact containing exchange calendar entries for registry construction.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeCalendarRegistryArtifact {
    pub schema_version: String,
    pub artifact_date: String,
    pub exchanges: Vec<ExchangeCalendarArtifact>,
}
