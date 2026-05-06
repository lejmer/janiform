use super::math::stable_gamma;
use super::{
    NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_DETERMINISM_POLICY,
    NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_REPORT_SCHEMA_VERSION,
    NIG_CHARACTERISTIC_EXPONENT_STABILITY_POLICY, NIG_INCREMENT_NEAR_ZERO_SCALE_POLICY,
    NIG_INCREMENT_SAMPLING_POLICY, NIG_SHARED_PLUS_SHIFT_CONFIGURATION_POLICY,
    NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY, NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY,
    NigCharacteristicExponentGuardrailCode, NigIncrementSamplingError, NigNumericalGuardrailStatus,
    NigParameterConstraint, NigParameterError, NigParameters, NigSharedPlusShiftConfiguration,
    NigSharedPlusShiftConfigurationError, NigTradingTimeIncrementSamplingError,
    NigValidationStatus,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_foundations::randomness::{
    SimulationRandomnessStream, SplitMix64SimulationRandomnessStream,
};

#[test]
fn valid_interior_parameter_payload_constructs_and_validates() {
    let parameters = NigParameters::try_new(2.5, -0.9, 0.35, -0.05)
        .expect("interior NIG parameters must construct successfully");

    assert_eq!(parameters.alpha, 2.5);
    assert_eq!(parameters.beta, -0.9);
    assert_eq!(parameters.delta, 0.35);
    assert_eq!(parameters.mu, -0.05);

    let report = parameters.validate();
    assert_eq!(report.status, NigValidationStatus::Pass);
    assert!(report.violations.is_empty());
}

#[test]
fn parameter_payload_serialization_round_trip_preserves_values() {
    let parameters =
        NigParameters::try_new(3.2, 0.6, 0.8, 0.01).expect("valid parameters must be serializable");

    let serialized =
        serde_json::to_string(&parameters).expect("canonical payload should serialize");
    let parsed: NigParameters =
        serde_json::from_str(&serialized).expect("serialized payload should deserialize");

    assert_eq!(parsed, parameters);
}

#[test]
fn shared_plus_shift_configuration_derives_expected_buy_and_sell_parameters() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct");

    let derived_driver_parameters = configuration
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");

    assert_eq!(
        derived_driver_parameters.buy_side_parameters,
        NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("buy-side expected payload should be valid")
    );
    assert_eq!(
        derived_driver_parameters.sell_side_parameters,
        NigParameters::try_new(2.3, -1.0, 0.7, 0.1)
            .expect("sell-side expected payload should be valid")
    );
    assert_eq!(configuration.derived_sell_beta(), -1.0);
    assert_eq!(
        derived_driver_parameters.policy,
        NIG_SHARED_PLUS_SHIFT_CONFIGURATION_POLICY
    );
}

#[test]
fn shared_plus_shift_configuration_rejects_invalid_derived_sell_side_domain() {
    let error = NigSharedPlusShiftConfiguration::try_new(1.0, 0.2, 0.3, 0.0, -1.3)
        .expect_err("derived sell-side beta at alpha boundary should fail");

    match error {
        NigSharedPlusShiftConfigurationError::SellSideParameterDomainViolation {
            beta_0,
            delta_beta,
            derived_sell_beta,
            source,
        } => {
            assert_eq!(beta_0, 0.2);
            assert_eq!(delta_beta, -1.3);
            assert_eq!(derived_sell_beta, -1.1);
            assert_eq!(
                source,
                NigParameterError::DomainViolation {
                    field: "alpha",
                    constraint: NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField,
                    value: 1.0,
                    comparison_field: Some("beta"),
                    comparison_value: Some(1.1),
                }
            );
        }
        _ => panic!("unexpected error variant: {error}"),
    }
}

#[test]
fn shared_plus_shift_configuration_rejects_non_negative_delta_beta() {
    for value in [0.0, 0.15] {
        let error = NigSharedPlusShiftConfiguration::try_new(2.0, -0.5, 0.4, 0.0, value)
            .expect_err("non-negative delta_beta must fail");
        assert_eq!(
            error,
            NigSharedPlusShiftConfigurationError::DeltaBetaDomainViolation { value }
        );
    }
}

#[test]
fn shared_plus_shift_configuration_rejects_non_finite_delta_beta() {
    let error = NigSharedPlusShiftConfiguration::try_new(2.0, -0.5, 0.4, 0.0, f64::NAN)
        .expect_err("non-finite delta_beta must fail");
    match error {
        NigSharedPlusShiftConfigurationError::DeltaBetaDomainViolation { value } => {
            assert!(value.is_nan());
        }
        _ => panic!("unexpected error variant: {error}"),
    }
}

#[test]
fn shared_plus_shift_configuration_rejects_invalid_buy_side_domain() {
    let error = NigSharedPlusShiftConfiguration::try_new(0.6, 0.8, 0.3, 0.0, -0.4)
        .expect_err("invalid buy-side domain should fail before sell-side checks");

    match error {
        NigSharedPlusShiftConfigurationError::BuySideParameterDomainViolation { source } => {
            assert_eq!(
                source,
                NigParameterError::DomainViolation {
                    field: "alpha",
                    constraint: NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField,
                    value: 0.6,
                    comparison_field: Some("beta"),
                    comparison_value: Some(0.8),
                }
            );
        }
        _ => panic!("unexpected error variant: {error}"),
    }
}

#[test]
fn shared_plus_shift_configuration_output_is_deterministic() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(2.4, -0.65, 0.55, -0.03, -0.12)
        .expect("valid shared-plus-shift payload should construct");

    let first_derived_driver_parameters = configuration
        .derive_driver_parameters()
        .expect("first derivation should succeed");
    let second_derived_driver_parameters = configuration
        .derive_driver_parameters()
        .expect("second derivation should succeed");

    assert_eq!(
        first_derived_driver_parameters,
        second_derived_driver_parameters
    );

    let first_serialized_payload = serde_json::to_string(&first_derived_driver_parameters)
        .expect("first derived payload should serialize");
    let second_serialized_payload = serde_json::to_string(&second_derived_driver_parameters)
        .expect("second derived payload should serialize");
    assert_eq!(first_serialized_payload, second_serialized_payload);
}

#[test]
fn boundary_and_out_of_domain_inputs_report_field_and_constraint() {
    let alpha_beta_boundary_error =
        NigParameters::try_new(1.2, -1.2, 0.5, 0.0).expect_err("alpha == abs(beta) must fail");
    assert_eq!(
        alpha_beta_boundary_error,
        NigParameterError::DomainViolation {
            field: "alpha",
            constraint: NigParameterConstraint::StrictlyGreaterThanAbsoluteComparisonField,
            value: 1.2,
            comparison_field: Some("beta"),
            comparison_value: Some(1.2),
        }
    );

    let delta_boundary_error =
        NigParameters::try_new(2.0, 0.5, 0.0, 0.0).expect_err("delta == 0 must fail");
    assert_eq!(
        delta_boundary_error,
        NigParameterError::DomainViolation {
            field: "delta",
            constraint: NigParameterConstraint::FiniteAndStrictlyPositive,
            value: 0.0,
            comparison_field: None,
            comparison_value: Some(0.0),
        }
    );

    let report = NigParameters::validate_domain(f64::NAN, f64::NEG_INFINITY, -1.0, f64::NAN);
    assert_eq!(report.status, NigValidationStatus::Fail);
    assert_eq!(
        report
            .violations
            .iter()
            .map(|violation| (violation.field, violation.constraint))
            .collect::<Vec<_>>(),
        vec![
            ("alpha", NigParameterConstraint::Finite),
            ("beta", NigParameterConstraint::Finite),
            ("delta", NigParameterConstraint::FiniteAndStrictlyPositive),
            ("mu", NigParameterConstraint::Finite),
        ],
    );
}

#[test]
fn deserialization_rejects_invalid_domain_payload() {
    let payload = serde_json::json!({
        "alpha": 1.0,
        "beta": -1.0,
        "delta": 0.25,
        "mu": 0.0
    });

    let error = serde_json::from_value::<NigParameters>(payload)
        .expect_err("invalid payload must fail deserialization");
    let error_message = error.to_string();
    assert!(error_message.contains("alpha"));
    assert!(error_message.contains("strictly greater than abs(beta)"));
}

#[test]
fn characteristic_exponent_matches_reference_values_for_selected_complex_arguments() {
    let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
        .expect("interior NIG parameters should construct");

    let reference_fixture = [
        (ComplexScalar::new(0.0, 0.0), ComplexScalar::new(0.0, 0.0)),
        (
            ComplexScalar::new(0.75, 0.0),
            ComplexScalar::new(-0.09910996139538936, -0.10776988880696167),
        ),
        (
            ComplexScalar::new(-1.25, 0.5),
            ComplexScalar::new(-0.20167135219211368, 0.3543390690583961),
        ),
        (
            ComplexScalar::new(2.0, -1.75),
            ComplexScalar::new(-0.39180925343099193, 0.6483982349995301),
        ),
        (
            ComplexScalar::new(-3.4, 1.2),
            ComplexScalar::new(-1.3912349251792828, 0.8582572631758696),
        ),
    ];

    for (argument, expected) in reference_fixture {
        let observed = parameters.characteristic_exponent(argument);
        assert_complex_close(observed, expected, 1.0e-13);
    }
}

#[test]
fn characteristic_exponent_respects_conjugate_symmetry_for_real_arguments() {
    let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
        .expect("interior NIG parameters should construct");

    let argument_grid = [0.2, 0.75, 2.5, 8.0];
    for argument in argument_grid {
        let positive = parameters.characteristic_exponent(ComplexScalar::new(argument, 0.0));
        let negative = parameters.characteristic_exponent(ComplexScalar::new(-argument, 0.0));
        assert_complex_close(negative, positive.conjugate(), 1.0e-13);
    }
}

#[test]
fn characteristic_exponent_is_deterministic_for_repeated_calls() {
    let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
        .expect("interior NIG parameters should construct");
    let argument = ComplexScalar::new(-1.7, 0.35);
    let expected = parameters.characteristic_exponent(argument);

    for _ in 0..128 {
        let observed = parameters.characteristic_exponent(argument);
        assert_eq!(observed, expected);
    }
}

#[test]
fn characteristic_exponent_returns_exact_zero_at_origin() {
    let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
        .expect("interior NIG parameters should construct");

    assert_eq!(
        parameters.characteristic_exponent(ComplexScalar::new(0.0, 0.0)),
        ComplexScalar::new(0.0, 0.0)
    );
}

#[test]
fn characteristic_exponent_is_finite_for_representative_pricing_grid_arguments() {
    let parameters = NigParameters::try_new(1.000001, 1.0, 0.2, -0.03)
        .expect("near-boundary but valid parameters should construct");

    let real_argument_grid = [-150.0, -50.0, -10.0, 0.0, 10.0, 50.0, 150.0];
    let imaginary_argument_grid = [-2.0, -0.5, 0.0, 0.5, 2.0];

    for real_argument in real_argument_grid {
        for imaginary_argument in imaginary_argument_grid {
            let value = parameters
                .characteristic_exponent(ComplexScalar::new(real_argument, imaginary_argument));
            assert!(value.real.is_finite());
            assert!(value.imaginary.is_finite());
        }
    }
}

#[test]
fn characteristic_exponent_guardrail_report_schema_is_machine_readable() {
    let parameters = NigParameters::try_new(1.00000000005, 1.0, 0.2, -0.03)
        .expect("near-boundary but valid parameters should construct");
    let report = parameters.characteristic_exponent_with_guardrails(ComplexScalar::new(0.0, 0.0));

    let serialized_report =
        serde_json::to_value(&report).expect("guardrail report should serialize");

    assert_eq!(
        serialized_report["schema_version"],
        serde_json::Value::String(
            NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_REPORT_SCHEMA_VERSION.to_string()
        )
    );
    assert_eq!(
        serialized_report["determinism_policy"],
        serde_json::Value::String(
            NIG_CHARACTERISTIC_EXPONENT_GUARDRAIL_DETERMINISM_POLICY.to_string()
        )
    );
    assert_eq!(
        serialized_report["stability_policy"],
        serde_json::Value::String(NIG_CHARACTERISTIC_EXPONENT_STABILITY_POLICY.to_string())
    );
    assert!(serialized_report.get("status").is_some());
    assert!(serialized_report.get("argument").is_some());
    assert!(serialized_report.get("events").is_some());
    assert!(serialized_report.get("value").is_some());
}

#[test]
fn characteristic_exponent_guardrails_trigger_near_alpha_beta_boundary() {
    let parameters = NigParameters::try_new(1.00000000005, 1.0, 0.2, -0.03)
        .expect("near-boundary but valid parameters should construct");
    let report = parameters.characteristic_exponent_with_guardrails(ComplexScalar::new(0.0, 0.0));

    assert_eq!(report.status, NigNumericalGuardrailStatus::Warning);
    assert!(report.value.is_some());
    assert!(
        report.events.iter().any(|event| event.code
            == NigCharacteristicExponentGuardrailCode::AlphaBetaBoundaryProximity)
    );
}

#[test]
fn characteristic_exponent_guardrails_fail_on_alpha_beta_domain_violation() {
    let invalid_parameters = NigParameters {
        alpha: 1.0,
        beta: 1.0,
        delta: 0.2,
        mu: -0.03,
    };
    let report =
        invalid_parameters.characteristic_exponent_with_guardrails(ComplexScalar::new(0.3, 0.0));

    assert_eq!(report.status, NigNumericalGuardrailStatus::Fail);
    assert!(report.value.is_none());
    assert!(report.events.iter().any(
        |event| event.code == NigCharacteristicExponentGuardrailCode::AlphaBetaDomainViolation
    ));
}

#[test]
fn characteristic_exponent_guardrail_path_is_stable_for_extreme_argument_ranges() {
    let parameters = NigParameters::try_new(1.00000000005, 1.0, 0.2, -0.03)
        .expect("near-boundary but valid parameters should construct");

    let real_argument_grid = [-250.0, -100.0, -20.0, 0.0, 20.0, 100.0, 250.0];
    let imaginary_argument_grid = [-6.0, -2.0, 0.0, 2.0, 6.0];

    for real_argument in real_argument_grid {
        for imaginary_argument in imaginary_argument_grid {
            let report = parameters.characteristic_exponent_with_guardrails(ComplexScalar::new(
                real_argument,
                imaginary_argument,
            ));

            assert_ne!(report.status, NigNumericalGuardrailStatus::Fail);
            assert!(report.value.is_some());

            let value = report
                .value
                .expect("successful guardrail evaluation should include a value");

            assert!(value.real.is_finite());
            assert!(value.imaginary.is_finite());
        }
    }
}

#[test]
fn buy_and_sell_sign_convention_helpers_match_equation_mapping() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(0.9, -0.15);

    let observed_buy_side = driver_parameters.buy_side_characteristic_exponent(argument);
    let expected_buy_side = driver_parameters
        .buy_side_parameters
        .characteristic_exponent(argument);
    assert_complex_close(observed_buy_side, expected_buy_side, 1.0e-13);

    let observed_sell_side =
        driver_parameters.sell_side_characteristic_exponent_for_return_composition(argument);
    let expected_sell_side = driver_parameters
        .sell_side_parameters
        .characteristic_exponent(argument.scale(-1.0));
    assert_complex_close(observed_sell_side, expected_sell_side, 1.0e-13);
}

#[test]
fn trading_time_weighted_exponent_composition_matches_manual_assembly() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(-0.6, 0.35);
    let integrated_buy_intensity = 1.7;
    let integrated_sell_intensity = 0.9;

    let observed = driver_parameters.compose_trading_time_weighted_characteristic_exponent(
        argument,
        integrated_buy_intensity,
        integrated_sell_intensity,
    );

    let expected_buy_side = driver_parameters
        .buy_side_parameters
        .characteristic_exponent(argument)
        .scale(integrated_buy_intensity);
    let expected_sell_side = driver_parameters
        .sell_side_parameters
        .characteristic_exponent(argument.scale(-1.0))
        .scale(integrated_sell_intensity);
    let expected_combined = expected_buy_side.plus(expected_sell_side);

    assert_complex_close(
        observed.buy_side_weighted_exponent,
        expected_buy_side,
        1.0e-13,
    );
    assert_complex_close(
        observed.sell_side_weighted_exponent,
        expected_sell_side,
        1.0e-13,
    );
    assert_complex_close(
        observed.combined_weighted_exponent,
        expected_combined,
        1.0e-13,
    );
    assert_eq!(
        observed.policy,
        NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY
    );
}

#[test]
fn trading_time_increment_sampling_composition_matches_manual_buy_minus_sell() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let integrated_buy_intensity = 0.75;
    let integrated_sell_intensity = 0.45;

    let mut shared_stream = SplitMix64SimulationRandomnessStream::from_seed(88_101);
    let sampled_contributions = driver_parameters
        .sample_trading_time_return_increment(
            integrated_buy_intensity,
            integrated_sell_intensity,
            &mut shared_stream,
        )
        .expect("shared sampling interface should succeed");

    let mut manual_buy_stream = SplitMix64SimulationRandomnessStream::from_seed(88_101);
    let expected_buy_increment = driver_parameters
        .buy_side_parameters
        .sample_increment(integrated_buy_intensity, &mut manual_buy_stream)
        .expect("buy-side manual sampling should succeed");
    let expected_sell_increment = driver_parameters
        .sell_side_parameters
        .sample_increment(integrated_sell_intensity, &mut manual_buy_stream)
        .expect("sell-side manual sampling should succeed");
    let expected_combined_increment = expected_buy_increment - expected_sell_increment;

    assert_float_close(
        sampled_contributions.buy_side_increment,
        expected_buy_increment,
        1.0e-13,
    );
    assert_float_close(
        sampled_contributions.sell_side_increment,
        expected_sell_increment,
        1.0e-13,
    );
    assert_float_close(
        sampled_contributions.combined_increment,
        expected_combined_increment,
        1.0e-13,
    );
    assert_eq!(
        sampled_contributions.policy,
        NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY
    );
}

#[test]
fn trading_time_increment_sampling_reports_side_specific_error_context() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(88_102);

    let error = driver_parameters
        .sample_trading_time_return_increment(-0.25, 0.2, &mut randomness_stream)
        .expect_err("invalid buy-side intensity should fail");

    match error {
        NigTradingTimeIncrementSamplingError::BuySideSamplingFailure {
            integrated_buy_intensity,
            source,
        } => {
            assert_eq!(integrated_buy_intensity, -0.25);
            assert_eq!(
                source,
                NigIncrementSamplingError::InvalidBusinessTimeIncrementScale { value: -0.25 }
            );
        }
        _ => panic!("unexpected error variant: {error}"),
    }
}

#[test]
fn increment_sampling_policies_are_stable() {
    assert_eq!(
        NIG_INCREMENT_SAMPLING_POLICY,
        "normal_inverse_gaussian_via_inverse_gaussian_subordinator_with_box_muller_standard_normal.v1"
    );
    assert_eq!(
        NIG_INCREMENT_NEAR_ZERO_SCALE_POLICY,
        "zero_business_time_scale_returns_zero_without_random_draws.v1"
    );
}

#[test]
fn increment_sampling_rejects_invalid_business_time_increment_scale() {
    let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
        .expect("interior NIG parameters should construct");
    let mut randomness_stream = DrawCountRandomnessStream::default();

    for invalid_scale in [f64::NAN, f64::INFINITY, -0.5] {
        let error = parameters
            .sample_increment(invalid_scale, &mut randomness_stream)
            .expect_err("invalid business-time scale should fail sampling");

        match error {
            NigIncrementSamplingError::InvalidBusinessTimeIncrementScale { value } => {
                if invalid_scale.is_nan() {
                    assert!(value.is_nan());
                } else {
                    assert_eq!(value, invalid_scale);
                }
            }
            _ => panic!("unexpected error variant: {error}"),
        }
    }
}

#[test]
fn increment_sampling_zero_business_time_returns_zero_without_random_draws() {
    let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
        .expect("interior NIG parameters should construct");
    let mut randomness_stream = DrawCountRandomnessStream::default();

    let sampled_increment = parameters
        .sample_increment(0.0, &mut randomness_stream)
        .expect("zero business-time increment should sample");

    assert_eq!(sampled_increment, 0.0);
    assert_eq!(randomness_stream.draw_count, 0);
}

#[test]
fn increment_sampling_small_and_moderate_business_time_scales_remain_finite() {
    let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
        .expect("interior NIG parameters should construct");
    let mut small_scale_stream = SplitMix64SimulationRandomnessStream::from_seed(11_009);
    let mut small_scale_maximum_absolute_sample = 0.0_f64;

    for _sample_number in 0..2_000 {
        let sampled_increment = parameters
            .sample_increment(1.0e-12, &mut small_scale_stream)
            .expect("small business-time increment should sample");
        assert!(sampled_increment.is_finite());
        small_scale_maximum_absolute_sample =
            small_scale_maximum_absolute_sample.max(sampled_increment.abs());
    }

    assert!(
        small_scale_maximum_absolute_sample < 1.0e-3,
        "very small business-time increments should stay numerically near zero, observed max abs sample={small_scale_maximum_absolute_sample}"
    );

    let mut moderate_scale_stream = SplitMix64SimulationRandomnessStream::from_seed(11_010);
    let moderate_scale_path = [0.25, 0.5, 1.0, 1.5];
    let sampled_path = parameters
        .sample_increment_path(&moderate_scale_path, &mut moderate_scale_stream)
        .expect("moderate business-time increment path should sample");

    assert_eq!(sampled_path.len(), moderate_scale_path.len());
    assert!(sampled_path.iter().all(|sample| sample.is_finite()));

    let path_span = sampled_path.iter().copied().fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
    );
    assert!(
        path_span.1 - path_span.0 > 1.0e-10,
        "moderate-scale path should contain non-identical sampled increments"
    );
}

#[test]
fn increment_sampling_is_seed_reproducible_for_splitmix_streams() {
    let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
        .expect("interior NIG parameters should construct");
    let business_time_scale_path = [0.4, 0.2, 1.0e-8, 0.0, 0.75, 1.25, 0.05];

    let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(4_711);
    let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(4_711);
    let mut third_stream = SplitMix64SimulationRandomnessStream::from_seed(4_712);

    let first_samples = parameters
        .sample_increment_path(&business_time_scale_path, &mut first_stream)
        .expect("first seeded path should sample");
    let second_samples = parameters
        .sample_increment_path(&business_time_scale_path, &mut second_stream)
        .expect("second seeded path should sample");
    let third_samples = parameters
        .sample_increment_path(&business_time_scale_path, &mut third_stream)
        .expect("different-seed path should sample");

    assert_eq!(first_samples, second_samples);
    assert_ne!(first_samples, third_samples);
}

#[test]
fn increment_sampling_distribution_sanity_matches_expected_moments() {
    let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
        .expect("interior NIG parameters should construct");
    let business_time_increment_scale = 0.75;
    let sample_count = 40_000_usize;
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(91_177);

    let gamma = stable_gamma(parameters.alpha, parameters.beta);
    let expected_mean = business_time_increment_scale
        * (parameters.mu + parameters.delta * parameters.beta / gamma);
    let expected_variance = business_time_increment_scale
        * (parameters.delta * parameters.alpha * parameters.alpha / gamma.powi(3));

    let mut running_mean = 0.0;
    let mut running_squared_error = 0.0;

    for sample_number in 1..=sample_count {
        let sampled_increment = parameters
            .sample_increment(business_time_increment_scale, &mut randomness_stream)
            .expect("distribution sanity sampling should succeed");
        let sample_number_as_float = sample_number as f64;

        let delta_from_running_mean = sampled_increment - running_mean;
        running_mean += delta_from_running_mean / sample_number_as_float;

        let updated_delta = sampled_increment - running_mean;

        running_squared_error += delta_from_running_mean * updated_delta;
    }

    let sample_variance = running_squared_error / (sample_count as f64 - 1.0);
    let mean_tolerance = 8.0 * (expected_variance / sample_count as f64).sqrt() + 5.0e-3;
    let variance_tolerance = expected_variance * 0.30 + 5.0e-3;

    assert!(
        (running_mean - expected_mean).abs() <= mean_tolerance,
        "sample mean deviated from expected mean beyond tolerance, expected={expected_mean}, observed={running_mean}, tolerance={mean_tolerance}"
    );
    assert!(
        (sample_variance - expected_variance).abs() <= variance_tolerance,
        "sample variance deviated from expected variance beyond tolerance, expected={expected_variance}, observed={sample_variance}, tolerance={variance_tolerance}"
    );
}

#[test]
fn expected_increment_matches_closed_form_first_moment() {
    let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
        .expect("interior NIG parameters should construct");

    let business_time_increment_scale = 0.75;

    let gamma = stable_gamma(parameters.alpha, parameters.beta);

    let expected_mean = business_time_increment_scale
        * (parameters.mu + parameters.delta * parameters.beta / gamma);

    let observed_mean = parameters
        .expected_increment(business_time_increment_scale)
        .expect("closed-form first moment should evaluate");

    assert_float_close(observed_mean, expected_mean, 1.0e-12);
}

fn assert_complex_close(observed: ComplexScalar, expected: ComplexScalar, tolerance: f64) {
    let difference = observed.subtract(expected).norm();
    assert!(
        difference <= tolerance,
        "difference {difference} exceeded tolerance {tolerance} for observed {observed} and expected {expected}"
    );
}

fn assert_float_close(observed: f64, expected: f64, tolerance: f64) {
    let difference = (observed - expected).abs();
    assert!(
        difference <= tolerance,
        "difference {difference} exceeded tolerance {tolerance} for observed {observed} and expected {expected}"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct DrawCountRandomnessStream {
    draw_count: u64,
}

impl SimulationRandomnessStream for DrawCountRandomnessStream {
    fn next_u64(&mut self) -> u64 {
        self.draw_count = self.draw_count.saturating_add(1);
        self.draw_count
    }
}
