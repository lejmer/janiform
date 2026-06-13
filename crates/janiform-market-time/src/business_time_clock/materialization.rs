use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono_tz::Tz;
use janiform_foundations::time::NANOS_PER_SECOND;
use serde::Serialize;

use super::FourierBusinessTimeClockModel;
use super::exposure::{
    BusinessTimeClockExposureBuildInput, BusinessTimeClockExposureError,
    build_business_time_clock_exposure_rows,
};
use crate::calendar::ExchangeCalendarDefinition;

#[derive(Debug, Clone, PartialEq)]
pub struct FourierBusinessTimeClockMaterializationInput<'a> {
    pub calendar_definition: &'a ExchangeCalendarDefinition,
    pub timezone: Tz,
    pub model: &'a FourierBusinessTimeClockModel,
    pub range_start_utc_nanos: i64,
    pub range_end_utc_nanos: i64,
    pub interval_duration_nanos: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FourierBusinessTimeClockMaterializationOutput {
    pub interval_start_utc_nanos: Vec<i64>,
    pub business_time_factors: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FourierBusinessTimeClockMaterializationError {
    InvalidModel {
        reason: String,
    },
    NonPositiveIntervalDuration,
    IntervalGridOverflow,
    ExposureAssemblyFailed {
        source: BusinessTimeClockExposureError,
    },
    InvalidRate,
    InvalidNormalization,
}

impl Display for FourierBusinessTimeClockMaterializationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidModel { reason } => {
                write!(formatter, "business-time clock model is invalid: {reason}")
            }
            Self::NonPositiveIntervalDuration => {
                formatter.write_str("business-time clock materialization requires a positive interval duration")
            }
            Self::IntervalGridOverflow => {
                formatter.write_str("business-time clock materialization overflowed the interval grid")
            }
            Self::ExposureAssemblyFailed { source } => {
                write!(formatter, "business-time clock exposure assembly failed: {source}")
            }
            Self::InvalidRate => {
                formatter.write_str("business-time clock materialization produced an invalid rate")
            }
            Self::InvalidNormalization => {
                formatter.write_str("business-time clock materialization requires a finite positive open-time mean rate")
            }
        }
    }
}

impl Error for FourierBusinessTimeClockMaterializationError {}

pub fn materialize_fourier_business_time_clock(
    input: FourierBusinessTimeClockMaterializationInput<'_>,
) -> Result<
    FourierBusinessTimeClockMaterializationOutput,
    FourierBusinessTimeClockMaterializationError,
> {
    input
        .model
        .validate()
        .map_err(|reason| FourierBusinessTimeClockMaterializationError::InvalidModel { reason })?;

    if input.range_end_utc_nanos <= input.range_start_utc_nanos {
        return Ok(FourierBusinessTimeClockMaterializationOutput {
            interval_start_utc_nanos: Vec::new(),
            business_time_factors: Vec::new(),
        });
    }
    if input.interval_duration_nanos <= 0 {
        return Err(FourierBusinessTimeClockMaterializationError::NonPositiveIntervalDuration);
    }

    let empty_event_counts = BTreeMap::new();

    let mut interval_contributions = Vec::new();
    let mut interval_start_utc_nanos = input.range_start_utc_nanos;

    while interval_start_utc_nanos < input.range_end_utc_nanos {
        let interval_end_utc_nanos = interval_start_utc_nanos
            .checked_add(input.interval_duration_nanos)
            .ok_or(FourierBusinessTimeClockMaterializationError::IntervalGridOverflow)?
            .min(input.range_end_utc_nanos);

        interval_contributions.push(materialize_interval(IntervalMaterializationInput {
            calendar_definition: input.calendar_definition,
            timezone: input.timezone,
            model: input.model,
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            empty_event_counts: &empty_event_counts,
        })?);

        interval_start_utc_nanos = interval_start_utc_nanos
            .checked_add(input.interval_duration_nanos)
            .ok_or(FourierBusinessTimeClockMaterializationError::IntervalGridOverflow)?;
    }

    let business_time_factors =
        open_time_mean_renormalize_interval_factor_grids(interval_contributions.as_slice())?;

    let interval_start_utc_nanos = interval_contributions
        .iter()
        .map(|contribution| contribution.interval_start_utc_nanos)
        .collect();

    Ok(FourierBusinessTimeClockMaterializationOutput {
        interval_start_utc_nanos,
        business_time_factors,
    })
}

struct IntervalMaterializationInput<'a> {
    calendar_definition: &'a ExchangeCalendarDefinition,
    timezone: Tz,
    model: &'a FourierBusinessTimeClockModel,
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    empty_event_counts: &'a BTreeMap<i64, usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct IntervalContribution {
    interval_start_utc_nanos: i64,
    interval_duration_seconds: f64,
    open_exposure_seconds: f64,
    weighted_rate_seconds: f64,
}

fn materialize_interval(
    input: IntervalMaterializationInput<'_>,
) -> Result<IntervalContribution, FourierBusinessTimeClockMaterializationError> {
    let exposure_rows =
        build_business_time_clock_exposure_rows(BusinessTimeClockExposureBuildInput {
            calendar_definition: input.calendar_definition,
            timezone: input.timezone,
            range_start_utc_nanos: input.interval_start_utc_nanos,
            range_end_utc_nanos: input.interval_end_utc_nanos,
            event_counts_by_minute_start_utc_nanos: input.empty_event_counts,
        })
        .map_err(|source| {
            FourierBusinessTimeClockMaterializationError::ExposureAssemblyFailed { source }
        })?;

    let mut open_exposure_seconds = 0.0;
    let mut weighted_rate_seconds = 0.0;

    for row in exposure_rows {
        let rate = input
            .model
            .rate_at_phases(row.phases)
            .ok_or(FourierBusinessTimeClockMaterializationError::InvalidRate)?;

        open_exposure_seconds += row.exposure_seconds;
        weighted_rate_seconds += row.exposure_seconds * rate;

        if !open_exposure_seconds.is_finite() || !weighted_rate_seconds.is_finite() {
            return Err(FourierBusinessTimeClockMaterializationError::InvalidRate);
        }
    }

    let interval_duration_seconds = (input.interval_end_utc_nanos - input.interval_start_utc_nanos)
        as f64
        / NANOS_PER_SECOND as f64;
    if !interval_duration_seconds.is_finite() || interval_duration_seconds <= 0.0 {
        return Err(FourierBusinessTimeClockMaterializationError::InvalidRate);
    }

    Ok(IntervalContribution {
        interval_start_utc_nanos: input.interval_start_utc_nanos,
        interval_duration_seconds,
        open_exposure_seconds,
        weighted_rate_seconds,
    })
}

fn open_time_mean_renormalize_interval_factor_grids(
    interval_contributions: &[IntervalContribution],
) -> Result<Vec<f64>, FourierBusinessTimeClockMaterializationError> {
    let total_open_exposure_seconds = interval_contributions
        .iter()
        .map(|contribution| contribution.open_exposure_seconds)
        .sum::<f64>();

    if total_open_exposure_seconds == 0.0 {
        return Ok(vec![0.0; interval_contributions.len()]);
    }

    if !total_open_exposure_seconds.is_finite() || total_open_exposure_seconds < 0.0 {
        return Err(FourierBusinessTimeClockMaterializationError::InvalidNormalization);
    }

    let total_weighted_rate_seconds = interval_contributions
        .iter()
        .map(|contribution| contribution.weighted_rate_seconds)
        .sum::<f64>();

    let normalization = total_weighted_rate_seconds / total_open_exposure_seconds;
    if !normalization.is_finite() || normalization <= 0.0 {
        return Err(FourierBusinessTimeClockMaterializationError::InvalidNormalization);
    }

    interval_contributions
        .iter()
        .map(|contribution| {
            let factor = contribution.weighted_rate_seconds
                / contribution.interval_duration_seconds
                / normalization;
            if !factor.is_finite() || factor < 0.0 {
                return Err(FourierBusinessTimeClockMaterializationError::InvalidRate);
            }

            Ok(factor)
        })
        .collect()
}
