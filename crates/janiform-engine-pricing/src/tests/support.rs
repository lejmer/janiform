use crate::{
    AnchoredVanillaOptionPricingBoundaryInput, FourierInversionDiagnostics,
    FourierInversionEngineSettings, VanillaOptionPayoffConfiguration, VanillaOptionPayoffType,
    VanillaOptionPricingDiagnostics, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingRequest, VanillaOptionPricingResponse,
    build_pricing_transform_input, price_vanilla_option_via_fourier_inversion,
    solve_exponential_affine_transform,
};
use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformDVector, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformNewsIntensityInput, ExponentialAffineTransformOutput,
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
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

pub(super) fn build_valid_vanilla_option_request() -> VanillaOptionPricingRequest {
    VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new(101.0, 100.0, 1.0, 0.02, 0.01)
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
    )
    .expect("request should construct")
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

pub(super) fn valid_model_parameters() -> VanillaOptionPricingModelParameters {
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
            .expect("nig shared-plus-shift configuration should construct"),
    )
    .expect("model parameters should construct")
}

pub(super) fn build_reference_market_inputs() -> VanillaOptionPricingMarketInputs {
    VanillaOptionPricingMarketInputs::try_new(100.0, 95.0, 0.5, 0.02, 0.01)
        .expect("reference market inputs should construct")
}

pub(super) fn build_reference_pricing_response(
    market_inputs: VanillaOptionPricingMarketInputs,
) -> VanillaOptionPricingResponse {
    let forward_price = market_inputs.spot
        * ((market_inputs.risk_free_rate - market_inputs.dividend_yield)
            * market_inputs.maturity_interval)
            .exp();

    let discount_factor = (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp();

    VanillaOptionPricingResponse::try_new(
        12.5,
        VanillaOptionPricingDiagnostics::try_new(
            forward_price,
            discount_factor,
            market_inputs.maturity_interval,
            0.55,
        )
        .expect("reference diagnostics should construct"),
        Some(0.2),
        crate::VanillaOptionImpliedVolatilityInversionStatus::Succeeded,
    )
    .expect("reference response should construct")
}

pub(super) fn build_reference_transform_diagnostics_source()
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

    solve_exponential_affine_transform(transform_input)
        .expect("reference transform solve should succeed")
}

pub(super) fn build_reference_integration_diagnostics_source(
    market_inputs: VanillaOptionPricingMarketInputs,
) -> FourierInversionDiagnostics {
    let result = price_vanilla_option_via_fourier_inversion(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        FourierInversionEngineSettings::default(),
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
