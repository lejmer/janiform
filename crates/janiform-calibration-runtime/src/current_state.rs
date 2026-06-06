use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelParameters,
    HawkesStationaryTargetSourceRates,
};
use janiform_model_kernels::long_variance::{
    LongVarianceParameters, LongVarianceParametersError, LongVarianceState, LongVarianceStateError,
    LongVarianceStateTransitionError, LongVarianceStationaryLevelError,
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
    pub long_variance_parameters: LongVarianceParameters,
    pub mean_reversion_fitted_parameters: MeanReversionFittedParameters,
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
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    InvalidObservedLogReturn {
        value: f64,
    },
    InvalidReferencePrice {
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
    InvalidLongVarianceParameters {
        source: LongVarianceParametersError,
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
            Self::InvalidObservedLogReturn { value } => write!(
                formatter,
                "current-state estimation requires a finite observed_log_return, found {value}"
            ),
            Self::InvalidReferencePrice { value } => write!(
                formatter,
                "batch current-state estimation requires a finite, strictly positive reference_price, found {value}"
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
            Self::InvalidLongVarianceParameters { source } => write!(
                formatter,
                "current-state estimation received invalid long-variance parameters: {source}"
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
        }
    }
}

impl Error for CurrentStateEstimationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMeanReversionFittedParameters { source } => Some(source),
            Self::InvalidLongVarianceParameters { source } => Some(source),
            Self::InvalidLongVarianceStationaryLevel { source } => Some(source),
            Self::InvalidLongVarianceStateTransition { source } => Some(source),
            Self::InvalidLongVarianceBoundaryState { source } => Some(source),
            Self::InvalidMeanReversionBoundaryState { source } => Some(source),
            Self::EmptyObservations
            | Self::EmptyStateTimes
            | Self::ObservationColumnLengthMismatch { .. }
            | Self::StateTimeOutOfOrder { .. }
            | Self::StateTimeBeforeFirstObservation { .. }
            | Self::InvalidObservedLogReturn { .. }
            | Self::InvalidReferencePrice { .. }
            | Self::InvalidObservationTradePrice { .. }
            | Self::ObservationTimestampOutOfOrder { .. }
            | Self::ObservationAfterStateTime { .. }
            | Self::InvalidLongVarianceStationaryIntensityMultipliers => None,
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

    input
        .long_variance_parameters
        .validate()
        .map_err(|source| CurrentStateEstimationError::InvalidLongVarianceParameters { source })?;

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

    if !input.observed_log_return.is_finite() {
        return Err(CurrentStateEstimationError::InvalidObservedLogReturn {
            value: input.observed_log_return,
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

    let long_variance_state = replay_long_variance_state(
        input.kernel_parameters,
        input.long_variance_parameters,
        input.state_time_utc_nanos,
        &input.observation_timestamp_utc_nanos,
        &input.observation_trade_sides,
    )?;

    let mean_reversion_boundary_state = MeanReversionBoundaryState::try_new(
        input.observed_log_return,
        input.observed_log_return,
        0.0,
        0.0,
    )
    .map_err(|source| CurrentStateEstimationError::InvalidMeanReversionBoundaryState { source })?;

    Ok(CurrentStateEstimationOutput {
        hawkes_excitation_state,
        long_variance_state,
        mean_reversion_boundary_state,
    })
}

pub fn estimate_current_states_from_observed_data(
    input: CurrentStateEstimationBatchInput,
) -> Result<Vec<CurrentStateEstimationBatchItem>, CurrentStateEstimationError> {
    estimate_current_states_from_observed_data_with_observer(input, None)
}

pub fn estimate_current_states_from_observed_data_with_observer(
    input: CurrentStateEstimationBatchInput,
    mut observer: Option<&mut dyn FnMut(CurrentStateEstimationBatchProgressEvent)>,
) -> Result<Vec<CurrentStateEstimationBatchItem>, CurrentStateEstimationError> {
    input
        .mean_reversion_fitted_parameters
        .validate()
        .map_err(
            |source| CurrentStateEstimationError::InvalidMeanReversionFittedParameters { source },
        )?;

    input
        .long_variance_parameters
        .validate()
        .map_err(|source| CurrentStateEstimationError::InvalidLongVarianceParameters { source })?;

    if input.state_times_utc_nanos.is_empty() {
        return Err(CurrentStateEstimationError::EmptyStateTimes);
    }

    if input.observation_timestamp_utc_nanos.is_empty() {
        return Err(CurrentStateEstimationError::EmptyObservations);
    }

    if !input.reference_price.is_finite() || input.reference_price <= 0.0 {
        return Err(CurrentStateEstimationError::InvalidReferencePrice {
            value: input.reference_price,
        });
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

    validate_state_times(input.state_times_utc_nanos.as_slice())?;

    let final_state_time_utc_nanos =
        input.state_times_utc_nanos[input.state_times_utc_nanos.len() - 1];

    let retained_observation_count = input
        .observation_timestamp_utc_nanos
        .iter()
        .take_while(|&&timestamp_utc_nanos| timestamp_utc_nanos <= final_state_time_utc_nanos)
        .count();

    if retained_observation_count == 0 {
        return Err(
            CurrentStateEstimationError::StateTimeBeforeFirstObservation {
                state_index: 0,
                state_time_utc_nanos: input.state_times_utc_nanos[0],
                first_observation_timestamp_utc_nanos: input.observation_timestamp_utc_nanos[0],
            },
        );
    }

    validate_observations(
        final_state_time_utc_nanos,
        &input.observation_timestamp_utc_nanos[0..retained_observation_count],
        &input.observation_trade_prices[0..retained_observation_count],
    )?;

    emit_batch_progress(
        &mut observer,
        0,
        retained_observation_count,
        0,
        input.state_times_utc_nanos.len(),
    );

    let mut current_state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };

    let mut current_long_variance_state = LongVarianceState::unit();

    let stationary_long_variance_level = long_variance_stationary_mean_reversion_level(
        input.kernel_parameters,
        input.long_variance_parameters,
    )?;

    let mut previous_timestamp_utc_nanos = input.observation_timestamp_utc_nanos[0];
    let mut observation_index = 0usize;
    let mut observed_price = None::<f64>;
    let mut state_items = Vec::with_capacity(input.state_times_utc_nanos.len());

    for (state_index, &state_time_utc_nanos) in input.state_times_utc_nanos.iter().enumerate() {
        while observation_index < retained_observation_count
            && input.observation_timestamp_utc_nanos[observation_index] <= state_time_utc_nanos
        {
            let timestamp_utc_nanos = input.observation_timestamp_utc_nanos[observation_index];

            let delta_hours =
                duration_nanos_to_hours(timestamp_utc_nanos - previous_timestamp_utc_nanos);

            current_state =
                current_state.apply_shared_decay_drift(input.kernel_parameters.beta, delta_hours);

            current_state = current_state.apply_endogenous_event_jump(
                input.observation_trade_sides[observation_index],
                input.kernel_parameters.alpha,
            );

            current_long_variance_state = current_long_variance_state
                .apply_mean_reversion_drift(
                    input.long_variance_parameters,
                    stationary_long_variance_level,
                    delta_hours,
                )
                .and_then(|state| {
                    state.apply_event_jump(
                        input.long_variance_parameters,
                        input.observation_trade_sides[observation_index],
                    )
                })
                .map_err(|source| {
                    CurrentStateEstimationError::InvalidLongVarianceStateTransition { source }
                })?;

            previous_timestamp_utc_nanos = timestamp_utc_nanos;
            observed_price = Some(input.observation_trade_prices[observation_index]);

            observation_index += 1;
        }

        let Some(observed_price) = observed_price else {
            return Err(
                CurrentStateEstimationError::StateTimeBeforeFirstObservation {
                    state_index,
                    state_time_utc_nanos,
                    first_observation_timestamp_utc_nanos: input.observation_timestamp_utc_nanos[0],
                },
            );
        };

        let observed_log_return = observed_price.ln() - input.reference_price.ln();

        if !observed_log_return.is_finite() {
            return Err(CurrentStateEstimationError::InvalidObservedLogReturn {
                value: observed_log_return,
            });
        }

        let trailing_delta_hours =
            duration_nanos_to_hours(state_time_utc_nanos - previous_timestamp_utc_nanos);

        let hawkes_excitation_state = current_state
            .apply_shared_decay_drift(input.kernel_parameters.beta, trailing_delta_hours);

        let long_variance_state = current_long_variance_state
            .apply_mean_reversion_drift(
                input.long_variance_parameters,
                stationary_long_variance_level,
                trailing_delta_hours,
            )
            .map_err(
                |source| CurrentStateEstimationError::InvalidLongVarianceStateTransition { source },
            )?;

        let mean_reversion_boundary_state =
            MeanReversionBoundaryState::try_new(observed_log_return, observed_log_return, 0.0, 0.0)
                .map_err(|source| {
                    CurrentStateEstimationError::InvalidMeanReversionBoundaryState { source }
                })?;

        state_items.push(CurrentStateEstimationBatchItem {
            state_time_utc_nanos,
            observed_price,
            state: CurrentStateEstimationOutput {
                hawkes_excitation_state,
                long_variance_state,
                mean_reversion_boundary_state,
            },
        });

        emit_batch_progress(
            &mut observer,
            observation_index,
            retained_observation_count,
            state_index + 1,
            input.state_times_utc_nanos.len(),
        );
    }

    Ok(state_items)
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

fn validate_state_times(state_times_utc_nanos: &[i64]) -> Result<(), CurrentStateEstimationError> {
    let mut previous_state_time_utc_nanos = state_times_utc_nanos[0];

    for (state_index, &state_time_utc_nanos) in state_times_utc_nanos.iter().enumerate() {
        if state_index > 0 && state_time_utc_nanos < previous_state_time_utc_nanos {
            return Err(CurrentStateEstimationError::StateTimeOutOfOrder {
                state_index,
                previous_state_time_utc_nanos,
                state_time_utc_nanos,
            });
        }

        previous_state_time_utc_nanos = state_time_utc_nanos;
    }

    Ok(())
}

fn emit_batch_progress(
    observer: &mut Option<&mut dyn FnMut(CurrentStateEstimationBatchProgressEvent)>,
    consumed_observation_count: usize,
    total_observation_count: usize,
    completed_state_count: usize,
    total_state_count: usize,
) {
    if let Some(observer) = observer.as_deref_mut() {
        observer(CurrentStateEstimationBatchProgressEvent {
            consumed_observation_count,
            total_observation_count,
            completed_state_count,
            total_state_count,
        });
    }
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

fn replay_long_variance_state(
    kernel_parameters: HawkesKernelParameters,
    long_variance_parameters: LongVarianceParameters,
    state_time_utc_nanos: i64,
    observation_timestamp_utc_nanos: &[i64],
    observation_trade_sides: &[HawkesEventSide],
) -> Result<LongVarianceState, CurrentStateEstimationError> {
    let stationary_long_variance_level =
        long_variance_stationary_mean_reversion_level(kernel_parameters, long_variance_parameters)?;

    let mut current_state = LongVarianceState::unit();
    let mut previous_timestamp_utc_nanos = observation_timestamp_utc_nanos[0];

    for (&timestamp_utc_nanos, &trade_side) in observation_timestamp_utc_nanos
        .iter()
        .zip(observation_trade_sides.iter())
    {
        let delta_hours =
            duration_nanos_to_hours(timestamp_utc_nanos - previous_timestamp_utc_nanos);

        current_state = current_state
            .apply_mean_reversion_drift(
                long_variance_parameters,
                stationary_long_variance_level,
                delta_hours,
            )
            .and_then(|state| state.apply_event_jump(long_variance_parameters, trade_side))
            .map_err(
                |source| CurrentStateEstimationError::InvalidLongVarianceStateTransition { source },
            )?;

        previous_timestamp_utc_nanos = timestamp_utc_nanos;
    }

    let trailing_delta_hours =
        duration_nanos_to_hours(state_time_utc_nanos - previous_timestamp_utc_nanos);

    current_state
        .apply_mean_reversion_drift(
            long_variance_parameters,
            stationary_long_variance_level,
            trailing_delta_hours,
        )
        .map_err(
            |source| CurrentStateEstimationError::InvalidLongVarianceStateTransition { source },
        )
}

fn long_variance_stationary_mean_reversion_level(
    kernel_parameters: HawkesKernelParameters,
    long_variance_parameters: LongVarianceParameters,
) -> Result<f64, CurrentStateEstimationError> {
    long_variance_parameters
        .stationary_mean_reversion_level_for_hawkes_kernel(
            kernel_parameters,
            HawkesStationaryTargetSourceRates::zero(),
        )
        .map_err(|source| match source {
            janiform_model_kernels::long_variance::LongVarianceStationaryLevelForHawkesKernelError::InvalidStationaryIntensityMultipliers => {
                CurrentStateEstimationError::InvalidLongVarianceStationaryIntensityMultipliers
            }
            janiform_model_kernels::long_variance::LongVarianceStationaryLevelForHawkesKernelError::InvalidStationaryLevel { source } => {
                CurrentStateEstimationError::InvalidLongVarianceStationaryLevel { source }
            }
        })
}

#[cfg(test)]
mod tests {
    use janiform_model_kernels::hawkes::{HawkesKernelLoadings, HawkesKernelParameters};
    use janiform_model_kernels::long_variance::LongVarianceParameters;
    use janiform_model_kernels::mean_reversion::{
        MeanReversionAnchorModel, MeanReversionFittedParameters,
    };

    use super::{
        CurrentStateEstimationBatchInput, CurrentStateEstimationError, CurrentStateEstimationInput,
        estimate_current_state_from_observed_data, estimate_current_states_from_observed_data,
        estimate_current_states_from_observed_data_with_observer,
    };
    use crate::current_state::HawkesEventSide;

    const NANOS_PER_HOUR: i64 = 3_600_000_000_000;

    #[test]
    fn batch_current_state_matches_repeated_single_state_estimation() {
        let input = batch_input_fixture(vec![NANOS_PER_HOUR, 3 * NANOS_PER_HOUR]);

        let batch_output = estimate_current_states_from_observed_data(input.clone())
            .expect("batch current-state estimation should succeed");

        let repeated_output = input
            .state_times_utc_nanos
            .iter()
            .map(|&state_time_utc_nanos| {
                let retained_observation_count = input
                    .observation_timestamp_utc_nanos
                    .iter()
                    .take_while(|&&timestamp_utc_nanos| timestamp_utc_nanos <= state_time_utc_nanos)
                    .count();
                let observed_price = input.observation_trade_prices[retained_observation_count - 1];
                let observed_log_return = observed_price.ln() - input.reference_price.ln();

                estimate_current_state_from_observed_data(CurrentStateEstimationInput {
                    kernel_parameters: input.kernel_parameters,
                    long_variance_parameters: input.long_variance_parameters,
                    mean_reversion_fitted_parameters: input
                        .mean_reversion_fitted_parameters
                        .clone(),
                    state_time_utc_nanos,
                    observed_log_return,
                    observation_timestamp_utc_nanos: input.observation_timestamp_utc_nanos
                        [0..retained_observation_count]
                        .to_vec(),
                    observation_trade_prices: input.observation_trade_prices
                        [0..retained_observation_count]
                        .to_vec(),
                    observation_trade_sides: input.observation_trade_sides
                        [0..retained_observation_count]
                        .to_vec(),
                })
                .expect("single current-state estimation should succeed")
            })
            .collect::<Vec<_>>();

        assert_eq!(
            batch_output
                .iter()
                .map(|item| item.state.clone())
                .collect::<Vec<_>>(),
            repeated_output
        );
    }

    #[test]
    fn batch_current_state_emits_observation_and_state_progress() {
        let input = batch_input_fixture(vec![NANOS_PER_HOUR, 2 * NANOS_PER_HOUR]);
        let mut events = Vec::new();

        estimate_current_states_from_observed_data_with_observer(
            input,
            Some(&mut |event| events.push(event)),
        )
        .expect("batch current-state estimation should succeed");

        assert_eq!(
            events
                .first()
                .expect("initial progress should be emitted")
                .completed_state_count,
            0
        );
        assert_eq!(
            events
                .last()
                .expect("final progress should be emitted")
                .completed_state_count,
            2
        );
        assert_eq!(
            events
                .last()
                .expect("final progress should be emitted")
                .consumed_observation_count,
            4
        );
    }

    #[test]
    fn batch_current_state_rejects_out_of_order_state_times() {
        let input = batch_input_fixture(vec![2 * NANOS_PER_HOUR, NANOS_PER_HOUR]);

        assert!(matches!(
            estimate_current_states_from_observed_data(input),
            Err(CurrentStateEstimationError::StateTimeOutOfOrder { state_index: 1, .. })
        ));
    }

    fn batch_input_fixture(state_times_utc_nanos: Vec<i64>) -> CurrentStateEstimationBatchInput {
        CurrentStateEstimationBatchInput {
            kernel_parameters: HawkesKernelParameters::try_new(
                0.5,
                std::f64::consts::LN_2,
                HawkesKernelLoadings {
                    plus_plus: 0.30,
                    plus_minus: 0.10,
                    minus_plus: 0.20,
                    minus_minus: 0.40,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.05,
                    plus_minus: 0.02,
                    minus_plus: 0.03,
                    minus_minus: 0.04,
                },
            )
            .expect("test kernel parameters should validate"),
            long_variance_parameters: LongVarianceParameters::neutral(),
            mean_reversion_fitted_parameters: MeanReversionFittedParameters::try_new(
                0.25,
                MeanReversionAnchorModel::endogenous_core_pressure_level(24.0, 0.5, 0.75),
            )
            .expect("test mean-reversion parameters should validate"),
            reference_price: 100.0,
            state_times_utc_nanos,
            observation_timestamp_utc_nanos: vec![
                0,
                NANOS_PER_HOUR,
                NANOS_PER_HOUR,
                2 * NANOS_PER_HOUR,
                3 * NANOS_PER_HOUR,
            ],
            observation_trade_prices: vec![100.0, 100.25, 100.5, 101.0, 100.75],
            observation_trade_sides: vec![
                HawkesEventSide::Buy,
                HawkesEventSide::Sell,
                HawkesEventSide::Buy,
                HawkesEventSide::Sell,
                HawkesEventSide::Buy,
            ],
        }
    }
}
