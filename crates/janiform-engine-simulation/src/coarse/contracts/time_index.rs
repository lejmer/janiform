use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::time::{
    DurationHoursToNanosError, duration_hours_to_nanos, utc_datetime_from_nanos,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;

#[derive(Debug, Clone, PartialEq)]
pub enum IntervalDeltaGridError {
    NonPositiveOrNonFinite {
        value: f64,
    },
    NotRepresentableAsUtcNanoseconds {
        interval_delta: f64,
        implied_interval_duration_nanos: f64,
    },
}

impl Display for IntervalDeltaGridError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonPositiveOrNonFinite { value } => write!(
                formatter,
                "interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::NotRepresentableAsUtcNanoseconds {
                interval_delta,
                implied_interval_duration_nanos,
            } => write!(
                formatter,
                "interval_delta ({interval_delta}) must map to an exact fixed-width UTC nanosecond duration, found implied duration {implied_interval_duration_nanos}"
            ),
        }
    }
}

impl Error for IntervalDeltaGridError {}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationTimeIndexError {
    InvalidIntervalDelta {
        source: IntervalDeltaGridError,
    },
    IntervalCountOverflow {
        value: usize,
    },
    EndTimeOverflow {
        start_time_utc_nanos: i64,
        interval_delta_nanos: i64,
        horizon_interval_count: usize,
    },
    InvalidEndTimeUtcNanos {
        value: i64,
    },
}

impl Display for CoarseSimulationTimeIndexError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIntervalDelta { source } => {
                write!(formatter, "interval_delta is invalid: {source}")
            }
            Self::IntervalCountOverflow { value } => write!(
                formatter,
                "horizon_interval_count ({value}) cannot be represented in the coarse UTC interval grid"
            ),
            Self::EndTimeOverflow {
                start_time_utc_nanos,
                interval_delta_nanos,
                horizon_interval_count,
            } => write!(
                formatter,
                "start_time_utc_nanos ({start_time_utc_nanos}) with interval_delta_nanos ({interval_delta_nanos}) and horizon_interval_count ({horizon_interval_count}) overflowed the UTC time index"
            ),
            Self::InvalidEndTimeUtcNanos { value } => write!(
                formatter,
                "derived end_time_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
        }
    }
}

impl Error for CoarseSimulationTimeIndexError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidIntervalDelta { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationIntervalBoundsError {
    InvalidStartTimeUtcNanos {
        value: i64,
    },
    InvalidEndTimeUtcNanos {
        value: i64,
    },
    NonIncreasingBounds {
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
}

impl Display for CoarseSimulationIntervalBoundsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStartTimeUtcNanos { value } => write!(
                formatter,
                "interval_start_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::InvalidEndTimeUtcNanos { value } => write!(
                formatter,
                "interval_end_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::NonIncreasingBounds {
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "interval_start_utc_nanos ({interval_start_utc_nanos}) must be strictly smaller than interval_end_utc_nanos ({interval_end_utc_nanos})"
            ),
        }
    }
}

impl Error for CoarseSimulationIntervalBoundsError {}

#[derive(Debug, Clone, PartialEq)]
pub struct ExactStringValidationError {
    pub(super) field_path: &'static str,
    pub(super) expected: String,
    pub(super) actual: String,
}

impl Display for ExactStringValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} must be {}, found {}",
            self.field_path, self.expected, self.actual
        )
    }
}

impl Error for ExactStringValidationError {}

pub(super) fn interval_delta_to_nanos(interval_delta: f64) -> Result<i64, IntervalDeltaGridError> {
    duration_hours_to_nanos(interval_delta).map_err(|source| match source {
        DurationHoursToNanosError::NonPositiveOrNonFinite { value } => {
            IntervalDeltaGridError::NonPositiveOrNonFinite { value }
        }
        DurationHoursToNanosError::NotRepresentableAsUtcNanoseconds {
            duration_hours,
            implied_duration_nanos,
        } => IntervalDeltaGridError::NotRepresentableAsUtcNanoseconds {
            interval_delta: duration_hours,
            implied_interval_duration_nanos: implied_duration_nanos,
        },
    })
}

pub(super) fn validate_horizon_end_timestamp(
    start_time_utc_nanos: i64,
    interval_delta_nanos: i64,
    horizon_interval_count: usize,
) -> Result<(), CoarseSimulationTimeIndexError> {
    compute_horizon_end_time_utc_nanos(
        start_time_utc_nanos,
        interval_delta_nanos,
        horizon_interval_count,
    )?;

    Ok(())
}

pub(super) fn compute_horizon_end_time_utc_nanos(
    start_time_utc_nanos: i64,
    interval_delta_nanos: i64,
    horizon_interval_count: usize,
) -> Result<i64, CoarseSimulationTimeIndexError> {
    let interval_count_i64 = i64::try_from(horizon_interval_count).map_err(|_| {
        CoarseSimulationTimeIndexError::IntervalCountOverflow {
            value: horizon_interval_count,
        }
    })?;

    let total_duration_nanos = interval_delta_nanos.checked_mul(interval_count_i64).ok_or(
        CoarseSimulationTimeIndexError::EndTimeOverflow {
            start_time_utc_nanos,
            interval_delta_nanos,
            horizon_interval_count,
        },
    )?;

    let end_time_utc_nanos = start_time_utc_nanos
        .checked_add(total_duration_nanos)
        .ok_or(CoarseSimulationTimeIndexError::EndTimeOverflow {
            start_time_utc_nanos,
            interval_delta_nanos,
            horizon_interval_count,
        })?;

    if utc_datetime_from_nanos(end_time_utc_nanos).is_none() {
        return Err(CoarseSimulationTimeIndexError::InvalidEndTimeUtcNanos {
            value: end_time_utc_nanos,
        });
    }

    Ok(end_time_utc_nanos)
}

pub(super) fn validate_interval_bounds(
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> Result<(), CoarseSimulationIntervalBoundsError> {
    if utc_datetime_from_nanos(interval_start_utc_nanos).is_none() {
        return Err(
            CoarseSimulationIntervalBoundsError::InvalidStartTimeUtcNanos {
                value: interval_start_utc_nanos,
            },
        );
    }

    if utc_datetime_from_nanos(interval_end_utc_nanos).is_none() {
        return Err(
            CoarseSimulationIntervalBoundsError::InvalidEndTimeUtcNanos {
                value: interval_end_utc_nanos,
            },
        );
    }

    if interval_end_utc_nanos <= interval_start_utc_nanos {
        return Err(CoarseSimulationIntervalBoundsError::NonIncreasingBounds {
            interval_start_utc_nanos,
            interval_end_utc_nanos,
        });
    }

    Ok(())
}

pub(super) fn validate_exact_string(
    field_path: &'static str,
    actual: &str,
    expected: &'static str,
) -> Result<(), ExactStringValidationError> {
    if actual != expected {
        return Err(ExactStringValidationError {
            field_path,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }

    Ok(())
}

pub(super) fn validate_excitation_state(
    state: HawkesExcitationState,
) -> Result<(), (&'static str, f64)> {
    let fields = [
        ("y_plus_plus", state.y_plus_plus),
        ("y_plus_minus", state.y_plus_minus),
        ("y_minus_plus", state.y_minus_plus),
        ("y_minus_minus", state.y_minus_minus),
    ];

    for (field, value) in fields {
        if !value.is_finite() || value < 0.0 {
            return Err((field, value));
        }
    }

    Ok(())
}

pub(super) fn build_string_list(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::NANOS_PER_HOUR;

    use super::{IntervalDeltaGridError, interval_delta_to_nanos};

    #[test]
    fn interval_delta_to_nanos_accepts_integer_nanosecond_grid_round_trip() {
        let interval_delta = 1_000_000_f64 / NANOS_PER_HOUR as f64;

        assert_eq!(
            interval_delta_to_nanos(interval_delta)
                .expect("integer nanosecond interval should round-trip"),
            1_000_000
        );
    }

    #[test]
    fn interval_delta_to_nanos_rejects_fractional_nanosecond_grid() {
        let interval_delta = 1.0 / 7.0;

        let error = interval_delta_to_nanos(interval_delta)
            .expect_err("fractional nanosecond interval should fail");

        assert!(matches!(
            error,
            IntervalDeltaGridError::NotRepresentableAsUtcNanoseconds { .. }
        ));
    }
}
