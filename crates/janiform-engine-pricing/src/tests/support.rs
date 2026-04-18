use crate::fourier::{
    FourierInversionContour, FourierInversionDiagnostics, FourierInversionEngineSettings,
    FourierInversionIntegrationStabilityControls, FourierInversionNumericQualityGate,
    FourierInversionNumericSettings, FourierInversionQuadratureRule,
    price_vanilla_option_via_fourier_inversion,
};
use crate::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
    PricingIntervalScheduleMetadata, VanillaOptionPayoffConfiguration, VanillaOptionPayoffType,
    VanillaOptionPricingDiagnostics, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingRequest, VanillaOptionPricingResponse,
    build_pricing_transform_input, solve_exponential_affine_transform,
};
use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformDVector, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformIntegratorMethod, ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformOutput, ExponentialAffineTransformSolverSettings,
    ExponentialAffineTransformTelemetryMode,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_foundations::time::ONE_HOUR_NANOS;
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

pub(super) fn build_valid_vanilla_option_request() -> VanillaOptionPricingRequest {
    VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(101.0, 100.0, 1.0, 0.02, 0.01)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        endogenous_boundary_input(
            1.0,
            HawkesExcitationState {
                y_plus_plus: 0.2,
                y_plus_minus: 0.1,
                y_minus_plus: 0.3,
                y_minus_minus: 0.4,
            },
        ),
        valid_model_parameters(),
        1.0,
    )
    .expect("request should construct")
}

pub(super) fn build_reference_fourier_inversion_settings() -> FourierInversionEngineSettings {
    FourierInversionEngineSettings::try_new(
        FourierInversionContour::try_new(1.5).expect("reference contour settings should construct"),
        FourierInversionNumericSettings::try_new(
            150.0,
            4_096,
            FourierInversionQuadratureRule::Midpoint,
        )
        .expect("reference numeric settings should construct"),
        FourierInversionIntegrationStabilityControls::try_new(
            1.0e-5,
            1.0e-4,
            5.0e-4,
            1.0e-2,
            FourierInversionNumericQualityGate::DiagnosticsOnly,
        )
        .expect("reference stability controls should construct"),
    )
    .expect("reference Fourier engine settings should construct")
}

pub(super) fn build_reference_affine_transform_solver_settings()
-> ExponentialAffineTransformSolverSettings {
    ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        64,
    )
    .expect("reference affine solver settings should construct")
    .with_telemetry_mode(ExponentialAffineTransformTelemetryMode::Disabled)
}

pub(crate) fn build_uniform_runtime_interval_schedule(
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

pub(super) fn endogenous_boundary_input(
    interval_delta: f64,
    hawkes_boundary_state: HawkesExcitationState,
) -> AnchoredVanillaOptionPricingBoundaryInput {
    AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        hawkes_boundary_state,
        MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
            .expect("zero-gain mean reversion parameters should construct"),
        interval_delta,
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
            .expect("zero observed boundary state should construct"),
    )
    .expect("endogenous pricing boundary input should construct")
}

pub(super) fn disabled_boundary_input(
    interval_delta: f64,
    hawkes_boundary_state: HawkesExcitationState,
) -> AnchoredVanillaOptionPricingBoundaryInput {
    endogenous_boundary_input(interval_delta, hawkes_boundary_state)
}

pub(crate) fn valid_model_parameters() -> VanillaOptionPricingModelParameters {
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
            .expect("nig shared-plus-shift configuration should construct"),
    )
    .expect("model parameters should construct")
}

pub(super) fn build_reference_market_inputs() -> VanillaOptionPricingMarketInputs {
    VanillaOptionPricingMarketInputs::try_new_single_node(100.0, 95.0, 0.5, 0.02, 0.01)
        .expect("reference market inputs should construct")
}

pub(super) fn build_reference_pricing_response(
    market_inputs: &VanillaOptionPricingMarketInputs,
) -> VanillaOptionPricingResponse {
    let maturity_interval = market_inputs
        .single_maturity_interval()
        .expect("reference response requires single-node market inputs");

    let forward_price = market_inputs.spot
        * ((market_inputs.risk_free_rate - market_inputs.dividend_yield) * maturity_interval).exp();

    let discount_factor = (-market_inputs.risk_free_rate * maturity_interval).exp();

    VanillaOptionPricingResponse::try_new(
        crate::VanillaOptionPricingStatus::Succeeded,
        None,
        Some(12.5),
        Some(
            VanillaOptionPricingDiagnostics::try_new(
                forward_price,
                discount_factor,
                maturity_interval,
                0.55,
            )
            .expect("reference diagnostics should construct"),
        ),
        Some(0.2),
        Some(crate::VanillaOptionImpliedVolatilityInversionStatus::Succeeded),
    )
    .expect("reference response should construct")
}

pub(crate) fn build_reference_transform_diagnostics_source()
-> ExponentialAffineTransformIntegrationResult {
    let transform_input = build_pricing_transform_input(
        &build_valid_vanilla_option_request(),
        ComplexScalar::new(0.2, -0.1),
        ComplexScalar::new(0.3, 0.05),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(0.95, 0.02),
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.03, 0.05)
            .expect("reference pricing news intensity should construct"),
    )
    .expect("pricing transform input should construct");

    solve_exponential_affine_transform(
        transform_input,
        build_reference_affine_transform_solver_settings(),
    )
    .expect("reference transform solve should succeed")
}

pub(super) fn build_reference_integration_diagnostics_source(
    market_inputs: &VanillaOptionPricingMarketInputs,
) -> FourierInversionDiagnostics {
    let result = price_vanilla_option_via_fourier_inversion(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        build_reference_fourier_inversion_settings(),
        |argument| black_scholes_log_return_characteristic_function(market_inputs, 0.2, argument),
    )
    .expect("reference Fourier pricing run should succeed");

    result.diagnostics
}

pub(super) use crate::{
    intraday_black_scholes_log_return_characteristic_function as black_scholes_log_return_characteristic_function,
    intraday_black_scholes_price as black_scholes_price,
};

pub(super) fn assert_complex_close(
    observed: ComplexScalar,
    expected: ComplexScalar,
    tolerance: f64,
) {
    let difference = observed.subtract(expected).norm();
    assert!(
        difference <= tolerance,
        "difference {difference} exceeded tolerance {tolerance} for observed {observed:?} and expected {expected:?}"
    );
}

pub(super) fn oracle_output_with_transform_value(
    transform_value: ComplexScalar,
) -> ExponentialAffineTransformOutput {
    ExponentialAffineTransformOutput::try_new(
        ComplexScalar::new(0.0, 0.0),
        ExponentialAffineTransformDVector::new(
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
        ),
        transform_value,
    )
    .expect("oracle output helper payload should validate")
}
