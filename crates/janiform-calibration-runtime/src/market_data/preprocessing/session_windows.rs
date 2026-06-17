use super::contracts::MarketDataTranslationCalibrationError;
use super::shared::{
    CalendarDate, DateTime, Datelike, ExchangeCalendarDefinition, ExchangeCalendarRegistry,
    ExchangeCalendarRegistryLookupError, FromStr, LocalResult, MarketState, NaiveDate, SessionTime,
    SessionWindowDefinition, SessionWindowUtc, TimeZone, Timelike, TradingWeekday, Tz, Utc,
};

pub(super) fn required_definition_and_timezone<'a>(
    calendar_registry: &'a ExchangeCalendarRegistry,
    exchange_code: &str,
) -> Result<(&'a ExchangeCalendarDefinition, Tz), MarketDataTranslationCalibrationError> {
    let definition =
        calendar_registry
            .get_required(exchange_code)
            .map_err(|error| match error {
                ExchangeCalendarRegistryLookupError::MissingExchangeId { exchange_code } => {
                    MarketDataTranslationCalibrationError::MissingExchangeId { exchange_code }
                }
            })?;

    let timezone_name = definition.timezone_name();
    let timezone = Tz::from_str(timezone_name).map_err(|_| {
        MarketDataTranslationCalibrationError::InvalidTimezoneName {
            exchange_code: exchange_code.to_string(),
            timezone_name: timezone_name.to_string(),
        }
    })?;

    Ok((definition, timezone))
}

pub(super) fn resolve_market_state_and_session_window(
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    exchange_code: &str,
    timestamp_utc: DateTime<Utc>,
) -> Result<(MarketState, Option<SessionWindowUtc>), MarketDataTranslationCalibrationError> {
    let timestamp_local = timestamp_utc.with_timezone(&timezone);
    let local_date = CalendarDate::from_local_timestamp(&timestamp_local);
    let local_weekday = TradingWeekday::from_chrono_weekday(timestamp_local.weekday());
    let local_hour = timestamp_local.hour() as u8;
    let local_minute = timestamp_local.minute() as u8;
    let local_clock = SessionTime::from_hour_minute(local_hour, local_minute);

    if definition.is_holiday(local_date) {
        return Ok((MarketState::Closed, None));
    }

    let session_windows = session_windows_for_local_date(definition, local_date, local_weekday);

    let Some(session_window) = session_windows
        .iter()
        .find(|session_window| session_window.contains_local_clock(local_clock))
    else {
        return Ok((MarketState::Closed, None));
    };

    Ok((
        MarketState::Open,
        Some(build_session_window_utc(
            timezone,
            exchange_code,
            local_date,
            session_window,
        )?),
    ))
}

pub(super) fn session_windows_for_local_date(
    definition: &ExchangeCalendarDefinition,
    local_date: CalendarDate,
    local_weekday: TradingWeekday,
) -> &[SessionWindowDefinition] {
    definition.session_windows_for_local_date(local_date, local_weekday)
}

pub(super) fn build_session_window_utc(
    timezone: Tz,
    exchange_code: &str,
    local_date: CalendarDate,
    session_window: &SessionWindowDefinition,
) -> Result<SessionWindowUtc, MarketDataTranslationCalibrationError> {
    let open_local = local_datetime_for_session_time(
        timezone,
        exchange_code,
        local_date,
        session_window.open_time().to_string().as_str(),
        session_window.open_time().hour(),
        session_window.open_time().minute(),
    )?;

    let close_local = local_datetime_for_session_time(
        timezone,
        exchange_code,
        local_date,
        session_window.close_time().to_string().as_str(),
        session_window.close_time().hour(),
        session_window.close_time().minute(),
    )?;

    Ok(SessionWindowUtc {
        open_utc_nanos: open_local
            .with_timezone(&Utc)
            .timestamp_nanos_opt()
            .ok_or_else(|| {
                MarketDataTranslationCalibrationError::SessionBoundaryOutOfNanosecondRange {
                    exchange_code: exchange_code.to_string(),
                    local_date: format!(
                        "{:04}-{:02}-{:02}",
                        local_date.year, local_date.month, local_date.day
                    ),
                    local_time: session_window.open_time().to_string(),
                }
            })?,
        close_utc_nanos: close_local
            .with_timezone(&Utc)
            .timestamp_nanos_opt()
            .ok_or_else(|| {
                MarketDataTranslationCalibrationError::SessionBoundaryOutOfNanosecondRange {
                    exchange_code: exchange_code.to_string(),
                    local_date: format!(
                        "{:04}-{:02}-{:02}",
                        local_date.year, local_date.month, local_date.day
                    ),
                    local_time: session_window.close_time().to_string(),
                }
            })?,
    })
}

pub(super) fn local_datetime_for_session_time(
    timezone: Tz,
    exchange_code: &str,
    local_date: CalendarDate,
    local_time_label: &str,
    hour: u8,
    minute: u8,
) -> Result<DateTime<Tz>, MarketDataTranslationCalibrationError> {
    let (resolved_year, resolved_month, resolved_day, resolved_hour, resolved_minute) =
        if hour == 24 && minute == 0 {
            let next_day = NaiveDate::from_ymd_opt(
                local_date.year,
                local_date.month as u32,
                local_date.day as u32,
            )
            .and_then(|date| date.succ_opt())
            .ok_or_else(|| {
                MarketDataTranslationCalibrationError::AmbiguousSessionBoundary {
                    exchange_code: exchange_code.to_string(),
                    timezone_name: timezone.name().to_string(),
                    local_date: local_date.to_string(),
                    local_time: local_time_label.to_string(),
                }
            })?;

            (
                next_day.year(),
                next_day.month(),
                next_day.day(),
                0_u32,
                0_u32,
            )
        } else {
            (
                local_date.year,
                local_date.month as u32,
                local_date.day as u32,
                hour as u32,
                minute as u32,
            )
        };

    match timezone.with_ymd_and_hms(
        resolved_year,
        resolved_month,
        resolved_day,
        resolved_hour,
        resolved_minute,
        0,
    ) {
        LocalResult::Single(timestamp_local) => Ok(timestamp_local),
        LocalResult::Ambiguous(_, _) | LocalResult::None => Err(
            MarketDataTranslationCalibrationError::AmbiguousSessionBoundary {
                exchange_code: exchange_code.to_string(),
                timezone_name: timezone.name().to_string(),
                local_date: local_date.to_string(),
                local_time: local_time_label.to_string(),
            },
        ),
    }
}
