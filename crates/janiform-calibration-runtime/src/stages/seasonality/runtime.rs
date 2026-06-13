use chrono::Timelike;
use chrono_tz::Tz;
use janiform_calibration_artifacts::CalibrationRunSpec;
use janiform_foundations::time::{
    MINUTES_PER_DAY, MINUTES_PER_HOUR, SECONDS_PER_HOUR, utc_datetime_from_nanos,
};
use janiform_market_time::business_time_clock::{
    FourierBusinessTimeClockMaterializationInput,
    FourierBusinessTimeClockOrderedIntervalExposureInput,
    integrate_fourier_business_time_clock_ordered_interval_exposures,
    materialize_fourier_business_time_clock,
};
use janiform_market_time::calendar::{
    ExchangeCalendarDefinition, ExchangeCalendarRegistry, OPEN_MARKET_STATE_CODE,
    TimestampEventPreprocessingOutput,
};

use super::{
    FourierBusinessTimeClockModel, SeasonalityStageExecutionFailure,
    SeasonalityStageExecutionReport, SeasonalityStageNormalizedPayload,
    build_seasonality_stage_failure_report,
};

pub(crate) fn maximum_runtime_interval_seasonality_factor_from_payload(
    _calendar_registry: &ExchangeCalendarRegistry,
    seasonality_payload: &SeasonalityStageNormalizedPayload,
) -> Result<f64, String> {
    seasonality_payload
        .clock_model
        .rate_upper_bound()
        .ok_or_else(|| "clock_model must admit a finite positive runtime rate bound".to_string())
}

pub(crate) struct RuntimeIntervalSeasonalityFactorsBuildInput<'a> {
    pub(crate) calendar_registry: &'a ExchangeCalendarRegistry,
    pub(crate) exchange_code: &'a str,
    pub(crate) clock_model: &'a FourierBusinessTimeClockModel,
    pub(crate) range_start_utc_nanos: i64,
    pub(crate) range_end_utc_nanos: i64,
    pub(crate) interval_duration_nanos: i64,
}

pub(crate) fn build_runtime_interval_seasonality_factors_from_components(
    input: RuntimeIntervalSeasonalityFactorsBuildInput<'_>,
) -> Result<Vec<f64>, String> {
    let calendar_definition = input
        .calendar_registry
        .get_required(input.exchange_code)
        .map_err(|error| error.to_string())?;

    let timezone: Tz = calendar_definition.timezone_name().parse().map_err(|_| {
        format!(
            "exchange `{}` has invalid timezone name `{}`",
            input.exchange_code,
            calendar_definition.timezone_name()
        )
    })?;

    let output =
        materialize_fourier_business_time_clock(FourierBusinessTimeClockMaterializationInput {
            calendar_definition,
            timezone,
            model: input.clock_model,
            range_start_utc_nanos: input.range_start_utc_nanos,
            range_end_utc_nanos: input.range_end_utc_nanos,
            interval_duration_nanos: input.interval_duration_nanos,
        })
        .map_err(|error| error.to_string())?;

    Ok(output.business_time_factors)
}

pub(super) fn materialize_event_rates(
    run_spec: &CalibrationRunSpec,
    timestamp_utc_nanos: &[i64],
    clock_model: &FourierBusinessTimeClockModel,
    timezone: Tz,
) -> Result<Vec<f64>, Box<SeasonalityStageExecutionReport>> {
    let mut event_rates = Vec::with_capacity(timestamp_utc_nanos.len());

    for &timestamp_utc_nanos in timestamp_utc_nanos {
        let Some(timestamp_utc) = utc_datetime_from_nanos(timestamp_utc_nanos) else {
            return Err(Box::new(build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::EventRateMaterializationFailed {
                    error_message: format!(
                        "timestamp {timestamp_utc_nanos} is outside the UTC nanosecond range"
                    ),
                },
            )));
        };

        let timestamp_local = timestamp_utc.with_timezone(&timezone);

        let minute_of_day = usize::try_from(timestamp_local.hour())
            .ok()
            .and_then(|hour| hour.checked_mul(MINUTES_PER_HOUR))
            .and_then(|minute_prefix| {
                usize::try_from(timestamp_local.minute())
                    .ok()
                    .and_then(|minute| minute_prefix.checked_add(minute))
            })
            .ok_or_else(|| {
                Box::new(build_seasonality_stage_failure_report(
                    run_spec,
                    SeasonalityStageExecutionFailure::EventRateMaterializationFailed {
                        error_message: "local minute conversion overflowed".to_string(),
                    },
                ))
            })?;

        let rate = clock_model
            .rate_at_local_minute(timestamp_local.date_naive(), minute_of_day)
            .ok_or_else(|| {
                Box::new(build_seasonality_stage_failure_report(
                    run_spec,
                    SeasonalityStageExecutionFailure::EventRateMaterializationFailed {
                        error_message: "business-time clock produced an invalid event rate"
                            .to_string(),
                    },
                ))
            })?;

        event_rates.push(rate);
    }

    Ok(event_rates)
}

pub(super) fn materialize_open_event_gap_exposures(
    run_spec: &CalibrationRunSpec,
    preprocessed: &TimestampEventPreprocessingOutput,
    calendar_definition: &ExchangeCalendarDefinition,
    clock_model: &FourierBusinessTimeClockModel,
    timezone: Tz,
) -> Result<Vec<f64>, Box<SeasonalityStageExecutionReport>> {
    if preprocessed.market_state_codes.len() != preprocessed.timestamp_utc_nanos.len() {
        return Err(Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::EventRateMaterializationFailed {
                error_message: "market state code count must match timestamp event count"
                    .to_string(),
            },
        )));
    }

    let mut interval_start_utc_nanos = Vec::with_capacity(preprocessed.timestamp_utc_nanos.len());
    let mut interval_end_utc_nanos = Vec::with_capacity(preprocessed.timestamp_utc_nanos.len());
    let mut previous_open_timestamp_utc_nanos = None;

    for (timestamp_utc_nanos, market_state_code) in preprocessed
        .timestamp_utc_nanos
        .iter()
        .copied()
        .zip(preprocessed.market_state_codes.iter().copied())
    {
        if market_state_code == OPEN_MARKET_STATE_CODE {
            interval_start_utc_nanos
                .push(previous_open_timestamp_utc_nanos.unwrap_or(timestamp_utc_nanos));

            interval_end_utc_nanos.push(timestamp_utc_nanos);

            previous_open_timestamp_utc_nanos = Some(timestamp_utc_nanos);
        } else {
            interval_start_utc_nanos.push(timestamp_utc_nanos);
            interval_end_utc_nanos.push(timestamp_utc_nanos);
        }
    }

    let output = integrate_fourier_business_time_clock_ordered_interval_exposures(
        FourierBusinessTimeClockOrderedIntervalExposureInput {
            calendar_definition,
            timezone,
            model: clock_model,
            interval_start_utc_nanos: interval_start_utc_nanos.as_slice(),
            interval_end_utc_nanos: interval_end_utc_nanos.as_slice(),
        },
    )
    .map_err(|source| {
        Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::EventRateMaterializationFailed {
                error_message: source.to_string(),
            },
        ))
    })?;

    Ok(output
        .business_time_exposure_seconds
        .into_iter()
        .map(|exposure_seconds| exposure_seconds / SECONDS_PER_HOUR)
        .collect())
}

pub(super) fn exchange_local_minute_of_week(
    run_spec: &CalibrationRunSpec,
    preprocessed: &TimestampEventPreprocessingOutput,
) -> Result<Vec<u16>, Box<SeasonalityStageExecutionReport>> {
    preprocessed
        .exchange_local_day_of_week
        .iter()
        .zip(preprocessed.exchange_local_minute_of_day.iter())
        .map(|(day_of_week, minute_of_day)| {
            let day_position = usize::from(day_of_week.saturating_sub(1));

            let minute_of_week = day_position
                .checked_mul(MINUTES_PER_DAY)
                .and_then(|value| value.checked_add(usize::from(*minute_of_day)))
                .ok_or_else(|| {
                    Box::new(build_seasonality_stage_failure_report(
                        run_spec,
                        SeasonalityStageExecutionFailure::EventRateMaterializationFailed {
                            error_message: "minute-of-week conversion overflowed".to_string(),
                        },
                    ))
                })?;

            u16::try_from(minute_of_week).map_err(|_| {
                Box::new(build_seasonality_stage_failure_report(
                    run_spec,
                    SeasonalityStageExecutionFailure::EventRateMaterializationFailed {
                        error_message: "minute-of-week does not fit in u16".to_string(),
                    },
                ))
            })
        })
        .collect()
}

pub(super) fn parse_preprocessed_timezone(
    run_spec: &CalibrationRunSpec,
    preprocessed: &TimestampEventPreprocessingOutput,
) -> Result<Tz, Box<SeasonalityStageExecutionReport>> {
    preprocessed.exchange_timezone_name.parse().map_err(|_| {
        Box::new(build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::CalendarResolutionFailed {
                error_message: format!(
                    "exchange `{}` has invalid timezone name `{}`",
                    preprocessed.exchange_code, preprocessed.exchange_timezone_name
                ),
            },
        ))
    })
}
