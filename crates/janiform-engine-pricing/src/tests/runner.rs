use crate::characteristic::build_pricing_equation_specification;
use crate::grids::{
    SpectralThetaWarmStartDriverConfiguration, SpectralThetaWarmStartKey,
    SpectralThetaWarmStartRowState, SpectralThetaWarmStartSensitivity, SpectralThetaWarmStartState,
    VanillaOptionGridBatchPricingExecutionInput,
    VanillaOptionGridBatchPricingOperatorApproximation, VanillaOptionGridBatchPricingRequest,
    VanillaOptionGridRequest, run_vanilla_option_grid_batch_pricing,
    spectral_theta_warm_start_payoff_type_mask,
};
use crate::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
};
use janiform_affine_oracle::endogenous_anchor::{
    EndogenousAnchorIntervalFactors, derive_expected_interval_projection_from_first_moment_dynamics,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use crate::runner::{
    IntervalTiltCurve, LocalTransitionRequestContext,
    build_pricing_sufficient_statistics_neutral_anchor_materialization,
    evaluate_core_pure_imaginary_log_moment_exponent_for_tilt,
    evaluate_pricing_log_return_characteristic_function, prepare_transition_row_kernel_with_curve,
    recover_pure_imaginary_log_moment_exponent, resolve_local_transition_theta,
    resolve_local_transition_theta_for_row_with_curve,
    resolve_local_transition_theta_for_row_with_parallelism,
    tilted_core_expected_increment_from_statistics,
};
use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformGuardFailureCode, ExponentialAffineTransformIntegrationStatus,
};
use janiform_affine_oracle::sufficient_statistics::SufficientStatisticsExpectedIntervalStatistics;

use super::support::{
    build_uniform_runtime_interval_schedule, disabled_boundary_input, valid_model_parameters,
};

fn transition_context_for_request(
    pricing_request: &VanillaOptionPricingRequest,
) -> LocalTransitionRequestContext {
    LocalTransitionRequestContext::from_pricing_request(
        pricing_request,
        VanillaOptionPricingIntervalFactors::try_new(0.0, 0.0, 0.0, 1.0)
            .expect("transition-context interval factors should construct"),
    )
    .expect("transition context should construct")
}

#[test]
fn theta_warm_start_state_predicts_interval_hint_from_parameter_delta() {
    let call_payoff_type_mask =
        spectral_theta_warm_start_payoff_type_mask(&[VanillaOptionPayoffType::Call]);
    let warm_start_key =
        SpectralThetaWarmStartKey::new(call_payoff_type_mask, 0.0_f64.to_bits(), 0.0_f64.to_bits());

    let previous_driver_configuration = SpectralThetaWarmStartDriverConfiguration {
        alpha: 2.0,
        beta_0: -0.1,
        delta: 0.4,
        mu: 0.05,
        delta_beta: -0.2,
    };
    let current_driver_configuration = SpectralThetaWarmStartDriverConfiguration {
        alpha: 2.3,
        beta_0: -0.2,
        delta: 0.45,
        mu: 0.04,
        delta_beta: -0.25,
    };

    let mut warm_start_state = SpectralThetaWarmStartState::new();

    warm_start_state.record_interval_theta_states(
        warm_start_key,
        0,
        previous_driver_configuration,
        vec![
            SpectralThetaWarmStartRowState::from_theta_hint(0.7).with_local_theta_sensitivity(
                SpectralThetaWarmStartSensitivity {
                    alpha_derivative: 2.0,
                    beta_0_derivative: -3.0,
                    delta_derivative: 5.0,
                    mu_derivative: 7.0,
                    delta_beta_derivative: 11.0,
                },
            ),
            SpectralThetaWarmStartRowState::from_theta_hint(0.9),
        ],
    );

    let predicted_hints =
        warm_start_state.interval_theta_hints(warm_start_key, 0, 2, current_driver_configuration);

    let expected_first_hint = 2.0_f64.mul_add(
        current_driver_configuration.alpha - previous_driver_configuration.alpha,
        (-3.0_f64).mul_add(
            current_driver_configuration.beta_0 - previous_driver_configuration.beta_0,
            5.0_f64.mul_add(
                current_driver_configuration.delta - previous_driver_configuration.delta,
                7.0_f64.mul_add(
                    current_driver_configuration.mu - previous_driver_configuration.mu,
                    11.0 * (current_driver_configuration.delta_beta
                        - previous_driver_configuration.delta_beta),
                ),
            ),
        ),
    ) + 0.7;

    assert_eq!(predicted_hints, vec![Some(expected_first_hint), Some(0.9)]);
}

#[test]
fn nested_transition_measure_detection_accepts_disabled_multi_interval_boundary() {
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

    let transition_context = transition_context_for_request(&pricing_request);
    let transition_row = transition_context
        .build_row_state(
            pricing_request
                .boundary_input
                .observed_boundary_state
                .deviation,
        )
        .expect("transition row should construct");

    assert!(transition_row.uses_nested_transition_measure());
}

#[test]
fn nested_transition_measure_detection_rejects_nonzero_local_martingale_deviation_adjustment() {
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
        1.0,
    )
    .expect("pricing request should construct");

    let transition_context = transition_context_for_request(&pricing_request);
    let transition_row = transition_context
        .build_row_state(
            pricing_request
                .boundary_input
                .observed_boundary_state
                .deviation,
        )
        .expect("transition row should construct");

    assert!(!transition_row.uses_nested_transition_measure());
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
fn grid_batch_pricing_reports_exact_node_price_alpha_sensitivity() {
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

    let configuration = model_parameters.nig_shared_plus_shift_configuration;
    let alpha_step = 1.0e-6 * configuration.alpha.max(1.0);

    model_parameters.nig_shared_plus_shift_configuration =
        NigSharedPlusShiftConfiguration::try_new(
            configuration.alpha + alpha_step,
            configuration.beta_0,
            configuration.delta,
            configuration.mu,
            configuration.delta_beta,
        )
        .expect("positive alpha perturbation should stay valid");

    let positive_price = single_node_price(model_parameters.clone());

    model_parameters.nig_shared_plus_shift_configuration =
        NigSharedPlusShiftConfiguration::try_new(
            configuration.alpha - alpha_step,
            configuration.beta_0,
            configuration.delta,
            configuration.mu,
            configuration.delta_beta,
        )
        .expect("negative alpha perturbation should stay valid");

    let negative_price = single_node_price(model_parameters);

    let finite_difference = (positive_price - negative_price) / (2.0 * alpha_step);
    assert!(
        (node_sensitivity.alpha_derivative - finite_difference).abs() <= 1.0e-4,
        "analytic alpha sensitivity {} should match finite difference {}",
        node_sensitivity.alpha_derivative,
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
                risk_free_rate: 0.02,
                dividend_yield: 0.01,
                strikes: vec![100.0],
                maturity_intervals: vec![1.0],
            }],
        },
        boundary_input: disabled_boundary_input(
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
        ),
        model_parameters,
        runtime_interval_schedule,
        interval_delta: 1.0,
        interval_factors: vec![
            VanillaOptionPricingIntervalFactors::try_new(0.0, 0.0, 0.0, 1.0)
                .expect("single-node interval factors should construct"),
        ],
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection {
            include_transform_diagnostics: false,
            include_integration_quality_diagnostics: false,
            include_calibration_hooks: false,
            include_node_price_sensitivities,
        },
        spectral_collocation_node_count_override: None,
        operator_approximation: VanillaOptionGridBatchPricingOperatorApproximation::Exact,
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
fn grid_batch_bucketed_search_reports_representative_kernel_reuse() {
    let pricing_result =
        run_vanilla_option_grid_batch_pricing(multi_interval_grid_execution_input(
            VanillaOptionGridBatchPricingOperatorApproximation::BucketedSearch,
            false,
        ))
        .expect("bucketed grid pricing run should succeed");

    let diagnostics = pricing_result
        .operator_approximation_diagnostics
        .expect("bucketed pricing should report approximation diagnostics");

    assert!(diagnostics.representative_kernel_count < diagnostics.exact_interval_count);
    assert_eq!(
        diagnostics.reused_interval_count,
        diagnostics.exact_interval_count - diagnostics.representative_kernel_count
    );
    assert!(!diagnostics.exact_pricing_forced_by_sensitivities);
}

#[test]
fn grid_batch_bucketed_search_uses_exact_pricing_for_sensitivities() {
    let pricing_result =
        run_vanilla_option_grid_batch_pricing(multi_interval_grid_execution_input(
            VanillaOptionGridBatchPricingOperatorApproximation::BucketedSearch,
            true,
        ))
        .expect("sensitivity grid pricing run should succeed");

    let diagnostics = pricing_result
        .operator_approximation_diagnostics
        .expect("forced exact pricing should report approximation diagnostics");

    assert_eq!(
        diagnostics.representative_kernel_count,
        diagnostics.exact_interval_count
    );
    assert_eq!(diagnostics.reused_interval_count, 0);
    assert!(diagnostics.exact_pricing_forced_by_sensitivities);
}

#[test]
fn grid_batch_exact_pricing_reports_no_operator_approximation() {
    let pricing_result =
        run_vanilla_option_grid_batch_pricing(multi_interval_grid_execution_input(
            VanillaOptionGridBatchPricingOperatorApproximation::Exact,
            false,
        ))
        .expect("exact grid pricing run should succeed");

    assert_eq!(pricing_result.operator_approximation_diagnostics, None);
}

fn multi_interval_grid_execution_input(
    operator_approximation: VanillaOptionGridBatchPricingOperatorApproximation,
    include_node_price_sensitivities: bool,
) -> VanillaOptionGridBatchPricingExecutionInput {
    let runtime_interval_schedule: PricingIntervalSchedule =
        build_uniform_runtime_interval_schedule(4, 0.25);

    VanillaOptionGridBatchPricingExecutionInput {
        request: VanillaOptionGridBatchPricingRequest {
            spot: 101.0,
            grids: vec![VanillaOptionGridRequest {
                payoff_type: VanillaOptionPayoffType::Call,
                risk_free_rate: 0.02,
                dividend_yield: 0.01,
                strikes: vec![100.0],
                maturity_intervals: vec![1.0],
            }],
        },
        boundary_input: disabled_boundary_input(
            0.25,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
        ),
        model_parameters: valid_model_parameters(),
        runtime_interval_schedule,
        interval_delta: 0.25,
        interval_factors: vec![
            VanillaOptionPricingIntervalFactors::try_new(0.0, 0.0, 0.0, 1.0)
                .expect("first interval factors should construct"),
            VanillaOptionPricingIntervalFactors::try_new(0.0, 0.0, 0.0, 1.0)
                .expect("second interval factors should construct"),
            VanillaOptionPricingIntervalFactors::try_new(0.0, 0.0, 0.0, 1.0)
                .expect("third interval factors should construct"),
            VanillaOptionPricingIntervalFactors::try_new(0.0, 0.0, 0.0, 1.0)
                .expect("fourth interval factors should construct"),
        ],
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection {
            include_transform_diagnostics: false,
            include_integration_quality_diagnostics: false,
            include_calibration_hooks: false,
            include_node_price_sensitivities,
        },
        spectral_collocation_node_count_override: None,
        operator_approximation,
        cpu_count: Some(1),
    }
}

#[test]
fn risk_neutral_anchor_materialization_accepts_zero_adjustment_nested_request() {
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
        VanillaOptionPricingMarketInputs::try_new_single_node(100.0, 100.0, 2.0, 0.02, 0.01)
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
        1.0,
    )
    .expect("pricing request should construct");

    let interval_factors = vec![
        VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0, 1.0)
            .expect("first interval factors should construct"),
        VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0, 1.0)
            .expect("second interval factors should construct"),
    ];

    let driver_parameters = request
        .model_parameters
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .expect("driver parameters should derive");

    let mut observer = None;

    let materialization = build_pricing_sufficient_statistics_neutral_anchor_materialization(
        &request,
        &build_uniform_runtime_interval_schedule(2, 1.0),
        interval_factors.as_slice(),
        driver_parameters,
        &mut observer,
    )
    .expect("risk-neutral anchor materialization should succeed");

    assert_eq!(
        materialization
            .runtime_interval_anchors
            .end_anchor_values
            .len(),
        2
    );
    assert_eq!(
        materialization
            .runtime_interval_anchors
            .end_pressure_states
            .len(),
        2
    );
    assert_eq!(materialization.expected_hawkes_state_path.len(), 3);
}

#[test]
fn tilted_core_expected_increment_matches_return_composition_log_moment_derivative() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(3.1, -1.2, 0.45, 0.08, -0.35)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");

    let expected_statistics =
        SufficientStatisticsExpectedIntervalStatistics::try_new(17.5, 43.25, 0.0, 0.0)
            .expect("expected statistics should construct");

    let theta = -0.35;
    let return_mark_scale = 1.7;

    let observed = tilted_core_expected_increment_from_statistics(
        driver_parameters,
        expected_statistics,
        theta,
        return_mark_scale,
    )
    .expect("tilted core expected increment should evaluate");

    let expected = return_mark_scale
        * driver_parameters
            .differentiate_return_composition_log_moment(
                expected_statistics.expected_a_plus,
                expected_statistics.expected_a_minus,
                return_mark_scale * theta,
            )
            .expect("return-composition log-moment derivative should evaluate");

    let tolerance = f64::EPSILON.sqrt() * observed.abs().max(expected.abs()).max(1.0);

    assert!(
        (observed - expected).abs() <= tolerance,
        "observed={observed} expected={expected} tolerance={tolerance}",
    );
}

#[test]
fn interval_tilt_curve_matches_direct_row_solver_across_ordered_rows() {
    let pricing_request = build_non_nested_transition_request();

    let transition_context = transition_context_for_request(&pricing_request);

    let driver_parameters = pricing_request
        .model_parameters
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .expect("driver parameters should construct");

    let resolved_rows =
        build_resolved_non_nested_transition_rows(&transition_context, driver_parameters);

    let mut interval_tilt_curve =
        IntervalTiltCurve::try_new(&transition_context, driver_parameters, None, None)
            .expect("interval tilt curve should construct");

    let nested_row = transition_context
        .build_row_state(0.0)
        .expect("nested transition row should construct");

    let nested_direct = resolve_local_transition_theta_for_row_with_parallelism(
        &transition_context,
        &nested_row,
        driver_parameters,
        None,
        None,
    )
    .expect("nested direct row solve should succeed");

    let nested_cached =
        resolve_local_transition_theta_for_row_with_curve(&mut interval_tilt_curve, &nested_row)
            .expect("nested curve-backed row solve should succeed");

    assert_eq!(nested_cached, nested_direct);

    for (transition_row, direct) in resolved_rows {
        let cached = resolve_local_transition_theta_for_row_with_curve(
            &mut interval_tilt_curve,
            &transition_row,
        )
        .expect("curve-backed row solve should succeed");

        assert_eq!(cached, direct);
    }
}

#[test]
fn prepared_transition_row_kernel_matches_direct_row_expectations() {
    let pricing_request = build_non_nested_transition_request();

    let transition_context = transition_context_for_request(&pricing_request);

    let driver_parameters = pricing_request
        .model_parameters
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .expect("driver parameters should construct");

    let (transition_row, direct_theta) =
        build_resolved_non_nested_transition_rows(&transition_context, driver_parameters)
            .into_iter()
            .next()
            .expect("at least one non-nested row should resolve");

    let direct_expectations =
        crate::runner::derive_local_tilted_interval_expectations_for_row_with_parallelism(
            &transition_context,
            &transition_row,
            driver_parameters,
            direct_theta.0,
            None,
            None,
        )
        .expect("direct expectations should succeed");

    let mut interval_tilt_curve =
        IntervalTiltCurve::try_new(&transition_context, driver_parameters, None, None)
            .expect("interval tilt curve should construct");
    let prepared_kernel =
        prepare_transition_row_kernel_with_curve(&mut interval_tilt_curve, transition_row.clone())
            .expect("prepared row kernel should succeed");

    assert_eq!(prepared_kernel.theta, direct_theta.0);
    assert_eq!(prepared_kernel.tilt_diagnostics, direct_theta.1);
    assert_eq!(prepared_kernel.local_expectations, direct_expectations);
}

#[test]
fn core_pure_imaginary_log_moment_matches_row_diagnostics() {
    let pricing_request = build_non_nested_transition_request();

    let transition_context = transition_context_for_request(&pricing_request);

    let driver_parameters = pricing_request
        .model_parameters
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .expect("driver parameters should construct");

    let (transition_row, direct_theta) =
        build_resolved_non_nested_transition_rows(&transition_context, driver_parameters)
            .into_iter()
            .next()
            .expect("at least one non-nested row should resolve");

    let theta = direct_theta.0;

    let diagnostics = direct_theta
        .1
        .expect("non-nested row should return diagnostics");

    let core_evaluation = evaluate_core_pure_imaginary_log_moment_exponent_for_tilt(
        &transition_context,
        driver_parameters,
        theta,
        None,
        None,
    )
    .expect("core pure-imaginary evaluation should succeed");

    assert_eq!(
        diagnostics.observed_log_moment_exponent,
        core_evaluation.observed_log_moment_exponent
    );
    assert_eq!(
        diagnostics.target_log_moment_exponent,
        transition_row.target_log_moment_exponent
    );
    assert_eq!(
        diagnostics.residual,
        core_evaluation.observed_log_moment_exponent - transition_row.target_log_moment_exponent
    );
}

fn build_non_nested_transition_request() -> VanillaOptionPricingRequest {
    VanillaOptionPricingRequest::try_new(
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
        1.0,
    )
    .expect("pricing request should construct")
}

fn build_resolved_non_nested_transition_rows(
    transition_context: &LocalTransitionRequestContext,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
) -> Vec<(
    crate::runner::LocalTransitionRowState,
    (f64, Option<crate::AnchoredLocalTiltSolveDiagnostics>),
)> {
    let candidate_deviations = [1.0e-8, 1.0e-7, 1.0e-6, 1.0e-5, 1.0e-4, 1.0e-3, 1.0e-2];

    let mut resolved_rows = Vec::new();

    for deviation in candidate_deviations {
        let transition_row = transition_context
            .build_row_state(deviation)
            .expect("transition row should construct");
        if transition_row.uses_nested_transition_measure() {
            continue;
        }

        let direct = resolve_local_transition_theta_for_row_with_parallelism(
            transition_context,
            &transition_row,
            driver_parameters,
            None,
            None,
        );
        if let Ok(direct) = direct {
            resolved_rows.push((transition_row, direct));
        }
    }

    assert!(
        !resolved_rows.is_empty(),
        "expected at least one resolvable non-nested transition row"
    );

    resolved_rows
}

#[test]
fn local_transition_theta_short_circuits_to_zero_for_zero_adjustment_requests() {
    let request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(100.0, 100.0, 2.0, 0.02, 0.01)
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
        1.0,
    )
    .expect("pricing request should construct");

    let interval_factors = VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0, 1.0)
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

#[test]
fn nested_zero_adjustment_materialization_matches_first_moment_hawkes_update() {
    let request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(100.0, 100.0, 1.0, 0.02, 0.01)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        disabled_boundary_input(
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.35,
                y_plus_minus: 0.15,
                y_minus_plus: 0.25,
                y_minus_minus: 0.4,
            },
        ),
        valid_model_parameters(),
        1.0,
    )
    .expect("pricing request should construct");

    let interval_factors = VanillaOptionPricingIntervalFactors::try_new(1.2, 0.3, 0.15, 1.0)
        .expect("interval factors should construct");

    let driver_parameters = request
        .model_parameters
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .expect("driver parameters should derive");

    let mut observer = None;

    let materialization = build_pricing_sufficient_statistics_neutral_anchor_materialization(
        &request,
        &build_uniform_runtime_interval_schedule(1, 1.0),
        &[interval_factors],
        driver_parameters,
        &mut observer,
    )
    .expect("risk-neutral anchor materialization should succeed");

    let expected_severity_mark = request
        .model_parameters
        .news_process_configuration
        .shared_mark_law
        .gamma_severity_marks
        .expected_severity_mark()
        .expect("expected severity mark should evaluate");

    let expected_projection = derive_expected_interval_projection_from_first_moment_dynamics(
        request.boundary_input.interval_index,
        build_pricing_equation_specification(&request),
        expected_severity_mark,
        EndogenousAnchorIntervalFactors::try_new(
            interval_factors.s_h,
            interval_factors.nu_h_known,
            interval_factors.nu_h_surprise,
            request.interval_delta,
        )
        .expect("endogenous interval factors should construct"),
        request.boundary_input.hawkes_boundary_state,
    )
    .expect("first-moment expected projection should evaluate");

    assert_eq!(
        materialization.expected_hawkes_state_path[1],
        expected_projection.structural_state_after
    );
}

#[test]
fn near_origin_positive_real_characteristic_function_evaluation_remains_finite() {
    let pricing_request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(100.0, 100.0, 1.0, 0.02, 0.01)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        disabled_boundary_input(
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.35,
                y_plus_minus: 0.15,
                y_minus_plus: 0.25,
                y_minus_minus: 0.4,
            },
        ),
        valid_model_parameters(),
        1.0,
    )
    .expect("pricing request should construct");

    let driver_parameters = pricing_request
        .model_parameters
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .expect("driver parameters should derive");

    let interval_factors = VanillaOptionPricingIntervalFactors::try_new(1.2, 0.3, 0.15, 1.0)
        .expect("interval factors should construct");

    let news_intensity = interval_factors
        .news_intensity_input()
        .expect("news intensity should construct");

    let evaluation = evaluate_pricing_log_return_characteristic_function(
        &pricing_request,
        driver_parameters,
        interval_factors,
        news_intensity,
        ComplexScalar::new(0.000_831_192_045_403_289_7, 0.0),
    )
    .expect("near-origin positive real characteristic-function evaluation should succeed");

    assert!(evaluation.value.is_finite());
    assert_eq!(
        evaluation.transform_diagnostics.status,
        ExponentialAffineTransformIntegrationStatus::Completed
    );
    assert!(
        evaluation
            .transform_diagnostics
            .output
            .expect("completed transform should include output")
            .transform_value
            .is_finite()
    );
    assert_ne!(
        evaluation
            .transform_diagnostics
            .metadata
            .guard_failure
            .map(|guard_failure| guard_failure.code),
        Some(ExponentialAffineTransformGuardFailureCode::TransformValueOverflow)
    );
}
