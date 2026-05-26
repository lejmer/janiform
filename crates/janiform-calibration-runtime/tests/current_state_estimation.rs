use janiform_calibration_runtime::{
    CurrentStateEstimationError, CurrentStateEstimationInput,
    estimate_current_state_from_observed_data,
};
use janiform_foundations::time::NANOS_PER_HOUR;
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;

#[test]
fn current_state_estimation_replays_hawkes_state_and_derives_boundary_from_observed_log_return() {
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

    let mean_reversion_fitted_parameters =
        MeanReversionFittedParameters::endogenous_core_pressure_level(0.25, 1.0, 0.0, 0.0)
            .expect("test mean-reversion fitted parameters should be valid");

    let state_time_utc_nanos = 15 * NANOS_PER_HOUR;

    let estimation_output =
        estimate_current_state_from_observed_data(CurrentStateEstimationInput {
            kernel_parameters,
            mean_reversion_fitted_parameters,
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
    assert_eq!(
        estimation_output
            .mean_reversion_boundary_state
            .observed_log_price,
        0.05
    );
    assert_eq!(
        estimation_output.mean_reversion_boundary_state.anchor_value,
        0.05
    );
    assert_eq!(
        estimation_output
            .mean_reversion_boundary_state
            .pressure_state,
        0.0
    );
    assert_eq!(
        estimation_output.mean_reversion_boundary_state.deviation,
        0.0
    );
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

    let mean_reversion_fitted_parameters =
        MeanReversionFittedParameters::endogenous_core_pressure_level(0.25, 1.0, 0.0, 0.0)
            .expect("test mean-reversion fitted parameters should be valid");

    let error = estimate_current_state_from_observed_data(CurrentStateEstimationInput {
        kernel_parameters,
        mean_reversion_fitted_parameters,
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
