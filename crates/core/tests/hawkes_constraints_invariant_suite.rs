mod support;

use janiform_core::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
    HawkesTransitionStep,
};
use support::hawkes_failure_diagnostics::{
    HawkesFailureDiagnosticViolation, build_hawkes_failure_diagnostic_report,
    classify_constraint_or_transition_violation_category, render_hawkes_failure_diagnostic_report,
};
use support::hawkes_invariant_catalog::{
    HawkesInvariantCheckClass, assert_hawkes_invariant_catalog_ids_have_check_class,
};

const NUMERIC_TOLERANCE: f64 = 1e-12;

#[derive(Debug, Clone, PartialEq)]
struct InvariantViolationDiagnostic {
    violation_kind: &'static str,
    scenario_id: &'static str,
    detail: String,
}

#[derive(Debug, Clone, Copy)]
struct ProjectionScenario {
    scenario_id: &'static str,
    state: HawkesExcitationState,
    lambda_bar: f64,
    seasonality_factor: f64,
}

#[derive(Debug, Clone, Copy)]
struct StabilityScenario {
    scenario_id: &'static str,
    beta: f64,
    alpha: HawkesKernelLoadings,
    rho: HawkesKernelLoadings,
    maximum_distance_from_threshold: Option<f64>,
}

#[derive(Debug, Clone)]
struct NonNegativeTransitionScenario {
    scenario_id: &'static str,
    initial_state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    transition_steps: Vec<HawkesTransitionStep>,
}

#[test]
fn constraint_projection_sanity_preserves_positive_intensity_domains() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-CON-001"],
        HawkesInvariantCheckClass::DeterministicEquation,
    );

    let scenarios = [
        ProjectionScenario {
            scenario_id: "projection_interior_positive",
            state: HawkesExcitationState {
                y_plus_plus: 0.45,
                y_plus_minus: 0.15,
                y_minus_plus: 0.30,
                y_minus_minus: 0.25,
            },
            lambda_bar: 0.75,
            seasonality_factor: 1.20,
        },
        ProjectionScenario {
            scenario_id: "projection_boundary_adjacent_positive",
            state: HawkesExcitationState {
                y_plus_plus: 0.000_000_2,
                y_plus_minus: 0.000_000_3,
                y_minus_plus: 0.000_000_1,
                y_minus_minus: 0.000_000_4,
            },
            lambda_bar: 0.000_000_5,
            seasonality_factor: 0.20,
        },
        ProjectionScenario {
            scenario_id: "projection_high_seasonality_scaling",
            state: HawkesExcitationState {
                y_plus_plus: 0.80,
                y_plus_minus: 0.60,
                y_minus_plus: 0.40,
                y_minus_minus: 1.00,
            },
            lambda_bar: 0.50,
            seasonality_factor: 3.75,
        },
    ];

    let mut violations: Vec<InvariantViolationDiagnostic> = Vec::new();

    for scenario in scenarios {
        let projection = scenario
            .state
            .project_intensities(scenario.lambda_bar, scenario.seasonality_factor);

        let expected_deseasonalized_plus =
            scenario.lambda_bar + scenario.state.y_plus_plus + scenario.state.y_plus_minus;
        let expected_deseasonalized_minus =
            scenario.lambda_bar + scenario.state.y_minus_plus + scenario.state.y_minus_minus;

        let expected_scaled_plus = expected_deseasonalized_plus * scenario.seasonality_factor;
        let expected_scaled_minus = expected_deseasonalized_minus * scenario.seasonality_factor;

        push_if_difference_exceeds_tolerance(
            &mut violations,
            "projection",
            scenario.scenario_id,
            "deseasonalized.lambda_plus",
            projection.deseasonalized.lambda_plus,
            expected_deseasonalized_plus,
            NUMERIC_TOLERANCE,
        );
        push_if_difference_exceeds_tolerance(
            &mut violations,
            "projection",
            scenario.scenario_id,
            "deseasonalized.lambda_minus",
            projection.deseasonalized.lambda_minus,
            expected_deseasonalized_minus,
            NUMERIC_TOLERANCE,
        );
        push_if_difference_exceeds_tolerance(
            &mut violations,
            "projection",
            scenario.scenario_id,
            "seasonality_scaled.lambda_plus",
            projection.seasonality_scaled.lambda_plus,
            expected_scaled_plus,
            NUMERIC_TOLERANCE,
        );
        push_if_difference_exceeds_tolerance(
            &mut violations,
            "projection",
            scenario.scenario_id,
            "seasonality_scaled.lambda_minus",
            projection.seasonality_scaled.lambda_minus,
            expected_scaled_minus,
            NUMERIC_TOLERANCE,
        );

        for (component_name, component_value) in [
            (
                "deseasonalized.lambda_plus",
                projection.deseasonalized.lambda_plus,
            ),
            (
                "deseasonalized.lambda_minus",
                projection.deseasonalized.lambda_minus,
            ),
            (
                "seasonality_scaled.lambda_plus",
                projection.seasonality_scaled.lambda_plus,
            ),
            (
                "seasonality_scaled.lambda_minus",
                projection.seasonality_scaled.lambda_minus,
            ),
        ] {
            if !component_value.is_finite() || component_value <= 0.0 {
                violations.push(InvariantViolationDiagnostic {
                    violation_kind: "projection",
                    scenario_id: scenario.scenario_id,
                    detail: format!(
                        "{component_name} must be finite and strictly positive for valid projection inputs, found {component_value}"
                    ),
                });
            }
        }
    }

    assert!(
        violations.is_empty(),
        "projection invariants failed\n{}",
        render_violation_diagnostics(
            "constraint_projection_sanity_preserves_positive_intensity_domains",
            &["HK-CON-001"],
            &violations,
        )
    );
}

#[test]
fn constraint_stability_boundary_regimes_remain_constructible_and_stable() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-CON-001"],
        HawkesInvariantCheckClass::DeterministicEquation,
    );

    let scenarios = [
        StabilityScenario {
            scenario_id: "stability_interior_regime",
            beta: 1.0,
            alpha: HawkesKernelLoadings {
                plus_plus: 0.58,
                plus_minus: 0.05,
                minus_plus: 0.04,
                minus_minus: 0.62,
            },
            rho: HawkesKernelLoadings {
                plus_plus: 0.02,
                plus_minus: 0.01,
                minus_plus: 0.03,
                minus_minus: 0.02,
            },
            maximum_distance_from_threshold: None,
        },
        StabilityScenario {
            scenario_id: "stability_near_threshold_diagonal",
            beta: 1.0,
            alpha: HawkesKernelLoadings {
                plus_plus: 0.999_999_999_5,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.41,
            },
            rho: HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.02,
                minus_plus: 0.02,
                minus_minus: 0.0,
            },
            maximum_distance_from_threshold: Some(1e-9),
        },
    ];

    let mut violations: Vec<InvariantViolationDiagnostic> = Vec::new();

    for scenario in scenarios {
        match HawkesKernelParameters::check_spectral_radius_stability(
            scenario.beta,
            scenario.alpha,
            0.0,
        ) {
            Ok(stability_check) => {
                if !stability_check.is_stable {
                    violations.push(InvariantViolationDiagnostic {
                        violation_kind: "parameter",
                        scenario_id: scenario.scenario_id,
                        detail: format!(
                            "expected valid regime with spectral radius strictly below threshold, observed spectral_radius={} and effective_threshold={}",
                            stability_check.spectral_radius_alpha_over_beta,
                            stability_check.effective_stationarity_threshold
                        ),
                    });
                }

                let threshold_distance = stability_check.effective_stationarity_threshold
                    - stability_check.spectral_radius_alpha_over_beta;

                if let Some(maximum_distance_from_threshold) =
                    scenario.maximum_distance_from_threshold
                {
                    if threshold_distance > maximum_distance_from_threshold {
                        violations.push(InvariantViolationDiagnostic {
                            violation_kind: "parameter",
                            scenario_id: scenario.scenario_id,
                            detail: format!(
                                "expected near-threshold regime but observed threshold distance {} exceeded maximum {}",
                                threshold_distance, maximum_distance_from_threshold
                            ),
                        });
                    }
                } else if threshold_distance <= 0.05 {
                    violations.push(InvariantViolationDiagnostic {
                        violation_kind: "parameter",
                        scenario_id: scenario.scenario_id,
                        detail: format!(
                            "interior stability regime should keep margin above 0.05, found threshold distance {threshold_distance}"
                        ),
                    });
                }
            }
            Err(source) => {
                violations.push(InvariantViolationDiagnostic {
                    violation_kind: "parameter",
                    scenario_id: scenario.scenario_id,
                    detail: format!("stability checker rejected valid payload: {source}"),
                });
            }
        }

        let domain_report = HawkesKernelParameters::validate_domain(
            0.65,
            scenario.beta,
            scenario.alpha,
            scenario.rho,
        );
        if !domain_report.is_valid() {
            violations.push(InvariantViolationDiagnostic {
                violation_kind: "parameter",
                scenario_id: scenario.scenario_id,
                detail: format!(
                    "domain report unexpectedly failed for valid regime: violations={:?}",
                    domain_report.violations
                ),
            });
        }

        if let Err(source) =
            HawkesKernelParameters::try_new(0.65, scenario.beta, scenario.alpha, scenario.rho)
        {
            violations.push(InvariantViolationDiagnostic {
                violation_kind: "parameter",
                scenario_id: scenario.scenario_id,
                detail: format!(
                    "kernel parameter construction unexpectedly failed for valid regime: {source}"
                ),
            });
        }
    }

    assert!(
        violations.is_empty(),
        "stability invariants failed\n{}",
        render_violation_diagnostics(
            "constraint_stability_boundary_regimes_remain_constructible_and_stable",
            &["HK-CON-001"],
            &violations,
        )
    );
}

#[test]
fn constraint_transition_steps_preserve_non_negative_state_increments() {
    assert_hawkes_invariant_catalog_ids_have_check_class(
        &["HK-CON-001"],
        HawkesInvariantCheckClass::DeterministicEquation,
    );

    let scenarios = vec![
        NonNegativeTransitionScenario {
            scenario_id: "non_grid_mixed_events",
            initial_state: HawkesExcitationState {
                y_plus_plus: 0.40,
                y_plus_minus: 0.30,
                y_minus_plus: 0.25,
                y_minus_minus: 0.45,
            },
            kernel_parameters: kernel_fixture_one(),
            transition_steps: vec![
                HawkesTransitionStep::new(0.000_000_4, Some(HawkesEventSide::Buy), None),
                HawkesTransitionStep::new(0.17, None, Some(0.8)),
                HawkesTransitionStep::new(0.031_25, Some(HawkesEventSide::Sell), Some(0.4)),
                HawkesTransitionStep::new(0.913, None, None),
                HawkesTransitionStep::new(0.000_000_2, Some(HawkesEventSide::Buy), Some(0.1)),
            ],
        },
        NonNegativeTransitionScenario {
            scenario_id: "boundary_adjacent_with_news_loading",
            initial_state: HawkesExcitationState {
                y_plus_plus: 0.15,
                y_plus_minus: 0.12,
                y_minus_plus: 0.20,
                y_minus_minus: 0.18,
            },
            kernel_parameters: kernel_fixture_two(),
            transition_steps: vec![
                HawkesTransitionStep::new(0.245, Some(HawkesEventSide::Sell), None),
                HawkesTransitionStep::new(0.000_000_6, None, Some(0.35)),
                HawkesTransitionStep::new(0.333_3, Some(HawkesEventSide::Buy), Some(0.7)),
                HawkesTransitionStep::new(0.041_7, None, None),
                HawkesTransitionStep::new(0.777_777, Some(HawkesEventSide::Sell), Some(0.2)),
            ],
        },
    ];

    let mut violations: Vec<InvariantViolationDiagnostic> = Vec::new();

    for scenario in scenarios {
        let has_boundary_adjacent_gap = scenario
            .transition_steps
            .iter()
            .any(|transition_step| transition_step.delta_t > 0.0 && transition_step.delta_t < 1e-6);
        if !has_boundary_adjacent_gap {
            violations.push(InvariantViolationDiagnostic {
                violation_kind: "transition",
                scenario_id: scenario.scenario_id,
                detail: "missing boundary-adjacent event-time gap smaller than 1e-6".to_string(),
            });
        }

        let has_non_grid_gap = scenario.transition_steps.iter().any(|transition_step| {
            transition_step.delta_t > 0.0
                && ((transition_step.delta_t * 4.0) - (transition_step.delta_t * 4.0).round()).abs()
                    > NUMERIC_TOLERANCE
        });
        if !has_non_grid_gap {
            violations.push(InvariantViolationDiagnostic {
                violation_kind: "transition",
                scenario_id: scenario.scenario_id,
                detail: "missing non-grid event-time gap not aligned to quarter-step grid"
                    .to_string(),
            });
        }

        let mut current_state = scenario.initial_state;
        for (step_number, transition_step) in scenario.transition_steps.iter().copied().enumerate()
        {
            let drifted_state = current_state
                .apply_shared_decay_drift(scenario.kernel_parameters.beta, transition_step.delta_t);
            let next_state = match current_state
                .apply_transition_step(scenario.kernel_parameters, transition_step)
            {
                Ok(next_state) => next_state,
                Err(source) => {
                    violations.push(InvariantViolationDiagnostic {
                        violation_kind: "transition",
                        scenario_id: scenario.scenario_id,
                        detail: format!(
                            "transition application failed at step {}: {}",
                            step_number + 1,
                            source
                        ),
                    });
                    continue;
                }
            };

            for ((component_name, drifted_value), (_, next_value)) in
                state_components(drifted_state)
                    .into_iter()
                    .zip(state_components(next_state))
            {
                let increment = next_value - drifted_value;
                if increment < -NUMERIC_TOLERANCE {
                    violations.push(InvariantViolationDiagnostic {
                        violation_kind: "transition",
                        scenario_id: scenario.scenario_id,
                        detail: format!(
                            "negative increment introduced at step {} for {}: increment={}",
                            step_number + 1,
                            component_name,
                            increment
                        ),
                    });
                }
            }

            for (component_name, component_value) in state_components(next_state) {
                if component_value < -NUMERIC_TOLERANCE {
                    violations.push(InvariantViolationDiagnostic {
                        violation_kind: "transition",
                        scenario_id: scenario.scenario_id,
                        detail: format!(
                            "state component became negative at step {} for {}: value={}",
                            step_number + 1,
                            component_name,
                            component_value
                        ),
                    });
                }
            }

            current_state = next_state;
        }
    }

    assert!(
        violations.is_empty(),
        "transition invariants failed\n{}",
        render_violation_diagnostics(
            "constraint_transition_steps_preserve_non_negative_state_increments",
            &["HK-CON-001"],
            &violations,
        )
    );
}

fn kernel_fixture_one() -> HawkesKernelParameters {
    HawkesKernelParameters::try_new(
        0.70,
        1.10,
        HawkesKernelLoadings {
            plus_plus: 0.55,
            plus_minus: 0.08,
            minus_plus: 0.12,
            minus_minus: 0.60,
        },
        HawkesKernelLoadings {
            plus_plus: 0.03,
            plus_minus: 0.01,
            minus_plus: 0.02,
            minus_minus: 0.04,
        },
    )
    .expect("kernel fixture one should remain in the valid parameter regime")
}

fn kernel_fixture_two() -> HawkesKernelParameters {
    HawkesKernelParameters::try_new(
        0.65,
        1.25,
        HawkesKernelLoadings {
            plus_plus: 0.48,
            plus_minus: 0.06,
            minus_plus: 0.09,
            minus_minus: 0.52,
        },
        HawkesKernelLoadings {
            plus_plus: 0.02,
            plus_minus: 0.03,
            minus_plus: 0.01,
            minus_minus: 0.04,
        },
    )
    .expect("kernel fixture two should remain in the valid parameter regime")
}

fn push_if_difference_exceeds_tolerance(
    violations: &mut Vec<InvariantViolationDiagnostic>,
    violation_kind: &'static str,
    scenario_id: &'static str,
    component_name: &'static str,
    observed_value: f64,
    expected_value: f64,
    tolerance: f64,
) {
    let absolute_difference = (observed_value - expected_value).abs();
    if absolute_difference > tolerance {
        violations.push(InvariantViolationDiagnostic {
            violation_kind,
            scenario_id,
            detail: format!(
                "projection mismatch for {component_name}: expected={expected_value}, observed={observed_value}, absolute_difference={absolute_difference}, tolerance={tolerance}"
            ),
        });
    }
}

fn state_components(state: HawkesExcitationState) -> [(&'static str, f64); 4] {
    [
        ("y_plus_plus", state.y_plus_plus),
        ("y_plus_minus", state.y_plus_minus),
        ("y_minus_plus", state.y_minus_plus),
        ("y_minus_minus", state.y_minus_minus),
    ]
}

fn render_violation_diagnostics(
    test_name: &str,
    invariant_ids: &[&str],
    violations: &[InvariantViolationDiagnostic],
) -> String {
    let mapped_violations: Vec<HawkesFailureDiagnosticViolation> = violations
        .iter()
        .map(|violation| HawkesFailureDiagnosticViolation {
            category: classify_constraint_or_transition_violation_category(
                violation.violation_kind,
            ),
            scope: format!(
                "violation_kind={} scenario_id={}",
                violation.violation_kind, violation.scenario_id
            ),
            detail: violation.detail.clone(),
            observed_value: None,
            reference_lower_bound: None,
            reference_upper_bound: None,
            delta_relative_to_reference: None,
        })
        .collect();

    let report = build_hawkes_failure_diagnostic_report(
        "hawkes_constraints_invariant_suite",
        test_name,
        HawkesInvariantCheckClass::DeterministicEquation,
        invariant_ids,
        mapped_violations,
    );

    render_hawkes_failure_diagnostic_report(&report)
}
