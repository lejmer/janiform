use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::complex_math::ComplexScalar;

/// Validation failure for market pressure transition-rate construction.
#[derive(Debug, Clone, PartialEq)]
pub enum MarketPressureTransitionRatesValidationError {
    /// The shared support was empty.
    EmptyStateSupport,
    /// The upward-rate vector length did not match the support size.
    InvalidUpwardRateCount { expected: usize, actual: usize },
    /// The downward-rate vector length did not match the support size.
    InvalidDownwardRateCount { expected: usize, actual: usize },
    /// One upward transition rate was invalid.
    InvalidUpwardRate { rate_position: usize, value: f64 },
    /// One downward transition rate was invalid.
    InvalidDownwardRate { rate_position: usize, value: f64 },
}

impl Display for MarketPressureTransitionRatesValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyStateSupport => write!(
                formatter,
                "market pressure side-imbalance support must contain at least one state"
            ),
            Self::InvalidUpwardRateCount { expected, actual } => write!(
                formatter,
                "upward_transition_rates length must be {expected}, found {actual}"
            ),
            Self::InvalidDownwardRateCount { expected, actual } => write!(
                formatter,
                "downward_transition_rates length must be {expected}, found {actual}"
            ),
            Self::InvalidUpwardRate {
                rate_position,
                value,
            } => write!(
                formatter,
                "upward_transition_rates[{rate_position}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidDownwardRate {
                rate_position,
                value,
            } => write!(
                formatter,
                "downward_transition_rates[{rate_position}] must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for MarketPressureTransitionRatesValidationError {}

/// Validation failure for market pressure measure construction.
#[derive(Debug, Clone, PartialEq)]
pub enum MarketPressureMeasureParametersValidationError {
    /// The transition rates were invalid.
    InvalidTransitionRates {
        source: MarketPressureTransitionRatesValidationError,
    },
    /// The anchor drift loading was invalid.
    InvalidAnchorDriftPerSignedBalance { value: f64 },
}

impl Display for MarketPressureMeasureParametersValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTransitionRates { source } => write!(formatter, "{source}"),
            Self::InvalidAnchorDriftPerSignedBalance { value } => write!(
                formatter,
                "anchor_drift_per_signed_balance must be finite, found {value}"
            ),
        }
    }
}

impl Error for MarketPressureMeasureParametersValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidTransitionRates { source } => Some(source),
            Self::InvalidAnchorDriftPerSignedBalance { .. } => None,
        }
    }
}

/// Validation failure for market pressure parameter construction.
#[derive(Debug, Clone, PartialEq)]
pub enum MarketPressureParametersValidationError {
    /// The side-imbalance support was empty.
    EmptySideImbalanceSupport,
    /// A support level was non-finite.
    InvalidSideImbalanceLevel { state_position: usize, value: f64 },
    /// A support level was outside the signed side-imbalance domain.
    SideImbalanceLevelOutOfRange { state_position: usize, value: f64 },
    /// The support levels were not strictly increasing.
    SideImbalanceLevelsNotIncreasing {
        previous_state_position: usize,
        previous_value: f64,
        state_position: usize,
        value: f64,
    },
    /// Physical measure parameters were invalid.
    InvalidPhysicalMeasure {
        source: MarketPressureMeasureParametersValidationError,
    },
}

impl Display for MarketPressureParametersValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySideImbalanceSupport => write!(
                formatter,
                "side_imbalance_levels must contain at least one ordered support point"
            ),
            Self::InvalidSideImbalanceLevel {
                state_position,
                value,
            } => write!(
                formatter,
                "side_imbalance_levels[{state_position}] must be finite, found {value}"
            ),
            Self::SideImbalanceLevelOutOfRange {
                state_position,
                value,
            } => write!(
                formatter,
                "side_imbalance_levels[{state_position}] must be between -1 and 1, found {value}"
            ),
            Self::SideImbalanceLevelsNotIncreasing {
                previous_state_position,
                previous_value,
                state_position,
                value,
            } => write!(
                formatter,
                "side_imbalance_levels must be strictly increasing, found side_imbalance_levels[{previous_state_position}]={previous_value} before side_imbalance_levels[{state_position}]={value}"
            ),
            Self::InvalidPhysicalMeasure { source } => {
                write!(formatter, "physical_measure is invalid: {source}")
            }
        }
    }
}

impl Error for MarketPressureParametersValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPhysicalMeasure { source } => Some(source),
            Self::EmptySideImbalanceSupport
            | Self::InvalidSideImbalanceLevel { .. }
            | Self::SideImbalanceLevelOutOfRange { .. }
            | Self::SideImbalanceLevelsNotIncreasing { .. } => None,
        }
    }
}

/// Validation failure for market pressure state construction.
#[derive(Debug, Clone, PartialEq)]
pub enum MarketPressureStateValidationError {
    /// The supplied market-pressure parameters were invalid.
    InvalidParameters(MarketPressureParametersValidationError),
    /// The state-weight vector was empty.
    EmptyStateWeights,
    /// One state weight was invalid.
    InvalidStateWeight { state_position: usize, value: f64 },
    /// The state-weight sum was not finite and strictly positive.
    InvalidStateWeightSum { value: f64 },
    /// A deterministic state position was outside the support.
    StatePositionOutOfRange {
        state_position: usize,
        state_count: usize,
    },
    /// State weights did not match the supplied parameter support.
    StateCountMismatch {
        expected_state_count: usize,
        actual_state_count: usize,
    },
}

impl Display for MarketPressureStateValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidParameters(source) => {
                write!(
                    formatter,
                    "market pressure parameters are invalid: {source}"
                )
            }
            Self::EmptyStateWeights => {
                write!(formatter, "state_weights must contain at least one weight")
            }
            Self::InvalidStateWeight {
                state_position,
                value,
            } => write!(
                formatter,
                "state_weights[{state_position}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidStateWeightSum { value } => write!(
                formatter,
                "state_weights sum must be finite and strictly positive, found {value}"
            ),
            Self::StatePositionOutOfRange {
                state_position,
                state_count,
            } => write!(
                formatter,
                "state_position {state_position} is out of range for {state_count} pressure states"
            ),
            Self::StateCountMismatch {
                expected_state_count,
                actual_state_count,
            } => write!(
                formatter,
                "state_weights length must match side_imbalance_levels length {expected_state_count}, found {actual_state_count}"
            ),
        }
    }
}

impl Error for MarketPressureStateValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidParameters(source) => Some(source),
            Self::EmptyStateWeights
            | Self::InvalidStateWeight { .. }
            | Self::InvalidStateWeightSum { .. }
            | Self::StatePositionOutOfRange { .. }
            | Self::StateCountMismatch { .. } => None,
        }
    }
}

/// Failure while applying or projecting market-pressure state evolution.
#[derive(Debug, Clone, PartialEq)]
pub enum MarketPressureStateEvolutionError {
    /// The supplied parameter contract was invalid.
    InvalidParameters(MarketPressureParametersValidationError),
    /// The supplied boundary state was invalid.
    InvalidState(MarketPressureStateValidationError),
    /// An interval length was not finite and non-negative.
    InvalidIntervalDelta { value: f64 },
    /// The signed-balance exposure scale was not finite and non-negative.
    InvalidSignedBalanceExposureScale { value: f64 },
    /// CTMC evolution failed.
    InvalidCtmcTransition(MarketPressureCtmcTransitionError),
    /// Expected side imbalance was invalid.
    InvalidExpectedSideImbalance { value: f64 },
    /// Expected side imbalance was outside the signed side-balance domain.
    ExpectedSideImbalanceOutOfRange { value: f64 },
    /// The observed side had zero or invalid likelihood under the state distribution.
    InvalidObservationLikelihood { value: f64 },
    /// The integrated signed side imbalance was invalid.
    InvalidIntegratedSideImbalance { value: f64 },
    /// The scaled signed side-balance exposure was invalid.
    InvalidSignedBalanceExposure { value: f64 },
    /// The anchor increment projection produced a non-finite value.
    InvalidAnchorIncrement { value: f64 },
    /// A finite-state pressure transform value was invalid.
    InvalidTransformValue {
        field: &'static str,
        value: ComplexScalar,
    },
    /// A terminal finite-state pressure transform value was invalid.
    InvalidTerminalTransformValue {
        state_position: usize,
        value: ComplexScalar,
    },
}

impl Display for MarketPressureStateEvolutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidParameters(source) => {
                write!(
                    formatter,
                    "market pressure parameters are invalid: {source}"
                )
            }
            Self::InvalidState(source) => {
                write!(formatter, "market pressure state is invalid: {source}")
            }
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be finite and non-negative, found {value}"
            ),
            Self::InvalidSignedBalanceExposureScale { value } => write!(
                formatter,
                "signed_balance_exposure_scale must be finite and non-negative, found {value}"
            ),
            Self::InvalidCtmcTransition(source) => {
                write!(
                    formatter,
                    "market pressure CTMC transition failed: {source}"
                )
            }
            Self::InvalidExpectedSideImbalance { value } => write!(
                formatter,
                "expected side imbalance must be finite, found {value}"
            ),
            Self::ExpectedSideImbalanceOutOfRange { value } => write!(
                formatter,
                "expected side imbalance must be between -1 and 1, found {value}"
            ),
            Self::InvalidObservationLikelihood { value } => write!(
                formatter,
                "observed event side likelihood must be finite and positive, found {value}"
            ),
            Self::InvalidIntegratedSideImbalance { value } => write!(
                formatter,
                "integrated side imbalance must be finite, found {value}"
            ),
            Self::InvalidSignedBalanceExposure { value } => write!(
                formatter,
                "signed balance exposure must be finite, found {value}"
            ),
            Self::InvalidAnchorIncrement { value } => write!(
                formatter,
                "market pressure anchor increment must be finite, found {value}"
            ),
            Self::InvalidTransformValue { field, value } => write!(
                formatter,
                "{field} must be a valid finite complex market pressure transform value, found {value:?}"
            ),
            Self::InvalidTerminalTransformValue {
                state_position,
                value,
            } => write!(
                formatter,
                "terminal market pressure transform value at state {state_position} must be finite, found {value:?}"
            ),
        }
    }
}

impl Error for MarketPressureStateEvolutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidParameters(source) => Some(source),
            Self::InvalidState(source) => Some(source),
            Self::InvalidCtmcTransition(source) => Some(source),
            Self::InvalidIntervalDelta { .. }
            | Self::InvalidSignedBalanceExposureScale { .. }
            | Self::InvalidExpectedSideImbalance { .. }
            | Self::ExpectedSideImbalanceOutOfRange { .. }
            | Self::InvalidObservationLikelihood { .. }
            | Self::InvalidIntegratedSideImbalance { .. }
            | Self::InvalidSignedBalanceExposure { .. }
            | Self::InvalidAnchorIncrement { .. }
            | Self::InvalidTransformValue { .. }
            | Self::InvalidTerminalTransformValue { .. } => None,
        }
    }
}

/// Failure while resolving finite-state CTMC transition quantities.
#[derive(Debug, Clone, PartialEq)]
pub enum MarketPressureCtmcTransitionError {
    /// Transition rates were invalid for the requested support.
    InvalidTransitionRates {
        source: MarketPressureTransitionRatesValidationError,
    },
}

impl Display for MarketPressureCtmcTransitionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTransitionRates { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for MarketPressureCtmcTransitionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidTransitionRates { source } => Some(source),
        }
    }
}
