use std::error::Error;
use std::fmt::{Display, Formatter};

pub use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsRiskIntervalFactorsError, SufficientStatisticsRiskModelInputsError,
};

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskRunRequestError {
    InvalidHorizonIntervalCount {
        value: usize,
    },
    InvalidIntervalDelta {
        value: f64,
    },
    InvalidModelInputs {
        source: SufficientStatisticsRiskModelInputsError,
    },
    InvalidIntervalFactors {
        source: SufficientStatisticsRiskIntervalFactorsError,
    },
    IntervalDeltaFactorMismatch {
        requested_interval_delta: f64,
        available_factor_interval_delta: f64,
    },
    HorizonExceedsAvailableIntervals {
        requested_interval_count: usize,
        available_interval_count: usize,
    },
}

impl Display for SufficientStatisticsRiskRunRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be strictly positive, found {value}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidModelInputs { source } => {
                write!(formatter, "model_inputs payload is invalid: {source}")
            }
            Self::InvalidIntervalFactors { source } => {
                write!(formatter, "interval_factors payload is invalid: {source}")
            }
            Self::IntervalDeltaFactorMismatch {
                requested_interval_delta,
                available_factor_interval_delta,
            } => write!(
                formatter,
                "interval_delta ({requested_interval_delta}) must match interval_factors.interval_delta ({available_factor_interval_delta}) under the explicit-factor policy; implicit factor resampling is unsupported"
            ),
            Self::HorizonExceedsAvailableIntervals {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "horizon_interval_count ({requested_interval_count}) exceeds available interval_factors entries ({available_interval_count})"
            ),
        }
    }
}

impl Error for SufficientStatisticsRiskRunRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidModelInputs { source } => Some(source),
            Self::InvalidIntervalFactors { source } => Some(source),
            _ => None,
        }
    }
}
