use chrono::{Datelike, NaiveDate};
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use crate::stages::seasonality::SeasonalityStageNormalizedPayload;

use super::components::{DAYS_PER_YEAR_WITH_LEAP_DAY, validate_runtime_seasonality_components};

const GREGORIAN_CYCLE_START_YEAR: i32 = 2000;
const GREGORIAN_CYCLE_END_YEAR_EXCLUSIVE: i32 = 2400;

pub(crate) fn maximum_runtime_hourly_seasonality_factor_from_payload(
    _calendar_registry: &ExchangeCalendarRegistry,
    seasonality_payload: &SeasonalityStageNormalizedPayload,
) -> Result<f64, String> {
    maximum_realizable_runtime_hourly_seasonality_factor_from_components(
        seasonality_payload.intraday_minutes_per_bin,
        seasonality_payload.intraday_factor_by_bin.as_slice(),
        seasonality_payload.day_of_week_factor.as_slice(),
        seasonality_payload.day_of_year_factor.as_slice(),
    )
}

fn maximum_realizable_runtime_hourly_seasonality_factor_from_components(
    intraday_minutes_per_bin: u16,
    intraday_factor_by_bin: &[f64],
    day_of_week_factor: &[f64],
    day_of_year_factor: &[f64],
) -> Result<f64, String> {
    validate_runtime_seasonality_components(
        "upper-bound estimation",
        intraday_minutes_per_bin,
        intraday_factor_by_bin,
        day_of_week_factor,
        day_of_year_factor,
    )?;

    maximum_realizable_intraday_day_of_week_day_of_year_factor_product(
        intraday_factor_by_bin,
        day_of_week_factor,
        day_of_year_factor,
    )
}

// The weekday and day-of-year alignment only repeats exactly over the Gregorian 400-year cycle.
fn maximum_realizable_intraday_day_of_week_day_of_year_factor_product(
    intraday_factor_by_bin: &[f64],
    day_of_week_factor: &[f64],
    day_of_year_factor: &[f64],
) -> Result<f64, String> {
    let maximum_intraday_factor = intraday_factor_by_bin
        .iter()
        .copied()
        .fold(0.0_f64, f64::max);

    let mut maximum_product = 0.0_f64;

    for year in GREGORIAN_CYCLE_START_YEAR..GREGORIAN_CYCLE_END_YEAR_EXCLUSIVE {
        for ordinal in 1..=DAYS_PER_YEAR_WITH_LEAP_DAY {
            let Some(date) = NaiveDate::from_yo_opt(year, ordinal as u32) else {
                continue;
            };

            let day_of_week_index = date.weekday().num_days_from_monday() as usize;
            let day_of_year_index = (date.ordinal() - 1) as usize;

            let Some(day_of_week_factor_value) = day_of_week_factor.get(day_of_week_index).copied()
            else {
                return Err(format!(
                    "runtime seasonality realizability scan day-of-week index {day_of_week_index} is out of range for {} factors",
                    day_of_week_factor.len()
                ));
            };

            let Some(day_of_year_factor_value) = day_of_year_factor.get(day_of_year_index).copied()
            else {
                return Err(format!(
                    "runtime seasonality realizability scan day-of-year index {day_of_year_index} is out of range for {} factors",
                    day_of_year_factor.len()
                ));
            };

            let product =
                maximum_intraday_factor * day_of_week_factor_value * day_of_year_factor_value;
            if product.is_finite() {
                maximum_product = maximum_product.max(product);
            }
        }
    }

    Ok(maximum_product)
}
