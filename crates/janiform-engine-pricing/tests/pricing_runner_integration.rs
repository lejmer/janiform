use janiform_engine_pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
    PricingIntervalScheduleMetadata, VanillaOptionGridBatchPricingExecutionInput,
    VanillaOptionGridBatchPricingOperatorApproximation, VanillaOptionGridBatchPricingRequest,
    VanillaOptionGridRequest, VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingExecutionInput,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingKernelParameters,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingRunResult, VanillaOptionPricingStatus,
    VanillaOptionSurfacePricingExecutionInput, VanillaOptionSurfacePricingRequest,
    run_vanilla_option_grid_batch_pricing, run_vanilla_option_pricing,
    run_vanilla_option_surface_pricing,
};
use janiform_foundations::time::NANOS_PER_HOUR;
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

#[test]
fn vanilla_option_pricing_runner_executes_public_workflow_with_optional_outputs() {
    let execution_input = VanillaOptionPricingExecutionInput::try_new(
        VanillaOptionPricingRequest::try_new(
            VanillaOptionPricingMarketInputs::try_new_single_node(101.0, 100.0, 1.0, 0.02, 0.01)
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
                    .expect("zero-gain mean reversion fit should construct"),
                1.0,
                MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
                    .expect("zero observed boundary state should construct"),
            )
            .expect("endogenous boundary input should construct"),
            valid_model_parameters(),
            1.0,
        )
        .expect("pricing request should construct"),
        build_uniform_runtime_interval_schedule(1, 1.0),
        vec![
            VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                .expect("interval factors should construct"),
        ],
        VanillaOptionPricingOptionalOutputSelection::new(true, true, true),
    )
    .expect("execution input should construct");

    let run_result: VanillaOptionPricingRunResult =
        run_vanilla_option_pricing(execution_input).expect("pricing runner should succeed");

    let price = run_result
        .response
        .price
        .expect("single pricing should return a price");
    assert!(price.is_finite());
    assert!(price >= 0.0);
    assert!(run_result.response.implied_volatility.is_some());
    assert_eq!(
        run_result.response.implied_volatility_inversion_status,
        Some(VanillaOptionImpliedVolatilityInversionStatus::Succeeded)
    );

    let diagnostics = run_result
        .response
        .diagnostics
        .as_ref()
        .expect("single pricing should return diagnostics");
    assert!(diagnostics.forward_price > 0.0);
    assert!(diagnostics.discount_factor > 0.0);
    assert!(diagnostics.boundary_state_norm >= 0.0);

    let run_diagnostics = run_result
        .run_diagnostics
        .expect("run diagnostics should be present");
    assert!(run_diagnostics.transform.is_some());
    assert!(run_diagnostics.integration_quality.is_some());

    let calibration_hooks = run_result
        .calibration_hooks
        .expect("calibration hooks should be present");
    assert_eq!(calibration_hooks.option_price, price);
    assert_eq!(
        calibration_hooks.maturity_interval,
        diagnostics.maturity_interval
    );
}

#[test]
fn vanilla_option_pricing_runner_matches_zero_anchor_and_shifted_anchor_for_zero_kappa_endogenous_fit()
 {
    let hawkes_boundary_state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };

    let market_inputs =
        VanillaOptionPricingMarketInputs::try_new_single_node(101.0, 100.0, 1.0, 0.02, 0.01)
            .expect("market inputs should construct");

    let payoff = VanillaOptionPayoffConfiguration {
        payoff_type: VanillaOptionPayoffType::Call,
    };

    let interval_factors = vec![
        VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
            .expect("interval factors should construct"),
    ];

    let optional_output_selection =
        VanillaOptionPricingOptionalOutputSelection::new(true, true, true);

    let zero_anchor_request = VanillaOptionPricingRequest::try_new(
        market_inputs.clone(),
        payoff,
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            hawkes_boundary_state,
            MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
                .expect("zero-gain mean reversion fit should construct"),
            1.0,
            MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
                .expect("zero observed boundary state should construct"),
        )
        .expect("zero-anchor boundary input should construct"),
        valid_model_parameters(),
        1.0,
    )
    .expect("zero-anchor pricing request should construct");

    let flat_anchor_request = VanillaOptionPricingRequest::try_new(
        market_inputs,
        payoff,
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            hawkes_boundary_state,
            MeanReversionFittedParameters::try_new(
                0.0,
                MeanReversionAnchorModel::zero_gain_endogenous_core_pressure_level(),
            )
            .expect("flat zero-kappa fitted parameters should construct"),
            1.0,
            MeanReversionBoundaryState::try_new(0.0, 0.25, 0.0, -0.25)
                .expect("observed boundary state should construct"),
        )
        .expect("flat-anchor boundary input should construct"),
        valid_model_parameters(),
        1.0,
    )
    .expect("flat-anchor pricing request should construct");

    let zero_anchor_run_result = run_vanilla_option_pricing(
        VanillaOptionPricingExecutionInput::try_new(
            zero_anchor_request,
            build_uniform_runtime_interval_schedule(1, 1.0),
            interval_factors.clone(),
            optional_output_selection,
        )
        .expect("zero-anchor execution input should construct"),
    )
    .expect("zero-anchor pricing runner should succeed");

    let flat_anchor_run_result = run_vanilla_option_pricing(
        VanillaOptionPricingExecutionInput::try_new(
            flat_anchor_request,
            build_uniform_runtime_interval_schedule(1, 1.0),
            interval_factors,
            optional_output_selection,
        )
        .expect("flat-anchor execution input should construct"),
    )
    .expect("flat-anchor pricing runner should succeed");

    assert_eq!(
        flat_anchor_run_result.response.price,
        zero_anchor_run_result.response.price
    );

    let flat_anchor_run_diagnostics = flat_anchor_run_result
        .run_diagnostics
        .expect("flat-anchor run diagnostics should be present");

    let spectral = flat_anchor_run_diagnostics
        .spectral
        .expect("spectral diagnostics should be present");

    assert_eq!(spectral.interval_diagnostics.len(), 1);
}

#[test]
fn vanilla_option_grid_batch_pricing_populates_shared_grids() {
    let boundary_input = AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
        MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
            .expect("zero-gain mean reversion fit should construct"),
        1.0,
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
            .expect("zero observed boundary state should construct"),
    )
    .expect("boundary input should construct");

    let model_parameters = valid_model_parameters();

    let optional_output_selection =
        VanillaOptionPricingOptionalOutputSelection::new(false, false, false);

    let grid_batch_result =
        run_vanilla_option_grid_batch_pricing(VanillaOptionGridBatchPricingExecutionInput {
            request: VanillaOptionGridBatchPricingRequest {
                spot: 101.0,
                grids: vec![
                    VanillaOptionGridRequest {
                        payoff_type: VanillaOptionPayoffType::Call,
                        risk_free_rate: 0.01,
                        dividend_yield: 0.0,
                        strikes: vec![95.0],
                        maturity_intervals: vec![1.0],
                    },
                    VanillaOptionGridRequest {
                        payoff_type: VanillaOptionPayoffType::Put,
                        risk_free_rate: 0.01,
                        dividend_yield: 0.0,
                        strikes: vec![105.0],
                        maturity_intervals: vec![1.0],
                    },
                ],
            },
            boundary_input: boundary_input.clone(),
            model_parameters: model_parameters.clone(),
            runtime_interval_schedule: build_uniform_runtime_interval_schedule(1, 1.0),
            interval_delta: 1.0,
            interval_factors: vec![
                VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                    .expect("interval factors should construct"),
            ],
            optional_output_selection,
            spectral_collocation_node_count_override: None,
            operator_approximation: VanillaOptionGridBatchPricingOperatorApproximation::Exact,
            cpu_count: Some(1),
        })
        .expect("grid batch pricing should succeed");

    for grid in grid_batch_result.grids.iter() {
        assert_successful_grid_node(
            grid.price_grid[0][0],
            grid.implied_volatility_grid[0][0],
            grid.implied_volatility_inversion_status_grid[0][0],
            grid.pricing_status_grid[0][0],
        );
        assert_successful_grid_diagnostics(
            grid.forward_price_grid[0][0],
            grid.discount_factor_grid[0][0],
            grid.boundary_state_norm_grid[0][0],
        );
    }
}

#[test]
fn vanilla_option_grid_batch_pricing_populates_rectangular_multi_maturity_grids() {
    let boundary_input = AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
        MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
            .expect("zero-gain mean reversion fit should construct"),
        1.0,
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
            .expect("zero observed boundary state should construct"),
    )
    .expect("boundary input should construct");

    let model_parameters = valid_model_parameters();

    let optional_output_selection =
        VanillaOptionPricingOptionalOutputSelection::new(false, false, false);

    let grid_batch_result =
        run_vanilla_option_grid_batch_pricing(VanillaOptionGridBatchPricingExecutionInput {
            request: VanillaOptionGridBatchPricingRequest {
                spot: 101.0,
                grids: vec![
                    VanillaOptionGridRequest {
                        payoff_type: VanillaOptionPayoffType::Call,
                        risk_free_rate: 0.01,
                        dividend_yield: 0.0,
                        strikes: vec![95.0, 105.0],
                        maturity_intervals: vec![1.0, 2.0],
                    },
                    VanillaOptionGridRequest {
                        payoff_type: VanillaOptionPayoffType::Put,
                        risk_free_rate: 0.01,
                        dividend_yield: 0.0,
                        strikes: vec![95.0, 105.0],
                        maturity_intervals: vec![1.0, 2.0],
                    },
                ],
            },
            boundary_input: boundary_input.clone(),
            model_parameters: model_parameters.clone(),
            runtime_interval_schedule: build_uniform_runtime_interval_schedule(2, 1.0),
            interval_delta: 1.0,
            interval_factors: vec![
                VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                    .expect("interval factors should construct"),
                VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                    .expect("interval factors should construct"),
            ],
            optional_output_selection,
            spectral_collocation_node_count_override: None,
            operator_approximation: VanillaOptionGridBatchPricingOperatorApproximation::Exact,
            cpu_count: Some(1),
        })
        .expect("grid batch pricing should succeed");

    for grid in grid_batch_result.grids.iter() {
        for maturity_index in 0..grid.maturity_intervals.len() {
            for strike_index in 0..grid.strikes.len() {
                assert_successful_grid_node(
                    grid.price_grid[maturity_index][strike_index],
                    grid.implied_volatility_grid[maturity_index][strike_index],
                    grid.implied_volatility_inversion_status_grid[maturity_index][strike_index],
                    grid.pricing_status_grid[maturity_index][strike_index],
                );
            }
        }
    }
}

#[test]
fn vanilla_option_grid_batch_pricing_preserves_zero_rate_put_call_parity() {
    let boundary_input = AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
        MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
            .expect("zero-gain mean reversion fit should construct"),
        24.0,
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
            .expect("zero observed boundary state should construct"),
    )
    .expect("boundary input should construct");

    let spot = 101.0;
    let strike = 100.0;
    let maturity_interval = 24.0;
    let optional_output_selection =
        VanillaOptionPricingOptionalOutputSelection::new(false, false, false);

    let grid_batch_result =
        run_vanilla_option_grid_batch_pricing(VanillaOptionGridBatchPricingExecutionInput {
            request: VanillaOptionGridBatchPricingRequest {
                spot,
                grids: vec![
                    VanillaOptionGridRequest {
                        payoff_type: VanillaOptionPayoffType::Call,
                        risk_free_rate: 0.0,
                        dividend_yield: 0.0,
                        strikes: vec![strike],
                        maturity_intervals: vec![maturity_interval],
                    },
                    VanillaOptionGridRequest {
                        payoff_type: VanillaOptionPayoffType::Put,
                        risk_free_rate: 0.0,
                        dividend_yield: 0.0,
                        strikes: vec![strike],
                        maturity_intervals: vec![maturity_interval],
                    },
                ],
            },
            boundary_input,
            model_parameters: valid_model_parameters(),
            runtime_interval_schedule: build_uniform_runtime_interval_schedule(
                1,
                maturity_interval,
            ),
            interval_delta: maturity_interval,
            interval_factors: vec![
                VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                    .expect("interval factors should construct"),
            ],
            optional_output_selection,
            spectral_collocation_node_count_override: Some(257),
            operator_approximation: VanillaOptionGridBatchPricingOperatorApproximation::Exact,
            cpu_count: Some(1),
        })
        .expect("grid batch pricing should succeed");

    let call_price =
        grid_batch_result.grids[0].price_grid[0][0].expect("call price should be present");
    let put_price =
        grid_batch_result.grids[1].price_grid[0][0].expect("put price should be present");
    let expected_forward_spread = spot - strike;

    let parity_error = (call_price - put_price - expected_forward_spread).abs();
    let single_price_resolution = spot / (257.0 * 257.0);
    let call_and_put_resolution = 2.0 * single_price_resolution;

    assert!(
        parity_error <= call_and_put_resolution,
        "zero-rate put-call parity error {parity_error} should be within spectral resolution {call_and_put_resolution}",
    );
}

#[test]
fn vanilla_option_surface_pricing_populates_shared_grid() {
    let boundary_input = AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
        MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
            .expect("zero-gain mean reversion fit should construct"),
        1.0,
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
            .expect("zero observed boundary state should construct"),
    )
    .expect("boundary input should construct");

    let model_parameters = valid_model_parameters();

    let optional_output_selection =
        VanillaOptionPricingOptionalOutputSelection::new(true, true, true);

    let surface_result =
        run_vanilla_option_surface_pricing(VanillaOptionSurfacePricingExecutionInput {
            request: VanillaOptionSurfacePricingRequest {
                market_inputs: VanillaOptionPricingMarketInputs::try_new(
                    101.0,
                    vec![95.0, 105.0],
                    vec![1.0],
                    0.01,
                    0.0,
                )
                .expect("surface market inputs should construct"),
                payoff: VanillaOptionPayoffConfiguration {
                    payoff_type: VanillaOptionPayoffType::Call,
                },
            },
            boundary_input: boundary_input.clone(),
            model_parameters: model_parameters.clone(),
            runtime_interval_schedule: build_uniform_runtime_interval_schedule(1, 1.0),
            interval_delta: 1.0,
            interval_factors: vec![
                VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                    .expect("interval factors should construct"),
            ],
            optional_output_selection,
            spectral_collocation_node_count_override: None,
            cpu_count: Some(1),
        })
        .expect("surface pricing should succeed");

    for maturity_index in 0..surface_result.response.maturity_intervals.len() {
        for strike_index in 0..surface_result.response.strikes.len() {
            assert_successful_grid_node(
                surface_result.response.price_grid[maturity_index][strike_index],
                surface_result.response.implied_volatility_grid[maturity_index][strike_index],
                surface_result
                    .response
                    .implied_volatility_inversion_status_grid[maturity_index][strike_index],
                surface_result.response.pricing_status_grid[maturity_index][strike_index],
            );
            assert_successful_grid_diagnostics(
                surface_result.response.forward_price_grid[maturity_index][strike_index],
                surface_result.response.discount_factor_grid[maturity_index][strike_index],
                surface_result.response.boundary_state_norm_grid[maturity_index][strike_index],
            );
        }
    }
}

fn assert_successful_grid_node(
    price: Option<f64>,
    implied_volatility: Option<f64>,
    implied_volatility_status: Option<VanillaOptionImpliedVolatilityInversionStatus>,
    pricing_status: VanillaOptionPricingStatus,
) {
    assert_eq!(pricing_status, VanillaOptionPricingStatus::Succeeded);
    assert_eq!(
        implied_volatility_status,
        Some(VanillaOptionImpliedVolatilityInversionStatus::Succeeded)
    );

    let price = price.expect("successful grid pricing should return a price");
    assert!(price.is_finite());
    assert!(price >= 0.0);

    let implied_volatility =
        implied_volatility.expect("successful grid inversion should return implied volatility");
    assert!(implied_volatility.is_finite());
    assert!(implied_volatility >= 0.0);
}

fn assert_successful_grid_diagnostics(
    forward_price: Option<f64>,
    discount_factor: Option<f64>,
    boundary_state_norm: Option<f64>,
) {
    let forward_price = forward_price.expect("successful grid pricing should return forward price");
    assert!(forward_price.is_finite());
    assert!(forward_price > 0.0);

    let discount_factor =
        discount_factor.expect("successful grid pricing should return discount factor");
    assert!(discount_factor.is_finite());
    assert!(discount_factor > 0.0);

    let boundary_state_norm =
        boundary_state_norm.expect("successful grid pricing should return boundary state norm");
    assert!(boundary_state_norm.is_finite());
    assert!(boundary_state_norm >= 0.0);
}

fn build_uniform_runtime_interval_schedule(
    interval_count: usize,
    interval_delta: f64,
) -> PricingIntervalSchedule {
    let interval_duration_nanos_float = interval_delta * NANOS_PER_HOUR as f64;
    let interval_duration_nanos = interval_duration_nanos_float as i64;

    assert_eq!(
        interval_duration_nanos as f64, interval_duration_nanos_float,
        "test runtime interval schedule requires an exact nanosecond interval"
    );

    PricingIntervalSchedule::try_new(
        0,
        PricingIntervalScheduleMetadata {
            policy_id: "explicit_interval_end_timestamps_utc.v1".to_string(),
            derivation_policy: "test_uniform_schedule.v1".to_string(),
            factor_assignment_policy: "overlap_weighted_resampling_from_hourly_source_buckets.v1"
                .to_string(),
        },
        (1..=interval_count)
            .map(|interval_index| interval_index as i64 * interval_duration_nanos)
            .collect(),
    )
    .expect("uniform runtime interval schedule should construct")
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
        NewsProcessConfiguration::try_new(
            NewsArrivalConfiguration {
                known: KnownNewsArrivalConfiguration {
                    scheduled_event_timestamps_utc_nanos: vec![],
                },
                surprise: SurpriseNewsArrivalConfiguration {
                    baseline_intensity: 0.75,
                },
            },
            SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration::new(1.8, 0.7),
                double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::new(
                    0.35, 2.1, 3.0,
                ),
                coupled_gap_marks: CoupledGapMarkConfiguration::new(0.6),
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.0,
                return_jump_scale: 1.0,
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.0,
                return_jump_scale: 1.0,
            },
        )
        .expect("news process configuration should construct"),
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("NIG shared-plus-shift configuration should construct"),
        janiform_model_kernels::return_clock::ReturnClockExposureParameters::neutral(),
        VanillaOptionPricingKernelParameters::identity(),
    )
    .expect("model parameters should construct")
}
