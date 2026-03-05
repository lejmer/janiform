mod support;

use std::collections::BTreeSet;
use std::fs;

use serde_json::Value;

use support::validation_fixture::{
    CalendarRegressionFixtureDocument, HourlyFactorRegressionFixtureDocument,
    OPEN_CLOSED_GAP_BEHAVIOR_SCENARIO_FIXTURE_SET_ID, OpenClosedGapBehaviorScenarioFixtureDocument,
    ValidationFixtureManifestDocument, fixtures_root_path,
    load_calendar_regression_fixture_from_manifest,
    load_hourly_factor_regression_fixture_from_manifest, load_manifest_entry_payload,
    load_open_closed_gap_behavior_scenario_fixture_from_manifest,
    load_validation_fixture_manifest_document, sorted_manifest_entries,
    validate_calendar_regression_fixture_document,
    validate_hourly_factor_regression_fixture_document,
    validate_open_closed_gap_behavior_calendar_integration,
    validate_open_closed_gap_behavior_scenario_fixture_document,
    validate_validation_fixture_manifest_document,
};
use support::stylized_fact_regression_fixture::{
    STYLIZED_FACT_METRIC_REGRESSION_FIXTURE_SET_ID, StylizedFactMetricRegressionFixtureDocument,
    load_stylized_fact_metric_regression_fixture_from_manifest,
    validate_stylized_fact_metric_regression_fixture_document,
};

#[test]
fn validation_fixture_manifest_loading_order_and_content_hashes_are_deterministic() {
    let manifest = load_validation_fixture_manifest_document()
        .expect("validation fixture manifest should load");

    let first_sorted_entries = sorted_manifest_entries(&manifest);
    let second_sorted_entries = sorted_manifest_entries(&manifest);

    let fixtures_root = fixtures_root_path();
    let first_snapshot: Vec<(String, String)> = first_sorted_entries
        .iter()
        .map(|manifest_entry| {
            let loaded_payload = load_manifest_entry_payload(&fixtures_root, manifest_entry)
                .expect("manifest fixture payload should load and pass hash checks");
            (
                loaded_payload.fixture_id,
                loaded_payload.observed_content_hash,
            )
        })
        .collect();
    let second_snapshot: Vec<(String, String)> = second_sorted_entries
        .iter()
        .map(|manifest_entry| {
            let loaded_payload = load_manifest_entry_payload(&fixtures_root, manifest_entry)
                .expect("manifest fixture payload should load and pass hash checks");
            (
                loaded_payload.fixture_id,
                loaded_payload.observed_content_hash,
            )
        })
        .collect();

    assert_eq!(first_snapshot, second_snapshot);
}

#[test]
fn calendar_regression_fixture_document_is_valid_for_multiple_exchanges() {
    let loaded_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from shared manifest");

    let exchange_codes: BTreeSet<String> = loaded_fixture
        .fixture_document
        .calendar_registry
        .exchanges
        .iter()
        .map(|exchange| exchange.exchange_code.clone())
        .collect();

    assert!(exchange_codes.len() >= 2);
    assert!(
        loaded_fixture
            .manifest_entry
            .exchange_codes
            .iter()
            .all(|exchange_code| exchange_codes.contains(exchange_code))
    );
}

#[test]
fn hourly_factor_regression_fixture_document_is_valid_for_manifest_exchange_coverage() {
    let loaded_fixture = load_hourly_factor_regression_fixture_from_manifest()
        .expect("hourly factor regression fixture should load from shared manifest");

    assert!(
        loaded_fixture
            .manifest_entry
            .exchange_codes
            .contains(&"XNYS".to_string())
    );
}

#[test]
fn open_closed_gap_behavior_fixture_document_is_valid_for_manifest_exchange_coverage() {
    let loaded_fixture = load_open_closed_gap_behavior_scenario_fixture_from_manifest()
        .expect("open and closed gap behavior fixture should load from shared manifest");

    assert_eq!(
        loaded_fixture.manifest_entry.fixture_id,
        OPEN_CLOSED_GAP_BEHAVIOR_SCENARIO_FIXTURE_SET_ID
    );
    assert!(
        loaded_fixture
            .manifest_entry
            .exchange_codes
            .contains(&"XNYS".to_string())
    );
    assert!(
        loaded_fixture
            .manifest_entry
            .exchange_codes
            .contains(&"XTKS".to_string())
    );
}

#[test]
fn stylized_fact_fixture_document_is_valid_for_manifest_exchange_coverage() {
    let loaded_fixture = load_stylized_fact_metric_regression_fixture_from_manifest()
        .expect("stylized fact regression fixture should load from shared manifest");

    assert_eq!(
        loaded_fixture.manifest_entry.fixture_id,
        STYLIZED_FACT_METRIC_REGRESSION_FIXTURE_SET_ID
    );
    assert!(
        loaded_fixture
            .manifest_entry
            .exchange_codes
            .contains(&"XNYS".to_string())
    );
}

#[test]
fn open_closed_gap_behavior_fixture_loading_order_and_content_hash_are_deterministic() {
    let loaded_fixture = load_open_closed_gap_behavior_scenario_fixture_from_manifest()
        .expect("open and closed gap behavior fixture should load from shared manifest");
    let fixture_payload = loaded_fixture.loaded_payload.payload_bytes.clone();

    let first_document =
        serde_json::from_slice::<OpenClosedGapBehaviorScenarioFixtureDocument>(&fixture_payload)
            .expect("open and closed gap behavior fixture should parse");
    let second_document =
        serde_json::from_slice::<OpenClosedGapBehaviorScenarioFixtureDocument>(&fixture_payload)
            .expect("open and closed gap behavior fixture should parse");
    let first_case_order: Vec<String> = first_document
        .cases
        .iter()
        .map(|fixture_case| fixture_case.fixture_id.clone())
        .collect();
    let second_case_order: Vec<String> = second_document
        .cases
        .iter()
        .map(|fixture_case| fixture_case.fixture_id.clone())
        .collect();
    let first_event_order: Vec<String> = first_document
        .cases
        .iter()
        .flat_map(|fixture_case| {
            fixture_case
                .injected_events
                .iter()
                .map(|event| event.event_id.clone())
        })
        .collect();
    let second_event_order: Vec<String> = second_document
        .cases
        .iter()
        .flat_map(|fixture_case| {
            fixture_case
                .injected_events
                .iter()
                .map(|event| event.event_id.clone())
        })
        .collect();
    let first_content_hash = support::validation_fixture::compute_content_hash_hex(
        loaded_fixture.loaded_payload.payload_bytes.as_slice(),
    );
    let second_content_hash = support::validation_fixture::compute_content_hash_hex(
        loaded_fixture.loaded_payload.payload_bytes.as_slice(),
    );

    assert_eq!(first_case_order, second_case_order);
    assert_eq!(first_event_order, second_event_order);
    assert_eq!(first_content_hash, second_content_hash);
}

#[test]
fn stylized_fact_fixture_loading_order_and_content_hash_are_deterministic() {
    let loaded_fixture = load_stylized_fact_metric_regression_fixture_from_manifest()
        .expect("stylized fact regression fixture should load from shared manifest");
    let fixture_payload = loaded_fixture.loaded_payload.payload_bytes.clone();

    let first_document =
        serde_json::from_slice::<StylizedFactMetricRegressionFixtureDocument>(&fixture_payload)
            .expect("stylized fact regression fixture should parse");
    let second_document =
        serde_json::from_slice::<StylizedFactMetricRegressionFixtureDocument>(&fixture_payload)
            .expect("stylized fact regression fixture should parse");
    let first_case_order: Vec<String> = first_document
        .cases
        .iter()
        .map(|fixture_case| fixture_case.fixture_id.clone())
        .collect();
    let second_case_order: Vec<String> = second_document
        .cases
        .iter()
        .map(|fixture_case| fixture_case.fixture_id.clone())
        .collect();
    let first_content_hash = support::validation_fixture::compute_content_hash_hex(
        loaded_fixture.loaded_payload.payload_bytes.as_slice(),
    );
    let second_content_hash = support::validation_fixture::compute_content_hash_hex(
        loaded_fixture.loaded_payload.payload_bytes.as_slice(),
    );

    assert_eq!(first_case_order, second_case_order);
    assert_eq!(first_content_hash, second_content_hash);
}

#[test]
fn validation_fixture_manifest_rejects_missing_required_fields() {
    let fixtures_root = fixtures_root_path();
    let manifest_path = fixtures_root
        .join("validation")
        .join("validation_fixture_manifest.json");
    let manifest_raw =
        fs::read_to_string(&manifest_path).expect("validation fixture manifest file should exist");

    let mut manifest_value: Value =
        serde_json::from_str(&manifest_raw).expect("manifest fixture should be valid JSON");
    manifest_value
        .as_object_mut()
        .expect("manifest fixture root should be an object")
        .remove("fixtures");

    let parse_result = serde_json::from_value::<ValidationFixtureManifestDocument>(manifest_value);

    assert!(parse_result.is_err());
}

#[test]
fn validation_fixture_manifest_rejects_invalid_domain_values() {
    let fixtures_root = fixtures_root_path();
    let manifest_path = fixtures_root
        .join("validation")
        .join("validation_fixture_manifest.json");
    let manifest_raw =
        fs::read_to_string(&manifest_path).expect("validation fixture manifest file should exist");

    let mut manifest_value: Value =
        serde_json::from_str(&manifest_raw).expect("manifest fixture should be valid JSON");
    manifest_value["fixtures"][0]["domain"] = Value::String("python_only".to_string());

    let parse_result = serde_json::from_value::<ValidationFixtureManifestDocument>(manifest_value);

    assert!(parse_result.is_err());
}

#[test]
fn validation_fixture_manifest_rejects_malformed_content_hash_values() {
    let fixtures_root = fixtures_root_path();
    let manifest_path = fixtures_root
        .join("validation")
        .join("validation_fixture_manifest.json");
    let manifest_raw =
        fs::read_to_string(&manifest_path).expect("validation fixture manifest file should exist");

    let mut manifest_value: Value =
        serde_json::from_str(&manifest_raw).expect("manifest fixture should be valid JSON");
    manifest_value["fixtures"][0]["content_hash"] = Value::String("ABCDEF0123456789".to_string());

    let parsed_manifest =
        serde_json::from_value::<ValidationFixtureManifestDocument>(manifest_value)
            .expect("manifest with malformed content_hash should still parse");

    let validation_result = validate_validation_fixture_manifest_document(&parsed_manifest);

    assert!(validation_result.is_err());
}

#[test]
fn stylized_fact_fixture_rejects_missing_required_fields() {
    let loaded_fixture = load_stylized_fact_metric_regression_fixture_from_manifest()
        .expect("stylized fact regression fixture should load");
    let mut fixture_value =
        serde_json::to_value(loaded_fixture.fixture_document).expect("fixture should serialize");

    fixture_value
        .as_object_mut()
        .expect("fixture value should be an object")
        .remove("cases");

    let parse_result = serde_json::from_value::<StylizedFactMetricRegressionFixtureDocument>(
        fixture_value,
    );
    assert!(parse_result.is_err());
}

#[test]
fn stylized_fact_fixture_rejects_unknown_tolerance_policy_reference() {
    let loaded_fixture = load_stylized_fact_metric_regression_fixture_from_manifest()
        .expect("stylized fact regression fixture should load");
    let mut fixture_document = loaded_fixture.fixture_document;
    fixture_document.cases[0].tolerance_policy_id = "missing_policy.v1".to_string();

    let validation_result = validate_stylized_fact_metric_regression_fixture_document(&fixture_document);

    assert!(validation_result.is_err());
}

#[test]
fn calendar_regression_fixture_rejects_missing_required_fields() {
    let loaded_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("calendar regression fixture should be valid JSON");
    fixture_value
        .as_object_mut()
        .expect("calendar fixture root should be an object")
        .remove("cases");

    let parse_result = serde_json::from_value::<CalendarRegressionFixtureDocument>(fixture_value);

    assert!(parse_result.is_err());
}

#[test]
fn calendar_regression_fixture_rejects_invalid_expected_state_values() {
    let loaded_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("calendar regression fixture should be valid JSON");
    fixture_value["cases"][0]["expected_state"] = Value::String("invalid".to_string());

    let parsed_fixture = serde_json::from_value::<CalendarRegressionFixtureDocument>(fixture_value)
        .expect("calendar regression fixture with invalid expected_state should still parse");

    let validation_result = validate_calendar_regression_fixture_document(&parsed_fixture);

    assert!(validation_result.is_err());
}

#[test]
fn calendar_regression_fixture_rejects_duplicate_exchange_timestamp_pairs() {
    let loaded_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from shared manifest");
    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("calendar regression fixture should be valid JSON");
    let duplicate_case = fixture_value["cases"][0].clone();

    fixture_value["cases"]
        .as_array_mut()
        .expect("calendar fixture cases should be an array")
        .push(duplicate_case);
    fixture_value["cases"]
        .as_array_mut()
        .expect("calendar fixture cases should be an array")[0]["fixture_id"] =
        Value::String("duplicate_exchange_timestamp_case_original".to_string());
    fixture_value["cases"]
        .as_array_mut()
        .expect("calendar fixture cases should be an array")
        .last_mut()
        .expect("duplicate case should exist")["fixture_id"] =
        Value::String("duplicate_exchange_timestamp_case_duplicate".to_string());

    let parsed_fixture = serde_json::from_value::<CalendarRegressionFixtureDocument>(fixture_value)
        .expect("calendar regression fixture with duplicate exchange timestamp should still parse");

    let validation_result = validate_calendar_regression_fixture_document(&parsed_fixture);

    assert!(validation_result.is_err());
}

#[test]
fn calendar_regression_fixture_rejects_malformed_canonical_metadata() {
    let loaded_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("calendar regression fixture should be valid JSON");
    fixture_value["canonical_regression_metadata"]["stable_fixture_ids_by_tag"]["boundary"] =
        Value::Array(vec![Value::String("missing_boundary_fixture".to_string())]);

    let parsed_fixture = serde_json::from_value::<CalendarRegressionFixtureDocument>(fixture_value)
        .expect("calendar regression fixture with malformed canonical metadata should still parse");

    let validation_result = validate_calendar_regression_fixture_document(&parsed_fixture);

    assert!(validation_result.is_err());
}

#[test]
fn calendar_regression_fixture_rejects_incomplete_fixture_coverage_for_stable_tag_sets() {
    let loaded_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("calendar regression fixture should be valid JSON");
    fixture_value["cases"]
        .as_array_mut()
        .expect("calendar fixture cases should be an array")
        .remove(0);

    let parsed_fixture = serde_json::from_value::<CalendarRegressionFixtureDocument>(fixture_value)
        .expect("calendar regression fixture with missing stable case should still parse");

    let validation_result = validate_calendar_regression_fixture_document(&parsed_fixture);

    assert!(validation_result.is_err());
}

#[test]
fn hourly_factor_regression_fixture_rejects_malformed_canonical_metadata() {
    let loaded_fixture = load_hourly_factor_regression_fixture_from_manifest()
        .expect("hourly factor regression fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("hourly factor regression fixture should be valid JSON");
    fixture_value["canonical_regression_metadata"]["stable_canonical_output_fixture_ids"] =
        Value::Array(vec![Value::String("missing_fixture".to_string())]);

    let parsed_fixture =
        serde_json::from_value::<HourlyFactorRegressionFixtureDocument>(fixture_value).expect(
            "hourly factor regression fixture with malformed canonical metadata should still parse",
        );

    let validation_result = validate_hourly_factor_regression_fixture_document(&parsed_fixture);

    assert!(validation_result.is_err());
}

#[test]
fn hourly_factor_regression_fixture_rejects_unsupported_state_classification_strictness_policy() {
    let loaded_fixture = load_hourly_factor_regression_fixture_from_manifest()
        .expect("hourly factor regression fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("hourly factor regression fixture should be valid JSON");
    fixture_value["threshold_configuration"]["state_classification_strictness"] =
        Value::String("fuzzy_match.v1".to_string());

    let parsed_fixture = serde_json::from_value::<HourlyFactorRegressionFixtureDocument>(
        fixture_value,
    )
    .expect("hourly factor regression fixture with unsupported strictness should still parse");

    let validation_result = validate_hourly_factor_regression_fixture_document(&parsed_fixture);

    assert!(validation_result.is_err());
}

#[test]
fn hourly_factor_regression_fixture_rejects_factor_range_with_inverted_bounds() {
    let loaded_fixture = load_hourly_factor_regression_fixture_from_manifest()
        .expect("hourly factor regression fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("hourly factor regression fixture should be valid JSON");
    fixture_value["threshold_configuration"]["factor_range_by_fixture_class"]["normal_session"]["s_h_min"] =
        Value::from(2.5);
    fixture_value["threshold_configuration"]["factor_range_by_fixture_class"]["normal_session"]["s_h_max"] =
        Value::from(2.0);

    let parsed_fixture =
        serde_json::from_value::<HourlyFactorRegressionFixtureDocument>(fixture_value)
            .expect("hourly factor regression fixture with inverted bounds should still parse");

    let validation_result = validate_hourly_factor_regression_fixture_document(&parsed_fixture);

    assert!(validation_result.is_err());
}

#[test]
fn hourly_factor_regression_fixture_rejects_change_log_without_current_policy_version() {
    let loaded_fixture = load_hourly_factor_regression_fixture_from_manifest()
        .expect("hourly factor regression fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("hourly factor regression fixture should be valid JSON");
    fixture_value["threshold_configuration"]["policy_version"] = Value::String("2.0.0".to_string());

    let parsed_fixture = serde_json::from_value::<HourlyFactorRegressionFixtureDocument>(
        fixture_value,
    )
    .expect("hourly factor regression fixture with missing current policy log should still parse");

    let validation_result = validate_hourly_factor_regression_fixture_document(&parsed_fixture);

    assert!(validation_result.is_err());
}

#[test]
fn open_closed_gap_behavior_fixture_rejects_missing_required_fields() {
    let loaded_fixture = load_open_closed_gap_behavior_scenario_fixture_from_manifest()
        .expect("open and closed gap behavior fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("open and closed gap behavior fixture should be valid JSON");
    fixture_value
        .as_object_mut()
        .expect("open and closed gap fixture root should be an object")
        .remove("cases");

    let parse_result =
        serde_json::from_value::<OpenClosedGapBehaviorScenarioFixtureDocument>(fixture_value);

    assert!(parse_result.is_err());
}

#[test]
fn open_closed_gap_behavior_fixture_rejects_open_window_closed_gap_application() {
    let loaded_fixture = load_open_closed_gap_behavior_scenario_fixture_from_manifest()
        .expect("open and closed gap behavior fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("open and closed gap behavior fixture should be valid JSON");
    fixture_value["cases"][0]["expected_outcome"]["event_outcomes"][0]["expected_closed_gap_g_t_increment"] =
        Value::from(0.25);

    let parsed_fixture =
        serde_json::from_value::<OpenClosedGapBehaviorScenarioFixtureDocument>(fixture_value)
            .expect("open and closed gap behavior fixture with invalid open-window closed gap should still parse");
    let validation_errors =
        validate_open_closed_gap_behavior_scenario_fixture_document(&parsed_fixture)
            .expect_err("fixture with invalid open-window closed gap should fail validation");

    assert!(validation_errors.iter().any(|error| {
        error.code == "open_window_closed_gap_not_allowed"
            && error
                .field_path
                .contains("expected_outcome.event_outcomes[0].expected_closed_gap_g_t_increment")
    }));
}

#[test]
fn open_closed_gap_behavior_fixture_rejects_event_id_mapping_mismatch() {
    let loaded_fixture = load_open_closed_gap_behavior_scenario_fixture_from_manifest()
        .expect("open and closed gap behavior fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_fixture.loaded_payload.payload_bytes)
            .expect("open and closed gap behavior fixture should be valid JSON");
    fixture_value["cases"][0]["expected_outcome"]["event_outcomes"][0]["event_id"] =
        Value::String("unknown_event_id".to_string());

    let parsed_fixture =
        serde_json::from_value::<OpenClosedGapBehaviorScenarioFixtureDocument>(fixture_value)
            .expect(
                "open and closed gap behavior fixture with mismatched event ids should still parse",
            );
    let validation_errors =
        validate_open_closed_gap_behavior_scenario_fixture_document(&parsed_fixture)
            .expect_err("fixture with mismatched expected event ids should fail validation");

    assert!(validation_errors.iter().any(|error| {
        error.code == "event_id_mapping_mismatch"
            && error.field_path.contains("expected_outcome.event_outcomes")
    }));
}

#[test]
fn open_closed_gap_behavior_fixture_calendar_state_classification_matches_calendar_fixture() {
    let loaded_open_closed_fixture = load_open_closed_gap_behavior_scenario_fixture_from_manifest()
        .expect("open and closed gap behavior fixture should load from shared manifest");
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from shared manifest");

    validate_open_closed_gap_behavior_calendar_integration(
        &loaded_open_closed_fixture.fixture_document,
        &loaded_calendar_fixture.fixture_document,
    )
    .expect(
        "open and closed gap behavior scenarios should match canonical calendar classification",
    );
}

#[test]
fn open_closed_gap_behavior_fixture_rejects_missing_calendar_state_fixture_reference() {
    let loaded_open_closed_fixture = load_open_closed_gap_behavior_scenario_fixture_from_manifest()
        .expect("open and closed gap behavior fixture should load from shared manifest");
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_open_closed_fixture.loaded_payload.payload_bytes)
            .expect("open and closed gap behavior fixture should be valid JSON");
    fixture_value["cases"][0]["calendar_state_fixture_references"][0] =
        Value::String("missing_calendar_fixture_case".to_string());

    let parsed_fixture =
        serde_json::from_value::<OpenClosedGapBehaviorScenarioFixtureDocument>(fixture_value)
            .expect("fixture with missing calendar state fixture reference should still parse");
    validate_open_closed_gap_behavior_scenario_fixture_document(&parsed_fixture)
        .expect("schema-level validation should pass before calendar integration check");
    let integration_errors = validate_open_closed_gap_behavior_calendar_integration(
        &parsed_fixture,
        &loaded_calendar_fixture.fixture_document,
    )
    .expect_err(
        "calendar integration should fail when scenario references unknown calendar fixture id",
    );

    assert!(integration_errors.iter().any(|error| {
        error.code == "calendar_fixture_reference_missing"
            && error
                .message
                .contains("references missing calendar fixture id `missing_calendar_fixture_case`")
    }));
}

#[test]
fn open_closed_gap_behavior_fixture_rejects_missing_boundary_window_tag_coverage() {
    let loaded_open_closed_fixture = load_open_closed_gap_behavior_scenario_fixture_from_manifest()
        .expect("open and closed gap behavior fixture should load from shared manifest");
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from shared manifest");

    let mut fixture_value: Value =
        serde_json::from_slice(&loaded_open_closed_fixture.loaded_payload.payload_bytes)
            .expect("open and closed gap behavior fixture should be valid JSON");

    if let Some(cases) = fixture_value.get_mut("cases").and_then(Value::as_array_mut) {
        for fixture_case in cases {
            if let Some(tags) = fixture_case.get_mut("tags").and_then(Value::as_array_mut) {
                tags.retain(|case_tag| case_tag != "boundary");
            }
        }
    }

    if let Some(required_tags) =
        fixture_value["canonical_regression_metadata"]["required_tags"].as_array_mut()
    {
        required_tags.retain(|required_tag| required_tag != "boundary");
    }

    if let Some(stable_fixture_ids_by_tag) =
        fixture_value["canonical_regression_metadata"]["stable_fixture_ids_by_tag"].as_object_mut()
    {
        stable_fixture_ids_by_tag.remove("boundary");
    }

    let parsed_fixture =
        serde_json::from_value::<OpenClosedGapBehaviorScenarioFixtureDocument>(fixture_value)
            .expect("fixture without boundary tags should still parse");
    validate_open_closed_gap_behavior_scenario_fixture_document(&parsed_fixture)
        .expect("schema-level validation should pass before calendar integration check");

    let integration_errors = validate_open_closed_gap_behavior_calendar_integration(
        &parsed_fixture,
        &loaded_calendar_fixture.fixture_document,
    )
    .expect_err("calendar integration should fail when boundary window coverage is removed");

    assert!(integration_errors.iter().any(|error| {
        error.code == "calendar_window_tag_missing" && error.message.contains("`boundary`")
    }));
}
