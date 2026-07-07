use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::numeric::IntegerToF64ExactConversionError;
use janiform_model_kernels::market_pressure::{
    MarketPressureParametersValidationError, MarketPressureStateEvolutionError,
    MarketPressureTransitionRatesValidationError,
};
use janiform_model_kernels::return_clock::SideSourceActivityDecompositionError;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MarketPressureFitError {
    EmptyObservedOpenSessionReturns,
    NonIncreasingSupport,
    InvalidElapsedSameSessionHours {
        return_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidLogReturn {
        return_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidSellShareProxy {
        return_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidSideImbalance {
        return_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidSourceActivity {
        return_timestamp_utc_nanos: i64,
        source: SideSourceActivityDecompositionError,
    },
    InvalidSignedBalanceExposureScale {
        return_timestamp_utc_nanos: i64,
        source: MarketPressureStateEvolutionError,
    },
    InvalidSignedBalanceExposure {
        return_timestamp_utc_nanos: i64,
        value: f64,
    },
    InvalidSupportLevel {
        value: f64,
    },
    DegenerateSideBalanceState,
    InvalidStateAssignments,
    InvalidObservationLikelihood {
        value: f64,
    },
    InvalidTransitionExposure {
        value: f64,
    },
    InvalidTransitionRate {
        value: f64,
    },
    InvalidTransitionCount {
        source: IntegerToF64ExactConversionError,
    },
    InvalidCoarseReturnObservationCount {
        source: IntegerToF64ExactConversionError,
    },
    InvalidCoarseReturnLaw {
        error_message: String,
    },
    InvalidCoarseReturnLikelihood {
        error_message: String,
    },
    InvalidTransitionRates {
        source: MarketPressureTransitionRatesValidationError,
    },
    InvalidParameters {
        source: MarketPressureParametersValidationError,
    },
    InvalidAnchorDriftPerSignedBalance {
        value: f64,
    },
}

impl Display for MarketPressureFitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyObservedOpenSessionReturns => write!(
                formatter,
                "market pressure fit requires observed open-session returns"
            ),
            Self::NonIncreasingSupport => write!(
                formatter,
                "market pressure side-balance support must be strictly increasing"
            ),
            Self::InvalidElapsedSameSessionHours {
                return_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "market pressure return at {return_timestamp_utc_nanos} has invalid elapsed_same_session_hours {value}"
            ),
            Self::InvalidLogReturn {
                return_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "market pressure return at {return_timestamp_utc_nanos} has invalid log_return {value}"
            ),
            Self::InvalidSellShareProxy {
                return_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "market pressure return at {return_timestamp_utc_nanos} has invalid sell_share_proxy {value}"
            ),
            Self::InvalidSideImbalance {
                return_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "market pressure return at {return_timestamp_utc_nanos} has invalid side imbalance {value}"
            ),
            Self::InvalidSourceActivity {
                return_timestamp_utc_nanos,
                source,
            } => write!(
                formatter,
                "market pressure return at {return_timestamp_utc_nanos} has invalid source activity: {source}"
            ),
            Self::InvalidSignedBalanceExposureScale {
                return_timestamp_utc_nanos,
                source,
            } => write!(
                formatter,
                "market pressure return at {return_timestamp_utc_nanos} has invalid signed-balance exposure scale: {source}"
            ),
            Self::InvalidSignedBalanceExposure {
                return_timestamp_utc_nanos,
                value,
            } => write!(
                formatter,
                "market pressure return at {return_timestamp_utc_nanos} has invalid signed-balance exposure {value}"
            ),
            Self::InvalidSupportLevel { value } => write!(
                formatter,
                "market pressure side-balance support level must be finite and inside [-1, 1], found {value}"
            ),
            Self::DegenerateSideBalanceState => write!(
                formatter,
                "market pressure side-balance fit produced an unoccupied latent state"
            ),
            Self::InvalidStateAssignments => write!(
                formatter,
                "market pressure state assignments are inconsistent with fitted support"
            ),
            Self::InvalidObservationLikelihood { value } => write!(
                formatter,
                "market pressure side-observation likelihood must be positive and finite, found {value}"
            ),
            Self::InvalidTransitionExposure { value } => write!(
                formatter,
                "market pressure transition exposure must be finite and positive when transitions are observed, found {value}"
            ),
            Self::InvalidTransitionRate { value } => write!(
                formatter,
                "market pressure transition rate must be finite and non-negative, found {value}"
            ),
            Self::InvalidTransitionCount { source } => write!(
                formatter,
                "market pressure transition count cannot be converted to f64 exactly: {source}"
            ),
            Self::InvalidCoarseReturnObservationCount { source } => write!(
                formatter,
                "market pressure coarse-return observation count cannot be converted to f64 exactly: {source}"
            ),
            Self::InvalidCoarseReturnLaw { error_message } => write!(
                formatter,
                "market pressure coarse-return law is invalid: {error_message}"
            ),
            Self::InvalidCoarseReturnLikelihood { error_message } => write!(
                formatter,
                "market pressure coarse-return likelihood failed: {error_message}"
            ),
            Self::InvalidTransitionRates { source } => write!(
                formatter,
                "market pressure transition rates are invalid: {source}"
            ),
            Self::InvalidParameters { source } => {
                write!(
                    formatter,
                    "market pressure parameters are invalid: {source}"
                )
            }
            Self::InvalidAnchorDriftPerSignedBalance { value } => write!(
                formatter,
                "market pressure anchor drift per signed balance must be finite, found {value}"
            ),
        }
    }
}

impl Error for MarketPressureFitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSourceActivity { source, .. } => Some(source),
            Self::InvalidSignedBalanceExposureScale { source, .. } => Some(source),
            Self::InvalidTransitionCount { source } => Some(source),
            Self::InvalidCoarseReturnObservationCount { source } => Some(source),
            Self::InvalidTransitionRates { source } => Some(source),
            Self::InvalidParameters { source } => Some(source),
            Self::EmptyObservedOpenSessionReturns
            | Self::NonIncreasingSupport
            | Self::InvalidElapsedSameSessionHours { .. }
            | Self::InvalidLogReturn { .. }
            | Self::InvalidSellShareProxy { .. }
            | Self::InvalidSideImbalance { .. }
            | Self::InvalidSignedBalanceExposure { .. }
            | Self::InvalidSupportLevel { .. }
            | Self::InvalidCoarseReturnLaw { .. }
            | Self::InvalidCoarseReturnLikelihood { .. }
            | Self::DegenerateSideBalanceState
            | Self::InvalidStateAssignments
            | Self::InvalidObservationLikelihood { .. }
            | Self::InvalidTransitionExposure { .. }
            | Self::InvalidTransitionRate { .. }
            | Self::InvalidAnchorDriftPerSignedBalance { .. } => None,
        }
    }
}
