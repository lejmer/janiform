use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::numeric::NonNegativeNumberValidationError;
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelParameters,
};
use janiform_model_kernels::long_variance::{
    LongVarianceParameters, LongVarianceParametersError, LongVarianceState, LongVarianceStateError,
    LongVarianceStateTransitionError, LongVarianceStationaryLevelError,
};
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionBoundaryStateEvolutionError,
    MeanReversionBoundaryStateValidationError, MeanReversionFittedParameters,
    MeanReversionFittedParametersValidationError,
};
use serde::{Deserialize, Serialize};

/// Rust-native input for fitted-model current-state estimation.
#[derive(Debug, Clone, PartialEq)]
pub struct CurrentStateEstimationInput {
    pub kernel_parameters: HawkesKernelParameters,
    pub long_variance_parameters: LongVarianceParameters,
    pub mean_reversion_fitted_parameters: MeanReversionFittedParameters,
    pub reference_price: f64,
    pub state_time_utc_nanos: i64,
    pub observed_log_return: f64,
    pub observation_timestamp_utc_nanos: Vec<i64>,
    pub observation_trade_prices: Vec<f64>,
    pub observation_trade_sides: Vec<HawkesEventSide>,
}

/// Rust-native input for estimating fitted-model state at multiple ordered boundaries.
#[derive(Debug, Clone, PartialEq)]
pub struct CurrentStateEstimationBatchInput {
    pub kernel_parameters: HawkesKernelParameters,
    pub long_variance_parameters: LongVarianceParameters,
    pub mean_reversion_fitted_parameters: MeanReversionFittedParameters,
    pub reference_price: f64,
    pub state_times_utc_nanos: Vec<i64>,
    pub observation_timestamp_utc_nanos: Vec<i64>,
    pub observation_trade_prices: Vec<f64>,
    pub observation_trade_sides: Vec<HawkesEventSide>,
}

/// Rust-native output for fitted-model current-state estimation.
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
pub struct CurrentStateEstimationOutput {
    pub hawkes_excitation_state: HawkesExcitationState,
    pub long_variance_state: LongVarianceState,
    pub mean_reversion_boundary_state: MeanReversionBoundaryState,
}

/// State estimate at one requested state boundary.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CurrentStateEstimationBatchItem {
    pub state_time_utc_nanos: i64,
    pub observed_price: f64,
    pub state: CurrentStateEstimationOutput,
}

/// Progress emitted by multi-boundary current-state estimation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurrentStateEstimationBatchProgressEvent {
    pub consumed_observation_count: usize,
    pub total_observation_count: usize,
    pub completed_state_count: usize,
    pub total_state_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CurrentStateEstimationError {
    EmptyObservations,
    EmptyStateTimes,
    ObservationColumnLengthMismatch {
        timestamp_count: usize,
        trade_price_count: usize,
        trade_side_count: usize,
    },
    StateTimeOutOfOrder {
        state_index: usize,
        previous_state_time_utc_nanos: i64,
        state_time_utc_nanos: i64,
    },
    StateTimeBeforeFirstObservation {
        state_index: usize,
        state_time_utc_nanos: i64,
        first_observation_timestamp_utc_nanos: i64,
    },
    StateTimeBeforePreviousState {
        previous_state_time_utc_nanos: i64,
        state_time_utc_nanos: i64,
    },
    InvalidObservedLogReturn {
        value: f64,
    },
    InvalidReferencePrice {
        value: f64,
    },
    MissingObservedPrice,
    InvalidPreviousObservedPrice {
        value: f64,
    },
    InvalidObservationTradePrice {
        observation_index: usize,
        value: f64,
    },
    ObservationBeforePreviousStateTime {
        observation_index: usize,
        previous_state_time_utc_nanos: i64,
        timestamp_utc_nanos: i64,
    },
    ObservationTimestampOutOfOrder {
        observation_index: usize,
        previous_timestamp_utc_nanos: i64,
        timestamp_utc_nanos: i64,
    },
    ObservationAfterStateTime {
        state_time_utc_nanos: i64,
        observation_index: usize,
        timestamp_utc_nanos: i64,
    },
    InvalidMeanReversionFittedParameters {
        source: MeanReversionFittedParametersValidationError,
    },
    InvalidHawkesExcitationState {
        source: NonNegativeNumberValidationError,
    },
    InvalidLongVarianceParameters {
        source: LongVarianceParametersError,
    },
    InvalidLongVarianceState {
        source: LongVarianceStateError,
    },
    InvalidLongVarianceStationaryIntensityMultipliers,
    InvalidLongVarianceStationaryLevel {
        source: LongVarianceStationaryLevelError,
    },
    InvalidLongVarianceStateTransition {
        source: LongVarianceStateTransitionError,
    },
    InvalidLongVarianceBoundaryState {
        source: LongVarianceStateError,
    },
    InvalidMeanReversionBoundaryState {
        source: MeanReversionBoundaryStateValidationError,
    },
    InvalidMeanReversionBoundaryStateEvolution {
        source: MeanReversionBoundaryStateEvolutionError,
    },
}

impl Display for CurrentStateEstimationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyObservations => {
                write!(
                    formatter,
                    "current-state estimation requires at least one observation"
                )
            }
            Self::EmptyStateTimes => {
                write!(
                    formatter,
                    "batch current-state estimation requires at least one state time"
                )
            }
            Self::ObservationColumnLengthMismatch {
                timestamp_count,
                trade_price_count,
                trade_side_count,
            } => write!(
                formatter,
                "current-state estimation requires aligned observation columns, received timestamps={timestamp_count}, trade_prices={trade_price_count}, trade_sides={trade_side_count}"
            ),
            Self::StateTimeOutOfOrder {
                state_index,
                previous_state_time_utc_nanos,
                state_time_utc_nanos,
            } => write!(
                formatter,
                "batch current-state estimation requires non-decreasing state times, but state_times[{state_index}]={state_time_utc_nanos} follows {previous_state_time_utc_nanos}"
            ),
            Self::StateTimeBeforeFirstObservation {
                state_index,
                state_time_utc_nanos,
                first_observation_timestamp_utc_nanos,
            } => write!(
                formatter,
                "batch current-state estimation requires state_times[{state_index}]={state_time_utc_nanos} to retain at least one observation, but the first observation is {first_observation_timestamp_utc_nanos}"
            ),
            Self::StateTimeBeforePreviousState {
                previous_state_time_utc_nanos,
                state_time_utc_nanos,
            } => write!(
                formatter,
                "current-state tracker update requires state_time_utc_nanos={state_time_utc_nanos} to be greater than or equal to the previous state time {previous_state_time_utc_nanos}"
            ),
            Self::InvalidObservedLogReturn { value } => write!(
                formatter,
                "current-state estimation requires a finite observed_log_return, found {value}"
            ),
            Self::InvalidReferencePrice { value } => write!(
                formatter,
                "current-state estimation requires a finite, strictly positive reference_price, found {value}"
            ),
            Self::MissingObservedPrice => write!(
                formatter,
                "current-state tracker update requires an observed price from observed_price, a new trade, or a previous snapshot"
            ),
            Self::InvalidPreviousObservedPrice { value } => write!(
                formatter,
                "current-state tracker update requires previous observed_price to be finite and strictly positive, found {value}"
            ),
            Self::InvalidObservationTradePrice {
                observation_index,
                value,
            } => write!(
                formatter,
                "current-state estimation requires observations[{observation_index}].trade_price to be finite and strictly positive, found {value}"
            ),
            Self::ObservationBeforePreviousStateTime {
                observation_index,
                previous_state_time_utc_nanos,
                timestamp_utc_nanos,
            } => write!(
                formatter,
                "current-state tracker update requires observations[{observation_index}]={timestamp_utc_nanos} to be no earlier than previous state time {previous_state_time_utc_nanos}"
            ),
            Self::ObservationTimestampOutOfOrder {
                observation_index,
                previous_timestamp_utc_nanos,
                timestamp_utc_nanos,
            } => write!(
                formatter,
                "current-state estimation requires non-decreasing observation timestamps, but observations[{observation_index}]={timestamp_utc_nanos} follows {previous_timestamp_utc_nanos}"
            ),
            Self::ObservationAfterStateTime {
                state_time_utc_nanos,
                observation_index,
                timestamp_utc_nanos,
            } => write!(
                formatter,
                "current-state estimation requires observations[{observation_index}]={timestamp_utc_nanos} to be no later than state_time_utc_nanos={state_time_utc_nanos}"
            ),
            Self::InvalidMeanReversionFittedParameters { source } => write!(
                formatter,
                "current-state estimation received invalid mean-reversion fitted parameters: {source}"
            ),
            Self::InvalidHawkesExcitationState { source } => write!(
                formatter,
                "current-state tracker update received invalid Hawkes excitation state: {source}"
            ),
            Self::InvalidLongVarianceParameters { source } => write!(
                formatter,
                "current-state estimation received invalid long-variance parameters: {source}"
            ),
            Self::InvalidLongVarianceState { source } => write!(
                formatter,
                "current-state tracker update received invalid long-variance state: {source}"
            ),
            Self::InvalidLongVarianceStationaryIntensityMultipliers => write!(
                formatter,
                "current-state estimation could not derive stationary Hawkes intensity multipliers for long-variance normalization"
            ),
            Self::InvalidLongVarianceStationaryLevel { source } => write!(
                formatter,
                "current-state estimation could not derive the long-variance stationary mean-reversion level: {source}"
            ),
            Self::InvalidLongVarianceStateTransition { source } => write!(
                formatter,
                "current-state estimation failed to replay long-variance state: {source}"
            ),
            Self::InvalidLongVarianceBoundaryState { source } => write!(
                formatter,
                "current-state estimation produced an invalid long-variance boundary state: {source}"
            ),
            Self::InvalidMeanReversionBoundaryState { source } => write!(
                formatter,
                "current-state estimation produced an invalid mean-reversion boundary state: {source}"
            ),
            Self::InvalidMeanReversionBoundaryStateEvolution { source } => write!(
                formatter,
                "current-state estimation failed to replay mean-reversion boundary state: {source}"
            ),
        }
    }
}

impl Error for CurrentStateEstimationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMeanReversionFittedParameters { source } => Some(source),
            Self::InvalidHawkesExcitationState { source } => Some(source),
            Self::InvalidLongVarianceParameters { source } => Some(source),
            Self::InvalidLongVarianceState { source } => Some(source),
            Self::InvalidLongVarianceStationaryLevel { source } => Some(source),
            Self::InvalidLongVarianceStateTransition { source } => Some(source),
            Self::InvalidLongVarianceBoundaryState { source } => Some(source),
            Self::InvalidMeanReversionBoundaryState { source } => Some(source),
            Self::InvalidMeanReversionBoundaryStateEvolution { source } => Some(source),
            Self::EmptyObservations
            | Self::EmptyStateTimes
            | Self::ObservationColumnLengthMismatch { .. }
            | Self::StateTimeOutOfOrder { .. }
            | Self::StateTimeBeforeFirstObservation { .. }
            | Self::StateTimeBeforePreviousState { .. }
            | Self::InvalidObservedLogReturn { .. }
            | Self::InvalidReferencePrice { .. }
            | Self::MissingObservedPrice
            | Self::InvalidPreviousObservedPrice { .. }
            | Self::InvalidObservationTradePrice { .. }
            | Self::ObservationBeforePreviousStateTime { .. }
            | Self::ObservationTimestampOutOfOrder { .. }
            | Self::ObservationAfterStateTime { .. }
            | Self::InvalidLongVarianceStationaryIntensityMultipliers => None,
        }
    }
}
