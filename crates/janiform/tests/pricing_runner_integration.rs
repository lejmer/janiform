use janiform::pricing::{
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingExecutionInput,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingRunResult, run_vanilla_option_pricing,
};
use janiform::{
    hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters},
    news::GammaSeverityMarkConfiguration,
    nig::NigSharedPlusShiftConfiguration,
    pricing::VanillaOptionPricingMarketInputs,
};

#[test]
fn vanilla_option_pricing_runner_executes_public_workflow_with_optional_outputs() {
    let execution_input = VanillaOptionPricingExecutionInput::try_new(
        VanillaOptionPricingRequest::try_new(
            VanillaOptionPricingMarketInputs::try_new(101.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct"),
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            valid_model_parameters(),
        )
        .expect("pricing request should construct"),
        VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
            .expect("interval factors should construct"),
        VanillaOptionPricingOptionalOutputSelection::new(true, true, true),
    )
    .expect("execution input should construct");

    let run_result: VanillaOptionPricingRunResult =
        run_vanilla_option_pricing(execution_input).expect("pricing runner should succeed");

    assert!(run_result.response.price.is_finite());
    assert!(run_result.response.price >= 0.0);
    assert!(run_result.response.diagnostics.forward_price > 0.0);
    assert!(run_result.response.diagnostics.discount_factor > 0.0);
    assert!(run_result.response.diagnostics.boundary_state_norm >= 0.0);

    let run_diagnostics = run_result
        .run_diagnostics
        .expect("run diagnostics should be present");
    assert!(run_diagnostics.transform.is_some());
    assert!(run_diagnostics.integration_quality.is_some());

    let calibration_hooks = run_result
        .calibration_hooks
        .expect("calibration hooks should be present");
    assert_eq!(calibration_hooks.option_price, run_result.response.price);
    assert_eq!(
        calibration_hooks.maturity_interval,
        run_result.response.diagnostics.maturity_interval
    );
}

fn valid_model_parameters() -> VanillaOptionPricingModelParameters {
    VanillaOptionPricingModelParameters::try_new(
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
    .expect("model parameters should construct")
}
