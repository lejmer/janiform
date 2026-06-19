use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelParameters,
};
use janiform_model_kernels::long_variance::{LongVarianceParameters, LongVarianceState};
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionFittedParameters,
};
use serde::{Deserialize, Serialize};

use crate::current_state::{
    CurrentStateEstimationError, CurrentStateEstimationOutput, MeanReversionBoundaryReplayState,
    advance_mean_reversion_boundary_replay_from_observed_price,
    long_variance_stationary_mean_reversion_level, zero_hawkes_excitation_state,
};

/// Rust-native input for advancing a fitted-model current-state tracker.
#[derive(Debug, Clone, PartialEq)]
pub struct CurrentStateTrackerUpdateInput {
    pub kernel_parameters: HawkesKernelParameters,
    pub long_variance_parameters: LongVarianceParameters,
    pub mean_reversion_fitted_parameters: MeanReversionFittedParameters,
    pub reference_price: f64,
    pub previous_snapshot: Option<CurrentStateTrackerSnapshot>,
    pub state_time_utc_nanos: i64,
    pub observed_price: Option<f64>,
    pub observation_timestamp_utc_nanos: Vec<i64>,
    pub observation_trade_prices: Vec<f64>,
    pub observation_trade_sides: Vec<HawkesEventSide>,
}

/// Durable tracker snapshot at one fitted-model state boundary.
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentStateTrackerSnapshot {
    pub state_time_utc_nanos: i64,
    pub observed_price: f64,
    pub state: CurrentStateEstimationOutput,
}

pub fn update_current_state_tracker(
    input: CurrentStateTrackerUpdateInput,
) -> Result<CurrentStateTrackerSnapshot, CurrentStateEstimationError> {
    validate_tracker_update_input(&input)?;

    let previous_snapshot = input.previous_snapshot.as_ref();

    let stationary_long_variance_level = long_variance_stationary_mean_reversion_level(
        input.kernel_parameters,
        input.long_variance_parameters,
    )?;

    let tracker_state =
        replay_tracker_update(&input, previous_snapshot, stationary_long_variance_level)?;

    let observed_price = tracker_state
        .observed_price
        .ok_or(CurrentStateEstimationError::MissingObservedPrice)?;
    
    let mean_reversion_boundary_state = tracker_state
        .mean_reversion_boundary_state
        .ok_or(CurrentStateEstimationError::MissingObservedPrice)?;

    Ok(CurrentStateTrackerSnapshot {
        state_time_utc_nanos: input.state_time_utc_nanos,
        observed_price,
        state: CurrentStateEstimationOutput {
            hawkes_excitation_state: tracker_state.hawkes_excitation_state,
            long_variance_state: tracker_state.long_variance_state,
            mean_reversion_boundary_state,
        },
    })
}

#[derive(Clone, Debug, PartialEq)]
struct ReplayedTrackerState {
    hawkes_excitation_state: HawkesExcitationState,
    long_variance_state: LongVarianceState,
    mean_reversion_boundary_state: Option<MeanReversionBoundaryState>,
    observed_price: Option<f64>,
}

fn validate_tracker_update_input(
    input: &CurrentStateTrackerUpdateInput,
) -> Result<(), CurrentStateEstimationError> {
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

    let previous_snapshot = input.previous_snapshot.as_ref();

    validate_tracker_previous_snapshot(previous_snapshot)?;

    validate_tracker_observations(
        input.state_time_utc_nanos,
        previous_snapshot,
        input.observation_timestamp_utc_nanos.as_slice(),
        input.observation_trade_prices.as_slice(),
    )
}

fn replay_tracker_update(
    input: &CurrentStateTrackerUpdateInput,
    previous_snapshot: Option<&CurrentStateTrackerSnapshot>,
    stationary_long_variance_level: f64,
) -> Result<ReplayedTrackerState, CurrentStateEstimationError> {
    let mut hawkes_state = previous_snapshot
        .map(|snapshot| snapshot.state.hawkes_excitation_state)
        .unwrap_or_else(zero_hawkes_excitation_state);

    let mut long_variance_state = previous_snapshot
        .map(|snapshot| snapshot.state.long_variance_state)
        .unwrap_or_else(LongVarianceState::unit);

    let mut mean_reversion_replay_state =
        previous_snapshot.map(|snapshot| MeanReversionBoundaryReplayState {
            timestamp_utc_nanos: snapshot.state_time_utc_nanos,
            // Copy the small boundary state out of the durable snapshot so replay can own it.
            boundary_state: snapshot.state.mean_reversion_boundary_state.clone(),
        });

    let mut previous_timestamp_utc_nanos = tracker_start_timestamp(
        previous_snapshot,
        input.state_time_utc_nanos,
        input.observation_timestamp_utc_nanos.as_slice(),
    );

    let mut observed_price = previous_snapshot.map(|snapshot| snapshot.observed_price);
    
    let reference_log_price = input.reference_price.ln();

    for (observation_index, ((&timestamp_utc_nanos, &trade_price), &trade_side)) in input
        .observation_timestamp_utc_nanos
        .iter()
        .zip(input.observation_trade_prices.iter())
        .zip(input.observation_trade_sides.iter())
        .enumerate()
    {
        let delta_hours =
            duration_nanos_to_hours(timestamp_utc_nanos - previous_timestamp_utc_nanos);

        hawkes_state =
            hawkes_state.apply_shared_decay_drift(input.kernel_parameters.beta, delta_hours);

        hawkes_state =
            hawkes_state.apply_endogenous_event_jump(trade_side, input.kernel_parameters.alpha);

        long_variance_state = long_variance_state
            .apply_mean_reversion_drift(
                input.long_variance_parameters,
                stationary_long_variance_level,
                delta_hours,
            )
            .and_then(|state| state.apply_event_jump(input.long_variance_parameters, trade_side))
            .map_err(
                |source| CurrentStateEstimationError::InvalidLongVarianceStateTransition { source },
            )?;

        mean_reversion_replay_state =
            Some(advance_mean_reversion_boundary_replay_from_observed_price(
                mean_reversion_replay_state,
                &input.mean_reversion_fitted_parameters,
                reference_log_price,
                timestamp_utc_nanos,
                trade_price,
                observation_index,
            )?);

        previous_timestamp_utc_nanos = timestamp_utc_nanos;
        observed_price = Some(trade_price);
    }

    let observed_price = input.observed_price.or(observed_price);
    
    let mean_reversion_boundary_state = match observed_price {
        Some(observed_price) => {
            let replay_state = advance_mean_reversion_boundary_replay_from_observed_price(
                mean_reversion_replay_state,
                &input.mean_reversion_fitted_parameters,
                reference_log_price,
                input.state_time_utc_nanos,
                observed_price,
                0,
            )?;

            Some(replay_state.boundary_state)
        }
        None => None,
    };

    let trailing_delta_hours =
        duration_nanos_to_hours(input.state_time_utc_nanos - previous_timestamp_utc_nanos);

    Ok(ReplayedTrackerState {
        hawkes_excitation_state: hawkes_state
            .apply_shared_decay_drift(input.kernel_parameters.beta, trailing_delta_hours),
        long_variance_state: long_variance_state
            .apply_mean_reversion_drift(
                input.long_variance_parameters,
                stationary_long_variance_level,
                trailing_delta_hours,
            )
            .map_err(
                |source| CurrentStateEstimationError::InvalidLongVarianceStateTransition { source },
            )?,
        mean_reversion_boundary_state,
        observed_price,
    })
}

fn validate_tracker_previous_snapshot(
    previous_snapshot: Option<&CurrentStateTrackerSnapshot>,
) -> Result<(), CurrentStateEstimationError> {
    let Some(previous_snapshot) = previous_snapshot else {
        return Ok(());
    };

    if !previous_snapshot.observed_price.is_finite() || previous_snapshot.observed_price <= 0.0 {
        return Err(CurrentStateEstimationError::InvalidPreviousObservedPrice {
            value: previous_snapshot.observed_price,
        });
    }

    previous_snapshot
        .state
        .hawkes_excitation_state
        .validate()
        .map_err(|source| CurrentStateEstimationError::InvalidHawkesExcitationState { source })?;

    previous_snapshot
        .state
        .long_variance_state
        .validate()
        .map_err(|source| CurrentStateEstimationError::InvalidLongVarianceState { source })?;

    previous_snapshot
        .state
        .mean_reversion_boundary_state
        .validate()
        .map_err(|source| CurrentStateEstimationError::InvalidMeanReversionBoundaryState { source })
}

fn validate_tracker_observations(
    state_time_utc_nanos: i64,
    previous_snapshot: Option<&CurrentStateTrackerSnapshot>,
    observation_timestamp_utc_nanos: &[i64],
    observation_trade_prices: &[f64],
) -> Result<(), CurrentStateEstimationError> {
    if let Some(previous_snapshot) = previous_snapshot
        && state_time_utc_nanos < previous_snapshot.state_time_utc_nanos
    {
        return Err(CurrentStateEstimationError::StateTimeBeforePreviousState {
            previous_state_time_utc_nanos: previous_snapshot.state_time_utc_nanos,
            state_time_utc_nanos,
        });
    }

    let mut previous_timestamp_utc_nanos =
        previous_snapshot.map(|snapshot| snapshot.state_time_utc_nanos);

    for (observation_index, (&timestamp_utc_nanos, &trade_price)) in observation_timestamp_utc_nanos
        .iter()
        .zip(observation_trade_prices.iter())
        .enumerate()
    {
        validate_tracker_observation(
            state_time_utc_nanos,
            previous_snapshot,
            previous_timestamp_utc_nanos,
            observation_index,
            timestamp_utc_nanos,
            trade_price,
        )?;

        previous_timestamp_utc_nanos = Some(timestamp_utc_nanos);
    }

    Ok(())
}

fn validate_tracker_observation(
    state_time_utc_nanos: i64,
    previous_snapshot: Option<&CurrentStateTrackerSnapshot>,
    previous_timestamp_utc_nanos: Option<i64>,
    observation_index: usize,
    timestamp_utc_nanos: i64,
    trade_price: f64,
) -> Result<(), CurrentStateEstimationError> {
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

    if let Some(previous_snapshot) = previous_snapshot
        && timestamp_utc_nanos < previous_snapshot.state_time_utc_nanos
    {
        return Err(
            CurrentStateEstimationError::ObservationBeforePreviousStateTime {
                observation_index,
                previous_state_time_utc_nanos: previous_snapshot.state_time_utc_nanos,
                timestamp_utc_nanos,
            },
        );
    }

    if let Some(previous_timestamp_utc_nanos) = previous_timestamp_utc_nanos
        && timestamp_utc_nanos < previous_timestamp_utc_nanos
    {
        return Err(
            CurrentStateEstimationError::ObservationTimestampOutOfOrder {
                observation_index,
                previous_timestamp_utc_nanos,
                timestamp_utc_nanos,
            },
        );
    }

    Ok(())
}

fn tracker_start_timestamp(
    previous_snapshot: Option<&CurrentStateTrackerSnapshot>,
    state_time_utc_nanos: i64,
    observation_timestamp_utc_nanos: &[i64],
) -> i64 {
    if let Some(previous_snapshot) = previous_snapshot {
        return previous_snapshot.state_time_utc_nanos;
    }

    observation_timestamp_utc_nanos
        .first()
        .copied()
        .unwrap_or(state_time_utc_nanos)
}

#[cfg(test)]
mod tests {
    use super::{CurrentStateTrackerUpdateInput, update_current_state_tracker};
    use crate::current_state::estimate_current_states_from_observed_data;
    use crate::current_state_test_support::{NANOS_PER_HOUR, batch_input_fixture};

    #[test]
    fn tracker_update_matches_batch_state_across_chunks() {
        let input = batch_input_fixture(vec![NANOS_PER_HOUR, 3 * NANOS_PER_HOUR]);
        let batch_output = estimate_current_states_from_observed_data(input.clone())
            .expect("batch current-state estimation should succeed");

        let first_snapshot = update_current_state_tracker(CurrentStateTrackerUpdateInput {
            kernel_parameters: input.kernel_parameters,
            long_variance_parameters: input.long_variance_parameters,
            mean_reversion_fitted_parameters: input.mean_reversion_fitted_parameters.clone(),
            reference_price: input.reference_price,
            previous_snapshot: None,
            state_time_utc_nanos: NANOS_PER_HOUR,
            observed_price: None,
            observation_timestamp_utc_nanos: input.observation_timestamp_utc_nanos[0..3].to_vec(),
            observation_trade_prices: input.observation_trade_prices[0..3].to_vec(),
            observation_trade_sides: input.observation_trade_sides[0..3].to_vec(),
        })
        .expect("first tracker update should succeed");

        let second_snapshot = update_current_state_tracker(CurrentStateTrackerUpdateInput {
            kernel_parameters: input.kernel_parameters,
            long_variance_parameters: input.long_variance_parameters,
            mean_reversion_fitted_parameters: input.mean_reversion_fitted_parameters,
            reference_price: input.reference_price,
            previous_snapshot: Some(first_snapshot.clone()),
            state_time_utc_nanos: 3 * NANOS_PER_HOUR,
            observed_price: None,
            observation_timestamp_utc_nanos: input.observation_timestamp_utc_nanos[3..5].to_vec(),
            observation_trade_prices: input.observation_trade_prices[3..5].to_vec(),
            observation_trade_sides: input.observation_trade_sides[3..5].to_vec(),
        })
        .expect("second tracker update should succeed");

        assert_eq!(first_snapshot.state, batch_output[0].state);
        assert_eq!(second_snapshot.state, batch_output[1].state);
        assert_ne!(
            second_snapshot
                .state
                .mean_reversion_boundary_state
                .pressure_state,
            0.0
        );
        assert_ne!(
            second_snapshot
                .state
                .mean_reversion_boundary_state
                .deviation,
            0.0
        );
    }

    #[test]
    fn tracker_update_advances_without_new_observations() {
        let mut input = batch_input_fixture(vec![NANOS_PER_HOUR, 2 * NANOS_PER_HOUR]);
        input.observation_timestamp_utc_nanos.truncate(3);
        input.observation_trade_prices.truncate(3);
        input.observation_trade_sides.truncate(3);

        let batch_output = estimate_current_states_from_observed_data(input.clone())
            .expect("batch current-state estimation should succeed");

        let first_snapshot = update_current_state_tracker(CurrentStateTrackerUpdateInput {
            kernel_parameters: input.kernel_parameters,
            long_variance_parameters: input.long_variance_parameters,
            mean_reversion_fitted_parameters: input.mean_reversion_fitted_parameters.clone(),
            reference_price: input.reference_price,
            previous_snapshot: None,
            state_time_utc_nanos: NANOS_PER_HOUR,
            observed_price: None,
            observation_timestamp_utc_nanos: input.observation_timestamp_utc_nanos[0..3].to_vec(),
            observation_trade_prices: input.observation_trade_prices[0..3].to_vec(),
            observation_trade_sides: input.observation_trade_sides[0..3].to_vec(),
        })
        .expect("first tracker update should succeed");

        let advanced_snapshot = update_current_state_tracker(CurrentStateTrackerUpdateInput {
            kernel_parameters: input.kernel_parameters,
            long_variance_parameters: input.long_variance_parameters,
            mean_reversion_fitted_parameters: input.mean_reversion_fitted_parameters,
            reference_price: input.reference_price,
            previous_snapshot: Some(first_snapshot.clone()),
            state_time_utc_nanos: 2 * NANOS_PER_HOUR,
            observed_price: None,
            observation_timestamp_utc_nanos: Vec::new(),
            observation_trade_prices: Vec::new(),
            observation_trade_sides: Vec::new(),
        })
        .expect("decay-only tracker update should succeed");

        assert_eq!(advanced_snapshot.state_time_utc_nanos, 2 * NANOS_PER_HOUR);
        assert_eq!(
            advanced_snapshot.observed_price,
            first_snapshot.observed_price
        );
        assert!(
            advanced_snapshot.state.hawkes_excitation_state.y_plus_plus
                < first_snapshot.state.hawkes_excitation_state.y_plus_plus
        );
        assert_eq!(advanced_snapshot.state, batch_output[1].state);
        assert_ne!(
            first_snapshot
                .state
                .mean_reversion_boundary_state
                .pressure_state,
            0.0
        );
        assert_ne!(
            advanced_snapshot
                .state
                .mean_reversion_boundary_state
                .pressure_state,
            0.0
        );
        assert_ne!(
            advanced_snapshot
                .state
                .mean_reversion_boundary_state
                .deviation,
            0.0
        );
    }
}
