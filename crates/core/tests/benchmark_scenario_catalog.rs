mod support;

use std::fs;

use janiform_core::benchmarking::{
    BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_ALGORITHM_POLICY,
    BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_SCOPE_POLICY,
    BENCHMARK_SCENARIO_CATALOG_FORWARD_EXTENSION_POLICY,
    BENCHMARK_SCENARIO_CATALOG_PROFILE_LOADING_ORDER_POLICY,
    BENCHMARK_SCENARIO_CATALOG_SCHEMA_FAMILY, BENCHMARK_SCENARIO_CATALOG_SCHEMA_GENERATION,
    BENCHMARK_SCENARIO_CATALOG_SCHEMA_VERSION, BenchmarkWorkloadScale,
    load_benchmark_scenario_catalog_from_json,
};
use serde_json::{Value, json};

use support::validation_fixture::fixtures_root_path;

#[test]
fn benchmark_scenario_catalog_parses_valid_fixture_profiles() {
    let catalog_payload = load_benchmark_scenario_catalog_fixture_payload();
    let catalog = load_benchmark_scenario_catalog_from_json(&catalog_payload)
        .expect("benchmark scenario catalog fixture should parse");

    assert_eq!(
        catalog.schema_version,
        BENCHMARK_SCENARIO_CATALOG_SCHEMA_VERSION
    );
    assert_eq!(
        catalog.schema_metadata.schema_family,
        BENCHMARK_SCENARIO_CATALOG_SCHEMA_FAMILY
    );
    assert_eq!(
        catalog.schema_metadata.schema_generation,
        BENCHMARK_SCENARIO_CATALOG_SCHEMA_GENERATION
    );
    assert_eq!(
        catalog.schema_metadata.forward_extension_policy,
        BENCHMARK_SCENARIO_CATALOG_FORWARD_EXTENSION_POLICY
    );
    assert_eq!(
        catalog.determinism_metadata.profile_loading_order,
        BENCHMARK_SCENARIO_CATALOG_PROFILE_LOADING_ORDER_POLICY
    );
    assert_eq!(
        catalog.determinism_metadata.content_hash_scope,
        BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_SCOPE_POLICY
    );
    assert_eq!(
        catalog.determinism_metadata.content_hash_algorithm,
        BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_ALGORITHM_POLICY
    );

    assert_eq!(catalog.profiles.len(), 3);

    let observed_scales: Vec<BenchmarkWorkloadScale> = catalog
        .profiles
        .iter()
        .map(|profile| profile.workload_scale)
        .collect();
    assert_eq!(
        observed_scales,
        vec![
            BenchmarkWorkloadScale::Small,
            BenchmarkWorkloadScale::Medium,
            BenchmarkWorkloadScale::Large,
        ]
    );

    for profile in &catalog.profiles {
        assert!(!profile.description.trim().is_empty());
        assert!(!profile.tags.is_empty());
        assert!(profile.interval_settings.interval_delta > 0.0);
        assert!(profile.interval_settings.horizon_interval_count > 0);
        assert!(profile.interval_settings.oracle_solver_step_count > 0);
        assert!(profile.portfolio_shape.underlying_count > 0);
        assert!(profile.portfolio_shape.option_position_count > 0);
        assert!(profile.run_length.warmup_iteration_count > 0);
        assert!(profile.run_length.measured_iteration_count > 0);
        assert!(profile.component_workload.oracle_requests_per_iteration > 0);
        assert!(profile.component_workload.pricing_requests_per_iteration > 0);
        assert!(profile.component_workload.risk_paths_per_iteration > 0);
    }
}

#[test]
fn benchmark_scenario_catalog_loader_rejects_invalid_profile_interval_delta() {
    let catalog_payload = load_benchmark_scenario_catalog_fixture_payload();
    let mut catalog_value: Value =
        serde_json::from_str(&catalog_payload).expect("fixture payload should parse as JSON value");

    catalog_value["profiles"][1]["interval_settings"]["interval_delta"] = json!(0.0);

    let mutated_payload = serde_json::to_string(&catalog_value)
        .expect("mutated benchmark scenario catalog payload should serialize");
    let error_text = load_benchmark_scenario_catalog_from_json(&mutated_payload)
        .expect_err("interval_delta equal to zero must fail")
        .to_string();

    assert!(error_text.contains("medium_hourly_balanced"));
    assert!(error_text.contains("interval_settings.interval_delta"));
}

#[test]
fn benchmark_scenario_catalog_loader_rejects_missing_required_scale_coverage() {
    let catalog_payload = load_benchmark_scenario_catalog_fixture_payload();
    let mut catalog_value: Value =
        serde_json::from_str(&catalog_payload).expect("fixture payload should parse as JSON value");

    let filtered_profiles: Vec<Value> = catalog_value["profiles"]
        .as_array()
        .expect("profiles should be an array")
        .iter()
        .filter(|profile| profile["workload_scale"] != "large")
        .cloned()
        .collect();
    catalog_value["profiles"] = Value::Array(filtered_profiles);

    let mutated_payload = serde_json::to_string(&catalog_value)
        .expect("mutated benchmark scenario catalog payload should serialize");
    let error_text = load_benchmark_scenario_catalog_from_json(&mutated_payload)
        .expect_err("missing large profile must fail")
        .to_string();

    assert!(error_text.contains("missing: large"));
}

#[test]
fn benchmark_scenario_catalog_loader_applies_deterministic_profile_ordering() {
    let catalog_payload = load_benchmark_scenario_catalog_fixture_payload();
    let baseline_catalog = load_benchmark_scenario_catalog_from_json(&catalog_payload)
        .expect("baseline fixture payload should parse");

    let mut reversed_value: Value =
        serde_json::from_str(&catalog_payload).expect("fixture payload should parse as JSON value");
    reversed_value["profiles"]
        .as_array_mut()
        .expect("profiles should be an array")
        .reverse();

    let reversed_payload = serde_json::to_string(&reversed_value)
        .expect("reversed benchmark scenario catalog payload should serialize");
    let reordered_catalog = load_benchmark_scenario_catalog_from_json(&reversed_payload)
        .expect("reversed payload should still parse via canonical ordering");

    let baseline_profile_ids: Vec<&str> = baseline_catalog
        .profiles
        .iter()
        .map(|profile| profile.profile_id.as_str())
        .collect();
    let reordered_profile_ids: Vec<&str> = reordered_catalog
        .profiles
        .iter()
        .map(|profile| profile.profile_id.as_str())
        .collect();

    assert_eq!(baseline_catalog, reordered_catalog);
    assert_eq!(baseline_profile_ids, reordered_profile_ids);
    assert_eq!(
        baseline_profile_ids,
        vec![
            "small_hourly_baseline",
            "medium_hourly_balanced",
            "large_hourly_portfolio",
        ]
    );
}

fn load_benchmark_scenario_catalog_fixture_payload() -> String {
    let fixture_path = fixtures_root_path()
        .join("benchmarks")
        .join("benchmark_scenario_catalog.json");

    fs::read_to_string(&fixture_path).unwrap_or_else(|source| {
        panic!(
            "failed to read benchmark scenario catalog fixture `{}`: {source}",
            fixture_path.display()
        )
    })
}
