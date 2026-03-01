use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::affine_oracle::ExponentialAffineTransformEquationSpecificationError;
use crate::nig::NigSharedPlusShiftConfigurationError;

use super::constants::NEWS_INTENSITY_COMPONENT_SUM_TOLERANCE;

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskIntervalFactorsError {
    MissingIntervals,
    FieldLengthMismatch {
        field: &'static str,
        expected_length: usize,
        actual_length: usize,
    },
    InvalidSeasonalityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidKnownNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidSurpriseNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    InconsistentNewsIntensityAggregation {
        interval_index: usize,
        nu_h_total: f64,
        nu_h_known: f64,
        nu_h_surprise: f64,
    },
    InvalidIntervalDelta {
        value: f64,
    },
}

impl Display for SufficientStatisticsRiskIntervalFactorsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingIntervals => write!(
                formatter,
                "interval_factors must provide at least one interval"
            ),
            Self::FieldLengthMismatch {
                field,
                expected_length,
                actual_length,
            } => write!(
                formatter,
                "interval_factors.{field} length must match s_h length ({expected_length}), found {actual_length}"
            ),
            Self::InvalidSeasonalityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.s_h[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.nu_h[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidKnownNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.nu_h_known[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidSurpriseNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.nu_h_surprise[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InconsistentNewsIntensityAggregation {
                interval_index,
                nu_h_total,
                nu_h_known,
                nu_h_surprise,
            } => write!(
                formatter,
                "interval_factors.nu_h[{interval_index}] must match nu_h_known + nu_h_surprise within tolerance {NEWS_INTENSITY_COMPONENT_SUM_TOLERANCE}, found total={nu_h_total}, known={nu_h_known}, surprise={nu_h_surprise}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_factors.interval_delta must be finite and strictly positive when provided, found {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsRiskIntervalFactorsError {}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskModelInputsError {
    InvalidEquationSpecification {
        source: ExponentialAffineTransformEquationSpecificationError,
    },
    InvalidNigSharedPlusShiftConfiguration {
        source: NigSharedPlusShiftConfigurationError,
    },
    InvalidInitialState {
        field: &'static str,
        value: f64,
    },
}

impl Display for SufficientStatisticsRiskModelInputsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEquationSpecification { source } => {
                write!(
                    formatter,
                    "model_inputs.equation_specification is invalid: {source}"
                )
            }
            Self::InvalidNigSharedPlusShiftConfiguration { source } => write!(
                formatter,
                "model_inputs.nig_shared_plus_shift_configuration is invalid: {source}"
            ),
            Self::InvalidInitialState { field, value } => write!(
                formatter,
                "model_inputs.initial_excitation_state.{field} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsRiskModelInputsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidEquationSpecification { source } => Some(source),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => Some(source),
            _ => None,
        }
    }
}

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
