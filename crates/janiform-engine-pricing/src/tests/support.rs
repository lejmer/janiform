use crate::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntegrationNumericQualityDiagnostics,
    PricingIntegrationProjectionRule, PricingIntegrationQualityDiagnostics,
    PricingIntegrationQualityGate, PricingIntervalSchedule, PricingIntervalScheduleMetadata,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingDiagnostics,
    VanillaOptionPricingKernelParameters, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingRequest, VanillaOptionPricingResponse,
    build_pricing_transform_input,
};
use crate::{
    PRICING_INTEGRATION_QUALITY_DIAGNOSTICS_SCHEMA_VERSION,
    PRICING_INTEGRATION_QUALITY_GATE_POLICY, SPECTRAL_PRICING_COLLOCATION_POLICY,
    SPECTRAL_PRICING_DEVIATION_DOMAIN_POLICY, SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
};
use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformDVector, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformIntegratorMethod, ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformOutput, ExponentialAffineTransformSolverSettings,
    ExponentialAffineTransformTelemetryMode, solve_exponential_affine_transform_with_settings,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_foundations::time::NANOS_PER_HOUR;
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::long_variance::{LongVarianceParameters, LongVarianceState};
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionFittedParameters,
};
use janiform_model_kernels::news::{
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

pub(crate) fn build_valid_vanilla_option_request() -> VanillaOptionPricingRequest {
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
            factor_assignment_policy:
                "overlap_weighted_resampling_from_interval_factor_grid_source_intervals.v1"
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
        LongVarianceState::unit(),
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
                    open_baseline_intensity: 0.75,
                    closed_baseline_intensity: 0.0,
                },
            },
            SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration::new(1.8),
            },
            NewsComponentMarkScaleConfiguration::neutral(),
            NewsComponentMarkScaleConfiguration::neutral(),
        )
        .expect("news process configuration should construct"),
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("nig shared-plus-shift configuration should construct"),
        janiform_model_kernels::return_clock::ReturnClockExposureParameters::neutral(),
        LongVarianceParameters::neutral(),
        VanillaOptionPricingKernelParameters::identity(),
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

    solve_exponential_affine_transform_with_settings(
        transform_input,
        build_reference_affine_transform_solver_settings(),
    )
    .expect("reference transform solve should succeed")
}

pub(super) fn build_reference_integration_diagnostics_source(
    market_inputs: &VanillaOptionPricingMarketInputs,
) -> PricingIntegrationQualityDiagnostics {
    let maturity_interval = market_inputs
        .single_maturity_interval()
        .expect("reference integration diagnostics require single-node market inputs");

    PricingIntegrationQualityDiagnostics {
        schema_version: PRICING_INTEGRATION_QUALITY_DIAGNOSTICS_SCHEMA_VERSION,
        engine_policy: SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
        domain_policy: SPECTRAL_PRICING_DEVIATION_DOMAIN_POLICY,
        projection_policy: SPECTRAL_PRICING_COLLOCATION_POLICY,
        stability_policy: SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
        projection_rule: PricingIntegrationProjectionRule::SpectralCosCollocation,
        integration_upper_bound: 10.0,
        integration_step_count: 64,
        integration_interval_width: 10.0 / 64.0,
        integrand_evaluation_count: 64,
        integral_estimate: 12.5,
        absolute_integral_estimate: 12.5,
        max_abs_integrand: 12.5,
        terminal_abs_integrand: 0.0,
        discounted_projection_price: 12.5,
        numeric_quality_diagnostics: PricingIntegrationNumericQualityDiagnostics {
            stability_policy: SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
            convergence_check_policy: SPECTRAL_PRICING_COLLOCATION_POLICY,
            truncation_check_policy: SPECTRAL_PRICING_DEVIATION_DOMAIN_POLICY,
            quality_gate_policy: PRICING_INTEGRATION_QUALITY_GATE_POLICY,
            quality_gate: PricingIntegrationQualityGate::DiagnosticsOnly,
            convergence_absolute_tolerance: maturity_interval,
            convergence_relative_tolerance: 1.0,
            terminal_integrand_absolute_tolerance: maturity_interval,
            terminal_integrand_ratio_tolerance: 1.0,
            coarse_integral_estimate: 12.5,
            convergence_absolute_difference: 0.0,
            convergence_relative_difference: 0.0,
            terminal_integrand_absolute_value: 0.0,
            terminal_integrand_ratio: 0.0,
            convergence_check_passed: true,
            truncation_check_passed: true,
            insufficient_convergence_resolution: false,
            numeric_quality_is_poor: false,
        },
    }
}

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
