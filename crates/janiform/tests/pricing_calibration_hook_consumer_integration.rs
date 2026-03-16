mod support;

use janiform::affine_oracle::ExponentialAffineTransformNewsIntensityInput;
use janiform::calibration::build_option_surface_calibration_observation_from_pricing_hooks;
use janiform::complex_math::ComplexScalar;
use janiform::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
use janiform::news::GammaSeverityMarkConfiguration;
use janiform::nig::NigSharedPlusShiftConfiguration;
use janiform::pricing::{
    FourierInversionEngineSettings, VanillaOptionPayoffConfiguration, VanillaOptionPayoffType,
    VanillaOptionPricingDiagnostics, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest, VanillaOptionPricingResponse, build_pricing_transform_input,
    build_vanilla_option_pricing_run_result, price_vanilla_option_via_fourier_inversion,
    solve_exponential_affine_transform,
};
use support::pricing_reference::black_scholes_log_return_characteristic_function;

#[test]
fn pricing_run_output_hooks_are_directly_consumable_by_calibration_adapter() {
    let market_inputs = VanillaOptionPricingMarketInputs::try_new(101.0, 100.0, 0.5, 0.02, 0.01)
        .expect("market inputs should construct");
    let forward_price = market_inputs.spot
        * ((market_inputs.risk_free_rate - market_inputs.dividend_yield)
            * market_inputs.maturity_interval)
            .exp();
    let discount_factor = (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp();
    let response = VanillaOptionPricingResponse::try_new(
        12.5,
        VanillaOptionPricingDiagnostics::try_new(
            forward_price,
            discount_factor,
            market_inputs.maturity_interval,
            0.55,
        )
        .expect("response diagnostics should construct"),
    )
    .expect("response should construct");

    let model_parameters = VanillaOptionPricingModelParameters::try_new(
        HawkesKernelParameters::try_new(
            0.8,
            1.6,
            HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.1,
                minus_plus: 0.1,
                minus_minus: 0.25,
            },
            HawkesKernelLoadings {
                plus_plus: 0.1,
                plus_minus: 0.05,
                minus_plus: 0.05,
                minus_minus: 0.1,
            },
        )
        .expect("kernel parameters should construct"),
        GammaSeverityMarkConfiguration::try_new(1.8, 0.7)
            .expect("gamma mark configuration should construct"),
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("NIG shared-plus-shift configuration should construct"),
    )
    .expect("pricing model parameters should construct");
    let pricing_request = VanillaOptionPricingRequest::try_new(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        HawkesExcitationState {
            y_plus_plus: 0.2,
            y_plus_minus: 0.1,
            y_minus_plus: 0.3,
            y_minus_minus: 0.4,
        },
        model_parameters,
    )
    .expect("pricing request should construct");
    let transform_input = build_pricing_transform_input(
        pricing_request,
        ComplexScalar::new(0.2, -0.1),
        ComplexScalar::new(0.3, 0.05),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(0.95, 0.02),
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.03, 0.05)
            .expect("pricing-hook test news intensity should construct"),
    )
    .expect("pricing transform input should construct");
    let transform_diagnostics = solve_exponential_affine_transform(transform_input)
        .expect("transform solve should succeed");

    let integration_diagnostics = price_vanilla_option_via_fourier_inversion(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        FourierInversionEngineSettings::default(),
        // Use a Black-Scholes baseline here to validate wiring, not Janiform dynamics.
        |argument| black_scholes_log_return_characteristic_function(market_inputs, 0.2, argument),
    )
    .expect("Fourier pricing run should succeed")
    .diagnostics;
    let numeric_quality = integration_diagnostics.numeric_quality_diagnostics;

    let run_output = build_vanilla_option_pricing_run_result(
        response,
        VanillaOptionPricingOptionalOutputSelection::new(true, true, true),
        Some(market_inputs),
        Some(transform_diagnostics),
        Some(integration_diagnostics),
    )
    .expect("run output should include diagnostics and calibration hooks");

    let run_diagnostics = run_output
        .run_diagnostics
        .expect("run diagnostics should be present");
    assert!(run_diagnostics.transform.is_some());
    assert!(run_diagnostics.integration_quality.is_some());

    let calibration_hooks = run_output
        .calibration_hooks
        .expect("calibration hooks should be present");
    let calibration_observation =
        build_option_surface_calibration_observation_from_pricing_hooks(calibration_hooks)
            .expect("calibration adapter should accept emitted hooks");

    assert_eq!(
        calibration_observation.option_price,
        calibration_hooks.option_price
    );
    assert_eq!(
        calibration_observation.log_moneyness,
        calibration_hooks.log_moneyness
    );
    assert_eq!(
        calibration_observation.forward_moneyness,
        calibration_hooks.forward_moneyness
    );
    assert_eq!(
        calibration_observation.integration_numeric_quality_is_poor,
        Some(numeric_quality.numeric_quality_is_poor)
    );
    assert_eq!(
        calibration_observation.integration_convergence_check_passed,
        Some(numeric_quality.convergence_check_passed)
    );
    assert_eq!(
        calibration_observation.integration_truncation_check_passed,
        Some(numeric_quality.truncation_check_passed)
    );
}
