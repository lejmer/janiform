use janiform_engine_pricing::{
    run_vanilla_option_pricing, AnchoredVanillaOptionPricingBoundaryInput,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingExecutionInput,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest, VanillaOptionPricingRunResult,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
};
use janiform_model_kernels::news::GammaSeverityMarkConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

#[test]
fn vanilla_option_pricing_runner_executes_public_workflow_with_optional_outputs() {
    let execution_input = VanillaOptionPricingExecutionInput::try_new(
        VanillaOptionPricingRequest::try_new(
            VanillaOptionPricingMarketInputs::try_new(101.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct"),
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            AnchoredVanillaOptionPricingBoundaryInput::disabled(
                1.0,
                HawkesExcitationState {
                    y_plus_plus: 0.0,
                    y_plus_minus: 0.0,
                    y_minus_plus: 0.0,
                    y_minus_minus: 0.0,
                },
                0.0,
            )
            .expect("disabled boundary input should construct"),
            valid_model_parameters(),
        )
        .expect("pricing request should construct"),
        vec![VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
            .expect("interval factors should construct")],
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

#[test]
fn vanilla_option_pricing_runner_matches_disabled_case_for_flat_anchor_with_zero_kappa() {
    let hawkes_boundary_state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };

    let market_inputs = VanillaOptionPricingMarketInputs::try_new(101.0, 100.0, 1.0, 0.02, 0.01)
        .expect("market inputs should construct");

    let payoff = VanillaOptionPayoffConfiguration {
        payoff_type: VanillaOptionPayoffType::Call,
    };

    let interval_factors = vec![VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
        .expect("interval factors should construct")];

    let optional_output_selection =
        VanillaOptionPricingOptionalOutputSelection::new(true, true, true);

    let disabled_request = VanillaOptionPricingRequest::try_new(
        market_inputs,
        payoff,
        AnchoredVanillaOptionPricingBoundaryInput::disabled(1.0, hawkes_boundary_state, 0.0)
            .expect("disabled boundary input should construct"),
        valid_model_parameters(),
    )
    .expect("disabled pricing request should construct");

    let flat_anchor_request = VanillaOptionPricingRequest::try_new(
        market_inputs,
        payoff,
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            hawkes_boundary_state,
            MeanReversionFittedParameters::try_new(
                0.0,
                MeanReversionAnchorModel::constant(0.25)
                    .expect("constant anchor model should construct"),
            )
            .expect("flat zero-kappa fitted parameters should construct"),
            MeanReversionRuntimeIntervalAnchors::try_new(
                "pricing_flat_constant_anchor_interval_grid.v1",
                0.25,
                vec![0.25],
                vec![1.0],
            )
            .expect("flat constant runtime anchors should construct"),
            MeanReversionBoundaryState::try_new(0.0, 0.25, -0.25)
                .expect("observed boundary state should construct"),
        )
        .expect("flat-anchor boundary input should construct"),
        valid_model_parameters(),
    )
    .expect("flat-anchor pricing request should construct");

    let disabled_run_result = run_vanilla_option_pricing(
        VanillaOptionPricingExecutionInput::try_new(
            disabled_request,
            interval_factors.clone(),
            optional_output_selection,
        )
        .expect("disabled execution input should construct"),
    )
    .expect("disabled pricing runner should succeed");

    let flat_anchor_run_result = run_vanilla_option_pricing(
        VanillaOptionPricingExecutionInput::try_new(
            flat_anchor_request,
            interval_factors,
            optional_output_selection,
        )
        .expect("flat-anchor execution input should construct"),
    )
    .expect("flat-anchor pricing runner should succeed");

    assert_eq!(
        flat_anchor_run_result.response.price,
        disabled_run_result.response.price
    );

    let flat_anchor_run_diagnostics = flat_anchor_run_result
        .run_diagnostics
        .expect("flat-anchor run diagnostics should be present");

    let anchored_recursion = flat_anchor_run_diagnostics
        .anchored_recursion
        .expect("anchored recursion diagnostics should be present");

    assert_eq!(anchored_recursion.interval_diagnostics.len(), 1);
    assert!(anchored_recursion.interval_diagnostics[0]
        .local_tilt_solve_diagnostics
        .is_empty());
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
