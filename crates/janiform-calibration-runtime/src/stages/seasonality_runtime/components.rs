use chrono::{Datelike, NaiveDate};

use janiform_foundations::time::{NANOS_PER_HOUR, NANOS_PER_MINUTE};

pub(super) const DAYS_PER_WEEK: usize = 7;
pub(super) const HOURS_PER_DAY: usize = 24;
pub(super) const MINUTES_PER_HOUR: usize = (NANOS_PER_HOUR / NANOS_PER_MINUTE) as usize;
pub(super) const MINUTES_PER_DAY: usize = HOURS_PER_DAY * MINUTES_PER_HOUR;
pub(super) const DAYS_PER_YEAR_WITH_LEAP_DAY: usize = 366;

pub(super) fn validate_runtime_seasonality_components(
    operation: &str,
    intraday_minutes_per_bin: u16,
    intraday_factor_by_bin: &[f64],
    day_of_week_factor: &[f64],
    day_of_year_factor: &[f64],
) -> Result<(), String> {
    if intraday_minutes_per_bin == 0 {
        return Err(format!(
            "runtime seasonality {operation} requires a positive intraday bin width"
        ));
    }

    let intraday_bin_width = usize::from(intraday_minutes_per_bin);
    if intraday_bin_width > MINUTES_PER_DAY || !MINUTES_PER_DAY.is_multiple_of(intraday_bin_width) {
        return Err(format!(
            "runtime seasonality {operation} requires an intraday bin width that divides one local day, found {intraday_minutes_per_bin}"
        ));
    }

    let expected_intraday_bin_count = MINUTES_PER_DAY / intraday_bin_width;
    if intraday_factor_by_bin.len() != expected_intraday_bin_count {
        return Err(format!(
            "runtime seasonality {operation} requires {expected_intraday_bin_count} intraday factors, found {}",
            intraday_factor_by_bin.len()
        ));
    }

    if day_of_week_factor.len() < DAYS_PER_WEEK {
        return Err(format!(
            "runtime seasonality {operation} requires at least 7 weekday factors, found {}",
            day_of_week_factor.len()
        ));
    }

    if day_of_year_factor.len() < DAYS_PER_YEAR_WITH_LEAP_DAY {
        return Err(format!(
            "runtime seasonality {operation} requires at least 366 day-of-year factors, found {}",
            day_of_year_factor.len()
        ));
    }

    validate_positive_factors(operation, "intraday", intraday_factor_by_bin)?;
    validate_positive_factors(operation, "weekday", day_of_week_factor)?;
    validate_positive_factors(operation, "day-of-year", day_of_year_factor)
}

pub(super) fn structural_seasonality_factor_for_local_segment(
    intraday_factor_by_bin: &[f64],
    day_of_week_factor: &[f64],
    day_of_year_factor: &[f64],
    local_date: NaiveDate,
    intraday_bin_index: usize,
) -> Result<f64, String> {
    let day_of_week_index = local_date.weekday().number_from_monday() as usize - 1;
    let day_of_year_index = local_date.ordinal0() as usize;

    let intraday_factor = intraday_factor_by_bin
        .get(intraday_bin_index)
        .copied()
        .ok_or_else(|| {
            format!(
                "runtime seasonality materialization intraday bin index {intraday_bin_index} is out of range for {} factors",
                intraday_factor_by_bin.len()
            )
        })?;

    let day_of_week_factor_value = day_of_week_factor
        .get(day_of_week_index)
        .copied()
        .ok_or_else(|| {
            format!(
                "runtime seasonality materialization day-of-week index {day_of_week_index} is out of range for {} factors",
                day_of_week_factor.len()
            )
        })?;

    let day_of_year_factor_value = day_of_year_factor
        .get(day_of_year_index)
        .copied()
        .ok_or_else(|| {
            format!(
                "runtime seasonality materialization day-of-year index {day_of_year_index} is out of range for {} factors",
                day_of_year_factor.len()
            )
        })?;

    let combined_factor = intraday_factor * day_of_week_factor_value * day_of_year_factor_value;
    if !combined_factor.is_finite() {
        return Err(format!(
            "runtime seasonality materialization produced non-finite factor for local date {local_date} and intraday bin {intraday_bin_index}"
        ));
    }

    Ok(combined_factor)
}

fn validate_positive_factors(
    operation: &str,
    component_name: &str,
    factors: &[f64],
) -> Result<(), String> {
    for (factor_index, factor) in factors.iter().copied().enumerate() {
        if !factor.is_finite() || factor <= 0.0 {
            return Err(format!(
                "runtime seasonality {operation} requires finite positive {component_name} factors, found {factor} at index {factor_index}"
            ));
        }
    }

    Ok(())
}
