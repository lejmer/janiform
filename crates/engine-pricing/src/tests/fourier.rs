use serde_json::json;

use crate::{
    FOURIER_INVERSION_DIAGNOSTICS_SCHEMA_VERSION, FourierInversionContour,
    FourierInversionContourError, FourierInversionEngineSettings,
    FourierInversionIntegrationStabilityControls,
    FourierInversionIntegrationStabilityControlsError, FourierInversionNumericQualityGate,
    FourierInversionNumericSettings, FourierInversionPricingError, FourierInversionQuadratureRule,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingMarketInputs,
    price_vanilla_option_via_fourier_inversion,
};
use janiform_foundations::complex_math::ComplexScalar;

use super::support::{black_scholes_log_return_characteristic_function, black_scholes_price};
#[test]
fn fourier_inversion_call_price_matches_black_scholes_sanity_case() {
    // Note: This validates Fourier inversion mechanics against a closed-form benchmark and does
    // NOT assume Janiform dynamics are Black-Scholes.

    let market_inputs = VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
        .expect("market inputs should construct");
    let volatility = 0.2;
    let settings = FourierInversionEngineSettings::try_new(
        FourierInversionContour::try_new(1.5).expect("contour settings should construct"),
        FourierInversionNumericSettings::try_new(
            160.0,
            8_192,
            FourierInversionQuadratureRule::Midpoint,
        )
        .expect("numeric settings should construct"),
    )
    .expect("engine settings should construct");

    let result = price_vanilla_option_via_fourier_inversion(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        settings,
        |argument| {
            black_scholes_log_return_characteristic_function(market_inputs, volatility, argument)
        },
    )
    .expect("fourier inversion call pricing should succeed");

    let expected_call_price =
        black_scholes_price(market_inputs, volatility, VanillaOptionPayoffType::Call);
    let absolute_error = (result.price - expected_call_price).abs();
    assert!(
        absolute_error <= 1.0e-3,
        "call absolute pricing error {absolute_error} exceeded tolerance"
    );
    assert_eq!(
        result.diagnostics.schema_version,
        FOURIER_INVERSION_DIAGNOSTICS_SCHEMA_VERSION
    );
}

#[test]
fn fourier_inversion_put_price_matches_black_scholes_sanity_case() {
    // Note: This validates Fourier inversion mechanics against a closed-form benchmark and does
    // NOT assume Janiform dynamics are Black-Scholes.

    let market_inputs = VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
        .expect("market inputs should construct");
    let volatility = 0.2;
    let settings = FourierInversionEngineSettings::try_new(
        FourierInversionContour::try_new(1.5).expect("contour settings should construct"),
        FourierInversionNumericSettings::try_new(
            160.0,
            8_192,
            FourierInversionQuadratureRule::Trapezoid,
        )
        .expect("numeric settings should construct"),
    )
    .expect("engine settings should construct");

    let result = price_vanilla_option_via_fourier_inversion(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Put,
        },
        settings,
        |argument| {
            black_scholes_log_return_characteristic_function(market_inputs, volatility, argument)
        },
    )
    .expect("fourier inversion put pricing should succeed");

    let expected_put_price =
        black_scholes_price(market_inputs, volatility, VanillaOptionPayoffType::Put);
    let absolute_error = (result.price - expected_put_price).abs();
    assert!(
        absolute_error <= 1.0e-3,
        "put absolute pricing error {absolute_error} exceeded tolerance"
    );
    assert_eq!(
        result.diagnostics.integrand_evaluation_count,
        result.diagnostics.integration_step_count + 1
    );
}

#[test]
fn fourier_inversion_rejects_invalid_integration_settings() {
    let contour_error = FourierInversionContour::try_new(0.0)
        .expect_err("zero damping factor must fail contour validation");
    assert!(matches!(
        contour_error,
        FourierInversionContourError::InvalidDampingFactor { value } if value == 0.0
    ));

    let market_inputs = VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
        .expect("market inputs should construct");
    let invalid_settings = FourierInversionEngineSettings::new(
        FourierInversionContour::new(1.0),
        FourierInversionNumericSettings::new(120.0, 0, FourierInversionQuadratureRule::Midpoint),
    );

    let error = price_vanilla_option_via_fourier_inversion(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        invalid_settings,
        |_argument| ComplexScalar::new(1.0, 0.0),
    )
    .expect_err("invalid integration settings must fail pricing");
    assert!(matches!(
        error,
        FourierInversionPricingError::InvalidSettings { .. }
    ));
}

#[test]
fn fourier_inversion_stability_controls_validate_and_deserialize_with_defaults() {
    let controls_error = FourierInversionIntegrationStabilityControls::try_new(
        f64::NAN,
        1.0e-4,
        1.0e-4,
        1.0e-2,
        FourierInversionNumericQualityGate::DiagnosticsOnly,
    )
    .expect_err("non-finite convergence tolerance must fail stability-control validation");
    assert!(matches!(
        controls_error,
        FourierInversionIntegrationStabilityControlsError::InvalidConvergenceAbsoluteTolerance { value } if value.is_nan()
    ));

    let parsed_settings: FourierInversionEngineSettings = serde_json::from_value(json!({
        "contour": {
            "damping_factor": 1.5
        },
        "numeric_settings": {
            "integration_upper_bound": 120.0,
            "integration_step_count": 4096,
            "quadrature_rule": "midpoint"
        }
    }))
    .expect("engine settings should deserialize with default stability controls");

    assert_eq!(
        parsed_settings.stability_controls,
        FourierInversionIntegrationStabilityControls::default()
    );
}

#[test]
fn fourier_inversion_emits_numeric_quality_diagnostics_for_poor_settings() {
    let market_inputs = VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
        .expect("market inputs should construct");
    let volatility = 0.2;

    let strict_controls = FourierInversionIntegrationStabilityControls::try_new(
        1.0e-8,
        1.0e-8,
        1.0e-8,
        1.0e-6,
        FourierInversionNumericQualityGate::DiagnosticsOnly,
    )
    .expect("strict stability controls should construct");
    let poor_settings = FourierInversionEngineSettings::try_new_with_stability_controls(
        FourierInversionContour::try_new(1.2).expect("contour settings should construct"),
        FourierInversionNumericSettings::try_new(3.0, 2, FourierInversionQuadratureRule::Midpoint)
            .expect("numeric settings should construct"),
        strict_controls,
    )
    .expect("engine settings should construct");

    let result = price_vanilla_option_via_fourier_inversion(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        poor_settings,
        |argument| {
            black_scholes_log_return_characteristic_function(market_inputs, volatility, argument)
        },
    )
    .expect("diagnostics-only quality gate should not fail pricing");

    assert!(
        result
            .diagnostics
            .numeric_quality_diagnostics
            .numeric_quality_is_poor
    );
}

#[test]
fn fourier_inversion_quality_gate_can_fail_under_poor_settings() {
    let market_inputs = VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
        .expect("market inputs should construct");
    let volatility = 0.2;

    let strict_controls = FourierInversionIntegrationStabilityControls::try_new(
        1.0e-8,
        1.0e-8,
        1.0e-8,
        1.0e-6,
        FourierInversionNumericQualityGate::EnforceFailure,
    )
    .expect("strict stability controls should construct");
    let poor_settings = FourierInversionEngineSettings::try_new_with_stability_controls(
        FourierInversionContour::try_new(1.2).expect("contour settings should construct"),
        FourierInversionNumericSettings::try_new(3.0, 2, FourierInversionQuadratureRule::Midpoint)
            .expect("numeric settings should construct"),
        strict_controls,
    )
    .expect("engine settings should construct");

    let error = price_vanilla_option_via_fourier_inversion(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        poor_settings,
        |argument| {
            black_scholes_log_return_characteristic_function(market_inputs, volatility, argument)
        },
    )
    .expect_err("enforced quality gate must fail on poor numeric quality");

    assert!(matches!(
        error,
        FourierInversionPricingError::NumericQualityFailure { diagnostics }
        if diagnostics.numeric_quality_is_poor
    ));
}

#[test]
fn fourier_inversion_quality_diagnostics_converge_on_reference_fixture() {
    let market_inputs = VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
        .expect("market inputs should construct");
    let volatility = 0.2;
    let settings = FourierInversionEngineSettings::try_new(
        FourierInversionContour::try_new(1.5).expect("contour settings should construct"),
        FourierInversionNumericSettings::try_new(
            160.0,
            8_192,
            FourierInversionQuadratureRule::Midpoint,
        )
        .expect("numeric settings should construct"),
    )
    .expect("engine settings should construct");

    let result = price_vanilla_option_via_fourier_inversion(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        settings,
        |argument| {
            black_scholes_log_return_characteristic_function(market_inputs, volatility, argument)
        },
    )
    .expect("reference settings should produce converged diagnostics");

    assert!(
        !result
            .diagnostics
            .numeric_quality_diagnostics
            .numeric_quality_is_poor
    );
    assert!(
        result
            .diagnostics
            .numeric_quality_diagnostics
            .convergence_check_passed
    );
    assert!(
        result
            .diagnostics
            .numeric_quality_diagnostics
            .truncation_check_passed
    );
}

#[test]
fn fourier_inversion_surfaces_non_finite_characteristic_function_boundary_failures() {
    let market_inputs = VanillaOptionPricingMarketInputs::try_new(100.0, 95.0, 0.5, 0.01, 0.0)
        .expect("market inputs should construct");
    let settings = FourierInversionEngineSettings::try_new(
        FourierInversionContour::try_new(1.2).expect("contour settings should construct"),
        FourierInversionNumericSettings::try_new(
            10.0,
            16,
            FourierInversionQuadratureRule::Midpoint,
        )
        .expect("numeric settings should construct"),
    )
    .expect("engine settings should construct");

    let error = price_vanilla_option_via_fourier_inversion(
        market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        settings,
        |argument| {
            if argument.real > 9.0 {
                return ComplexScalar::new(f64::NAN, 0.0);
            }
            ComplexScalar::new(1.0, 0.0)
        },
    )
    .expect_err("non-finite characteristic-function output should fail pricing");

    assert!(matches!(
        error,
        FourierInversionPricingError::NonFiniteCharacteristicFunctionValue { .. }
    ));
}

#[test]
fn fourier_inversion_output_is_deterministic_for_reference_fixture() {
    let market_inputs = VanillaOptionPricingMarketInputs::try_new(103.0, 100.0, 0.75, 0.03, 0.01)
        .expect("market inputs should construct");
    let volatility = 0.25;
    let settings = FourierInversionEngineSettings::default();
    let payoff = VanillaOptionPayoffConfiguration {
        payoff_type: VanillaOptionPayoffType::Call,
    };

    let first_result =
        price_vanilla_option_via_fourier_inversion(market_inputs, payoff, settings, |argument| {
            black_scholes_log_return_characteristic_function(market_inputs, volatility, argument)
        })
        .expect("first deterministic pricing run should succeed");
    let second_result =
        price_vanilla_option_via_fourier_inversion(market_inputs, payoff, settings, |argument| {
            black_scholes_log_return_characteristic_function(market_inputs, volatility, argument)
        })
        .expect("second deterministic pricing run should succeed");

    assert_eq!(first_result, second_result);
}
