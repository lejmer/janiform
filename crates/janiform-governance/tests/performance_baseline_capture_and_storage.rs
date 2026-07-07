mod support;

use std::fs;

use janiform_governance::{
    OraclePricerSufficientStatisticsPerformanceBaselineArtifact,
    OraclePricerSufficientStatisticsPerformanceBaselineCaptureConfig,
    OraclePricerSufficientStatisticsPerformanceBaselineComponentMetrics,
    capture_oracle_pricer_sufficient_statistics_performance_baseline_artifact,
    compare_oracle_pricer_sufficient_statistics_performance_baseline_versions,
    load_oracle_pricer_sufficient_statistics_performance_baseline_from_json,
    serialize_oracle_pricer_sufficient_statistics_performance_baseline_to_json,
};
use janiform_governance_bench::benchmarking::{
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
    OraclePricerSufficientStatisticsBenchmarkExecutionConfig,
    load_oracle_pricer_sufficient_statistics_benchmark_policy_from_json,
    run_oracle_pricer_sufficient_statistics_benchmark_harness,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use serde_json::{Value, json};

use support::validation_fixture::fixtures_root_path;

#[test]
fn performance_baseline_capture_round_trip_is_lossless() {
    let benchmark_catalog = build_bounded_catalog_fixture();
    let benchmark_policy = load_benchmark_policy_fixture();
    let execution_config = OraclePricerSufficientStatisticsBenchmarkExecutionConfig {
        warmup_iteration_count_override: Some(0),
        measured_iteration_count_override: Some(1),
        reproducibility_seed: 23,
    };

    let benchmark_report = run_oracle_pricer_sufficient_statistics_benchmark_harness(
        &benchmark_catalog,
        &benchmark_policy,
        execution_config,
    )
    .expect("benchmark harness should run with bounded fixture");
    let baseline = capture_oracle_pricer_sufficient_statistics_performance_baseline_artifact(
        &benchmark_report,
        OraclePricerSufficientStatisticsPerformanceBaselineCaptureConfig {
            baseline_id: "oracle_pricer_sufficient_statistics_ci_performance_baseline".to_string(),
            baseline_version: "2.0.0".to_string(),
            owner_team: "coarse_time_pricing_and_sufficient_statistics_engines".to_string(),
            owner_oncall: "janiform-owners".to_string(),
            refresh_process:
                "tests/fixtures/benchmarks/README.md#performance-baseline-refresh-process"
                    .to_string(),
        },
    )
    .expect("capturing performance baseline from report should succeed");

    let baseline_payload =
        serialize_oracle_pricer_sufficient_statistics_performance_baseline_to_json(&baseline)
            .expect("captured baseline should serialize");
    let reloaded_baseline =
        load_oracle_pricer_sufficient_statistics_performance_baseline_from_json(&baseline_payload)
            .expect("serialized baseline payload should load");

    assert_baseline_round_trip_close(&baseline, &reloaded_baseline);
    assert_eq!(reloaded_baseline.summary.profile_count, 3);
}

#[test]
fn performance_baseline_interval_matrix_profiles_are_versioned_and_comparable() {
    let benchmark_catalog = build_interval_matrix_bounded_catalog_fixture();
    let benchmark_policy = load_benchmark_policy_fixture();
    let execution_config = OraclePricerSufficientStatisticsBenchmarkExecutionConfig {
        warmup_iteration_count_override: Some(0),
        measured_iteration_count_override: Some(1),
        reproducibility_seed: 23,
    };

    let benchmark_report = run_oracle_pricer_sufficient_statistics_benchmark_harness(
        &benchmark_catalog,
        &benchmark_policy,
        execution_config,
    )
    .expect("interval-matrix benchmark harness run should succeed");

    let reference_baseline =
        capture_oracle_pricer_sufficient_statistics_performance_baseline_artifact(
            &benchmark_report,
            OraclePricerSufficientStatisticsPerformanceBaselineCaptureConfig {
                baseline_id: "oracle_pricer_sufficient_statistics_ci_performance_baseline"
                    .to_string(),
                baseline_version: "2.0.0-matrix".to_string(),
                owner_team: "coarse_time_pricing_and_sufficient_statistics_engines".to_string(),
                owner_oncall: "janiform-owners".to_string(),
                refresh_process:
                    "tests/fixtures/benchmarks/README.md#performance-baseline-refresh-process"
                        .to_string(),
            },
        )
        .expect("capturing reference interval-matrix baseline should succeed");

    let candidate_baseline =
        capture_oracle_pricer_sufficient_statistics_performance_baseline_artifact(
            &benchmark_report,
            OraclePricerSufficientStatisticsPerformanceBaselineCaptureConfig {
                baseline_id: "oracle_pricer_sufficient_statistics_ci_performance_baseline"
                    .to_string(),
                baseline_version: "2.1.0-matrix".to_string(),
                owner_team: "coarse_time_pricing_and_sufficient_statistics_engines".to_string(),
                owner_oncall: "janiform-owners".to_string(),
                refresh_process:
                    "tests/fixtures/benchmarks/README.md#performance-baseline-refresh-process"
                        .to_string(),
            },
        )
        .expect("capturing candidate interval-matrix baseline should succeed");

    let comparison = compare_oracle_pricer_sufficient_statistics_performance_baseline_versions(
        &reference_baseline,
        &candidate_baseline,
    )
    .expect("comparing interval-matrix baseline versions should succeed");

    let expected_profile_count = benchmark_catalog.profiles.len();
    assert_eq!(comparison.summary.profile_count, expected_profile_count);
    assert_eq!(
        comparison.summary.component_entry_count,
        expected_profile_count * 3
    );
    assert!(
        comparison
            .component_entries
            .iter()
            .any(|entry| entry.profile_id == "interval_matrix_small_half_hour"),
        "comparison should include sub-hour profile entries",
    );
}

#[test]
fn performance_baseline_loader_rejects_invalid_schema_version() {
    let baseline_payload =
        load_fixture_payload("oracle_pricer_sufficient_statistics_performance_baseline_v1.json");
    let mut baseline_value: Value =
        serde_json::from_str(&baseline_payload).expect("baseline payload should parse as JSON");
    baseline_value["schema_version"] =
        json!("benchmark.oracle_pricer_sufficient_statistics.performance_baseline.v0");

    let mutated_payload =
        serde_json::to_string(&baseline_value).expect("mutated baseline payload should serialize");
    let error_text =
        load_oracle_pricer_sufficient_statistics_performance_baseline_from_json(&mutated_payload)
            .expect_err("invalid schema_version must fail")
            .to_string();

    assert!(error_text.contains("schema_version"));
}

#[test]
fn performance_baseline_comparison_reports_cross_version_component_deltas() {
    let reference_payload =
        load_fixture_payload("oracle_pricer_sufficient_statistics_performance_baseline_v1.json");
    let candidate_payload =
        load_fixture_payload("oracle_pricer_sufficient_statistics_performance_baseline_v1_1.json");

    let reference_baseline =
        load_oracle_pricer_sufficient_statistics_performance_baseline_from_json(&reference_payload)
            .expect("reference baseline fixture should load");
    let candidate_baseline =
        load_oracle_pricer_sufficient_statistics_performance_baseline_from_json(&candidate_payload)
            .expect("candidate baseline fixture should load");

    let comparison_report =
        compare_oracle_pricer_sufficient_statistics_performance_baseline_versions(
            &reference_baseline,
            &candidate_baseline,
        )
        .expect("comparing two compatible baseline versions should succeed");

    assert_eq!(
        comparison_report.reference_baseline_version,
        reference_baseline.baseline_version
    );
    assert_eq!(
        comparison_report.candidate_baseline_version,
        candidate_baseline.baseline_version
    );
    assert_eq!(comparison_report.summary.profile_count, 3);
    assert_eq!(comparison_report.summary.component_entry_count, 9);

    let medium_pricing_entry = comparison_report
        .component_entries
        .iter()
        .find(|entry| {
            entry.profile_id == "medium_hourly_balanced"
                && entry.component_id == "characteristic_function_pricer"
        })
        .expect("comparison report should include medium profile pricing component entry");

    let expected_delta = 0.00083 - 0.00079;
    assert!((medium_pricing_entry.mean_latency_seconds_delta - expected_delta).abs() < 1.0e-12);
    assert!(medium_pricing_entry.throughput_operations_per_second_delta < 0.0);
}

fn load_benchmark_policy_fixture()
-> janiform_governance_bench::benchmarking::OraclePricerSufficientStatisticsBenchmarkPolicy {
    let policy_payload =
        load_fixture_payload("oracle_pricer_sufficient_statistics_benchmark_policy.json");
    load_oracle_pricer_sufficient_statistics_benchmark_policy_from_json(&policy_payload)
        .expect("benchmark policy fixture should parse")
}

fn load_fixture_payload(fixture_name: &str) -> String {
    let fixture_path = fixtures_root_path().join("benchmarks").join(fixture_name);
    fs::read_to_string(&fixture_path).unwrap_or_else(|source| {
        panic!(
            "failed to read benchmark fixture `{}`: {source}",
            fixture_path.display()
        )
    })
}

fn build_bounded_catalog_fixture() -> BenchmarkScenarioCatalog {
    BenchmarkScenarioCatalog {
        schema_version: BENCHMARK_SCENARIO_CATALOG_SCHEMA_VERSION.to_string(),
        catalog_id: "bounded_benchmark_catalog".to_string(),
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
                    sufficient_statistics_paths_per_iteration: 2,
                },
            ),
            build_profile(
                "bounded_medium_profile",
                BenchmarkWorkloadScale::Medium,
                0.87,
                BenchmarkScenarioComponentWorkload {
                    oracle_requests_per_iteration: 3,
                    pricing_requests_per_iteration: 2,
                    sufficient_statistics_paths_per_iteration: 3,
                },
            ),
            build_profile(
                "bounded_large_profile",
                BenchmarkWorkloadScale::Large,
                1.02,
                BenchmarkScenarioComponentWorkload {
                    oracle_requests_per_iteration: 4,
                    pricing_requests_per_iteration: 4,
                    sufficient_statistics_paths_per_iteration: 3,
                },
            ),
        ],
    }
}

fn build_interval_matrix_bounded_catalog_fixture() -> BenchmarkScenarioCatalog {
    BenchmarkScenarioCatalog {
        schema_version: BENCHMARK_SCENARIO_CATALOG_SCHEMA_VERSION.to_string(),
        catalog_id: "bounded_benchmark_catalog_interval_matrix".to_string(),
        catalog_version: "1.0.1".to_string(),
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
            build_profile_with_interval(
                "interval_matrix_small_hourly",
                BenchmarkWorkloadScale::Small,
                1.0,
                0.72,
                BenchmarkScenarioComponentWorkload {
                    oracle_requests_per_iteration: 1,
                    pricing_requests_per_iteration: 1,
                    sufficient_statistics_paths_per_iteration: 1,
                },
            ),
            build_profile_with_interval(
                "interval_matrix_small_half_hour",
                BenchmarkWorkloadScale::Small,
                0.5,
                0.74,
                BenchmarkScenarioComponentWorkload {
                    oracle_requests_per_iteration: 1,
                    pricing_requests_per_iteration: 1,
                    sufficient_statistics_paths_per_iteration: 1,
                },
            ),
            build_profile(
                "bounded_medium_profile",
                BenchmarkWorkloadScale::Medium,
                0.87,
                BenchmarkScenarioComponentWorkload {
                    oracle_requests_per_iteration: 2,
                    pricing_requests_per_iteration: 2,
                    sufficient_statistics_paths_per_iteration: 2,
                },
            ),
            build_profile(
                "bounded_large_profile",
                BenchmarkWorkloadScale::Large,
                1.02,
                BenchmarkScenarioComponentWorkload {
                    oracle_requests_per_iteration: 2,
                    pricing_requests_per_iteration: 2,
                    sufficient_statistics_paths_per_iteration: 2,
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
            option_position_count: 6,
        },
        run_length: BenchmarkScenarioRunLength {
            warmup_iteration_count: 1,
            measured_iteration_count: 2,
        },
        component_workload,
        tags: vec!["bounded".to_string(), "test".to_string()],
        description: format!("Bounded benchmark profile {profile_id}"),
    }
}

fn build_profile_with_interval(
    profile_id: &str,
    workload_scale: BenchmarkWorkloadScale,
    interval_delta: f64,
    lambda_bar: f64,
    component_workload: BenchmarkScenarioComponentWorkload,
) -> BenchmarkScenarioWorkloadProfile {
    let mut profile = build_profile(profile_id, workload_scale, lambda_bar, component_workload);
    profile.interval_settings.interval_delta = interval_delta;
    profile
}

fn assert_baseline_round_trip_close(
    baseline: &OraclePricerSufficientStatisticsPerformanceBaselineArtifact,
    reloaded_baseline: &OraclePricerSufficientStatisticsPerformanceBaselineArtifact,
) {
    assert_eq!(baseline.schema_version, reloaded_baseline.schema_version);
    assert_eq!(baseline.baseline_id, reloaded_baseline.baseline_id);
    assert_eq!(
        baseline.baseline_version,
        reloaded_baseline.baseline_version
    );
    assert_eq!(baseline.schema_metadata, reloaded_baseline.schema_metadata);
    assert_eq!(
        baseline.ownership_metadata,
        reloaded_baseline.ownership_metadata
    );
    assert_eq!(baseline.capture_policy, reloaded_baseline.capture_policy);
    assert_eq!(
        baseline.output_schema_metadata,
        reloaded_baseline.output_schema_metadata
    );
    assert_eq!(baseline.source_report, reloaded_baseline.source_report);
    assert_eq!(baseline.summary, reloaded_baseline.summary);
    assert_eq!(
        baseline.profile_records.len(),
        reloaded_baseline.profile_records.len()
    );

    for (baseline_profile, reloaded_profile) in baseline
        .profile_records
        .iter()
        .zip(reloaded_baseline.profile_records.iter())
    {
        assert_eq!(baseline_profile.profile_id, reloaded_profile.profile_id);
        assert_eq!(
            baseline_profile.workload_scale,
            reloaded_profile.workload_scale
        );
        assert_eq!(
            baseline_profile.interval_delta, reloaded_profile.interval_delta,
            "interval_delta should round-trip exactly in baseline profile records",
        );
        assert_component_metrics_close(
            baseline_profile.oracle_solver_metrics,
            reloaded_profile.oracle_solver_metrics,
        );
        assert_component_metrics_close(
            baseline_profile.pricing_metrics,
            reloaded_profile.pricing_metrics,
        );
        assert_component_metrics_close(
            baseline_profile.sufficient_statistics_sampling_metrics,
            reloaded_profile.sufficient_statistics_sampling_metrics,
        );
    }
}

fn assert_component_metrics_close(
    baseline_metrics: OraclePricerSufficientStatisticsPerformanceBaselineComponentMetrics,
    reloaded_metrics: OraclePricerSufficientStatisticsPerformanceBaselineComponentMetrics,
) {
    assert_float_close(
        baseline_metrics.mean_latency_seconds,
        reloaded_metrics.mean_latency_seconds,
    );
    assert_float_close(
        baseline_metrics.throughput_operations_per_second,
        reloaded_metrics.throughput_operations_per_second,
    );
    assert_eq!(
        baseline_metrics.measured_operation_count,
        reloaded_metrics.measured_operation_count
    );
}

fn assert_float_close(left: f64, right: f64) {
    let tolerance = 1.0e-12_f64 * left.abs().max(right.abs()).max(1.0);
    assert!(
        (left - right).abs() <= tolerance,
        "expected {left} and {right} to match within tolerance {tolerance}",
    );
}
