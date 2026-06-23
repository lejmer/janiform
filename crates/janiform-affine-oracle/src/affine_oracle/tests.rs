use serde_json::{Value, json};

use super::test_support::{standard_initial_state, valid_equation_specification};
use super::{
    EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_DEFAULT_SOLVER_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_EQUATION_MAPPING_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_TELEMETRY_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_NEWS_INTENSITY_USAGE_POLICY,
    ExponentialAffineTransformActivityLoadings, ExponentialAffineTransformDVector,
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformGuardFailureCode,
    ExponentialAffineTransformInput, ExponentialAffineTransformIntegrationStatus,
    ExponentialAffineTransformIntegratorMethod, ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformOutput, ExponentialAffineTransformSolverError,
    ExponentialAffineTransformSolverSettings, ExponentialAffineTransformTelemetryMode,
    evaluate_exponential_affine_transform_hamiltonians,
    evaluate_exponential_affine_transform_ode_rhs, solve_exponential_affine_transform,
    solve_exponential_affine_transform_input_sensitivities,
    solve_exponential_affine_transform_with_settings,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::long_variance::LongVarianceParameters;
use janiform_model_kernels::market_pressure::{MarketPressureParameters, MarketPressureState};
use janiform_model_kernels::news::{
    GammaSeverityMarkConfiguration, NewsComponentMarkScaleConfiguration,
};

fn total_activity_loadings(
    u_plus: ComplexScalar,
    u_minus: ComplexScalar,
) -> ExponentialAffineTransformActivityLoadings {
    ExponentialAffineTransformActivityLoadings::total_activity(u_plus, u_minus)
}

fn neutral_market_pressure_parameters() -> MarketPressureParameters {
    MarketPressureParameters::no_anchor_impact()
}

fn neutral_market_pressure_state() -> MarketPressureState {
    MarketPressureState::neutral()
}

fn valid_input() -> ExponentialAffineTransformInput {
    ExponentialAffineTransformInput::try_new(
        valid_equation_specification(),
        total_activity_loadings(ComplexScalar::new(0.2, -0.1), ComplexScalar::new(0.3, 0.05)),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(0.95, 0.02),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        0.0,
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.03, 0.05)
            .expect("news intensity components should construct"),
        NewsComponentMarkScaleConfiguration::neutral(),
        NewsComponentMarkScaleConfiguration::neutral(),
        1.0,
        standard_initial_state(),
        neutral_market_pressure_state(),
    )
    .expect("valid transform input should construct")
}

fn reference_transform_input_payload() -> Value {
    json!({
        "equation_specification": {
            "hawkes_kernel_parameters": {
                "lambda_bar": 0.8,
                "beta": 1.2,
                "alpha": {
                    "plus_plus": 0.20,
                    "plus_minus": 0.08,
                    "minus_plus": 0.10,
                    "minus_minus": 0.24
                },
                "rho": {
                    "plus_plus": 0.03,
                    "plus_minus": 0.02,
                    "minus_plus": 0.01,
                    "minus_minus": 0.04
                }
            },
            "gamma_severity_mark_configuration": {
                "shape_k": 1.8
            },
            "long_variance_parameters": {
                "mean_reversion_rate": 1.0,
                "vol_of_vol": 0.0,
                "stationary_return_energy_rate_per_hour": 0.0,
                "return_energy_loading": 0.0,
                "buy_event_loading": 0.0,
                "sell_event_loading": 0.0
            },
            "market_pressure_parameters": {
                "side_imbalance_levels": [0.0],
                "physical_measure": {
                    "transition_rates": {
                        "upward_transition_rates": [],
                        "downward_transition_rates": []
                    },
                    "anchor_drift_per_signed_balance": 0.0
                }
            }
        },
        "activity_loadings": {
            "u_plus_baseline": {
                "real": 0.2,
                "imaginary": -0.1
            },
            "u_plus_endogenous": {
                "real": 0.2,
                "imaginary": -0.1
            },
            "u_minus_baseline": {
                "real": 0.3,
                "imaginary": 0.05
            },
            "u_minus_endogenous": {
                "real": 0.3,
                "imaginary": 0.05
            }
        },
        "z_plus": {
            "real": 1.0,
            "imaginary": 0.0
        },
        "z_minus": {
            "real": 0.95,
            "imaginary": 0.02
        },
        "news_return_loading": {
            "real": 0.0,
            "imaginary": 0.0
        },
        "market_pressure_return_loading": {
            "real": 0.0,
            "imaginary": 0.0
        },
        "pressure_signed_imbalance_exposure_risk_price": 0.0,
        "seasonality_factor": 1.1,
        "news_intensity": {
            "known_news_intensity": 0.03,
            "surprise_news_intensity": 0.05
        },
        "known_news_component_mark_scales": {
            "activity_coupling_scale": 0.0,
            "return_jump_drift": 0.0,
            "return_jump_volatility": 0.0
        },
        "surprise_news_component_mark_scales": {
            "activity_coupling_scale": 0.0,
            "return_jump_drift": 0.0,
            "return_jump_volatility": 0.0
        },
        "delta": 1.0,
        "initial_state": {
            "y_plus_plus": 0.4,
            "y_plus_minus": 0.2,
            "y_minus_plus": 0.3,
            "y_minus_minus": 0.5
        },
        "initial_market_pressure_state": {
            "state_weights": [1.0]
        }
    })
}

fn valid_output() -> ExponentialAffineTransformOutput {
    ExponentialAffineTransformOutput::try_new(
        ComplexScalar::new(-0.03, 0.01),
        ExponentialAffineTransformDVector::try_new(
            ComplexScalar::new(0.10, -0.02),
            ComplexScalar::new(0.08, 0.01),
            ComplexScalar::new(0.07, -0.03),
            ComplexScalar::new(0.09, 0.00),
        )
        .expect("valid d_at_delta should construct"),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.98, -0.04),
    )
    .expect("valid output should construct")
}

fn linear_closed_form_fixture_input() -> ExponentialAffineTransformInput {
    ExponentialAffineTransformInput::try_new(
        ExponentialAffineTransformEquationSpecification::try_new(
            HawkesKernelParameters::try_new(
                0.8,
                1.2,
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
            )
            .expect("closed-form fixture kernel payload should construct"),
            GammaSeverityMarkConfiguration::try_new(1.8)
                .expect("closed-form fixture gamma payload should construct"),
            LongVarianceParameters::neutral(),
            neutral_market_pressure_parameters(),
        )
        .expect("closed-form fixture equation specification should construct"),
        total_activity_loadings(ComplexScalar::new(0.2, 0.0), ComplexScalar::new(0.3, 0.0)),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        0.0,
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.0, 0.0)
            .expect("closed-form fixture news-intensity payload should construct"),
        NewsComponentMarkScaleConfiguration::neutral(),
        NewsComponentMarkScaleConfiguration::neutral(),
        1.0,
        standard_initial_state(),
        neutral_market_pressure_state(),
    )
    .expect("closed-form fixture input should construct")
}

fn domain_violation_fixture_input() -> ExponentialAffineTransformInput {
    ExponentialAffineTransformInput::try_new(
        ExponentialAffineTransformEquationSpecification::try_new(
            HawkesKernelParameters::try_new(
                0.8,
                1.2,
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
                HawkesKernelLoadings {
                    plus_plus: 1.0,
                    plus_minus: 1.0,
                    minus_plus: 1.0,
                    minus_minus: 1.0,
                },
            )
            .expect("domain violation fixture kernel payload should construct"),
            GammaSeverityMarkConfiguration::try_new(1.8)
                .expect("domain violation fixture gamma payload should construct"),
            LongVarianceParameters::neutral(),
            neutral_market_pressure_parameters(),
        )
        .expect("domain violation fixture equation specification should construct"),
        total_activity_loadings(ComplexScalar::new(10.0, 0.0), ComplexScalar::new(10.0, 0.0)),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(2.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        0.0,
        2.0,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.0, 1.0)
            .expect("domain violation fixture news-intensity payload should construct"),
        NewsComponentMarkScaleConfiguration::neutral(),
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 1.0,
            return_jump_drift: 1.0,
            return_jump_volatility: 0.0,
        },
        0.5,
        standard_initial_state(),
        neutral_market_pressure_state(),
    )
    .expect("domain violation fixture input should construct")
}

fn blowup_fixture_input() -> ExponentialAffineTransformInput {
    ExponentialAffineTransformInput::try_new(
        ExponentialAffineTransformEquationSpecification::try_new(
            HawkesKernelParameters::try_new(
                0.8,
                1.2,
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.01,
                    plus_minus: 0.02,
                    minus_plus: 0.03,
                    minus_minus: 0.04,
                },
            )
            .expect("blowup fixture kernel payload should construct"),
            GammaSeverityMarkConfiguration::try_new(1.8)
                .expect("blowup fixture gamma payload should construct"),
            LongVarianceParameters::neutral(),
            neutral_market_pressure_parameters(),
        )
        .expect("blowup fixture equation specification should construct"),
        total_activity_loadings(ComplexScalar::new(0.0, 0.0), ComplexScalar::new(0.0, 0.0)),
        ComplexScalar::new(1.0e308, 0.0),
        ComplexScalar::new(1.0e308, 0.0),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        0.0,
        1.0,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.0, 0.0)
            .expect("blowup fixture news-intensity payload should construct"),
        NewsComponentMarkScaleConfiguration::neutral(),
        NewsComponentMarkScaleConfiguration::neutral(),
        2.0,
        standard_initial_state(),
        neutral_market_pressure_state(),
    )
    .expect("blowup fixture input should construct")
}

fn transform_value_overflow_fixture_input() -> ExponentialAffineTransformInput {
    let mut transform_input = linear_closed_form_fixture_input();

    transform_input.activity_loadings =
        total_activity_loadings(ComplexScalar::new(-0.2, 0.0), ComplexScalar::new(-0.3, 0.0));

    transform_input.initial_state = HawkesExcitationState {
        y_plus_plus: 1.0e308,
        y_plus_minus: 1.0e308,
        y_minus_plus: 1.0e308,
        y_minus_minus: 1.0e308,
    };

    transform_input
}

fn stiff_near_identity_fixture_input() -> ExponentialAffineTransformInput {
    ExponentialAffineTransformInput::try_new(
        ExponentialAffineTransformEquationSpecification::try_new(
            HawkesKernelParameters::try_new(
                100.0,
                700.0,
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
            )
            .expect("stiff fixture kernel payload should construct"),
            GammaSeverityMarkConfiguration::try_new(1.8)
                .expect("stiff fixture gamma payload should construct"),
            LongVarianceParameters::neutral(),
            neutral_market_pressure_parameters(),
        )
        .expect("stiff fixture equation specification should construct"),
        total_activity_loadings(
            ComplexScalar::new(-4.5e-10, 0.0),
            ComplexScalar::new(-4.5e-10, 0.0),
        ),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        0.0,
        0.36,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.0, 0.0)
            .expect("stiff fixture news-intensity payload should construct"),
        NewsComponentMarkScaleConfiguration::neutral(),
        NewsComponentMarkScaleConfiguration::neutral(),
        1.0,
        HawkesExcitationState {
            y_plus_plus: 991.0,
            y_plus_minus: 991.0,
            y_minus_plus: 991.0,
            y_minus_minus: 991.0,
        },
        neutral_market_pressure_state(),
    )
    .expect("stiff fixture input should construct")
}

fn exact_identity_query_input() -> ExponentialAffineTransformInput {
    ExponentialAffineTransformInput::try_new(
        valid_equation_specification(),
        total_activity_loadings(ComplexScalar::new(0.0, -0.0), ComplexScalar::new(0.0, 0.0)),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, -0.0),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        0.0,
        1.3,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.17, 0.29)
            .expect("identity-query news-intensity payload should construct"),
        NewsComponentMarkScaleConfiguration::neutral(),
        NewsComponentMarkScaleConfiguration::neutral(),
        0.5,
        standard_initial_state(),
        neutral_market_pressure_state(),
    )
    .expect("identity-query transform input should construct")
}

fn assert_complex_close(
    observed: ComplexScalar,
    expected: ComplexScalar,
    tolerance: f64,
    context: &str,
) {
    assert!(
        (observed.real - expected.real).abs() <= tolerance,
        "{context}: real mismatch, observed={} expected={}",
        observed.real,
        expected.real
    );
    assert!(
        (observed.imaginary - expected.imaginary).abs() <= tolerance,
        "{context}: imaginary mismatch, observed={} expected={}",
        observed.imaginary,
        expected.imaginary
    );
}

#[test]
fn exact_identity_query_short_circuits_to_exact_identity_output() {
    let input = exact_identity_query_input();

    let result = solve_exponential_affine_transform(input.clone())
        .expect("identity query should solve exactly");

    assert_eq!(
        result.status,
        ExponentialAffineTransformIntegrationStatus::Completed
    );
    assert_eq!(result.metadata.completed_step_count, 0);
    assert_eq!(result.metadata.rhs_evaluation_count, 0);
    assert_eq!(result.metadata.news_intensity, input.news_intensity);

    let output = result
        .output
        .expect("identity query should produce an exact output");

    assert_eq!(output.c_at_delta, ComplexScalar::new(0.0, 0.0));
    assert_eq!(
        output.d_at_delta,
        ExponentialAffineTransformDVector::new(
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
            ComplexScalar::new(0.0, 0.0),
        )
    );
    assert_eq!(output.transform_value, ComplexScalar::new(1.0, 0.0));
}

#[test]
fn transform_input_constructs_from_reference_payload() {
    let payload = reference_transform_input_payload();

    let parsed: ExponentialAffineTransformInput =
        serde_json::from_value(payload).expect("reference payload should deserialize");

    assert_eq!(parsed, valid_input());
}

#[test]
fn transform_models_round_trip_through_json() {
    let input = valid_input();
    let output = valid_output();

    let serialized_input = serde_json::to_string(&input).expect("input payload should serialize");

    assert!(
        serialized_input.contains("\"news_intensity\""),
        "serialized input should include component-aware news_intensity payload"
    );

    let parsed_input: ExponentialAffineTransformInput =
        serde_json::from_str(&serialized_input).expect("input payload should deserialize");

    assert_eq!(parsed_input, input);

    let serialized_output =
        serde_json::to_string(&output).expect("output payload should serialize");

    let parsed_output: ExponentialAffineTransformOutput =
        serde_json::from_str(&serialized_output).expect("output payload should deserialize");

    assert_eq!(parsed_output, output);
}

#[test]
fn transform_input_deserialization_reports_missing_required_field() {
    let mut payload = reference_transform_input_payload();

    payload
        .as_object_mut()
        .expect("reference payload should be a JSON object")
        .remove("delta");

    let parse_result = serde_json::from_value::<ExponentialAffineTransformInput>(payload);
    let error_text = parse_result
        .expect_err("missing delta field must fail")
        .to_string();

    assert!(
        error_text.contains("missing field `delta`"),
        "unexpected error message: {error_text}"
    );
}

#[test]
fn transform_input_deserialization_reports_malformed_field_type() {
    let mut payload = reference_transform_input_payload();
    payload["seasonality_factor"] = json!("high");

    let parse_result = serde_json::from_value::<ExponentialAffineTransformInput>(payload);

    let error_text = parse_result
        .expect_err("malformed seasonality_factor field type must fail")
        .to_string();

    assert!(
        error_text.contains("invalid type"),
        "unexpected error message: {error_text}"
    );
}

#[test]
fn transform_input_deserialization_reports_invalid_delta_value() {
    let mut payload = reference_transform_input_payload();
    payload["delta"] = json!(0.0);

    let parse_result = serde_json::from_value::<ExponentialAffineTransformInput>(payload);

    let error_text = parse_result
        .expect_err("invalid delta value must fail")
        .to_string();

    assert!(
        error_text.contains("delta must be finite and strictly positive"),
        "unexpected error message: {error_text}"
    );
}

#[test]
fn equation_mapping_policy_constant_is_stable() {
    assert_eq!(
        EXPONENTIAL_AFFINE_TRANSFORM_EQUATION_MAPPING_POLICY,
        "compact_model_spec_section_8_named_component_mapping.v1"
    );
    assert_eq!(
        EXPONENTIAL_AFFINE_TRANSFORM_DEFAULT_SOLVER_POLICY,
        "fourth_order_runge_kutta_fixed_step_with_hawkes_decay_stability_floor.v1"
    );
    assert_eq!(
        EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY,
        "enforce_d_dot_r_real_strict_upper_bound_at_runtime_checkpoints.v1"
    );
    assert_eq!(
        EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY,
        "terminate_on_non_finite_state_or_guard_metric_without_clamping.v1"
    );
    assert_eq!(
        EXPONENTIAL_AFFINE_TRANSFORM_NEWS_INTENSITY_USAGE_POLICY,
        "gamma_moment_uses_total_news_intensity_components_are_diagnostic_inputs.v1"
    );
    assert_eq!(
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION,
        "affine_oracle.integration_result.v1"
    );
    assert_eq!(
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION,
        "affine_oracle.integration_metadata.v1"
    );
    assert_eq!(
        EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION,
        "affine_oracle.integration_guard_failure.v1"
    );
    assert_eq!(
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION,
        "affine_oracle.integration_telemetry.v1"
    );
    assert_eq!(
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION,
        "affine_oracle.integration_output_schema_metadata.v1"
    );
}

#[test]
fn hamiltonians_match_hand_checked_complex_fixture() {
    let transform_input = valid_input();

    let d_vector = ExponentialAffineTransformDVector::new(
        ComplexScalar::new(0.10, -0.02),
        ComplexScalar::new(0.08, 0.01),
        ComplexScalar::new(0.07, -0.03),
        ComplexScalar::new(0.09, 0.00),
    );

    let hamiltonians = evaluate_exponential_affine_transform_hamiltonians(
        &transform_input,
        d_vector,
        ComplexScalar::new(0.0, 0.0),
    );

    assert_complex_close(
        hamiltonians.h_plus_endogenous,
        ComplexScalar::new(-0.17265736764489886, 0.09280848411170442),
        1.0e-12,
        "H_plus",
    );
    assert_complex_close(
        hamiltonians.h_minus_endogenous,
        ComplexScalar::new(-0.32304086676111327, -0.028650512256511856),
        1.0e-12,
        "H_minus",
    );
}

#[test]
fn ode_rhs_matches_hand_checked_real_fixture() {
    let transform_input = ExponentialAffineTransformInput::try_new(
        valid_equation_specification(),
        total_activity_loadings(ComplexScalar::new(0.2, 0.0), ComplexScalar::new(0.3, 0.0)),
        ComplexScalar::new(0.9, 0.0),
        ComplexScalar::new(1.1, 0.0),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        0.0,
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.03, 0.05)
            .expect("fixture news intensity should construct"),
        NewsComponentMarkScaleConfiguration::neutral(),
        NewsComponentMarkScaleConfiguration::neutral(),
        1.0,
        standard_initial_state(),
        neutral_market_pressure_state(),
    )
    .expect("fixture input should construct");

    let d_vector = ExponentialAffineTransformDVector::new(
        ComplexScalar::new(0.4, 0.0),
        ComplexScalar::new(0.2, 0.0),
        ComplexScalar::new(0.3, 0.0),
        ComplexScalar::new(0.1, 0.0),
    );

    let rhs = evaluate_exponential_affine_transform_ode_rhs(
        &transform_input,
        d_vector,
        ComplexScalar::new(0.0, 0.0),
    );

    assert_complex_close(
        rhs.d_derivative.plus_plus,
        ComplexScalar::new(-0.6753497365870158, 0.0),
        1.0e-12,
        "D_++'",
    );
    assert_complex_close(
        rhs.d_derivative.plus_minus,
        ComplexScalar::new(-0.4353497365870158, 0.0),
        1.0e-12,
        "D_+-'",
    );
    assert_complex_close(
        rhs.d_derivative.minus_plus,
        ComplexScalar::new(-0.5151081483883728, 0.0),
        1.0e-12,
        "D_-+'",
    );
    assert_complex_close(
        rhs.d_derivative.minus_minus,
        ComplexScalar::new(-0.275_108_148_388_372_9, 0.0),
        1.0e-12,
        "D_--'",
    );
    assert_complex_close(
        rhs.c_derivative,
        ComplexScalar::new(-0.30840293877834213, 0.0),
        1.0e-12,
        "C'",
    );
}

#[test]
fn ode_rhs_is_deterministic_for_identical_inputs() {
    let transform_input = valid_input();

    let d_vector = ExponentialAffineTransformDVector::new(
        ComplexScalar::new(0.14, -0.02),
        ComplexScalar::new(0.05, 0.04),
        ComplexScalar::new(0.09, -0.01),
        ComplexScalar::new(0.11, 0.03),
    );

    let first = evaluate_exponential_affine_transform_ode_rhs(
        &transform_input,
        d_vector,
        ComplexScalar::new(0.0, 0.0),
    );
    let second = evaluate_exponential_affine_transform_ode_rhs(
        &transform_input,
        d_vector,
        ComplexScalar::new(0.0, 0.0),
    );

    assert_eq!(first, second);
}

#[test]
fn default_solver_applies_hawkes_decay_stability_floor_for_stiff_input() {
    let result = solve_exponential_affine_transform(stiff_near_identity_fixture_input())
        .expect("stiff near-identity query should solve with default settings");

    assert_eq!(
        result.status,
        ExponentialAffineTransformIntegrationStatus::Completed
    );
    assert!(result.output.is_some());
    assert!(
        result.metadata.configured_step_count
            > ExponentialAffineTransformSolverSettings::default().step_count
    );
    assert_eq!(
        result.metadata.completed_step_count,
        result.metadata.configured_step_count
    );
}

#[test]
fn solver_settings_validation_rejects_zero_step_count() {
    let settings_error = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        0,
    )
    .expect_err("zero step_count must be rejected during settings construction");

    assert!(
        settings_error
            .to_string()
            .contains("step_count must be strictly positive"),
        "unexpected settings error: {settings_error}"
    );

    let solver_error = solve_exponential_affine_transform_with_settings(
        valid_input(),
        ExponentialAffineTransformSolverSettings {
            integrator_method: ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
            step_count: 0,
            telemetry_mode: ExponentialAffineTransformTelemetryMode::Disabled,
        },
    )
    .expect_err("solver execution must reject invalid settings before runtime");

    assert!(
        matches!(
            solver_error,
            ExponentialAffineTransformSolverError::InvalidSolverSettings { .. }
        ),
        "unexpected solver error variant: {solver_error}"
    );
}

#[test]
fn solver_integrator_selection_wiring_uses_requested_method() {
    let transform_input = linear_closed_form_fixture_input();

    let euler_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::ForwardEuler,
        1,
    )
    .expect("valid Euler settings should construct");

    let runge_kutta_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        1,
    )
    .expect("valid Runge-Kutta settings should construct");

    let euler_result =
        solve_exponential_affine_transform_with_settings(transform_input.clone(), euler_settings)
            .expect("Euler integration should succeed");

    let runge_kutta_result =
        solve_exponential_affine_transform_with_settings(transform_input, runge_kutta_settings)
            .expect("Runge-Kutta integration should succeed");

    assert_eq!(
        euler_result.metadata.integrator_method,
        ExponentialAffineTransformIntegratorMethod::ForwardEuler
    );
    assert_eq!(
        euler_result.metadata.rhs_evaluation_count,
        euler_result.metadata.configured_step_count
    );
    assert_eq!(euler_result.metadata.guard_failure, None);
    assert_eq!(
        runge_kutta_result.metadata.integrator_method,
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta
    );
    assert_eq!(
        runge_kutta_result.metadata.rhs_evaluation_count,
        runge_kutta_result
            .metadata
            .configured_step_count
            .saturating_mul(4)
    );
    assert_eq!(runge_kutta_result.metadata.guard_failure, None);
}

#[test]
fn default_solver_executes_deterministic_closed_form_fixture() {
    let transform_input = linear_closed_form_fixture_input();

    let solve_result = solve_exponential_affine_transform(transform_input.clone())
        .expect("default solver should succeed");

    let output = solve_result
        .output
        .expect("default solver should emit output when status is completed");

    assert_eq!(
        solve_result.status,
        ExponentialAffineTransformIntegrationStatus::Completed
    );
    assert_eq!(
        solve_result.metadata.integrator_method,
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta
    );
    assert_eq!(
        solve_result.metadata.completed_step_count,
        solve_result.metadata.configured_step_count
    );
    assert!(
        solve_result.metadata.step_size > 0.0 && solve_result.metadata.step_size.is_finite(),
        "default solver step size must be finite and positive"
    );
    assert_eq!(solve_result.metadata.guard_failure, None);
    assert_eq!(
        solve_result.metadata.domain_guard_policy,
        EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY
    );
    assert_eq!(
        solve_result.metadata.blowup_termination_policy,
        EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY
    );
    assert!(solve_result.telemetry.is_none());

    let decay = (-transform_input
        .equation_specification
        .hawkes_kernel_parameters
        .beta
        * transform_input.delta)
        .exp();

    let h_plus = -transform_input.activity_loadings.u_plus_endogenous.real;
    let h_minus = -transform_input.activity_loadings.u_minus_endogenous.real;

    let expected_d_plus = h_plus * (1.0 - decay)
        / transform_input
            .equation_specification
            .hawkes_kernel_parameters
            .beta;

    let expected_d_minus = h_minus * (1.0 - decay)
        / transform_input
            .equation_specification
            .hawkes_kernel_parameters
            .beta;

    let expected_c = transform_input.seasonality_factor
        * transform_input
            .equation_specification
            .hawkes_kernel_parameters
            .lambda_bar
        * (h_plus + h_minus)
        * transform_input.delta;

    let expected_exponent = expected_c
        + expected_d_plus
            * (transform_input.initial_state.y_plus_plus
                + transform_input.initial_state.y_plus_minus)
        + expected_d_minus
            * (transform_input.initial_state.y_minus_plus
                + transform_input.initial_state.y_minus_minus);

    assert_complex_close(
        output.c_at_delta,
        ComplexScalar::new(expected_c, 0.0),
        1.0e-9,
        "C(delta)",
    );
    assert_complex_close(
        output.d_at_delta.plus_plus,
        ComplexScalar::new(expected_d_plus, 0.0),
        1.0e-9,
        "D_++(delta)",
    );
    assert_complex_close(
        output.d_at_delta.plus_minus,
        ComplexScalar::new(expected_d_plus, 0.0),
        1.0e-9,
        "D_+-(delta)",
    );
    assert_complex_close(
        output.d_at_delta.minus_plus,
        ComplexScalar::new(expected_d_minus, 0.0),
        1.0e-9,
        "D_-+(delta)",
    );
    assert_complex_close(
        output.d_at_delta.minus_minus,
        ComplexScalar::new(expected_d_minus, 0.0),
        1.0e-9,
        "D_--(delta)",
    );
    assert_complex_close(
        output.transform_value,
        ComplexScalar::new(expected_exponent.exp(), 0.0),
        1.0e-9,
        "F",
    );
}

#[test]
fn input_sensitivity_tracks_event_count_marking_variables() {
    let sensitivities =
        solve_exponential_affine_transform_input_sensitivities(linear_closed_form_fixture_input())
            .expect("linear transform sensitivity solve should succeed");

    assert_complex_close(
        sensitivities.z_plus_sensitivity.c_at_delta_derivative,
        sensitivities
            .u_plus_total_activity_sensitivity()
            .c_at_delta_derivative
            .scale(-1.0),
        1.0e-12,
        "z_plus C sensitivity",
    );
    assert_complex_close(
        sensitivities
            .z_plus_sensitivity
            .d_at_delta_derivative
            .plus_plus,
        sensitivities
            .u_plus_total_activity_sensitivity()
            .d_at_delta_derivative
            .plus_plus
            .scale(-1.0),
        1.0e-12,
        "z_plus D_++ sensitivity",
    );
    assert_complex_close(
        sensitivities.z_plus_sensitivity.transform_value_derivative,
        sensitivities
            .u_plus_total_activity_sensitivity()
            .transform_value_derivative
            .scale(-1.0),
        1.0e-12,
        "z_plus transform sensitivity",
    );
    assert_complex_close(
        sensitivities.z_minus_sensitivity.c_at_delta_derivative,
        sensitivities
            .u_minus_total_activity_sensitivity()
            .c_at_delta_derivative
            .scale(-1.0),
        1.0e-12,
        "z_minus C sensitivity",
    );
    assert_complex_close(
        sensitivities
            .z_minus_sensitivity
            .d_at_delta_derivative
            .minus_minus,
        sensitivities
            .u_minus_total_activity_sensitivity()
            .d_at_delta_derivative
            .minus_minus
            .scale(-1.0),
        1.0e-12,
        "z_minus D_-- sensitivity",
    );
    assert_complex_close(
        sensitivities.z_minus_sensitivity.transform_value_derivative,
        sensitivities
            .u_minus_total_activity_sensitivity()
            .transform_value_derivative
            .scale(-1.0),
        1.0e-12,
        "z_minus transform sensitivity",
    );
}

#[test]
fn solver_domain_guard_violation_terminates_with_deterministic_metadata() {
    let transform_input = domain_violation_fixture_input();

    let solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::ForwardEuler,
        2,
    )
    .expect("domain-violation settings should construct");

    let solve_result =
        solve_exponential_affine_transform_with_settings(transform_input, solver_settings)
            .expect("domain-violation run should return guarded termination result");

    assert_eq!(
        solve_result.status,
        ExponentialAffineTransformIntegrationStatus::DomainGuardViolation
    );
    assert!(solve_result.output.is_none());

    let guard_failure = solve_result
        .metadata
        .guard_failure
        .expect("guarded run should include guard failure metadata");

    assert_eq!(
        guard_failure.code,
        ExponentialAffineTransformGuardFailureCode::DomainUpperBoundViolation
    );
    assert_eq!(
        guard_failure.schema_version,
        EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION
    );
    assert_eq!(guard_failure.step_index, 0);
    assert!(
        guard_failure.time.abs() <= 1.0e-15,
        "unexpected guard failure time: {}",
        guard_failure.time
    );
    assert!(
        guard_failure.d_dot_r.real > guard_failure.d_dot_r_upper_bound,
        "domain guard trigger should require d_dot_r.real > upper bound"
    );
    assert_eq!(
        solve_result.metadata.completed_step_count,
        guard_failure.step_index
    );
    assert_eq!(solve_result.metadata.rhs_evaluation_count, 0);
}

#[test]
fn solver_blowup_detection_terminates_without_output() {
    let transform_input = blowup_fixture_input();

    let solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::ForwardEuler,
        1,
    )
    .expect("blowup settings should construct");

    let solve_result =
        solve_exponential_affine_transform_with_settings(transform_input, solver_settings)
            .expect("blowup run should return guarded termination result");

    assert_eq!(
        solve_result.status,
        ExponentialAffineTransformIntegrationStatus::BlowupTerminated
    );
    assert!(solve_result.output.is_none());

    let guard_failure = solve_result
        .metadata
        .guard_failure
        .expect("blowup run should include guard failure metadata");

    assert_eq!(
        guard_failure.code,
        ExponentialAffineTransformGuardFailureCode::NonFiniteState
    );
    assert_eq!(
        guard_failure.schema_version,
        EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION
    );
    assert_eq!(guard_failure.step_index, 1);
    assert!(
        guard_failure.d_dot_r.real.is_nan() && guard_failure.d_dot_r.imaginary.is_nan(),
        "non-finite-state guard marker should use NaN d_dot_r sentinel"
    );
    assert_eq!(solve_result.metadata.rhs_evaluation_count, 1);
}

#[test]
fn solver_transform_value_overflow_terminates_without_output() {
    let transform_input = transform_value_overflow_fixture_input();

    let solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        8,
    )
    .expect("overflow settings should construct");

    let solve_result =
        solve_exponential_affine_transform_with_settings(transform_input, solver_settings)
            .expect("overflow run should return guarded termination result");

    assert_eq!(
        solve_result.status,
        ExponentialAffineTransformIntegrationStatus::BlowupTerminated
    );
    assert!(solve_result.output.is_none());

    let guard_failure = solve_result
        .metadata
        .guard_failure
        .expect("overflow run should include guard failure metadata");

    assert_eq!(
        guard_failure.code,
        ExponentialAffineTransformGuardFailureCode::TransformValueOverflow
    );
    assert_eq!(
        guard_failure.schema_version,
        EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION
    );
    assert_eq!(guard_failure.step_index, 8);
    assert!(guard_failure.d_dot_r.is_finite());
    assert_eq!(solve_result.metadata.rhs_evaluation_count, 32);
}

#[test]
fn telemetry_toggle_preserves_solver_semantics() {
    let transform_input = linear_closed_form_fixture_input();

    let telemetry_disabled_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        8,
    )
    .expect("telemetry-disabled settings should construct");

    let telemetry_enabled_settings = telemetry_disabled_settings
        .with_telemetry_mode(ExponentialAffineTransformTelemetryMode::Enabled);

    let telemetry_disabled_result = solve_exponential_affine_transform_with_settings(
        transform_input.clone(),
        telemetry_disabled_settings,
    )
    .expect("telemetry-disabled solve should succeed");

    let telemetry_enabled_result = solve_exponential_affine_transform_with_settings(
        transform_input,
        telemetry_enabled_settings,
    )
    .expect("telemetry-enabled solve should succeed");

    assert_eq!(
        telemetry_disabled_result.status,
        telemetry_enabled_result.status
    );
    assert_eq!(
        telemetry_disabled_result.output,
        telemetry_enabled_result.output
    );
    assert_eq!(
        telemetry_disabled_result.metadata.status_flags,
        telemetry_enabled_result.metadata.status_flags
    );
    assert_eq!(
        telemetry_disabled_result.metadata.telemetry_mode,
        ExponentialAffineTransformTelemetryMode::Disabled
    );
    assert_eq!(
        telemetry_enabled_result.metadata.telemetry_mode,
        ExponentialAffineTransformTelemetryMode::Enabled
    );
    assert!(telemetry_disabled_result.telemetry.is_none());

    let telemetry = telemetry_enabled_result
        .telemetry
        .expect("telemetry-enabled solve should emit telemetry payload");

    assert!(!telemetry.guard_triggered);
    assert_eq!(telemetry.guard_trigger, None);
    assert_eq!(telemetry.checkpoint_count, 9);
}

#[test]
fn diagnostics_payload_schema_contains_required_fields() {
    let solve_result = solve_exponential_affine_transform(linear_closed_form_fixture_input())
        .expect("solve should succeed");

    let serialized_payload =
        serde_json::to_value(solve_result).expect("solver payload should serialize");

    let serialized_payload_object = serialized_payload
        .as_object()
        .expect("serialized payload should be an object");

    for required_result_field in EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS {
        assert!(
            serialized_payload_object.contains_key(required_result_field),
            "missing required result field '{required_result_field}'"
        );
    }

    assert_eq!(
        serialized_payload["schema_version"],
        Value::String(EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION.to_string())
    );

    let output_schema_metadata = &serialized_payload["output_schema_metadata"];

    assert_eq!(
        output_schema_metadata["schema_version"],
        Value::String(
            EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION.to_string()
        )
    );
    assert_eq!(
        output_schema_metadata["required_result_fields"],
        serde_json::to_value(EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS)
            .expect("required result fields should serialize")
    );
    assert_eq!(
        output_schema_metadata["required_metadata_fields"],
        serde_json::to_value(EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS)
            .expect("required metadata fields should serialize")
    );
    assert_eq!(
        output_schema_metadata["required_guard_failure_fields"],
        serde_json::to_value(
            EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS
        )
        .expect("required guard failure fields should serialize")
    );
    assert_eq!(
        output_schema_metadata["required_telemetry_fields"],
        serde_json::to_value(EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_TELEMETRY_FIELDS)
            .expect("required telemetry fields should serialize")
    );

    let metadata = &serialized_payload["metadata"];
    let metadata_object = metadata
        .as_object()
        .expect("serialized metadata should be an object");

    for required_metadata_field in EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS
    {
        assert!(
            metadata_object.contains_key(required_metadata_field),
            "missing required metadata field '{required_metadata_field}'"
        );
    }

    assert_eq!(
        metadata["schema_version"],
        Value::String(EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION.to_string())
    );
}

#[test]
fn telemetry_payload_reports_guarded_failure_context() {
    let transform_input = domain_violation_fixture_input();

    let solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::ForwardEuler,
        2,
    )
    .expect("guarded-failure telemetry settings should construct")
    .with_telemetry_mode(ExponentialAffineTransformTelemetryMode::Enabled);

    let solve_result =
        solve_exponential_affine_transform_with_settings(transform_input.clone(), solver_settings)
            .expect("guarded-failure solve should return deterministic result");

    assert_eq!(
        solve_result.status,
        ExponentialAffineTransformIntegrationStatus::DomainGuardViolation
    );

    let guard_failure = solve_result
        .metadata
        .guard_failure
        .expect("guarded-failure solve should include guard metadata");

    let telemetry = solve_result
        .telemetry
        .expect("guarded-failure solve should emit telemetry when enabled");

    assert_eq!(
        telemetry.schema_version,
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION
    );
    assert_eq!(telemetry.news_intensity, transform_input.news_intensity);
    assert!(telemetry.guard_triggered);
    assert_eq!(telemetry.guard_trigger, Some(guard_failure));
    assert!(
        telemetry.checkpoint_count >= 1,
        "telemetry must include the guarded initial checkpoint"
    );
    assert!(
        telemetry
            .min_domain_margin
            .expect("domain-violation telemetry must include a domain margin")
            <= 0.0
    );

    let serialized_guard_failure =
        serde_json::to_value(guard_failure).expect("guard failure payload should serialize");

    for required_guard_failure_field in
        EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS
    {
        assert_ne!(
            serialized_guard_failure[required_guard_failure_field],
            Value::Null,
            "guard failure field '{required_guard_failure_field}' must be present"
        );
    }
}
