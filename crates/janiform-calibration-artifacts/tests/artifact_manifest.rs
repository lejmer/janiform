mod support;
use support::shared::{build_valid_artifact_manifest, json, utc_nanos};
use janiform_calibration_artifacts::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY,
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION,
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CalibrationArtifactManifest,
    CalibrationArtifactManifestError, CalibrationExecutionMode,
};

#[test]
fn calibration_artifact_manifest_round_trip_serialization_preserves_required_fields() {
    let manifest = build_valid_artifact_manifest();

    assert_eq!(
        CalibrationArtifactManifest::schema_version(),
        CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION
    );

    let serialized = serde_json::to_value(&manifest).expect("manifest should serialize");
    assert_eq!(
        serialized["schema_version"],
        json!(CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION)
    );
    assert_eq!(serialized["run_id"], json!("run-001"));
    assert_eq!(serialized["execution_mode"], json!("disk"));
    assert_eq!(serialized["index_metadata"]["exchange_code"], json!("XNYS"));
    assert_eq!(
        serialized["index_metadata"]["validation_metric_summaries"][0]["overall_status"],
        json!("fail")
    );
    assert_eq!(
        serialized["artifact_references"][0]["artifact_kind"],
        json!("run_contract")
    );
    assert_eq!(
        serialized["artifact_references"][1]["stage"],
        json!("seasonality")
    );

    let parsed: CalibrationArtifactManifest =
        serde_json::from_value(serialized).expect("manifest should deserialize");
    assert_eq!(parsed, manifest);
    parsed
        .validate()
        .expect("deserialized manifest should validate");
}

#[test]
fn calibration_artifact_manifest_deserialization_rejects_missing_required_fields() {
    let mut missing_run_id =
        serde_json::to_value(build_valid_artifact_manifest()).expect("fixture should serialize");
    let run_id_object = missing_run_id
        .as_object_mut()
        .expect("serialized manifest should be an object");
    run_id_object.remove("run_id");

    let parse_result = serde_json::from_value::<CalibrationArtifactManifest>(missing_run_id);
    assert!(parse_result.is_err());

    let mut missing_artifact_references =
        serde_json::to_value(build_valid_artifact_manifest()).expect("fixture should serialize");
    let artifact_references_object = missing_artifact_references
        .as_object_mut()
        .expect("serialized manifest should be an object");
    artifact_references_object.remove("artifact_references");

    let parse_result =
        serde_json::from_value::<CalibrationArtifactManifest>(missing_artifact_references);
    assert!(parse_result.is_err());

    let mut missing_index_metadata =
        serde_json::to_value(build_valid_artifact_manifest()).expect("fixture should serialize");
    let index_metadata_object = missing_index_metadata
        .as_object_mut()
        .expect("serialized manifest should be an object");
    index_metadata_object.remove("index_metadata");

    let parse_result =
        serde_json::from_value::<CalibrationArtifactManifest>(missing_index_metadata);
    assert!(parse_result.is_err());
}

#[test]
fn calibration_artifact_manifest_validation_rejects_unsupported_schema_generation() {
    let mut manifest = build_valid_artifact_manifest();
    manifest.schema_version = format!("{CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY}.v2");

    let error = manifest
        .validate()
        .expect_err("unsupported schema generation should fail validation");

    assert_eq!(
        error,
        CalibrationArtifactManifestError::UnsupportedSchemaGeneration {
            expected: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION.to_string(),
            found: "v2".to_string(),
        }
    );
}

#[test]
fn calibration_artifact_manifest_validation_rejects_unexpected_schema_family() {
    let mut manifest = build_valid_artifact_manifest();
    manifest.schema_version = "pricing.artifact_registry.manifest.v1".to_string();

    let error = manifest
        .validate()
        .expect_err("unexpected schema family should fail validation");

    assert_eq!(
        error,
        CalibrationArtifactManifestError::UnexpectedSchemaFamily {
            expected: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY.to_string(),
            found: "pricing.artifact_registry.manifest".to_string(),
        }
    );
}

#[test]
fn calibration_artifact_manifest_validation_rejects_invalid_payload_content() {
    let mut empty_run_id_manifest = build_valid_artifact_manifest();
    empty_run_id_manifest.run_id = "   ".to_string();
    assert_eq!(
        empty_run_id_manifest
            .validate()
            .expect_err("blank run_id must fail validation"),
        CalibrationArtifactManifestError::MissingRunId
    );

    let mut memory_manifest = build_valid_artifact_manifest();
    memory_manifest.execution_mode = CalibrationExecutionMode::Memory;
    assert_eq!(
        memory_manifest
            .validate()
            .expect_err("memory-mode manifest must fail validation"),
        CalibrationArtifactManifestError::UnsupportedExecutionMode {
            expected: CalibrationExecutionMode::Disk,
            found: CalibrationExecutionMode::Memory,
        }
    );

    let mut missing_artifacts_manifest = build_valid_artifact_manifest();
    missing_artifacts_manifest.artifact_references.clear();
    assert_eq!(
        missing_artifacts_manifest
            .validate()
            .expect_err("manifest requires at least one artifact reference"),
        CalibrationArtifactManifestError::MissingArtifactReferences
    );

    let mut duplicate_artifact_id_manifest = build_valid_artifact_manifest();
    duplicate_artifact_id_manifest.artifact_references[1].artifact_id = "run_contract".to_string();
    assert_eq!(
        duplicate_artifact_id_manifest
            .validate()
            .expect_err("duplicate artifact ids must fail validation"),
        CalibrationArtifactManifestError::DuplicateArtifactId {
            artifact_id: "run_contract".to_string(),
        }
    );

    let mut empty_reference_manifest = build_valid_artifact_manifest();
    empty_reference_manifest.artifact_references[0].artifact_reference = "  ".to_string();
    assert_eq!(
        empty_reference_manifest
            .validate()
            .expect_err("blank artifact reference must fail validation"),
        CalibrationArtifactManifestError::EmptyArtifactReference {
            artifact_id: "run_contract".to_string(),
        }
    );

    let mut invalid_timestamp_manifest = build_valid_artifact_manifest();
    invalid_timestamp_manifest
        .index_metadata
        .committed_at_unix_nanos += 1;
    assert_eq!(
        invalid_timestamp_manifest
            .validate()
            .expect_err("mismatched commit timestamp must fail validation"),
        CalibrationArtifactManifestError::CommittedAtTimestampMismatch {
            committed_at_utc: "2026-03-05T12:30:00.000000000Z".to_string(),
            committed_at_unix_nanos: utc_nanos(2026, 3, 5, 12, 30, 0) + 1,
            expected_unix_nanos: utc_nanos(2026, 3, 5, 12, 30, 0),
        }
    );

    let mut invalid_checksum_manifest = build_valid_artifact_manifest();
    invalid_checksum_manifest
        .index_metadata
        .trade_events_reference_checksum = "0000000000000000".to_string();
    assert_eq!(
        invalid_checksum_manifest
            .validate()
            .expect_err("incorrect reference checksum must fail validation"),
        CalibrationArtifactManifestError::ReferenceChecksumMismatch {
            field: "index_metadata.trade_events_reference_checksum",
            expected: format!("{:016x}", 0xc08b_bacd_c5d6_a5f6_u64),
            found: "0000000000000000".to_string(),
        }
    );
}
