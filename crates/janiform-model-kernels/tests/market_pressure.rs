use janiform_model_kernels::market_pressure::{
    MarketPressureIntervalTransitionInput, MarketPressureParameters, MarketPressureState,
    MarketPressureStateValidationError, MarketPressureTransitionRates, buy_probability_from_side_imbalance,
    condition_market_pressure_state_on_event_side, market_pressure_interval_transition_moments,
    signed_balance_exposure_scale_from_side_exposures,
};

#[test]
fn market_pressure_parameters_validate_side_balance_contract() {
    let parameters = test_market_pressure_parameters(0.25);

    assert_eq!(parameters.side_imbalance_levels, vec![-0.5, 0.0, 0.5]);
    assert_eq!(
        parameters.anchor_drift_per_signed_balance,
        0.25
    );
    assert!(
        MarketPressureParameters::try_new(
            Vec::new(),
            MarketPressureTransitionRates::single_state(),
            0.0
        )
        .is_err()
    );
    assert!(
        MarketPressureParameters::try_new(
            vec![0.2, 0.2],
            MarketPressureTransitionRates::new(vec![0.25], vec![0.25]),
            0.0
        )
        .is_err()
    );
    assert!(
        MarketPressureParameters::try_new(
            vec![-1.2],
            MarketPressureTransitionRates::single_state(),
            0.0
        )
        .is_err()
    );
}

#[test]
fn market_pressure_state_neutral_for_parameters_has_zero_expected_balance() {
    let parameters = test_market_pressure_parameters(0.25);

    let state = MarketPressureState::neutral_for_parameters(&parameters)
        .expect("neutral state should validate");

    assert_eq!(
        state,
        MarketPressureState::from_state_position(3, 1).expect("zero support state should validate")
    );
    assert_eq!(
        state
            .expected_side_imbalance(&parameters)
            .expect("expected side imbalance should resolve"),
        0.0
    );
}

#[test]
fn event_side_conditioning_updates_state_weights_without_changing_support() {
    let parameters = test_market_pressure_parameters(0.0);
    let state =
        MarketPressureState::try_new(vec![1.0, 1.0, 1.0]).expect("state weights should validate");

    let buy_conditioned = condition_market_pressure_state_on_event_side(&parameters, &state, true)
        .expect("buy conditioning should succeed");
    let sell_conditioned =
        condition_market_pressure_state_on_event_side(&parameters, &state, false)
            .expect("sell conditioning should succeed");

    assert_weight_vector_close(&buy_conditioned.state_weights, &[1.0 / 6.0, 1.0 / 3.0, 0.5]);
    assert_weight_vector_close(
        &sell_conditioned.state_weights,
        &[0.5, 1.0 / 3.0, 1.0 / 6.0],
    );
}

#[test]
fn market_pressure_state_rejects_parameter_support_mismatch() {
    let parameters = test_market_pressure_parameters(0.0);
    let state =
        MarketPressureState::try_new(vec![0.5, 0.5]).expect("state weights should validate");

    let error = state
        .validate_for_parameters(&parameters)
        .expect_err("state length must match side-imbalance support");

    assert_eq!(
        error,
        MarketPressureStateValidationError::StateCountMismatch {
            expected_state_count: 3,
            actual_state_count: 2
        }
    );
}

#[test]
fn interval_transition_moments_project_signed_balance_and_anchor_increment() {
    let parameters = test_market_pressure_parameters(2.0);
    let initial_state =
        MarketPressureState::from_state_position(3, 2).expect("state should validate");

    let moments =
        market_pressure_interval_transition_moments(MarketPressureIntervalTransitionInput {
            parameters: &parameters,
            initial_state: &initial_state,
            interval_delta: 0.5,
            signed_balance_exposure_scale: 4.0,
        })
        .expect("moments should resolve");

    assert_eq!(moments.terminal_state_mean.state_weights.len(), 3);
    assert!(moments.integrated_side_imbalance_mean > 0.0);
    assert_eq!(
        moments.signed_balance_exposure_mean,
        moments.integrated_side_imbalance_mean * 4.0
    );
    assert_eq!(
        moments.anchor_increment_mean,
        moments.signed_balance_exposure_mean * 2.0
    );
}

#[test]
fn side_balance_helpers_validate_probability_and_exposure_domains() {
    assert_eq!(
        buy_probability_from_side_imbalance(-1.0).expect("valid"),
        0.0
    );
    assert_eq!(
        buy_probability_from_side_imbalance(1.0).expect("valid"),
        1.0
    );
    assert!(buy_probability_from_side_imbalance(1.5).is_err());

    assert_eq!(
        signed_balance_exposure_scale_from_side_exposures(2.0, 6.0).expect("valid"),
        8.0
    );
    assert!(signed_balance_exposure_scale_from_side_exposures(-1.0, 1.0).is_err());
}

fn test_market_pressure_parameters(anchor_loading: f64) -> MarketPressureParameters {
    MarketPressureParameters::try_new(
        vec![-0.5, 0.0, 0.5],
        MarketPressureTransitionRates::new(vec![0.25, 0.5], vec![0.75, 1.0]),
        anchor_loading,
    )
        .expect("market-pressure parameters should validate")
}

fn assert_weight_vector_close(observed: &[f64], expected: &[f64]) {
    assert_eq!(observed.len(), expected.len());

    for (observed_value, expected_value) in observed.iter().zip(expected.iter()) {
        assert!(
            (*observed_value - *expected_value).abs() <= f64::EPSILON,
            "observed {observed_value} expected {expected_value}"
        );
    }
}
