mod support;

use std::collections::BTreeSet;
use std::fs;

use serde_json::Value;

use support::validation_fixture::{
    CalendarRegressionFixtureDocument, HourlyFactorRegressionFixtureDocument,
    ValidationFixtureManifestDocument, fixtures_root_path,
    load_calendar_regression_fixture_from_manifest,
    load_hourly_factor_regression_fixture_from_manifest, load_manifest_entry_payload,
    load_validation_fixture_manifest_document, sorted_manifest_entries,
    validate_calendar_regression_fixture_document,
    validate_hourly_factor_regression_fixture_document,
    validate_validation_fixture_manifest_document,
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
