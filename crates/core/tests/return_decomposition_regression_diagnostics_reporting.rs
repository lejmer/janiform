mod support;

use janiform_core::nig::{
    ComplexScalar, NigBuySellDriverParameters, NigSharedPlusShiftConfiguration,
};
use janiform_core::pricing::{
    compose_nig_trading_time_exponent_with_edge_mode,
    compose_total_return_characteristic_function_with_edge_mode,
};
use janiform_core::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionInput,
};
use serde_json::Value;
use support::return_decomposition_failure_diagnostics::{
    RETURN_DECOMPOSITION_FAILURE_DIAGNOSTIC_REPORT_SCHEMA_VERSION,
    ReturnDecompositionDiagnosticComplexScalar, ReturnDecompositionDiagnosticContributions,
    ReturnDecompositionFailureClassificationHints, ReturnDecompositionFailureDiagnosticCategory,
    ReturnDecompositionFailureDiagnosticReport, ReturnDecompositionScenarioDiagnosticContext,
    build_return_decomposition_failure_diagnostic_report,
    build_return_decomposition_failure_diagnostic_violation,
    render_return_decomposition_failure_diagnostic_report,
};
use support::return_decomposition_sign_fixture::{
    ReturnDecompositionSignAnalyticFixtureCase, ReturnDecompositionSignDriverConfiguration,
    load_return_decomposition_sign_analytic_fixture_document,
};

const RETURN_DECOMPOSITION_DIAGNOSTIC_SUITE_ID: &str =
    "return_decomposition_regression_diagnostics_reporting";
const DIAGNOSTIC_ABSOLUTE_TOLERANCE: f64 = 1.0e-12;
const COMPONENT_CHECK_IDS: [&str; 5] = [
    "buy_side_weighted_exponent",
    "sell_side_weighted_exponent",
    "combined_weighted_exponent",
    "trading_time_characteristic_factor",
    "combined_characteristic_factor",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegressionObservedOutputMode {
    Canonical,
    InjectSignRegression,
    InjectMappingRegression,
    InjectComponentDisableModeRegression,
}

#[derive(Debug, Clone)]
struct DecompositionRegressionScenario {
    context: ReturnDecompositionScenarioDiagnosticContext,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    closed_gap_characteristic_factor: ComplexScalar,
    expected_contributions: ReturnDecompositionDiagnosticContributions,
    wrong_positive_u_sell_side_weighted_exponent: ComplexScalar,
}

#[test]
fn decomposition_regression_suite_covers_component_and_total_outputs() {
    let (driver_parameters, scenarios) = load_regression_scenarios();

    let mut violations = Vec::new();
    for scenario in &scenarios {
        violations.extend(collect_scenario_violations(
            driver_parameters,
            scenario,
            RegressionObservedOutputMode::Canonical,
            DIAGNOSTIC_ABSOLUTE_TOLERANCE,
        ));
    }

    let report = build_return_decomposition_failure_diagnostic_report(
        RETURN_DECOMPOSITION_DIAGNOSTIC_SUITE_ID,
        scenarios.len(),
        violations,
    );
    assert_eq!(
        report.schema_version,
        RETURN_DECOMPOSITION_FAILURE_DIAGNOSTIC_REPORT_SCHEMA_VERSION
    );
    assert_eq!(report.summary.scenario_count, scenarios.len());

    if report.summary.failure_count != 0 {
        panic!(
            "{}",
            render_return_decomposition_failure_diagnostic_report(&report)
        );
    }
}

#[test]
fn diagnostics_schema_includes_required_failure_metadata_fields() {
    let (driver_parameters, scenarios) = load_regression_scenarios();
    let scenario = scenarios
        .iter()
        .find(|scenario| scenario.context.scenario_intent == "sell_only")
        .expect("fixture should include one sell_only scenario");

    let injected_violations = collect_scenario_violations(
        driver_parameters,
        scenario,
        RegressionObservedOutputMode::InjectSignRegression,
        DIAGNOSTIC_ABSOLUTE_TOLERANCE,
    );
    assert!(!injected_violations.is_empty());

    let report = build_return_decomposition_failure_diagnostic_report(
        RETURN_DECOMPOSITION_DIAGNOSTIC_SUITE_ID,
        1,
        injected_violations,
    );
    let serialized_report =
        serde_json::to_value(&report).expect("diagnostic report should serialize");
    assert_eq!(
        serialized_report["schema_version"],
        Value::String(RETURN_DECOMPOSITION_FAILURE_DIAGNOSTIC_REPORT_SCHEMA_VERSION.to_string())
    );
    assert_eq!(
        serialized_report["suite_id"],
        Value::String(RETURN_DECOMPOSITION_DIAGNOSTIC_SUITE_ID.to_string())
    );
    assert_eq!(
        serialized_report["violations"][0]["scenario_id"],
        Value::String(scenario.context.scenario_id.clone())
    );
    assert_eq!(
        serialized_report["violations"][0]["seed"],
        scenario.context.seed
    );

    let mut missing_seed_payload = serialized_report.clone();
    missing_seed_payload["violations"][0]
        .as_object_mut()
        .expect("violations entry should be an object")
        .remove("seed");
    assert!(
        serde_json::from_value::<ReturnDecompositionFailureDiagnosticReport>(missing_seed_payload)
            .is_err()
    );

    let mut missing_scenario_id_payload = serialized_report;
    missing_scenario_id_payload["violations"][0]
        .as_object_mut()
        .expect("violations entry should be an object")
        .remove("scenario_id");
    assert!(
        serde_json::from_value::<ReturnDecompositionFailureDiagnosticReport>(
            missing_scenario_id_payload
        )
        .is_err()
    );
}

#[test]
fn diagnostics_classification_detects_sign_mapping_and_component_disable_mode_regressions() {
    let (driver_parameters, scenarios) = load_regression_scenarios();

    let sign_scenario = scenarios
        .iter()
        .find(|scenario| scenario.context.scenario_intent == "sell_only")
        .expect("fixture should include one sell_only scenario");
    let sign_violation = collect_scenario_violations(
        driver_parameters,
        sign_scenario,
        RegressionObservedOutputMode::InjectSignRegression,
        DIAGNOSTIC_ABSOLUTE_TOLERANCE,
    )
    .into_iter()
    .find(|violation| violation.check_id == "sell_side_weighted_exponent")
    .expect("injected sign regression should produce sell-side mismatch");
    assert_eq!(
        sign_violation.category,
        ReturnDecompositionFailureDiagnosticCategory::Sign
    );

    let mapping_scenario = scenarios
        .iter()
        .find(|scenario| scenario.context.scenario_intent == "combined")
        .expect("fixture should include one combined scenario");
    let mapping_violation = collect_scenario_violations(
        driver_parameters,
        mapping_scenario,
        RegressionObservedOutputMode::InjectMappingRegression,
        DIAGNOSTIC_ABSOLUTE_TOLERANCE,
    )
    .into_iter()
    .find(|violation| violation.check_id == "buy_side_weighted_exponent")
    .expect("injected mapping regression should produce buy-side mismatch");
    assert_eq!(
        mapping_violation.category,
        ReturnDecompositionFailureDiagnosticCategory::Mapping
    );

    let mut component_disable_mode_scenario = scenarios
        .iter()
        .find(|scenario| scenario.context.scenario_intent == "combined")
        .expect("fixture should include one combined scenario")
        .clone();
    component_disable_mode_scenario.context.scenario_id =
        "component_disable_mode_probe_from_combined_case".to_string();
    component_disable_mode_scenario.context.scenario_intent = "combined_probe_buy_only".to_string();
    component_disable_mode_scenario.context.edge_mode = ReturnDecompositionEdgeMode::BuyOnly;
    component_disable_mode_scenario.expected_contributions = compose_observed_contributions(
        driver_parameters,
        &component_disable_mode_scenario,
        RegressionObservedOutputMode::Canonical,
    );
    let component_disable_mode_violation = collect_scenario_violations(
        driver_parameters,
        &component_disable_mode_scenario,
        RegressionObservedOutputMode::InjectComponentDisableModeRegression,
        DIAGNOSTIC_ABSOLUTE_TOLERANCE,
    )
    .into_iter()
    .find(|violation| violation.check_id == "sell_side_weighted_exponent")
    .expect("edge-mode leakage regression should produce sell-side mismatch");
    assert_eq!(
        component_disable_mode_violation.category,
        ReturnDecompositionFailureDiagnosticCategory::ComponentDisableMode
    );
}

fn load_regression_scenarios() -> (
    NigBuySellDriverParameters,
    Vec<DecompositionRegressionScenario>,
) {
    let fixture_document = load_return_decomposition_sign_analytic_fixture_document()
        .expect("return decomposition sign analytic fixture should load");
    let driver_parameters = derive_driver_parameters(fixture_document.driver_configuration);

    let scenarios: Vec<DecompositionRegressionScenario> = fixture_document
        .cases
        .iter()
        .enumerate()
        .map(|(case_position, fixture_case)| {
            let edge_mode = edge_mode_for_scenario_intent(&fixture_case.scenario_intent);
            let seed = 941_000_u64
                + u64::try_from(case_position).expect("fixture case index should fit u64");
            DecompositionRegressionScenario {
                context: ReturnDecompositionScenarioDiagnosticContext {
                    scenario_id: fixture_case.fixture_id.clone(),
                    scenario_intent: fixture_case.scenario_intent.clone(),
                    seed,
                    edge_mode,
                },
                argument: fixture_case.argument.into_core(),
                return_decomposition: ReturnDecomposition::try_new(ReturnDecompositionInput {
                    a_plus: fixture_case.return_decomposition.a_plus,
                    a_minus: fixture_case.return_decomposition.a_minus,
                    open_jump: 0.0,
                    closed_gap: fixture_case.return_decomposition.gap,
                })
                .expect("fixture decomposition payload should construct"),
                closed_gap_characteristic_factor: fixture_case
                    .gap_characteristic_factor
                    .into_core(),
                expected_contributions: expected_contributions_from_fixture_case(fixture_case),
                wrong_positive_u_sell_side_weighted_exponent: fixture_case
                    .expected_output
                    .wrong_positive_u_sell_side_weighted_exponent
                    .into_core(),
            }
        })
        .collect();

    (driver_parameters, scenarios)
}

fn collect_scenario_violations(
    driver_parameters: NigBuySellDriverParameters,
    scenario: &DecompositionRegressionScenario,
    observed_output_mode: RegressionObservedOutputMode,
    tolerance: f64,
) -> Vec<support::return_decomposition_failure_diagnostics::ReturnDecompositionFailureDiagnosticViolation>
{
    let observed_contributions =
        compose_observed_contributions(driver_parameters, scenario, observed_output_mode);
    let mut violations = Vec::new();

    let classification_hints = ReturnDecompositionFailureClassificationHints {
        wrong_positive_u_sell_side_weighted_exponent: scenario
            .wrong_positive_u_sell_side_weighted_exponent,
    };
    for check_id in COMPONENT_CHECK_IDS {
        let violation = build_return_decomposition_failure_diagnostic_violation(
            &scenario.context,
            check_id,
            scenario.expected_contributions,
            observed_contributions,
            classification_hints,
            tolerance,
        );
        if violation.absolute_distance > tolerance {
            violations.push(violation);
        }
    }

    violations
}

fn compose_observed_contributions(
    driver_parameters: NigBuySellDriverParameters,
    scenario: &DecompositionRegressionScenario,
    observed_output_mode: RegressionObservedOutputMode,
) -> ReturnDecompositionDiagnosticContributions {
    match observed_output_mode {
        RegressionObservedOutputMode::Canonical => {
            let exponent_contributions = compose_nig_trading_time_exponent_with_edge_mode(
                driver_parameters,
                scenario.argument,
                scenario.return_decomposition,
                scenario.context.edge_mode,
            );
            let characteristic_function_assembly =
                compose_total_return_characteristic_function_with_edge_mode(
                    driver_parameters,
                    scenario.argument,
                    scenario.return_decomposition,
                    ComplexScalar::new(1.0, 0.0),
                    scenario.closed_gap_characteristic_factor,
                    scenario.context.edge_mode,
                )
                .expect("canonical decomposition assembly should succeed");

            ReturnDecompositionDiagnosticContributions {
                buy_side_weighted_exponent: exponent_contributions
                    .buy_side_weighted_exponent
                    .into(),
                sell_side_weighted_exponent: exponent_contributions
                    .sell_side_weighted_exponent
                    .into(),
                combined_weighted_exponent: exponent_contributions
                    .combined_weighted_exponent
                    .into(),
                trading_time_characteristic_factor: characteristic_function_assembly
                    .trading_time_characteristic_factor
                    .into(),
                combined_characteristic_factor: characteristic_function_assembly
                    .combined_characteristic_factor
                    .into(),
            }
        }
        RegressionObservedOutputMode::InjectSignRegression => {
            let effective_decomposition = scenario
                .return_decomposition
                .with_edge_mode(scenario.context.edge_mode);
            let buy_side_weighted_exponent = driver_parameters
                .buy_side_characteristic_exponent(scenario.argument)
                .scale(effective_decomposition.a_plus);
            let sell_side_weighted_exponent = driver_parameters
                .sell_side_parameters
                .characteristic_exponent(scenario.argument)
                .scale(effective_decomposition.a_minus);
            let combined_weighted_exponent =
                buy_side_weighted_exponent.add(sell_side_weighted_exponent);
            let trading_time_characteristic_factor =
                complex_exponential(combined_weighted_exponent);
            let effective_closed_gap_characteristic_factor =
                effective_closed_gap_characteristic_factor(
                    scenario.closed_gap_characteristic_factor,
                    scenario.context.edge_mode,
                );
            let combined_characteristic_factor = complex_multiply(
                trading_time_characteristic_factor,
                effective_closed_gap_characteristic_factor,
            );

            ReturnDecompositionDiagnosticContributions {
                buy_side_weighted_exponent: buy_side_weighted_exponent.into(),
                sell_side_weighted_exponent: sell_side_weighted_exponent.into(),
                combined_weighted_exponent: combined_weighted_exponent.into(),
                trading_time_characteristic_factor: trading_time_characteristic_factor.into(),
                combined_characteristic_factor: combined_characteristic_factor.into(),
            }
        }
        RegressionObservedOutputMode::InjectMappingRegression => {
            let effective_decomposition = scenario
                .return_decomposition
                .with_edge_mode(scenario.context.edge_mode);
            let buy_side_weighted_exponent = driver_parameters
                .buy_side_characteristic_exponent(scenario.argument)
                .scale(effective_decomposition.a_minus);
            let sell_side_weighted_exponent = driver_parameters
                .sell_side_characteristic_exponent_for_return_composition(scenario.argument)
                .scale(effective_decomposition.a_plus);
            let combined_weighted_exponent =
                buy_side_weighted_exponent.add(sell_side_weighted_exponent);
            let trading_time_characteristic_factor =
                complex_exponential(combined_weighted_exponent);
            let effective_closed_gap_characteristic_factor =
                effective_closed_gap_characteristic_factor(
                    scenario.closed_gap_characteristic_factor,
                    scenario.context.edge_mode,
                );
            let combined_characteristic_factor = complex_multiply(
                trading_time_characteristic_factor,
                effective_closed_gap_characteristic_factor,
            );

            ReturnDecompositionDiagnosticContributions {
                buy_side_weighted_exponent: buy_side_weighted_exponent.into(),
                sell_side_weighted_exponent: sell_side_weighted_exponent.into(),
                combined_weighted_exponent: combined_weighted_exponent.into(),
                trading_time_characteristic_factor: trading_time_characteristic_factor.into(),
                combined_characteristic_factor: combined_characteristic_factor.into(),
            }
        }
        RegressionObservedOutputMode::InjectComponentDisableModeRegression => {
            let exponent_contributions = compose_nig_trading_time_exponent_with_edge_mode(
                driver_parameters,
                scenario.argument,
                scenario.return_decomposition,
                ReturnDecompositionEdgeMode::Disabled,
            );
            let characteristic_function_assembly =
                compose_total_return_characteristic_function_with_edge_mode(
                    driver_parameters,
                    scenario.argument,
                    scenario.return_decomposition,
                    ComplexScalar::new(1.0, 0.0),
                    scenario.closed_gap_characteristic_factor,
                    ReturnDecompositionEdgeMode::Disabled,
                )
                .expect("disable-mode regression injection assembly should succeed");

            ReturnDecompositionDiagnosticContributions {
                buy_side_weighted_exponent: exponent_contributions
                    .buy_side_weighted_exponent
                    .into(),
                sell_side_weighted_exponent: exponent_contributions
                    .sell_side_weighted_exponent
                    .into(),
                combined_weighted_exponent: exponent_contributions
                    .combined_weighted_exponent
                    .into(),
                trading_time_characteristic_factor: characteristic_function_assembly
                    .trading_time_characteristic_factor
                    .into(),
                combined_characteristic_factor: characteristic_function_assembly
                    .combined_characteristic_factor
                    .into(),
            }
        }
    }
}

fn expected_contributions_from_fixture_case(
    fixture_case: &ReturnDecompositionSignAnalyticFixtureCase,
) -> ReturnDecompositionDiagnosticContributions {
    ReturnDecompositionDiagnosticContributions {
        buy_side_weighted_exponent: ReturnDecompositionDiagnosticComplexScalar {
            real: fixture_case.expected_output.buy_side_weighted_exponent.real,
            imaginary: fixture_case
                .expected_output
                .buy_side_weighted_exponent
                .imaginary,
        },
        sell_side_weighted_exponent: ReturnDecompositionDiagnosticComplexScalar {
            real: fixture_case
                .expected_output
                .sell_side_weighted_exponent
                .real,
            imaginary: fixture_case
                .expected_output
                .sell_side_weighted_exponent
                .imaginary,
        },
        combined_weighted_exponent: ReturnDecompositionDiagnosticComplexScalar {
            real: fixture_case.expected_output.combined_weighted_exponent.real,
            imaginary: fixture_case
                .expected_output
                .combined_weighted_exponent
                .imaginary,
        },
        trading_time_characteristic_factor: ReturnDecompositionDiagnosticComplexScalar {
            real: fixture_case
                .expected_output
                .trading_time_characteristic_factor
                .real,
            imaginary: fixture_case
                .expected_output
                .trading_time_characteristic_factor
                .imaginary,
        },
        combined_characteristic_factor: ReturnDecompositionDiagnosticComplexScalar {
            real: fixture_case
                .expected_output
                .combined_characteristic_factor
                .real,
            imaginary: fixture_case
                .expected_output
                .combined_characteristic_factor
                .imaginary,
        },
    }
}

fn derive_driver_parameters(
    driver_configuration: ReturnDecompositionSignDriverConfiguration,
) -> NigBuySellDriverParameters {
    NigSharedPlusShiftConfiguration::try_new(
        driver_configuration.alpha,
        driver_configuration.beta_0,
        driver_configuration.delta,
        driver_configuration.mu,
        driver_configuration.delta_beta,
    )
    .expect("fixture driver configuration should construct")
    .derive_driver_parameters()
    .expect("fixture driver configuration should derive drivers")
}

fn edge_mode_for_scenario_intent(scenario_intent: &str) -> ReturnDecompositionEdgeMode {
    match scenario_intent {
        "buy_only" => ReturnDecompositionEdgeMode::BuyOnly,
        "sell_only" => ReturnDecompositionEdgeMode::SellOnly,
        _ => ReturnDecompositionEdgeMode::Disabled,
    }
}

fn effective_closed_gap_characteristic_factor(
    closed_gap_characteristic_factor: ComplexScalar,
    edge_mode: ReturnDecompositionEdgeMode,
) -> ComplexScalar {
    if edge_mode.closed_gap_component_enabled() {
        closed_gap_characteristic_factor
    } else {
        ComplexScalar::new(1.0, 0.0)
    }
}

fn complex_exponential(value: ComplexScalar) -> ComplexScalar {
    let scaling_factor = value.real.exp();
    ComplexScalar::new(
        scaling_factor * value.imaginary.cos(),
        scaling_factor * value.imaginary.sin(),
    )
}

fn complex_multiply(left: ComplexScalar, right: ComplexScalar) -> ComplexScalar {
    ComplexScalar::new(
        left.real
            .mul_add(right.real, -(left.imaginary * right.imaginary)),
        left.real
            .mul_add(right.imaginary, left.imaginary * right.real),
    )
}
