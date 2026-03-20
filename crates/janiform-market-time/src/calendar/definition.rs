use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;

use crate::calendar::errors::ExchangeCalendarValidationError;
use crate::calendar::types::{
    CalendarDate, CalendarRuleMetadata, ExchangeCalendarArtifact, ExchangeCalendarDefinition,
    ExchangeMetadata, MarketState, RegularSessionDefinition, SessionTime, SessionWindowDefinition,
    TradingWeekday,
};
use crate::calendar::validation::{
    build_early_close_sessions, build_holiday_dates, normalize_optional_string, require_non_empty,
};

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

        let mut grouped_windows: std::collections::BTreeMap<
            TradingWeekday,
            Vec<SessionWindowDefinition>,
        > = std::collections::BTreeMap::new();

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

    pub fn resolve_market_state_at_utc_with_timezone(
        &self,
        timezone: Tz,
        timestamp_utc: DateTime<Utc>,
    ) -> MarketState {
        resolve_market_state_for_definition_at_utc(self, timezone, timestamp_utc)
    }

    pub fn rule_metadata(&self) -> &CalendarRuleMetadata {
        &self.rule_metadata
    }

    pub fn regular_sessions(&self) -> &[RegularSessionDefinition] {
        &self.regular_sessions
    }

    pub fn is_holiday(&self, local_date: CalendarDate) -> bool {
        self.holiday_dates.contains(&local_date)
    }

    pub fn session_windows_for_local_date(
        &self,
        local_date: CalendarDate,
        local_weekday: TradingWeekday,
    ) -> &[SessionWindowDefinition] {
        if self.is_holiday(local_date) {
            return &[];
        }

        if let Some(early_close_windows) = self.early_close_sessions_by_date.get(&local_date) {
            return early_close_windows.as_ref();
        }

        if let Some(regular_session) = self
            .regular_sessions
            .iter()
            .find(|definition| definition.weekday == local_weekday)
        {
            return regular_session.windows();
        }

        &[]
    }

    pub(crate) fn resolve_market_state_for_local_timestamp(
        &self,
        local_date: CalendarDate,
        weekday: TradingWeekday,
        local_clock: SessionTime,
    ) -> MarketState {
        // Rule precedence is deterministic and centralized
        // 1. Holidays always resolve closed
        // 2. Early-close windows override regular sessions for the same date
        // 3. Regular sessions apply for all remaining dates
        if self.is_holiday(local_date) {
            return MarketState::Closed;
        }

        resolve_market_state_for_session_windows(
            self.session_windows_for_local_date(local_date, weekday),
            local_clock,
        )
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

pub(crate) fn resolve_market_state_for_definition_at_utc(
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    timestamp_utc: DateTime<Utc>,
) -> MarketState {
    let timestamp_local = timestamp_utc.with_timezone(&timezone);
    resolve_market_state_for_definition_at_local(definition, timestamp_local)
}

pub(crate) fn resolve_market_state_for_definition_at_local(
    definition: &ExchangeCalendarDefinition,
    timestamp_local: DateTime<Tz>,
) -> MarketState {
    let local_date = CalendarDate::from_local_timestamp(&timestamp_local);
    let local_weekday = TradingWeekday::from_chrono_weekday(timestamp_local.weekday());
    let local_clock =
        SessionTime::from_hour_minute(timestamp_local.hour() as u8, timestamp_local.minute() as u8);

    definition.resolve_market_state_for_local_timestamp(local_date, local_weekday, local_clock)
}
