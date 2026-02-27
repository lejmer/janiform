mod support;

use std::fs;

use janiform_core::benchmarking::{
    BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_ALGORITHM_POLICY,
    BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_SCOPE_POLICY,
    BENCHMARK_SCENARIO_CATALOG_FORWARD_EXTENSION_POLICY,
    BENCHMARK_SCENARIO_CATALOG_PROFILE_LOADING_ORDER_POLICY,
    BENCHMARK_SCENARIO_CATALOG_SCHEMA_FAMILY, BENCHMARK_SCENARIO_CATALOG_SCHEMA_GENERATION,
    BENCHMARK_SCENARIO_CATALOG_SCHEMA_VERSION, BenchmarkScenarioCatalog,
    BenchmarkScenarioCatalogDeterminismMetadata, BenchmarkScenarioCatalogSchemaMetadata,
    BenchmarkScenarioComponentWorkload, BenchmarkScenarioIntervalSettings,
    BenchmarkScenarioModelState, BenchmarkScenarioPortfolioShape, BenchmarkScenarioRunLength,
    BenchmarkScenarioSharedNewsState, BenchmarkScenarioWorkloadProfile, BenchmarkWorkloadScale,
    COARSE_FINE_CONSISTENCY_REGRESSION_DETERMINISM_POLICY,
    COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY,
    COARSE_FINE_CONSISTENCY_REGRESSION_METRIC_POLICY,
    COARSE_FINE_CONSISTENCY_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
    COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION,
    COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION,
    COARSE_FINE_CONSISTENCY_REGRESSION_VARIANCE_CONTROL_POLICY,
    CoarseFineConsistencyRegressionExecutionConfig, CoarseFineConsistencyRegressionToleranceConfig,
    CoarseFineConsistencyScenarioStatus, CoarseFineConsistencyScenarioSummary,
    OraclePricerRiskBenchmarkPolicy, load_oracle_pricer_risk_benchmark_policy_from_json,
    run_coarse_fine_consistency_regression_harness,
};
use janiform_core::hawkes::HawkesExcitationState;

use support::validation_fixture::fixtures_root_path;

#[test]
fn coarse_fine_harness_runs_paired_scenarios_and_emits_metric_deltas() {
    let catalog = build_bounded_catalog_fixture();
    let benchmark_policy = load_benchmark_policy_fixture();
    let tolerance_config = permissive_tolerance_config();
    let execution_config = CoarseFineConsistencyRegressionExecutionConfig {
        reproducibility_seed: 19,
        fine_subinterval_count_per_coarse_interval: 2,
        paired_risk_path_count: 8,
        fine_pricing_reference_path_count: 256,
        fine_pricing_reference_step_count: 8,
    };

    let report = run_coarse_fine_consistency_regression_harness(
        &catalog,
        &benchmark_policy,
        tolerance_config,
        execution_config,
    )
    .expect("coarse fine consistency harness should run for bounded fixture");

    assert_eq!(
        report.schema_version,
        COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION
    );
    assert_eq!(
        report.determinism_policy,
        COARSE_FINE_CONSISTENCY_REGRESSION_DETERMINISM_POLICY
    );
    assert_eq!(
        report.variance_control_policy,
        COARSE_FINE_CONSISTENCY_REGRESSION_VARIANCE_CONTROL_POLICY
    );
    assert_eq!(
        report.metric_policy,
        COARSE_FINE_CONSISTENCY_REGRESSION_METRIC_POLICY
    );
    assert_eq!(
        report.governance_scope_policy,
        COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY
    );
    assert_eq!(
        report.output_schema_metadata.schema_version,
        COARSE_FINE_CONSISTENCY_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION
    );
    assert_eq!(
        report.output_schema_metadata.tolerance_schema_version,
        COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION
    );
    assert_eq!(report.summary.total_scenario_count, 3);
    assert_eq!(report.summary.failed_scenario_count, 0);
    assert_eq!(
        report.summary.passed_scenario_count,
        report.summary.total_scenario_count
    );
    assert_eq!(report.scenario_reports.len(), 3);

    let scenario_ids: Vec<&str> = report
        .scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.scenario_id.as_str())
        .collect();
    assert_eq!(
        scenario_ids,
        vec![
            "bounded_small_profile",
            "bounded_medium_profile",
            "bounded_large_profile",
        ]
    );

    for scenario_report in &report.scenario_reports {
        assert_eq!(
            scenario_report.status,
            CoarseFineConsistencyScenarioStatus::Pass
        );
        assert_eq!(scenario_report.failed_metric_count, 0);
        assert_eq!(
            scenario_report.run_metadata.scenario_id,
            scenario_report.scenario_id
        );
        assert_eq!(scenario_report.metric_deltas.len(), 5);

        let metric_ids: Vec<&str> = scenario_report
            .metric_deltas
            .iter()
            .map(|metric_delta| metric_delta.metric_id)
            .collect();
        assert_eq!(
            metric_ids,
            vec![
                "pricing_vanilla_option_price",
                "risk_mean_a_plus",
                "risk_mean_a_minus",
                "risk_mean_n_plus",
                "risk_mean_n_minus",
            ]
        );
        assert!(scenario_report.max_absolute_delta.is_finite());

        assert_summary_values_finite(&scenario_report.summary);
    }
}

#[test]
fn coarse_fine_harness_threshold_boundaries_and_failures_are_deterministic() {
    let catalog = build_bounded_catalog_fixture();
    let benchmark_policy = load_benchmark_policy_fixture();
    let execution_config = CoarseFineConsistencyRegressionExecutionConfig {
        reproducibility_seed: 7,
        fine_subinterval_count_per_coarse_interval: 2,
        paired_risk_path_count: 4,
        fine_pricing_reference_path_count: 32,
        fine_pricing_reference_step_count: 4,
    };

    let baseline_report = run_coarse_fine_consistency_regression_harness(
        &catalog,
        &benchmark_policy,
        permissive_tolerance_config(),
        execution_config,
    )
    .expect("baseline coarse fine report should run");

    let mut pricing_abs_max = 0.0_f64;
    let mut pricing_rel_max = 0.0_f64;
    let mut risk_integrated_abs_max = 0.0_f64;
    let mut risk_integrated_rel_max = 0.0_f64;
    let mut risk_event_abs_max = 0.0_f64;
    let mut risk_event_rel_max = 0.0_f64;
    let mut max_observed_metric = baseline_report.scenario_reports[0].metric_deltas[0].clone();

    for scenario_report in &baseline_report.scenario_reports {
        for metric_delta in &scenario_report.metric_deltas {
            if metric_delta.absolute_delta > max_observed_metric.absolute_delta {
                max_observed_metric = metric_delta.clone();
            }

            match metric_delta.metric_id {
                "pricing_vanilla_option_price" => {
                    pricing_abs_max = pricing_abs_max.max(metric_delta.absolute_delta);
                    pricing_rel_max = pricing_rel_max.max(metric_delta.relative_delta);
                }
                "risk_mean_a_plus" | "risk_mean_a_minus" => {
                    risk_integrated_abs_max =
                        risk_integrated_abs_max.max(metric_delta.absolute_delta);
                    risk_integrated_rel_max =
                        risk_integrated_rel_max.max(metric_delta.relative_delta);
                }
                "risk_mean_n_plus" | "risk_mean_n_minus" => {
                    risk_event_abs_max = risk_event_abs_max.max(metric_delta.absolute_delta);
                    risk_event_rel_max = risk_event_rel_max.max(metric_delta.relative_delta);
                }
                metric_id => panic!("unexpected metric id in report: {metric_id}"),
            }
        }
    }

    let boundary_tolerance = CoarseFineConsistencyRegressionToleranceConfig {
        schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION.to_string(),
        policy_id: "coarse_fine_boundary_tolerance".to_string(),
        policy_version: "1.0.0".to_string(),
        pricing_price_absolute_delta_tolerance: pricing_abs_max,
        pricing_price_relative_delta_tolerance: pricing_rel_max,
        risk_integrated_intensity_mean_absolute_delta_tolerance: risk_integrated_abs_max,
        risk_integrated_intensity_mean_relative_delta_tolerance: risk_integrated_rel_max,
        risk_event_count_mean_absolute_delta_tolerance: risk_event_abs_max,
        risk_event_count_mean_relative_delta_tolerance: risk_event_rel_max,
    };
    let boundary_report = run_coarse_fine_consistency_regression_harness(
        &catalog,
        &benchmark_policy,
        boundary_tolerance.clone(),
        execution_config,
    )
    .expect("boundary coarse fine report should run");

    assert_eq!(boundary_report.summary.failed_scenario_count, 0);
    assert_eq!(boundary_report.summary.failed_metric_count, 0);

    assert!(
        max_observed_metric.absolute_delta > 0.0 || max_observed_metric.relative_delta > 0.0,
        "expected at least one non-zero delta to test strict threshold behavior",
    );

    let mut strict_tolerance = boundary_tolerance;
    match max_observed_metric.metric_id {
        "pricing_vanilla_option_price" => {
            strict_tolerance.pricing_price_absolute_delta_tolerance =
                decrement_tolerance(max_observed_metric.absolute_delta);
            strict_tolerance.pricing_price_relative_delta_tolerance =
                decrement_tolerance(max_observed_metric.relative_delta);
        }
        "risk_mean_a_plus" | "risk_mean_a_minus" => {
            strict_tolerance.risk_integrated_intensity_mean_absolute_delta_tolerance =
                decrement_tolerance(max_observed_metric.absolute_delta);
            strict_tolerance.risk_integrated_intensity_mean_relative_delta_tolerance =
                decrement_tolerance(max_observed_metric.relative_delta);
        }
        "risk_mean_n_plus" | "risk_mean_n_minus" => {
            strict_tolerance.risk_event_count_mean_absolute_delta_tolerance =
                decrement_tolerance(max_observed_metric.absolute_delta);
            strict_tolerance.risk_event_count_mean_relative_delta_tolerance =
                decrement_tolerance(max_observed_metric.relative_delta);
        }
        metric_id => panic!("unexpected metric id for strict tolerance test: {metric_id}"),
    }

    let strict_report = run_coarse_fine_consistency_regression_harness(
        &catalog,
        &benchmark_policy,
        strict_tolerance,
        execution_config,
    )
    .expect("strict coarse fine report should run");
    assert!(strict_report.summary.failed_metric_count > 0);
    assert!(strict_report.summary.failed_scenario_count > 0);
}

#[test]
fn coarse_fine_harness_report_is_repeatable() {
    let catalog = build_bounded_catalog_fixture();
    let benchmark_policy = load_benchmark_policy_fixture();
    let execution_config = CoarseFineConsistencyRegressionExecutionConfig {
        reproducibility_seed: 31,
        fine_subinterval_count_per_coarse_interval: 2,
        paired_risk_path_count: 8,
        fine_pricing_reference_path_count: 128,
        fine_pricing_reference_step_count: 8,
    };
    let tolerance_config = permissive_tolerance_config();

    let first_report = run_coarse_fine_consistency_regression_harness(
        &catalog,
        &benchmark_policy,
        tolerance_config.clone(),
        execution_config,
    )
    .expect("first coarse fine report should run");
    let second_report = run_coarse_fine_consistency_regression_harness(
        &catalog,
        &benchmark_policy,
        tolerance_config,
        execution_config,
    )
    .expect("second coarse fine report should run");

    assert_eq!(first_report, second_report);
    assert_eq!(
        serde_json::to_string(&first_report).expect("first report should serialize"),
        serde_json::to_string(&second_report).expect("second report should serialize"),
    );
}

fn assert_summary_values_finite(summary: &CoarseFineConsistencyScenarioSummary) {
    let values = [
        summary.coarse_pricing_value,
        summary.fine_pricing_reference_value,
        summary.coarse_risk_mean_a_plus,
        summary.fine_risk_mean_a_plus,
        summary.coarse_risk_mean_a_minus,
        summary.fine_risk_mean_a_minus,
        summary.coarse_risk_mean_n_plus,
        summary.fine_risk_mean_n_plus,
        summary.coarse_risk_mean_n_minus,
        summary.fine_risk_mean_n_minus,
    ];
    for value in values {
        assert!(value.is_finite());
    }
}

fn decrement_tolerance(value: f64) -> f64 {
    if value <= 0.0 {
        0.0
    } else {
        (value - 1.0e-12).max(0.0)
    }
}

fn permissive_tolerance_config() -> CoarseFineConsistencyRegressionToleranceConfig {
    CoarseFineConsistencyRegressionToleranceConfig {
        schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION.to_string(),
        policy_id: "coarse_fine_permissive_tolerance".to_string(),
        policy_version: "1.0.0".to_string(),
        pricing_price_absolute_delta_tolerance: 1.0e9,
        pricing_price_relative_delta_tolerance: 1.0e9,
        risk_integrated_intensity_mean_absolute_delta_tolerance: 1.0e9,
        risk_integrated_intensity_mean_relative_delta_tolerance: 1.0e9,
        risk_event_count_mean_absolute_delta_tolerance: 1.0e9,
        risk_event_count_mean_relative_delta_tolerance: 1.0e9,
    }
}

fn build_bounded_catalog_fixture() -> BenchmarkScenarioCatalog {
    BenchmarkScenarioCatalog {
        schema_version: BENCHMARK_SCENARIO_CATALOG_SCHEMA_VERSION.to_string(),
        catalog_id: "bounded_coarse_fine_catalog".to_string(),
        catalog_version: "1.0.0".to_string(),
        schema_metadata: BenchmarkScenarioCatalogSchemaMetadata {
            schema_family: BENCHMARK_SCENARIO_CATALOG_SCHEMA_FAMILY.to_string(),
            schema_generation: BENCHMARK_SCENARIO_CATALOG_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: BENCHMARK_SCENARIO_CATALOG_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        determinism_metadata: BenchmarkScenarioCatalogDeterminismMetadata {
            profile_loading_order: BENCHMARK_SCENARIO_CATALOG_PROFILE_LOADING_ORDER_POLICY
                .to_string(),
            content_hash_scope: BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_SCOPE_POLICY.to_string(),
            content_hash_algorithm: BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_ALGORITHM_POLICY
                .to_string(),
        },
        profiles: vec![
            build_profile(
                "bounded_small_profile",
                BenchmarkWorkloadScale::Small,
                0.72,
                BenchmarkScenarioComponentWorkload {
                    oracle_requests_per_iteration: 2,
                    pricing_requests_per_iteration: 3,
                    risk_paths_per_iteration: 2,
                },
            ),
            build_profile(
                "bounded_medium_profile",
                BenchmarkWorkloadScale::Medium,
                0.87,
                BenchmarkScenarioComponentWorkload {
                    oracle_requests_per_iteration: 3,
                    pricing_requests_per_iteration: 2,
                    risk_paths_per_iteration: 3,
                },
            ),
            build_profile(
                "bounded_large_profile",
                BenchmarkWorkloadScale::Large,
                1.02,
                BenchmarkScenarioComponentWorkload {
                    oracle_requests_per_iteration: 4,
                    pricing_requests_per_iteration: 4,
                    risk_paths_per_iteration: 3,
                },
            ),
        ],
    }
}

fn build_profile(
    profile_id: &str,
    workload_scale: BenchmarkWorkloadScale,
    lambda_bar: f64,
    component_workload: BenchmarkScenarioComponentWorkload,
) -> BenchmarkScenarioWorkloadProfile {
    BenchmarkScenarioWorkloadProfile {
        profile_id: profile_id.to_string(),
        workload_scale,
        model_state: BenchmarkScenarioModelState {
            baseline_deseasonalized_intensity_lambda_bar: lambda_bar,
            initial_excitation_state: HawkesExcitationState {
                y_plus_plus: 0.08,
                y_plus_minus: 0.04,
                y_minus_plus: 0.06,
                y_minus_minus: 0.09,
            },
            initial_shared_news_state: BenchmarkScenarioSharedNewsState {
                j_t: 0.03,
                g_t: 0.01,
            },
        },
        interval_settings: BenchmarkScenarioIntervalSettings {
            interval_delta: 1.0,
            horizon_interval_count: 2,
            oracle_solver_step_count: 12,
        },
        portfolio_shape: BenchmarkScenarioPortfolioShape {
            underlying_count: 1,
            maturity_bucket_count_per_underlying: 2,
            strikes_per_maturity_bucket: 3,
            option_position_count: 4,
        },
        run_length: BenchmarkScenarioRunLength {
            warmup_iteration_count: 1,
            measured_iteration_count: 2,
        },
        component_workload,
        tags: vec!["bounded".to_string(), "ci".to_string()],
        description: "Bounded benchmark profile for coarse-fine harness tests.".to_string(),
    }
}

fn load_benchmark_policy_fixture() -> OraclePricerRiskBenchmarkPolicy {
    load_oracle_pricer_risk_benchmark_policy_from_json(&load_policy_fixture_payload())
        .expect("benchmark policy fixture should parse")
}

fn load_policy_fixture_payload() -> String {
    let fixture_path = fixtures_root_path()
        .join("benchmarks")
        .join("oracle_pricer_risk_benchmark_policy.json");

    fs::read_to_string(&fixture_path).unwrap_or_else(|source| {
        panic!(
            "failed to read benchmark policy fixture `{}`: {source}",
            fixture_path.display()
        )
    })
}
