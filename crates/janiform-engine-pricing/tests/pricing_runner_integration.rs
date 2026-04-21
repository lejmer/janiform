use janiform_engine_pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
    PricingIntervalScheduleMetadata, VanillaOptionGridBatchPricingExecutionInput,
    VanillaOptionGridBatchPricingRequest, VanillaOptionGridRequest,
    VanillaOptionImpliedVolatilityInversionStatus, VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType, VanillaOptionPricingExecutionInput,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest, VanillaOptionPricingRunResult,
    VanillaOptionSurfacePricingExecutionInput, VanillaOptionSurfacePricingRequest,
    run_vanilla_option_grid_batch_pricing, run_vanilla_option_pricing,
    run_vanilla_option_surface_pricing,
};
use janiform_foundations::time::ONE_HOUR_NANOS;
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
fn vanilla_option_grid_batch_pricing_matches_single_option_runner_on_shared_grids() {
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
            cpu_count: Some(1),
        })
        .expect("grid batch pricing should succeed");

    let single_specs = [
        (95.0, 1.0, VanillaOptionPayoffType::Call, 0.01, 0.0),
        (105.0, 1.0, VanillaOptionPayoffType::Put, 0.01, 0.0),
    ];

    for (node_index, (strike, maturity_interval, payoff_type, risk_free_rate, dividend_yield)) in
        single_specs.iter().copied().enumerate()
    {
        let interval_factors = if maturity_interval == 1.0 {
            vec![
                VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                    .expect("interval factors should construct"),
            ]
        } else {
            vec![
                VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                    .expect("interval factors should construct"),
                VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                    .expect("interval factors should construct"),
            ]
        };

        let single_result = run_vanilla_option_pricing(
            VanillaOptionPricingExecutionInput::try_new(
                VanillaOptionPricingRequest::try_new(
                    VanillaOptionPricingMarketInputs::try_new_single_node(
                        101.0,
                        strike,
                        maturity_interval,
                        risk_free_rate,
                        dividend_yield,
                    )
                    .expect("market inputs should construct"),
                    VanillaOptionPayoffConfiguration { payoff_type },
                    boundary_input.clone(),
                    model_parameters.clone(),
                    1.0,
                )
                .expect("pricing request should construct"),
                build_uniform_runtime_interval_schedule(interval_factors.len(), 1.0),
                interval_factors,
                optional_output_selection,
            )
            .expect("execution input should construct"),
        )
        .expect("single pricing should succeed");

        assert_eq!(
            grid_batch_result.grids[node_index].price_grid[0][0],
            single_result.response.price
        );
        assert_eq!(
            grid_batch_result.grids[node_index].implied_volatility_grid[0][0],
            single_result.response.implied_volatility
        );
        assert_eq!(
            grid_batch_result.grids[node_index].implied_volatility_inversion_status_grid[0][0],
            single_result.response.implied_volatility_inversion_status
        );
        assert_eq!(
            grid_batch_result.grids[node_index].forward_price_grid[0][0],
            Some(
                single_result
                    .response
                    .diagnostics
                    .as_ref()
                    .expect("single pricing should return diagnostics")
                    .forward_price
            )
        );
        assert_eq!(
            grid_batch_result.grids[node_index].discount_factor_grid[0][0],
            Some(
                single_result
                    .response
                    .diagnostics
                    .as_ref()
                    .expect("single pricing should return diagnostics")
                    .discount_factor
            )
        );
        assert_eq!(
            grid_batch_result.grids[node_index].boundary_state_norm_grid[0][0],
            Some(
                single_result
                    .response
                    .diagnostics
                    .as_ref()
                    .expect("single pricing should return diagnostics")
                    .boundary_state_norm
            )
        );
    }
}

#[test]
fn vanilla_option_grid_batch_pricing_matches_single_option_runner_on_rectangular_multi_maturity_grids()
 {
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
            cpu_count: Some(1),
        })
        .expect("grid batch pricing should succeed");

    for (grid_index, payoff_type) in [VanillaOptionPayoffType::Call, VanillaOptionPayoffType::Put]
        .iter()
        .copied()
        .enumerate()
    {
        for (maturity_index, maturity_interval) in [1.0, 2.0].iter().copied().enumerate() {
            let interval_factors = if maturity_interval == 1.0 {
                vec![
                    VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                        .expect("interval factors should construct"),
                ]
            } else {
                vec![
                    VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                        .expect("interval factors should construct"),
                    VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                        .expect("interval factors should construct"),
                ]
            };

            let strike_index = maturity_index;
            let strike = [95.0, 105.0][strike_index];

            let single_result = run_vanilla_option_pricing(
                VanillaOptionPricingExecutionInput::try_new(
                    VanillaOptionPricingRequest::try_new(
                        VanillaOptionPricingMarketInputs::try_new_single_node(
                            101.0,
                            strike,
                            maturity_interval,
                            0.01,
                            0.0,
                        )
                        .expect("market inputs should construct"),
                        VanillaOptionPayoffConfiguration { payoff_type },
                        boundary_input.clone(),
                        model_parameters.clone(),
                        1.0,
                    )
                    .expect("pricing request should construct"),
                    build_uniform_runtime_interval_schedule(interval_factors.len(), 1.0),
                    interval_factors.clone(),
                    optional_output_selection,
                )
                .expect("execution input should construct"),
            )
            .expect("single pricing should succeed");

            assert_eq!(
                grid_batch_result.grids[grid_index].price_grid[maturity_index][strike_index],
                single_result.response.price
            );
            assert_eq!(
                grid_batch_result.grids[grid_index].implied_volatility_grid[maturity_index]
                    [strike_index],
                single_result.response.implied_volatility
            );

            let other_strike_index = 1 - strike_index;
            assert!(
                grid_batch_result.grids[grid_index].price_grid[maturity_index][other_strike_index]
                    .is_some(),
                "rectangular batch pricing should populate every grid cell",
            );
            assert!(
                grid_batch_result.grids[grid_index].implied_volatility_grid[maturity_index]
                    [other_strike_index]
                    .is_some(),
                "rectangular batch pricing should populate every grid cell",
            );
        }
    }
}

#[test]
fn vanilla_option_surface_pricing_matches_single_option_runner_on_shared_grid() {
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

    for (maturity_index, maturity_interval) in [1.0].iter().copied().enumerate() {
        for (strike_index, strike) in [95.0, 105.0].iter().copied().enumerate() {
            let interval_factors = if maturity_interval == 1.0 {
                vec![
                    VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                        .expect("interval factors should construct"),
                ]
            } else {
                vec![
                    VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                        .expect("interval factors should construct"),
                    VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                        .expect("interval factors should construct"),
                ]
            };

            let single_result = run_vanilla_option_pricing(
                VanillaOptionPricingExecutionInput::try_new(
                    VanillaOptionPricingRequest::try_new(
                        VanillaOptionPricingMarketInputs::try_new_single_node(
                            101.0,
                            strike,
                            maturity_interval,
                            0.01,
                            0.0,
                        )
                        .expect("market inputs should construct"),
                        VanillaOptionPayoffConfiguration {
                            payoff_type: VanillaOptionPayoffType::Call,
                        },
                        boundary_input.clone(),
                        model_parameters.clone(),
                        1.0,
                    )
                    .expect("pricing request should construct"),
                    build_uniform_runtime_interval_schedule(interval_factors.len(), 1.0),
                    interval_factors,
                    optional_output_selection,
                )
                .expect("execution input should construct"),
            )
            .expect("single pricing should succeed");

            assert_eq!(
                surface_result.response.price_grid[maturity_index][strike_index],
                single_result.response.price
            );
            assert_eq!(
                surface_result.response.implied_volatility_grid[maturity_index][strike_index],
                single_result.response.implied_volatility
            );
            assert_eq!(
                surface_result
                    .response
                    .implied_volatility_inversion_status_grid[maturity_index][strike_index],
                single_result.response.implied_volatility_inversion_status
            );
            assert_eq!(
                surface_result.response.forward_price_grid[maturity_index][strike_index],
                Some(
                    single_result
                        .response
                        .diagnostics
                        .as_ref()
                        .expect("single pricing should return diagnostics")
                        .forward_price
                )
            );
            assert_eq!(
                surface_result.response.discount_factor_grid[maturity_index][strike_index],
                Some(
                    single_result
                        .response
                        .diagnostics
                        .as_ref()
                        .expect("single pricing should return diagnostics")
                        .discount_factor
                )
            );
            assert_eq!(
                surface_result.response.boundary_state_norm_grid[maturity_index][strike_index],
                Some(
                    single_result
                        .response
                        .diagnostics
                        .as_ref()
                        .expect("single pricing should return diagnostics")
                        .boundary_state_norm
                )
            );
        }
    }
}

fn build_uniform_runtime_interval_schedule(
    interval_count: usize,
    interval_delta: f64,
) -> PricingIntervalSchedule {
    let interval_duration_nanos_float = interval_delta * ONE_HOUR_NANOS as f64;
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
    )
    .expect("model parameters should construct")
}
