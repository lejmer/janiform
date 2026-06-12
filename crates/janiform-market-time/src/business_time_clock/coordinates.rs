use chrono::{Datelike, NaiveDate};
use janiform_foundations::time::{
    DAYS_PER_COMMON_YEAR, DAYS_PER_LEAP_YEAR, DAYS_PER_WEEK, MINUTES_PER_DAY,
};

pub(super) fn intraday_phase(minute_of_day: usize) -> Option<f64> {
    if minute_of_day >= MINUTES_PER_DAY {
        return None;
    }

    Some(minute_of_day as f64 / MINUTES_PER_DAY as f64)
}

pub(super) fn weekly_phase(local_date: NaiveDate) -> Option<f64> {
    let weekday_position = usize::try_from(local_date.weekday().num_days_from_monday()).ok()?;

    Some(weekday_position as f64 / DAYS_PER_WEEK as f64)
}

pub(super) fn annual_phase(local_date: NaiveDate, minute_of_day: usize) -> Option<f64> {
    if minute_of_day >= MINUTES_PER_DAY {
        return None;
    }

    let ordinal_position = usize::try_from(local_date.ordinal0()).ok()?;

    let minute_of_year = ordinal_position
        .checked_mul(MINUTES_PER_DAY)?
        .checked_add(minute_of_day)?;

    let year_minutes = civil_year_minute_count(local_date.year())?;

    Some(minute_of_year as f64 / year_minutes as f64)
}

pub(super) fn civil_year_minute_count(year: i32) -> Option<usize> {
    let day_count = if is_gregorian_leap_year(year) {
        DAYS_PER_LEAP_YEAR
    } else {
        DAYS_PER_COMMON_YEAR
    };

    day_count.checked_mul(MINUTES_PER_DAY)
}

pub(super) fn fourier_log_rate(
    coefficients: FourierCoefficientSlices<'_>,
    phases: FourierPhases,
) -> Option<f64> {
    if coefficients.intraday_cosine.len() != coefficients.intraday_sine.len()
        || coefficients.weekly_cosine.len() != coefficients.weekly_sine.len()
        || coefficients.annual_cosine.len() != coefficients.annual_sine.len()
        || !coefficients.intercept.is_finite()
        || !phases.intraday.is_finite()
        || !phases.weekly.is_finite()
        || !phases.annual.is_finite()
    {
        return None;
    }

    let mut log_rate = coefficients.intercept;

    for (harmonic_position, (cosine_coefficient, sine_coefficient)) in coefficients
        .intraday_cosine
        .iter()
        .zip(coefficients.intraday_sine.iter())
        .enumerate()
    {
        let harmonic_order = harmonic_position + 1;
        let angle = std::f64::consts::TAU * harmonic_order as f64 * phases.intraday;

        log_rate += cosine_coefficient * angle.cos() + sine_coefficient * angle.sin();
    }

    for (harmonic_position, (cosine_coefficient, sine_coefficient)) in coefficients
        .weekly_cosine
        .iter()
        .zip(coefficients.weekly_sine.iter())
        .enumerate()
    {
        let harmonic_order = harmonic_position + 1;
        let angle = std::f64::consts::TAU * harmonic_order as f64 * phases.weekly;

        log_rate += cosine_coefficient * angle.cos() + sine_coefficient * angle.sin();
    }

    for (harmonic_position, (cosine_coefficient, sine_coefficient)) in coefficients
        .annual_cosine
        .iter()
        .zip(coefficients.annual_sine.iter())
        .enumerate()
    {
        let harmonic_order = harmonic_position + 1;
        let angle = std::f64::consts::TAU * harmonic_order as f64 * phases.annual;

        log_rate += cosine_coefficient * angle.cos() + sine_coefficient * angle.sin();
    }

    log_rate.is_finite().then_some(log_rate)
}

pub(super) struct FourierCoefficientSlices<'a> {
    pub(super) intercept: f64,
    pub(super) intraday_cosine: &'a [f64],
    pub(super) intraday_sine: &'a [f64],
    pub(super) weekly_cosine: &'a [f64],
    pub(super) weekly_sine: &'a [f64],
    pub(super) annual_cosine: &'a [f64],
    pub(super) annual_sine: &'a [f64],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FourierPhases {
    pub(super) intraday: f64,
    pub(super) weekly: f64,
    pub(super) annual: f64,
}

pub(super) fn feature_vector(
    intraday_harmonic_order: usize,
    weekly_harmonic_order: usize,
    annual_harmonic_order: usize,
    intraday_phase: f64,
    weekly_phase: f64,
    annual_phase: f64,
) -> Option<Vec<f64>> {
    if !intraday_phase.is_finite() || !weekly_phase.is_finite() || !annual_phase.is_finite() {
        return None;
    }

    let harmonic_count = intraday_harmonic_order
        .checked_add(weekly_harmonic_order)?
        .checked_add(annual_harmonic_order)?;

    let mut features = Vec::with_capacity(1 + 2_usize.checked_mul(harmonic_count)?);

    features.push(1.0);

    for harmonic_order in 1..=intraday_harmonic_order {
        let angle = std::f64::consts::TAU * harmonic_order as f64 * intraday_phase;

        features.push(angle.cos());
        features.push(angle.sin());
    }

    for harmonic_order in 1..=weekly_harmonic_order {
        let angle = std::f64::consts::TAU * harmonic_order as f64 * weekly_phase;

        features.push(angle.cos());
        features.push(angle.sin());
    }

    for harmonic_order in 1..=annual_harmonic_order {
        let angle = std::f64::consts::TAU * harmonic_order as f64 * annual_phase;

        features.push(angle.cos());
        features.push(angle.sin());
    }

    Some(features)
}

fn is_gregorian_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
