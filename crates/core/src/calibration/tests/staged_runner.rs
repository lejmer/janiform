use super::support::*;

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

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);
    assert!(report.failure.is_none());
    assert_eq!(report.emitted_checkpoints.len(), 3);
    assert_eq!(
        checkpoint_store.checkpoint_count_for_run(run_contract.run_spec.run_id.as_str()),
        3
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
        calibration_checkpoint_reference_for_stage(
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
        CalibrationStage::ReturnComponents
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

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);
    assert_eq!(report.stage_reports.len(), 3);

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

    assert_eq!(first_memory_report, second_memory_report);

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
    let second_disk_report = run_staged_calibration_with_checkpointing(
        &disk_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        Some(checkpoint_directory.as_path()),
    );

    assert_eq!(first_disk_report, second_disk_report);

    let _ = fs::remove_dir_all(checkpoint_directory);
}

#[test]
fn staged_runner_full_completion_supports_default_and_override_mode_paths() {
    let calendar_registry = calibration_stage_test_registry();

    let python_default_contract = build_run_contract_for_caller_surface(
        "run-python-default-completion",
        CalibrationCallerSurface::PythonImportableApi,
        None,
    );
    let mut python_default_store = CalibrationMemoryCheckpointStore::new();
    let python_default_report = run_staged_calibration_with_checkpointing(
        &python_default_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        Some(&mut python_default_store),
        None,
    );
    assert_eq!(
        python_default_report.status,
        CalibrationStageExecutionStatus::Success
    );
    assert_eq!(
        python_default_report.execution_mode,
        CalibrationExecutionMode::Memory
    );
    assert_eq!(python_default_report.stage_reports.len(), 3);

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
    assert_eq!(
        cli_default_report.status,
        CalibrationStageExecutionStatus::Success
    );
    assert_eq!(
        cli_default_report.execution_mode,
        CalibrationExecutionMode::Disk
    );
    assert_eq!(cli_default_report.stage_reports.len(), 3);

    let cli_override_contract = build_run_contract_for_caller_surface(
        "run-cli-memory-override",
        CalibrationCallerSurface::Cli,
        Some(CalibrationExecutionMode::Memory),
    );
    let mut cli_override_store = CalibrationMemoryCheckpointStore::new();
    let cli_override_report = run_staged_calibration_with_checkpointing(
        &cli_override_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        Some(&mut cli_override_store),
        None,
    );
    assert_eq!(
        cli_override_report.status,
        CalibrationStageExecutionStatus::Success
    );
    assert_eq!(
        cli_override_report.execution_mode,
        CalibrationExecutionMode::Memory
    );
    assert_eq!(cli_override_report.stage_reports.len(), 3);

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
    assert_eq!(
        python_override_report.status,
        CalibrationStageExecutionStatus::Success
    );
    assert_eq!(
        python_override_report.execution_mode,
        CalibrationExecutionMode::Disk
    );
    assert_eq!(python_override_report.stage_reports.len(), 3);

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
            stage: CalibrationStage::Hawkes
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
        "../../../../../docs/development/guides/staged_calibration_failure_playbook.json"
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

    assert_eq!(
        resume_report.status,
        CalibrationStageExecutionStatus::Success
    );
    assert_eq!(
        resume_report.resumed_from_last_successful_stage,
        Some(CalibrationStage::Hawkes)
    );
    assert_eq!(
        resume_report.started_from_stage,
        Some(CalibrationStage::ReturnComponents)
    );
    assert_eq!(resume_report.stage_reports.len(), 1);
    assert_eq!(resume_report.emitted_checkpoints.len(), 1);
}

#[test]
fn staged_runner_resume_from_selected_stage_works_in_memory_mode() {
    let run_id = "run-memory-resume-stage";
    let calendar_registry = calibration_stage_test_registry();
    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

    let initial_run_contract =
        build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Memory, None);
    let _initial_report = run_staged_calibration_with_checkpointing(
        &initial_run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, false),
        Some(&mut checkpoint_store),
        None,
    );

    let resume_run_contract = build_run_contract_with_mode_and_resume(
        run_id,
        CalibrationExecutionMode::Memory,
        Some(CalibrationResumeSettings::same_process(Some(
            CalibrationStage::Hawkes,
        ))),
    );
    let resume_report = run_staged_calibration_with_checkpointing(
        &resume_run_contract,
        &calendar_registry,
        staged_run_inputs(false, true, true),
        Some(&mut checkpoint_store),
        None,
    );

    assert_eq!(
        resume_report.status,
        CalibrationStageExecutionStatus::Success
    );
    assert_eq!(
        resume_report.started_from_stage,
        Some(CalibrationStage::Hawkes)
    );
    assert_eq!(resume_report.stage_reports.len(), 2);
    assert_eq!(resume_report.emitted_checkpoints.len(), 2);
}

#[test]
fn staged_runner_resume_from_last_and_selected_stage_work_in_disk_mode() {
    let run_id = "run-disk-resume";
    let checkpoint_directory = temporary_checkpoint_directory("disk-resume");
    let calendar_registry = calibration_stage_test_registry();

    let initial_run_contract =
        build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Disk, None);
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
        checkpoint_directory
            .join(run_id)
            .join("hawkes.checkpoint.json")
            .exists()
    );
    assert!(
        checkpoint_directory
            .join(run_id)
            .join("last-success.checkpoint.json")
            .exists()
    );

    let resume_from_last_contract = build_run_contract_with_mode_and_resume(
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
        CalibrationStageExecutionStatus::Success
    );
    assert_eq!(
        resume_from_last_report.started_from_stage,
        Some(CalibrationStage::ReturnComponents)
    );
    assert_eq!(resume_from_last_report.stage_reports.len(), 1);

    let resume_from_stage_contract = build_run_contract_with_mode_and_resume(
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
        Some(checkpoint_directory.as_path()),
    );
    assert_eq!(
        resume_from_stage_report.status,
        CalibrationStageExecutionStatus::Success
    );
    assert_eq!(
        resume_from_stage_report.started_from_stage,
        Some(CalibrationStage::Hawkes)
    );
    assert_eq!(resume_from_stage_report.stage_reports.len(), 2);

    let _ = fs::remove_dir_all(checkpoint_directory);
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
