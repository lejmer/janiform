use std::error::Error;
use std::fmt::{Display, Formatter};

use super::discrete::estimate_discrete_seasonality_by_bucket;
use super::{
    DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION, DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    DAYS_PER_WEEK, DAYS_PER_YEAR,
};

/// Configuration for day-of-week seasonality estimation from timestamp event density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayOfWeekSeasonalityEstimatorSettings {
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
}

impl Default for DayOfWeekSeasonalityEstimatorSettings {
    fn default() -> Self {
        Self {
            minimum_bucket_events: 1,
            pseudocount: 1e-6,
        }
    }
}

/// Diagnostics emitted by day-of-week seasonality estimation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayOfWeekSeasonalityDiagnostics {
    pub input_event_count: usize,
    pub bucket_count: usize,
    pub zero_event_bucket_count: usize,
    pub sparse_bucket_count: usize,
    pub fallback_bucket_count: usize,
}

/// Deterministic nonparametric day-of-week seasonality estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct DayOfWeekSeasonalityEstimateOutput {
    pub schema_version: &'static str,
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
    pub day_of_week_number: Vec<u8>,
    pub event_count_by_day_of_week: Vec<u64>,
    pub sparse_bucket_flags: Vec<u8>,
    pub fallback_bucket_flags: Vec<u8>,
    pub day_of_week_log_component: Vec<f64>,
    pub day_of_week_factor: Vec<f64>,
    pub diagnostics: DayOfWeekSeasonalityDiagnostics,
}

/// Estimate a nonparametric day-of-week seasonality component.
pub fn estimate_day_of_week_seasonality(
    exchange_local_day_of_week: &[u8],
    settings: DayOfWeekSeasonalityEstimatorSettings,
) -> Result<DayOfWeekSeasonalityEstimateOutput, DayOfWeekSeasonalityEstimationError> {
    if exchange_local_day_of_week.is_empty() {
        return Err(DayOfWeekSeasonalityEstimationError::EmptyInputEvents);
    }

    if settings.minimum_bucket_events < 1 {
        return Err(
            DayOfWeekSeasonalityEstimationError::InvalidMinimumBucketEvents {
                minimum_bucket_events: settings.minimum_bucket_events,
            },
        );
    }

    if !settings.pseudocount.is_finite() || settings.pseudocount <= 0.0 {
        return Err(DayOfWeekSeasonalityEstimationError::InvalidPseudocount {
            pseudocount: settings.pseudocount,
        });
    }

    let mut bucket_index_by_event: Vec<usize> =
        Vec::with_capacity(exchange_local_day_of_week.len());
    for (record_position, day_of_week) in exchange_local_day_of_week.iter().copied().enumerate() {
        if !(1..=7).contains(&day_of_week) {
            return Err(DayOfWeekSeasonalityEstimationError::InvalidDayOfWeek {
                record_position,
                day_of_week,
            });
        }

        bucket_index_by_event.push((day_of_week - 1) as usize);
    }

    let estimate_by_bucket = estimate_discrete_seasonality_by_bucket(
        &bucket_index_by_event,
        DAYS_PER_WEEK,
        settings.minimum_bucket_events,
        settings.pseudocount,
    )
    .map_err(|_| DayOfWeekSeasonalityEstimationError::UnstableNormalization)?;

    Ok(DayOfWeekSeasonalityEstimateOutput {
        schema_version: DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
        minimum_bucket_events: settings.minimum_bucket_events,
        pseudocount: settings.pseudocount,
        day_of_week_number: (1..=DAYS_PER_WEEK)
            .map(|day_of_week| day_of_week as u8)
            .collect(),
        event_count_by_day_of_week: estimate_by_bucket.event_count_by_bucket,
        sparse_bucket_flags: estimate_by_bucket.sparse_bucket_flags,
        fallback_bucket_flags: estimate_by_bucket.fallback_bucket_flags,
        day_of_week_log_component: estimate_by_bucket.log_component_by_bucket,
        day_of_week_factor: estimate_by_bucket.factor_by_bucket,
        diagnostics: DayOfWeekSeasonalityDiagnostics {
            input_event_count: exchange_local_day_of_week.len(),
            bucket_count: DAYS_PER_WEEK,
            zero_event_bucket_count: estimate_by_bucket.zero_event_bucket_count,
            sparse_bucket_count: estimate_by_bucket.sparse_bucket_count,
            fallback_bucket_count: estimate_by_bucket.fallback_bucket_count,
        },
    })
}

/// Configuration for day-of-year seasonality estimation from timestamp event density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayOfYearSeasonalityEstimatorSettings {
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
}

impl Default for DayOfYearSeasonalityEstimatorSettings {
    fn default() -> Self {
        Self {
            minimum_bucket_events: 1,
            pseudocount: 1e-6,
        }
    }
}

/// Diagnostics emitted by day-of-year seasonality estimation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayOfYearSeasonalityDiagnostics {
    pub input_event_count: usize,
    pub bucket_count: usize,
    pub zero_event_bucket_count: usize,
    pub sparse_bucket_count: usize,
    pub fallback_bucket_count: usize,
}

/// Deterministic nonparametric day-of-year seasonality estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct DayOfYearSeasonalityEstimateOutput {
    pub schema_version: &'static str,
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
    pub day_of_year_number: Vec<u16>,
    pub event_count_by_day_of_year: Vec<u64>,
    pub sparse_bucket_flags: Vec<u8>,
    pub fallback_bucket_flags: Vec<u8>,
    pub day_of_year_log_component: Vec<f64>,
    pub day_of_year_factor: Vec<f64>,
    pub diagnostics: DayOfYearSeasonalityDiagnostics,
}

/// Estimate a nonparametric day-of-year seasonality component.
pub fn estimate_day_of_year_seasonality(
    exchange_local_day_of_year: &[u16],
    settings: DayOfYearSeasonalityEstimatorSettings,
) -> Result<DayOfYearSeasonalityEstimateOutput, DayOfYearSeasonalityEstimationError> {
    if exchange_local_day_of_year.is_empty() {
        return Err(DayOfYearSeasonalityEstimationError::EmptyInputEvents);
    }

    if settings.minimum_bucket_events < 1 {
        return Err(
            DayOfYearSeasonalityEstimationError::InvalidMinimumBucketEvents {
                minimum_bucket_events: settings.minimum_bucket_events,
            },
        );
    }

    if !settings.pseudocount.is_finite() || settings.pseudocount <= 0.0 {
        return Err(DayOfYearSeasonalityEstimationError::InvalidPseudocount {
            pseudocount: settings.pseudocount,
        });
    }

    let mut bucket_index_by_event: Vec<usize> =
        Vec::with_capacity(exchange_local_day_of_year.len());
    for (record_position, day_of_year) in exchange_local_day_of_year.iter().copied().enumerate() {
        if !(1..=366).contains(&day_of_year) {
            return Err(DayOfYearSeasonalityEstimationError::InvalidDayOfYear {
                record_position,
                day_of_year,
            });
        }

        bucket_index_by_event.push((day_of_year - 1) as usize);
    }

    let estimate_by_bucket = estimate_discrete_seasonality_by_bucket(
        &bucket_index_by_event,
        DAYS_PER_YEAR,
        settings.minimum_bucket_events,
        settings.pseudocount,
    )
    .map_err(|_| DayOfYearSeasonalityEstimationError::UnstableNormalization)?;

    Ok(DayOfYearSeasonalityEstimateOutput {
        schema_version: DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
        minimum_bucket_events: settings.minimum_bucket_events,
        pseudocount: settings.pseudocount,
        day_of_year_number: (1..=DAYS_PER_YEAR)
            .map(|day_of_year| day_of_year as u16)
            .collect(),
        event_count_by_day_of_year: estimate_by_bucket.event_count_by_bucket,
        sparse_bucket_flags: estimate_by_bucket.sparse_bucket_flags,
        fallback_bucket_flags: estimate_by_bucket.fallback_bucket_flags,
        day_of_year_log_component: estimate_by_bucket.log_component_by_bucket,
        day_of_year_factor: estimate_by_bucket.factor_by_bucket,
        diagnostics: DayOfYearSeasonalityDiagnostics {
            input_event_count: exchange_local_day_of_year.len(),
            bucket_count: DAYS_PER_YEAR,
            zero_event_bucket_count: estimate_by_bucket.zero_event_bucket_count,
            sparse_bucket_count: estimate_by_bucket.sparse_bucket_count,
            fallback_bucket_count: estimate_by_bucket.fallback_bucket_count,
        },
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum DayOfWeekSeasonalityEstimationError {
    EmptyInputEvents,
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
    UnstableNormalization,
}

impl Display for DayOfWeekSeasonalityEstimationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInputEvents => {
                write!(
                    formatter,
                    "day-of-week seasonality estimation requires at least one accepted event"
                )
            }
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
            Self::UnstableNormalization => write!(
                formatter,
                "day-of-week seasonality estimation produced a non-positive normalization constant"
            ),
        }
    }
}

impl Error for DayOfWeekSeasonalityEstimationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum DayOfYearSeasonalityEstimationError {
    EmptyInputEvents,
    InvalidMinimumBucketEvents {
        minimum_bucket_events: usize,
    },
    InvalidPseudocount {
        pseudocount: f64,
    },
    InvalidDayOfYear {
        record_position: usize,
        day_of_year: u16,
    },
    UnstableNormalization,
}

impl Display for DayOfYearSeasonalityEstimationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInputEvents => {
                write!(
                    formatter,
                    "day-of-year seasonality estimation requires at least one accepted event"
                )
            }
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
            Self::InvalidDayOfYear {
                record_position,
                day_of_year,
            } => write!(
                formatter,
                "event record at position {record_position} has invalid local day-of-year value {day_of_year}"
            ),
            Self::UnstableNormalization => write!(
                formatter,
                "day-of-year seasonality estimation produced a non-positive normalization constant"
            ),
        }
    }
}

impl Error for DayOfYearSeasonalityEstimationError {}
