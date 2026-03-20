mod support;
use support::shared::*;

#[test]
fn calibration_dataset_snapshot_artifact_round_trip_serialization_preserves_provenance_metadata() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-dataset-snapshot",
        CalibrationExecutionMode::Disk,
        None,
    );
    let artifact = build_calibration_dataset_snapshot_artifact(&run_contract)
        .expect("dataset snapshot artifact should be built");

    assert_eq!(
        artifact.schema_version,
        CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_VERSION
    );
    assert_eq!(
        artifact.schema_metadata.schema_family,
        CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_FAMILY
    );
    assert_eq!(
        artifact.schema_metadata.schema_generation,
        CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_GENERATION
    );
    assert_eq!(
        artifact.output_schema_metadata.schema_version,
        CALIBRATION_DATASET_SNAPSHOT_OUTPUT_SCHEMA_METADATA_VERSION
    );
    assert_eq!(
        artifact.provenance.data_inputs_schema_version,
        CALIBRATION_RUN_DATA_INPUTS_SCHEMA_VERSION
    );
    assert_eq!(
        artifact.provenance.persistence_scope_policy,
        CALIBRATION_DATASET_SNAPSHOT_PERSISTENCE_SCOPE_POLICY
    );
    assert_eq!(
        artifact.provenance.stage_input_naming_policy,
        CALIBRATION_DATASET_SNAPSHOT_STAGE_INPUT_NAMING_POLICY
    );
    assert_eq!(
        artifact.provenance.reference_checksum_algorithm,
        CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_ALGORITHM
    );
    assert_eq!(artifact.execution_mode, CalibrationExecutionMode::Disk);
    assert_eq!(artifact.input_datasets.len(), 2);
    assert_eq!(
        artifact.input_datasets[0].input_name,
        CalibrationDatasetInputName::TradeEventsDataReference
    );
    assert_eq!(
        artifact.input_datasets[1].input_name,
        CalibrationDatasetInputName::OptionSurfaceDataReference
    );

    let serialized = serde_json::to_value(&artifact).expect("artifact should serialize");
    assert_eq!(
        serialized["schema_version"],
        json!(CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_VERSION)
    );
    assert_eq!(serialized["execution_mode"], json!("disk"));
    assert_eq!(
        serialized["provenance"]["run_contract_schema_version"],
        json!(CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION)
    );
    assert_eq!(
        serialized["input_datasets"][0]["input_name"],
        json!("trade_events_data_reference")
    );
    assert_eq!(
        serialized["input_datasets"][0]["primary_stage"],
        json!("seasonality")
    );

    let parsed: CalibrationDatasetSnapshotArtifact =
        serde_json::from_value(serialized).expect("artifact should deserialize");
    parsed
        .validate()
        .expect("deserialized dataset snapshot artifact should validate");
    assert_eq!(parsed, artifact);
}

#[test]
fn calibration_dataset_snapshot_artifact_persist_and_load_round_trip_succeeds() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-dataset-snapshot-disk-roundtrip",
        CalibrationExecutionMode::Disk,
        None,
    );
    let artifact = build_calibration_dataset_snapshot_artifact(&run_contract)
        .expect("dataset snapshot artifact should be built");
    let checkpoint_directory = temporary_checkpoint_directory("dataset-snapshot-roundtrip");

    let persisted_path = persist_calibration_dataset_snapshot_artifact_to_disk(
        checkpoint_directory.as_path(),
        &artifact,
    )
    .expect("dataset snapshot artifact should persist");
    assert_eq!(
        persisted_path,
        calibration_dataset_snapshot_artifact_path(
            checkpoint_directory.as_path(),
            run_contract.run_spec.run_id.as_str()
        )
    );

    let loaded_artifact =
        load_calibration_dataset_snapshot_artifact_from_disk(persisted_path.as_path())
            .expect("persisted dataset snapshot artifact should load");
    assert_eq!(loaded_artifact, artifact);

    let _ = fs::remove_dir_all(checkpoint_directory);
}

#[test]
fn calibration_dataset_snapshot_artifact_deserialization_rejects_missing_provenance_fields() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-dataset-missing-provenance",
        CalibrationExecutionMode::Disk,
        None,
    );
    let artifact = build_calibration_dataset_snapshot_artifact(&run_contract)
        .expect("dataset snapshot artifact should be built");

    let mut serialized = serde_json::to_value(&artifact).expect("artifact should serialize");
    let provenance = serialized["provenance"]
        .as_object_mut()
        .expect("provenance should serialize as an object");
    provenance.remove("run_contract_schema_version");

    let parse_result = serde_json::from_value::<CalibrationDatasetSnapshotArtifact>(serialized);
    assert!(parse_result.is_err());
}

#[test]
fn calibration_dataset_snapshot_artifact_validation_rejects_blank_provenance_and_inconsistent_checksums()
 {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-dataset-checksum-validation",
        CalibrationExecutionMode::Disk,
        None,
    );
    let mut artifact = build_calibration_dataset_snapshot_artifact(&run_contract)
        .expect("dataset snapshot artifact should be built");

    artifact.provenance.run_spec_schema_version = "  ".to_string();
    assert_eq!(
        artifact
            .validate()
            .expect_err("blank provenance fields must fail validation"),
        CalibrationDatasetSnapshotArtifactValidationError::EmptyProvenanceField {
            field: "provenance.run_spec_schema_version",
        }
    );

    let mut artifact = build_calibration_dataset_snapshot_artifact(&run_contract)
        .expect("dataset snapshot artifact should be rebuilt");
    artifact.input_datasets[0].data_reference = "dataset://tampered/trades/v2".to_string();
    assert!(matches!(
        artifact
            .validate()
            .expect_err("tampered data reference should invalidate reference checksum"),
        CalibrationDatasetSnapshotArtifactValidationError::ReferenceChecksumMismatch {
            input_name: CalibrationDatasetInputName::TradeEventsDataReference,
            ..
        }
    ));

    let mut artifact = build_calibration_dataset_snapshot_artifact(&run_contract)
        .expect("dataset snapshot artifact should be rebuilt");
    artifact.snapshot_checksum = "0000000000000000".to_string();
    assert!(matches!(
        artifact
            .validate()
            .expect_err("tampered snapshot checksum should fail validation"),
        CalibrationDatasetSnapshotArtifactValidationError::SnapshotChecksumMismatch { .. }
    ));
}
