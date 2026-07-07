use super::contracts::MarketDataTranslationCalibrationError;
use super::known_news_samples::activity_lift;
use super::session_windows::{build_session_window_utc, session_windows_for_local_date};
use super::shared::{
    BTreeMap, CalendarDate, CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    Datelike, ExchangeCalendarDefinition, NaiveDate, OpenTickRecord, TradingWeekday, Tz,
    datetime_from_nanos,
};

pub(super) fn derive_close_open_boundary_observations(
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    exchange_code: &str,
    open_price_ticks: &[OpenTickRecord],
    open_price_tick_timestamps: &[i64],
) -> Result<
    Vec<CalibrationReturnComponentsObservedCloseOpenBoundaryObservation>,
    MarketDataTranslationCalibrationError,
> {
    let mut observations_by_boundary: BTreeMap<
        (i64, i64),
        CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    > = BTreeMap::new();

    for (previous_tick, current_tick) in
        open_price_ticks.iter().zip(open_price_ticks.iter().skip(1))
    {
        if previous_tick.session_close_utc_nanos == current_tick.session_close_utc_nanos
            && previous_tick.session_open_utc_nanos == current_tick.session_open_utc_nanos
        {
            continue;
        }

        let close_timestamp_utc_nanos = previous_tick.session_close_utc_nanos;

        let Some(open_timestamp_utc_nanos) = first_open_boundary_after_close(
            definition,
            timezone,
            exchange_code,
            close_timestamp_utc_nanos,
            current_tick.timestamp_utc_nanos,
        )?
        else {
            continue;
        };

        if open_timestamp_utc_nanos <= close_timestamp_utc_nanos {
            continue;
        }

        observations_by_boundary.insert(
            (close_timestamp_utc_nanos, open_timestamp_utc_nanos),
            CalibrationReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos,
                close_price: previous_tick.price,
                open_timestamp_utc_nanos,
                open_price: current_tick.price,
                post_shock_activity_lift: activity_lift(
                    open_timestamp_utc_nanos,
                    open_price_ticks,
                    open_price_tick_timestamps,
                ),
            },
        );
    }

    Ok(observations_by_boundary.into_values().collect())
}

pub(super) fn first_open_boundary_after_close(
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    exchange_code: &str,
    close_timestamp_utc_nanos: i64,
    upper_bound_timestamp_utc_nanos: i64,
) -> Result<Option<i64>, MarketDataTranslationCalibrationError> {
    if upper_bound_timestamp_utc_nanos <= close_timestamp_utc_nanos {
        return Ok(None);
    }

    let search_start_timestamp = datetime_from_nanos(
        close_timestamp_utc_nanos.saturating_add(1),
        MarketDataTranslationCalibrationError::InvalidSessionBoundary {
            exchange_code: exchange_code.to_string(),
            timestamp_utc_nanos: close_timestamp_utc_nanos.saturating_add(1),
        },
    )?;
    let search_end_timestamp = datetime_from_nanos(
        upper_bound_timestamp_utc_nanos,
        MarketDataTranslationCalibrationError::InvalidSessionBoundary {
            exchange_code: exchange_code.to_string(),
            timestamp_utc_nanos: upper_bound_timestamp_utc_nanos,
        },
    )?;

    let mut current_local_date =
        CalendarDate::from_local_timestamp(&search_start_timestamp.with_timezone(&timezone));
    let final_local_date =
        CalendarDate::from_local_timestamp(&search_end_timestamp.with_timezone(&timezone));

    loop {
        let local_weekday = TradingWeekday::from_chrono_weekday(
            NaiveDate::from_ymd_opt(
                current_local_date.year,
                current_local_date.month as u32,
                current_local_date.day as u32,
            )
            .ok_or_else(
                || MarketDataTranslationCalibrationError::InvalidSessionBoundary {
                    exchange_code: exchange_code.to_string(),
                    timestamp_utc_nanos: close_timestamp_utc_nanos,
                },
            )?
            .weekday(),
        );

        let session_windows =
            session_windows_for_local_date(definition, current_local_date, local_weekday);

        for session_window in session_windows {
            let session_window_utc = build_session_window_utc(
                timezone,
                exchange_code,
                current_local_date,
                session_window,
            )?;
            if session_window_utc.open_utc_nanos <= close_timestamp_utc_nanos {
                continue;
            }
            if session_window_utc.open_utc_nanos > upper_bound_timestamp_utc_nanos {
                return Ok(None);
            }

            return Ok(Some(session_window_utc.open_utc_nanos));
        }

        if current_local_date == final_local_date {
            break;
        }

        let next_date = NaiveDate::from_ymd_opt(
            current_local_date.year,
            current_local_date.month as u32,
            current_local_date.day as u32,
        )
        .and_then(|date| date.succ_opt())
        .ok_or_else(
            || MarketDataTranslationCalibrationError::InvalidSessionBoundary {
                exchange_code: exchange_code.to_string(),
                timestamp_utc_nanos: upper_bound_timestamp_utc_nanos,
            },
        )?;

        current_local_date = CalendarDate {
            year: next_date.year(),
            month: next_date.month() as u8,
            day: next_date.day() as u8,
        };
    }

    Ok(None)
}
