use std::fs;

use janiform_verification::{
    COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY,
    COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION, RegressionCaseStatus,
    SCALE_CONSISTENCY_DECISION_REPORT_SCHEMA_VERSION,
    SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
    SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY,
    SCALE_CONSISTENCY_THRESHOLD_POLICY_GOVERNANCE_SCOPE_POLICY,
    SCALE_CONSISTENCY_THRESHOLD_POLICY_SCHEMA_VERSION, ScaleConsistencyDecisionReasonCode,
    ScaleConsistencyEngineBenchmarkInput, ScaleConsistencyMetricCategory,
    ScaleConsistencyMetricComputationFailure, ScaleConsistencyMetricComputationStatus,
    ScaleConsistencyMetricDatasetRole, ScaleConsistencyMetricDelta,
    ScaleConsistencyMetricDeltaBatchReport, ScaleConsistencyMetricDeltaKind,
    ScaleConsistencyMetricDeltaScenarioReport, ScaleConsistencyMetricFailureReasonCode,
    ScaleConsistencyThresholdPolicy, load_scale_consistency_threshold_policy_from_json,
    run_scale_consistency_threshold_decision_engine,
};
use serde_json::{Value, json};

mod support;

use support::validation_fixture::fixtures_root_path;

#[test]
fn scale_consistency_threshold_policy_fixture_parses_and_validates() {
    let threshold_policy = load_threshold_policy_fixture();

    assert_eq!(
        threshold_policy.schema_version,
        SCALE_CONSISTENCY_THRESHOLD_POLICY_SCHEMA_VERSION
    );
    assert_eq!(
        threshold_policy.governance_scope_policy,
        SCALE_CONSISTENCY_THRESHOLD_POLICY_GOVERNANCE_SCOPE_POLICY
    );
    assert_eq!(
        threshold_policy.policy_id,
        "scale_consistency_release_gate_policy_v1"
    );
    assert_eq!(threshold_policy.policy_version, "1.0.0");
    assert_eq!(threshold_policy.interval_threshold_bands.len(), 2);
}

#[test]
fn scale_consistency_threshold_policy_rejects_change_log_without_current_version() {
    let mut policy_value = load_threshold_policy_fixture_value();
    policy_value["policy_version"] = json!("2.0.0");

    let mutated_policy_payload =
        serde_json::to_string(&policy_value).expect("mutated threshold policy should serialize");
    let error_text = load_scale_consistency_threshold_policy_from_json(&mutated_policy_payload)
        .expect_err("missing current policy version must fail validation")
        .to_string();
    assert!(
        error_text.contains("change_log must include an entry for policy_version"),
        "unexpected parse error: {error_text}"
    );
}

#[test]
fn scale_consistency_threshold_policy_rejects_duplicate_threshold_ids() {
    let mut policy_value = load_threshold_policy_fixture_value();
    policy_value["interval_threshold_bands"][1]["metric_thresholds"][0]["threshold_id"] =
        json!("subhour/returns_central_variance");

    let mutated_policy_payload =
        serde_json::to_string(&policy_value).expect("mutated threshold policy should serialize");
    let error_text = load_scale_consistency_threshold_policy_from_json(&mutated_policy_payload)
        .expect_err("duplicate threshold_id must fail validation")
        .to_string();
    assert!(
        error_text.contains("duplicate threshold_id"),
        "unexpected parse error: {error_text}"
    );
}

#[test]
fn scale_consistency_threshold_decision_engine_treats_threshold_boundary_as_inclusive() {
    let threshold_policy = load_threshold_policy_fixture();
    let metric_delta_report = ScaleConsistencyMetricDeltaBatchReport {
        schema_version: SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
        ordering_policy: SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY,
        scenario_reports: vec![
            build_scenario_report(
                "scenario_at_boundary",
                vec![computed_metric(
                    "returns_central_variance",
                    ScaleConsistencyMetricCategory::Returns,
                    Some(0.50),
                )],
            ),
            build_scenario_report(
                "scenario_above_boundary",
                vec![computed_metric(
                    "returns_central_variance",
                    ScaleConsistencyMetricCategory::Returns,
                    Some(0.500_001),
                )],
            ),
        ],
    };

    let report = run_scale_consistency_threshold_decision_engine(
        &metric_delta_report,
        &threshold_policy,
        &benchmark_input(RegressionCaseStatus::Pass),
    )
    .expect("decision engine should run on boundary fixture");

    assert_eq!(
        report.schema_version,
        SCALE_CONSISTENCY_DECISION_REPORT_SCHEMA_VERSION
    );
    assert_eq!(report.summary.total_scenario_count, 2);
    assert_eq!(report.summary.failed_scenario_count, 1);
    assert_eq!(
        report.scenario_reports[0].status,
        RegressionCaseStatus::Pass
    );
    assert_eq!(
        report.scenario_reports[1].status,
        RegressionCaseStatus::Fail
    );
    assert_eq!(
        report.scenario_reports[0].metric_decisions[0]
            .threshold_context
            .absolute_delta_exceeds_threshold,
        Some(false)
    );
    assert_eq!(
        report.scenario_reports[1].metric_decisions[0]
            .threshold_context
            .absolute_delta_exceeds_threshold,
        Some(true)
    );
}

#[test]
fn scale_consistency_threshold_decision_engine_is_deterministic_and_preserves_ordering() {
    let threshold_policy = load_threshold_policy_fixture();
    let metric_delta_report = ScaleConsistencyMetricDeltaBatchReport {
        schema_version: SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
        ordering_policy: SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY,
        scenario_reports: vec![
            build_scenario_report(
                "scenario_b",
                vec![
                    computed_metric(
                        "returns_distribution_wasserstein_distance",
                        ScaleConsistencyMetricCategory::Returns,
                        Some(0.20),
                    ),
                    missing_metric(
                        "closed_gap_abs_tail_expected_shortfall_ratio",
                        ScaleConsistencyMetricCategory::Events,
                    ),
                ],
            ),
            build_scenario_report(
                "scenario_a",
                vec![computed_metric(
                    "volatility_abs_return_acf_distribution_distance",
                    ScaleConsistencyMetricCategory::Volatility,
                    Some(0.10),
                )],
            ),
        ],
    };

    let first_report = run_scale_consistency_threshold_decision_engine(
        &metric_delta_report,
        &threshold_policy,
        &benchmark_input(RegressionCaseStatus::Pass),
    )
    .expect("first report should succeed");
    let second_report = run_scale_consistency_threshold_decision_engine(
        &metric_delta_report,
        &threshold_policy,
        &benchmark_input(RegressionCaseStatus::Pass),
    )
    .expect("second report should succeed");

    assert_eq!(first_report, second_report);
    assert_eq!(
        first_report
            .scenario_reports
            .iter()
            .map(|scenario_report| scenario_report.scenario_id.as_str())
            .collect::<Vec<_>>(),
        vec!["scenario_b", "scenario_a"]
    );
    assert_eq!(
        first_report.scenario_reports[0]
            .metric_decisions
            .iter()
            .map(|metric_decision| metric_decision.metric_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "returns_distribution_wasserstein_distance",
            "closed_gap_abs_tail_expected_shortfall_ratio",
        ]
    );
    assert_eq!(
        first_report.scenario_reports[0].metric_decisions[1].status,
        RegressionCaseStatus::Fail
    );
    assert!(
        first_report.scenario_reports[0].metric_decisions[1]
            .decision_reason_codes
            .contains(&ScaleConsistencyDecisionReasonCode::MetricComputationMissing)
    );
    assert_eq!(
        first_report.scenario_reports[0].metric_decisions[0]
            .threshold_context
            .threshold_id,
        "hourly/returns_distribution_wasserstein_distance"
    );
}

#[test]
fn scale_consistency_threshold_decision_engine_fails_when_engine_benchmark_prerequisite_fails() {
    let threshold_policy = load_threshold_policy_fixture();
    let metric_delta_report = ScaleConsistencyMetricDeltaBatchReport {
        schema_version: SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
        ordering_policy: SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY,
        scenario_reports: vec![build_scenario_report(
            "scenario_benchmark_gated",
            vec![computed_metric(
                "open_jump_abs_tail_expected_shortfall_ratio",
                ScaleConsistencyMetricCategory::Events,
                Some(0.01),
            )],
        )],
    };

    let report = run_scale_consistency_threshold_decision_engine(
        &metric_delta_report,
        &threshold_policy,
        &benchmark_input(RegressionCaseStatus::Fail),
    )
    .expect("benchmark prerequisite failure should still produce a decision report");

    assert_eq!(report.status, RegressionCaseStatus::Fail);
    assert_eq!(report.summary.failed_scenario_count, 1);
    assert_eq!(
        report.scenario_reports[0].status,
        RegressionCaseStatus::Fail
    );
    assert!(
        report.scenario_reports[0].metric_decisions[0]
            .decision_reason_codes
            .contains(&ScaleConsistencyDecisionReasonCode::EngineBenchmarkPrerequisiteFailed)
    );
}

fn load_threshold_policy_fixture() -> ScaleConsistencyThresholdPolicy {
    let fixture_payload =
        fs::read_to_string(threshold_policy_fixture_path()).expect("fixture file should exist");
    load_scale_consistency_threshold_policy_from_json(&fixture_payload)
        .expect("threshold policy fixture should parse")
}

fn load_threshold_policy_fixture_value() -> Value {
    let fixture_payload =
        fs::read_to_string(threshold_policy_fixture_path()).expect("fixture file should exist");
    serde_json::from_str(&fixture_payload).expect("fixture payload should parse as json")
}

fn threshold_policy_fixture_path() -> std::path::PathBuf {
    fixtures_root_path()
        .join("validation")
        .join("scale_consistency_threshold_policy.json")
}

fn benchmark_input(status: RegressionCaseStatus) -> ScaleConsistencyEngineBenchmarkInput {
    ScaleConsistencyEngineBenchmarkInput {
        source_report_schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION
            .to_string(),
        source_governance_scope_policy: COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY
            .to_string(),
        benchmark_policy_id: "engine_benchmark_policy".to_string(),
        benchmark_policy_version: "1.2.0".to_string(),
        tolerance_policy_id: "engine_tolerance_policy".to_string(),
        tolerance_policy_version: "1.1.0".to_string(),
        status,
    }
}

fn build_scenario_report(
    scenario_id: &str,
    metric_deltas: Vec<ScaleConsistencyMetricDelta>,
) -> ScaleConsistencyMetricDeltaScenarioReport {
    ScaleConsistencyMetricDeltaScenarioReport {
        scenario_id: scenario_id.to_string(),
        parameter_set_id: format!("parameter_set_{scenario_id}"),
        horizon_label: "hourly".to_string(),
        horizon_interval_count: 8,
        coarse_interval_delta: 1.0,
        fine_interval_delta: 0.5,
        fine_subinterval_count_per_coarse_interval: 2,
        metric_deltas,
    }
}

fn computed_metric(
    metric_id: &str,
    metric_category: ScaleConsistencyMetricCategory,
    absolute_delta: Option<f64>,
) -> ScaleConsistencyMetricDelta {
    ScaleConsistencyMetricDelta {
        metric_id: metric_id.to_string(),
        metric_name: Some(metric_id.replace('_', " ")),
        metric_variant: None,
        metric_category,
        delta_kind: ScaleConsistencyMetricDeltaKind::AbsoluteDistance,
        horizon_label: "hourly".to_string(),
        horizon_interval_count: 8,
        coarse_interval_delta: 1.0,
        fine_interval_delta: 0.5,
        status: ScaleConsistencyMetricComputationStatus::Computed,
        coarse_value: None,
        fine_value: None,
        delta_value: absolute_delta,
        absolute_delta,
        computation_failures: Vec::new(),
    }
}

fn missing_metric(
    metric_id: &str,
    metric_category: ScaleConsistencyMetricCategory,
) -> ScaleConsistencyMetricDelta {
    ScaleConsistencyMetricDelta {
        metric_id: metric_id.to_string(),
        metric_name: Some(metric_id.replace('_', " ")),
        metric_variant: None,
        metric_category,
        delta_kind: ScaleConsistencyMetricDeltaKind::SignedDifference,
        horizon_label: "hourly".to_string(),
        horizon_interval_count: 8,
        coarse_interval_delta: 1.0,
        fine_interval_delta: 0.5,
        status: ScaleConsistencyMetricComputationStatus::Missing,
        coarse_value: None,
        fine_value: None,
        delta_value: None,
        absolute_delta: None,
        computation_failures: vec![ScaleConsistencyMetricComputationFailure {
            dataset_role: ScaleConsistencyMetricDatasetRole::Coarse,
            reason_code: ScaleConsistencyMetricFailureReasonCode::MissingClosedGapObservations,
            lag_interval_count: None,
        }],
    }
}
