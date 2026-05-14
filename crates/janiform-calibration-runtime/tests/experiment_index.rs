mod support;
use support::artifact_fixtures::build_stage_parameter_artifact_fixture_set;
use support::shared::{
    CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CalibrationArtifactManifest,
    CalibrationArtifactManifestMetricStatus, CalibrationArtifactManifestMetricStatusCount,
    CalibrationArtifactRunPreparationMode, CalibrationArtifactRunWriter, CalibrationExecutionMode,
    CalibrationExperimentIndexLookupRequest, CalibrationStageParameterArtifact,
    CalibrationValidationMetricArtifact, CalibrationValidationMetricArtifactRole::Diagnostics,
    CalibrationValidationMetricStageOutputLink, CalibrationValidationMetricStatus, TimeZone, Utc,
    build_calibration_artifact_manifest, build_calibration_dataset_snapshot_artifact,
    build_run_contract_with_mode_and_resume, build_validation_metric_artifact_fixture,
    calibration_experiment_index_path_for_root, fs, load_calibration_experiment_index_from_disk,
    lookup_calibration_experiment_index_records, persist_calibration_artifact_manifest_to_disk,
    persist_calibration_artifact_manifest_to_run_directory,
    persist_calibration_dataset_snapshot_artifact_to_run_directory,
    persist_calibration_stage_parameter_artifact_to_run_directory,
    persist_calibration_validation_metric_artifact_to_disk,
    rebuild_calibration_experiment_index_from_disk, temporary_checkpoint_directory,
    update_calibration_experiment_index_for_run, utc_nanos,
};

#[test]
fn experiment_index_commit_flow_creates_and_updates_run_records() {
    let persistence_root_directory = temporary_checkpoint_directory("experiment-index-commit");

    let committed_at = Utc
        .with_ymd_and_hms(2026, 3, 5, 12, 0, 0)
        .single()
        .expect("fixture timestamp should be valid");

    let fixture = commit_indexed_run(
        persistence_root_directory.as_path(),
        "run-index-001",
        committed_at,
        CalibrationArtifactManifestMetricStatus::Fail,
        "tail_skew.run-index-001",
    );

    let index_document =
        load_calibration_experiment_index_from_disk(persistence_root_directory.as_path())
            .expect("index document should load after commit");
    assert_eq!(index_document.records.len(), 1);
    assert_eq!(index_document.records[0].run_id, "run-index-001");
    assert_eq!(
        index_document.records[0]
            .index_metadata
            .validation_metric_summaries[0]
            .overall_status,
        CalibrationArtifactManifestMetricStatus::Fail
    );

    let updated_committed_at = Utc
        .with_ymd_and_hms(2026, 3, 6, 9, 15, 0)
        .single()
        .expect("fixture timestamp should be valid");

    let mut updated_manifest = fixture.manifest.clone();

    updated_manifest.index_metadata.committed_at_utc = "2026-03-06T09:15:00.000000000Z".to_string();
    updated_manifest.index_metadata.committed_at_unix_nanos = updated_committed_at
        .timestamp_nanos_opt()
        .expect("unix nanos");
    updated_manifest.index_metadata.validation_metric_summaries[0].overall_status =
        CalibrationArtifactManifestMetricStatus::Pass;
    updated_manifest.index_metadata.validation_metric_summaries[0].collection_status_counts =
        vec![CalibrationArtifactManifestMetricStatusCount {
            status: CalibrationArtifactManifestMetricStatus::Pass,
            count: 1,
        }];
    updated_manifest.index_metadata.validation_metric_summaries[0].metric_status_counts =
        vec![CalibrationArtifactManifestMetricStatusCount {
            status: CalibrationArtifactManifestMetricStatus::Pass,
            count: 2,
        }];
    updated_manifest.index_metadata.validation_metric_summaries[0]
        .failing_metric_ids
        .clear();

    persist_calibration_artifact_manifest_to_disk(
        persistence_root_directory.as_path(),
        &updated_manifest,
    )
    .expect("updated manifest should persist");

    update_calibration_experiment_index_for_run(
        persistence_root_directory.as_path(),
        "run-index-001",
    )
    .expect("index update should replace existing run record");

    let updated_index_document =
        load_calibration_experiment_index_from_disk(persistence_root_directory.as_path())
            .expect("updated index document should load");
    assert_eq!(updated_index_document.records.len(), 1);
    assert_eq!(
        updated_index_document.records[0]
            .index_metadata
            .committed_at_unix_nanos,
        updated_committed_at
            .timestamp_nanos_opt()
            .expect("unix nanos should fit")
    );
    assert_eq!(
        updated_index_document.records[0]
            .index_metadata
            .validation_metric_summaries[0]
            .overall_status,
        CalibrationArtifactManifestMetricStatus::Pass
    );

    let _ = fs::remove_dir_all(persistence_root_directory);
}

#[test]
fn experiment_index_rebuild_reads_committed_calibration_manifest() {
    let persistence_root_directory = temporary_checkpoint_directory("experiment-index-rebuild");

    commit_indexed_run(
        persistence_root_directory.as_path(),
        "run-index-c",
        Utc.with_ymd_and_hms(2026, 3, 6, 8, 0, 0)
            .single()
            .expect("fixture timestamp should be valid"),
        CalibrationArtifactManifestMetricStatus::Fail,
        "tail_skew.run-index-c",
    );

    fs::remove_file(calibration_experiment_index_path_for_root(
        persistence_root_directory.as_path(),
    ))
    .expect("index file should be removable");

    let rebuilt_index =
        rebuild_calibration_experiment_index_from_disk(persistence_root_directory.as_path())
            .expect("rebuild should succeed");

    assert_eq!(
        rebuilt_index
            .records
            .iter()
            .map(|record| record.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run-index-c"]
    );

    let _ = fs::remove_dir_all(persistence_root_directory);
}

#[test]
fn experiment_index_lookup_applies_filters_deterministically() {
    let persistence_root_directory = temporary_checkpoint_directory("experiment-index-lookup");

    let run_b_fixture = commit_indexed_run(
        persistence_root_directory.as_path(),
        "run-index-b",
        Utc.with_ymd_and_hms(2026, 3, 5, 11, 0, 0)
            .single()
            .expect("fixture timestamp should be valid"),
        CalibrationArtifactManifestMetricStatus::Fail,
        "tail_skew.run-index-b",
    );
    let first_page = lookup_calibration_experiment_index_records(
        persistence_root_directory.as_path(),
        &CalibrationExperimentIndexLookupRequest {
            trade_events_data_reference: Some("dataset://synthetic/trades/v1".to_string()),
            run_contract_schema_version: Some(CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION.to_string()),
            seasonality_estimator_version: run_b_fixture
                .manifest
                .index_metadata
                .model_version
                .seasonality_estimator_version
                .clone(),
            diagnostics_overall_status: Some(CalibrationArtifactManifestMetricStatus::Fail),
            committed_at_or_after_unix_nanos: Some(utc_nanos(2026, 3, 5, 0, 0, 0)),
            offset: 0,
            limit: 10,
            ..CalibrationExperimentIndexLookupRequest::default()
        },
    )
    .expect("filtered lookup should succeed");

    assert_eq!(first_page.total_matching_records, 1);
    assert_eq!(first_page.records.len(), 1);
    assert_eq!(first_page.records[0].run_id, "run-index-b");
    assert_eq!(first_page.next_offset, None);

    let exact_dataset_lookup = lookup_calibration_experiment_index_records(
        persistence_root_directory.as_path(),
        &CalibrationExperimentIndexLookupRequest {
            dataset_snapshot_checksum: Some(
                run_b_fixture
                    .manifest
                    .index_metadata
                    .dataset_snapshot_checksum
                    .clone(),
            ),
            offset: 0,
            limit: 10,
            ..CalibrationExperimentIndexLookupRequest::default()
        },
    )
    .expect("exact dataset checksum lookup should succeed");

    assert_eq!(exact_dataset_lookup.total_matching_records, 1);
    assert_eq!(exact_dataset_lookup.records[0].run_id, "run-index-b");

    let failing_metric_lookup = lookup_calibration_experiment_index_records(
        persistence_root_directory.as_path(),
        &CalibrationExperimentIndexLookupRequest {
            failing_metric_id: Some("tail_skew.run-index-b".to_string()),
            offset: 0,
            limit: 10,
            ..CalibrationExperimentIndexLookupRequest::default()
        },
    )
    .expect("failing metric lookup should succeed");

    assert_eq!(failing_metric_lookup.total_matching_records, 1);
    assert_eq!(failing_metric_lookup.records[0].run_id, "run-index-b");

    let _ = fs::remove_dir_all(persistence_root_directory);
}

struct IndexedRunFixture {
    manifest: CalibrationArtifactManifest,
}

fn commit_indexed_run(
    persistence_root_directory: &std::path::Path,
    run_id: &str,
    committed_at: chrono::DateTime<Utc>,
    diagnostics_overall_status: CalibrationArtifactManifestMetricStatus,
    failing_metric_id: &str,
) -> IndexedRunFixture {
    let run_contract =
        build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Disk, None);
    let stage_parameter_artifacts = build_stage_parameter_artifact_fixture_set(&run_contract);

    let dataset_snapshot = build_calibration_dataset_snapshot_artifact(&run_contract)
        .expect("dataset snapshot should build");
    let diagnostics_artifact = build_diagnostics_artifact(
        run_id,
        stage_parameter_artifacts.as_slice(),
        diagnostics_overall_status,
        failing_metric_id,
    );

    let manifest = build_calibration_artifact_manifest(
        &run_contract,
        &dataset_snapshot,
        stage_parameter_artifacts.as_slice(),
        &[],
        std::slice::from_ref(&diagnostics_artifact),
        &[],
        committed_at,
    )
    .expect("manifest should build");

    let writer = CalibrationArtifactRunWriter::prepare(
        persistence_root_directory,
        run_id,
        CalibrationArtifactRunPreparationMode::FreshRun,
    )
    .expect("writer should prepare");

    persist_calibration_dataset_snapshot_artifact_to_run_directory(
        writer.pending_run_directory(),
        &dataset_snapshot,
    )
    .expect("dataset snapshot should persist");

    for stage_parameter_artifact in &stage_parameter_artifacts {
        persist_calibration_stage_parameter_artifact_to_run_directory(
            writer.pending_run_directory(),
            stage_parameter_artifact,
        )
        .expect("stage artifact should persist");
    }

    persist_calibration_artifact_manifest_to_run_directory(
        writer.pending_run_directory(),
        &manifest,
    )
    .expect("manifest should persist");

    writer
        .commit()
        .expect("commit should update experiment index");

    persist_calibration_validation_metric_artifact_to_disk(
        persistence_root_directory,
        &diagnostics_artifact,
    )
    .expect("diagnostics artifact should persist");

    IndexedRunFixture { manifest }
}

fn build_diagnostics_artifact(
    run_id: &str,
    stage_parameter_artifacts: &[CalibrationStageParameterArtifact],
    diagnostics_overall_status: CalibrationArtifactManifestMetricStatus,
    failing_metric_id: &str,
) -> CalibrationValidationMetricArtifact {
    let mut metric_collections = build_validation_metric_artifact_fixture()
        .metric_collections
        .clone();

    metric_collections[0].collection_id = format!("diagnostics.{run_id}");
    metric_collections[0].status =
        lookup_metric_status_to_validation_metric_status(diagnostics_overall_status);

    let mut passing_metric = metric_collections[0].metrics[0].clone();

    passing_metric.metric_id = format!("activity_clustering.{run_id}");
    passing_metric.status = CalibrationValidationMetricStatus::Pass;

    let mut outcome_metric = metric_collections[0].metrics[0].clone();

    outcome_metric.metric_id = failing_metric_id.to_string();
    outcome_metric.status =
        lookup_metric_status_to_validation_metric_status(diagnostics_overall_status);
    outcome_metric
        .threshold_metadata
        .absolute_delta_exceeds_threshold =
        Some(diagnostics_overall_status == CalibrationArtifactManifestMetricStatus::Fail);

    metric_collections[0].metrics = vec![passing_metric, outcome_metric];

    CalibrationValidationMetricArtifact::try_new(
        run_id,
        Diagnostics,
        stage_parameter_artifacts
            .iter()
            .map(|artifact| CalibrationValidationMetricStageOutputLink {
                stage: artifact.stage,
                artifact_reference: artifact.artifact_reference.clone(),
                stage_output_schema_version: artifact.stage_output_schema_version.clone(),
            })
            .collect(),
        metric_collections,
    )
    .expect("diagnostics artifact should build")
}

fn lookup_metric_status_to_validation_metric_status(
    status: CalibrationArtifactManifestMetricStatus,
) -> CalibrationValidationMetricStatus {
    match status {
        CalibrationArtifactManifestMetricStatus::Pass => CalibrationValidationMetricStatus::Pass,
        CalibrationArtifactManifestMetricStatus::Fail => CalibrationValidationMetricStatus::Fail,
        CalibrationArtifactManifestMetricStatus::InsufficientData => {
            CalibrationValidationMetricStatus::InsufficientData
        }
        CalibrationArtifactManifestMetricStatus::Available => {
            CalibrationValidationMetricStatus::Available
        }
        CalibrationArtifactManifestMetricStatus::Missing => {
            CalibrationValidationMetricStatus::Missing
        }
        CalibrationArtifactManifestMetricStatus::Error => CalibrationValidationMetricStatus::Error,
    }
}
