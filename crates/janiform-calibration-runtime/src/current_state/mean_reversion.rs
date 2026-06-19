use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionFittedParameters,
    evolve_mean_reversion_boundary_state_on_interval_grid,
    evolve_mean_reversion_boundary_state_with_endogenous_pressure,
};

use super::contracts::{CurrentStateEstimationError, CurrentStateEstimationInput};

#[cfg(test)]
pub(crate) const MEAN_REVERSION_CURRENT_STATE_BOUNDARY_SEED_POLICY: &str =
    "first_observed_log_price_flat_anchor_zero_pressure_deviation.v1";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MeanReversionBoundaryReplayState {
    pub(crate) timestamp_utc_nanos: i64,
    pub(crate) boundary_state: MeanReversionBoundaryState,
}

pub(crate) fn advance_mean_reversion_boundary_replay(
    replay_state: Option<MeanReversionBoundaryReplayState>,
    fitted_parameters: &MeanReversionFittedParameters,
    timestamp_utc_nanos: i64,
    observed_log_price: f64,
) -> Result<MeanReversionBoundaryReplayState, CurrentStateEstimationError> {
    if !observed_log_price.is_finite() {
        return Err(CurrentStateEstimationError::InvalidObservedLogReturn {
            value: observed_log_price,
        });
    }

    let Some(replay_state) = replay_state else {
        return Ok(MeanReversionBoundaryReplayState {
            timestamp_utc_nanos,
            boundary_state: seed_mean_reversion_boundary_state_from_observed_log_price(
                observed_log_price,
            )?,
        });
    };

    let interval_delta =
        duration_nanos_to_hours(timestamp_utc_nanos - replay_state.timestamp_utc_nanos);

    let boundary_state = evolve_mean_reversion_boundary_state_to_observed_log_price(
        replay_state.boundary_state,
        fitted_parameters,
        interval_delta,
        observed_log_price,
    )?;

    Ok(MeanReversionBoundaryReplayState {
        timestamp_utc_nanos,
        boundary_state,
    })
}

pub(crate) fn advance_mean_reversion_boundary_replay_from_observed_price(
    replay_state: Option<MeanReversionBoundaryReplayState>,
    fitted_parameters: &MeanReversionFittedParameters,
    reference_log_price: f64,
    timestamp_utc_nanos: i64,
    observed_price: f64,
    observation_index: usize,
) -> Result<MeanReversionBoundaryReplayState, CurrentStateEstimationError> {
    let observed_log_price = observed_log_price_from_reference_log_price(
        reference_log_price,
        observed_price,
        observation_index,
    )?;

    advance_mean_reversion_boundary_replay(
        replay_state,
        fitted_parameters,
        timestamp_utc_nanos,
        observed_log_price,
    )
}

pub(crate) fn replay_mean_reversion_state_from_single_input(
    input: &CurrentStateEstimationInput,
) -> Result<MeanReversionBoundaryState, CurrentStateEstimationError> {
    let mut replay_state = None::<MeanReversionBoundaryReplayState>;

    let reference_log_price = input.reference_price.ln();

    for (observation_index, (&timestamp_utc_nanos, &trade_price)) in input
        .observation_timestamp_utc_nanos
        .iter()
        .zip(input.observation_trade_prices.iter())
        .enumerate()
    {
        replay_state = Some(advance_mean_reversion_boundary_replay_from_observed_price(
            replay_state,
            &input.mean_reversion_fitted_parameters,
            reference_log_price,
            timestamp_utc_nanos,
            trade_price,
            observation_index,
        )?);
    }

    let replay_state = advance_mean_reversion_boundary_replay(
        replay_state,
        &input.mean_reversion_fitted_parameters,
        input.state_time_utc_nanos,
        input.observed_log_return,
    )?;

    Ok(replay_state.boundary_state)
}

pub(crate) fn observed_log_price_from_reference_log_price(
    reference_log_price: f64,
    observed_price: f64,
    observation_index: usize,
) -> Result<f64, CurrentStateEstimationError> {
    if !observed_price.is_finite() || observed_price <= 0.0 {
        return Err(CurrentStateEstimationError::InvalidObservationTradePrice {
            observation_index,
            value: observed_price,
        });
    }

    let observed_log_price = observed_price.ln() - reference_log_price;
    if !observed_log_price.is_finite() {
        return Err(CurrentStateEstimationError::InvalidObservedLogReturn {
            value: observed_log_price,
        });
    }

    Ok(observed_log_price)
}

pub(crate) fn seed_mean_reversion_boundary_state_from_observed_log_price(
    observed_log_price: f64,
) -> Result<MeanReversionBoundaryState, CurrentStateEstimationError> {
    MeanReversionBoundaryState::try_new(observed_log_price, observed_log_price, 0.0, 0.0)
        .map_err(|source| CurrentStateEstimationError::InvalidMeanReversionBoundaryState { source })
}

fn evolve_mean_reversion_boundary_state_to_observed_log_price(
    boundary_state: MeanReversionBoundaryState,
    fitted_parameters: &MeanReversionFittedParameters,
    interval_delta: f64,
    observed_log_price_after: f64,
) -> Result<MeanReversionBoundaryState, CurrentStateEstimationError> {
    let deviation_decay_factor = (-fitted_parameters.kappa * interval_delta).exp();

    let core_total_return = observed_log_price_after
        - boundary_state.anchor_value
        - deviation_decay_factor * boundary_state.deviation;

    let evolved_state = if interval_delta == 0.0 {
        let anchor_value = boundary_state.anchor_value;
        let pressure_state = boundary_state.pressure_state;

        evolve_mean_reversion_boundary_state_on_interval_grid(
            boundary_state,
            fitted_parameters.kappa,
            interval_delta,
            core_total_return,
            anchor_value,
            pressure_state,
        )
    } else {
        evolve_mean_reversion_boundary_state_with_endogenous_pressure(
            boundary_state,
            fitted_parameters,
            interval_delta,
            core_total_return,
            core_total_return,
        )
    };

    evolved_state.map_err(|source| {
        CurrentStateEstimationError::InvalidMeanReversionBoundaryStateEvolution { source }
    })
}
