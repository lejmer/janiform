use crate::grids::{
    SpectralThetaWarmStartKey, SpectralThetaWarmStartState,
    VanillaOptionGridBatchPricingExecutionInput, VanillaOptionGridBatchPricingRequest,
    VanillaOptionGridRequest, run_vanilla_option_grid_batch_pricing,
};
use crate::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingKernelParameters, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionFittedParameters,
};

use super::support::{
    build_uniform_runtime_interval_schedule, disabled_boundary_input, valid_model_parameters,
};

fn nontrivial_hawkes_state() -> HawkesExcitationState {
    HawkesExcitationState {
        y_plus_plus: 0.35,
        y_plus_minus: 0.15,
        y_minus_plus: 0.25,
        y_minus_minus: 0.4,
    }
}

fn nontrivial_interval_factors() -> VanillaOptionPricingIntervalFactors {
    VanillaOptionPricingIntervalFactors::try_new(1.2, 0.3, 0.15)
        .expect("non-trivial interval factors should construct")
}

#[test]
fn theta_warm_start_state_records_terminal_theta_hint() {
    let warm_start_key = SpectralThetaWarmStartKey::new(0.0_f64.to_bits(), 0.0_f64.to_bits(), 7);

    let mut warm_start_state = SpectralThetaWarmStartState::new();
    assert_eq!(warm_start_state.theta_hint(warm_start_key), None);

    warm_start_state.record_theta_solution(warm_start_key, 0.7, -0.2, 0.3);

    assert_eq!(warm_start_state.theta_hint(warm_start_key), Some(0.7));
}

#[test]
fn grid_batch_pricing_reports_exact_node_price_activity_premium_sensitivity() {
    let mut model_parameters = valid_model_parameters();

    let execution_input = single_node_grid_execution_input(model_parameters.clone(), true);

    let pricing_result =
        run_vanilla_option_grid_batch_pricing(execution_input).expect("pricing run should succeed");

    let node_sensitivity = pricing_result.grids[0]
        .node_price_sensitivity_grid
        .as_ref()
        .and_then(|grid| grid.first())
        .and_then(|row| row.first())
        .and_then(|node| *node)
        .expect("node price sensitivity should be present");

    let log_premium_step = 1.0e-5_f64;

    model_parameters.pricing_kernel_parameters =
        VanillaOptionPricingKernelParameters::try_new(log_premium_step.exp(), 1.0)
            .expect("positive buy activity premium perturbation should stay valid");

    let positive_price = single_node_price(model_parameters.clone());

    model_parameters.pricing_kernel_parameters =
        VanillaOptionPricingKernelParameters::try_new((-log_premium_step).exp(), 1.0)
            .expect("negative buy activity premium perturbation should stay valid");

    let negative_price = single_node_price(model_parameters);

    let finite_difference = (positive_price - negative_price) / (2.0 * log_premium_step);
    assert!(
        (node_sensitivity.buy_activity_premium_derivative - finite_difference).abs() <= 1.0e-4,
        "analytic buy activity premium sensitivity {} should match finite difference {}",
        node_sensitivity.buy_activity_premium_derivative,
        finite_difference,
    );
}

fn single_node_grid_execution_input(
    model_parameters: crate::VanillaOptionPricingModelParameters,
    include_node_price_sensitivities: bool,
) -> VanillaOptionGridBatchPricingExecutionInput {
    let runtime_interval_schedule: PricingIntervalSchedule =
        build_uniform_runtime_interval_schedule(1, 1.0);

    VanillaOptionGridBatchPricingExecutionInput {
        request: VanillaOptionGridBatchPricingRequest {
            spot: 101.0,
            grids: vec![VanillaOptionGridRequest {
                payoff_type: VanillaOptionPayoffType::Call,
                risk_free_rate: 0.0,
                dividend_yield: 0.0,
                strikes: vec![100.0],
                maturity_intervals: vec![1.0],
            }],
        },
        boundary_input: disabled_boundary_input(1.0, nontrivial_hawkes_state()),
        model_parameters,
        runtime_interval_schedule,
        interval_delta: 1.0,
        interval_factors: vec![nontrivial_interval_factors()],
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection {
            include_transform_diagnostics: false,
            include_integration_quality_diagnostics: false,
            include_calibration_hooks: false,
            include_node_price_sensitivities,
        },
        spectral_collocation_node_count_override: None,
        cpu_count: Some(1),
    }
}

fn single_node_price(model_parameters: crate::VanillaOptionPricingModelParameters) -> f64 {
    let pricing_result = run_vanilla_option_grid_batch_pricing(single_node_grid_execution_input(
        model_parameters,
        false,
    ))
    .expect("finite-difference pricing run should succeed");

    pricing_result.grids[0].price_grid[0][0].expect("single-node price should exist")
}

#[test]
fn single_option_pricing_execution_input_validates_single_maturity_interval() {
    let pricing_request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(100.0, 105.0, 1.0, 0.0, 0.0)
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
        1.0,
    )
    .expect("pricing request should construct");

    assert_eq!(
        pricing_request
            .market_inputs
            .single_maturity_interval()
            .expect("single maturity should resolve"),
        1.0
    );
}
