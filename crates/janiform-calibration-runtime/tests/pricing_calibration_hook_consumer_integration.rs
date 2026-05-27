mod support;

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformIntegratorMethod, ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformSolverSettings, ExponentialAffineTransformTelemetryMode,
    solve_exponential_affine_transform_with_settings,
};
use janiform_calibration_runtime::build_option_surface_calibration_observation_from_pricing_hooks;
use janiform_engine_pricing::internal::{
    FourierInversionContour, FourierInversionEngineSettings,
    FourierInversionIntegrationStabilityControls, FourierInversionNumericQualityGate,
    FourierInversionNumericSettings, FourierInversionQuadratureRule,
    price_vanilla_option_via_fourier_inversion,
};
use janiform_engine_pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
    PricingIntervalScheduleMetadata, VanillaOptionPayoffConfiguration, VanillaOptionPayoffType,
    VanillaOptionPricingDiagnostics, VanillaOptionPricingKernelParameters,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingResponse, VanillaOptionPricingRunResultBuildInput,
    VanillaOptionPricingStatus, build_pricing_transform_input,
    build_vanilla_option_pricing_run_result,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_foundations::time::NANOS_PER_HOUR;
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
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use support::shared::black_scholes_log_return_characteristic_function;

fn reference_fourier_inversion_settings() -> FourierInversionEngineSettings {
    FourierInversionEngineSettings::try_new(
        FourierInversionContour::try_new(1.5).expect("contour should construct"),
        FourierInversionNumericSettings::try_new(
            150.0,
            4_096,
            FourierInversionQuadratureRule::Midpoint,
        )
        .expect("numeric settings should construct"),
        FourierInversionIntegrationStabilityControls::try_new(
            1.0e-5,
            1.0e-4,
            5.0e-4,
            1.0e-2,
            FourierInversionNumericQualityGate::DiagnosticsOnly,
        )
        .expect("stability controls should construct"),
    )
    .expect("engine settings should construct")
}

fn reference_affine_transform_solver_settings() -> ExponentialAffineTransformSolverSettings {
    ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        64,
    )
    .expect("solver settings should construct")
    .with_telemetry_mode(ExponentialAffineTransformTelemetryMode::Disabled)
}

#[test]
fn pricing_run_output_hooks_are_directly_consumable_by_calibration_adapter() {
    let market_inputs =
        VanillaOptionPricingMarketInputs::try_new_single_node(101.0, 100.0, 0.5, 0.02, 0.01)
            .expect("market inputs should construct");
    let maturity_interval = market_inputs
        .single_maturity_interval()
        .expect("market inputs should have a single maturity interval");

    let forward_price = market_inputs.spot
        * ((market_inputs.risk_free_rate - market_inputs.dividend_yield) * maturity_interval).exp();

    let discount_factor = (-market_inputs.risk_free_rate * maturity_interval).exp();

    let response = VanillaOptionPricingResponse::try_new(
        VanillaOptionPricingStatus::Succeeded,
        None,
        Some(12.5),
        Some(
            VanillaOptionPricingDiagnostics::try_new(
                forward_price,
                discount_factor,
                maturity_interval,
                0.55,
            )
            .expect("response diagnostics should construct"),
        ),
        Some(0.2),
        Some(janiform_engine_pricing::VanillaOptionImpliedVolatilityInversionStatus::Succeeded),
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
                activity_coupling_scale: 1.0,
                return_jump_scale: 1.0,
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 1.0,
                return_jump_scale: 1.0,
            },
        )
        .expect("news process configuration should construct"),
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("NIG shared-plus-shift configuration should construct"),
        janiform_model_kernels::return_clock::ReturnClockExposureParameters::neutral(),
        janiform_model_kernels::long_variance::LongVarianceParameters::neutral(),
        VanillaOptionPricingKernelParameters::identity(),
    )
    .expect("pricing model parameters should construct");

    let pricing_request = VanillaOptionPricingRequest::try_new(
        market_inputs.clone(),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            HawkesExcitationState {
                y_plus_plus: 0.2,
                y_plus_minus: 0.1,
                y_minus_plus: 0.3,
                y_minus_minus: 0.4,
            },
            janiform_model_kernels::long_variance::LongVarianceState::unit(),
            MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
                .expect("zero-gain mean reversion parameters should construct"),
            1.0,
            MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
                .expect("zero observed boundary state should construct"),
        )
        .expect("endogenous anchored boundary input should construct"),
        model_parameters,
        1.0,
    )
    .expect("pricing request should construct");

    let transform_input = build_pricing_transform_input(
        &pricing_request,
        ComplexScalar::new(0.2, -0.1),
        ComplexScalar::new(0.3, 0.05),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(0.95, 0.02),
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.03, 0.05)
            .expect("pricing-hook test news intensity should construct"),
    )
    .expect("pricing transform input should construct");

    let transform_diagnostics = solve_exponential_affine_transform_with_settings(
        transform_input,
        reference_affine_transform_solver_settings(),
    )
    .expect("transform solve should succeed");

    let integration_diagnostics = price_vanilla_option_via_fourier_inversion(
        &market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        reference_fourier_inversion_settings(),
        // Use a Black-Scholes baseline here to validate wiring, not Janiform dynamics.
        |argument| black_scholes_log_return_characteristic_function(&market_inputs, 0.2, argument),
    )
    .expect("Fourier pricing run should succeed")
    .diagnostics;

    let numeric_quality = integration_diagnostics.numeric_quality_diagnostics;

    let run_output =
        build_vanilla_option_pricing_run_result(VanillaOptionPricingRunResultBuildInput {
            response,
            output_selection: VanillaOptionPricingOptionalOutputSelection::new(true, true, true),
            market_inputs_for_calibration_hooks: Some(market_inputs),
            transform_diagnostics_source: Some(transform_diagnostics),
            integration_quality_source: Some(integration_diagnostics),
            spectral_source: None,
            runtime_interval_schedule: build_uniform_runtime_interval_schedule(1, 0.5),
            maturity_terminal_interval_indices: vec![0],
        })
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
            derivation_policy:
                "pricing_calibration_hook_consumer_integration.synthetic_schedule.v1".to_string(),
            factor_assignment_policy: "overlap_weighted_resampling_from_hourly_source_buckets.v1"
                .to_string(),
        },
        (1..=interval_count)
            .map(|interval_index| interval_index as i64 * interval_duration_nanos)
            .collect(),
    )
    .expect("uniform runtime interval schedule should construct")
}
