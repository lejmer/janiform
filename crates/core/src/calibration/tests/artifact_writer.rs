use super::support::*;

#[test]
fn calibration_artifact_storage_layout_paths_are_deterministic() {
    let persistence_root_directory = PathBuf::from("/tmp/janiform-persistence-root");
    let run_id = "run-layout-001";

    assert_eq!(
        CALIBRATION_ARTIFACT_STORAGE_LAYOUT_POLICY,
        "root_directory_separates_committed_artifacts_pending_artifacts_quarantine_and_checkpoints.v1"
    );
    assert_eq!(
        CALIBRATION_ARTIFACT_ROOT_CONFIGURATION_POLICY,
        "caller_supplied_root_directory_supports_local_and_ci_filesystem_contexts.v1"
    );
    assert_eq!(
        CALIBRATION_ARTIFACT_ATOMIC_COMMIT_POLICY,
        "stage_all_run_artifacts_under_pending_then_publish_with_single_directory_rename.v1"
    );
    assert_eq!(
        CALIBRATION_ARTIFACT_PARTIAL_WRITE_RECOVERY_POLICY,
        "fresh_runs_quarantine_stale_pending_or_incomplete_run_directories_before_restarting.v1"
    );

    assert_eq!(
        calibration_committed_artifact_run_directory(persistence_root_directory.as_path(), run_id,),
        PathBuf::from("/tmp/janiform-persistence-root/artifacts/runs/run-layout-001")
    );
    assert_eq!(
        calibration_checkpoint_run_directory(persistence_root_directory.as_path(), run_id),
        PathBuf::from("/tmp/janiform-persistence-root/checkpoints/run-layout-001")
    );
    assert_eq!(
        calibration_dataset_snapshot_artifact_path(persistence_root_directory.as_path(), run_id),
        PathBuf::from(
            "/tmp/janiform-persistence-root/artifacts/runs/run-layout-001/dataset_snapshot.artifact.json"
        )
    );
    assert_eq!(
        calibration_stage_parameter_artifact_path(
            persistence_root_directory.as_path(),
            run_id,
            CalibrationStage::Hawkes,
        ),
        PathBuf::from(
            "/tmp/janiform-persistence-root/artifacts/runs/run-layout-001/hawkes.stage_parameter.artifact.json"
        )
    );
    assert_eq!(
        calibration_artifact_manifest_path(persistence_root_directory.as_path(), run_id),
        PathBuf::from(
            "/tmp/janiform-persistence-root/artifacts/runs/run-layout-001/artifact_manifest.json"
        )
    );
    assert_eq!(
        calibration_experiment_index_path_for_root(persistence_root_directory.as_path()),
        PathBuf::from(
            "/tmp/janiform-persistence-root/artifacts/index/calibration_experiment_index.json"
        )
    );
    assert_eq!(
        calibration_stage_checkpoint_path(
            persistence_root_directory.as_path(),
            run_id,
            CalibrationStage::Hawkes,
        ),
        PathBuf::from(
            "/tmp/janiform-persistence-root/checkpoints/run-layout-001/hawkes.checkpoint.json"
        )
    );
    assert_eq!(
        calibration_last_success_checkpoint_path(persistence_root_directory.as_path(), run_id),
        PathBuf::from(
            "/tmp/janiform-persistence-root/checkpoints/run-layout-001/last-success.checkpoint.json"
        )
    );
}

#[test]
fn calibration_artifact_run_writer_quarantines_stale_pending_runs_before_fresh_restart() {
    let persistence_root_directory = temporary_checkpoint_directory("artifact-writer-quarantine");
    let run_id = "run-stale-pending";
    let stale_pending_directory = persistence_root_directory
        .join("artifacts")
        .join("pending")
        .join(run_id);
    fs::create_dir_all(stale_pending_directory.as_path())
        .expect("stale pending directory should be created");
    fs::write(
        stale_pending_directory.join("stale.txt"),
        "stale-artifact-payload",
    )
    .expect("stale pending marker should be written");

    let writer = CalibrationArtifactRunWriter::prepare(
        persistence_root_directory.as_path(),
        run_id,
        CalibrationArtifactRunPreparationMode::FreshRun,
    )
    .expect("fresh run should quarantine stale pending directory");

    let quarantined_directory = persistence_root_directory
        .join("artifacts")
        .join("quarantine")
        .join("run-stale-pending.stale_pending.0001");
    assert!(quarantined_directory.exists());
    assert!(quarantined_directory.join("stale.txt").exists());
    assert!(writer.pending_run_directory().exists());
    assert_eq!(
        writer.committed_run_directory(),
        calibration_committed_artifact_run_directory(persistence_root_directory.as_path(), run_id,)
            .as_path()
    );

    let _ = fs::remove_dir_all(persistence_root_directory);
}
