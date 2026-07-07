use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono_tz::Tz;
use janiform_foundations::time::NANOS_PER_SECOND;
use serde::Serialize;

use super::FourierBusinessTimeClockModel;
use super::exposure::{
    BusinessTimeClockExposureBuildInput, BusinessTimeClockExposureError,
    BusinessTimeClockExposureRow, build_business_time_clock_exposure_rows,
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

/// Input for integrating normalized business-time clock exposure over ordered intervals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FourierBusinessTimeClockOrderedIntervalExposureInput<'a> {
    /// Exchange calendar defining open and closed market windows.
    pub calendar_definition: &'a ExchangeCalendarDefinition,
    /// Exchange timezone used to compute clock model coordinates.
    pub timezone: Tz,
    /// Fitted Fourier business-time clock model to integrate.
    pub model: &'a FourierBusinessTimeClockModel,
    /// UTC nanosecond start timestamp for each requested interval.
    pub interval_start_utc_nanos: &'a [i64],
    /// UTC nanosecond end timestamp for each requested interval.
    pub interval_end_utc_nanos: &'a [i64],
}

/// Normalized business-time exposure for each requested interval.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FourierBusinessTimeClockOrderedIntervalExposureOutput {
    /// Integrated normalized clock exposure in seconds for each input interval.
    pub business_time_exposure_seconds: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FourierBusinessTimeClockMaterializationError {
    InvalidModel {
        reason: String,
    },
    NonPositiveIntervalDuration,
    MismatchedIntervalEndpointLengths {
        start_count: usize,
        end_count: usize,
    },
    DecreasingInterval {
        interval_position: usize,
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
    UnorderedIntervals {
        interval_position: usize,
        previous_interval_start_utc_nanos: i64,
        interval_start_utc_nanos: i64,
    },
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
            Self::MismatchedIntervalEndpointLengths {
                start_count,
                end_count,
            } => write!(
                formatter,
                "business-time clock ordered interval exposure requires matching start and end counts, found start_count={start_count} end_count={end_count}"
            ),
            Self::DecreasingInterval {
                interval_position,
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "business-time clock ordered interval exposure interval {interval_position} has end before start, found start={interval_start_utc_nanos} end={interval_end_utc_nanos}"
            ),
            Self::UnorderedIntervals {
                interval_position,
                previous_interval_start_utc_nanos,
                interval_start_utc_nanos,
            } => write!(
                formatter,
                "business-time clock ordered interval exposure intervals must be sorted by start timestamp, interval {interval_position} has start={interval_start_utc_nanos} before previous_start={previous_interval_start_utc_nanos}"
            ),
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

/// Integrate the normalized Fourier clock over ordered UTC intervals.
///
/// Intervals must be sorted by start timestamp. Zero-length intervals are
/// accepted and produce zero exposure.
pub fn integrate_fourier_business_time_clock_ordered_interval_exposures(
    input: FourierBusinessTimeClockOrderedIntervalExposureInput<'_>,
) -> Result<
    FourierBusinessTimeClockOrderedIntervalExposureOutput,
    FourierBusinessTimeClockMaterializationError,
> {
    input
        .model
        .validate()
        .map_err(|reason| FourierBusinessTimeClockMaterializationError::InvalidModel { reason })?;

    let interval_count = input.interval_start_utc_nanos.len();
    if input.interval_end_utc_nanos.len() != interval_count {
        return Err(
            FourierBusinessTimeClockMaterializationError::MismatchedIntervalEndpointLengths {
                start_count: interval_count,
                end_count: input.interval_end_utc_nanos.len(),
            },
        );
    }

    let mut output = vec![0.0; interval_count];

    let Some((range_start_utc_nanos, range_end_utc_nanos)) =
        validate_ordered_interval_ranges(&input)?
    else {
        return Ok(FourierBusinessTimeClockOrderedIntervalExposureOutput {
            business_time_exposure_seconds: output,
        });
    };

    let empty_event_counts = BTreeMap::new();

    let exposure_rows =
        build_business_time_clock_exposure_rows(BusinessTimeClockExposureBuildInput {
            calendar_definition: input.calendar_definition,
            timezone: input.timezone,
            range_start_utc_nanos,
            range_end_utc_nanos,
            event_counts_by_minute_start_utc_nanos: &empty_event_counts,
        })
        .map_err(|source| {
            FourierBusinessTimeClockMaterializationError::ExposureAssemblyFailed { source }
        })?;

    fill_ordered_interval_exposures(&input, exposure_rows.as_slice(), output.as_mut_slice())?;

    Ok(FourierBusinessTimeClockOrderedIntervalExposureOutput {
        business_time_exposure_seconds: output,
    })
}

fn validate_ordered_interval_ranges(
    input: &FourierBusinessTimeClockOrderedIntervalExposureInput<'_>,
) -> Result<Option<(i64, i64)>, FourierBusinessTimeClockMaterializationError> {
    let mut previous_interval_start_utc_nanos = None;
    let mut range_start_utc_nanos = None;
    let mut range_end_utc_nanos = None;

    for (interval_position, (interval_start_utc_nanos, interval_end_utc_nanos)) in input
        .interval_start_utc_nanos
        .iter()
        .copied()
        .zip(input.interval_end_utc_nanos.iter().copied())
        .enumerate()
    {
        if interval_end_utc_nanos < interval_start_utc_nanos {
            return Err(
                FourierBusinessTimeClockMaterializationError::DecreasingInterval {
                    interval_position,
                    interval_start_utc_nanos,
                    interval_end_utc_nanos,
                },
            );
        }

        if let Some(previous_start_utc_nanos) = previous_interval_start_utc_nanos
            && interval_start_utc_nanos < previous_start_utc_nanos
        {
            return Err(
                FourierBusinessTimeClockMaterializationError::UnorderedIntervals {
                    interval_position,
                    previous_interval_start_utc_nanos: previous_start_utc_nanos,
                    interval_start_utc_nanos,
                },
            );
        }

        previous_interval_start_utc_nanos = Some(interval_start_utc_nanos);

        if interval_start_utc_nanos == interval_end_utc_nanos {
            continue;
        }

        range_start_utc_nanos = Some(
            range_start_utc_nanos
                .map(|range_start: i64| range_start.min(interval_start_utc_nanos))
                .unwrap_or(interval_start_utc_nanos),
        );

        range_end_utc_nanos = Some(
            range_end_utc_nanos
                .map(|range_end: i64| range_end.max(interval_end_utc_nanos))
                .unwrap_or(interval_end_utc_nanos),
        );
    }

    Ok(range_start_utc_nanos.zip(range_end_utc_nanos))
}

fn fill_ordered_interval_exposures(
    input: &FourierBusinessTimeClockOrderedIntervalExposureInput<'_>,
    exposure_rows: &[BusinessTimeClockExposureRow],
    output: &mut [f64],
) -> Result<(), FourierBusinessTimeClockMaterializationError> {
    let mut exposure_row_position = 0;

    for (interval_position, (interval_start_utc_nanos, interval_end_utc_nanos)) in input
        .interval_start_utc_nanos
        .iter()
        .copied()
        .zip(input.interval_end_utc_nanos.iter().copied())
        .enumerate()
    {
        if interval_start_utc_nanos == interval_end_utc_nanos {
            continue;
        }

        while exposure_row_position < exposure_rows.len()
            && exposure_rows[exposure_row_position].interval_end_utc_nanos
                <= interval_start_utc_nanos
        {
            exposure_row_position += 1;
        }

        let exposure_seconds = integrate_ordered_interval_exposure_seconds(
            input.model,
            &exposure_rows[exposure_row_position..],
            interval_start_utc_nanos,
            interval_end_utc_nanos,
        )?;

        let Some(output_slot) = output.get_mut(interval_position) else {
            return Err(FourierBusinessTimeClockMaterializationError::InvalidRate);
        };

        *output_slot = exposure_seconds;
    }

    Ok(())
}

fn integrate_ordered_interval_exposure_seconds(
    model: &FourierBusinessTimeClockModel,
    exposure_rows: &[BusinessTimeClockExposureRow],
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> Result<f64, FourierBusinessTimeClockMaterializationError> {
    let mut weighted_rate_seconds = 0.0;

    for row in exposure_rows {
        if row.interval_start_utc_nanos >= interval_end_utc_nanos {
            break;
        }

        let overlap_start_utc_nanos = interval_start_utc_nanos.max(row.interval_start_utc_nanos);
        let overlap_end_utc_nanos = interval_end_utc_nanos.min(row.interval_end_utc_nanos);

        if overlap_start_utc_nanos >= overlap_end_utc_nanos {
            continue;
        }

        let overlap_seconds =
            (overlap_end_utc_nanos - overlap_start_utc_nanos) as f64 / NANOS_PER_SECOND as f64;
        if !overlap_seconds.is_finite() || overlap_seconds <= 0.0 {
            return Err(FourierBusinessTimeClockMaterializationError::InvalidRate);
        }

        let rate = model
            .rate_at_phases(row.phases)
            .ok_or(FourierBusinessTimeClockMaterializationError::InvalidRate)?;

        weighted_rate_seconds += overlap_seconds * rate;

        if !weighted_rate_seconds.is_finite() {
            return Err(FourierBusinessTimeClockMaterializationError::InvalidRate);
        }
    }

    Ok(weighted_rate_seconds)
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

    for row in &exposure_rows {
        open_exposure_seconds += row.exposure_seconds;

        if !open_exposure_seconds.is_finite() {
            return Err(FourierBusinessTimeClockMaterializationError::InvalidRate);
        }
    }

    let weighted_rate_seconds = integrate_ordered_interval_exposure_seconds(
        input.model,
        exposure_rows.as_slice(),
        input.interval_start_utc_nanos,
        input.interval_end_utc_nanos,
    )?;

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
