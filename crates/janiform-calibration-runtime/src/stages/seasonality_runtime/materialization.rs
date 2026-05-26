use chrono_tz::Tz;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::calendar_exposure::{
    RuntimeSeasonalityIntervalContribution, runtime_seasonality_interval_contribution,
};
use super::components::validate_runtime_seasonality_components;

pub(crate) fn build_runtime_hourly_seasonality_factors_from_components(
    input: RuntimeHourlySeasonalityFactorsBuildInput<'_>,
) -> Result<Vec<f64>, String> {
    let RuntimeHourlySeasonalityFactorsBuildInput {
        calendar_registry,
        exchange_code,
        intraday_minutes_per_bin,
        intraday_factor_by_bin,
        day_of_week_factor,
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

    validate_runtime_seasonality_components(
        "materialization",
        intraday_minutes_per_bin,
        intraday_factor_by_bin,
        day_of_week_factor,
        day_of_year_factor,
    )?;

    let calendar_definition = calendar_registry
        .get_required(exchange_code)
        .map_err(|error| error.to_string())?;

    let timezone_name = calendar_definition.timezone_name().to_string();
    let timezone: Tz = timezone_name.parse().map_err(|_| {
        format!("exchange `{exchange_code}` has invalid timezone name `{timezone_name}`")
    })?;

    let mut interval_contributions = Vec::new();
    let mut interval_start_utc_nanos = range_start_utc_nanos;

    while interval_start_utc_nanos < range_end_utc_nanos {
        let interval_end_utc_nanos = interval_start_utc_nanos
            .checked_add(interval_duration_nanos)
            .ok_or_else(|| {
                "runtime seasonality materialization overflowed the interval grid".to_string()
            })?
            .min(range_end_utc_nanos);

        interval_contributions.push(runtime_seasonality_interval_contribution(
            calendar_definition,
            timezone,
            intraday_minutes_per_bin,
            intraday_factor_by_bin,
            day_of_week_factor,
            day_of_year_factor,
            interval_start_utc_nanos,
            interval_end_utc_nanos,
        )?);

        interval_start_utc_nanos = interval_start_utc_nanos
            .checked_add(interval_duration_nanos)
            .ok_or_else(|| {
                "runtime seasonality materialization overflowed the interval grid".to_string()
            })?;
    }

    open_time_mean_renormalize_runtime_hourly_factors(interval_contributions)
}

pub(crate) struct RuntimeHourlySeasonalityFactorsBuildInput<'a> {
    pub(crate) calendar_registry: &'a ExchangeCalendarRegistry,
    pub(crate) exchange_code: &'a str,
    pub(crate) intraday_minutes_per_bin: u16,
    pub(crate) intraday_factor_by_bin: &'a [f64],
    pub(crate) day_of_week_factor: &'a [f64],
    pub(crate) day_of_year_factor: &'a [f64],
    pub(crate) range_start_utc_nanos: i64,
    pub(crate) range_end_utc_nanos: i64,
    pub(crate) interval_duration_nanos: i64,
}

fn open_time_mean_renormalize_runtime_hourly_factors(
    interval_contributions: Vec<RuntimeSeasonalityIntervalContribution>,
) -> Result<Vec<f64>, String> {
    if interval_contributions.is_empty() {
        return Ok(Vec::new());
    }

    let total_open_exposure_nanos =
        interval_contributions
            .iter()
            .try_fold(0_i64, |accumulator, interval_contribution| {
                accumulator
                    .checked_add(interval_contribution.open_exposure_nanos)
                    .ok_or_else(|| {
                        "runtime seasonality materialization overflowed total open exposure"
                            .to_string()
                    })
            })?;

    if total_open_exposure_nanos == 0 {
        return Ok(vec![0.0; interval_contributions.len()]);
    }

    let total_weighted_factor_nanos: f64 = interval_contributions
        .iter()
        .map(|interval_contribution| interval_contribution.weighted_factor_nanos)
        .sum();

    let normalization_constant = total_weighted_factor_nanos / total_open_exposure_nanos as f64;
    if !normalization_constant.is_finite() || normalization_constant <= 0.0 {
        return Err(
            "runtime seasonality materialization requires a finite positive open-time mean factor"
                .to_string(),
        );
    }

    interval_contributions
        .into_iter()
        .map(|interval_contribution| {
            if interval_contribution.interval_duration_nanos <= 0 {
                return Err(
                    "runtime seasonality materialization encountered a non-positive interval duration"
                        .to_string(),
                );
            }

            let interval_factor = interval_contribution.weighted_factor_nanos
                / interval_contribution.interval_duration_nanos as f64
                / normalization_constant;
            if !interval_factor.is_finite() || interval_factor < 0.0 {
                return Err(
                    "runtime seasonality materialization produced an invalid interval factor"
                        .to_string(),
                );
            }

            Ok(interval_factor)
        })
        .collect()
}
