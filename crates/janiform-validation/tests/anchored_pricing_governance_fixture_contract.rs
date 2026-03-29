mod support;

use std::collections::BTreeSet;
use std::fs;

use serde::Deserialize;

use support::validation_fixture::fixtures_root_path;

const ANCHORED_PRICING_REFERENCE_POLICY_SCHEMA_VERSION: &str =
    "benchmark.anchored_pricing_reference.policy.v1";
const ANCHORED_PRICING_REFERENCE_BASELINE_SCHEMA_VERSION: &str =
    "benchmark.anchored_pricing_reference.baseline.v1";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchoredPricingReferencePolicy {
    schema_version: String,
    policy_id: String,
    policy_version: String,
    governance_scope_policy: String,
    simulation_reference: AnchoredPricingSimulationReferencePolicy,
    diagnostic_requirements: AnchoredPricingDiagnosticRequirements,
    scenario_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchoredPricingSimulationReferencePolicy {
    simulation_engine: String,
    discounted_payoff_estimator: String,
    path_count: usize,
    confidence_multiple: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchoredPricingDiagnosticRequirements {
    observed_state_boundary_input_required: bool,
    anchor_alignment_required: bool,
    local_martingale_diagnostics_required: bool,
    deviation_refinement_diagnostics_required: bool,
    anchored_engine_numeric_quality_required: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchoredPricingReferenceBaselineArtifact {
    schema_version: String,
    baseline_id: String,
    baseline_version: String,
    source_policy_id: String,
    source_policy_version: String,
    reference_construction_policy: String,
    regression_tolerances: AnchoredPricingReferenceRegressionTolerances,
    cases: Vec<AnchoredPricingReferenceBaselineCase>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchoredPricingReferenceRegressionTolerances {
    native_price_absolute_tolerance: f64,
    native_minus_simulation_gap_absolute_tolerance: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "outcome_kind", rename_all = "snake_case", deny_unknown_fields)]
enum AnchoredPricingReferenceBaselineCase {
    Success {
        scenario_id: String,
        anchor_model_kind: String,
        kappa: f64,
        observed_boundary_state: AnchoredPricingObservedBoundaryState,
        current_anchor_value: f64,
        next_anchor_value: f64,
        observed_return_adjustment: f64,
        simulation_reference_price: f64,
        native_price: f64,
        native_minus_simulation_absolute_gap: f64,
        local_martingale_diagnostic_mode: String,
        local_tilt_solve_count: usize,
        deviation_grid_node_count: usize,
        refined_deviation_grid_node_count: usize,
        interval_count: usize,
        maximum_tail_probability_mass: f64,
        maximum_row_probability_mass_error: f64,
        integration_numeric_quality_is_poor: bool,
        integration_convergence_check_passed: bool,
        integration_truncation_check_passed: bool,
    },
    ExpectedFailure {
        scenario_id: String,
        anchor_model_kind: String,
        kappa: f64,
        observed_boundary_state: AnchoredPricingObservedBoundaryState,
        current_anchor_value: f64,
        next_anchor_value: f64,
        observed_return_adjustment: f64,
        local_martingale_diagnostic_mode: String,
        expected_error_substring: String,
        expected_error_detail_substring: String,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchoredPricingObservedBoundaryState {
    observed_log_price: f64,
    anchor_value: f64,
    deviation: f64,
}

#[test]
fn anchored_pricing_reference_policy_and_baseline_cover_all_governed_scenarios() {
    let policy = load_reference_policy_fixture();
    let baseline = load_reference_baseline_fixture();

    assert_eq!(
        policy.schema_version,
        ANCHORED_PRICING_REFERENCE_POLICY_SCHEMA_VERSION
    );
    assert_eq!(
        baseline.schema_version,
        ANCHORED_PRICING_REFERENCE_BASELINE_SCHEMA_VERSION
    );
    assert_eq!(baseline.source_policy_id, policy.policy_id);
    assert_eq!(baseline.source_policy_version, policy.policy_version);
    assert!(!policy.governance_scope_policy.trim().is_empty());
    assert_eq!(policy.simulation_reference.simulation_engine, "seeded_tick");
    assert_eq!(
        policy.simulation_reference.discounted_payoff_estimator,
        "discounted_terminal_observed_log_price_payoff_mean.v1"
    );
    assert!(policy.simulation_reference.path_count > 0);
    assert!(policy.simulation_reference.confidence_multiple > 0.0);

    let expected_scenarios: BTreeSet<String> = policy.scenario_ids.iter().cloned().collect();
    let baseline_scenarios: BTreeSet<String> = baseline
        .cases
        .iter()
        .map(AnchoredPricingReferenceBaselineCase::scenario_id)
        .cloned()
        .collect();

    assert_eq!(baseline_scenarios, expected_scenarios);
    assert!(!baseline.baseline_id.trim().is_empty());
    assert!(!baseline.baseline_version.trim().is_empty());
    assert!(!baseline.reference_construction_policy.trim().is_empty());
    assert!(
        baseline
            .regression_tolerances
            .native_price_absolute_tolerance
            > 0.0
    );
    assert!(
        baseline
            .regression_tolerances
            .native_minus_simulation_gap_absolute_tolerance
            > 0.0
    );
}

#[test]
fn anchored_pricing_reference_fixtures_record_required_boundary_and_diagnostic_fields() {
    let policy = load_reference_policy_fixture();
    let baseline = load_reference_baseline_fixture();

    assert!(
        policy
            .diagnostic_requirements
            .observed_state_boundary_input_required
    );
    assert!(policy.diagnostic_requirements.anchor_alignment_required);
    assert!(
        policy
            .diagnostic_requirements
            .local_martingale_diagnostics_required
    );
    assert!(
        policy
            .diagnostic_requirements
            .deviation_refinement_diagnostics_required
    );
    assert!(
        policy
            .diagnostic_requirements
            .anchored_engine_numeric_quality_required
    );

    for baseline_case in &baseline.cases {
        match baseline_case {
            AnchoredPricingReferenceBaselineCase::Success {
                observed_boundary_state,
                current_anchor_value,
                next_anchor_value,
                simulation_reference_price,
                native_price,
                native_minus_simulation_absolute_gap,
                local_martingale_diagnostic_mode,
                deviation_grid_node_count,
                refined_deviation_grid_node_count,
                interval_count,
                maximum_tail_probability_mass,
                maximum_row_probability_mass_error,
                integration_numeric_quality_is_poor,
                integration_convergence_check_passed,
                integration_truncation_check_passed,
                ..
            } => {
                assert_eq!(
                    observed_boundary_state.observed_log_price,
                    observed_boundary_state.anchor_value + observed_boundary_state.deviation
                );
                assert!(current_anchor_value.is_finite());
                assert!(next_anchor_value.is_finite());
                assert!(*simulation_reference_price >= 0.0);
                assert!(*native_price >= 0.0);
                assert!(*native_minus_simulation_absolute_gap >= 0.0);
                assert!(!local_martingale_diagnostic_mode.trim().is_empty());
                assert!(*deviation_grid_node_count >= 1);
                assert!(*refined_deviation_grid_node_count >= *deviation_grid_node_count);
                assert_eq!(*interval_count, 1);
                assert!(*maximum_tail_probability_mass >= 0.0);
                assert!(*maximum_row_probability_mass_error >= 0.0);
                assert!(!integration_numeric_quality_is_poor);
                assert!(*integration_convergence_check_passed);
                assert!(*integration_truncation_check_passed);
            }
            AnchoredPricingReferenceBaselineCase::ExpectedFailure {
                observed_boundary_state,
                local_martingale_diagnostic_mode,
                expected_error_substring,
                expected_error_detail_substring,
                ..
            } => {
                assert_eq!(
                    observed_boundary_state.observed_log_price,
                    observed_boundary_state.anchor_value + observed_boundary_state.deviation
                );
                assert!(!local_martingale_diagnostic_mode.trim().is_empty());
                assert!(!expected_error_substring.trim().is_empty());
                assert!(!expected_error_detail_substring.trim().is_empty());
            }
        }
    }
}

impl AnchoredPricingReferenceBaselineCase {
    fn scenario_id(&self) -> &String {
        match self {
            Self::Success { scenario_id, .. } | Self::ExpectedFailure { scenario_id, .. } => {
                scenario_id
            }
        }
    }
}

fn load_reference_policy_fixture() -> AnchoredPricingReferencePolicy {
    serde_json::from_str(&load_reference_policy_fixture_payload())
        .expect("anchored pricing reference policy fixture should parse")
}

fn load_reference_baseline_fixture() -> AnchoredPricingReferenceBaselineArtifact {
    serde_json::from_str(&load_reference_baseline_fixture_payload())
        .expect("anchored pricing reference baseline fixture should parse")
}

fn load_reference_policy_fixture_payload() -> String {
    load_benchmark_fixture_payload("anchored_pricing_reference_policy.json")
}

fn load_reference_baseline_fixture_payload() -> String {
    load_benchmark_fixture_payload("anchored_pricing_reference_baseline_v1.json")
}

fn load_benchmark_fixture_payload(file_name: &str) -> String {
    let fixture_path = fixtures_root_path().join("benchmarks").join(file_name);
    fs::read_to_string(&fixture_path).unwrap_or_else(|source| {
        panic!(
            "failed to read benchmark fixture `{}`: {source}",
            fixture_path.display()
        )
    })
}
