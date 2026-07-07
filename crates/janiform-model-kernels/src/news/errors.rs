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
    InvalidSurpriseOpenBaselineIntensity {
        value: f64,
    },
    InvalidSurpriseClosedBaselineIntensity {
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
            Self::InvalidSurpriseOpenBaselineIntensity { value } => write!(
                formatter,
                "surprise.open_baseline_intensity must be a finite non-negative value, found {value}"
            ),
            Self::InvalidSurpriseClosedBaselineIntensity { value } => write!(
                formatter,
                "surprise.closed_baseline_intensity must be a finite non-negative value, found {value}"
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
    InvalidSurpriseOpenWindowShare {
        value: f64,
    },
    InvalidSurpriseClosedWindowShare {
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
            Self::InvalidSurpriseOpenWindowShare { value } => write!(
                formatter,
                "surprise_open_window_share must be finite and between 0 and 1, found {value}"
            ),
            Self::InvalidSurpriseClosedWindowShare { value } => write!(
                formatter,
                "surprise_closed_window_share must be finite and between 0 and 1, found {value}"
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
    InvalidActivityCouplingScale {
        value: f64,
    },
    InvalidReturnJumpDrift {
        value: f64,
    },
    InvalidReturnJumpVolatility {
        value: f64,
    },
    ReturnJumpWithoutActivityCoupling {
        return_jump_drift: f64,
        return_jump_volatility: f64,
    },
}

impl Display for NewsComponentMarkScaleConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidActivityCouplingScale { value } => write!(
                formatter,
                "activity_coupling_scale must be a finite non-negative value, found {value}"
            ),
            Self::InvalidReturnJumpDrift { value } => {
                write!(formatter, "return_jump_drift must be finite, found {value}")
            }
            Self::InvalidReturnJumpVolatility { value } => write!(
                formatter,
                "return_jump_volatility must be a finite non-negative value, found {value}"
            ),
            Self::ReturnJumpWithoutActivityCoupling {
                return_jump_drift,
                return_jump_volatility,
            } => write!(
                formatter,
                "return-jump parameters require positive activity_coupling_scale, found return_jump_drift={return_jump_drift} and return_jump_volatility={return_jump_volatility}"
            ),
        }
    }
}

impl Error for NewsComponentMarkScaleConfigurationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsMarkConfigurationError {
    InvalidGammaShapeK { value: f64 },
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
        }
    }
}

impl Error for NewsMarkConfigurationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsActivityLoadingRateError {
    InvalidKnownIntensity {
        value: f64,
    },
    InvalidSurpriseIntensity {
        value: f64,
    },
    InvalidKnownComponentMarkScales {
        source: NewsComponentMarkScaleConfigurationError,
    },
    InvalidSurpriseComponentMarkScales {
        source: NewsComponentMarkScaleConfigurationError,
    },
    InvalidGammaSeverityMarkConfiguration {
        source: NewsMarkConfigurationError,
    },
    InvalidDerivedRate {
        value: f64,
    },
}

impl Display for NewsActivityLoadingRateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKnownIntensity { value } => write!(
                formatter,
                "known news intensity must be finite and non-negative, found {value}"
            ),
            Self::InvalidSurpriseIntensity { value } => write!(
                formatter,
                "surprise news intensity must be finite and non-negative, found {value}"
            ),
            Self::InvalidKnownComponentMarkScales { source } => write!(formatter, "{source}"),
            Self::InvalidSurpriseComponentMarkScales { source } => write!(formatter, "{source}"),
            Self::InvalidGammaSeverityMarkConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidDerivedRate { value } => write!(
                formatter,
                "expected news activity-loading rate must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for NewsActivityLoadingRateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidKnownComponentMarkScales { source } => Some(source),
            Self::InvalidSurpriseComponentMarkScales { source } => Some(source),
            Self::InvalidGammaSeverityMarkConfiguration { source } => Some(source),
            Self::InvalidKnownIntensity { .. } | Self::InvalidSurpriseIntensity { .. } => None,
            Self::InvalidDerivedRate { .. } => None,
        }
    }
}

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
            Self::InvalidKnownComponentMarkScales { source } => Some(source),
            Self::InvalidSurpriseComponentMarkScales { source } => Some(source),
            Self::UnsupportedSchemaVersion { .. } => None,
        }
    }
}

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
pub enum GammaNormalNewsMarkConstructionError {
    InvalidComponentMarkScaleConfiguration {
        source: NewsComponentMarkScaleConfigurationError,
    },
    InvalidSharedSeverityMark {
        value: f64,
    },
    InvalidStandardNormalMark {
        value: f64,
    },
    InvalidConstructedActivityShockMark {
        value: f64,
    },
    InvalidConstructedReturnJumpMark {
        value: f64,
    },
}

impl Display for GammaNormalNewsMarkConstructionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidComponentMarkScaleConfiguration { source } => write!(
                formatter,
                "news component mark scale configuration is invalid for Gamma-normal construction: {source}"
            ),
            Self::InvalidSharedSeverityMark { value } => write!(
                formatter,
                "shared severity mark must be finite and non-negative, found {value}"
            ),
            Self::InvalidStandardNormalMark { value } => write!(
                formatter,
                "standard normal return-shock mark must be finite, found {value}"
            ),
            Self::InvalidConstructedActivityShockMark { value } => write!(
                formatter,
                "constructed news activity-shock mark must be finite and non-negative, found {value}"
            ),
            Self::InvalidConstructedReturnJumpMark { value } => write!(
                formatter,
                "constructed news return-jump mark must be finite, found {value}"
            ),
        }
    }
}

impl Error for GammaNormalNewsMarkConstructionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidComponentMarkScaleConfiguration { source } => Some(source),
            Self::InvalidSharedSeverityMark { .. }
            | Self::InvalidStandardNormalMark { .. }
            | Self::InvalidConstructedActivityShockMark { .. }
            | Self::InvalidConstructedReturnJumpMark { .. } => None,
        }
    }
}
