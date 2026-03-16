use crate::{
    FourierInversionDiagnostics, FourierInversionEngineSettings, VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType, VanillaOptionPricingDiagnostics, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingRequest, VanillaOptionPricingResponse,
    build_pricing_transform_input, price_vanilla_option_via_fourier_inversion,
    solve_exponential_affine_transform,
};
use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformDVector, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformNewsIntensityInput, ExponentialAffineTransformOutput,
};
use janiform_foundations::complex_math::{ComplexScalar, complex_exponential};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::news::GammaSeverityMarkConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

pub(super) fn build_valid_vanilla_option_request() -> VanillaOptionPricingRequest {
    VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new(101.0, 100.0, 0.5, 0.02, 0.01)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        HawkesExcitationState {
            y_plus_plus: 0.2,
            y_plus_minus: 0.1,
            y_minus_plus: 0.3,
            y_minus_minus: 0.4,
        },
        valid_model_parameters(),
    )
    .expect("request should construct")
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
        GammaSeverityMarkConfiguration::try_new(1.8, 0.7)
            .expect("gamma severity mark configuration should construct"),
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
    )
    .expect("reference response should construct")
}

pub(super) fn build_reference_transform_diagnostics_source()
-> ExponentialAffineTransformIntegrationResult {
    let transform_input = build_pricing_transform_input(
        build_valid_vanilla_option_request(),
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

pub(super) fn black_scholes_log_return_characteristic_function(
    market_inputs: VanillaOptionPricingMarketInputs,
    volatility: f64,
    argument: ComplexScalar,
) -> ComplexScalar {
    let variance = volatility * volatility * market_inputs.maturity_interval;
    let drift = (market_inputs.risk_free_rate
        - market_inputs.dividend_yield
        - 0.5 * volatility * volatility)
        * market_inputs.maturity_interval;
    let imaginary_unit_term = ComplexScalar::new(-argument.imaginary, argument.real);
    let drift_term = imaginary_unit_term.scale(drift);
    let variance_term = argument.square().scale(-0.5 * variance);

    complex_exponential(drift_term.add(variance_term))
}

pub(super) fn black_scholes_price(
    market_inputs: VanillaOptionPricingMarketInputs,
    volatility: f64,
    payoff_type: VanillaOptionPayoffType,
) -> f64 {
    let sigma_square_root_time = volatility * market_inputs.maturity_interval.sqrt();
    let log_moneyness = (market_inputs.spot / market_inputs.strike).ln();
    let carry = (market_inputs.risk_free_rate - market_inputs.dividend_yield)
        * market_inputs.maturity_interval;
    let d1 = (log_moneyness + carry + 0.5 * sigma_square_root_time * sigma_square_root_time)
        / sigma_square_root_time;
    let d2 = d1 - sigma_square_root_time;
    let discounted_spot = market_inputs.spot
        * (-market_inputs.dividend_yield * market_inputs.maturity_interval).exp();
    let discounted_strike = market_inputs.strike
        * (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp();

    match payoff_type {
        VanillaOptionPayoffType::Call => {
            discounted_spot * standard_normal_cdf(d1) - discounted_strike * standard_normal_cdf(d2)
        }
        VanillaOptionPayoffType::Put => {
            discounted_strike * standard_normal_cdf(-d2)
                - discounted_spot * standard_normal_cdf(-d1)
        }
    }
}

fn standard_normal_cdf(value: f64) -> f64 {
    if value < 0.0 {
        return 1.0 - standard_normal_cdf(-value);
    }

    let polynomial_scale = 1.0 / (1.0 + 0.231_641_9 * value);
    let polynomial_value = polynomial_scale
        * (0.319_381_53
            + polynomial_scale
                * (-0.356_563_782
                    + polynomial_scale
                        * (1.781_477_937
                            + polynomial_scale
                                * (-1.821_255_978 + polynomial_scale * 1.330_274_429))));
    let density = 0.398_942_280_401_432_7 * (-0.5 * value * value).exp();

    1.0 - density * polynomial_value
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
