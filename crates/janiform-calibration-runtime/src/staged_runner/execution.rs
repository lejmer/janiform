use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::Utc;
use janiform_calibration_artifacts::CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY;
use janiform_calibration_artifacts::CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION;
use janiform_calibration_artifacts::{
    CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_SPEC_SCHEMA_VERSION,
    CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION, CalibrationArtifactKind, CalibrationArtifactReference,
    CalibrationArtifactRunPreparationMode, CalibrationArtifactRunWriter,
    CalibrationDiskCheckpointError, CalibrationExecutionMode, CalibrationResumeSource,
    CalibrationRunContract, build_calibration_artifact_manifest,
    build_calibration_dataset_snapshot_artifact, calibration_dataset_snapshot_artifact_reference,
    calibration_last_success_checkpoint_reference,
    calibration_stage_parameter_artifact_load_error_message,
    calibration_stage_parameter_artifact_path_in_run_directory,
    load_calibration_stage_parameter_artifact_from_disk, load_stage_checkpoint_from_disk,
    persist_calibration_artifact_manifest_to_run_directory,
    persist_calibration_dataset_snapshot_artifact_to_run_directory,
    persist_calibration_stage_parameter_artifact_to_run_directory,
    persist_stage_checkpoint_to_disk, resolve_calibration_stage_execution_order,
};
use serde::Serialize;
use serde_json::Value;

use super::checkpointing::{build_stage_checkpoint_metadata, build_stage_parameter_artifact_input};
use super::contracts::{
    CalibrationStagedRunFailure, CalibrationStagedRunInputs, CalibrationStagedRunObserverEvent,
    CalibrationStagedRunReport, CalibrationStagedRunStageReport,
};
use super::dispatch::{
    RunStageForStagedExecutionInput, expected_stage_output_artifact_reference,
    run_stage_for_staged_execution,
};
use super::observer::emit_observer_event;
use super::payload_state::{
    restore_in_memory_payload_state_from_stage_parameter_artifact, update_in_memory_payload_state,
};
use super::reporting::{
    build_staged_run_failure_report, diagnostic_runtime_profile_entry_for_stage_report,
    retained_stage_report,
};
use crate::checkpointing::{
    CalibrationInMemoryStagePayloadState, CalibrationMemoryCheckpointStore,
    LoadedInMemoryCheckpointState,
};
use crate::diagnostics::artifacts::{
    build_diagnostic_artifact_bundle, persist_diagnostic_artifact_bundle,
};
use crate::diagnostics::types::CalibrationDiagnosticRuntimeProfileEntry;
use crate::stages::return_components::{
    ReturnComponentsStageCandidateDiagnostic, ReturnComponentsStageExecutionFailure,
};
use crate::stages::seasonality::CalibrationStageExecutionStatus;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

/// Execute staged calibration with deterministic checkpoint emission after each successful stage.
///
/// Resume behavior is mode-aware by contract:
/// - `execution_mode=memory` supports resume only through `CalibrationMemoryCheckpointStore`
/// - `execution_mode=disk` supports cross-process resume from persisted checkpoint references
pub fn run_staged_calibration_with_checkpointing(
    run_contract: &CalibrationRunContract,
    calendar_registry: &ExchangeCalendarRegistry,
    staged_run_inputs: CalibrationStagedRunInputs,
    memory_checkpoint_store: Option<&mut CalibrationMemoryCheckpointStore>,
    disk_checkpoint_directory: Option<&Path>,
) -> CalibrationStagedRunReport {
    run_staged_calibration_with_checkpointing_and_observer(
        run_contract,
        calendar_registry,
        staged_run_inputs,
        memory_checkpoint_store,
        disk_checkpoint_directory,
        None,
    )
}

pub fn run_staged_calibration_with_checkpointing_and_observer(
    run_contract: &CalibrationRunContract,
    calendar_registry: &ExchangeCalendarRegistry,
    mut staged_run_inputs: CalibrationStagedRunInputs,
    mut memory_checkpoint_store: Option<&mut CalibrationMemoryCheckpointStore>,
    disk_checkpoint_directory: Option<&Path>,
    mut observer: Option<&mut dyn FnMut(CalibrationStagedRunObserverEvent)>,
) -> CalibrationStagedRunReport {
    if let Err(error_message) = validate_run_contract_for_staged_execution(run_contract) {
        return build_staged_run_failure_report(
            run_contract,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            CalibrationStagedRunFailure::InvalidRunContract { error_message },
        );
    }

    let run_spec = &run_contract.run_spec;
    if run_spec.execution_mode == CalibrationExecutionMode::Disk
        && disk_checkpoint_directory.is_none()
    {
        return build_staged_run_failure_report(
            run_contract,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            CalibrationStagedRunFailure::MissingDiskCheckpointDirectory,
        );
    }

    let execution_stage_order = run_contract.execution_stage_order.clone();

    let mut stage_reports = Vec::new();
    let mut emitted_checkpoints = Vec::new();
    let mut persisted_stage_parameter_artifacts = Vec::new();
    let mut payload_state = CalibrationInMemoryStagePayloadState::default();
    let mut stage_runtime_entries = Vec::<CalibrationDiagnosticRuntimeProfileEntry>::new();
    let mut resumed_from_checkpoint_reference = None;
    let mut resumed_from_last_successful_stage = None;
    let mut start_position = 0usize;

    if let Some(resume_settings) = &run_spec.resume_settings {
        let loaded_checkpoint = match &resume_settings.source {
            CalibrationResumeSource::SameProcess => {
                let Some(checkpoint_store) = memory_checkpoint_store.as_deref() else {
                    return build_staged_run_failure_report(
                        run_contract,
                        stage_reports,
                        emitted_checkpoints,
                        None,
                        None,
                        None,
                        CalibrationStagedRunFailure::MissingMemoryCheckpointStore,
                    );
                };

                let Some(loaded_state) = checkpoint_store.load_resume_state(&run_spec.run_id)
                else {
                    return build_staged_run_failure_report(
                        run_contract,
                        stage_reports,
                        emitted_checkpoints,
                        None,
                        None,
                        None,
                        CalibrationStagedRunFailure::MissingResumeCheckpointForRun {
                            run_id: run_spec.run_id.clone(),
                        },
                    );
                };

                loaded_state
            }
            CalibrationResumeSource::PersistedCheckpointReference {
                checkpoint_reference,
            } => {
                let Some(disk_checkpoint_directory) = disk_checkpoint_directory else {
                    return build_staged_run_failure_report(
                        run_contract,
                        stage_reports,
                        emitted_checkpoints,
                        None,
                        None,
                        None,
                        CalibrationStagedRunFailure::MissingDiskCheckpointDirectory,
                    );
                };

                let checkpoint_reference = checkpoint_reference.to_string();

                let checkpoint_metadata = match load_stage_checkpoint_from_disk(
                    disk_checkpoint_directory,
                    checkpoint_reference.as_str(),
                ) {
                    Ok(checkpoint_metadata) => checkpoint_metadata,
                    Err(CalibrationDiskCheckpointError::InvalidCheckpointReference {
                        checkpoint_reference,
                        source,
                    }) => {
                        return build_staged_run_failure_report(
                            run_contract,
                            stage_reports,
                            emitted_checkpoints,
                            None,
                            None,
                            None,
                            CalibrationStagedRunFailure::InvalidCheckpointReference {
                                checkpoint_reference,
                                error_message: source.to_string(),
                            },
                        );
                    }
                    Err(source) => {
                        return build_staged_run_failure_report(
                            run_contract,
                            stage_reports,
                            emitted_checkpoints,
                            None,
                            None,
                            None,
                            CalibrationStagedRunFailure::CheckpointLoadFailed {
                                checkpoint_reference,
                                error_message: source.to_string(),
                            },
                        );
                    }
                };

                LoadedInMemoryCheckpointState {
                    checkpoint_metadata,
                    payload_state: CalibrationInMemoryStagePayloadState::default(),
                }
            }
        };

        if let Err(source) = loaded_checkpoint
            .checkpoint_metadata
            .validate_compatibility_with_run_contract(run_contract)
        {
            let resume_reference = match &resume_settings.source {
                CalibrationResumeSource::SameProcess => loaded_checkpoint
                    .checkpoint_metadata
                    .checkpoint_reference
                    .clone(),
                CalibrationResumeSource::PersistedCheckpointReference {
                    checkpoint_reference,
                } => checkpoint_reference.clone(),
            };

            return build_staged_run_failure_report(
                run_contract,
                stage_reports,
                emitted_checkpoints,
                None,
                None,
                None,
                CalibrationStagedRunFailure::CheckpointCompatibilityFailed {
                    checkpoint_reference: resume_reference,
                    error_message: source.to_string(),
                },
            );
        }

        resumed_from_checkpoint_reference = Some(match &resume_settings.source {
            CalibrationResumeSource::SameProcess => loaded_checkpoint
                .checkpoint_metadata
                .checkpoint_reference
                .clone(),
            CalibrationResumeSource::PersistedCheckpointReference {
                checkpoint_reference,
            } => checkpoint_reference.clone(),
        });

        resumed_from_last_successful_stage =
            Some(loaded_checkpoint.checkpoint_metadata.completed_stage);

        payload_state = loaded_checkpoint.payload_state;

        start_position = if let Some(resume_from_stage) = resume_settings.resume_from_stage {
            let Some(position) = execution_stage_order
                .iter()
                .position(|stage| *stage == resume_from_stage)
            else {
                return build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    None,
                    CalibrationStagedRunFailure::CheckpointCompatibilityFailed {
                        checkpoint_reference: calibration_last_success_checkpoint_reference(
                            run_spec.run_id.as_str(),
                        ),
                        error_message: format!(
                            "resume_from_stage `{resume_from_stage:?}` is not in execution order `{:?}`",
                            execution_stage_order
                        ),
                    },
                );
            };

            position
        } else {
            loaded_checkpoint
                .checkpoint_metadata
                .completed_stage_position
                .saturating_add(1)
        };
    }

    let dataset_snapshot_artifact = if run_spec.execution_mode == CalibrationExecutionMode::Disk {
        match build_calibration_dataset_snapshot_artifact(run_contract) {
            Ok(artifact) => Some(artifact),
            Err(source) => {
                return build_staged_run_failure_report(
                    run_contract,
                    Vec::new(),
                    Vec::new(),
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    None,
                    CalibrationStagedRunFailure::DatasetSnapshotPersistFailed {
                        artifact_reference: calibration_dataset_snapshot_artifact_reference(
                            run_spec.run_id.as_str(),
                        ),
                        error_message: source.to_string(),
                    },
                );
            }
        }
    } else {
        None
    };

    let artifact_run_writer = if run_spec.execution_mode == CalibrationExecutionMode::Disk {
        let Some(disk_checkpoint_directory) = disk_checkpoint_directory else {
            return build_staged_run_failure_report(
                run_contract,
                Vec::new(),
                Vec::new(),
                resumed_from_checkpoint_reference,
                resumed_from_last_successful_stage,
                None,
                CalibrationStagedRunFailure::MissingDiskCheckpointDirectory,
            );
        };

        let preparation_mode = if run_spec.resume_settings.is_some() {
            CalibrationArtifactRunPreparationMode::ResumeRun
        } else {
            CalibrationArtifactRunPreparationMode::FreshRun
        };

        match CalibrationArtifactRunWriter::prepare(
            disk_checkpoint_directory,
            run_spec.run_id.as_str(),
            preparation_mode,
        ) {
            Ok(writer) => Some(writer),
            Err(source) => {
                return build_staged_run_failure_report(
                    run_contract,
                    Vec::new(),
                    Vec::new(),
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    None,
                    CalibrationStagedRunFailure::ArtifactManifestPersistFailed {
                        error_message: source.to_string(),
                    },
                );
            }
        }
    } else {
        None
    };

    if run_spec.execution_mode == CalibrationExecutionMode::Disk && start_position > 0 {
        let Some(artifact_run_writer) = artifact_run_writer.as_ref() else {
            return build_staged_run_failure_report(
                run_contract,
                stage_reports,
                emitted_checkpoints,
                resumed_from_checkpoint_reference,
                resumed_from_last_successful_stage,
                None,
                CalibrationStagedRunFailure::ArtifactManifestPersistFailed {
                    error_message: "disk-mode execution did not prepare an artifact run writer"
                        .to_string(),
                },
            );
        };

        for stage in execution_stage_order.iter().copied().take(start_position) {
            let artifact_reference = expected_stage_output_artifact_reference(run_spec, stage);

            let artifact_path = calibration_stage_parameter_artifact_path_in_run_directory(
                artifact_run_writer.pending_run_directory(),
                stage,
            );

            let artifact = match load_calibration_stage_parameter_artifact_from_disk(
                artifact_path.as_path(),
            ) {
                Ok(artifact) => artifact,
                Err(source) => {
                    return build_staged_run_failure_report(
                        run_contract,
                        stage_reports,
                        emitted_checkpoints,
                        resumed_from_checkpoint_reference,
                        resumed_from_last_successful_stage,
                        None,
                        CalibrationStagedRunFailure::StageParameterArtifactLoadFailed {
                            stage,
                            artifact_reference,
                            error_message: calibration_stage_parameter_artifact_load_error_message(
                                stage, &source,
                            ),
                        },
                    );
                }
            };

            restore_in_memory_payload_state_from_stage_parameter_artifact(
                &mut payload_state,
                &artifact,
            );

            persisted_stage_parameter_artifacts.push(artifact);
        }
    }

    let run_start_time = Instant::now();

    let started_from_stage = execution_stage_order.get(start_position).copied();
    let completed_stage_count = start_position;

    let load_current_stage_resume_state = run_spec
        .resume_settings
        .as_ref()
        .and_then(|resume_settings| resume_settings.resume_from_stage)
        .is_none();

    emit_observer_event(
        &mut observer,
        CalibrationStagedRunObserverEvent::RunStarted {
            run_id: run_spec.run_id.clone(),
            execution_mode: run_spec.execution_mode,
            stage_count: execution_stage_order.len(),
            stage_sequence: execution_stage_order.clone(),
            completed_stage_count,
            resumed_from_checkpoint_reference: resumed_from_checkpoint_reference.clone(),
            resumed_from_last_successful_stage,
            started_from_stage,
        },
    );

    if start_position >= execution_stage_order.len() {
        let completed_status = if stage_reports.iter().any(|stage_report| {
            stage_report.status() == CalibrationStageExecutionStatus::SuccessWithWarnings
        }) {
            CalibrationStageExecutionStatus::SuccessWithWarnings
        } else {
            CalibrationStageExecutionStatus::Success
        };

        let report = CalibrationStagedRunReport {
            schema_version: CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
            failure_triage_policy: CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
            run_id: run_spec.run_id.clone(),
            execution_mode: run_spec.execution_mode,
            status: completed_status,
            stage_reports,
            emitted_checkpoints,
            diagnostic_artifact_references: Vec::new(),
            diagnostic_artifact_bundle: None,
            resumed_from_checkpoint_reference,
            resumed_from_last_successful_stage,
            started_from_stage: None,
            failure: None,
        };

        emit_observer_event(
            &mut observer,
            CalibrationStagedRunObserverEvent::RunCompleted {
                run_id: report.run_id.clone(),
                execution_mode: report.execution_mode,
                total_runtime_seconds: run_start_time.elapsed().as_secs_f64(),
                stage_report_count: report.stage_reports.len(),
            },
        );

        return report;
    }

    for (stage_position, stage) in execution_stage_order
        .iter()
        .copied()
        .enumerate()
        .skip(start_position)
    {
        emit_observer_event(
            &mut observer,
            CalibrationStagedRunObserverEvent::StageStarted {
                run_id: run_spec.run_id.clone(),
                stage,
                stage_index: stage_position + 1,
                stage_count: execution_stage_order.len(),
            },
        );

        let stage_start_time = Instant::now();

        let stage_report = match run_stage_for_staged_execution(RunStageForStagedExecutionInput {
            run_spec,
            calendar_registry,
            disk_checkpoint_directory,
            stage,
            stage_index: stage_position + 1,
            stage_count: execution_stage_order.len(),
            staged_run_inputs: &mut staged_run_inputs,
            payload_state: &payload_state,
            load_current_stage_resume_state,
            observer: &mut observer,
        }) {
            Ok(stage_report) => stage_report,
            Err(failure) => {
                let failure_report = build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    started_from_stage,
                    failure.clone(),
                );

                emit_observer_event(
                    &mut observer,
                    CalibrationStagedRunObserverEvent::RunFailed {
                        run_id: run_spec.run_id.clone(),
                        execution_mode: run_spec.execution_mode,
                        total_runtime_seconds: run_start_time.elapsed().as_secs_f64(),
                        completed_stage_count: failure_report.stage_reports.len(),
                        failed_stage: Some(stage),
                        failure,
                    },
                );

                return failure_report;
            }
        };

        let stage_runtime_seconds = stage_start_time.elapsed().as_secs_f64();

        stage_runtime_entries.push(diagnostic_runtime_profile_entry_for_stage_report(
            &stage_report,
            stage_runtime_seconds,
        ));

        let stage_status = stage_report.status();
        let retained_stage_report = retained_stage_report(&stage_report);

        emit_observer_event(
            &mut observer,
            CalibrationStagedRunObserverEvent::StageCompleted {
                run_id: run_spec.run_id.clone(),
                stage,
                stage_index: stage_position + 1,
                stage_count: execution_stage_order.len(),
                runtime_seconds: stage_runtime_seconds,
                stage_report: Box::new(retained_stage_report.clone()),
            },
        );

        stage_reports.push(retained_stage_report.clone());

        if stage_status == CalibrationStageExecutionStatus::Failure {
            let failure = CalibrationStagedRunFailure::StageExecutionFailed {
                stage,
                error_message: stage_execution_error_message(&stage_report),
            };
            let failure_report = build_staged_run_failure_report(
                run_contract,
                stage_reports,
                emitted_checkpoints,
                resumed_from_checkpoint_reference,
                resumed_from_last_successful_stage,
                started_from_stage,
                failure.clone(),
            );

            emit_observer_event(
                &mut observer,
                CalibrationStagedRunObserverEvent::RunFailed {
                    run_id: run_spec.run_id.clone(),
                    execution_mode: run_spec.execution_mode,
                    total_runtime_seconds: run_start_time.elapsed().as_secs_f64(),
                    completed_stage_count: failure_report.stage_reports.len(),
                    failed_stage: Some(stage),
                    failure,
                },
            );

            return failure_report;
        }

        update_in_memory_payload_state(&mut payload_state, &stage_report);

        if run_spec.execution_mode == CalibrationExecutionMode::Disk {
            let Some(artifact_run_writer) = artifact_run_writer.as_ref() else {
                return build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    started_from_stage,
                    CalibrationStagedRunFailure::ArtifactManifestPersistFailed {
                        error_message: "disk-mode execution did not prepare an artifact run writer"
                            .to_string(),
                    },
                );
            };

            let stage_parameter_input = match build_stage_parameter_artifact_input(&stage_report) {
                Ok(input) => input,
                Err(error_message) => {
                    return build_staged_run_failure_report(
                        run_contract,
                        stage_reports,
                        emitted_checkpoints,
                        resumed_from_checkpoint_reference,
                        resumed_from_last_successful_stage,
                        started_from_stage,
                        CalibrationStagedRunFailure::StageParameterArtifactBuildFailed {
                            stage,
                            artifact_reference: expected_stage_output_artifact_reference(
                                run_spec, stage,
                            ),
                            error_message,
                        },
                    );
                }
            };

            let stage_parameter_artifact =
                match janiform_calibration_artifacts::build_calibration_stage_parameter_artifact(
                    run_contract,
                    stage_parameter_input,
                ) {
                    Ok(artifact) => artifact,
                    Err(source) => {
                        return build_staged_run_failure_report(
                            run_contract,
                            stage_reports,
                            emitted_checkpoints,
                            resumed_from_checkpoint_reference,
                            resumed_from_last_successful_stage,
                            started_from_stage,
                            CalibrationStagedRunFailure::StageParameterArtifactBuildFailed {
                                stage,
                                artifact_reference: expected_stage_output_artifact_reference(
                                    run_spec, stage,
                                ),
                                error_message: source.to_string(),
                            },
                        );
                    }
                };

            if let Err(source) = persist_calibration_stage_parameter_artifact_to_run_directory(
                artifact_run_writer.pending_run_directory(),
                &stage_parameter_artifact,
            ) {
                return build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    started_from_stage,
                    CalibrationStagedRunFailure::StageParameterArtifactPersistFailed {
                        stage,
                        artifact_reference: stage_parameter_artifact.artifact_reference.clone(),
                        error_message: source.to_string(),
                    },
                );
            }

            if let Err(error_message) = persist_stage_report_snapshot_to_run_directory(
                artifact_run_writer.pending_run_directory(),
                &retained_stage_report,
            ) {
                return build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    started_from_stage,
                    CalibrationStagedRunFailure::StageParameterArtifactPersistFailed {
                        stage,
                        artifact_reference: format!(
                            "artifact://stage_report/{}/{}",
                            run_contract.run_spec.run_id,
                            stage_report_snapshot_file_name(stage)
                        ),
                        error_message,
                    },
                );
            }

            persisted_stage_parameter_artifacts.push(stage_parameter_artifact);
        }

        let checkpoint_metadata =
            match build_stage_checkpoint_metadata(run_contract, &stage_report, stage_position) {
                Ok(checkpoint_metadata) => checkpoint_metadata,
                Err(failure) => {
                    return build_staged_run_failure_report(
                        run_contract,
                        stage_reports,
                        emitted_checkpoints,
                        resumed_from_checkpoint_reference,
                        resumed_from_last_successful_stage,
                        started_from_stage,
                        failure,
                    );
                }
            };

        if run_spec.execution_mode == CalibrationExecutionMode::Memory {
            if let Some(checkpoint_store) = memory_checkpoint_store.as_deref_mut() {
                checkpoint_store
                    .record_stage_checkpoint(checkpoint_metadata.clone(), &payload_state);
            }
        } else {
            let Some(disk_checkpoint_directory) = disk_checkpoint_directory else {
                return build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    started_from_stage,
                    CalibrationStagedRunFailure::MissingDiskCheckpointDirectory,
                );
            };

            if let Err(source) =
                persist_stage_checkpoint_to_disk(disk_checkpoint_directory, &checkpoint_metadata)
            {
                return build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    started_from_stage,
                    CalibrationStagedRunFailure::CheckpointPersistFailed {
                        checkpoint_reference: checkpoint_metadata.checkpoint_reference.clone(),
                        error_message: source.to_string(),
                    },
                );
            }
        }

        emitted_checkpoints.push(checkpoint_metadata);
    }

    let diagnostic_artifact_bundle =
        build_diagnostic_artifact_bundle(stage_reports.as_slice(), stage_runtime_entries.clone());

    let mut diagnostic_artifact_references = Vec::new();
    let mut diagnostic_manifest_artifact_references = Vec::new();

    if run_spec.execution_mode == CalibrationExecutionMode::Disk {
        let Some(artifact_run_writer) = artifact_run_writer else {
            return build_staged_run_failure_report(
                run_contract,
                stage_reports,
                emitted_checkpoints,
                resumed_from_checkpoint_reference,
                resumed_from_last_successful_stage,
                started_from_stage,
                CalibrationStagedRunFailure::ArtifactManifestPersistFailed {
                    error_message: "disk-mode execution did not prepare an artifact run writer"
                        .to_string(),
                },
            );
        };

        let Some(dataset_snapshot_artifact) = dataset_snapshot_artifact.as_ref() else {
            return build_staged_run_failure_report(
                run_contract,
                stage_reports,
                emitted_checkpoints,
                resumed_from_checkpoint_reference,
                resumed_from_last_successful_stage,
                started_from_stage,
                CalibrationStagedRunFailure::DatasetSnapshotPersistFailed {
                    artifact_reference: calibration_dataset_snapshot_artifact_reference(
                        run_spec.run_id.as_str(),
                    ),
                    error_message: "disk-mode execution did not build a dataset snapshot artifact"
                        .to_string(),
                },
            );
        };

        if let Err(source) = persist_calibration_dataset_snapshot_artifact_to_run_directory(
            artifact_run_writer.pending_run_directory(),
            dataset_snapshot_artifact,
        ) {
            return build_staged_run_failure_report(
                run_contract,
                stage_reports,
                emitted_checkpoints,
                resumed_from_checkpoint_reference,
                resumed_from_last_successful_stage,
                started_from_stage,
                CalibrationStagedRunFailure::DatasetSnapshotPersistFailed {
                    artifact_reference: calibration_dataset_snapshot_artifact_reference(
                        run_spec.run_id.as_str(),
                    ),
                    error_message: source.to_string(),
                },
            );
        }

        if let Some(diagnostic_bundle) = diagnostic_artifact_bundle.as_ref() {
            diagnostic_artifact_references = match persist_diagnostic_artifact_bundle(
                artifact_run_writer.pending_run_directory(),
                diagnostic_bundle,
            ) {
                Ok(references) => references,
                Err(error_message) => {
                    return build_staged_run_failure_report(
                        run_contract,
                        stage_reports,
                        emitted_checkpoints,
                        resumed_from_checkpoint_reference,
                        resumed_from_last_successful_stage,
                        started_from_stage,
                        CalibrationStagedRunFailure::ArtifactManifestPersistFailed {
                            error_message,
                        },
                    );
                }
            };

            diagnostic_manifest_artifact_references = diagnostic_artifact_references
                .iter()
                .map(|reference| CalibrationArtifactReference {
                    artifact_id: format!("diagnostics_{}", reference.artifact_family),
                    artifact_kind: CalibrationArtifactKind::Diagnostics,
                    artifact_reference: format!(
                        "artifact://diagnostics/{}/{}",
                        run_spec.run_id, reference.artifact_family
                    ),
                    stage: None,
                })
                .collect();
        }

        let manifest = match build_calibration_artifact_manifest(
            run_contract,
            dataset_snapshot_artifact,
            persisted_stage_parameter_artifacts.as_slice(),
            emitted_checkpoints.as_slice(),
            &[],
            diagnostic_manifest_artifact_references.as_slice(),
            Utc::now(),
        ) {
            Ok(manifest) => manifest,
            Err(source) => {
                return build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    started_from_stage,
                    CalibrationStagedRunFailure::ArtifactManifestPersistFailed {
                        error_message: source.to_string(),
                    },
                );
            }
        };

        if let Err(source) = persist_calibration_artifact_manifest_to_run_directory(
            artifact_run_writer.pending_run_directory(),
            &manifest,
        ) {
            return build_staged_run_failure_report(
                run_contract,
                stage_reports,
                emitted_checkpoints,
                resumed_from_checkpoint_reference,
                resumed_from_last_successful_stage,
                started_from_stage,
                CalibrationStagedRunFailure::ArtifactManifestPersistFailed {
                    error_message: source.to_string(),
                },
            );
        }

        if let Err(source) = artifact_run_writer.commit() {
            return build_staged_run_failure_report(
                run_contract,
                stage_reports,
                emitted_checkpoints,
                resumed_from_checkpoint_reference,
                resumed_from_last_successful_stage,
                started_from_stage,
                CalibrationStagedRunFailure::ArtifactManifestPersistFailed {
                    error_message: source.to_string(),
                },
            );
        }
    }

    let completed_status = if stage_reports.iter().any(|stage_report| {
        stage_report.status() == CalibrationStageExecutionStatus::SuccessWithWarnings
    }) {
        CalibrationStageExecutionStatus::SuccessWithWarnings
    } else {
        CalibrationStageExecutionStatus::Success
    };

    let report = CalibrationStagedRunReport {
        schema_version: CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
        failure_triage_policy: CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        status: completed_status,
        stage_reports,
        emitted_checkpoints,
        diagnostic_artifact_references,
        diagnostic_artifact_bundle: (run_spec.execution_mode == CalibrationExecutionMode::Memory)
            .then_some(diagnostic_artifact_bundle)
            .flatten(),
        resumed_from_checkpoint_reference,
        resumed_from_last_successful_stage,
        started_from_stage,
        failure: None,
    };

    emit_observer_event(
        &mut observer,
        CalibrationStagedRunObserverEvent::RunCompleted {
            run_id: report.run_id.clone(),
            execution_mode: report.execution_mode,
            total_runtime_seconds: run_start_time.elapsed().as_secs_f64(),
            stage_report_count: report.stage_reports.len(),
        },
    );

    report
}

fn persist_stage_report_snapshot_to_run_directory(
    run_directory: &Path,
    stage_report: &CalibrationStagedRunStageReport,
) -> Result<PathBuf, String> {
    fs::create_dir_all(run_directory).map_err(|source| {
        format!(
            "unable to prepare stage report directory `{}`: {source}",
            run_directory.display()
        )
    })?;

    let artifact_path =
        stage_report_snapshot_path_in_run_directory(run_directory, stage_report.stage());
    let temporary_path = artifact_path.with_extension("json.tmp");

    let serialized_payload =
        serde_json::to_vec_pretty(stage_report).map_err(|source| source.to_string())?;

    let mut temporary_file = File::create(temporary_path.as_path()).map_err(|source| {
        format!(
            "unable to create temporary stage report `{}`: {source}",
            temporary_path.display()
        )
    })?;

    temporary_file
        .write_all(serialized_payload.as_slice())
        .and_then(|_| temporary_file.flush())
        .map_err(|source| {
            format!(
                "unable to write temporary stage report `{}`: {source}",
                temporary_path.display()
            )
        })?;

    fs::rename(temporary_path.as_path(), artifact_path.as_path()).map_err(|source| {
        format!(
            "unable to publish stage report `{}` from `{}`: {source}",
            artifact_path.display(),
            temporary_path.display()
        )
    })?;

    Ok(artifact_path)
}

fn stage_report_snapshot_path_in_run_directory(
    run_directory: &Path,
    stage: janiform_calibration_artifacts::CalibrationStage,
) -> PathBuf {
    run_directory.join(stage_report_snapshot_file_name(stage))
}

fn stage_report_snapshot_file_name(
    stage: janiform_calibration_artifacts::CalibrationStage,
) -> String {
    format!(
        "{}.stage_report.json",
        janiform_calibration_artifacts::calibration_stage_token(stage)
    )
}

fn stage_execution_error_message(stage_report: &CalibrationStagedRunStageReport) -> Option<String> {
    match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => {
            serialized_failure_error_message(report.failure.as_ref())
        }
        CalibrationStagedRunStageReport::Hawkes(report) => {
            serialized_failure_error_message(report.failure.as_ref())
        }
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            return_components_failure_error_message(report.failure.as_ref())
        }
        CalibrationStagedRunStageReport::MeanReversion(report) => {
            serialized_failure_error_message(report.failure.as_ref())
        }
        CalibrationStagedRunStageReport::JointRefinement(report) => {
            serialized_failure_error_message(report.failure.as_ref())
        }
        CalibrationStagedRunStageReport::OptionRefinement(report) => {
            serialized_failure_error_message(report.failure.as_ref())
        }
        CalibrationStagedRunStageReport::Finalization(report) => {
            serialized_failure_error_message(report.failure.as_ref())
        }
    }
}

fn serialized_failure_error_message<T>(failure: Option<&T>) -> Option<String>
where
    T: Serialize,
{
    let serialized_failure = serde_json::to_value(failure).ok()?;
    let Value::Object(serialized_failure_object) = serialized_failure else {
        return None;
    };

    serialized_failure_object
        .get("error_message")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn return_components_failure_error_message(
    failure: Option<&ReturnComponentsStageExecutionFailure>,
) -> Option<String> {
    match failure {
        Some(ReturnComponentsStageExecutionFailure::ConstraintViolationsForAllCandidates {
            candidate_diagnostics,
        }) => Some(return_components_all_candidates_rejected_message(
            candidate_diagnostics.as_slice(),
        )),
        source => serialized_failure_error_message(source),
    }
}

fn return_components_all_candidates_rejected_message(
    candidate_diagnostics: &[ReturnComponentsStageCandidateDiagnostic],
) -> String {
    let Some(best_candidate) = candidate_diagnostics.iter().min_by(|left, right| {
        left.penalized_objective_value
            .total_cmp(&right.penalized_objective_value)
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    }) else {
        return "all return distribution candidates violated constraints, but no candidate diagnostics were recorded".to_string();
    };

    let first_violation_message = best_candidate
        .constraint_violations
        .first()
        .map(|violation| violation.message.as_str())
        .unwrap_or("no violation detail was recorded");

    format!(
        "all return distribution candidates violated constraints; best candidate `{}` had {} violation(s): {}",
        best_candidate.candidate_id,
        best_candidate.constraint_violation_count,
        first_violation_message
    )
}

fn validate_run_contract_for_staged_execution(
    run_contract: &CalibrationRunContract,
) -> Result<(), String> {
    if run_contract.schema_version != CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION {
        return Err(format!(
            "run_contract.schema_version must be `{}`, found `{}`",
            CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, run_contract.schema_version
        ));
    }

    if run_contract.run_spec_schema_version != CALIBRATION_RUN_SPEC_SCHEMA_VERSION {
        return Err(format!(
            "run_contract.run_spec_schema_version must be `{}`, found `{}`",
            CALIBRATION_RUN_SPEC_SCHEMA_VERSION, run_contract.run_spec_schema_version
        ));
    }

    if run_contract.stage_graph_schema_version != CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION {
        return Err(format!(
            "run_contract.stage_graph_schema_version must be `{}`, found `{}`",
            CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION, run_contract.stage_graph_schema_version
        ));
    }

    let resolved_execution_order = resolve_calibration_stage_execution_order(
        &run_contract.run_spec,
        &run_contract.stage_graph,
    )
    .map_err(|source| source.to_string())?;

    if resolved_execution_order != run_contract.execution_stage_order {
        return Err(format!(
            "run_contract.execution_stage_order is stale, expected `{:?}`, found `{:?}`",
            resolved_execution_order, run_contract.execution_stage_order
        ));
    }

    Ok(())
}
