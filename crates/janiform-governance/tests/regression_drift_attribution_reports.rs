use janiform_governance::{
    ORACLE_PRICER_SUFFICIENT_STATISTICS_PERFORMANCE_BASELINE_COMPARISON_POLICY,
    ORACLE_PRICER_SUFFICIENT_STATISTICS_PERFORMANCE_BASELINE_COMPARISON_REPORT_SCHEMA_VERSION,
    OraclePricerSufficientStatisticsPerformanceBaselineComparisonComponentEntry,
    OraclePricerSufficientStatisticsPerformanceBaselineComparisonReport,
    OraclePricerSufficientStatisticsPerformanceBaselineComparisonSummary,
    REGRESSION_DRIFT_ATTRIBUTION_CLASSIFICATION_POLICY,
    REGRESSION_DRIFT_ATTRIBUTION_OUTPUT_SCHEMA_METADATA_VERSION,
    REGRESSION_DRIFT_ATTRIBUTION_PERFORMANCE_LATENCY_THRESHOLD_ID,
    REGRESSION_DRIFT_ATTRIBUTION_REPORT_SCHEMA_VERSION,
    REGRESSION_DRIFT_ATTRIBUTION_SCENARIO_SET_THRESHOLD_ID,
    RegressionDriftAttributionPerformanceThresholds, RegressionDriftCategory,
    build_regression_drift_attribution_report, render_regression_drift_attribution_report,
};
use janiform_governance_bench::benchmarking::BenchmarkWorkloadScale;
use janiform_validation::{
    COARSE_FINE_CONSISTENCY_REGRESSION_DETERMINISM_POLICY,
    COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY,
    COARSE_FINE_CONSISTENCY_REGRESSION_METRIC_POLICY,
    COARSE_FINE_CONSISTENCY_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
    COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION,
    COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION,
    COARSE_FINE_CONSISTENCY_REGRESSION_VARIANCE_CONTROL_POLICY, CoarseFineConsistencyMetricDelta,
    CoarseFineConsistencyRegressionExecutionConfig,
    CoarseFineConsistencyRegressionOutputSchemaMetadata, CoarseFineConsistencyRegressionReport,
    CoarseFineConsistencyRegressionSummary, CoarseFineConsistencyRegressionToleranceConfig,
    CoarseFineConsistencyScenarioReport, CoarseFineConsistencyScenarioRunMetadata,
    CoarseFineConsistencyScenarioStatus, CoarseFineConsistencyScenarioSummary,
    VERIFICATION_CANONICAL_COMPONENT_VOCABULARY, VerificationSemanticBucket,
};
use serde_json::Value;

#[test]
fn regression_drift_report_schema_lists_required_fields_and_is_renderable() {
    let current_report = build_coarse_fine_report(
        "benchmark_catalog",
        "1.0.0",
        vec![
            build_scenario_report(
                "scenario_a",
                vec![
                    build_metric_delta("oracle_c_at_delta_absolute_difference", false, 0.25, 0.31),
                    build_metric_delta("sufficient_statistics_mean_a_plus", true, 0.01, 0.02),
                ],
            ),
            build_scenario_report(
                "scenario_added",
                vec![build_metric_delta(
                    "pricing_vanilla_option_price",
                    true,
                    0.00,
                    0.00,
                )],
            ),
        ],
    );

    let reference_report = build_coarse_fine_report(
        "benchmark_catalog",
        "0.9.0",
        vec![build_scenario_report(
            "scenario_a",
            vec![build_metric_delta(
                "oracle_c_at_delta_absolute_difference",
                true,
                0.00,
                0.00,
            )],
        )],
    );

    let performance_comparison_report = build_performance_comparison_report(vec![
        OraclePricerSufficientStatisticsPerformanceBaselineComparisonComponentEntry {
            profile_id: "scenario_a".to_string(),
            workload_scale: BenchmarkWorkloadScale::Small,
            component_id: "characteristic_function_pricer".to_string(),
            reference_mean_latency_seconds: 1.0,
            candidate_mean_latency_seconds: 1.4,
            mean_latency_seconds_delta: 0.4,
            mean_latency_relative_change: 0.4,
            reference_throughput_operations_per_second: 1000.0,
            candidate_throughput_operations_per_second: 700.0,
            throughput_operations_per_second_delta: -300.0,
            throughput_relative_change: -0.3,
        },
    ]);

    let report = build_regression_drift_attribution_report(
        &current_report,
        Some(&reference_report),
        Some(&performance_comparison_report),
        RegressionDriftAttributionPerformanceThresholds {
            max_latency_relative_increase: 0.10,
            max_throughput_relative_decrease: 0.10,
        },
    )
    .expect("drift attribution report should build");

    assert_eq!(
        report.schema_version,
        REGRESSION_DRIFT_ATTRIBUTION_REPORT_SCHEMA_VERSION
    );
    assert_eq!(
        report.classification_policy,
        REGRESSION_DRIFT_ATTRIBUTION_CLASSIFICATION_POLICY
    );
    assert_eq!(
        report.output_schema_metadata.schema_version,
        REGRESSION_DRIFT_ATTRIBUTION_OUTPUT_SCHEMA_METADATA_VERSION
    );

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

    let serialized_entries = serialized
        .get("entries")
        .and_then(Value::as_array)
        .expect("entries should be present");
    for serialized_entry in serialized_entries {
        let entry_object = serialized_entry
            .as_object()
            .expect("entry should serialize as object");
        for required_field in &report.output_schema_metadata.required_entry_fields {
            assert!(
                entry_object.contains_key(*required_field),
                "entry is missing required field `{required_field}`"
            );
        }
    }

    let summary_object = serialized
        .get("summary")
        .and_then(Value::as_object)
        .expect("summary should be present");
    for required_field in &report.output_schema_metadata.required_summary_fields {
        assert!(
            summary_object.contains_key(*required_field),
            "summary is missing required field `{required_field}`"
        );
    }

    let rendered_report = render_regression_drift_attribution_report(&report);
    assert!(rendered_report.contains("Regression Drift Attribution Report"));
    assert!(rendered_report.contains("scenario_id=scenario_a"));
    assert!(
        rendered_report.contains(REGRESSION_DRIFT_ATTRIBUTION_PERFORMANCE_LATENCY_THRESHOLD_ID)
    );
}

#[test]
fn regression_drift_classification_rules_are_deterministic_and_boundary_inclusive() {
    let current_report = build_coarse_fine_report(
        "benchmark_catalog",
        "1.0.0",
        vec![build_scenario_report(
            "scenario_boundary",
            vec![
                build_metric_delta("pricing_vanilla_option_price", false, 0.12, 0.18),
                build_metric_delta("sufficient_statistics_mean_n_plus", true, 0.00, 0.00),
            ],
        )],
    );

    let performance_comparison_report = build_performance_comparison_report(vec![
        OraclePricerSufficientStatisticsPerformanceBaselineComparisonComponentEntry {
            profile_id: "scenario_boundary".to_string(),
            workload_scale: BenchmarkWorkloadScale::Medium,
            component_id: "characteristic_function_pricer".to_string(),
            reference_mean_latency_seconds: 1.0,
            candidate_mean_latency_seconds: 1.1,
            mean_latency_seconds_delta: 0.1,
            mean_latency_relative_change: 0.10,
            reference_throughput_operations_per_second: 1000.0,
            candidate_throughput_operations_per_second: 900.0,
            throughput_operations_per_second_delta: -100.0,
            throughput_relative_change: -0.10,
        },
    ]);

    let thresholds = RegressionDriftAttributionPerformanceThresholds {
        max_latency_relative_increase: 0.10,
        max_throughput_relative_decrease: 0.10,
    };

    let first_report = build_regression_drift_attribution_report(
        &current_report,
        None,
        Some(&performance_comparison_report),
        thresholds,
    )
    .expect("first drift attribution report should build");
    let second_report = build_regression_drift_attribution_report(
        &current_report,
        None,
        Some(&performance_comparison_report),
        thresholds,
    )
    .expect("second drift attribution report should build");

    assert_eq!(first_report, second_report);
    assert_eq!(first_report.summary.total_entry_count, 1);
    assert_eq!(first_report.summary.numerical_drift_count, 1);
    assert_eq!(first_report.summary.performance_drift_count, 0);
    assert_eq!(first_report.summary.scenario_change_count, 0);
    assert_eq!(
        first_report.entries[0].drift_category,
        RegressionDriftCategory::NumericalDrift
    );
}

#[test]
fn regression_drift_report_compares_against_prior_baseline_runs_with_scenario_set_diffs() {
    let current_report = build_coarse_fine_report(
        "benchmark_catalog",
        "1.1.0",
        vec![
            build_scenario_report(
                "scenario_existing",
                vec![build_metric_delta(
                    "sufficient_statistics_mean_a_plus",
                    true,
                    0.00,
                    0.00,
                )],
            ),
            build_scenario_report(
                "scenario_new",
                vec![build_metric_delta(
                    "sufficient_statistics_mean_a_minus",
                    true,
                    0.00,
                    0.00,
                )],
            ),
        ],
    );

    let reference_report = build_coarse_fine_report(
        "benchmark_catalog",
        "1.0.0",
        vec![
            build_scenario_report(
                "scenario_existing",
                vec![build_metric_delta(
                    "sufficient_statistics_mean_a_plus",
                    true,
                    0.00,
                    0.00,
                )],
            ),
            build_scenario_report(
                "scenario_removed",
                vec![build_metric_delta(
                    "sufficient_statistics_mean_a_minus",
                    true,
                    0.00,
                    0.00,
                )],
            ),
        ],
    );

    let performance_comparison_report = build_performance_comparison_report(vec![
        OraclePricerSufficientStatisticsPerformanceBaselineComparisonComponentEntry {
            profile_id: "scenario_existing".to_string(),
            workload_scale: BenchmarkWorkloadScale::Large,
            component_id: "sufficient_statistics_sampler".to_string(),
            reference_mean_latency_seconds: 1.0,
            candidate_mean_latency_seconds: 1.3,
            mean_latency_seconds_delta: 0.3,
            mean_latency_relative_change: 0.3,
            reference_throughput_operations_per_second: 1200.0,
            candidate_throughput_operations_per_second: 1000.0,
            throughput_operations_per_second_delta: -200.0,
            throughput_relative_change: -0.166_666_666_666,
        },
    ]);

    let report = build_regression_drift_attribution_report(
        &current_report,
        Some(&reference_report),
        Some(&performance_comparison_report),
        RegressionDriftAttributionPerformanceThresholds {
            max_latency_relative_increase: 0.20,
            max_throughput_relative_decrease: 0.20,
        },
    )
    .expect("drift attribution report should build");

    let scenario_change_entries: Vec<_> = report
        .entries
        .iter()
        .filter(|entry| entry.drift_category == RegressionDriftCategory::ScenarioChange)
        .collect();
    assert_eq!(scenario_change_entries.len(), 2);
    assert!(scenario_change_entries.iter().any(|entry| {
        entry.metric_id == "scenario_added_in_candidate"
            && entry.scenario_id == "scenario_new"
            && entry.threshold_id == REGRESSION_DRIFT_ATTRIBUTION_SCENARIO_SET_THRESHOLD_ID
    }));
    assert!(scenario_change_entries.iter().any(|entry| {
        entry.metric_id == "scenario_missing_from_candidate"
            && entry.scenario_id == "scenario_removed"
            && entry.threshold_id == REGRESSION_DRIFT_ATTRIBUTION_SCENARIO_SET_THRESHOLD_ID
    }));

    let performance_entries: Vec<_> = report
        .entries
        .iter()
        .filter(|entry| entry.drift_category == RegressionDriftCategory::PerformanceDrift)
        .collect();
    assert_eq!(performance_entries.len(), 1);
    assert_eq!(
        performance_entries[0].threshold_id,
        REGRESSION_DRIFT_ATTRIBUTION_PERFORMANCE_LATENCY_THRESHOLD_ID
    );

    assert_eq!(
        report
            .comparison_context
            .reference_performance_baseline_version
            .as_deref(),
        Some("1.0.0")
    );
    assert_eq!(
        report
            .comparison_context
            .candidate_performance_baseline_version
            .as_deref(),
        Some("1.1.0")
    );
}

fn build_coarse_fine_report(
    catalog_id: &str,
    catalog_version: &str,
    scenario_reports: Vec<CoarseFineConsistencyScenarioReport>,
) -> CoarseFineConsistencyRegressionReport {
    let total_scenario_count = scenario_reports.len();
    let failed_scenario_count = scenario_reports
        .iter()
        .filter(|scenario_report| {
            scenario_report.status == CoarseFineConsistencyScenarioStatus::Fail
        })
        .count();
    let passed_scenario_count = total_scenario_count - failed_scenario_count;
    let total_metric_count: usize = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.metric_deltas.len())
        .sum();
    let failed_metric_count: usize = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.failed_metric_count)
        .sum();

    CoarseFineConsistencyRegressionReport {
        schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION,
        determinism_policy: COARSE_FINE_CONSISTENCY_REGRESSION_DETERMINISM_POLICY,
        variance_control_policy: COARSE_FINE_CONSISTENCY_REGRESSION_VARIANCE_CONTROL_POLICY,
        metric_policy: COARSE_FINE_CONSISTENCY_REGRESSION_METRIC_POLICY,
        governance_scope_policy: COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY,
        output_schema_metadata: CoarseFineConsistencyRegressionOutputSchemaMetadata {
            schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
            tolerance_schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION,
            canonical_component_vocabulary: VERIFICATION_CANONICAL_COMPONENT_VOCABULARY.to_vec(),
            required_report_fields: Vec::new(),
            required_scenario_report_fields: Vec::new(),
            required_metric_delta_fields: Vec::new(),
            required_run_metadata_fields: Vec::new(),
        },
        catalog_id: catalog_id.to_string(),
        catalog_version: catalog_version.to_string(),
        benchmark_policy_id: "benchmark_policy".to_string(),
        benchmark_policy_version: "1.0.0".to_string(),
        tolerance_config: CoarseFineConsistencyRegressionToleranceConfig::default(),
        execution_config: CoarseFineConsistencyRegressionExecutionConfig::default(),
        summary: CoarseFineConsistencyRegressionSummary {
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
    metric_deltas: Vec<CoarseFineConsistencyMetricDelta>,
) -> CoarseFineConsistencyScenarioReport {
    let failed_metric_count = metric_deltas
        .iter()
        .filter(|metric_delta| !metric_delta.passed)
        .count();

    let status = if failed_metric_count == 0 {
        CoarseFineConsistencyScenarioStatus::Pass
    } else {
        CoarseFineConsistencyScenarioStatus::Fail
    };

    let max_absolute_delta = metric_deltas
        .iter()
        .map(|metric_delta| metric_delta.absolute_delta)
        .fold(0.0_f64, f64::max);

    CoarseFineConsistencyScenarioReport {
        scenario_id: scenario_id.to_string(),
        applied_tolerance_band_label: "hourly_grid".to_string(),
        status,
        failed_metric_count,
        max_absolute_delta,
        run_metadata: CoarseFineConsistencyScenarioRunMetadata {
            scenario_id: scenario_id.to_string(),
            profile_seed: 1,
            oracle_component_seed: 2,
            pricing_component_seed: 3,
            sufficient_statistics_component_seed: 4,
            coarse_interval_delta: 1.0,
            fine_interval_delta: 0.25,
            horizon_interval_count: 2,
            fine_subinterval_count_per_coarse_interval: 4,
            paired_sufficient_statistics_path_count: 32,
            fine_pricing_reference_path_count: 128,
            fine_pricing_reference_step_count: 8,
        },
        summary: CoarseFineConsistencyScenarioSummary {
            coarse_pricing_value: 1.0,
            fine_pricing_reference_value: 1.0,
            coarse_sufficient_statistics_mean_a_plus: 1.0,
            fine_sufficient_statistics_mean_a_plus: 1.0,
            coarse_sufficient_statistics_mean_a_minus: 1.0,
            fine_sufficient_statistics_mean_a_minus: 1.0,
            coarse_sufficient_statistics_mean_n_plus: 1.0,
            fine_sufficient_statistics_mean_n_plus: 1.0,
            coarse_sufficient_statistics_mean_n_minus: 1.0,
            fine_sufficient_statistics_mean_n_minus: 1.0,
        },
        metric_deltas,
    }
}

fn build_metric_delta(
    metric_id: &'static str,
    passed: bool,
    absolute_delta: f64,
    relative_delta: f64,
) -> CoarseFineConsistencyMetricDelta {
    CoarseFineConsistencyMetricDelta {
        metric_id,
        semantic_bucket: VerificationSemanticBucket::TradingReturn,
        coarse_value: 1.0 + absolute_delta,
        fine_reference_value: 1.0,
        absolute_delta,
        relative_delta,
        absolute_tolerance: 0.10,
        relative_tolerance: 0.10,
        passed,
    }
}

fn build_performance_comparison_report(
    component_entries: Vec<
        OraclePricerSufficientStatisticsPerformanceBaselineComparisonComponentEntry,
    >,
) -> OraclePricerSufficientStatisticsPerformanceBaselineComparisonReport {
    let profile_count = component_entries
        .iter()
        .map(|entry| entry.profile_id.as_str())
        .collect::<std::collections::BTreeSet<&str>>()
        .len();
    let component_entry_count = component_entries.len();

    OraclePricerSufficientStatisticsPerformanceBaselineComparisonReport {
        schema_version: ORACLE_PRICER_SUFFICIENT_STATISTICS_PERFORMANCE_BASELINE_COMPARISON_REPORT_SCHEMA_VERSION,
        comparison_policy: ORACLE_PRICER_SUFFICIENT_STATISTICS_PERFORMANCE_BASELINE_COMPARISON_POLICY,
        baseline_id: "oracle_pricer_sufficient_statistics_ci_performance_baseline".to_string(),
        reference_baseline_version: "1.0.0".to_string(),
        candidate_baseline_version: "1.1.0".to_string(),
        catalog_id: "benchmark_catalog".to_string(),
        catalog_version: "1.0.0".to_string(),
        policy_id: "benchmark_policy".to_string(),
        policy_version: "1.0.0".to_string(),
        summary: OraclePricerSufficientStatisticsPerformanceBaselineComparisonSummary {
            profile_count,
            component_entry_count,
        },
        component_entries,
    }
}
