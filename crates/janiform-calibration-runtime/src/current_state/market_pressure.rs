use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::{HawkesEventSide, HawkesKernelParameters};
use janiform_model_kernels::market_pressure::{
    MarketPressureParameters, MarketPressureState, condition_market_pressure_state_on_event_side,
    evolve_market_pressure_state,
};

use super::contracts::CurrentStateEstimationError;

pub(crate) fn replay_market_pressure_state(
    _kernel_parameters: HawkesKernelParameters,
    market_pressure_parameters: &MarketPressureParameters,
    _reference_price: f64,
    state_time_utc_nanos: i64,
    observation_timestamp_utc_nanos: &[i64],
    observation_trade_prices: &[f64],
    observation_trade_sides: &[HawkesEventSide],
) -> Result<MarketPressureState, CurrentStateEstimationError> {
    if observation_timestamp_utc_nanos.is_empty() {
        return Err(CurrentStateEstimationError::EmptyObservations);
    }

    if observation_timestamp_utc_nanos.len() != observation_trade_sides.len() {
        return Err(
            CurrentStateEstimationError::MarketPressureObservationColumnLengthMismatch {
                timestamp_count: observation_timestamp_utc_nanos.len(),
                trade_side_count: observation_trade_sides.len(),
            },
        );
    }

    if observation_timestamp_utc_nanos.len() != observation_trade_prices.len() {
        return Err(
            CurrentStateEstimationError::ObservationColumnLengthMismatch {
                timestamp_count: observation_timestamp_utc_nanos.len(),
                trade_price_count: observation_trade_prices.len(),
                trade_side_count: observation_trade_sides.len(),
            },
        );
    }

    let mut market_pressure_state =
        MarketPressureState::neutral_for_parameters(market_pressure_parameters)
            .map_err(|source| CurrentStateEstimationError::InvalidMarketPressureState { source })?;
    let mut previous_timestamp_utc_nanos = observation_timestamp_utc_nanos[0];

    for (&timestamp_utc_nanos, &trade_side) in observation_timestamp_utc_nanos
        .iter()
        .zip(observation_trade_sides.iter())
    {
        market_pressure_state = advance_market_pressure_state_from_observed_side(
            market_pressure_parameters,
            &market_pressure_state,
            previous_timestamp_utc_nanos,
            timestamp_utc_nanos,
            trade_side,
        )?;

        previous_timestamp_utc_nanos = timestamp_utc_nanos;
    }

    advance_market_pressure_state_to_timestamp(
        market_pressure_parameters,
        &market_pressure_state,
        previous_timestamp_utc_nanos,
        state_time_utc_nanos,
    )
}

pub(crate) fn advance_market_pressure_state_from_observed_side(
    market_pressure_parameters: &MarketPressureParameters,
    market_pressure_state: &MarketPressureState,
    previous_timestamp_utc_nanos: i64,
    timestamp_utc_nanos: i64,
    observed_trade_side: HawkesEventSide,
) -> Result<MarketPressureState, CurrentStateEstimationError> {
    let evolved_state = advance_market_pressure_state_to_timestamp(
        market_pressure_parameters,
        market_pressure_state,
        previous_timestamp_utc_nanos,
        timestamp_utc_nanos,
    )?;

    let observed_buy_side = matches!(observed_trade_side, HawkesEventSide::Buy);

    condition_market_pressure_state_on_event_side(
        market_pressure_parameters,
        &evolved_state,
        observed_buy_side,
    )
    .map_err(|source| CurrentStateEstimationError::InvalidMarketPressureStateTransition { source })
}

pub(crate) fn advance_market_pressure_state_to_timestamp(
    market_pressure_parameters: &MarketPressureParameters,
    market_pressure_state: &MarketPressureState,
    previous_timestamp_utc_nanos: i64,
    timestamp_utc_nanos: i64,
) -> Result<MarketPressureState, CurrentStateEstimationError> {
    let delta_hours = duration_nanos_to_hours(timestamp_utc_nanos - previous_timestamp_utc_nanos);

    evolve_market_pressure_state(
        market_pressure_parameters,
        market_pressure_state,
        delta_hours,
    )
    .map_err(|source| CurrentStateEstimationError::InvalidMarketPressureStateTransition { source })
}
