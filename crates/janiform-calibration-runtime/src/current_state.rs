use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelParameters,
};
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionBoundaryStateValidationError,
    MeanReversionFittedParameters, MeanReversionFittedParametersValidationError,
};
use serde::Serialize;

/// Rust-native input for fitted-model current-state estimation.
#[derive(Debug, Clone, PartialEq)]
pub struct CurrentStateEstimationInput {
    pub kernel_parameters: HawkesKernelParameters,
    pub mean_reversion_fitted_parameters: MeanReversionFittedParameters,
    pub state_time_utc_nanos: i64,
    pub observed_log_price: f64,
    pub observation_timestamp_utc_nanos: Vec<i64>,
    pub observation_trade_prices: Vec<f64>,
    pub observation_trade_sides: Vec<HawkesEventSide>,
}

/// Rust-native output for fitted-model current-state estimation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CurrentStateEstimationOutput {
    pub hawkes_excitation_state: HawkesExcitationState,
    pub mean_reversion_boundary_state: MeanReversionBoundaryState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CurrentStateEstimationError {
    EmptyObservations,
    ObservationColumnLengthMismatch {
        timestamp_count: usize,
        trade_price_count: usize,
        trade_side_count: usize,
    },
    InvalidObservedLogPrice {
        value: f64,
    },
    InvalidObservationTradePrice {
        observation_index: usize,
        value: f64,
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
    InvalidMeanReversionBoundaryState {
        source: MeanReversionBoundaryStateValidationError,
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
            Self::ObservationColumnLengthMismatch {
                timestamp_count,
                trade_price_count,
                trade_side_count,
            } => write!(
                formatter,
                "current-state estimation requires aligned observation columns, received timestamps={timestamp_count}, trade_prices={trade_price_count}, trade_sides={trade_side_count}"
            ),
            Self::InvalidObservedLogPrice { value } => write!(
                formatter,
                "current-state estimation requires a finite observed_log_price, found {value}"
            ),
            Self::InvalidObservationTradePrice {
                observation_index,
                value,
            } => write!(
                formatter,
                "current-state estimation requires observations[{observation_index}].trade_price to be finite and strictly positive, found {value}"
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
            Self::InvalidMeanReversionBoundaryState { source } => write!(
                formatter,
                "current-state estimation produced an invalid mean-reversion boundary state: {source}"
            ),
        }
    }
}

impl Error for CurrentStateEstimationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMeanReversionFittedParameters { source } => Some(source),
            Self::InvalidMeanReversionBoundaryState { source } => Some(source),
            Self::EmptyObservations
            | Self::ObservationColumnLengthMismatch { .. }
            | Self::InvalidObservedLogPrice { .. }
            | Self::InvalidObservationTradePrice { .. }
            | Self::ObservationTimestampOutOfOrder { .. }
            | Self::ObservationAfterStateTime { .. } => None,
        }
    }
}

pub fn estimate_current_state_from_observed_data(
    input: CurrentStateEstimationInput,
) -> Result<CurrentStateEstimationOutput, CurrentStateEstimationError> {
    input
        .mean_reversion_fitted_parameters
        .validate()
        .map_err(
            |source| CurrentStateEstimationError::InvalidMeanReversionFittedParameters { source },
        )?;

    if input.observation_timestamp_utc_nanos.is_empty() {
        return Err(CurrentStateEstimationError::EmptyObservations);
    }
    if input.observation_timestamp_utc_nanos.len() != input.observation_trade_prices.len()
        || input.observation_timestamp_utc_nanos.len() != input.observation_trade_sides.len()
    {
        return Err(
            CurrentStateEstimationError::ObservationColumnLengthMismatch {
                timestamp_count: input.observation_timestamp_utc_nanos.len(),
                trade_price_count: input.observation_trade_prices.len(),
                trade_side_count: input.observation_trade_sides.len(),
            },
        );
    }
    if !input.observed_log_price.is_finite() {
        return Err(CurrentStateEstimationError::InvalidObservedLogPrice {
            value: input.observed_log_price,
        });
    }

    validate_observations(
        input.state_time_utc_nanos,
        &input.observation_timestamp_utc_nanos,
        &input.observation_trade_prices,
    )?;

    let hawkes_excitation_state = replay_hawkes_state(
        input.kernel_parameters,
        input.state_time_utc_nanos,
        &input.observation_timestamp_utc_nanos,
        &input.observation_trade_sides,
    );

    let mean_reversion_boundary_state = MeanReversionBoundaryState::try_new(
        input.observed_log_price,
        input.observed_log_price,
        0.0,
        0.0,
    )
    .map_err(|source| CurrentStateEstimationError::InvalidMeanReversionBoundaryState { source })?;

    Ok(CurrentStateEstimationOutput {
        hawkes_excitation_state,
        mean_reversion_boundary_state,
    })
}

fn validate_observations(
    state_time_utc_nanos: i64,
    observation_timestamp_utc_nanos: &[i64],
    observation_trade_prices: &[f64],
) -> Result<(), CurrentStateEstimationError> {
    let mut previous_timestamp_utc_nanos = observation_timestamp_utc_nanos[0];

    for (observation_index, (&timestamp_utc_nanos, &trade_price)) in observation_timestamp_utc_nanos
        .iter()
        .zip(observation_trade_prices.iter())
        .enumerate()
    {
        if !trade_price.is_finite() || trade_price <= 0.0 {
            return Err(CurrentStateEstimationError::InvalidObservationTradePrice {
                observation_index,
                value: trade_price,
            });
        }

        if timestamp_utc_nanos > state_time_utc_nanos {
            return Err(CurrentStateEstimationError::ObservationAfterStateTime {
                state_time_utc_nanos,
                observation_index,
                timestamp_utc_nanos,
            });
        }

        if observation_index > 0 && timestamp_utc_nanos < previous_timestamp_utc_nanos {
            return Err(
                CurrentStateEstimationError::ObservationTimestampOutOfOrder {
                    observation_index,
                    previous_timestamp_utc_nanos,
                    timestamp_utc_nanos,
                },
            );
        }

        previous_timestamp_utc_nanos = timestamp_utc_nanos;
    }

    Ok(())
}

fn replay_hawkes_state(
    kernel_parameters: HawkesKernelParameters,
    state_time_utc_nanos: i64,
    observation_timestamp_utc_nanos: &[i64],
    observation_trade_sides: &[HawkesEventSide],
) -> HawkesExcitationState {
    let mut current_state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };
    let mut previous_timestamp_utc_nanos = observation_timestamp_utc_nanos[0];

    for (&timestamp_utc_nanos, &trade_side) in observation_timestamp_utc_nanos
        .iter()
        .zip(observation_trade_sides.iter())
    {
        let delta_hours =
            duration_nanos_to_hours(timestamp_utc_nanos - previous_timestamp_utc_nanos);

        current_state = current_state.apply_shared_decay_drift(kernel_parameters.beta, delta_hours);
        current_state =
            current_state.apply_endogenous_event_jump(trade_side, kernel_parameters.alpha);

        previous_timestamp_utc_nanos = timestamp_utc_nanos;
    }

    let trailing_delta_hours =
        duration_nanos_to_hours(state_time_utc_nanos - previous_timestamp_utc_nanos);

    current_state.apply_shared_decay_drift(kernel_parameters.beta, trailing_delta_hours)
}
