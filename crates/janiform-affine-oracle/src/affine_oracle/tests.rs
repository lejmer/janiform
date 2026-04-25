use serde_json::{Value, json};

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
    EXPONENTIAL_AFFINE_TRANSFORM_NEWS_INTENSITY_USAGE_POLICY, ExponentialAffineTransformDVector,
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformGuardFailureCode,
    ExponentialAffineTransformInput, ExponentialAffineTransformIntegrationStatus,
    ExponentialAffineTransformIntegratorMethod, ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformOutput, ExponentialAffineTransformSolverError,
    ExponentialAffineTransformSolverSettings, ExponentialAffineTransformTelemetryMode,
    evaluate_exponential_affine_transform_hamiltonians,
    evaluate_exponential_affine_transform_ode_rhs, solve_exponential_affine_transform,
    solve_exponential_affine_transform_with_settings,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::news::GammaSeverityMarkConfiguration;

fn valid_equation_specification() -> ExponentialAffineTransformEquationSpecification {
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
        .expect("valid kernel payload should construct"),
        GammaSeverityMarkConfiguration::try_new(1.8, 0.9)
            .expect("valid gamma configuration should construct"),
    )
    .expect("valid equation specification should construct")
}

fn valid_input() -> ExponentialAffineTransformInput {
    ExponentialAffineTransformInput::try_new(
        valid_equation_specification(),
        ComplexScalar::new(0.2, -0.1),
        ComplexScalar::new(0.3, 0.05),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(0.95, 0.02),
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.03, 0.05)
            .expect("news intensity components should construct"),
        1.0,
        HawkesExcitationState {
            y_plus_plus: 0.4,
            y_plus_minus: 0.2,
            y_minus_plus: 0.3,
            y_minus_minus: 0.5,
        },
    )
    .expect("valid transform input should construct")
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
            GammaSeverityMarkConfiguration::try_new(1.8, 0.9)
                .expect("closed-form fixture gamma payload should construct"),
        )
        .expect("closed-form fixture equation specification should construct"),
        ComplexScalar::new(0.2, 0.0),
        ComplexScalar::new(0.3, 0.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.0, 0.0)
            .expect("closed-form fixture news-intensity payload should construct"),
        1.0,
        HawkesExcitationState {
            y_plus_plus: 0.4,
            y_plus_minus: 0.2,
            y_minus_plus: 0.3,
            y_minus_minus: 0.5,
        },
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
            GammaSeverityMarkConfiguration::try_new(1.8, 0.9)
                .expect("domain violation fixture gamma payload should construct"),
        )
        .expect("domain violation fixture equation specification should construct"),
        ComplexScalar::new(-10.0, 0.0),
        ComplexScalar::new(-10.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        2.0,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.0, 0.0)
            .expect("domain violation fixture news-intensity payload should construct"),
        0.5,
        HawkesExcitationState {
            y_plus_plus: 0.4,
            y_plus_minus: 0.2,
            y_minus_plus: 0.3,
            y_minus_minus: 0.5,
        },
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
            GammaSeverityMarkConfiguration::try_new(1.8, 0.9)
                .expect("blowup fixture gamma payload should construct"),
        )
        .expect("blowup fixture equation specification should construct"),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(1.0e308, 0.0),
        ComplexScalar::new(1.0e308, 0.0),
        1.0,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.0, 0.0)
            .expect("blowup fixture news-intensity payload should construct"),
        2.0,
        HawkesExcitationState {
            y_plus_plus: 0.4,
            y_plus_minus: 0.2,
            y_minus_plus: 0.3,
            y_minus_minus: 0.5,
        },
    )
    .expect("blowup fixture input should construct")
}

fn transform_value_overflow_fixture_input() -> ExponentialAffineTransformInput {
    let mut transform_input = linear_closed_form_fixture_input();
    transform_input.u_plus = ComplexScalar::new(-0.2, 0.0);
    transform_input.u_minus = ComplexScalar::new(-0.3, 0.0);
    transform_input.initial_state = HawkesExcitationState {
        y_plus_plus: 1.0e308,
        y_plus_minus: 1.0e308,
        y_minus_plus: 1.0e308,
        y_minus_minus: 1.0e308,
    };
    transform_input
}

fn exact_identity_query_input() -> ExponentialAffineTransformInput {
    ExponentialAffineTransformInput::try_new(
        valid_equation_specification(),
        ComplexScalar::new(0.0, -0.0),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, -0.0),
        1.3,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.17, 0.29)
            .expect("identity-query news-intensity payload should construct"),
        0.5,
        HawkesExcitationState {
            y_plus_plus: 0.4,
            y_plus_minus: 0.2,
            y_minus_plus: 0.3,
            y_minus_minus: 0.5,
        },
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

    let result =
        solve_exponential_affine_transform(input).expect("identity query should solve exactly");

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
    let payload = json!({
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
                "shape_k": 1.8,
                "scale_theta": 0.9
            }
        },
        "u_plus": {
            "real": 0.2,
            "imaginary": -0.1
        },
        "u_minus": {
            "real": 0.3,
            "imaginary": 0.05
        },
        "z_plus": {
            "real": 1.0,
            "imaginary": 0.0
        },
        "z_minus": {
            "real": 0.95,
            "imaginary": 0.02
        },
        "s_h": 1.1,
        "news_intensity": {
            "known_nu_h": 0.03,
            "surprise_nu_h": 0.05
        },
        "delta": 1.0,
        "initial_state": {
            "y_plus_plus": 0.4,
            "y_plus_minus": 0.2,
            "y_minus_plus": 0.3,
            "y_minus_minus": 0.5
        }
    });

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
    let payload = json!({
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
                "shape_k": 1.8,
                "scale_theta": 0.9
            }
        },
        "u_plus": {
            "real": 0.2,
            "imaginary": -0.1
        },
        "u_minus": {
            "real": 0.3,
            "imaginary": 0.05
        },
        "z_plus": {
            "real": 1.0,
            "imaginary": 0.0
        },
        "z_minus": {
            "real": 0.95,
            "imaginary": 0.02
        },
        "s_h": 1.1,
        "news_intensity": {
            "known_nu_h": 0.03,
            "surprise_nu_h": 0.05
        },
        "initial_state": {
            "y_plus_plus": 0.4,
            "y_plus_minus": 0.2,
            "y_minus_plus": 0.3,
            "y_minus_minus": 0.5
        }
    });

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
    let payload = json!({
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
                "shape_k": 1.8,
                "scale_theta": 0.9
            }
        },
        "u_plus": {
            "real": 0.2,
            "imaginary": -0.1
        },
        "u_minus": {
            "real": 0.3,
            "imaginary": 0.05
        },
        "z_plus": {
            "real": 1.0,
            "imaginary": 0.0
        },
        "z_minus": {
            "real": 0.95,
            "imaginary": 0.02
        },
        "s_h": "high",
        "news_intensity": {
            "known_nu_h": 0.03,
            "surprise_nu_h": 0.05
        },
        "delta": 1.0,
        "initial_state": {
            "y_plus_plus": 0.4,
            "y_plus_minus": 0.2,
            "y_minus_plus": 0.3,
            "y_minus_minus": 0.5
        }
    });

    let parse_result = serde_json::from_value::<ExponentialAffineTransformInput>(payload);
    let error_text = parse_result
        .expect_err("malformed s_h field type must fail")
        .to_string();
    assert!(
        error_text.contains("invalid type"),
        "unexpected error message: {error_text}"
    );
}

#[test]
fn transform_input_deserialization_reports_invalid_delta_value() {
    let payload = json!({
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
                "shape_k": 1.8,
                "scale_theta": 0.9
            }
        },
        "u_plus": {
            "real": 0.2,
            "imaginary": -0.1
        },
        "u_minus": {
            "real": 0.3,
            "imaginary": 0.05
        },
        "z_plus": {
            "real": 1.0,
            "imaginary": 0.0
        },
        "z_minus": {
            "real": 0.95,
            "imaginary": 0.02
        },
        "s_h": 1.1,
        "news_intensity": {
            "known_nu_h": 0.03,
            "surprise_nu_h": 0.05
        },
        "delta": 0.0,
        "initial_state": {
            "y_plus_plus": 0.4,
            "y_plus_minus": 0.2,
            "y_minus_plus": 0.3,
            "y_minus_minus": 0.5
        }
    });

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
        "fourth_order_runge_kutta_fixed_step.v1"
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

    let hamiltonians =
        evaluate_exponential_affine_transform_hamiltonians(transform_input, d_vector);

    assert_complex_close(
        hamiltonians.h_plus,
        ComplexScalar::new(-0.17265736764489886, 0.09280848411170442),
        1.0e-12,
        "H_plus",
    );
    assert_complex_close(
        hamiltonians.h_minus,
        ComplexScalar::new(-0.32304086676111327, -0.028650512256511856),
        1.0e-12,
        "H_minus",
    );
}

#[test]
fn ode_rhs_matches_hand_checked_real_fixture() {
    let transform_input = ExponentialAffineTransformInput::try_new(
        valid_equation_specification(),
        ComplexScalar::new(0.2, 0.0),
        ComplexScalar::new(0.3, 0.0),
        ComplexScalar::new(0.9, 0.0),
        ComplexScalar::new(1.1, 0.0),
        1.1,
        ExponentialAffineTransformNewsIntensityInput::try_new(0.03, 0.05)
            .expect("fixture news intensity should construct"),
        1.0,
        HawkesExcitationState {
            y_plus_plus: 0.4,
            y_plus_minus: 0.2,
            y_minus_plus: 0.3,
            y_minus_minus: 0.5,
        },
    )
    .expect("fixture input should construct");
    let d_vector = ExponentialAffineTransformDVector::new(
        ComplexScalar::new(0.4, 0.0),
        ComplexScalar::new(0.2, 0.0),
        ComplexScalar::new(0.3, 0.0),
        ComplexScalar::new(0.1, 0.0),
    );

    let rhs = evaluate_exponential_affine_transform_ode_rhs(transform_input, d_vector);

    assert_complex_close(
        rhs.d_derivative.plus_plus,
        ComplexScalar::new(-0.6948847102457174, 0.0),
        1.0e-12,
        "D_++'",
    );
    assert_complex_close(
        rhs.d_derivative.plus_minus,
        ComplexScalar::new(-0.45488471024571747, 0.0),
        1.0e-12,
        "D_+-'",
    );
    assert_complex_close(
        rhs.d_derivative.minus_plus,
        ComplexScalar::new(-0.5306189632272101, 0.0),
        1.0e-12,
        "D_-+'",
    );
    assert_complex_close(
        rhs.d_derivative.minus_minus,
        ComplexScalar::new(-0.29061896322721015, 0.0),
        1.0e-12,
        "D_--'",
    );
    assert_complex_close(
        rhs.c_derivative,
        ComplexScalar::new(-0.3053334325713209, 0.0),
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

    let first = evaluate_exponential_affine_transform_ode_rhs(transform_input, d_vector);
    let second = evaluate_exponential_affine_transform_ode_rhs(transform_input, d_vector);

    assert_eq!(first, second);
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
        solve_exponential_affine_transform_with_settings(transform_input, euler_settings)
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
    let solve_result =
        solve_exponential_affine_transform(transform_input).expect("default solver should succeed");
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
    let h_plus = -transform_input.u_plus.real;
    let h_minus = -transform_input.u_minus.real;

    let expected_d_plus = transform_input.s_h * h_plus * (1.0 - decay)
        / transform_input
            .equation_specification
            .hawkes_kernel_parameters
            .beta;
    let expected_d_minus = transform_input.s_h * h_minus * (1.0 - decay)
        / transform_input
            .equation_specification
            .hawkes_kernel_parameters
            .beta;
    let expected_c = transform_input.s_h
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
    assert_eq!(guard_failure.step_index, 1);
    assert!(
        (guard_failure.time - 0.25).abs() <= 1.0e-15,
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
    assert_eq!(solve_result.metadata.rhs_evaluation_count, 1);
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
        transform_input,
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
        solve_exponential_affine_transform_with_settings(transform_input, solver_settings)
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
        telemetry.checkpoint_count >= 2,
        "telemetry must include at least initial and first-step checkpoints"
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
