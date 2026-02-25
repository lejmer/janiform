use janiform_core::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformInput,
    ExponentialAffineTransformIntegrationStatus, ExponentialAffineTransformIntegratorMethod,
    ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_optional_settings,
};
use janiform_core::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
use janiform_core::news::GammaSeverityMarkConfiguration;
use janiform_core::nig::ComplexScalar;
use janiform_core::pricing::{
    solve_exponential_affine_transform_for_pricing,
    solve_exponential_affine_transform_for_pricing_with_settings,
};
use janiform_core::risk::{
    solve_exponential_affine_transform_for_risk_sampling,
    solve_exponential_affine_transform_for_risk_sampling_with_settings,
};

#[test]
fn solver_api_contract_is_compatible_across_pricing_and_risk_consumers() {
    let transform_input = build_transform_input(1.0);

    let pricing_result = solve_exponential_affine_transform_for_pricing(transform_input)
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

    let pricing_result = solve_exponential_affine_transform_for_pricing_with_settings(
        transform_input,
        solver_settings,
    )
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
        0.08,
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
