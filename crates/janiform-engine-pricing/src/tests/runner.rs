use crate::{
    AnchoredVanillaOptionPricingBoundaryInput, VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType, VanillaOptionPricingExecutionInput,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    infer_expected_news_loading_under_local_measure, recover_pure_imaginary_log_moment_exponent,
    run_vanilla_option_pricing, use_nested_transition_measure_for_local_request,
};
use janiform_affine_oracle::sufficient_statistics::SufficientStatisticsExpectedIntervalStatistics;
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
};

use crate::runner::resolve_local_transition_theta;

use super::support::{disabled_boundary_input, valid_model_parameters};

#[test]
fn nested_transition_measure_detection_accepts_disabled_multi_interval_boundary() {
    let pricing_request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new(100.0, 105.0, 1.0, 0.0, 0.0)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
                .expect("zero-gain mean reversion should construct"),
            1.0,
            MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
                .expect("observed boundary state should construct"),
        )
        .expect("boundary input should construct"),
        valid_model_parameters(),
    )
    .expect("pricing request should construct");

    assert!(
        use_nested_transition_measure_for_local_request(&pricing_request)
            .expect("nested transition measure detection should succeed")
    );
}

#[test]
fn nested_transition_measure_detection_rejects_nonzero_local_martingale_deviation_adjustment() {
    let pricing_request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new(100.0, 105.0, 1.0, 0.0, 0.0)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            MeanReversionFittedParameters::try_new(
                0.5,
                MeanReversionAnchorModel::zero_gain_endogenous_core_pressure_level(),
            )
            .expect("mean-reversion fitted parameters should construct"),
            1.0,
            MeanReversionBoundaryState::try_new(0.2, 0.0, 0.0, 0.2)
                .expect("observed boundary state should construct"),
        )
        .expect("boundary input should construct"),
        valid_model_parameters(),
    )
    .expect("pricing request should construct");

    assert!(
        !use_nested_transition_measure_for_local_request(&pricing_request)
            .expect("nested transition measure detection should succeed")
    );
}

#[test]
fn pure_imaginary_log_moment_recovery_uses_characteristic_factor_modulus() {
    let characteristic_value = ComplexScalar::new(1.2312366723143512e-7, 1.2315346281055351e-7);

    let recovered =
        recover_pure_imaginary_log_moment_exponent(characteristic_value, -0.9999999999531611)
            .expect("finite contaminated pure-imaginary moment should recover");

    assert_eq!(recovered, characteristic_value.norm().ln());
}

#[test]
fn pure_imaginary_log_moment_recovery_rejects_zero_characteristic_factor() {
    let error = recover_pure_imaginary_log_moment_exponent(ComplexScalar::new(0.0, 0.0), -0.5)
        .expect_err("zero characteristic factor should fail");

    assert!(matches!(
        error,
        crate::AnchoredLocalTiltSolveError::NonFiniteTransformInput {
            theta,
            value,
        } if theta == -0.5 && value == ComplexScalar::new(0.0, 0.0)
    ));
}

#[test]
fn expected_news_loading_inference_short_circuits_when_activity_coupling_is_zero() {
    let mut model_parameters = valid_model_parameters();
    model_parameters
        .news_process_configuration
        .known_component_mark_scales
        .activity_coupling_scale = 0.0;
    model_parameters
        .news_process_configuration
        .surprise_component_mark_scales
        .activity_coupling_scale = 0.0;

    let request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 2.0, 0.0, 0.0)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
                .expect("zero-gain mean reversion should construct"),
            1.0,
            MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
                .expect("observed boundary state should construct"),
        )
        .expect("boundary input should construct"),
        model_parameters,
    )
    .expect("pricing request should construct");

    let interval_factors = VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
        .expect("interval factors should construct");
    let expected_statistics =
        SufficientStatisticsExpectedIntervalStatistics::try_new(1.0, 1.0, 0.0, 0.0)
            .expect("expected statistics should construct");

    let expected_news_loading = infer_expected_news_loading_under_local_measure(
        &request,
        interval_factors,
        expected_statistics,
    )
    .expect("zero activity coupling should not require inferred loading");

    assert_eq!(expected_news_loading, 0.0);
}

#[test]
fn multi_interval_pricing_accepts_zero_adjustment_nested_anchor_materialization() {
    let mut model_parameters = valid_model_parameters();
    model_parameters
        .news_process_configuration
        .known_component_mark_scales
        .activity_coupling_scale = 0.0;
    model_parameters
        .news_process_configuration
        .surprise_component_mark_scales
        .activity_coupling_scale = 0.0;

    let request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 2.0, 0.02, 0.01)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        disabled_boundary_input(
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
        ),
        model_parameters,
    )
    .expect("pricing request should construct");

    let execution_input = VanillaOptionPricingExecutionInput::try_new(
        request,
        vec![
            VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                .expect("first interval factors should construct"),
            VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                .expect("second interval factors should construct"),
        ],
        VanillaOptionPricingOptionalOutputSelection::default(),
    )
    .expect("execution input should construct");

    let run_result =
        run_vanilla_option_pricing(execution_input).expect("multi-interval pricing should succeed");

    assert!(run_result.response.price.is_finite());
    assert!(run_result.response.price >= 0.0);
}

#[test]
fn local_transition_theta_short_circuits_to_zero_for_zero_adjustment_requests() {
    let request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 2.0, 0.02, 0.01)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        disabled_boundary_input(
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
        ),
        valid_model_parameters(),
    )
    .expect("pricing request should construct");

    let interval_factors = VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
        .expect("interval factors should construct");

    let (theta, diagnostics) = resolve_local_transition_theta(
        &request,
        interval_factors,
        request
            .model_parameters
            .nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .expect("driver parameters should derive"),
    )
    .expect("zero-adjustment request should resolve transition theta");

    assert_eq!(theta, 0.0);
    assert_eq!(diagnostics, None);
}
