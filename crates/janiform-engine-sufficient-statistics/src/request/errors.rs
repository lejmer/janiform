use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsIntervalFactorsError, SufficientStatisticsModelInputsError,
};

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRunRequestError {
    InvalidHorizonIntervalCount {
        value: usize,
    },
    InvalidIntervalDelta {
        value: f64,
    },
    InvalidModelInputs {
        source: SufficientStatisticsModelInputsError,
    },
    InvalidIntervalFactors {
        source: SufficientStatisticsIntervalFactorsError,
    },
    IntervalDeltaFactorMismatch {
        requested_interval_delta: f64,
        available_factor_interval_delta: f64,
    },
    HorizonExceedsAvailableIntervals {
        requested_interval_count: usize,
        available_interval_count: usize,
    },
    MeanReversionRuntimeAnchorIntervalCountMismatch {
        expected_interval_count: usize,
        observed_interval_count: usize,
    },
    MeanReversionRuntimeAnchorIntervalDeltaMismatch {
        interval_index: usize,
        expected_interval_delta: f64,
        observed_interval_delta: f64,
    },
}

impl Display for SufficientStatisticsRunRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be strictly positive, found {value}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be finite, strictly positive, and map to an exact integer UTC nanosecond duration, found {value}"
            ),
            Self::InvalidModelInputs { source } => {
                write!(formatter, "model_inputs payload is invalid: {source}")
            }
            Self::InvalidIntervalFactors { source } => {
                write!(
                    formatter,
                    "interval_factor_grids payload is invalid: {source}"
                )
            }
            Self::IntervalDeltaFactorMismatch {
                requested_interval_delta,
                available_factor_interval_delta,
            } => write!(
                formatter,
                "interval_delta ({requested_interval_delta}) must match interval_factor_grids.interval_delta ({available_factor_interval_delta}) under the explicit-factor policy; implicit factor resampling is unsupported"
            ),
            Self::HorizonExceedsAvailableIntervals {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "horizon_interval_count ({requested_interval_count}) exceeds available interval_factor_grids entries ({available_interval_count})"
            ),
            Self::MeanReversionRuntimeAnchorIntervalCountMismatch {
                expected_interval_count,
                observed_interval_count,
            } => write!(
                formatter,
                "model_inputs.mean_reversion_runtime_interval_anchors.end_anchor_values must contain {expected_interval_count} entries for the requested horizon, found {observed_interval_count}"
            ),
            Self::MeanReversionRuntimeAnchorIntervalDeltaMismatch {
                interval_index,
                expected_interval_delta,
                observed_interval_delta,
            } => write!(
                formatter,
                "model_inputs.mean_reversion_runtime_interval_anchors.interval_deltas[{interval_index}] must equal interval_delta ({expected_interval_delta}), found {observed_interval_delta}"
            ),
        }
    }
}

impl Error for SufficientStatisticsRunRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidModelInputs { source } => Some(source),
            Self::InvalidIntervalFactors { source } => Some(source),
            _ => None,
        }
    }
}
