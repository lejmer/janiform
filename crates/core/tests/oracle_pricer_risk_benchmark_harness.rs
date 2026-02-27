mod support;

use std::collections::BTreeMap;
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
    ORACLE_PRICER_RISK_BENCHMARK_POLICY_SCHEMA_VERSION,
    ORACLE_PRICER_RISK_BENCHMARK_REPORT_SCHEMA_VERSION, OraclePricerRiskBenchmarkExecutionConfig,
    OraclePricerRiskBenchmarkHarnessError, load_oracle_pricer_risk_benchmark_policy_from_json,
    run_oracle_pricer_risk_benchmark_harness,
};
use janiform_core::hawkes::HawkesExcitationState;
use serde_json::Value;

use support::validation_fixture::fixtures_root_path;

#[test]
fn benchmark_policy_fixture_is_data_driven_and_documents_rationale() {
    let policy_payload = load_policy_fixture_payload();
    let policy = load_oracle_pricer_risk_benchmark_policy_from_json(&policy_payload)
        .expect("benchmark policy fixture should parse");

    assert_eq!(
        policy.schema_version,
        ORACLE_PRICER_RISK_BENCHMARK_POLICY_SCHEMA_VERSION
    );
    assert!(!policy.policy_id.trim().is_empty());
    assert!(!policy.policy_version.trim().is_empty());
    assert!(!policy.rationale.model_parameters.trim().is_empty());
    assert!(!policy.rationale.oracle_solver_probes.trim().is_empty());
    assert!(!policy.rationale.pricing_input_ranges.trim().is_empty());
    assert!(!policy.rationale.seed_partition.trim().is_empty());
}

#[test]
fn benchmark_harness_output_schema_is_stable_and_machine_readable() {
    let catalog = build_bounded_catalog_fixture();
    let benchmark_policy = load_benchmark_policy_fixture();
    let execution_config = OraclePricerRiskBenchmarkExecutionConfig {
        warmup_iteration_count_override: Some(0),
        measured_iteration_count_override: Some(1),
        reproducibility_seed: 17,
    };

    let report =
        run_oracle_pricer_risk_benchmark_harness(&catalog, &benchmark_policy, execution_config)
            .expect("benchmark harness should run with bounded fixture catalog");

    assert_eq!(
        report.schema_version,
        ORACLE_PRICER_RISK_BENCHMARK_REPORT_SCHEMA_VERSION
    );
    assert_eq!(report.profile_reports.len(), 3);

    let serialized_report =
        serde_json::to_value(&report).expect("benchmark harness report should serialize");
    let serialized_report_object = serialized_report
        .as_object()
        .expect("serialized benchmark report should be an object");
    let output_schema_metadata = serialized_report["output_schema_metadata"]
        .as_object()
        .expect("output_schema_metadata should be present");
    let required_report_fields = output_schema_metadata["required_report_fields"]
        .as_array()
        .expect("required_report_fields should be present");
    let required_profile_report_fields = output_schema_metadata["required_profile_report_fields"]
        .as_array()
        .expect("required_profile_report_fields should be present");
    let required_component_metrics_fields = output_schema_metadata
        .get("required_component_metrics_fields")
        .and_then(Value::as_array)
        .expect("required_component_metrics_fields should be present");

    for required_field in required_report_fields {
        let required_field_name = required_field
            .as_str()
            .expect("required report field names should be strings");
        assert!(
            serialized_report_object.contains_key(required_field_name),
            "benchmark report is missing required field `{required_field_name}`",
        );
    }

    for profile_report in serialized_report["profile_reports"]
        .as_array()
        .expect("profile_reports should be an array")
    {
        let profile_report_object = profile_report
            .as_object()
            .expect("profile report should be an object");
        for required_field in required_profile_report_fields {
            let required_field_name = required_field
                .as_str()
                .expect("required profile-report field names should be strings");
            assert!(
                profile_report_object.contains_key(required_field_name),
                "profile report is missing required field `{required_field_name}`",
            );
        }

        for component_field in [
            "oracle_solver_metrics",
            "pricing_metrics",
            "risk_sampling_metrics",
        ] {
            let component_metrics = profile_report[component_field]
                .as_object()
                .expect("component metrics should be objects");
            for required_field in required_component_metrics_fields {
                let required_field_name = required_field
                    .as_str()
                    .expect("required component-metrics field names should be strings");
                assert!(
                    component_metrics.contains_key(required_field_name),
                    "component metrics `{component_field}` is missing required field `{required_field_name}`",
                );
            }
        }
    }
}

#[test]
fn benchmark_harness_validates_config_and_invocation_wiring_per_component() {
    let catalog = build_bounded_catalog_fixture();
    let benchmark_policy = load_benchmark_policy_fixture();
    let invalid_config = OraclePricerRiskBenchmarkExecutionConfig {
        warmup_iteration_count_override: Some(0),
        measured_iteration_count_override: Some(0),
        reproducibility_seed: 5,
    };

    let invalid_error =
        run_oracle_pricer_risk_benchmark_harness(&catalog, &benchmark_policy, invalid_config)
            .expect_err("measured_iteration_count_override equal to zero must fail");
    assert!(matches!(
        invalid_error,
        OraclePricerRiskBenchmarkHarnessError::InvalidExecutionConfig { .. }
    ));

    let execution_config = OraclePricerRiskBenchmarkExecutionConfig {
        warmup_iteration_count_override: Some(1),
        measured_iteration_count_override: Some(2),
        reproducibility_seed: 5,
    };
    let report =
        run_oracle_pricer_risk_benchmark_harness(&catalog, &benchmark_policy, execution_config)
            .expect("benchmark harness should run with bounded fixture catalog");

    let profile_report_by_id: BTreeMap<&str, _> = report
        .profile_reports
        .iter()
        .map(|profile_report| (profile_report.profile_id.as_str(), profile_report))
        .collect();

    let mut expected_oracle_total_operation_count: usize = 0;
    let mut expected_pricing_total_operation_count: usize = 0;
    let mut expected_risk_total_operation_count: usize = 0;

    for profile in &catalog.profiles {
        let profile_report = profile_report_by_id
            .get(profile.profile_id.as_str())
            .expect("profile report should exist for each profile");

        let expected_oracle_measured_operation_count =
            2 * profile.component_workload.oracle_requests_per_iteration;
        let expected_pricing_measured_operation_count =
            2 * profile.component_workload.pricing_requests_per_iteration;
        let expected_risk_measured_operation_count =
            2 * profile.component_workload.risk_paths_per_iteration;

        assert_eq!(profile_report.warmup_iteration_count, 1);
        assert_eq!(profile_report.measured_iteration_count, 2);
        assert_eq!(
            profile_report
                .oracle_solver_metrics
                .measured_operation_count,
            expected_oracle_measured_operation_count,
        );
        assert_eq!(
            profile_report.pricing_metrics.measured_operation_count,
            expected_pricing_measured_operation_count,
        );
        assert_eq!(
            profile_report
                .risk_sampling_metrics
                .measured_operation_count,
            expected_risk_measured_operation_count,
        );
        assert_eq!(
            profile_report
                .oracle_solver_metrics
                .invocation_count_including_warmup,
            profile.component_workload.oracle_requests_per_iteration * 3,
        );
        assert_eq!(
            profile_report
                .pricing_metrics
                .invocation_count_including_warmup,
            profile.component_workload.pricing_requests_per_iteration * 3,
        );
        assert_eq!(
            profile_report
                .risk_sampling_metrics
                .invocation_count_including_warmup,
            profile.component_workload.risk_paths_per_iteration * 3,
        );

        expected_oracle_total_operation_count += expected_oracle_measured_operation_count;
        expected_pricing_total_operation_count += expected_pricing_measured_operation_count;
        expected_risk_total_operation_count += expected_risk_measured_operation_count;
    }

    assert_eq!(
        report.summary.oracle_solver_total_operation_count,
        expected_oracle_total_operation_count,
    );
    assert_eq!(
        report.summary.pricing_total_operation_count,
        expected_pricing_total_operation_count,
    );
    assert_eq!(
        report.summary.risk_sampling_total_operation_count,
        expected_risk_total_operation_count,
    );
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
        description: "Bounded benchmark profile for harness validation tests.".to_string(),
    }
}

fn load_benchmark_policy_fixture() -> janiform_core::benchmarking::OraclePricerRiskBenchmarkPolicy {
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
