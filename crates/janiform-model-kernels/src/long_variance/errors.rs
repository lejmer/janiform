use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceParametersError {
    InvalidPositiveParameter {
        field: &'static str,
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
    InvalidNonNegativeParameter {
        field: &'static str,
        source: janiform_foundations::numeric::NonNegativeNumberValidationError,
    },
}

impl Display for LongVarianceParametersError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPositiveParameter { field, source } => {
                write!(formatter, "{field} is invalid: {source}")
            }
            Self::InvalidNonNegativeParameter { field, source } => {
                write!(formatter, "{field} is invalid: {source}")
            }
        }
    }
}

impl Error for LongVarianceParametersError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPositiveParameter { source, .. } => Some(source),
            Self::InvalidNonNegativeParameter { source, .. } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceStateError {
    InvalidLevel {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
}

impl Display for LongVarianceStateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLevel { source } => write!(formatter, "level is invalid: {source}"),
        }
    }
}

impl Error for LongVarianceStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidLevel { source } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceStateTransitionError {
    InvalidInitialState {
        source: LongVarianceStateError,
    },
    InvalidLongVarianceParameters {
        source: LongVarianceParametersError,
    },
    InvalidStationaryMeanReversionLevel {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
    InvalidElapsedHours {
        source: janiform_foundations::numeric::FiniteNumberValidationError,
    },
    NegativeElapsedHours {
        value: f64,
    },
    InvalidReturnEnergyIncrement {
        source: janiform_foundations::numeric::NonNegativeNumberValidationError,
    },
    InvalidBaselineClockScale {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
    InvalidTransitionState {
        source: LongVarianceStateError,
    },
}

impl Display for LongVarianceStateTransitionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInitialState { source } => {
                write!(
                    formatter,
                    "initial long-variance state is invalid: {source}"
                )
            }
            Self::InvalidLongVarianceParameters { source } => {
                write!(formatter, "long-variance parameters are invalid: {source}")
            }
            Self::InvalidStationaryMeanReversionLevel { source } => write!(
                formatter,
                "stationary_mean_reversion_level is invalid: {source}"
            ),
            Self::InvalidElapsedHours { source } => {
                write!(formatter, "delta_hours is invalid: {source}")
            }
            Self::NegativeElapsedHours { value } => {
                write!(formatter, "delta_hours must be non-negative, found {value}")
            }
            Self::InvalidReturnEnergyIncrement { source } => {
                write!(formatter, "return_energy_increment is invalid: {source}")
            }
            Self::InvalidBaselineClockScale { source } => {
                write!(formatter, "baseline_clock_scale is invalid: {source}")
            }
            Self::InvalidTransitionState { source } => {
                write!(
                    formatter,
                    "transitioned long-variance state is invalid: {source}"
                )
            }
        }
    }
}

impl Error for LongVarianceStateTransitionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidInitialState { source } => Some(source),
            Self::InvalidLongVarianceParameters { source } => Some(source),
            Self::InvalidStationaryMeanReversionLevel { source } => Some(source),
            Self::InvalidElapsedHours { source } => Some(source),
            Self::InvalidReturnEnergyIncrement { source } => Some(source),
            Self::InvalidBaselineClockScale { source } => Some(source),
            Self::InvalidTransitionState { source } => Some(source),
            Self::NegativeElapsedHours { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceStationaryLevelError {
    InvalidLongVarianceParameters {
        source: LongVarianceParametersError,
    },
    InvalidBaselineIntensity {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
    InvalidStationaryEventRate {
        field: &'static str,
        source: janiform_foundations::numeric::NonNegativeNumberValidationError,
    },
    InvalidDerivedMeanReversionLevel {
        source: janiform_foundations::numeric::PositiveNumberValidationError,
    },
}

impl Display for LongVarianceStationaryLevelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLongVarianceParameters { source } => {
                write!(formatter, "long-variance parameters are invalid: {source}")
            }
            Self::InvalidBaselineIntensity { source } => {
                write!(formatter, "baseline_intensity is invalid: {source}")
            }
            Self::InvalidStationaryEventRate { field, source } => {
                write!(formatter, "{field} is invalid: {source}")
            }
            Self::InvalidDerivedMeanReversionLevel { source } => write!(
                formatter,
                "derived stationary mean-reversion level is invalid: {source}"
            ),
        }
    }
}

impl Error for LongVarianceStationaryLevelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidLongVarianceParameters { source } => Some(source),
            Self::InvalidBaselineIntensity { source } => Some(source),
            Self::InvalidStationaryEventRate { source, .. } => Some(source),
            Self::InvalidDerivedMeanReversionLevel { source } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceStationaryLevelForHawkesKernelError {
    InvalidStationaryIntensityMultipliers,
    InvalidStationaryLevel {
        source: LongVarianceStationaryLevelError,
    },
}

impl Display for LongVarianceStationaryLevelForHawkesKernelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStationaryIntensityMultipliers => write!(
                formatter,
                "stationary Hawkes intensity multipliers could not be derived"
            ),
            Self::InvalidStationaryLevel { source } => write!(
                formatter,
                "long-variance stationary mean-reversion level is invalid: {source}"
            ),
        }
    }
}

impl Error for LongVarianceStationaryLevelForHawkesKernelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidStationaryLevel { source } => Some(source),
            Self::InvalidStationaryIntensityMultipliers => None,
        }
    }
}
