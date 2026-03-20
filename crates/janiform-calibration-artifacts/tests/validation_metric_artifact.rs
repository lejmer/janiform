mod support;
use support::shared::*;

#[test]
fn validation_metric_artifact_round_trip_preserves_machine_readable_metadata() {
    let artifact = build_validation_metric_artifact_fixture();
    assert_eq!(
        artifact.schema_version,
        CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_VERSION
    );
    assert_eq!(
        artifact.schema_metadata.schema_family,
        CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_FAMILY
    );
    assert_eq!(
        artifact.schema_metadata.schema_generation,
        CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_GENERATION
    );
    assert_eq!(
        artifact.output_schema_metadata.schema_version,
        CALIBRATION_VALIDATION_METRIC_ARTIFACT_OUTPUT_SCHEMA_METADATA_VERSION
    );
    assert_eq!(
        artifact.manifest_link.artifact_reference,
        calibration_artifact_manifest_reference("run-001")
    );
    assert_eq!(artifact.stage_output_links.len(), 4);
    assert_eq!(artifact.metric_collections.len(), 1);
    assert_eq!(
        artifact.output_schema_metadata.artifact_role_vocabulary,
        vec!["diagnostics".to_string(), "comparison".to_string()]
    );

    let serialized = serde_json::to_value(&artifact).expect("artifact should serialize");
    assert_eq!(
        serialized["schema_version"],
        json!(CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_VERSION)
    );
    assert_eq!(serialized["artifact_role"], json!("diagnostics"));
    assert_eq!(
        serialized["manifest_link"]["schema_version"],
        json!(CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION)
    );
    assert_eq!(
        serialized["metric_collections"][0]["metrics"][0]["threshold_metadata"]["threshold_id"],
        json!("abs_delta_q99_q95")
    );

    let parsed: CalibrationValidationMetricArtifact =
        serde_json::from_value(serialized).expect("artifact should deserialize");
    parsed
        .validate()
        .expect("deserialized validation metric artifact should validate");
    assert_eq!(parsed, artifact);
}

#[test]
fn validation_metric_artifact_persist_and_load_round_trip_succeeds() {
    let artifact = build_validation_metric_artifact_fixture();
    let checkpoint_directory = temporary_checkpoint_directory("validation-metric-artifact");

    let persisted_path = persist_calibration_validation_metric_artifact_to_disk(
        checkpoint_directory.as_path(),
        &artifact,
    )
    .expect("validation metric artifact should persist");
    assert_eq!(
        persisted_path,
        calibration_validation_metric_artifact_path(
            checkpoint_directory.as_path(),
            "run-001",
            CalibrationValidationMetricArtifactRole::Diagnostics,
        )
    );

    let loaded_artifact =
        load_calibration_validation_metric_artifact_from_disk(persisted_path.as_path())
            .expect("persisted validation metric artifact should load");
    assert_eq!(loaded_artifact, artifact);

    let _ = fs::remove_dir_all(checkpoint_directory);
}

#[test]
fn validation_metric_artifact_rejects_incomplete_metric_records() {
    let mut artifact = build_validation_metric_artifact_fixture();
    artifact.metric_collections[0].metrics[0].absolute_delta = None;
    artifact.metric_collections[0].metrics[0].delta_value = None;
    artifact.metric_collections[0].metrics[0].standardized_delta = None;
    artifact.metric_collections[0].metrics[0]
        .primary_value
        .value = None;
    artifact.metric_collections[0].metrics[0]
        .secondary_value
        .as_mut()
        .expect("fixture should include secondary value")
        .value = None;

    assert_eq!(
        artifact
            .validate()
            .expect_err("available metric without values should fail"),
        CalibrationValidationMetricArtifactValidationError::MissingMetricValueForStatus {
            collection_id: "activity-clustering-1h".to_string(),
            metric_id: "returns_abs_quantile_shape_ratio_q99_q95".to_string(),
            status: CalibrationValidationMetricStatus::Pass,
        }
    );

    let mut artifact = build_validation_metric_artifact_fixture();
    artifact.metric_collections[0].metrics[0]
        .threshold_metadata
        .threshold_policy_id = " ".to_string();
    assert!(matches!(
        artifact
            .validate()
            .expect_err("blank threshold policy id must fail"),
        CalibrationValidationMetricArtifactValidationError::InvalidThresholdMetadata { .. }
    ));
}

#[test]
fn validation_metric_artifact_reference_integrity_requires_manifest_and_stage_output_matches() {
    let artifact = build_validation_metric_artifact_fixture();
    let manifest = build_validation_metric_manifest_fixture();
    artifact
        .validate_against_manifest(&manifest)
        .expect("artifact references should match manifest");

    let mut invalid_manifest_reference_artifact = build_validation_metric_artifact_fixture();
    invalid_manifest_reference_artifact.manifest_link = CalibrationValidationMetricManifestLink {
        artifact_reference: "artifact://manifest/other-run".to_string(),
        schema_version: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
    };
    assert_eq!(
        invalid_manifest_reference_artifact
            .validate_against_manifest(&manifest)
            .expect_err("unexpected manifest reference must fail"),
        CalibrationValidationMetricArtifactReferenceIntegrityError::UnexpectedManifestReference {
            expected: calibration_artifact_manifest_reference("run-001"),
            found: "artifact://manifest/other-run".to_string(),
        }
    );

    let mut missing_stage_reference_artifact = build_validation_metric_artifact_fixture();
    missing_stage_reference_artifact.stage_output_links[1].artifact_reference =
        "artifact://hawkes/missing".to_string();
    assert_eq!(
        missing_stage_reference_artifact
            .validate_against_manifest(&manifest)
            .expect_err("unknown stage output reference must fail"),
        CalibrationValidationMetricArtifactReferenceIntegrityError::UnknownStageOutputReference {
            stage: CalibrationStage::Hawkes,
            artifact_reference: "artifact://hawkes/missing".to_string(),
        }
    );
}
