use janiform_core::verification::{
    COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY,
    COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION, RegressionCaseStatus,
    SCALE_CONSISTENCY_DECISION_ORDERING_POLICY, SCALE_CONSISTENCY_DECISION_POLICY,
    SCALE_CONSISTENCY_DECISION_REPORT_SCHEMA_VERSION,
    SCALE_CONSISTENCY_ENGINE_BENCHMARK_INPUT_POLICY,
    SCALE_CONSISTENCY_REPORT_CLASSIFICATION_POLICY, SCALE_CONSISTENCY_REPORT_SCHEMA_VERSION,
    ScaleConsistencyDecisionBatchReport, ScaleConsistencyDecisionReasonCode,
    ScaleConsistencyDecisionScenarioReport, ScaleConsistencyDecisionSummary,
    ScaleConsistencyDecisionThresholdContext, ScaleConsistencyDriftCategory,
    ScaleConsistencyEngineBenchmarkInput, ScaleConsistencyMetricCategory,
    ScaleConsistencyMetricComputationFailure, ScaleConsistencyMetricComputationStatus,
    ScaleConsistencyMetricDatasetRole, ScaleConsistencyMetricDecision,
    ScaleConsistencyMetricDeltaKind, build_scale_consistency_report,
    render_scale_consistency_report_summary,
};
use serde_json::Value;

#[test]
fn scale_consistency_report_classifies_drift_categories_deterministically() {
    let decision_report = build_decision_report(vec![
        build_scenario_report(
            "scenario-drift",
            vec![
                failed_metric_decision(
                    "returns_central_variance",
                    None,
                    ScaleConsistencyMetricCategory::Returns,
                    Some(0.42),
                    vec![ScaleConsistencyDecisionReasonCode::AbsoluteDeltaThresholdExceeded],
                ),
                failed_metric_decision(
                    "volatility_abs_return_acf_distribution_distance",
                    Some("lag_grid_1"),
                    ScaleConsistencyMetricCategory::Volatility,
                    None,
                    vec![ScaleConsistencyDecisionReasonCode::MetricComputationMissing],
                ),
                failed_metric_decision(
                    "open_jump_abs_tail_expected_shortfall_ratio",
                    None,
                    ScaleConsistencyMetricCategory::Events,
                    Some(0.08),
                    vec![ScaleConsistencyDecisionReasonCode::AbsoluteDeltaThresholdExceeded],
                ),
            ],
        ),
        build_scenario_report(
            "scenario-benchmark-only",
            vec![failed_metric_decision(
                "returns_distribution_wasserstein_distance",
                None,
                ScaleConsistencyMetricCategory::Returns,
                Some(0.01),
                vec![ScaleConsistencyDecisionReasonCode::EngineBenchmarkPrerequisiteFailed],
            )],
        ),
    ]);

    let first_report =
        build_scale_consistency_report(&decision_report).expect("first report should build");
    let second_report =
        build_scale_consistency_report(&decision_report).expect("second report should build");

    assert_eq!(first_report, second_report);
    assert_eq!(
        first_report.schema_version,
        SCALE_CONSISTENCY_REPORT_SCHEMA_VERSION
    );
    assert_eq!(
        first_report.classification_policy,
        SCALE_CONSISTENCY_REPORT_CLASSIFICATION_POLICY
    );
    assert_eq!(first_report.summary.total_scenario_count, 2);
    assert_eq!(first_report.summary.failed_scenario_count, 2);
    assert_eq!(first_report.summary.returns_primary_drift_scenario_count, 1);
    assert_eq!(first_report.summary.uncategorized_failed_scenario_count, 1);

    let drift_scenario = &first_report.scenario_reports[0];
    assert_eq!(
        drift_scenario.primary_drift_category,
        Some(ScaleConsistencyDriftCategory::Returns)
    );
    assert_eq!(drift_scenario.drift_diagnostics.len(), 3);
    assert_eq!(
        drift_scenario
            .drift_diagnostics
            .iter()
            .map(|drift_diagnostic| drift_diagnostic.drift_category)
            .collect::<Vec<_>>(),
        vec![
            ScaleConsistencyDriftCategory::Returns,
            ScaleConsistencyDriftCategory::Volatility,
            ScaleConsistencyDriftCategory::EventDerived,
        ]
    );
    assert_eq!(drift_scenario.drift_diagnostics[1].missing_metric_count, 1);
    assert_eq!(
        drift_scenario.drift_diagnostics[1].metric_references[0]
            .metric_variant
            .as_deref(),
        Some("lag_grid_1")
    );

    let benchmark_only_scenario = &first_report.scenario_reports[1];
    assert_eq!(benchmark_only_scenario.primary_drift_category, None);
    assert!(benchmark_only_scenario.drift_diagnostics.is_empty());
    assert_eq!(
        benchmark_only_scenario.non_drift_failure_reason_codes,
        vec![ScaleConsistencyDecisionReasonCode::EngineBenchmarkPrerequisiteFailed]
    );
}

#[test]
fn scale_consistency_report_schema_lists_required_fields() {
    let report =
        build_scale_consistency_report(&build_decision_report(vec![build_scenario_report(
            "scenario-a",
            vec![failed_metric_decision(
                "returns_central_variance",
                None,
                ScaleConsistencyMetricCategory::Returns,
                Some(0.5),
                vec![ScaleConsistencyDecisionReasonCode::AbsoluteDeltaThresholdExceeded],
            )],
        )]))
        .expect("report should build");

    let serialized = serde_json::to_value(&report).expect("report should serialize");
    let serialized_object = serialized
        .as_object()
        .expect("serialized report should be an object");
    for required_field in &report.output_schema_metadata.required_report_fields {
        assert!(
            serialized_object.contains_key(*required_field),
            "report is missing required field `{required_field}`"
        );
    }

    let summary_object = serialized
        .get("summary")
        .and_then(Value::as_object)
        .expect("summary should serialize as object");
    for required_field in &report.output_schema_metadata.required_summary_fields {
        assert!(
            summary_object.contains_key(*required_field),
            "summary is missing required field `{required_field}`"
        );
    }

    let scenario_object = serialized
        .get("scenario_reports")
        .and_then(Value::as_array)
        .and_then(|scenario_reports| scenario_reports.first())
        .and_then(Value::as_object)
        .expect("scenario should serialize as object");
    for required_field in &report.output_schema_metadata.required_scenario_fields {
        assert!(
            scenario_object.contains_key(*required_field),
            "scenario is missing required field `{required_field}`"
        );
    }

    let drift_diagnostic_object = scenario_object
        .get("drift_diagnostics")
        .and_then(Value::as_array)
        .and_then(|drift_diagnostics| drift_diagnostics.first())
        .and_then(Value::as_object)
        .expect("drift diagnostic should serialize as object");
    for required_field in &report
        .output_schema_metadata
        .required_drift_diagnostic_fields
    {
        assert!(
            drift_diagnostic_object.contains_key(*required_field),
            "drift diagnostic is missing required field `{required_field}`"
        );
    }

    let drift_metric_reference_object = drift_diagnostic_object
        .get("metric_references")
        .and_then(Value::as_array)
        .and_then(|metric_references| metric_references.first())
        .and_then(Value::as_object)
        .expect("metric reference should serialize as object");
    for required_field in &report
        .output_schema_metadata
        .required_drift_metric_reference_fields
    {
        assert!(
            drift_metric_reference_object.contains_key(*required_field),
            "metric reference is missing required field `{required_field}`"
        );
    }

    let metric_decision_object = scenario_object
        .get("metric_decisions")
        .and_then(Value::as_array)
        .and_then(|metric_decisions| metric_decisions.first())
        .and_then(Value::as_object)
        .expect("metric decision should serialize as object");
    for required_field in &report
        .output_schema_metadata
        .required_metric_decision_fields
    {
        assert!(
            metric_decision_object.contains_key(*required_field),
            "metric decision is missing required field `{required_field}`"
        );
    }

    let threshold_context_object = metric_decision_object
        .get("threshold_context")
        .and_then(Value::as_object)
        .expect("threshold context should serialize as object");
    for required_field in &report
        .output_schema_metadata
        .required_threshold_context_fields
    {
        assert!(
            threshold_context_object.contains_key(*required_field),
            "threshold context is missing required field `{required_field}`"
        );
    }
}

#[test]
fn scale_consistency_report_rendering_preserves_scenario_and_diagnostic_ordering() {
    let report = build_scale_consistency_report(&build_decision_report(vec![
        build_scenario_report(
            "scenario-beta",
            vec![
                failed_metric_decision(
                    "returns_central_variance",
                    None,
                    ScaleConsistencyMetricCategory::Returns,
                    Some(0.4),
                    vec![ScaleConsistencyDecisionReasonCode::AbsoluteDeltaThresholdExceeded],
                ),
                failed_metric_decision(
                    "closed_gap_abs_tail_expected_shortfall_ratio",
                    None,
                    ScaleConsistencyMetricCategory::Events,
                    Some(0.2),
                    vec![ScaleConsistencyDecisionReasonCode::AbsoluteDeltaThresholdExceeded],
                ),
            ],
        ),
        build_scenario_report("scenario-alpha", vec![passing_metric_decision()]),
    ]))
    .expect("report should build");

    let rendered_report = render_scale_consistency_report_summary(&report);

    assert!(rendered_report.contains("Scale Consistency Report"));
    assert!(rendered_report.contains("status: fail"));

    let beta_position = rendered_report
        .find("scenario_id=scenario-beta")
        .expect("scenario-beta should be rendered");
    let alpha_position = rendered_report
        .find("scenario_id=scenario-alpha")
        .expect("scenario-alpha should be rendered");
    assert!(beta_position < alpha_position);

    let returns_position = rendered_report
        .find("drift_category=returns")
        .expect("returns diagnostic should be rendered");
    let event_position = rendered_report
        .find("drift_category=event_derived")
        .expect("event diagnostic should be rendered");
    assert!(returns_position < event_position);
    assert!(rendered_report.contains("drift_diagnostics=none"));
}

fn build_decision_report(
    scenario_reports: Vec<ScaleConsistencyDecisionScenarioReport>,
) -> ScaleConsistencyDecisionBatchReport {
    let total_scenario_count = scenario_reports.len();
    let passed_scenario_count = scenario_reports
        .iter()
        .filter(|scenario_report| scenario_report.status == RegressionCaseStatus::Pass)
        .count();
    let failed_scenario_count = total_scenario_count - passed_scenario_count;
    let total_metric_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.metric_decisions.len())
        .sum();
    let failed_metric_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.failed_metric_count)
        .sum();

    ScaleConsistencyDecisionBatchReport {
        schema_version: SCALE_CONSISTENCY_DECISION_REPORT_SCHEMA_VERSION,
        ordering_policy: SCALE_CONSISTENCY_DECISION_ORDERING_POLICY,
        decision_policy: SCALE_CONSISTENCY_DECISION_POLICY,
        governance_scope_policy: "model_validation_release_and_calibration_governance_thresholds_with_engine_benchmark_prerequisite_inputs.v1",
        benchmark_input_policy: SCALE_CONSISTENCY_ENGINE_BENCHMARK_INPUT_POLICY,
        threshold_policy_id: "scale_consistency_release_gate_policy_v1".to_string(),
        threshold_policy_version: "1.0.0".to_string(),
        engine_benchmark_input: ScaleConsistencyEngineBenchmarkInput {
            source_report_schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION
                .to_string(),
            source_governance_scope_policy:
                COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY.to_string(),
            benchmark_policy_id: "engine_benchmark_policy".to_string(),
            benchmark_policy_version: "1.0.0".to_string(),
            tolerance_policy_id: "engine_tolerance_policy".to_string(),
            tolerance_policy_version: "1.0.0".to_string(),
            status: RegressionCaseStatus::Pass,
        },
        status: if failed_scenario_count == 0 {
            RegressionCaseStatus::Pass
        } else {
            RegressionCaseStatus::Fail
        },
        summary: ScaleConsistencyDecisionSummary {
            total_scenario_count,
            passed_scenario_count,
            failed_scenario_count,
            total_metric_count,
            failed_metric_count,
        },
        scenario_reports,
    }
}

fn build_scenario_report(
    scenario_id: &str,
    metric_decisions: Vec<ScaleConsistencyMetricDecision>,
) -> ScaleConsistencyDecisionScenarioReport {
    let failed_metric_count = metric_decisions
        .iter()
        .filter(|metric_decision| metric_decision.status == RegressionCaseStatus::Fail)
        .count();

    ScaleConsistencyDecisionScenarioReport {
        scenario_id: scenario_id.to_string(),
        parameter_set_id: format!("parameter-set-{scenario_id}"),
        horizon_label: "hourly".to_string(),
        coarse_interval_delta: 1.0,
        fine_interval_delta: 0.25,
        applied_interval_threshold_band_label: "hourly_grid".to_string(),
        status: if failed_metric_count == 0 {
            RegressionCaseStatus::Pass
        } else {
            RegressionCaseStatus::Fail
        },
        failed_metric_count,
        metric_decisions,
    }
}

fn failed_metric_decision(
    metric_id: &str,
    metric_variant: Option<&str>,
    metric_category: ScaleConsistencyMetricCategory,
    absolute_delta: Option<f64>,
    decision_reason_codes: Vec<ScaleConsistencyDecisionReasonCode>,
) -> ScaleConsistencyMetricDecision {
    let metric_computation_status = if decision_reason_codes
        .contains(&ScaleConsistencyDecisionReasonCode::MetricComputationMissing)
    {
        ScaleConsistencyMetricComputationStatus::Missing
    } else {
        ScaleConsistencyMetricComputationStatus::Computed
    };
    let computation_failures = if metric_computation_status
        == ScaleConsistencyMetricComputationStatus::Missing
    {
        vec![ScaleConsistencyMetricComputationFailure {
            dataset_role: ScaleConsistencyMetricDatasetRole::Coarse,
            reason_code:
                janiform_core::verification::ScaleConsistencyMetricFailureReasonCode::InsufficientReturnObservations,
            lag_interval_count: None,
        }]
    } else {
        Vec::new()
    };

    ScaleConsistencyMetricDecision {
        metric_id: metric_id.to_string(),
        metric_name: Some(metric_id.replace('_', " ")),
        metric_variant: metric_variant.map(str::to_string),
        metric_category,
        delta_kind: ScaleConsistencyMetricDeltaKind::AbsoluteDistance,
        metric_computation_status,
        coarse_value: None,
        fine_value: None,
        delta_value: absolute_delta,
        absolute_delta,
        computation_failures,
        status: RegressionCaseStatus::Fail,
        threshold_context: ScaleConsistencyDecisionThresholdContext {
            policy_id: "scale_consistency_release_gate_policy_v1".to_string(),
            policy_version: "1.0.0".to_string(),
            interval_threshold_band_label: "hourly_grid".to_string(),
            threshold_id: format!("hourly/{metric_id}"),
            threshold_rationale: "validation threshold".to_string(),
            max_absolute_delta: 0.25,
            absolute_delta_exceeds_threshold: absolute_delta.map(|value| value > 0.25),
        },
        decision_reason_codes,
    }
}

fn passing_metric_decision() -> ScaleConsistencyMetricDecision {
    ScaleConsistencyMetricDecision {
        metric_id: "returns_distribution_wasserstein_distance".to_string(),
        metric_name: Some("returns distribution wasserstein distance".to_string()),
        metric_variant: None,
        metric_category: ScaleConsistencyMetricCategory::Returns,
        delta_kind: ScaleConsistencyMetricDeltaKind::AbsoluteDistance,
        metric_computation_status: ScaleConsistencyMetricComputationStatus::Computed,
        coarse_value: None,
        fine_value: None,
        delta_value: Some(0.05),
        absolute_delta: Some(0.05),
        computation_failures: Vec::new(),
        status: RegressionCaseStatus::Pass,
        threshold_context: ScaleConsistencyDecisionThresholdContext {
            policy_id: "scale_consistency_release_gate_policy_v1".to_string(),
            policy_version: "1.0.0".to_string(),
            interval_threshold_band_label: "hourly_grid".to_string(),
            threshold_id: "hourly/returns_distribution_wasserstein_distance".to_string(),
            threshold_rationale: "validation threshold".to_string(),
            max_absolute_delta: 0.25,
            absolute_delta_exceeds_threshold: Some(false),
        },
        decision_reason_codes: Vec::new(),
    }
}
