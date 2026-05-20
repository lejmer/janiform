use chrono::{DateTime, Datelike, Days, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

use janiform_market_time::calendar::{
    CalendarDate, ExchangeCalendarDefinition, SessionTime, TradingWeekday,
};

use super::components::{
    MINUTES_PER_DAY, MINUTES_PER_HOUR, structural_seasonality_factor_for_local_segment,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RuntimeSeasonalityIntervalContribution {
    pub(super) weighted_factor_nanos: f64,
    pub(super) open_exposure_nanos: i64,
    pub(super) interval_duration_nanos: i64,
}

pub(super) fn runtime_seasonality_interval_contribution(
    calendar_definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    intraday_minutes_per_bin: u16,
    intraday_factor_by_bin: &[f64],
    day_of_week_factor: &[f64],
    day_of_year_factor: &[f64],
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> Result<RuntimeSeasonalityIntervalContribution, String> {
    if interval_end_utc_nanos <= interval_start_utc_nanos {
        return Err(format!(
            "runtime seasonality interval has non-increasing bounds start={interval_start_utc_nanos}, end={interval_end_utc_nanos}"
        ));
    }

    let interval_start_utc = DateTime::<Utc>::from_timestamp_nanos(interval_start_utc_nanos);
    let interval_end_exclusive_utc_nanos = interval_end_utc_nanos.checked_sub(1).ok_or_else(|| {
        "runtime seasonality materialization cannot resolve an interval ending before i64::MIN + 1"
            .to_string()
    })?;
    let interval_end_exclusive_utc =
        DateTime::<Utc>::from_timestamp_nanos(interval_end_exclusive_utc_nanos);

    let mut local_date = interval_start_utc.with_timezone(&timezone).date_naive();
    let last_local_date = interval_end_exclusive_utc
        .with_timezone(&timezone)
        .date_naive();

    let mut weighted_factor_nanos = 0.0_f64;
    let mut open_exposure_nanos = 0_i64;

    while local_date <= last_local_date {
        let calendar_date = calendar_date_from_naive_date(local_date)?;
        let local_weekday = TradingWeekday::from_chrono_weekday(local_date.weekday());

        for session_window in
            calendar_definition.session_windows_for_local_date(calendar_date, local_weekday)
        {
            add_session_window_contribution(
                &mut weighted_factor_nanos,
                &mut open_exposure_nanos,
                SessionWindowContributionInput {
                    timezone,
                    local_date,
                    session_open_time: session_window.open_time(),
                    session_close_time: session_window.close_time(),
                    intraday_minutes_per_bin,
                    intraday_factor_by_bin,
                    day_of_week_factor,
                    day_of_year_factor,
                    interval_start_utc_nanos,
                    interval_end_utc_nanos,
                },
            )?;
        }

        local_date = local_date.checked_add_days(Days::new(1)).ok_or_else(|| {
            "runtime seasonality materialization overflowed local date iteration".to_string()
        })?;
    }

    Ok(RuntimeSeasonalityIntervalContribution {
        weighted_factor_nanos,
        open_exposure_nanos,
        interval_duration_nanos: interval_end_utc_nanos - interval_start_utc_nanos,
    })
}

#[derive(Debug, Clone, Copy)]
struct SessionWindowContributionInput<'a> {
    timezone: Tz,
    local_date: NaiveDate,
    session_open_time: SessionTime,
    session_close_time: SessionTime,
    intraday_minutes_per_bin: u16,
    intraday_factor_by_bin: &'a [f64],
    day_of_week_factor: &'a [f64],
    day_of_year_factor: &'a [f64],
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
}

fn add_session_window_contribution(
    weighted_factor_nanos: &mut f64,
    open_exposure_nanos: &mut i64,
    input: SessionWindowContributionInput<'_>,
) -> Result<(), String> {
    let session_open_minute = session_time_minute_of_day(input.session_open_time);
    let session_close_minute = session_time_minute_of_day(input.session_close_time);

    let first_intraday_bin = session_open_minute / usize::from(input.intraday_minutes_per_bin);
    let last_intraday_bin =
        (session_close_minute - 1) / usize::from(input.intraday_minutes_per_bin);

    for intraday_bin_index in first_intraday_bin..=last_intraday_bin {
        let bin_start_minute = intraday_bin_index * usize::from(input.intraday_minutes_per_bin);
        let bin_end_minute = bin_start_minute + usize::from(input.intraday_minutes_per_bin);

        let segment_start_minute = session_open_minute.max(bin_start_minute);
        let segment_end_minute = session_close_minute.min(bin_end_minute);
        if segment_start_minute >= segment_end_minute {
            continue;
        }

        let segment_start_utc_nanos = local_minute_boundary_utc_nanos(
            input.timezone,
            input.local_date,
            segment_start_minute,
        )?;
        let segment_end_utc_nanos =
            local_minute_boundary_utc_nanos(input.timezone, input.local_date, segment_end_minute)?;

        let overlap_start_utc_nanos = input.interval_start_utc_nanos.max(segment_start_utc_nanos);
        let overlap_end_utc_nanos = input.interval_end_utc_nanos.min(segment_end_utc_nanos);
        if overlap_start_utc_nanos >= overlap_end_utc_nanos {
            continue;
        }

        let overlap_nanos = overlap_end_utc_nanos
            .checked_sub(overlap_start_utc_nanos)
            .ok_or_else(|| {
                "runtime seasonality materialization overflowed open exposure overlap".to_string()
            })?;

        let segment_factor = structural_seasonality_factor_for_local_segment(
            input.intraday_factor_by_bin,
            input.day_of_week_factor,
            input.day_of_year_factor,
            input.local_date,
            intraday_bin_index,
        )?;

        let weighted_increment = segment_factor * overlap_nanos as f64;
        if !weighted_increment.is_finite() {
            return Err(
                "runtime seasonality materialization produced non-finite weighted exposure"
                    .to_string(),
            );
        }

        *weighted_factor_nanos += weighted_increment;

        if !weighted_factor_nanos.is_finite() {
            return Err(
                "runtime seasonality materialization accumulated non-finite weighted exposure"
                    .to_string(),
            );
        }

        *open_exposure_nanos = open_exposure_nanos
            .checked_add(overlap_nanos)
            .ok_or_else(|| {
                "runtime seasonality materialization overflowed open exposure accumulation"
                    .to_string()
            })?;
    }

    Ok(())
}

fn calendar_date_from_naive_date(date: NaiveDate) -> Result<CalendarDate, String> {
    Ok(CalendarDate {
        year: date.year(),
        month: u8::try_from(date.month()).map_err(|_| {
            format!("runtime seasonality materialization cannot represent local month for {date}")
        })?,
        day: u8::try_from(date.day()).map_err(|_| {
            format!("runtime seasonality materialization cannot represent local day for {date}")
        })?,
    })
}

fn session_time_minute_of_day(session_time: SessionTime) -> usize {
    usize::from(session_time.hour()) * MINUTES_PER_HOUR + usize::from(session_time.minute())
}

fn local_minute_boundary_utc_nanos(
    timezone: Tz,
    local_date: NaiveDate,
    minute_of_day: usize,
) -> Result<i64, String> {
    if minute_of_day > MINUTES_PER_DAY {
        return Err(format!(
            "runtime seasonality materialization received invalid local minute boundary {minute_of_day}"
        ));
    }

    let day_offset = minute_of_day / MINUTES_PER_DAY;
    let minute_within_day = minute_of_day % MINUTES_PER_DAY;

    let boundary_date = local_date
        .checked_add_days(Days::new(day_offset as u64))
        .ok_or_else(|| {
            "runtime seasonality materialization overflowed local boundary date".to_string()
        })?;

    let hour = (minute_within_day / MINUTES_PER_HOUR) as u32;
    let minute = (minute_within_day % MINUTES_PER_HOUR) as u32;

    match timezone.with_ymd_and_hms(
        boundary_date.year(),
        boundary_date.month(),
        boundary_date.day(),
        hour,
        minute,
        0,
    ) {
        LocalResult::Single(timestamp) => {
            timestamp.with_timezone(&Utc).timestamp_nanos_opt().ok_or_else(|| {
                format!(
                    "runtime seasonality materialization timestamp is outside supported nanosecond range for local boundary {boundary_date} {hour:02}:{minute:02}"
                )
            })
        }
        LocalResult::Ambiguous(_, _) => Err(format!(
            "runtime seasonality materialization encountered ambiguous local boundary {boundary_date} {hour:02}:{minute:02} in timezone {timezone}"
        )),
        LocalResult::None => Err(format!(
            "runtime seasonality materialization encountered nonexistent local boundary {boundary_date} {hour:02}:{minute:02} in timezone {timezone}"
        )),
    }
}
