use std::error::Error;
use std::fmt::{Display, Formatter};

use super::interface::NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION;

#[derive(Debug, Clone, PartialEq)]
pub enum NewsArrivalConfigurationError {
    InvalidKnownScheduledTimestampUtcNanos {
        value: i64,
    },
    KnownScheduledTimestampsNotStrictlyIncreasing {
        previous_timestamp_utc_nanos: i64,
        current_timestamp_utc_nanos: i64,
    },
    InvalidSurpriseBaselineIntensity {
        value: f64,
    },
}

impl Display for NewsArrivalConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKnownScheduledTimestampUtcNanos { value } => write!(
                formatter,
                "known.scheduled_event_timestamps_utc_nanos contains an invalid UTC timestamp nanos value {value}"
            ),
            Self::KnownScheduledTimestampsNotStrictlyIncreasing {
                previous_timestamp_utc_nanos,
                current_timestamp_utc_nanos,
            } => write!(
                formatter,
                "known.scheduled_event_timestamps_utc_nanos must be strictly increasing, found previous={previous_timestamp_utc_nanos} and current={current_timestamp_utc_nanos}"
            ),
            Self::InvalidSurpriseBaselineIntensity { value } => write!(
                formatter,
                "surprise.baseline_intensity must be a finite non-negative value, found {value}"
            ),
        }
    }
}

impl Error for NewsArrivalConfigurationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsComponentIntensityEvaluationError {
    InvalidConfiguration {
        source: NewsArrivalConfigurationError,
    },
    NonIncreasingInterval {
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
    InvalidIntervalStartTimestampUtcNanos {
        value: i64,
    },
    InvalidIntervalEndTimestampUtcNanos {
        value: i64,
    },
    InvalidSurpriseDeterministicTimeMultiplier {
        value: f64,
    },
    KnownScheduledArrivalCountOverflow {
        value: usize,
    },
    InvalidKnownComponentIntensity {
        value: f64,
    },
    InvalidSurpriseComponentIntensity {
        value: f64,
    },
    InvalidTotalIntensity {
        value: f64,
    },
}

impl Display for NewsComponentIntensityEvaluationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration { source } => write!(formatter, "{source}"),
            Self::NonIncreasingInterval {
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "arrival-intensity interval must be increasing, found start={interval_start_utc_nanos} and end={interval_end_utc_nanos}"
            ),
            Self::InvalidIntervalStartTimestampUtcNanos { value } => write!(
                formatter,
                "interval_start_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::InvalidIntervalEndTimestampUtcNanos { value } => write!(
                formatter,
                "interval_end_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::InvalidSurpriseDeterministicTimeMultiplier { value } => write!(
                formatter,
                "surprise_deterministic_time_multiplier must be finite and non-negative, found {value}"
            ),
            Self::KnownScheduledArrivalCountOverflow { value } => write!(
                formatter,
                "known scheduled arrival count overflowed u64 conversion, found {value}"
            ),
            Self::InvalidKnownComponentIntensity { value } => write!(
                formatter,
                "known-component arrival intensity must be finite and non-negative, found {value}"
            ),
            Self::InvalidSurpriseComponentIntensity { value } => write!(
                formatter,
                "surprise-component arrival intensity must be finite and non-negative, found {value}"
            ),
            Self::InvalidTotalIntensity { value } => write!(
                formatter,
                "total arrival intensity must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for NewsComponentIntensityEvaluationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfiguration { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsComponentMarkScaleConfigurationError {
    InvalidActivityCouplingScale { value: f64 },
    InvalidReturnJumpScale { value: f64 },
}

impl Display for NewsComponentMarkScaleConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidActivityCouplingScale { value } => write!(
                formatter,
                "activity_coupling_scale must be a finite non-negative value, found {value}"
            ),
            Self::InvalidReturnJumpScale { value } => write!(
                formatter,
                "return_jump_scale must be a finite non-negative value, found {value}"
            ),
        }
    }
}

impl Error for NewsComponentMarkScaleConfigurationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsIntensityError {
    InvalidClosedWindowBaselineIntensity { value: f64 },
}

impl Display for NewsIntensityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidClosedWindowBaselineIntensity { value } => write!(
                formatter,
                "closed_window_baseline_intensity must be a finite non-negative value, found {value}"
            ),
        }
    }
}

impl Error for NewsIntensityError {}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsMarkConfigurationError {
    InvalidGammaShapeK { value: f64 },
    InvalidGammaScaleTheta { value: f64 },
    InvalidPositiveJumpProbability { value: f64 },
    InvalidEtaPlus { value: f64 },
    InvalidEtaMinus { value: f64 },
    InvalidSigmaG { value: f64 },
}

impl Display for NewsMarkConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidGammaShapeK { value } => {
                write!(
                    formatter,
                    "gamma severity shape_k must be finite and strictly positive, found {value}"
                )
            }
            Self::InvalidGammaScaleTheta { value } => write!(
                formatter,
                "gamma severity scale_theta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidPositiveJumpProbability { value } => write!(
                formatter,
                "double_exponential_base_shock.probability_positive must be finite and strictly between 0 and 1, found {value}"
            ),
            Self::InvalidEtaPlus { value } => write!(
                formatter,
                "double_exponential_base_shock.eta_plus must be finite and strictly positive, found {value}"
            ),
            Self::InvalidEtaMinus { value } => write!(
                formatter,
                "double_exponential_base_shock.eta_minus must be finite and strictly positive, found {value}"
            ),
            Self::InvalidSigmaG { value } => write!(
                formatter,
                "coupled_gap_marks.sigma_g must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for NewsMarkConfigurationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsProcessConfigurationError {
    UnsupportedSchemaVersion {
        value: String,
    },
    InvalidArrivalConfiguration {
        source: NewsArrivalConfigurationError,
    },
    InvalidGammaSeverityMarkConfiguration {
        source: NewsMarkConfigurationError,
    },
    InvalidBaseShockConfiguration {
        source: NewsMarkConfigurationError,
    },
    InvalidCoupledGapMarkConfiguration {
        source: NewsMarkConfigurationError,
    },
    InvalidKnownComponentMarkScales {
        source: NewsComponentMarkScaleConfigurationError,
    },
    InvalidSurpriseComponentMarkScales {
        source: NewsComponentMarkScaleConfigurationError,
    },
}

impl Display for NewsProcessConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { value } => write!(
                formatter,
                "unsupported news process configuration schema version `{value}`, expected `{NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION}`"
            ),
            Self::InvalidArrivalConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidGammaSeverityMarkConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidBaseShockConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidCoupledGapMarkConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidKnownComponentMarkScales { source } => write!(formatter, "{source}"),
            Self::InvalidSurpriseComponentMarkScales { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for NewsProcessConfigurationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidArrivalConfiguration { source } => Some(source),
            Self::InvalidGammaSeverityMarkConfiguration { source } => Some(source),
            Self::InvalidBaseShockConfiguration { source } => Some(source),
            Self::InvalidCoupledGapMarkConfiguration { source } => Some(source),
            Self::InvalidKnownComponentMarkScales { source } => Some(source),
            Self::InvalidSurpriseComponentMarkScales { source } => Some(source),
            Self::UnsupportedSchemaVersion { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsDeterministicIntensityEvaluationError {
    InvalidClosedWindowShare { value: f64 },
    InvalidDeterministicTimeMultiplier { value: f64 },
    InvalidEvaluatedIntensity { value: f64 },
}

impl Display for NewsDeterministicIntensityEvaluationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidClosedWindowShare { value } => write!(
                formatter,
                "closed_window_share must be finite and in [0, 1], found {value}"
            ),
            Self::InvalidDeterministicTimeMultiplier { value } => write!(
                formatter,
                "deterministic_time_multiplier must be finite and non-negative, found {value}"
            ),
            Self::InvalidEvaluatedIntensity { value } => write!(
                formatter,
                "evaluated deterministic news intensity must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for NewsDeterministicIntensityEvaluationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum GammaSeverityMarkSamplingError {
    InvalidConfiguration { source: NewsMarkConfigurationError },
    InvalidSampledMark { value: f64 },
}

impl Display for GammaSeverityMarkSamplingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration { source } => {
                write!(
                    formatter,
                    "gamma severity mark configuration is invalid for sampling: {source}"
                )
            }
            Self::InvalidSampledMark { value } => write!(
                formatter,
                "gamma severity sampling produced a non-finite or negative mark, found {value}"
            ),
        }
    }
}

impl Error for GammaSeverityMarkSamplingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfiguration { source } => Some(source),
            Self::InvalidSampledMark { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DoubleExponentialBaseShockSamplingError {
    InvalidConfiguration { source: NewsMarkConfigurationError },
    InvalidSampledMark { value: f64 },
}

impl Display for DoubleExponentialBaseShockSamplingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration { source } => {
                write!(
                    formatter,
                    "double-exponential base-shock configuration is invalid for sampling: {source}"
                )
            }
            Self::InvalidSampledMark { value } => write!(
                formatter,
                "double-exponential base-shock sampling produced a non-finite mark, found {value}"
            ),
        }
    }
}

impl Error for DoubleExponentialBaseShockSamplingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfiguration { source } => Some(source),
            Self::InvalidSampledMark { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoupledGapMarkConstructionError {
    InvalidConfiguration { source: NewsMarkConfigurationError },
    InvalidSharedSeverityMark { value: f64 },
    InvalidSharedBaseShockMark { value: f64 },
    InvalidConstructedGapMark { value: f64 },
}

impl Display for CoupledGapMarkConstructionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration { source } => {
                write!(
                    formatter,
                    "coupled gap-mark configuration is invalid for construction: {source}"
                )
            }
            Self::InvalidSharedSeverityMark { value } => write!(
                formatter,
                "shared severity mark must be finite and non-negative, found {value}"
            ),
            Self::InvalidSharedBaseShockMark { value } => write!(
                formatter,
                "shared base-shock mark must be finite, found {value}"
            ),
            Self::InvalidConstructedGapMark { value } => write!(
                formatter,
                "constructed coupled gap mark must be finite, found {value}"
            ),
        }
    }
}

impl Error for CoupledGapMarkConstructionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfiguration { source } => Some(source),
            Self::InvalidSharedSeverityMark { .. }
            | Self::InvalidSharedBaseShockMark { .. }
            | Self::InvalidConstructedGapMark { .. } => None,
        }
    }
}
