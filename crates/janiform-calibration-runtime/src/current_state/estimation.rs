use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelParameters,
    HawkesStationaryTargetSourceRates,
};
use janiform_model_kernels::long_variance::{
    LongVarianceParameters, LongVarianceState, return_energy_increment_from_log_return,
};

use super::contracts::{
    CurrentStateEstimationBatchInput, CurrentStateEstimationBatchItem,
    CurrentStateEstimationBatchProgressEvent, CurrentStateEstimationError,
    CurrentStateEstimationInput, CurrentStateEstimationOutput,
};
use super::market_pressure::{
    advance_market_pressure_state_from_observed_side, advance_market_pressure_state_to_timestamp,
    replay_market_pressure_state,
};
use super::mean_reversion::{
    MeanReversionBoundaryReplayState, advance_mean_reversion_boundary_replay,
    advance_mean_reversion_boundary_replay_from_observed_price,
    replay_mean_reversion_state_from_single_input,
};

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

    input
        .market_pressure_parameters
        .validate()
        .map_err(
            |source| CurrentStateEstimationError::InvalidMarketPressureParameters { source },
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

    if !input.reference_price.is_finite() || input.reference_price <= 0.0 {
        return Err(CurrentStateEstimationError::InvalidReferencePrice {
            value: input.reference_price,
        });
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
        &input.observation_trade_prices,
        &input.observation_trade_sides,
    )?;

    let mean_reversion_boundary_state = replay_mean_reversion_state_from_single_input(&input)?;

    let market_pressure_state = replay_market_pressure_state(
        input.kernel_parameters,
        &input.market_pressure_parameters,
        input.reference_price,
        input.state_time_utc_nanos,
        input.observation_timestamp_utc_nanos.as_slice(),
        input.observation_trade_prices.as_slice(),
        input.observation_trade_sides.as_slice(),
    )?;

    Ok(CurrentStateEstimationOutput {
        hawkes_excitation_state,
        long_variance_state,
        mean_reversion_boundary_state,
        market_pressure_state,
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

    input
        .market_pressure_parameters
        .validate()
        .map_err(
            |source| CurrentStateEstimationError::InvalidMarketPressureParameters { source },
        )?;

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

    let mut current_state = zero_hawkes_excitation_state();
    let mut current_long_variance_state = LongVarianceState::unit();

    let stationary_long_variance_level = long_variance_stationary_mean_reversion_level(
        input.kernel_parameters,
        input.long_variance_parameters,
    )?;

    let mut previous_timestamp_utc_nanos = input.observation_timestamp_utc_nanos[0];
    let mut observation_index = 0usize;
    let mut observed_price = None::<f64>;
    let mut previous_observation_trade_price = None::<f64>;
    let mut mean_reversion_replay_state = None::<MeanReversionBoundaryReplayState>;
    let mut current_market_pressure_state =
        janiform_model_kernels::market_pressure::MarketPressureState::neutral_for_parameters(
            &input.market_pressure_parameters,
        )
        .map_err(|source| CurrentStateEstimationError::InvalidMarketPressureState { source })?;
    let mut state_items = Vec::with_capacity(input.state_times_utc_nanos.len());

    let reference_log_price = input.reference_price.ln();

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

            let trade_price = input.observation_trade_prices[observation_index];

            current_long_variance_state = current_long_variance_state
                .apply_mean_reversion_drift(
                    input.long_variance_parameters,
                    stationary_long_variance_level,
                    delta_hours,
                )
                .and_then(|state| match previous_observation_trade_price {
                    Some(previous_trade_price) => state.apply_return_energy_jump(
                        input.long_variance_parameters,
                        return_energy_increment_from_log_return(
                            trade_price.ln() - previous_trade_price.ln(),
                        )?,
                    ),
                    None => Ok(state),
                })
                .and_then(|state| {
                    state.apply_event_jump(
                        input.long_variance_parameters,
                        input.observation_trade_sides[observation_index],
                    )
                })
                .map_err(|source| {
                    CurrentStateEstimationError::InvalidLongVarianceStateTransition { source }
                })?;

            current_market_pressure_state = advance_market_pressure_state_from_observed_side(
                &input.market_pressure_parameters,
                &current_market_pressure_state,
                previous_timestamp_utc_nanos,
                timestamp_utc_nanos,
                input.observation_trade_sides[observation_index],
            )?;

            mean_reversion_replay_state =
                Some(advance_mean_reversion_boundary_replay_from_observed_price(
                    mean_reversion_replay_state,
                    &input.mean_reversion_fitted_parameters,
                    reference_log_price,
                    timestamp_utc_nanos,
                    input.observation_trade_prices[observation_index],
                    observation_index,
                )?);

            previous_timestamp_utc_nanos = timestamp_utc_nanos;
            previous_observation_trade_price = Some(trade_price);
            observed_price = Some(trade_price);
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

        let observed_log_return = observed_price.ln() - reference_log_price;

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

        let state_time_mean_reversion_replay_state = advance_mean_reversion_boundary_replay(
            mean_reversion_replay_state,
            &input.mean_reversion_fitted_parameters,
            state_time_utc_nanos,
            observed_log_return,
        )?;
        let market_pressure_state = advance_market_pressure_state_to_timestamp(
            &input.market_pressure_parameters,
            &current_market_pressure_state,
            previous_timestamp_utc_nanos,
            state_time_utc_nanos,
        )?;

        // Copy the small boundary state into the output while retaining replay state for later boundaries.
        let mean_reversion_boundary_state = state_time_mean_reversion_replay_state.boundary_state;

        mean_reversion_replay_state = Some(state_time_mean_reversion_replay_state);

        state_items.push(CurrentStateEstimationBatchItem {
            state_time_utc_nanos,
            observed_price,
            state: CurrentStateEstimationOutput {
                hawkes_excitation_state,
                long_variance_state,
                mean_reversion_boundary_state,
                market_pressure_state,
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

pub(crate) fn zero_hawkes_excitation_state() -> HawkesExcitationState {
    HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    }
}

fn replay_hawkes_state(
    kernel_parameters: HawkesKernelParameters,
    state_time_utc_nanos: i64,
    observation_timestamp_utc_nanos: &[i64],
    observation_trade_sides: &[HawkesEventSide],
) -> HawkesExcitationState {
    let mut current_state = zero_hawkes_excitation_state();
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
    observation_trade_prices: &[f64],
    observation_trade_sides: &[HawkesEventSide],
) -> Result<LongVarianceState, CurrentStateEstimationError> {
    let stationary_long_variance_level =
        long_variance_stationary_mean_reversion_level(kernel_parameters, long_variance_parameters)?;

    let mut current_state = LongVarianceState::unit();
    let mut previous_timestamp_utc_nanos = observation_timestamp_utc_nanos[0];
    let mut previous_trade_price = None::<f64>;

    for ((&timestamp_utc_nanos, &trade_price), &trade_side) in observation_timestamp_utc_nanos
        .iter()
        .zip(observation_trade_prices.iter())
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
            .and_then(|state| match previous_trade_price {
                Some(previous_trade_price) => state.apply_return_energy_jump(
                    long_variance_parameters,
                    return_energy_increment_from_log_return(
                        trade_price.ln() - previous_trade_price.ln(),
                    )?,
                ),
                None => Ok(state),
            })
            .and_then(|state| state.apply_event_jump(long_variance_parameters, trade_side))
            .map_err(
                |source| CurrentStateEstimationError::InvalidLongVarianceStateTransition { source },
            )?;

        previous_timestamp_utc_nanos = timestamp_utc_nanos;
        previous_trade_price = Some(trade_price);
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

pub(crate) fn long_variance_stationary_mean_reversion_level(
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
    use super::super::mean_reversion::{
        MEAN_REVERSION_CURRENT_STATE_BOUNDARY_SEED_POLICY,
        seed_mean_reversion_boundary_state_from_observed_log_price,
    };
    use super::{
        CurrentStateEstimationError, estimate_current_states_from_observed_data,
        estimate_current_states_from_observed_data_with_observer,
    };
    use crate::current_state_test_support::{NANOS_PER_HOUR, batch_input_fixture};

    #[test]
    fn batch_mean_reversion_replay_uses_named_flat_first_observation_seed() {
        assert_eq!(
            MEAN_REVERSION_CURRENT_STATE_BOUNDARY_SEED_POLICY,
            "first_observed_log_price_flat_anchor_zero_deviation.v1"
        );

        let input = batch_input_fixture(vec![0]);

        let batch_output = estimate_current_states_from_observed_data(input.clone())
            .expect("batch current-state estimation should succeed");

        let expected_seed = seed_mean_reversion_boundary_state_from_observed_log_price(0.0)
            .expect("first observation seed should validate");

        assert_eq!(
            batch_output[0].state.mean_reversion_boundary_state,
            expected_seed
        );
    }

    #[test]
    fn batch_state_replay_propagates_market_pressure_state_and_deviation() {
        let input = batch_input_fixture(vec![3 * NANOS_PER_HOUR]);

        let batch_output = estimate_current_states_from_observed_data(input)
            .expect("batch current-state estimation should succeed");

        let output_state = &batch_output[0].state;
        let boundary_state = &output_state.mean_reversion_boundary_state;

        assert_eq!(output_state.market_pressure_state.state_weights.len(), 2);
        assert_ne!(
            output_state.market_pressure_state.state_weights[0],
            output_state.market_pressure_state.state_weights[1]
        );
        assert_ne!(boundary_state.deviation, 0.0);

        boundary_state
            .validate()
            .expect("replayed boundary state should validate");
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
}
