mod support;
use janiform_calibration_artifacts::{
    CalibrationArtifactReloadRequest, CalibrationArtifactRunPreparationMode,
    CalibrationArtifactRunWriter, CalibrationStage, build_calibration_artifact_manifest,
    build_calibration_dataset_snapshot_artifact,
    persist_calibration_artifact_manifest_to_run_directory,
    persist_calibration_dataset_snapshot_artifact_to_run_directory,
    persist_calibration_stage_parameter_artifact_to_run_directory,
    reload_calibration_artifact_run_from_disk,
};
use support::shared::{
    TimeZone, Utc, build_seasonality_stage_parameter_artifact_fixture,
    build_single_stage_disk_run_contract, fs, temporary_checkpoint_directory,
};

#[test]
fn reload_calibration_artifact_run_from_disk_restores_committed_artifacts() {
    let run_contract = build_single_stage_disk_run_contract("run-reload-roundtrip");
    let persistence_root_directory = temporary_checkpoint_directory("artifact-reload-roundtrip");
    let writer = CalibrationArtifactRunWriter::prepare(
        persistence_root_directory.as_path(),
        run_contract.run_spec.run_id.as_str(),
        CalibrationArtifactRunPreparationMode::FreshRun,
    )
    .expect("artifact writer should prepare a fresh run");
    let dataset_snapshot = build_calibration_dataset_snapshot_artifact(&run_contract)
        .expect("dataset snapshot should build");
    let stage_parameter_artifact =
        build_seasonality_stage_parameter_artifact_fixture(&run_contract);
    let manifest = build_calibration_artifact_manifest(
        &run_contract,
        &dataset_snapshot,
        std::slice::from_ref(&stage_parameter_artifact),
        &[],
        &[],
        &[],
        Utc.with_ymd_and_hms(2026, 3, 6, 9, 0, 0)
            .single()
            .expect("commit timestamp should be valid"),
    )
    .expect("manifest should build from the committed artifacts");

    persist_calibration_dataset_snapshot_artifact_to_run_directory(
        writer.pending_run_directory(),
        &dataset_snapshot,
    )
    .expect("dataset snapshot should persist to the pending run directory");
    persist_calibration_stage_parameter_artifact_to_run_directory(
        writer.pending_run_directory(),
        &stage_parameter_artifact,
    )
    .expect("stage parameter artifact should persist to the pending run directory");
    persist_calibration_artifact_manifest_to_run_directory(
        writer.pending_run_directory(),
        &manifest,
    )
    .expect("manifest should persist to the pending run directory");
    writer
        .commit()
        .expect("pending run artifacts should commit successfully");

    let request = CalibrationArtifactReloadRequest::complete_run("run-reload-roundtrip")
        .expect("reload request should validate");
    let reloaded_run =
        reload_calibration_artifact_run_from_disk(persistence_root_directory.as_path(), &request)
            .expect("committed artifact run should reload");

    assert_eq!(reloaded_run.manifest, manifest);
    assert_eq!(reloaded_run.dataset_snapshot, dataset_snapshot);
    assert_eq!(
        reloaded_run.stage_parameter_artifacts,
        vec![stage_parameter_artifact.clone()]
    );
    assert_eq!(
        reloaded_run.stage_parameter_artifact(CalibrationStage::Seasonality),
        Some(&stage_parameter_artifact)
    );
    assert!(reloaded_run.diagnostics_artifact.is_none());
    assert!(reloaded_run.comparison_artifact.is_none());

    let _ = fs::remove_dir_all(persistence_root_directory);
}
