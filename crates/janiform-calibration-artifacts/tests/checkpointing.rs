mod support;
use support::shared::{
    build_single_stage_disk_run_contract, build_stage_checkpoint_metadata_fixture, fs,
    temporary_checkpoint_directory,
};
use janiform_calibration_artifacts::{
    CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationStage,
    load_stage_checkpoint_from_disk, persist_stage_checkpoint_to_disk,
};

#[test]
fn stage_checkpoint_persist_and_load_round_trip_preserves_compatibility_metadata() {
    let run_contract = build_single_stage_disk_run_contract("run-checkpoint-roundtrip");
    let checkpoint_directory = temporary_checkpoint_directory("artifact-checkpoint-roundtrip");
    let checkpoint_metadata = build_stage_checkpoint_metadata_fixture(
        &run_contract,
        CalibrationStage::Seasonality,
        CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION,
    );

    persist_stage_checkpoint_to_disk(checkpoint_directory.as_path(), &checkpoint_metadata)
        .expect("checkpoint metadata should persist");

    let loaded_stage_checkpoint = load_stage_checkpoint_from_disk(
        checkpoint_directory.as_path(),
        "checkpoint://run-checkpoint-roundtrip/seasonality",
    )
    .expect("stage checkpoint should load");
    assert_eq!(loaded_stage_checkpoint, checkpoint_metadata);
    loaded_stage_checkpoint
        .validate_compatibility_with_run_contract(&run_contract)
        .expect("stage checkpoint should remain compatible with the run contract");

    let loaded_last_success = load_stage_checkpoint_from_disk(
        checkpoint_directory.as_path(),
        "checkpoint://run-checkpoint-roundtrip/last-success",
    )
    .expect("last-success checkpoint should load");
    assert_eq!(loaded_last_success, checkpoint_metadata);
    loaded_last_success
        .validate_compatibility_with_run_contract(&run_contract)
        .expect("last-success checkpoint should remain compatible with the run contract");

    let _ = fs::remove_dir_all(checkpoint_directory);
}
