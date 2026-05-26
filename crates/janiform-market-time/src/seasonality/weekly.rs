use std::error::Error;
use std::fmt::{Display, Formatter};

use super::discrete::estimate_discrete_seasonality_by_bucket;
use super::{
    DAYS_PER_WEEK, HOUR_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION, HOURS_PER_DAY, HOURS_PER_WEEK,
    MINUTES_PER_DAY, MINUTES_PER_HOUR,
};

/// Configuration for local hour-of-week seasonality estimation from timestamp event density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HourOfWeekSeasonalityEstimatorSettings {
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
}

impl Default for HourOfWeekSeasonalityEstimatorSettings {
    fn default() -> Self {
        Self {
            minimum_bucket_events: 1,
            pseudocount: 1e-6,
        }
    }
}

/// Diagnostics emitted by local hour-of-week seasonality estimation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HourOfWeekSeasonalityDiagnostics {
    pub input_event_count: usize,
    pub bucket_count: usize,
    pub zero_event_bucket_count: usize,
    pub sparse_bucket_count: usize,
    pub fallback_bucket_count: usize,
}

/// Deterministic nonparametric local hour-of-week seasonality estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct HourOfWeekSeasonalityEstimateOutput {
    pub schema_version: &'static str,
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
    pub hour_of_week_index: Vec<u16>,
    pub event_count_by_hour_of_week: Vec<u64>,
    pub sparse_bucket_flags: Vec<u8>,
    pub fallback_bucket_flags: Vec<u8>,
    pub hour_of_week_log_component: Vec<f64>,
    pub hour_of_week_factor: Vec<f64>,
    pub diagnostics: HourOfWeekSeasonalityDiagnostics,
}

/// Estimate a nonparametric local hour-of-week seasonality component.
pub fn estimate_hour_of_week_seasonality(
    exchange_local_day_of_week: &[u8],
    exchange_local_minute_of_day: &[u16],
    settings: HourOfWeekSeasonalityEstimatorSettings,
) -> Result<HourOfWeekSeasonalityEstimateOutput, HourOfWeekSeasonalityEstimationError> {
    if exchange_local_day_of_week.is_empty() {
        return Err(HourOfWeekSeasonalityEstimationError::EmptyInputEvents);
    }

    if exchange_local_day_of_week.len() != exchange_local_minute_of_day.len() {
        return Err(
            HourOfWeekSeasonalityEstimationError::MismatchedInputLengths {
                day_of_week_count: exchange_local_day_of_week.len(),
                minute_of_day_count: exchange_local_minute_of_day.len(),
            },
        );
    }

    if settings.minimum_bucket_events < 1 {
        return Err(
            HourOfWeekSeasonalityEstimationError::InvalidMinimumBucketEvents {
                minimum_bucket_events: settings.minimum_bucket_events,
            },
        );
    }

    if !settings.pseudocount.is_finite() || settings.pseudocount <= 0.0 {
        return Err(HourOfWeekSeasonalityEstimationError::InvalidPseudocount {
            pseudocount: settings.pseudocount,
        });
    }

    let mut bucket_index_by_event: Vec<usize> =
        Vec::with_capacity(exchange_local_day_of_week.len());
    for (record_position, (day_of_week, minute_of_day)) in exchange_local_day_of_week
        .iter()
        .copied()
        .zip(exchange_local_minute_of_day.iter().copied())
        .enumerate()
    {
        if !(1..=DAYS_PER_WEEK as u8).contains(&day_of_week) {
            return Err(HourOfWeekSeasonalityEstimationError::InvalidDayOfWeek {
                record_position,
                day_of_week,
            });
        }

        if usize::from(minute_of_day) >= MINUTES_PER_DAY {
            return Err(HourOfWeekSeasonalityEstimationError::InvalidMinuteOfDay {
                record_position,
                minute_of_day,
            });
        }

        let hour_of_day = usize::from(minute_of_day) / MINUTES_PER_HOUR;
        let weekday_offset = usize::from(day_of_week - 1) * HOURS_PER_DAY;
        bucket_index_by_event.push(weekday_offset + hour_of_day);
    }

    let estimate_by_bucket = estimate_discrete_seasonality_by_bucket(
        &bucket_index_by_event,
        HOURS_PER_WEEK,
        settings.minimum_bucket_events,
        settings.pseudocount,
    )
    .map_err(|_| HourOfWeekSeasonalityEstimationError::UnstableNormalization)?;

    Ok(HourOfWeekSeasonalityEstimateOutput {
        schema_version: HOUR_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
        minimum_bucket_events: settings.minimum_bucket_events,
        pseudocount: settings.pseudocount,
        hour_of_week_index: (0..HOURS_PER_WEEK)
            .map(|hour_of_week| hour_of_week as u16)
            .collect(),
        event_count_by_hour_of_week: estimate_by_bucket.event_count_by_bucket,
        sparse_bucket_flags: estimate_by_bucket.sparse_bucket_flags,
        fallback_bucket_flags: estimate_by_bucket.fallback_bucket_flags,
        hour_of_week_log_component: estimate_by_bucket.log_component_by_bucket,
        hour_of_week_factor: estimate_by_bucket.factor_by_bucket,
        diagnostics: HourOfWeekSeasonalityDiagnostics {
            input_event_count: exchange_local_day_of_week.len(),
            bucket_count: HOURS_PER_WEEK,
            zero_event_bucket_count: estimate_by_bucket.zero_event_bucket_count,
            sparse_bucket_count: estimate_by_bucket.sparse_bucket_count,
            fallback_bucket_count: estimate_by_bucket.fallback_bucket_count,
        },
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum HourOfWeekSeasonalityEstimationError {
    EmptyInputEvents,
    MismatchedInputLengths {
        day_of_week_count: usize,
        minute_of_day_count: usize,
    },
    InvalidMinimumBucketEvents {
        minimum_bucket_events: usize,
    },
    InvalidPseudocount {
        pseudocount: f64,
    },
    InvalidDayOfWeek {
        record_position: usize,
        day_of_week: u8,
    },
    InvalidMinuteOfDay {
        record_position: usize,
        minute_of_day: u16,
    },
    UnstableNormalization,
}

impl Display for HourOfWeekSeasonalityEstimationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInputEvents => {
                write!(
                    formatter,
                    "hour-of-week seasonality estimation requires at least one accepted event"
                )
            }
            Self::MismatchedInputLengths {
                day_of_week_count,
                minute_of_day_count,
            } => write!(
                formatter,
                "hour-of-week seasonality estimation requires equal day-of-week and minute-of-day lengths, received {day_of_week_count} and {minute_of_day_count}"
            ),
            Self::InvalidMinimumBucketEvents {
                minimum_bucket_events,
            } => write!(
                formatter,
                "minimum_bucket_events must be at least 1, received {minimum_bucket_events}"
            ),
            Self::InvalidPseudocount { pseudocount } => write!(
                formatter,
                "pseudocount must be a finite positive value, received {pseudocount}"
            ),
            Self::InvalidDayOfWeek {
                record_position,
                day_of_week,
            } => write!(
                formatter,
                "event record at position {record_position} has invalid local day-of-week value {day_of_week}"
            ),
            Self::InvalidMinuteOfDay {
                record_position,
                minute_of_day,
            } => write!(
                formatter,
                "event record at position {record_position} has invalid local minute-of-day value {minute_of_day}"
            ),
            Self::UnstableNormalization => write!(
                formatter,
                "hour-of-week seasonality estimation produced a non-positive normalization constant"
            ),
        }
    }
}

impl Error for HourOfWeekSeasonalityEstimationError {}
