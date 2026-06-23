use janiform_calibration_runtime::{
    CurrentStateEstimationError, CurrentStateEstimationInput, CurrentStateTrackerUpdateInput,
    estimate_current_state_from_observed_data, update_current_state_tracker,
};
use janiform_foundations::time::NANOS_PER_HOUR;
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::long_variance::{LongVarianceParameters, LongVarianceState};
use janiform_model_kernels::market_pressure::{
    MarketPressureMeasureParameters, MarketPressureParameters, MarketPressureTransitionRates,
};
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;

#[test]
fn current_state_estimation_replays_hawkes_state_and_mean_reversion_boundary() {
    let kernel_parameters = HawkesKernelParameters::try_new(
        0.8,
        1.6,
        HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.1,
            minus_plus: 0.1,
            minus_minus: 0.25,
        },
        HawkesKernelLoadings {
            plus_plus: 0.08,
            plus_minus: 0.04,
            minus_plus: 0.05,
            minus_minus: 0.1,
        },
    )
    .expect("test kernel parameters should be valid");

    let mean_reversion_fitted_parameters = MeanReversionFittedParameters::try_new(0.25)
        .expect("test mean-reversion fitted parameters should be valid");
    let market_pressure_parameters = test_market_pressure_parameters();
    let long_variance_parameters = LongVarianceParameters::try_new(2.0, 0.0, 0.0, 0.0, 0.25, 0.50)
        .expect("test long-variance parameters should be valid");

    let state_time_utc_nanos = 15 * NANOS_PER_HOUR;

    let estimation_output =
        estimate_current_state_from_observed_data(CurrentStateEstimationInput {
            kernel_parameters,
            long_variance_parameters,
            mean_reversion_fitted_parameters,
            market_pressure_parameters,
            reference_price: 100.0,
            state_time_utc_nanos,
            observed_log_return: 0.05,
            observation_timestamp_utc_nanos: vec![13 * NANOS_PER_HOUR, 14 * NANOS_PER_HOUR],
            observation_trade_prices: vec![100.0, 100.5],
            observation_trade_sides: vec![HawkesEventSide::Buy, HawkesEventSide::Sell],
        })
        .expect("current-state estimation should succeed");

    let decay_factor = (-1.6_f64).exp();
    assert_eq!(
        estimation_output.hawkes_excitation_state.y_plus_plus,
        0.2 * decay_factor * decay_factor
    );
    assert_eq!(
        estimation_output.hawkes_excitation_state.y_plus_minus,
        0.1 * decay_factor
    );
    assert_eq!(
        estimation_output.hawkes_excitation_state.y_minus_plus,
        0.1 * decay_factor * decay_factor
    );
    assert_eq!(
        estimation_output.hawkes_excitation_state.y_minus_minus,
        0.25 * decay_factor
    );

    let stationary_level = long_variance_parameters
        .stationary_mean_reversion_level_for_hawkes_kernel(
            kernel_parameters,
            janiform_model_kernels::hawkes::HawkesStationaryTargetSourceRates::zero(),
        )
        .expect("stationary level should build");
    let expected_long_variance_state = LongVarianceState::unit()
        .apply_event_jump(long_variance_parameters, HawkesEventSide::Buy)
        .and_then(|state| {
            state.apply_mean_reversion_drift(long_variance_parameters, stationary_level, 1.0)
        })
        .and_then(|state| state.apply_event_jump(long_variance_parameters, HawkesEventSide::Sell))
        .and_then(|state| {
            state.apply_mean_reversion_drift(long_variance_parameters, stationary_level, 1.0)
        })
        .expect("expected long-variance state should evolve");

    assert_eq!(
        estimation_output.long_variance_state.level,
        expected_long_variance_state.level
    );
    assert_eq!(
        estimation_output
            .mean_reversion_boundary_state
            .observed_log_price,
        0.05
    );
    assert_eq!(
        estimation_output.market_pressure_state.state_weights.len(),
        2
    );
    assert_ne!(
        estimation_output.market_pressure_state.state_weights[0],
        estimation_output.market_pressure_state.state_weights[1]
    );
    assert_ne!(
        estimation_output.mean_reversion_boundary_state.deviation,
        0.0
    );
    estimation_output
        .mean_reversion_boundary_state
        .validate()
        .expect("replayed mean-reversion boundary state should validate");
}

#[test]
fn current_state_estimation_rejects_observation_after_requested_state_time() {
    let kernel_parameters = HawkesKernelParameters::try_new(
        0.8,
        1.6,
        HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.1,
            minus_plus: 0.1,
            minus_minus: 0.25,
        },
        HawkesKernelLoadings {
            plus_plus: 0.08,
            plus_minus: 0.04,
            minus_plus: 0.05,
            minus_minus: 0.1,
        },
    )
    .expect("test kernel parameters should be valid");

    let mean_reversion_fitted_parameters = MeanReversionFittedParameters::try_new(0.25)
        .expect("test mean-reversion fitted parameters should be valid");

    let error = estimate_current_state_from_observed_data(CurrentStateEstimationInput {
        kernel_parameters,
        long_variance_parameters: LongVarianceParameters::neutral(),
        mean_reversion_fitted_parameters,
        market_pressure_parameters: test_market_pressure_parameters(),
        reference_price: 100.0,
        state_time_utc_nanos: 14 * NANOS_PER_HOUR,
        observed_log_return: 0.05,
        observation_timestamp_utc_nanos: vec![15 * NANOS_PER_HOUR],
        observation_trade_prices: vec![100.0],
        observation_trade_sides: vec![HawkesEventSide::Buy],
    })
    .expect_err("observation after state time must fail");

    assert_eq!(
        error,
        CurrentStateEstimationError::ObservationAfterStateTime {
            state_time_utc_nanos: 14 * NANOS_PER_HOUR,
            observation_index: 0,
            timestamp_utc_nanos: 15 * NANOS_PER_HOUR,
        }
    );
}

#[test]
fn current_state_estimation_uses_reference_price_when_observed_price_override_differs_from_last_trade()
 {
    let kernel_parameters = HawkesKernelParameters::try_new(
        0.8,
        1.6,
        HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.1,
            minus_plus: 0.1,
            minus_minus: 0.25,
        },
        HawkesKernelLoadings {
            plus_plus: 0.08,
            plus_minus: 0.04,
            minus_plus: 0.05,
            minus_minus: 0.1,
        },
    )
    .expect("test kernel parameters should be valid");

    let mean_reversion_fitted_parameters = MeanReversionFittedParameters::try_new(0.25)
        .expect("test mean-reversion fitted parameters should be valid");
    let market_pressure_parameters = test_market_pressure_parameters();

    let long_variance_parameters = LongVarianceParameters::neutral();
    let state_time_utc_nanos = 15 * NANOS_PER_HOUR;
    let reference_price: f64 = 100.0;
    let observed_price_override: f64 = 102.0;
    let observed_log_return = observed_price_override.ln() - reference_price.ln();
    let observation_timestamp_utc_nanos = vec![13 * NANOS_PER_HOUR, 14 * NANOS_PER_HOUR];
    let observation_trade_prices = vec![100.0, 101.0];
    let observation_trade_sides = vec![HawkesEventSide::Buy, HawkesEventSide::Sell];

    let estimation_output =
        estimate_current_state_from_observed_data(CurrentStateEstimationInput {
            kernel_parameters,
            long_variance_parameters,
            mean_reversion_fitted_parameters,
            market_pressure_parameters: market_pressure_parameters.clone(),
            reference_price,
            state_time_utc_nanos,
            observed_log_return,
            observation_timestamp_utc_nanos: observation_timestamp_utc_nanos.clone(),
            observation_trade_prices: observation_trade_prices.clone(),
            observation_trade_sides: observation_trade_sides.clone(),
        })
        .expect("current-state estimation should succeed");

    let tracker_snapshot = update_current_state_tracker(CurrentStateTrackerUpdateInput {
        kernel_parameters,
        long_variance_parameters,
        mean_reversion_fitted_parameters,
        market_pressure_parameters,
        reference_price,
        previous_snapshot: None,
        state_time_utc_nanos,
        observed_price: Some(observed_price_override),
        observation_timestamp_utc_nanos,
        observation_trade_prices,
        observation_trade_sides,
    })
    .expect("tracker update should succeed");

    assert_eq!(estimation_output, tracker_snapshot.state);
    assert_eq!(
        estimation_output
            .mean_reversion_boundary_state
            .observed_log_price,
        observed_log_return
    );
    assert_eq!(
        estimation_output.market_pressure_state.state_weights.len(),
        2
    );
    assert_ne!(
        estimation_output.market_pressure_state.state_weights[0],
        estimation_output.market_pressure_state.state_weights[1]
    );
    assert_ne!(
        estimation_output.mean_reversion_boundary_state.deviation,
        0.0
    );
}

#[test]
fn current_state_estimation_rejects_invalid_reference_price() {
    let kernel_parameters = HawkesKernelParameters::try_new(
        0.8,
        1.6,
        HawkesKernelLoadings {
            plus_plus: 0.2,
            plus_minus: 0.1,
            minus_plus: 0.1,
            minus_minus: 0.25,
        },
        HawkesKernelLoadings {
            plus_plus: 0.08,
            plus_minus: 0.04,
            minus_plus: 0.05,
            minus_minus: 0.1,
        },
    )
    .expect("test kernel parameters should be valid");

    let mean_reversion_fitted_parameters = MeanReversionFittedParameters::try_new(0.25)
        .expect("test mean-reversion fitted parameters should be valid");

    let error = estimate_current_state_from_observed_data(CurrentStateEstimationInput {
        kernel_parameters,
        long_variance_parameters: LongVarianceParameters::neutral(),
        mean_reversion_fitted_parameters,
        market_pressure_parameters: test_market_pressure_parameters(),
        reference_price: 0.0,
        state_time_utc_nanos: 14 * NANOS_PER_HOUR,
        observed_log_return: 0.05,
        observation_timestamp_utc_nanos: vec![14 * NANOS_PER_HOUR],
        observation_trade_prices: vec![100.0],
        observation_trade_sides: vec![HawkesEventSide::Buy],
    })
    .expect_err("nonpositive reference price must fail");

    assert_eq!(
        error,
        CurrentStateEstimationError::InvalidReferencePrice { value: 0.0 }
    );
}

fn test_market_pressure_parameters() -> MarketPressureParameters {
    let measure_parameters = MarketPressureMeasureParameters::new(
        MarketPressureTransitionRates::new(vec![0.25], vec![0.25]),
        0.01,
    );

    MarketPressureParameters::try_new(vec![-0.5, 0.5], measure_parameters)
        .expect("test market-pressure parameters should validate")
}
