use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use chrono_tz::Tz;

use janiform_foundations::numeric::arithmetic_mean;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use crate::stages::seasonality::SeasonalityStageNormalizedPayload;

const DAYS_PER_WEEK: usize = 7;
const HOURS_PER_DAY: usize = 24;
const HOURS_PER_WEEK: usize = DAYS_PER_WEEK * HOURS_PER_DAY;
const DAYS_PER_YEAR_WITH_LEAP_DAY: usize = 366;
const GREGORIAN_CYCLE_START_YEAR: i32 = 2000;
const GREGORIAN_CYCLE_END_YEAR_EXCLUSIVE: i32 = 2400;

pub(crate) fn build_runtime_hourly_seasonality_factors_from_components(
    input: RuntimeHourlySeasonalityFactorsBuildInput<'_>,
) -> Result<Vec<f64>, String> {
    let RuntimeHourlySeasonalityFactorsBuildInput {
        calendar_registry,
        exchange_code,
        intraday_minutes_per_bin,
        intraday_factor_by_bin,
        day_of_week_factor,
        hour_of_week_factor,
        day_of_year_factor,
        range_start_utc_nanos,
        range_end_utc_nanos,
        interval_duration_nanos,
    } = input;

    if range_end_utc_nanos <= range_start_utc_nanos {
        return Ok(Vec::new());
    }

    if interval_duration_nanos <= 0 {
        return Err("runtime seasonality materialization requires a positive interval".to_string());
    }
    if intraday_factor_by_bin.is_empty() {
        return Err(
            "runtime seasonality materialization requires non-empty intraday factors".to_string(),
        );
    }

    if day_of_week_factor.len() < DAYS_PER_WEEK {
        return Err(format!(
            "runtime seasonality materialization requires at least 7 weekday factors, found {}",
            day_of_week_factor.len()
        ));
    }
    if hour_of_week_factor.len() < HOURS_PER_WEEK {
        return Err(format!(
            "runtime seasonality materialization requires at least 168 hour-of-week factors, found {}",
            hour_of_week_factor.len()
        ));
    }
    if day_of_year_factor.len() < DAYS_PER_YEAR_WITH_LEAP_DAY {
        return Err(format!(
            "runtime seasonality materialization requires at least 366 day-of-year factors, found {}",
            day_of_year_factor.len()
        ));
    }

    let timezone_name = calendar_registry
        .get_required(exchange_code)
        .map_err(|error| error.to_string())?
        .timezone_name()
        .to_string();
    let timezone: Tz = timezone_name.parse().map_err(|_| {
        format!("exchange `{exchange_code}` has invalid timezone name `{timezone_name}`")
    })?;

    let mut hourly_factors = Vec::new();
    let mut interval_start_utc_nanos = range_start_utc_nanos;
    while interval_start_utc_nanos < range_end_utc_nanos {
        hourly_factors.push(structural_seasonality_factor_at_timestamp(
            intraday_minutes_per_bin,
            intraday_factor_by_bin,
            day_of_week_factor,
            hour_of_week_factor,
            day_of_year_factor,
            timezone,
            interval_start_utc_nanos,
        )?);

        interval_start_utc_nanos = interval_start_utc_nanos
            .checked_add(interval_duration_nanos)
            .ok_or_else(|| {
                "runtime seasonality materialization overflowed the interval grid".to_string()
            })?;
    }

    mean_renormalize_runtime_hourly_factors(hourly_factors)
}

pub(crate) struct RuntimeHourlySeasonalityFactorsBuildInput<'a> {
    pub(crate) calendar_registry: &'a ExchangeCalendarRegistry,
    pub(crate) exchange_code: &'a str,
    pub(crate) intraday_minutes_per_bin: u16,
    pub(crate) intraday_factor_by_bin: &'a [f64],
    pub(crate) day_of_week_factor: &'a [f64],
    pub(crate) hour_of_week_factor: &'a [f64],
    pub(crate) day_of_year_factor: &'a [f64],
    pub(crate) range_start_utc_nanos: i64,
    pub(crate) range_end_utc_nanos: i64,
    pub(crate) interval_duration_nanos: i64,
}

pub(crate) fn maximum_runtime_hourly_seasonality_factor_from_payload(
    _calendar_registry: &ExchangeCalendarRegistry,
    seasonality_payload: &SeasonalityStageNormalizedPayload,
) -> Result<f64, String> {
    maximum_realizable_runtime_hourly_seasonality_factor_from_components(
        seasonality_payload.intraday_minutes_per_bin,
        seasonality_payload.intraday_factor_by_bin.as_slice(),
        seasonality_payload.day_of_week_factor.as_slice(),
        seasonality_payload.hour_of_week_factor.as_slice(),
        seasonality_payload.day_of_year_factor.as_slice(),
    )
}

pub(crate) fn maximum_realizable_runtime_hourly_seasonality_factor_from_components(
    intraday_minutes_per_bin: u16,
    intraday_factor_by_bin: &[f64],
    day_of_week_factor: &[f64],
    hour_of_week_factor: &[f64],
    day_of_year_factor: &[f64],
) -> Result<f64, String> {
    if intraday_minutes_per_bin == 0 {
        return Err(
            "runtime seasonality upper-bound estimation requires a positive intraday bin width"
                .to_string(),
        );
    }
    if intraday_factor_by_bin.is_empty() {
        return Err(
            "runtime seasonality upper-bound estimation requires non-empty intraday factors"
                .to_string(),
        );
    }
    if day_of_week_factor.len() < DAYS_PER_WEEK {
        return Err(format!(
            "runtime seasonality upper-bound estimation requires at least 7 weekday factors, found {}",
            day_of_week_factor.len()
        ));
    }
    if hour_of_week_factor.len() < HOURS_PER_WEEK {
        return Err(format!(
            "runtime seasonality upper-bound estimation requires at least 168 hour-of-week factors, found {}",
            hour_of_week_factor.len()
        ));
    }
    if day_of_year_factor.len() < DAYS_PER_YEAR_WITH_LEAP_DAY {
        return Err(format!(
            "runtime seasonality upper-bound estimation requires at least 366 day-of-year factors, found {}",
            day_of_year_factor.len()
        ));
    }

    maximum_realizable_hour_of_week_day_of_year_factor_product(
        hour_of_week_factor,
        day_of_year_factor,
    )
}

// The weekday and day-of-year alignment only repeats exactly over the Gregorian 400-year cycle.
fn maximum_realizable_hour_of_week_day_of_year_factor_product(
    hour_of_week_factor: &[f64],
    day_of_year_factor: &[f64],
) -> Result<f64, String> {
    let mut maximum_product = 1.0_f64;

    for year in GREGORIAN_CYCLE_START_YEAR..GREGORIAN_CYCLE_END_YEAR_EXCLUSIVE {
        for ordinal in 1..=DAYS_PER_YEAR_WITH_LEAP_DAY {
            let Some(date) = NaiveDate::from_yo_opt(year, ordinal as u32) else {
                continue;
            };

            let day_of_week_index = date.weekday().num_days_from_monday() as usize;
            let day_of_year_index = (date.ordinal() - 1) as usize;

            let Some(day_of_year_factor_value) = day_of_year_factor.get(day_of_year_index).copied()
            else {
                return Err(format!(
                    "runtime seasonality realizability scan day-of-year index {day_of_year_index} is out of range for {} factors",
                    day_of_year_factor.len()
                ));
            };

            for hour_of_day in 0..HOURS_PER_DAY {
                let hour_of_week_index = day_of_week_index * HOURS_PER_DAY + hour_of_day;
                let Some(hour_of_week_factor_value) =
                    hour_of_week_factor.get(hour_of_week_index).copied()
                else {
                    return Err(format!(
                        "runtime seasonality realizability scan hour-of-week index {hour_of_week_index} is out of range for {} factors",
                        hour_of_week_factor.len()
                    ));
                };

                if hour_of_week_factor_value.is_finite() && day_of_year_factor_value.is_finite() {
                    maximum_product =
                        maximum_product.max(hour_of_week_factor_value * day_of_year_factor_value);
                }
            }
        }
    }

    Ok(maximum_product)
}

fn structural_seasonality_factor_at_timestamp(
    _intraday_minutes_per_bin: u16,
    _intraday_factor_by_bin: &[f64],
    _day_of_week_factor: &[f64],
    hour_of_week_factor: &[f64],
    day_of_year_factor: &[f64],
    timezone: Tz,
    timestamp_utc_nanos: i64,
) -> Result<f64, String> {
    let timestamp_utc = DateTime::<Utc>::from_timestamp_nanos(timestamp_utc_nanos);
    let exchange_local_timestamp = timestamp_utc.with_timezone(&timezone);

    let day_of_week_index = exchange_local_timestamp.weekday().number_from_monday() as usize - 1;
    let hour_of_week_index =
        day_of_week_index * HOURS_PER_DAY + exchange_local_timestamp.hour() as usize;
    let day_of_year_index = exchange_local_timestamp.ordinal0() as usize;

    let Some(hour_of_week_factor) = hour_of_week_factor.get(hour_of_week_index).copied() else {
        return Err(format!(
            "runtime seasonality materialization hour-of-week index {hour_of_week_index} is out of range for {} factors",
            hour_of_week_factor.len()
        ));
    };

    let Some(day_of_year_factor) = day_of_year_factor.get(day_of_year_index).copied() else {
        return Err(format!(
            "runtime seasonality materialization day-of-year index {day_of_year_index} is out of range for {} factors",
            day_of_year_factor.len()
        ));
    };

    let combined_factor = hour_of_week_factor * day_of_year_factor;
    if !combined_factor.is_finite() {
        return Err(format!(
            "runtime seasonality materialization produced non-finite factor at timestamp {timestamp_utc_nanos}"
        ));
    }

    Ok(combined_factor)
}

fn mean_renormalize_runtime_hourly_factors(hourly_factors: Vec<f64>) -> Result<Vec<f64>, String> {
    if hourly_factors.is_empty() {
        return Ok(hourly_factors);
    }

    let mean_factor = arithmetic_mean(hourly_factors.as_slice()).ok_or_else(|| {
        "runtime seasonality materialization requires a non-empty factor sequence".to_string()
    })?;
    if !mean_factor.is_finite() || mean_factor == 0.0 {
        return Err(
            "runtime seasonality materialization requires factors with a finite non-zero mean"
                .to_string(),
        );
    }

    Ok(hourly_factors
        .into_iter()
        .map(|factor| factor / mean_factor)
        .collect())
}
