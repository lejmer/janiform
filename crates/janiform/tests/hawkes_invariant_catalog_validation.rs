mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use support::hawkes_invariant_catalog::{
    HAWKES_INVARIANT_CATALOG_SCHEMA_VERSION, HAWKES_INVARIANT_CATEGORY_BOUNDARY_BEHAVIOR,
    HAWKES_INVARIANT_CATEGORY_CONSTRAINTS, HAWKES_INVARIANT_CATEGORY_DYNAMICS,
    HAWKES_INVARIANT_CATEGORY_SAMPLING_PROPERTIES, HAWKES_INVARIANT_CATEGORY_SCALE_CONSISTENCY,
    HAWKES_REQUIRED_INVARIANT_CLASS_COARSE_AGGREGATED_SCALE_CONSISTENCY,
    HAWKES_REQUIRED_INVARIANT_CLASS_NON_GRID_EVENT_TIME_SEMANTICS, HawkesInvariantCatalogDocument,
    HawkesInvariantCheckClass, assert_hawkes_invariant_catalog_ids_have_check_class,
    collect_invariant_ids_for_suite_test, load_hawkes_invariant_catalog_document,
    validate_hawkes_invariant_catalog_document,
};
use support::validation_fixture::fixtures_root_path;

struct SuiteTestCatalogReference {
    test_name: &'static str,
    catalog_invariant_ids: &'static [&'static str],
    check_class: HawkesInvariantCheckClass,
}

const HAWKES_EQUATION_SUITE_ID: &str = "hawkes_equation_reference_suite";
const HAWKES_CONSTRAINTS_SUITE_ID: &str = "hawkes_constraints_invariant_suite";
const SEEDED_BEHAVIOR_SUITE_ID: &str = "seeded_tick_behavior_validation";
const CLUSTER_SIGNATURE_STATISTICAL_SUITE_ID: &str =
    "hawkes_cluster_signature_statistical_regression";

const HAWKES_EQUATION_TEST_REFERENCES: [SuiteTestCatalogReference; 4] = [
    SuiteTestCatalogReference {
        test_name: "hawkes_reference_scenarios_match_expected_checkpoints",
        catalog_invariant_ids: &["HK-DYN-001"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
    SuiteTestCatalogReference {
        test_name: "hawkes_reference_mismatch_diagnostics_include_step_and_component_identifiers",
        catalog_invariant_ids: &["HK-DYN-002"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
    SuiteTestCatalogReference {
        test_name: "irregular_timestamp_transition_schedule_matches_expected_checkpoints",
        catalog_invariant_ids: &["HK-DYN-001"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
    SuiteTestCatalogReference {
        test_name: "seeded_hawkes_transition_schedule_and_state_path_are_repeatable",
        catalog_invariant_ids: &["HK-BND-001"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
];

const SEEDED_BEHAVIOR_TEST_REFERENCES: [SuiteTestCatalogReference; 8] = [
    SuiteTestCatalogReference {
        test_name: "seeded_behavior_validation_is_strictly_deterministic_for_repeated_seeded_runs",
        catalog_invariant_ids: &["HK-BND-002"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
    SuiteTestCatalogReference {
        test_name: "seeded_behavior_validation_preserves_full_4d_state_carryover_across_multi_hour_boundaries",
        catalog_invariant_ids: &["HK-BND-003"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
    SuiteTestCatalogReference {
        test_name: "seeded_behavior_validation_boundary_adjacent_scenarios_are_repeatable_with_fixed_seed",
        catalog_invariant_ids: &["HK-BND-003"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
    SuiteTestCatalogReference {
        test_name: "seeded_behavior_validation_tied_timestamps_follow_documented_event_ordering_policy",
        catalog_invariant_ids: &["HK-BND-004"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
    SuiteTestCatalogReference {
        test_name: "seeded_behavior_validation_near_tied_timestamps_follow_timestamp_priority_ordering",
        catalog_invariant_ids: &["HK-BND-004"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
    SuiteTestCatalogReference {
        test_name: "seeded_behavior_validation_exhibits_clustering_and_non_grid_event_time_signatures",
        catalog_invariant_ids: &["HK-SAMP-001"],
        check_class: HawkesInvariantCheckClass::StatisticalProperty,
    },
    SuiteTestCatalogReference {
        test_name: "seeded_behavior_validation_exhibits_expected_buy_sell_asymmetry",
        catalog_invariant_ids: &["HK-SAMP-002"],
        check_class: HawkesInvariantCheckClass::StatisticalProperty,
    },
    SuiteTestCatalogReference {
        test_name: "seeded_behavior_validation_compares_aggregated_fine_and_direct_coarse_summaries",
        catalog_invariant_ids: &["HK-SCALE-001"],
        check_class: HawkesInvariantCheckClass::StatisticalProperty,
    },
];

const CLUSTER_SIGNATURE_STATISTICAL_TEST_REFERENCES: [SuiteTestCatalogReference; 6] = [
    SuiteTestCatalogReference {
        test_name: "cluster_signature_metric_calculation_distinguishes_synthetic_clustered_and_non_clustered_fixtures",
        catalog_invariant_ids: &["HK-SAMP-001"],
        check_class: HawkesInvariantCheckClass::StatisticalProperty,
    },
    SuiteTestCatalogReference {
        test_name: "cluster_signature_batch_metrics_are_reproducible_with_fixed_seed_batches",
        catalog_invariant_ids: &["HK-SAMP-001"],
        check_class: HawkesInvariantCheckClass::StatisticalProperty,
    },
    SuiteTestCatalogReference {
        test_name: "cluster_signature_regression_reference_bands_pass_for_seeded_batches",
        catalog_invariant_ids: &["HK-SAMP-001"],
        check_class: HawkesInvariantCheckClass::StatisticalProperty,
    },
    SuiteTestCatalogReference {
        test_name: "cluster_signature_regression_reports_metric_deltas_when_bands_are_violated",
        catalog_invariant_ids: &["HK-SAMP-001"],
        check_class: HawkesInvariantCheckClass::StatisticalProperty,
    },
    SuiteTestCatalogReference {
        test_name: "cluster_signature_scale_consistency_reference_bands_pass_for_seeded_horizon_panel",
        catalog_invariant_ids: &["HK-SCALE-001"],
        check_class: HawkesInvariantCheckClass::StatisticalProperty,
    },
    SuiteTestCatalogReference {
        test_name: "cluster_signature_scale_consistency_reports_metric_deltas_when_bands_are_violated",
        catalog_invariant_ids: &["HK-SCALE-001"],
        check_class: HawkesInvariantCheckClass::StatisticalProperty,
    },
];

const HAWKES_CONSTRAINTS_TEST_REFERENCES: [SuiteTestCatalogReference; 3] = [
    SuiteTestCatalogReference {
        test_name: "constraint_projection_sanity_preserves_positive_intensity_domains",
        catalog_invariant_ids: &["HK-CON-001"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
    SuiteTestCatalogReference {
        test_name: "constraint_stability_boundary_regimes_remain_constructible_and_stable",
        catalog_invariant_ids: &["HK-CON-001"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
    SuiteTestCatalogReference {
        test_name: "constraint_transition_steps_preserve_non_negative_state_increments",
        catalog_invariant_ids: &["HK-CON-001"],
        check_class: HawkesInvariantCheckClass::DeterministicEquation,
    },
];

#[test]
fn hawkes_invariant_catalog_fixture_is_valid_and_contains_required_groupings() {
    let catalog_document = load_hawkes_invariant_catalog_document()
        .expect("hawkes invariant catalog fixture should load");

    assert_eq!(
        catalog_document.schema_version,
        HAWKES_INVARIANT_CATALOG_SCHEMA_VERSION
    );

    let observed_category_ids: BTreeSet<&str> = catalog_document
        .categories
        .iter()
        .map(|category| category.category_id.as_str())
        .collect();

    assert!(observed_category_ids.contains(HAWKES_INVARIANT_CATEGORY_CONSTRAINTS));
    assert!(observed_category_ids.contains(HAWKES_INVARIANT_CATEGORY_DYNAMICS));
    assert!(observed_category_ids.contains(HAWKES_INVARIANT_CATEGORY_BOUNDARY_BEHAVIOR));
    assert!(observed_category_ids.contains(HAWKES_INVARIANT_CATEGORY_SAMPLING_PROPERTIES));
    assert!(observed_category_ids.contains(HAWKES_INVARIANT_CATEGORY_SCALE_CONSISTENCY));

    let observed_invariant_classes: BTreeSet<&str> = catalog_document
        .invariants
        .iter()
        .map(|invariant| invariant.invariant_class.as_str())
        .collect();

    assert!(
        observed_invariant_classes
            .contains(HAWKES_REQUIRED_INVARIANT_CLASS_NON_GRID_EVENT_TIME_SEMANTICS)
    );
    assert!(
        observed_invariant_classes
            .contains(HAWKES_REQUIRED_INVARIANT_CLASS_COARSE_AGGREGATED_SCALE_CONSISTENCY)
    );

    assert!(
        catalog_document
            .invariants
            .iter()
            .all(|invariant| !invariant.rationale.trim().is_empty())
    );
}

#[test]
fn hawkes_invariant_catalog_rejects_missing_required_fields() {
    let fixture_path = hawkes_invariant_catalog_fixture_path();
    let fixture_raw =
        fs::read_to_string(&fixture_path).expect("hawkes invariant catalog fixture should exist");

    let mut fixture_value: Value =
        serde_json::from_str(&fixture_raw).expect("hawkes invariant catalog fixture should parse");
    fixture_value
        .as_object_mut()
        .expect("hawkes invariant catalog root should be an object")
        .remove("invariants");

    let parse_result = serde_json::from_value::<HawkesInvariantCatalogDocument>(fixture_value);

    assert!(parse_result.is_err());
}

#[test]
fn hawkes_invariant_catalog_rejects_invalid_check_class_values() {
    let fixture_document = load_hawkes_invariant_catalog_fixture_as_value();

    let mut mutated_fixture_document = fixture_document;
    mutated_fixture_document["invariants"][0]["check_class"] = Value::String("mixed".to_string());

    let parse_result =
        serde_json::from_value::<HawkesInvariantCatalogDocument>(mutated_fixture_document);
    assert!(parse_result.is_err());
}

#[test]
fn hawkes_invariant_catalog_rejects_duplicate_invariant_ids() {
    let fixture_document = load_hawkes_invariant_catalog_fixture_as_value();

    let mut mutated_fixture_document = fixture_document;
    mutated_fixture_document["invariants"][1]["invariant_id"] =
        Value::String("HK-CON-001".to_string());

    let parsed_fixture_document =
        serde_json::from_value::<HawkesInvariantCatalogDocument>(mutated_fixture_document)
            .expect("duplicate invariant id fixture should still parse");

    let validation_result = validate_hawkes_invariant_catalog_document(&parsed_fixture_document);
    assert!(validation_result.is_err());
}

#[test]
fn hawkes_invariant_catalog_rejects_missing_required_invariant_classes() {
    let fixture_document = load_hawkes_invariant_catalog_fixture_as_value();

    let mut mutated_fixture_document = fixture_document;

    let invariant_entries = mutated_fixture_document["invariants"]
        .as_array_mut()
        .expect("invariants should serialize as an array");
    let non_grid_invariant_entry = invariant_entries
        .iter_mut()
        .find(|invariant_entry| invariant_entry["invariant_id"].as_str() == Some("HK-SAMP-001"))
        .expect("fixture should include HK-SAMP-001");

    non_grid_invariant_entry["invariant_class"] = Value::String("clustering_signature".to_string());

    let parsed_fixture_document =
        serde_json::from_value::<HawkesInvariantCatalogDocument>(mutated_fixture_document)
            .expect("fixture with missing required invariant class should still parse");

    let validation_result = validate_hawkes_invariant_catalog_document(&parsed_fixture_document);
    assert!(validation_result.is_err());
}

#[test]
fn hawkes_suite_test_reference_matrix_is_complete_and_catalog_linked() {
    let catalog_document = load_hawkes_invariant_catalog_document()
        .expect("hawkes invariant catalog fixture should load");

    assert_suite_reference_matrix_covers_all_suite_tests(
        &catalog_document,
        HAWKES_EQUATION_SUITE_ID,
        &HAWKES_EQUATION_TEST_REFERENCES,
        test_suite_source_path("hawkes_equation_reference_suite.rs"),
    );
    assert_suite_reference_matrix_covers_all_suite_tests(
        &catalog_document,
        HAWKES_CONSTRAINTS_SUITE_ID,
        &HAWKES_CONSTRAINTS_TEST_REFERENCES,
        test_suite_source_path("hawkes_constraints_invariant_suite.rs"),
    );
    assert_suite_reference_matrix_covers_all_suite_tests(
        &catalog_document,
        SEEDED_BEHAVIOR_SUITE_ID,
        &SEEDED_BEHAVIOR_TEST_REFERENCES,
        test_suite_source_path("seeded_tick_behavior_validation.rs"),
    );
    assert_suite_reference_matrix_covers_all_suite_tests(
        &catalog_document,
        CLUSTER_SIGNATURE_STATISTICAL_SUITE_ID,
        &CLUSTER_SIGNATURE_STATISTICAL_TEST_REFERENCES,
        test_suite_source_path("hawkes_cluster_signature_statistical_regression.rs"),
    );
}

fn assert_suite_reference_matrix_covers_all_suite_tests(
    catalog_document: &HawkesInvariantCatalogDocument,
    suite_id: &str,
    expected_references: &[SuiteTestCatalogReference],
    suite_source_path: PathBuf,
) {
    let source_payload = fs::read_to_string(&suite_source_path).unwrap_or_else(|source| {
        panic!(
            "failed to read suite source `{}`: {source}",
            suite_source_path.display()
        )
    });
    let observed_test_count = source_payload
        .lines()
        .filter(|line| line.trim() == "#[test]")
        .count();

    assert_eq!(
        expected_references.len(),
        observed_test_count,
        "suite reference matrix out of sync with test declarations for suite {suite_id}"
    );

    for expected_reference in expected_references {
        assert_hawkes_invariant_catalog_ids_have_check_class(
            expected_reference.catalog_invariant_ids,
            expected_reference.check_class,
        );
        let expected_invariant_id_set: BTreeSet<String> = expected_reference
            .catalog_invariant_ids
            .iter()
            .map(|invariant_id| (*invariant_id).to_string())
            .collect();
        let catalog_reference_id_set: BTreeSet<String> = collect_invariant_ids_for_suite_test(
            catalog_document,
            suite_id,
            expected_reference.test_name,
        )
        .into_iter()
        .collect();

        assert!(
            !expected_invariant_id_set.is_empty(),
            "test reference mapping must include at least one catalog id: suite_id={suite_id} test_name={}",
            expected_reference.test_name
        );
        assert_eq!(
            expected_invariant_id_set, catalog_reference_id_set,
            "suite reference mapping diverged from catalog for suite_id={suite_id} test_name={}",
            expected_reference.test_name
        );
    }
}

fn hawkes_invariant_catalog_fixture_path() -> PathBuf {
    fixtures_root_path()
        .join("hawkes")
        .join("hawkes_invariant_catalog.json")
}

fn load_hawkes_invariant_catalog_fixture_as_value() -> Value {
    let fixture_path = hawkes_invariant_catalog_fixture_path();
    let fixture_raw =
        fs::read_to_string(&fixture_path).expect("hawkes invariant catalog fixture should exist");

    serde_json::from_str::<Value>(&fixture_raw)
        .expect("hawkes invariant catalog fixture should be valid JSON")
}

fn test_suite_source_path(file_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(file_name)
}
