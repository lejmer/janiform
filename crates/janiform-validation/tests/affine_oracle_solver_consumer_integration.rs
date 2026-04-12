use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformInput,
    ExponentialAffineTransformIntegrationStatus, ExponentialAffineTransformIntegratorMethod,
    ExponentialAffineTransformNewsIntensityInput, ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_optional_settings,
};
use janiform_engine_pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingRequest,
    build_pricing_transform_input, solve_exponential_affine_transform,
};
use janiform_engine_risk::{
    solve_exponential_affine_transform_for_risk_sampling,
    solve_exponential_affine_transform_for_risk_sampling_with_settings,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionFittedParameters,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};

#[test]
fn solver_api_contract_is_compatible_across_pricing_and_risk_consumers() {
    let transform_input = build_transform_input(1.0);
    let solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        64,
    )
    .expect("pricing integration settings should construct");

    let pricing_result = solve_exponential_affine_transform(transform_input, solver_settings)
        .expect("pricing consumer solve should succeed");
    let risk_result = solve_exponential_affine_transform_for_risk_sampling(transform_input)
        .expect("risk consumer solve should succeed");

    assert_eq!(pricing_result, risk_result);
    assert_eq!(
        pricing_result.status,
        ExponentialAffineTransformIntegrationStatus::Completed
    );
    assert!(
        pricing_result.output.is_some(),
        "completed solver result must include transform output"
    );
}

#[test]
fn pricing_consumer_calls_shared_solver_entrypoint() {
    let transform_input = build_transform_input(1.0);
    let solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::ForwardEuler,
        32,
    )
    .expect("pricing integration settings should construct");

    let pricing_result = solve_exponential_affine_transform(transform_input, solver_settings)
        .expect("pricing solve with explicit settings should succeed");
    let shared_result = solve_exponential_affine_transform_with_optional_settings(
        transform_input,
        Some(solver_settings),
    )
    .expect("shared solver entrypoint should succeed");

    assert_eq!(pricing_result, shared_result);
    assert_eq!(pricing_result.metadata.configured_step_count, 32);
    assert_eq!(
        pricing_result.metadata.integrator_method,
        ExponentialAffineTransformIntegratorMethod::ForwardEuler
    );
}

#[test]
fn risk_consumer_calls_shared_solver_entrypoint_with_interval_parametric_delta() {
    let short_interval_input = build_transform_input(0.5);
    let long_interval_input = build_transform_input(1.5);
    let solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        64,
    )
    .expect("risk integration settings should construct");

    let short_interval_result = solve_exponential_affine_transform_for_risk_sampling_with_settings(
        short_interval_input,
        solver_settings,
    )
    .expect("risk solve for short interval should succeed");
    let long_interval_result = solve_exponential_affine_transform_for_risk_sampling_with_settings(
        long_interval_input,
        solver_settings,
    )
    .expect("risk solve for long interval should succeed");

    let short_interval_shared_result = solve_exponential_affine_transform_with_optional_settings(
        short_interval_input,
        Some(solver_settings),
    )
    .expect("shared solver short-interval call should succeed");
    let long_interval_shared_result = solve_exponential_affine_transform_with_optional_settings(
        long_interval_input,
        Some(solver_settings),
    )
    .expect("shared solver long-interval call should succeed");

    assert_eq!(short_interval_result, short_interval_shared_result);
    assert_eq!(long_interval_result, long_interval_shared_result);

    let short_interval_output = short_interval_result
        .output
        .expect("completed short-interval solve should include output");
    let long_interval_output = long_interval_result
        .output
        .expect("completed long-interval solve should include output");
    assert!(
        short_interval_output
            .transform_value
            .subtract(long_interval_output.transform_value)
            .norm()
            > 1.0e-6,
        "interval-parametric delta regression detected with near-identical transform values"
    );
}

#[test]
fn pricing_pipeline_propagates_interval_delta_to_solver_input() {
    let request_with_hourly_default =
        build_pricing_request(DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA);
    let request_with_non_hourly_delta = build_pricing_request(0.5);

    let hourly_transform_input = build_pricing_transform_input(
        &request_with_hourly_default,
        ComplexScalar::new(0.2, -0.1),
        ComplexScalar::new(0.3, 0.05),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(0.95, 0.02),
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.04, 0.04)
            .expect("hourly test news intensity should construct"),
    )
    .expect("hourly pricing transform input should build");

    let non_hourly_transform_input = build_pricing_transform_input(
        &request_with_non_hourly_delta,
        ComplexScalar::new(0.2, -0.1),
        ComplexScalar::new(0.3, 0.05),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(0.95, 0.02),
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.04, 0.04)
            .expect("non-hourly test news intensity should construct"),
    )
    .expect("non-hourly pricing transform input should build");

    assert_eq!(
        hourly_transform_input.delta,
        DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA
    );
    assert_eq!(non_hourly_transform_input.delta, 0.5);

    let hourly_result = solve_exponential_affine_transform(
        hourly_transform_input,
        ExponentialAffineTransformSolverSettings::try_new(
            ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
            64,
        )
        .expect("hourly solver settings should construct"),
    )
    .expect("hourly pricing solve should succeed");
    let non_hourly_result = solve_exponential_affine_transform(
        non_hourly_transform_input,
        ExponentialAffineTransformSolverSettings::try_new(
            ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
            64,
        )
        .expect("non-hourly solver settings should construct"),
    )
    .expect("non-hourly pricing solve should succeed");

    let hourly_output = hourly_result
        .output
        .expect("completed hourly solve should include output");
    let non_hourly_output = non_hourly_result
        .output
        .expect("completed non-hourly solve should include output");
    assert!(
        hourly_output
            .transform_value
            .subtract(non_hourly_output.transform_value)
            .norm()
            > 1.0e-6,
        "pricing pipeline interval-delta propagation regression detected with near-identical transform values"
    );
}

fn build_transform_input(delta: f64) -> ExponentialAffineTransformInput {
    ExponentialAffineTransformInput::try_new(
        ExponentialAffineTransformEquationSpecification::try_new(
            HawkesKernelParameters::try_new(
                0.8,
                1.2,
                HawkesKernelLoadings {
                    plus_plus: 0.20,
                    plus_minus: 0.08,
                    minus_plus: 0.10,
                    minus_minus: 0.24,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.03,
                    plus_minus: 0.02,
                    minus_plus: 0.01,
                    minus_minus: 0.04,
                },
            )
            .expect("fixture hawkes kernel parameters should construct"),
            GammaSeverityMarkConfiguration::try_new(1.8, 0.9)
                .expect("fixture gamma mark configuration should construct"),
        )
        .expect("fixture equation specification should construct"),
        ComplexScalar::new(0.2, -0.1),
        ComplexScalar::new(0.3, 0.05),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(0.95, 0.02),
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.03, 0.05)
            .expect("fixture news intensity should construct"),
        delta,
        HawkesExcitationState {
            y_plus_plus: 0.4,
            y_plus_minus: 0.2,
            y_minus_plus: 0.3,
            y_minus_minus: 0.5,
        },
    )
    .expect("fixture transform input should construct")
}

fn build_pricing_request(interval_delta: f64) -> VanillaOptionPricingRequest {
    let boundary_state = HawkesExcitationState {
        y_plus_plus: 0.4,
        y_plus_minus: 0.2,
        y_minus_plus: 0.3,
        y_minus_minus: 0.5,
    };

    let model_parameters = VanillaOptionPricingModelParameters::try_new(
        HawkesKernelParameters::try_new(
            0.8,
            1.2,
            HawkesKernelLoadings {
                plus_plus: 0.20,
                plus_minus: 0.08,
                minus_plus: 0.10,
                minus_minus: 0.24,
            },
            HawkesKernelLoadings {
                plus_plus: 0.03,
                plus_minus: 0.02,
                minus_plus: 0.01,
                minus_minus: 0.04,
            },
        )
        .expect("pricing fixture hawkes kernel parameters should construct"),
        NewsProcessConfiguration::try_new(
            NewsArrivalConfiguration {
                known: KnownNewsArrivalConfiguration {
                    scheduled_event_timestamps_utc_nanos: vec![],
                },
                surprise: SurpriseNewsArrivalConfiguration {
                    baseline_intensity: 0.0,
                },
            },
            SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration::new(1.8, 0.9),
                double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::new(
                    0.35, 2.1, 3.0,
                ),
                coupled_gap_marks: CoupledGapMarkConfiguration::new(0.6),
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 1.0,
                return_jump_scale: 1.0,
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 1.0,
                return_jump_scale: 1.0,
            },
        )
        .expect("pricing fixture news process configuration should construct"),
        janiform_model_kernels::nig::NigSharedPlusShiftConfiguration::try_new(
            2.3, -0.8, 0.7, 0.1, -0.2,
        )
        .expect("pricing fixture NIG configuration should construct"),
    )
    .expect("pricing fixture model parameters should construct");

    VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(101.0, 100.0, 0.5, 0.02, 0.01)
            .expect("pricing fixture market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            boundary_state,
            MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
                .expect("zero-kappa endogenous fitted parameters should construct"),
            interval_delta,
            MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
                .expect("zero observed boundary state should construct"),
        )
        .expect("endogenous anchored boundary input should construct"),
        model_parameters,
        interval_delta,
    )
    .expect("pricing fixture request should construct")
}
