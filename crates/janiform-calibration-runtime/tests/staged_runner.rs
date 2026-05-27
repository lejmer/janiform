mod support;
use janiform_calibration_artifacts::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY,
    CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY, CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
    CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION, CalibrationArtifactKind,
    CalibrationCallerSurface, CalibrationDatasetInputName, CalibrationExecutionMode,
    CalibrationResumeSettings, CalibrationStage, CalibrationStageSeedSource,
    calibration_artifact_manifest_path, calibration_dataset_snapshot_artifact_path,
    calibration_last_success_checkpoint_path, calibration_last_success_checkpoint_reference,
    calibration_stage_checkpoint_path, calibration_stage_checkpoint_reference,
    calibration_stage_parameter_artifact_path, calibration_stage_token,
    derive_calibration_stage_seed_from_global_seed, load_calibration_artifact_manifest_from_disk,
    load_calibration_dataset_snapshot_artifact_from_disk,
    load_calibration_stage_parameter_artifact_from_disk,
};
use janiform_calibration_runtime::{
    CalibrationMemoryCheckpointStore, CalibrationStageExecutionStatus,
    CalibrationStageProgressPhase, CalibrationStagedRunFailure, CalibrationStagedRunObserverEvent,
    CalibrationStagedRunReport, CalibrationStagedRunStageReport, HawkesStageExecutionFailure,
    JointRefinementStageProgressPhase, run_staged_calibration_with_checkpointing,
    run_staged_calibration_with_checkpointing_and_observer,
};
use serde_json::Value;
use support::shared::{
    BTreeSet, build_pre_joint_run_contract_with_mode_and_resume,
    build_run_contract_for_caller_surface, build_run_contract_with_mode_and_resume,
    build_through_mean_reversion_run_contract_with_mode_and_resume,
    calibration_stage_test_registry, fs, hawkes_stage_constraint_violating_candidate_fixture,
    joint_refinement_stage_option_pricing_input_fixture, json,
    normalize_joint_refinement_runtime_profile_for_reproducibility,
    resolve_calibration_execution_mode_for_surface, staged_run_inputs,
    temporary_checkpoint_directory,
};

#[test]
fn staged_runner_skips_option_pricing_progress_during_baseline_search() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-stage-progress-option-pricing",
        CalibrationExecutionMode::Memory,
        None,
    );
    let calendar_registry = calibration_stage_test_registry();

    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();
    let mut inputs = staged_run_inputs(true, true, true);

    inputs.option_refinement_pricing_input =
        Some(joint_refinement_stage_option_pricing_input_fixture());

    let mut observed_events = Vec::<CalibrationStagedRunObserverEvent>::new();

    let report = run_staged_calibration_with_checkpointing_and_observer(
        &run_contract,
        &calendar_registry,
        inputs,
        Some(&mut checkpoint_store),
        None,
        Some(&mut |event| observed_events.push(event)),
    );

    assert!(
        observed_events.iter().any(|event| matches!(
            event,
            CalibrationStagedRunObserverEvent::StageProgress(progress)
                if progress.stage == CalibrationStage::JointRefinement
        )),
        "expected staged runner to emit joint refinement progress events, report was: {report:#?}"
    );

    let baseline_local_option_pricing_labels = observed_events
        .iter()
        .filter_map(|event| match event {
            CalibrationStagedRunObserverEvent::StageProgress(progress)
                if progress.stage == CalibrationStage::JointRefinement
                    && progress.progress_phase
                        == CalibrationStageProgressPhase::JointRefinement(
                            JointRefinementStageProgressPhase::BaselineLocalEvaluation,
                        ) =>
            {
                progress.current_item_label.as_deref()
            }
            _ => None,
        })
        .filter(|label| {
            label.starts_with("strike ")
                || label.starts_with("grid_batch ")
                || label.starts_with("maturity ")
                || label.starts_with("interval ")
        })
        .collect::<Vec<_>>();

    assert!(
        baseline_local_option_pricing_labels.is_empty(),
        "expected baseline local evaluation to skip nested option-pricing progress labels, observed events were: {observed_events:#?}"
    );

    let baseline_final_cache_labels = observed_events
        .iter()
        .filter_map(|event| match event {
            CalibrationStagedRunObserverEvent::StageProgress(progress)
                if progress.stage == CalibrationStage::JointRefinement
                    && progress.progress_phase
                        == CalibrationStageProgressPhase::JointRefinement(
                            JointRefinementStageProgressPhase::BaselineFinalEvaluation,
                        ) =>
            {
                progress.current_item_label.as_deref()
            }
            _ => None,
        })
        .filter(|label| {
            *label == "option_pricing cache_lookup"
                || *label == "option_pricing cache_hit"
                || *label == "option_pricing cache_hit_resolved"
                || *label == "option_pricing cache_hit_value"
                || *label == "option_pricing cache_hit_error"
                || *label == "option_pricing cache_miss"
                || *label == "option_pricing resolved_ok"
                || *label == "option_pricing resolved_error"
        })
        .collect::<Vec<_>>();

    assert!(
        baseline_final_cache_labels.is_empty(),
        "expected baseline final evaluation to skip option-pricing cache progress labels during task-1 search, observed events were: {observed_events:#?}"
    );

    let baseline_final_nested_option_pricing_labels = observed_events
        .iter()
        .filter_map(|event| match event {
            CalibrationStagedRunObserverEvent::StageProgress(progress)
                if progress.stage == CalibrationStage::JointRefinement
                    && progress.progress_phase
                        == CalibrationStageProgressPhase::JointRefinement(
                            JointRefinementStageProgressPhase::BaselineFinalEvaluation,
                        ) =>
            {
                progress.current_item_label.as_deref()
            }
            _ => None,
        })
        .filter(|label| {
            label.starts_with("strike ")
                || label.starts_with("grid_batch ")
                || label.starts_with("maturity ")
                || label.starts_with("interval ")
        })
        .collect::<Vec<_>>();

    assert!(
        baseline_final_nested_option_pricing_labels.is_empty(),
        "expected baseline final evaluation to skip nested spectral option-pricing progress labels during task-1 search, observed events were: {observed_events:#?}"
    );

    let joint_refinement_digit_only_item_labels = observed_events
        .iter()
        .filter_map(|event| match event {
            CalibrationStagedRunObserverEvent::StageProgress(progress)
                if progress.stage == CalibrationStage::JointRefinement =>
            {
                progress.current_item_label.as_deref()
            }
            _ => None,
        })
        .filter(|label| {
            !label.is_empty() && label.chars().all(|character| character.is_ascii_digit())
        })
        .collect::<Vec<_>>();

    assert!(
        joint_refinement_digit_only_item_labels.is_empty(),
        "expected joint refinement progress labels to remain human-readable, observed digit-only labels were {joint_refinement_digit_only_item_labels:?}, all events were: {observed_events:#?}"
    );

    let baseline_coarse_consistency_labels = observed_events
        .iter()
        .filter_map(|event| match event {
            CalibrationStagedRunObserverEvent::StageProgress(progress)
                if progress.stage == CalibrationStage::JointRefinement
                    && progress.progress_phase
                        == CalibrationStageProgressPhase::JointRefinement(
                            JointRefinementStageProgressPhase::BaselineCoarseConsistencyEvaluation,
                        ) =>
            {
                progress.current_item_label.as_deref()
            }
            _ => None,
        })
        .filter(|label| *label == "baseline coarse_consistency")
        .collect::<Vec<_>>();

    assert!(
        !baseline_coarse_consistency_labels.is_empty(),
        "expected staged runner to expose baseline coarse-consistency progress labels, observed events were: {observed_events:#?}"
    );
}

#[test]
fn staged_runner_emits_checkpoint_metadata_with_deterministic_identifiers_in_memory_mode() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-checkpoint-memory",
        CalibrationExecutionMode::Memory,
        None,
    );
    let calendar_registry = calibration_stage_test_registry();

    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

    let report = run_staged_calibration_with_checkpointing(
        &run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        Some(&mut checkpoint_store),
        None,
    );

    assert!(report.status.is_successful(), "{report:#?}");
    assert!(report.failure.is_none());
    assert_eq!(report.emitted_checkpoints.len(), 7);
    assert_eq!(
        checkpoint_store.checkpoint_count_for_run(run_contract.run_spec.run_id.as_str()),
        7
    );

    let first_checkpoint = report
        .emitted_checkpoints
        .first()
        .expect("first stage checkpoint should exist");

    assert_eq!(
        first_checkpoint.schema_version,
        CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION
    );
    assert_eq!(
        first_checkpoint.checkpoint_reference_policy,
        CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY
    );
    assert_eq!(
        first_checkpoint.disk_write_policy,
        CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY
    );
    assert_eq!(
        first_checkpoint.artifact_manifest_schema_version,
        CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION
    );
    assert_eq!(
        first_checkpoint.checkpoint_reference,
        calibration_stage_checkpoint_reference(
            run_contract.run_spec.run_id.as_str(),
            CalibrationStage::Seasonality
        )
    );
    assert_eq!(
        first_checkpoint.completed_stage,
        CalibrationStage::Seasonality
    );
    assert_eq!(first_checkpoint.completed_stage_position, 0);
    assert_eq!(first_checkpoint.run_global_seed, 17);
    assert_eq!(first_checkpoint.completed_stage_seed, 101);
    assert_eq!(
        first_checkpoint.completed_stage_seed_source,
        CalibrationStageSeedSource::StageOverride
    );
    assert_eq!(
        first_checkpoint.stage_seed_derivation_policy,
        CALIBRATION_STAGE_SEED_DERIVATION_POLICY
    );

    let return_components_checkpoint = report
        .emitted_checkpoints
        .iter()
        .find(|checkpoint| checkpoint.completed_stage == CalibrationStage::ReturnComponents)
        .expect("return-components stage checkpoint should exist");

    assert_eq!(
        return_components_checkpoint.completed_stage_seed_source,
        CalibrationStageSeedSource::DerivedFromGlobalSeed
    );
    assert_eq!(
        return_components_checkpoint.completed_stage_seed,
        derive_calibration_stage_seed_from_global_seed(17, CalibrationStage::ReturnComponents)
    );
    assert_eq!(
        checkpoint_store
            .last_checkpoint_for_run(run_contract.run_spec.run_id.as_str())
            .expect("store should expose last checkpoint")
            .completed_stage,
        CalibrationStage::Finalization
    );
}

#[test]
fn staged_runner_reports_run_and_stage_seed_metadata_for_all_stages() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-stage-seed-metadata",
        CalibrationExecutionMode::Memory,
        None,
    );
    let calendar_registry = calibration_stage_test_registry();

    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

    let report = run_staged_calibration_with_checkpointing(
        &run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        Some(&mut checkpoint_store),
        None,
    );

    assert!(report.status.is_successful(), "{report:#?}");
    assert_eq!(report.stage_reports.len(), 7);

    for stage_report in &report.stage_reports {
        match stage_report {
            CalibrationStagedRunStageReport::Seasonality(seasonality_report) => {
                let metadata = seasonality_report
                    .metadata
                    .as_ref()
                    .expect("seasonality stage metadata should be present");
                assert_eq!(metadata.run_global_seed, 17);
                assert_eq!(metadata.stage_seed, 101);
                assert_eq!(
                    metadata.stage_seed_source,
                    CalibrationStageSeedSource::StageOverride
                );
                assert_eq!(
                    metadata.stage_seed_derivation_policy,
                    CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                );
            }
            CalibrationStagedRunStageReport::Hawkes(hawkes_report) => {
                let metadata = hawkes_report
                    .metadata
                    .as_ref()
                    .expect("hawkes stage metadata should be present");
                assert_eq!(metadata.run_global_seed, 17);
                assert_eq!(metadata.stage_seed, 202);
                assert_eq!(
                    metadata.stage_seed_source,
                    CalibrationStageSeedSource::StageOverride
                );
                assert_eq!(
                    metadata.stage_seed_derivation_policy,
                    CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                );
            }
            CalibrationStagedRunStageReport::ReturnComponents(return_components_report) => {
                let metadata = return_components_report
                    .metadata
                    .as_ref()
                    .expect("return-components stage metadata should be present");
                assert_eq!(metadata.run_global_seed, 17);
                assert_eq!(
                    metadata.stage_seed,
                    derive_calibration_stage_seed_from_global_seed(
                        17,
                        CalibrationStage::ReturnComponents
                    )
                );
                assert_eq!(
                    metadata.stage_seed_source,
                    CalibrationStageSeedSource::DerivedFromGlobalSeed
                );
                assert_eq!(
                    metadata.stage_seed_derivation_policy,
                    CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                );
            }
            CalibrationStagedRunStageReport::LongVariance(long_variance_report) => {
                let metadata = long_variance_report
                    .metadata
                    .as_ref()
                    .expect("long-variance stage metadata should be present");
                assert_eq!(metadata.run_global_seed, 17);
                assert_eq!(
                    metadata.stage_seed,
                    derive_calibration_stage_seed_from_global_seed(
                        17,
                        CalibrationStage::LongVariance
                    )
                );
                assert_eq!(
                    metadata.stage_seed_source,
                    CalibrationStageSeedSource::DerivedFromGlobalSeed
                );
                assert_eq!(
                    metadata.stage_seed_derivation_policy,
                    CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                );
            }
            CalibrationStagedRunStageReport::MeanReversion(mean_reversion_report) => {
                let metadata = mean_reversion_report
                    .metadata
                    .as_ref()
                    .expect("mean-reversion stage metadata should be present");
                assert_eq!(metadata.run_global_seed, 17);
                assert_eq!(
                    metadata.stage_seed,
                    derive_calibration_stage_seed_from_global_seed(
                        17,
                        CalibrationStage::MeanReversion
                    )
                );
                assert_eq!(
                    metadata.stage_seed_source,
                    CalibrationStageSeedSource::DerivedFromGlobalSeed
                );
                assert_eq!(
                    metadata.stage_seed_derivation_policy,
                    CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                );
            }
            CalibrationStagedRunStageReport::JointRefinement(joint_refinement_report) => {
                let metadata = joint_refinement_report
                    .metadata
                    .as_ref()
                    .expect("joint-refinement stage metadata should be present");
                assert_eq!(metadata.run_global_seed, 17);
                assert_eq!(
                    metadata.stage_seed,
                    derive_calibration_stage_seed_from_global_seed(
                        17,
                        CalibrationStage::JointRefinement
                    )
                );
                assert_eq!(
                    metadata.stage_seed_source,
                    CalibrationStageSeedSource::DerivedFromGlobalSeed
                );
                assert_eq!(
                    metadata.stage_seed_derivation_policy,
                    CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                );
            }
            CalibrationStagedRunStageReport::OptionRefinement(option_refinement_report) => {
                let metadata = option_refinement_report
                    .metadata
                    .as_ref()
                    .expect("option-refinement stage metadata should be present");
                assert_eq!(metadata.run_global_seed, 17);
                assert_eq!(
                    metadata.stage_seed,
                    derive_calibration_stage_seed_from_global_seed(
                        17,
                        CalibrationStage::OptionRefinement
                    )
                );
                assert_eq!(
                    metadata.stage_seed_source,
                    CalibrationStageSeedSource::DerivedFromGlobalSeed
                );
                assert_eq!(
                    metadata.stage_seed_derivation_policy,
                    CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                );
            }
            CalibrationStagedRunStageReport::Finalization(finalization_report) => {
                let metadata = finalization_report
                    .metadata
                    .as_ref()
                    .expect("finalization stage metadata should be present");
                assert_eq!(metadata.run_global_seed, 17);
                assert_eq!(
                    metadata.stage_seed,
                    derive_calibration_stage_seed_from_global_seed(
                        17,
                        CalibrationStage::Finalization
                    )
                );
                assert_eq!(
                    metadata.stage_seed_source,
                    CalibrationStageSeedSource::DerivedFromGlobalSeed
                );
                assert_eq!(
                    metadata.stage_seed_derivation_policy,
                    CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                );
            }
        }
    }
}

#[test]
fn staged_runner_full_workflow_is_reproducible_across_repeated_runs_in_both_modes() {
    let calendar_registry = calibration_stage_test_registry();

    let memory_contract = build_run_contract_with_mode_and_resume(
        "run-repeat-memory",
        CalibrationExecutionMode::Memory,
        None,
    );

    let mut first_memory_checkpoint_store = CalibrationMemoryCheckpointStore::new();

    let first_memory_report = run_staged_calibration_with_checkpointing(
        &memory_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        Some(&mut first_memory_checkpoint_store),
        None,
    );

    let mut second_memory_checkpoint_store = CalibrationMemoryCheckpointStore::new();

    let second_memory_report = run_staged_calibration_with_checkpointing(
        &memory_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        Some(&mut second_memory_checkpoint_store),
        None,
    );

    assert_reproducible_reports_match(first_memory_report, second_memory_report);

    let checkpoint_directory = temporary_checkpoint_directory("repeat-full-disk");

    let disk_contract = build_run_contract_with_mode_and_resume(
        "run-repeat-disk",
        CalibrationExecutionMode::Disk,
        None,
    );

    let first_disk_report = run_staged_calibration_with_checkpointing(
        &disk_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(checkpoint_directory.as_path()),
    );

    let second_checkpoint_directory = temporary_checkpoint_directory("repeat-full-disk-second");

    let second_disk_report = run_staged_calibration_with_checkpointing(
        &disk_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(second_checkpoint_directory.as_path()),
    );

    assert_reproducible_reports_match(first_disk_report, second_disk_report);

    let _ = fs::remove_dir_all(checkpoint_directory);
    let _ = fs::remove_dir_all(second_checkpoint_directory);
}

#[test]
fn staged_runner_persists_dataset_snapshot_artifact_only_for_disk_mode_runs() {
    let calendar_registry = calibration_stage_test_registry();

    let disk_checkpoint_directory = temporary_checkpoint_directory("disk-dataset-snapshot");

    let disk_contract = build_pre_joint_run_contract_with_mode_and_resume(
        "run-disk-dataset-snapshot",
        CalibrationExecutionMode::Disk,
        None,
    );

    let disk_report = run_staged_calibration_with_checkpointing(
        &disk_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(disk_checkpoint_directory.as_path()),
    );
    if disk_report.status != CalibrationStageExecutionStatus::Success {
        panic!("{disk_report:#?}");
    }

    let disk_artifact_path = calibration_dataset_snapshot_artifact_path(
        disk_checkpoint_directory.as_path(),
        disk_contract.run_spec.run_id.as_str(),
    );

    let disk_artifact =
        load_calibration_dataset_snapshot_artifact_from_disk(disk_artifact_path.as_path())
            .expect("disk-mode run should persist dataset snapshot artifact");

    assert_eq!(disk_artifact.run_id, disk_contract.run_spec.run_id);
    assert_eq!(disk_artifact.execution_mode, CalibrationExecutionMode::Disk);
    assert_eq!(
        disk_artifact
            .input_datasets
            .iter()
            .map(|input_dataset| input_dataset.input_name)
            .collect::<Vec<CalibrationDatasetInputName>>(),
        vec![
            CalibrationDatasetInputName::TradeEventsDataReference,
            CalibrationDatasetInputName::OptionSurfaceDataReference,
        ]
    );

    let memory_checkpoint_directory = temporary_checkpoint_directory("memory-dataset-snapshot");

    let memory_contract = build_pre_joint_run_contract_with_mode_and_resume(
        "run-memory-dataset-snapshot",
        CalibrationExecutionMode::Memory,
        None,
    );

    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

    let memory_report = run_staged_calibration_with_checkpointing(
        &memory_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        Some(&mut checkpoint_store),
        Some(memory_checkpoint_directory.as_path()),
    );

    assert_eq!(
        memory_report.status,
        CalibrationStageExecutionStatus::Success
    );
    assert!(
        !calibration_dataset_snapshot_artifact_path(
            memory_checkpoint_directory.as_path(),
            memory_contract.run_spec.run_id.as_str(),
        )
        .exists()
    );

    let _ = fs::remove_dir_all(disk_checkpoint_directory);
    let _ = fs::remove_dir_all(memory_checkpoint_directory);
}

#[test]
fn staged_runner_persists_stage_parameter_artifacts_and_manifest_only_for_disk_mode_runs() {
    let calendar_registry = calibration_stage_test_registry();

    let disk_checkpoint_directory =
        temporary_checkpoint_directory("disk-stage-parameter-artifacts");

    let disk_contract = build_pre_joint_run_contract_with_mode_and_resume(
        "run-disk-stage-parameter-artifacts",
        CalibrationExecutionMode::Disk,
        None,
    );

    let disk_report = run_staged_calibration_with_checkpointing(
        &disk_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(disk_checkpoint_directory.as_path()),
    );

    assert_eq!(disk_report.status, CalibrationStageExecutionStatus::Success);

    for stage in [
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
        CalibrationStage::ReturnComponents,
    ] {
        let artifact_path = calibration_stage_parameter_artifact_path(
            disk_checkpoint_directory.as_path(),
            disk_contract.run_spec.run_id.as_str(),
            stage,
        );
        let artifact = load_calibration_stage_parameter_artifact_from_disk(artifact_path.as_path())
            .expect("disk-mode run should persist stage parameter artifact");

        assert_eq!(artifact.run_id, disk_contract.run_spec.run_id);
        assert_eq!(artifact.execution_mode, CalibrationExecutionMode::Disk);
        assert_eq!(artifact.stage, stage);
        assert!(
            disk_checkpoint_directory
                .join("artifacts")
                .join("calibration")
                .join(format!(
                    "{}.stage_report.json",
                    calibration_stage_token(stage)
                ))
                .exists(),
            "disk-mode run should persist stage report snapshot for {stage:?}"
        );
    }

    let manifest_path = calibration_artifact_manifest_path(
        disk_checkpoint_directory.as_path(),
        disk_contract.run_spec.run_id.as_str(),
    );
    let manifest = load_calibration_artifact_manifest_from_disk(manifest_path.as_path())
        .expect("disk-mode run should persist artifact manifest");

    assert_eq!(manifest.run_id, disk_contract.run_spec.run_id);
    assert_eq!(manifest.execution_mode, CalibrationExecutionMode::Disk);
    assert_eq!(
        manifest
            .artifact_references
            .iter()
            .filter(|artifact_reference| artifact_reference.artifact_kind
                == CalibrationArtifactKind::StageOutput)
            .count(),
        3
    );
    assert_eq!(
        manifest
            .artifact_references
            .iter()
            .filter(|artifact_reference| artifact_reference.artifact_kind
                == CalibrationArtifactKind::StageCheckpoint)
            .count(),
        3
    );

    let memory_checkpoint_directory =
        temporary_checkpoint_directory("memory-stage-parameter-artifacts");

    let memory_contract = build_pre_joint_run_contract_with_mode_and_resume(
        "run-memory-stage-parameter-artifacts",
        CalibrationExecutionMode::Memory,
        None,
    );

    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

    let memory_report = run_staged_calibration_with_checkpointing(
        &memory_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        Some(&mut checkpoint_store),
        Some(memory_checkpoint_directory.as_path()),
    );

    assert_eq!(
        memory_report.status,
        CalibrationStageExecutionStatus::Success
    );

    for stage in [
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
        CalibrationStage::ReturnComponents,
        CalibrationStage::MeanReversion,
        CalibrationStage::JointRefinement,
    ] {
        assert!(
            !calibration_stage_parameter_artifact_path(
                memory_checkpoint_directory.as_path(),
                memory_contract.run_spec.run_id.as_str(),
                stage,
            )
            .exists()
        );
    }

    assert!(
        !calibration_artifact_manifest_path(
            memory_checkpoint_directory.as_path(),
            memory_contract.run_spec.run_id.as_str(),
        )
        .exists()
    );

    let _ = fs::remove_dir_all(disk_checkpoint_directory);
    let _ = fs::remove_dir_all(memory_checkpoint_directory);
}

#[test]
fn staged_runner_rejects_silent_overwrite_of_committed_disk_artifacts() {
    let calendar_registry = calibration_stage_test_registry();

    let checkpoint_directory = temporary_checkpoint_directory("disk-overwrite-protection");

    let run_contract = build_pre_joint_run_contract_with_mode_and_resume(
        "run-disk-overwrite-protection",
        CalibrationExecutionMode::Disk,
        None,
    );

    let first_report = run_staged_calibration_with_checkpointing(
        &run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(checkpoint_directory.as_path()),
    );

    assert_eq!(
        first_report.status,
        CalibrationStageExecutionStatus::Success
    );

    let second_report = run_staged_calibration_with_checkpointing(
        &run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(checkpoint_directory.as_path()),
    );

    assert_eq!(
        second_report.status,
        CalibrationStageExecutionStatus::Failure
    );

    match second_report.failure {
        Some(CalibrationStagedRunFailure::ArtifactManifestPersistFailed { error_message }) => {
            assert!(error_message.contains("already exists"));
        }
        _ => panic!("expected committed artifact overwrite to fail before execution"),
    }

    let _ = fs::remove_dir_all(checkpoint_directory);
}

#[test]
fn staged_runner_disk_resume_replaces_committed_artifacts_after_success() {
    let calendar_registry = calibration_stage_test_registry();
    let checkpoint_directory = temporary_checkpoint_directory("disk-resume-replace-artifacts");
    let run_id = "run-disk-resume-replace-artifacts";

    let initial_run_contract = build_through_mean_reversion_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Disk,
        None,
    );
    let initial_report = run_staged_calibration_with_checkpointing(
        &initial_run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(checkpoint_directory.as_path()),
    );
    assert_eq!(
        initial_report.status,
        CalibrationStageExecutionStatus::Success
    );
    assert!(
        calibration_stage_parameter_artifact_path(
            checkpoint_directory.as_path(),
            run_id,
            CalibrationStage::MeanReversion,
        )
        .is_file()
    );

    let resume_run_contract = build_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Disk,
        Some(CalibrationResumeSettings::persisted_checkpoint_reference(
            calibration_stage_checkpoint_reference(run_id, CalibrationStage::MeanReversion),
            Some(CalibrationStage::JointRefinement),
        )),
    );
    let resume_report = run_staged_calibration_with_checkpointing(
        &resume_run_contract,
        &calendar_registry,
        staged_run_inputs(false, false, true),
        None,
        Some(checkpoint_directory.as_path()),
    );

    assert!(
        resume_report.status.is_successful(),
        "resume failure: {:#?}",
        resume_report.failure
    );
    assert_eq!(
        resume_report.started_from_stage,
        Some(CalibrationStage::JointRefinement)
    );
    assert!(
        calibration_stage_parameter_artifact_path(
            checkpoint_directory.as_path(),
            run_id,
            CalibrationStage::JointRefinement,
        )
        .is_file()
    );

    let _ = fs::remove_dir_all(checkpoint_directory);
}

#[test]
fn staged_runner_disk_resume_reports_incompatible_upstream_stage_artifact() {
    let calendar_registry = calibration_stage_test_registry();
    let checkpoint_directory = temporary_checkpoint_directory("disk-resume-stale-stage-artifact");
    let run_id = "run-disk-resume-stale-stage-artifact";

    let initial_run_contract = build_through_mean_reversion_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Disk,
        None,
    );
    let initial_report = run_staged_calibration_with_checkpointing(
        &initial_run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(checkpoint_directory.as_path()),
    );
    assert_eq!(
        initial_report.status,
        CalibrationStageExecutionStatus::Success
    );

    let return_components_artifact_path = calibration_stage_parameter_artifact_path(
        checkpoint_directory.as_path(),
        run_id,
        CalibrationStage::ReturnComponents,
    );
    let mut artifact_json: Value = serde_json::from_str(
        fs::read_to_string(return_components_artifact_path.as_path())
            .expect("return-components artifact should be readable")
            .as_str(),
    )
    .expect("return-components artifact should parse as JSON");

    artifact_json
        .get_mut("stage_parameters")
        .and_then(|stage_parameters| stage_parameters.get_mut("payload"))
        .and_then(|payload| payload.get_mut("fitted_parameters"))
        .and_then(Value::as_object_mut)
        .expect("return-components fitted parameters should be an object")
        .remove("return_clock_exposure_parameters");

    fs::write(
        return_components_artifact_path.as_path(),
        serde_json::to_string_pretty(&artifact_json)
            .expect("corrupted return-components artifact should serialize"),
    )
    .expect("corrupted return-components artifact should be writable");

    let resume_run_contract = build_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Disk,
        Some(CalibrationResumeSettings::persisted_checkpoint_reference(
            calibration_stage_checkpoint_reference(run_id, CalibrationStage::MeanReversion),
            Some(CalibrationStage::JointRefinement),
        )),
    );
    let resume_report = run_staged_calibration_with_checkpointing(
        &resume_run_contract,
        &calendar_registry,
        staged_run_inputs(false, false, true),
        None,
        Some(checkpoint_directory.as_path()),
    );

    assert_eq!(
        resume_report.status,
        CalibrationStageExecutionStatus::Failure
    );
    match resume_report.failure {
        Some(CalibrationStagedRunFailure::StageParameterArtifactLoadFailed {
            stage,
            error_message,
            ..
        }) => {
            assert_eq!(stage, CalibrationStage::ReturnComponents);
            assert!(error_message.contains("current calibration artifact contract"));
            assert!(error_message.contains("return_clock_exposure_parameters"));
        }
        failure => panic!("expected stage artifact load failure, found {failure:#?}"),
    }

    let _ = fs::remove_dir_all(checkpoint_directory);
}

#[test]
fn staged_runner_full_completion_supports_default_and_override_mode_paths() {
    let calendar_registry = calibration_stage_test_registry();

    assert_eq!(
        resolve_calibration_execution_mode_for_surface(CalibrationCallerSurface::Cli, None),
        CalibrationExecutionMode::Disk
    );
    assert_eq!(
        resolve_calibration_execution_mode_for_surface(
            CalibrationCallerSurface::PythonImportableApi,
            None
        ),
        CalibrationExecutionMode::Memory
    );
    assert_eq!(
        resolve_calibration_execution_mode_for_surface(
            CalibrationCallerSurface::Cli,
            Some(CalibrationExecutionMode::Memory)
        ),
        CalibrationExecutionMode::Memory
    );
    assert_eq!(
        resolve_calibration_execution_mode_for_surface(
            CalibrationCallerSurface::PythonImportableApi,
            Some(CalibrationExecutionMode::Disk)
        ),
        CalibrationExecutionMode::Disk
    );

    let cli_default_checkpoint_directory = temporary_checkpoint_directory("cli-default-run");
    let cli_default_contract = build_run_contract_for_caller_surface(
        "run-cli-default-completion",
        CalibrationCallerSurface::Cli,
        None,
    );
    let cli_default_report = run_staged_calibration_with_checkpointing(
        &cli_default_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(cli_default_checkpoint_directory.as_path()),
    );
    assert!(
        cli_default_report.status.is_successful(),
        "{cli_default_report:#?}"
    );
    assert_eq!(
        cli_default_report.execution_mode,
        CalibrationExecutionMode::Disk
    );
    assert_eq!(cli_default_report.stage_reports.len(), 7);

    let python_override_checkpoint_directory =
        temporary_checkpoint_directory("python-disk-override-run");
    let python_override_contract = build_run_contract_for_caller_surface(
        "run-python-disk-override",
        CalibrationCallerSurface::PythonImportableApi,
        Some(CalibrationExecutionMode::Disk),
    );
    let python_override_report = run_staged_calibration_with_checkpointing(
        &python_override_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(python_override_checkpoint_directory.as_path()),
    );
    assert!(
        python_override_report.status.is_successful(),
        "{python_override_report:#?}"
    );
    assert_eq!(
        python_override_report.execution_mode,
        CalibrationExecutionMode::Disk
    );
    assert_eq!(python_override_report.stage_reports.len(), 7);

    let _ = fs::remove_dir_all(cli_default_checkpoint_directory);
    let _ = fs::remove_dir_all(python_override_checkpoint_directory);
}

#[test]
fn staged_runner_stage_abort_failure_report_exposes_schema_and_triage_metadata() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-stage-abort-metadata",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

    let mut staged_inputs = staged_run_inputs(true, true, true);
    staged_inputs.hawkes_candidate_inputs =
        Some(vec![hawkes_stage_constraint_violating_candidate_fixture()]);

    let report = run_staged_calibration_with_checkpointing(
        &run_contract,
        &calendar_registry,
        staged_inputs,
        Some(&mut checkpoint_store),
        None,
    );

    assert_eq!(
        report.schema_version,
        CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION
    );
    assert_eq!(
        report.failure_triage_policy,
        CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY
    );
    assert_eq!(report.status, CalibrationStageExecutionStatus::Failure);
    assert_eq!(
        report.started_from_stage,
        Some(CalibrationStage::Seasonality)
    );
    assert_eq!(
        report.failure,
        Some(CalibrationStagedRunFailure::StageExecutionFailed {
            stage: CalibrationStage::Hawkes,
            error_message: None,
        })
    );
    assert_eq!(report.stage_reports.len(), 2);
    assert_eq!(report.emitted_checkpoints.len(), 1);

    let hawkes_stage_report = report
        .stage_reports
        .iter()
        .find_map(|stage_report| match stage_report {
            CalibrationStagedRunStageReport::Hawkes(hawkes_report) => Some(hawkes_report),
            _ => None,
        })
        .expect("stage-abort report should include hawkes stage report");
    assert_eq!(
        hawkes_stage_report.schema_version,
        CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION
    );
    assert_eq!(
        hawkes_stage_report.status,
        CalibrationStageExecutionStatus::Failure
    );
    assert!(matches!(
        hawkes_stage_report.failure,
        Some(HawkesStageExecutionFailure::ConstraintViolationsForAllCandidates { .. })
    ));
    let candidate_diagnostics = match hawkes_stage_report
        .failure
        .as_ref()
        .expect("hawkes failure payload should be present")
    {
        HawkesStageExecutionFailure::ConstraintViolationsForAllCandidates {
            candidate_diagnostics,
        } => candidate_diagnostics,
        _ => panic!("expected constraint violation diagnostics"),
    };
    assert_eq!(candidate_diagnostics.len(), 1);
    assert_eq!(
        candidate_diagnostics[0].candidate_id,
        "candidate_invalid_constraints"
    );
    assert!(candidate_diagnostics[0].constraint_violation_count > 0);

    let serialized_report =
        serde_json::to_value(&report).expect("staged run report should serialize");
    assert_eq!(
        serialized_report["schema_version"],
        json!(CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION)
    );
    assert_eq!(
        serialized_report["failure_triage_policy"],
        json!(CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY)
    );
    assert_eq!(
        serialized_report["failure"]["failure_kind"],
        json!("stage_execution_failed")
    );
    assert_eq!(serialized_report["failure"]["stage"], json!("hawkes"));
    assert_eq!(
        serialized_report["started_from_stage"],
        json!("seasonality")
    );
}

#[test]
fn staged_runner_reports_missing_persisted_checkpoint_artifacts_in_disk_mode() {
    let run_id = "run-disk-missing-artifact";
    let checkpoint_directory = temporary_checkpoint_directory("missing-artifact");
    let calendar_registry = calibration_stage_test_registry();

    let run_contract = build_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Disk,
        Some(CalibrationResumeSettings::persisted_checkpoint_reference(
            calibration_last_success_checkpoint_reference(run_id),
            None,
        )),
    );
    let report = run_staged_calibration_with_checkpointing(
        &run_contract,
        &calendar_registry,
        staged_run_inputs(false, false, true),
        None,
        Some(checkpoint_directory.as_path()),
    );

    assert_eq!(
        report.schema_version,
        CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION
    );
    assert_eq!(report.status, CalibrationStageExecutionStatus::Failure);
    assert!(report.stage_reports.is_empty());
    assert!(report.emitted_checkpoints.is_empty());
    match report.failure {
        Some(CalibrationStagedRunFailure::CheckpointLoadFailed {
            checkpoint_reference,
            error_message,
        }) => {
            assert_eq!(
                checkpoint_reference,
                calibration_last_success_checkpoint_reference(run_id)
            );
            assert!(!error_message.trim().is_empty());
        }
        _ => panic!("expected missing persisted artifact to produce CheckpointLoadFailed"),
    }

    let _ = fs::remove_dir_all(checkpoint_directory);
}

#[test]
fn staged_calibration_failure_playbook_is_machine_readable_and_complete() {
    let playbook_json = include_str!(
        "../../../tests/fixtures/calibration/staged_calibration_failure_playbook.json"
    );
    let playbook_value: serde_json::Value =
        serde_json::from_str(playbook_json).expect("playbook JSON should parse");

    assert_eq!(
        playbook_value["schema_version"],
        json!("calibration.orchestration.failure_playbook.v1")
    );
    assert_eq!(
        playbook_value["triage_policy"],
        json!(CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY)
    );

    let entries = playbook_value["entries"]
        .as_array()
        .expect("playbook entries should be an array");
    let failure_kinds: BTreeSet<String> = entries
        .iter()
        .map(|entry| {
            entry["failure_kind"]
                .as_str()
                .expect("failure_kind should be a string")
                .to_string()
        })
        .collect();

    let expected_failure_kinds: BTreeSet<String> = [
        "invalid_run_contract",
        "missing_memory_checkpoint_store",
        "missing_disk_checkpoint_directory",
        "missing_resume_checkpoint_for_run",
        "dataset_snapshot_persist_failed",
        "missing_stage_input",
        "missing_in_memory_upstream_payload",
        "invalid_checkpoint_reference",
        "checkpoint_load_failed",
        "checkpoint_compatibility_failed",
        "checkpoint_persist_failed",
        "missing_checkpoint_metadata_for_successful_stage",
        "stage_execution_failed",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();

    assert_eq!(failure_kinds, expected_failure_kinds);

    for entry in entries {
        let triage_steps = entry["triage_steps"]
            .as_array()
            .expect("triage_steps should be an array");
        let remediation_steps = entry["remediation_steps"]
            .as_array()
            .expect("remediation_steps should be an array");
        assert!(!triage_steps.is_empty());
        assert!(!remediation_steps.is_empty());
    }
}

#[test]
fn staged_runner_resume_from_last_successful_stage_works_in_memory_mode() {
    let run_id = "run-memory-resume-last";
    let calendar_registry = calibration_stage_test_registry();
    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

    let initial_run_contract =
        build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Memory, None);
    let initial_report = run_staged_calibration_with_checkpointing(
        &initial_run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, false),
        Some(&mut checkpoint_store),
        None,
    );
    assert_eq!(
        initial_report.status,
        CalibrationStageExecutionStatus::Failure
    );
    assert_eq!(initial_report.emitted_checkpoints.len(), 2);
    assert_eq!(
        initial_report.failure,
        Some(CalibrationStagedRunFailure::MissingStageInput {
            stage: CalibrationStage::ReturnComponents
        })
    );

    let resume_run_contract = build_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Memory,
        Some(CalibrationResumeSettings::same_process(None)),
    );
    let resume_report = run_staged_calibration_with_checkpointing(
        &resume_run_contract,
        &calendar_registry,
        staged_run_inputs(false, false, true),
        Some(&mut checkpoint_store),
        None,
    );

    assert!(resume_report.status.is_successful(), "{resume_report:#?}");
    assert_eq!(
        resume_report.resumed_from_last_successful_stage,
        Some(CalibrationStage::Hawkes)
    );
    assert_eq!(
        resume_report.started_from_stage,
        Some(CalibrationStage::ReturnComponents)
    );
    assert_eq!(resume_report.stage_reports.len(), 5);
    assert_eq!(resume_report.emitted_checkpoints.len(), 5);
}

#[test]
fn staged_runner_resume_from_selected_stage_works_in_memory_mode() {
    let run_id = "run-memory-resume-stage";
    let calendar_registry = calibration_stage_test_registry();
    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

    let initial_run_contract =
        build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Memory, None);
    let initial_report = run_staged_calibration_with_checkpointing(
        &initial_run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        Some(&mut checkpoint_store),
        None,
    );
    assert!(initial_report.status.is_successful(), "{initial_report:#?}");

    let resume_run_contract = build_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Memory,
        Some(CalibrationResumeSettings::same_process(Some(
            CalibrationStage::ReturnComponents,
        ))),
    );
    let resume_report = run_staged_calibration_with_checkpointing(
        &resume_run_contract,
        &calendar_registry,
        staged_run_inputs(false, false, true),
        Some(&mut checkpoint_store),
        None,
    );

    assert!(resume_report.status.is_successful(), "{resume_report:#?}");
    assert_eq!(
        resume_report.started_from_stage,
        Some(CalibrationStage::ReturnComponents)
    );
    assert_eq!(resume_report.stage_reports.len(), 5);
    assert_eq!(resume_report.emitted_checkpoints.len(), 5);
}

#[test]
fn staged_runner_resume_from_last_and_selected_stage_work_in_disk_mode() {
    let run_id = "run-disk-resume";
    let checkpoint_directory = temporary_checkpoint_directory("disk-resume");
    let calendar_registry = calibration_stage_test_registry();

    let initial_run_contract = build_pre_joint_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Disk,
        None,
    );
    let initial_report = run_staged_calibration_with_checkpointing(
        &initial_run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, false),
        None,
        Some(checkpoint_directory.as_path()),
    );
    assert_eq!(
        initial_report.status,
        CalibrationStageExecutionStatus::Failure
    );
    assert_eq!(initial_report.emitted_checkpoints.len(), 2);
    assert!(
        calibration_stage_checkpoint_path(
            checkpoint_directory.as_path(),
            run_id,
            CalibrationStage::Hawkes,
        )
        .exists()
    );
    assert!(
        calibration_last_success_checkpoint_path(checkpoint_directory.as_path(), run_id).exists()
    );
    assert!(!calibration_artifact_manifest_path(checkpoint_directory.as_path(), run_id).exists());
    assert!(
        checkpoint_directory
            .join("artifacts")
            .join("pending")
            .join("calibration")
            .exists()
    );
    assert!(
        checkpoint_directory
            .join("artifacts")
            .join("pending")
            .join("calibration")
            .join("seasonality.stage_report.json")
            .exists()
    );
    assert!(
        checkpoint_directory
            .join("artifacts")
            .join("pending")
            .join("calibration")
            .join("hawkes.stage_report.json")
            .exists()
    );

    let resume_from_last_contract = build_pre_joint_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Disk,
        Some(CalibrationResumeSettings::persisted_checkpoint_reference(
            calibration_last_success_checkpoint_reference(run_id),
            None,
        )),
    );
    let resume_from_last_report = run_staged_calibration_with_checkpointing(
        &resume_from_last_contract,
        &calendar_registry,
        staged_run_inputs(false, false, true),
        None,
        Some(checkpoint_directory.as_path()),
    );

    assert_eq!(
        resume_from_last_report.status,
        CalibrationStageExecutionStatus::Success,
        "{resume_from_last_report:#?}"
    );
    assert_eq!(
        resume_from_last_report.started_from_stage,
        Some(CalibrationStage::ReturnComponents)
    );
    assert_eq!(resume_from_last_report.stage_reports.len(), 1);
    assert_eq!(resume_from_last_report.emitted_checkpoints.len(), 1);
    assert!(calibration_artifact_manifest_path(checkpoint_directory.as_path(), run_id).exists());
    assert!(
        !checkpoint_directory
            .join("artifacts")
            .join("pending")
            .join("calibration")
            .exists()
    );
    assert!(
        checkpoint_directory
            .join("artifacts")
            .join("calibration")
            .join("return_components.stage_report.json")
            .exists()
    );

    let second_checkpoint_directory = temporary_checkpoint_directory("disk-resume-selected-stage");
    let second_initial_run_contract = build_pre_joint_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Disk,
        None,
    );
    let second_initial_report = run_staged_calibration_with_checkpointing(
        &second_initial_run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, false),
        None,
        Some(second_checkpoint_directory.as_path()),
    );
    assert_eq!(
        second_initial_report.status,
        CalibrationStageExecutionStatus::Failure
    );

    let resume_from_stage_contract = build_pre_joint_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Disk,
        Some(CalibrationResumeSettings::persisted_checkpoint_reference(
            calibration_last_success_checkpoint_reference(run_id),
            Some(CalibrationStage::Hawkes),
        )),
    );
    let resume_from_stage_report = run_staged_calibration_with_checkpointing(
        &resume_from_stage_contract,
        &calendar_registry,
        staged_run_inputs(false, true, true),
        None,
        Some(second_checkpoint_directory.as_path()),
    );
    assert_eq!(
        resume_from_stage_report.status,
        CalibrationStageExecutionStatus::Failure
    );
    assert_eq!(
        resume_from_stage_report.started_from_stage,
        Some(CalibrationStage::Hawkes)
    );
    assert!(matches!(
        resume_from_stage_report.failure,
        Some(
            CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                required_stage: CalibrationStage::Seasonality,
                consuming_stage: CalibrationStage::Hawkes,
            }
        )
    ));

    let _ = fs::remove_dir_all(checkpoint_directory);
    let _ = fs::remove_dir_all(second_checkpoint_directory);
}

#[test]
fn staged_runner_rejects_incompatible_checkpoints_and_mode_specific_runtime_requirements() {
    let checkpoint_directory = temporary_checkpoint_directory("incompatible-checkpoint");
    let calendar_registry = calibration_stage_test_registry();

    let source_run_contract =
        build_run_contract_with_mode_and_resume("run-source", CalibrationExecutionMode::Disk, None);
    let _source_report = run_staged_calibration_with_checkpointing(
        &source_run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, false),
        None,
        Some(checkpoint_directory.as_path()),
    );

    let incompatible_run_contract = build_run_contract_with_mode_and_resume(
        "run-target",
        CalibrationExecutionMode::Disk,
        Some(CalibrationResumeSettings::persisted_checkpoint_reference(
            calibration_last_success_checkpoint_reference("run-source"),
            None,
        )),
    );
    let incompatible_report = run_staged_calibration_with_checkpointing(
        &incompatible_run_contract,
        &calendar_registry,
        staged_run_inputs(false, false, true),
        None,
        Some(checkpoint_directory.as_path()),
    );

    assert_eq!(
        incompatible_report.status,
        CalibrationStageExecutionStatus::Failure
    );
    assert!(matches!(
        incompatible_report.failure,
        Some(CalibrationStagedRunFailure::CheckpointCompatibilityFailed { .. })
    ));

    let memory_resume_contract = build_run_contract_with_mode_and_resume(
        "run-memory-missing-store",
        CalibrationExecutionMode::Memory,
        Some(CalibrationResumeSettings::same_process(None)),
    );
    let missing_store_report = run_staged_calibration_with_checkpointing(
        &memory_resume_contract,
        &calendar_registry,
        staged_run_inputs(false, false, true),
        None,
        None,
    );

    assert_eq!(
        missing_store_report.status,
        CalibrationStageExecutionStatus::Failure
    );
    assert_eq!(
        missing_store_report.failure,
        Some(CalibrationStagedRunFailure::MissingMemoryCheckpointStore)
    );

    let disk_missing_directory_contract = build_run_contract_with_mode_and_resume(
        "run-disk-missing-directory",
        CalibrationExecutionMode::Disk,
        None,
    );
    let missing_directory_report = run_staged_calibration_with_checkpointing(
        &disk_missing_directory_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        None,
    );

    assert_eq!(
        missing_directory_report.failure,
        Some(CalibrationStagedRunFailure::MissingDiskCheckpointDirectory)
    );

    let _ = fs::remove_dir_all(checkpoint_directory);
}

fn normalized_report_for_reproducibility(
    mut report: CalibrationStagedRunReport,
) -> CalibrationStagedRunReport {
    for stage_report in &mut report.stage_reports {
        match stage_report {
            CalibrationStagedRunStageReport::JointRefinement(report)
            | CalibrationStagedRunStageReport::OptionRefinement(report)
            | CalibrationStagedRunStageReport::Finalization(report) => {
                if let Some(fitted_stage_payload) = report.fitted_stage_payload.as_mut() {
                    normalize_joint_refinement_runtime_profile_for_reproducibility(
                        &mut fitted_stage_payload.optimizer_diagnostics.runtime_profile,
                    );
                    for timing_diagnostic in &mut fitted_stage_payload
                        .optimizer_diagnostics
                        .restart_evaluation_timing_diagnostics
                    {
                        timing_diagnostic.elapsed_millis = 0;
                    }
                }
            }
            CalibrationStagedRunStageReport::Seasonality(_)
            | CalibrationStagedRunStageReport::Hawkes(_)
            | CalibrationStagedRunStageReport::ReturnComponents(_)
            | CalibrationStagedRunStageReport::LongVariance(_)
            | CalibrationStagedRunStageReport::MeanReversion(_) => {}
        }
    }

    report.diagnostic_artifact_bundle = None;

    report
}

fn assert_reproducible_reports_match(
    left_report: CalibrationStagedRunReport,
    right_report: CalibrationStagedRunReport,
) {
    let left_report = normalized_report_for_reproducibility(left_report);
    let right_report = normalized_report_for_reproducibility(right_report);

    if left_report == right_report {
        return;
    }

    let left_value =
        serde_json::to_value(&left_report).expect("normalized staged report should serialize");
    let right_value =
        serde_json::to_value(&right_report).expect("normalized staged report should serialize");

    let difference_path = first_json_difference_path(&left_value, &right_value)
        .unwrap_or_else(|| String::from("<unknown>"));

    panic!(
        "normalized staged reports drifted at {difference_path}: left={:?}, right={:?}",
        value_at_debug_path(&left_value, &difference_path),
        value_at_debug_path(&right_value, &difference_path)
    );
}

fn first_json_difference_path(left_value: &Value, right_value: &Value) -> Option<String> {
    first_json_difference_path_at(left_value, right_value, "$")
}

fn first_json_difference_path_at(
    left_value: &Value,
    right_value: &Value,
    path: &str,
) -> Option<String> {
    match (left_value, right_value) {
        (Value::Object(left_object), Value::Object(right_object)) => {
            for key in left_object.keys() {
                if !right_object.contains_key(key) {
                    return Some(format!("{path}.{key}"));
                }

                let left_child = &left_object[key];
                let right_child = &right_object[key];
                if let Some(child_path) =
                    first_json_difference_path_at(left_child, right_child, &format!("{path}.{key}"))
                {
                    return Some(child_path);
                }
            }

            for key in right_object.keys() {
                if !left_object.contains_key(key) {
                    return Some(format!("{path}.{key}"));
                }
            }

            None
        }
        (Value::Array(left_array), Value::Array(right_array)) => {
            if left_array.len() != right_array.len() {
                return Some(format!("{path}.len"));
            }

            for (array_index, (left_child, right_child)) in
                left_array.iter().zip(right_array.iter()).enumerate()
            {
                if let Some(child_path) = first_json_difference_path_at(
                    left_child,
                    right_child,
                    &format!("{path}[{array_index}]"),
                ) {
                    return Some(child_path);
                }
            }

            None
        }
        _ => {
            if left_value == right_value {
                None
            } else {
                Some(path.to_owned())
            }
        }
    }
}

fn value_at_debug_path<'value>(value: &'value Value, path: &str) -> Option<&'value Value> {
    let mut current_value = value;

    for segment in path.trim_start_matches('$').split('.') {
        if segment.is_empty() {
            continue;
        }

        let (field_name, array_indexes) = split_debug_path_segment(segment);
        if !field_name.is_empty() {
            current_value = current_value.get(field_name)?;
        }

        for array_index in array_indexes {
            current_value = current_value.get(array_index)?;
        }
    }

    Some(current_value)
}

fn split_debug_path_segment(segment: &str) -> (&str, Vec<usize>) {
    let field_end = segment.find('[').unwrap_or(segment.len());

    let mut array_indexes = Vec::<usize>::new();
    let mut remaining = &segment[field_end..];

    while let Some(index_start) = remaining.strip_prefix('[') {
        let Some(index_end) = index_start.find(']') else {
            break;
        };
        if let Ok(array_index) = index_start[..index_end].parse::<usize>() {
            array_indexes.push(array_index);
        }
        remaining = &index_start[index_end + 1..];
    }

    (&segment[..field_end], array_indexes)
}
