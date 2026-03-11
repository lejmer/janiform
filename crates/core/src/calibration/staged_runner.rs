use std::path::Path;

use chrono::Utc;
use serde::Serialize;

use crate::calendar::ExchangeCalendarRegistry;

use super::artifact_registry::{
    CalibrationArtifactRunPreparationMode, CalibrationArtifactRunWriter,
    CalibrationStageParameterArtifact, build_calibration_artifact_manifest,
    build_calibration_dataset_snapshot_artifact, build_calibration_stage_parameter_artifact,
    calibration_dataset_snapshot_artifact_reference,
    calibration_stage_parameter_artifact_path_in_run_directory,
    load_calibration_stage_parameter_artifact_from_disk,
    persist_calibration_artifact_manifest_to_run_directory,
    persist_calibration_dataset_snapshot_artifact_to_run_directory,
    persist_calibration_stage_parameter_artifact_to_run_directory,
    stage_output_artifact_references_for_stage,
};
use super::checkpointing::{
    CalibrationDiskCheckpointError, CalibrationInMemoryStagePayloadState,
    CalibrationMemoryCheckpointStore, CalibrationStageCheckpointMetadata,
    LoadedInMemoryCheckpointState, calibration_stage_token, load_stage_checkpoint_from_disk,
    persist_stage_checkpoint_to_disk,
};
use super::contracts::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY,
    CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_SPEC_SCHEMA_VERSION,
    CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY, CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION,
    CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION, CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
    CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationResumeSource, CalibrationRunContract,
    CalibrationRunSpec, CalibrationStage, CalibrationStageHandoffTransport,
    CalibrationStageSeedSource, resolve_calibration_stage_execution_order,
};
use super::stages::hawkes::{
    HawkesStageCandidateInput, HawkesStageExecutionInput, HawkesStageExecutionMetadata,
    HawkesStageExecutionReport, HawkesStageSeasonalityInput, run_hawkes_calibration_stage,
};
use super::stages::return_components::{
    ReturnComponentsStageActivityInput, ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageExecutionInput, ReturnComponentsStageExecutionMetadata,
    ReturnComponentsStageExecutionReport, run_return_components_calibration_stage,
};
use super::stages::seasonality::{
    CalibrationStageExecutionStatus, SeasonalityStageExecutionInput,
    SeasonalityStageExecutionMetadata, SeasonalityStageExecutionReport,
    run_seasonality_calibration_stage,
};
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalibrationStagedRunInputs {
    pub seasonality_stage_input: Option<SeasonalityStageExecutionInput>,
    pub hawkes_candidate_inputs: Option<Vec<HawkesStageCandidateInput>>,
    pub return_components_candidate_generation_input:
        Option<ReturnComponentsStageCandidateGenerationInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "stage_report_kind",
    content = "report",
    rename_all = "snake_case"
)]
pub enum CalibrationStagedRunStageReport {
    Seasonality(SeasonalityStageExecutionReport),
    Hawkes(HawkesStageExecutionReport),
    ReturnComponents(ReturnComponentsStageExecutionReport),
}

#[derive(Debug, Clone, PartialEq)]
struct CalibrationStageCheckpointComponents {
    stage_output_schema_version: String,
    run_global_seed: u64,
    stage_seed: u64,
    stage_seed_source: CalibrationStageSeedSource,
    stage_seed_derivation_policy: &'static str,
    stage_solver_settings: CalibrationOptimizerSettings,
}

trait CalibrationStageExecutionMetadataForCheckpoint {
    fn stage_output_schema_version(&self) -> &'static str;
    fn run_global_seed(&self) -> u64;
    fn stage_seed(&self) -> u64;
    fn stage_seed_source(&self) -> CalibrationStageSeedSource;
    fn stage_seed_derivation_policy(&self) -> &'static str;
    fn solver_settings(&self) -> &CalibrationOptimizerSettings;
}

impl CalibrationStageExecutionMetadataForCheckpoint for SeasonalityStageExecutionMetadata {
    fn stage_output_schema_version(&self) -> &'static str {
        self.stage_output_schema_version
    }

    fn run_global_seed(&self) -> u64 {
        self.run_global_seed
    }

    fn stage_seed(&self) -> u64 {
        self.stage_seed
    }

    fn stage_seed_source(&self) -> CalibrationStageSeedSource {
        self.stage_seed_source
    }

    fn stage_seed_derivation_policy(&self) -> &'static str {
        self.stage_seed_derivation_policy
    }

    fn solver_settings(&self) -> &CalibrationOptimizerSettings {
        &self.solver_settings
    }
}

impl CalibrationStageExecutionMetadataForCheckpoint for HawkesStageExecutionMetadata {
    fn stage_output_schema_version(&self) -> &'static str {
        self.stage_output_schema_version
    }

    fn run_global_seed(&self) -> u64 {
        self.run_global_seed
    }

    fn stage_seed(&self) -> u64 {
        self.stage_seed
    }

    fn stage_seed_source(&self) -> CalibrationStageSeedSource {
        self.stage_seed_source
    }

    fn stage_seed_derivation_policy(&self) -> &'static str {
        self.stage_seed_derivation_policy
    }

    fn solver_settings(&self) -> &CalibrationOptimizerSettings {
        &self.solver_settings
    }
}

impl CalibrationStageExecutionMetadataForCheckpoint for ReturnComponentsStageExecutionMetadata {
    fn stage_output_schema_version(&self) -> &'static str {
        self.stage_output_schema_version
    }

    fn run_global_seed(&self) -> u64 {
        self.run_global_seed
    }

    fn stage_seed(&self) -> u64 {
        self.stage_seed
    }

    fn stage_seed_source(&self) -> CalibrationStageSeedSource {
        self.stage_seed_source
    }

    fn stage_seed_derivation_policy(&self) -> &'static str {
        self.stage_seed_derivation_policy
    }

    fn solver_settings(&self) -> &CalibrationOptimizerSettings {
        &self.solver_settings
    }
}

fn checkpoint_components_from_metadata<M: CalibrationStageExecutionMetadataForCheckpoint>(
    metadata: Option<&M>,
) -> Option<CalibrationStageCheckpointComponents> {
    metadata.map(|metadata| CalibrationStageCheckpointComponents {
        stage_output_schema_version: metadata.stage_output_schema_version().to_string(),
        run_global_seed: metadata.run_global_seed(),
        stage_seed: metadata.stage_seed(),
        stage_seed_source: metadata.stage_seed_source(),
        stage_seed_derivation_policy: metadata.stage_seed_derivation_policy(),
        stage_solver_settings: metadata.solver_settings().clone(),
    })
}

impl CalibrationStagedRunStageReport {
    const fn stage(&self) -> CalibrationStage {
        match self {
            Self::Seasonality(_) => CalibrationStage::Seasonality,
            Self::Hawkes(_) => CalibrationStage::Hawkes,
            Self::ReturnComponents(_) => CalibrationStage::ReturnComponents,
        }
    }

    const fn status(&self) -> CalibrationStageExecutionStatus {
        match self {
            Self::Seasonality(report) => report.status,
            Self::Hawkes(report) => report.status,
            Self::ReturnComponents(report) => report.status,
        }
    }

    fn checkpoint_components(&self) -> Option<CalibrationStageCheckpointComponents> {
        match self {
            Self::Seasonality(report) => {
                checkpoint_components_from_metadata(report.metadata.as_ref())
            }
            Self::Hawkes(report) => checkpoint_components_from_metadata(report.metadata.as_ref()),
            Self::ReturnComponents(report) => {
                checkpoint_components_from_metadata(report.metadata.as_ref())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum CalibrationStagedRunFailure {
    InvalidRunContract {
        error_message: String,
    },
    MissingMemoryCheckpointStore,
    MissingDiskCheckpointDirectory,
    MissingResumeCheckpointForRun {
        run_id: String,
    },
    DatasetSnapshotPersistFailed {
        artifact_reference: String,
        error_message: String,
    },
    StageParameterArtifactIoFailed {
        stage: CalibrationStage,
        artifact_reference: String,
        error_message: String,
    },
    ArtifactManifestPersistFailed {
        error_message: String,
    },
    MissingStageInput {
        stage: CalibrationStage,
    },
    MissingInMemoryUpstreamPayload {
        required_stage: CalibrationStage,
        consuming_stage: CalibrationStage,
    },
    InvalidCheckpointReference {
        checkpoint_reference: String,
        error_message: String,
    },
    CheckpointLoadFailed {
        checkpoint_reference: String,
        error_message: String,
    },
    CheckpointCompatibilityFailed {
        checkpoint_reference: String,
        error_message: String,
    },
    CheckpointPersistFailed {
        checkpoint_reference: String,
        error_message: String,
    },
    MissingCheckpointMetadataForSuccessfulStage {
        stage: CalibrationStage,
    },
    StageExecutionFailed {
        stage: CalibrationStage,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationStagedRunReport {
    pub schema_version: &'static str,
    pub failure_triage_policy: &'static str,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub status: CalibrationStageExecutionStatus,
    pub stage_reports: Vec<CalibrationStagedRunStageReport>,
    pub emitted_checkpoints: Vec<CalibrationStageCheckpointMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed_from_checkpoint_reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed_from_last_successful_stage: Option<CalibrationStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_from_stage: Option<CalibrationStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<CalibrationStagedRunFailure>,
}

pub fn calibration_checkpoint_reference_for_stage(run_id: &str, stage: CalibrationStage) -> String {
    format!("checkpoint://{run_id}/{}", calibration_stage_token(stage))
}

pub fn calibration_last_success_checkpoint_reference(run_id: &str) -> String {
    format!("checkpoint://{run_id}/last-success")
}

/// Execute staged calibration with deterministic checkpoint emission after each successful stage.
///
/// Resume behavior is mode-aware by contract:
/// - `execution_mode=memory` supports resume only through `CalibrationMemoryCheckpointStore`
/// - `execution_mode=disk` supports cross-process resume from persisted checkpoint references
pub fn run_staged_calibration_with_checkpointing(
    run_contract: &CalibrationRunContract,
    calendar_registry: &ExchangeCalendarRegistry,
    mut staged_run_inputs: CalibrationStagedRunInputs,
    mut memory_checkpoint_store: Option<&mut CalibrationMemoryCheckpointStore>,
    disk_checkpoint_directory: Option<&Path>,
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
    let mut persisted_stage_parameter_artifacts = Vec::<CalibrationStageParameterArtifact>::new();
    let mut payload_state = CalibrationInMemoryStagePayloadState::default();
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
        let artifact_run_writer = artifact_run_writer
            .as_ref()
            .expect("disk-mode execution should have prepared an artifact run writer");

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
                        CalibrationStagedRunFailure::StageParameterArtifactIoFailed {
                            stage,
                            artifact_reference,
                            error_message: source.to_string(),
                        },
                    );
                }
            };

            persisted_stage_parameter_artifacts.push(artifact);
        }
    }

    if start_position >= execution_stage_order.len() {
        return CalibrationStagedRunReport {
            schema_version: CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
            failure_triage_policy: CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
            run_id: run_spec.run_id.clone(),
            execution_mode: run_spec.execution_mode,
            status: CalibrationStageExecutionStatus::Success,
            stage_reports,
            emitted_checkpoints,
            resumed_from_checkpoint_reference,
            resumed_from_last_successful_stage,
            started_from_stage: None,
            failure: None,
        };
    }

    let started_from_stage = execution_stage_order.get(start_position).copied();
    for (stage_position, stage) in execution_stage_order
        .iter()
        .copied()
        .enumerate()
        .skip(start_position)
    {
        let stage_report = match run_stage_for_staged_execution(
            run_spec,
            calendar_registry,
            stage,
            &mut staged_run_inputs,
            &payload_state,
        ) {
            Ok(stage_report) => stage_report,
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

        let stage_status = stage_report.status();
        stage_reports.push(stage_report.clone());

        if stage_status == CalibrationStageExecutionStatus::Failure {
            return build_staged_run_failure_report(
                run_contract,
                stage_reports,
                emitted_checkpoints,
                resumed_from_checkpoint_reference,
                resumed_from_last_successful_stage,
                started_from_stage,
                CalibrationStagedRunFailure::StageExecutionFailed { stage },
            );
        }

        update_in_memory_payload_state(&mut payload_state, &stage_report);

        if run_spec.execution_mode == CalibrationExecutionMode::Disk {
            let artifact_run_writer = artifact_run_writer
                .as_ref()
                .expect("disk-mode execution should have prepared an artifact run writer");

            let stage_parameter_artifact =
                match build_calibration_stage_parameter_artifact(run_contract, &stage_report) {
                    Ok(artifact) => artifact,
                    Err(source) => {
                        return build_staged_run_failure_report(
                            run_contract,
                            stage_reports,
                            emitted_checkpoints,
                            resumed_from_checkpoint_reference,
                            resumed_from_last_successful_stage,
                            started_from_stage,
                            CalibrationStagedRunFailure::StageParameterArtifactIoFailed {
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
                    CalibrationStagedRunFailure::StageParameterArtifactIoFailed {
                        stage,
                        artifact_reference: stage_parameter_artifact.artifact_reference.clone(),
                        error_message: source.to_string(),
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

    if run_spec.execution_mode == CalibrationExecutionMode::Disk {
        let artifact_run_writer = artifact_run_writer
            .expect("disk-mode execution should have prepared an artifact run writer");
        let dataset_snapshot_artifact = dataset_snapshot_artifact
            .as_ref()
            .expect("disk-mode execution should have built a dataset snapshot artifact");

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

        let manifest = match build_calibration_artifact_manifest(
            run_contract,
            dataset_snapshot_artifact,
            persisted_stage_parameter_artifacts.as_slice(),
            emitted_checkpoints.as_slice(),
            &[],
            &[],
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

    CalibrationStagedRunReport {
        schema_version: CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
        failure_triage_policy: CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        status: CalibrationStageExecutionStatus::Success,
        stage_reports,
        emitted_checkpoints,
        resumed_from_checkpoint_reference,
        resumed_from_last_successful_stage,
        started_from_stage,
        failure: None,
    }
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

fn run_stage_for_staged_execution(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage: CalibrationStage,
    staged_run_inputs: &mut CalibrationStagedRunInputs,
    payload_state: &CalibrationInMemoryStagePayloadState,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    match stage {
        CalibrationStage::Seasonality => {
            let Some(seasonality_stage_input) = staged_run_inputs.seasonality_stage_input.take()
            else {
                return Err(CalibrationStagedRunFailure::MissingStageInput {
                    stage: CalibrationStage::Seasonality,
                });
            };

            Ok(CalibrationStagedRunStageReport::Seasonality(
                run_seasonality_calibration_stage(
                    run_spec,
                    calendar_registry,
                    seasonality_stage_input,
                ),
            ))
        }
        CalibrationStage::Hawkes => {
            let Some(candidate_inputs) = staged_run_inputs.hawkes_candidate_inputs.take() else {
                return Err(CalibrationStagedRunFailure::MissingStageInput {
                    stage: CalibrationStage::Hawkes,
                });
            };
            let seasonality_input_transport = expected_stage_handoff_transport(
                run_spec,
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
            );
            let seasonality_input = match seasonality_input_transport {
                CalibrationStageHandoffTransport::InMemoryOutput => {
                    let Some(seasonality_stage_payload) =
                        payload_state.seasonality_stage_payload.clone()
                    else {
                        return Err(
                            CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                                required_stage: CalibrationStage::Seasonality,
                                consuming_stage: CalibrationStage::Hawkes,
                            },
                        );
                    };

                    HawkesStageSeasonalityInput::InMemoryOutput {
                        payload: seasonality_stage_payload,
                    }
                }
                CalibrationStageHandoffTransport::PersistedArtifactReference => {
                    let Some(seasonality_stage_payload) =
                        payload_state.seasonality_stage_payload.clone()
                    else {
                        return Err(
                            CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                                required_stage: CalibrationStage::Seasonality,
                                consuming_stage: CalibrationStage::Hawkes,
                            },
                        );
                    };

                    HawkesStageSeasonalityInput::PersistedArtifactReference {
                        artifact_reference: expected_stage_handoff_artifact_reference(
                            run_spec,
                            CalibrationStage::Seasonality,
                            CalibrationStage::Hawkes,
                        ),
                        payload: seasonality_stage_payload,
                    }
                }
            };

            Ok(CalibrationStagedRunStageReport::Hawkes(
                run_hawkes_calibration_stage(
                    run_spec,
                    HawkesStageExecutionInput {
                        seasonality_input,
                        candidate_inputs,
                    },
                ),
            ))
        }
        CalibrationStage::ReturnComponents => {
            let Some(candidate_generation_input) = staged_run_inputs
                .return_components_candidate_generation_input
                .take()
            else {
                return Err(CalibrationStagedRunFailure::MissingStageInput {
                    stage: CalibrationStage::ReturnComponents,
                });
            };

            let activity_input_transport = expected_stage_handoff_transport(
                run_spec,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            );

            let activity_input = match activity_input_transport {
                CalibrationStageHandoffTransport::InMemoryOutput => {
                    let Some(hawkes_stage_payload) = payload_state.hawkes_stage_payload.clone()
                    else {
                        return Err(
                            CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                                required_stage: CalibrationStage::Hawkes,
                                consuming_stage: CalibrationStage::ReturnComponents,
                            },
                        );
                    };

                    ReturnComponentsStageActivityInput::InMemoryOutput {
                        payload: hawkes_stage_payload,
                    }
                }
                CalibrationStageHandoffTransport::PersistedArtifactReference => {
                    ReturnComponentsStageActivityInput::PersistedArtifactReference {
                        artifact_reference: expected_stage_handoff_artifact_reference(
                            run_spec,
                            CalibrationStage::Hawkes,
                            CalibrationStage::ReturnComponents,
                        ),
                    }
                }
            };

            Ok(CalibrationStagedRunStageReport::ReturnComponents(
                run_return_components_calibration_stage(
                    run_spec,
                    calendar_registry,
                    ReturnComponentsStageExecutionInput {
                        activity_input,
                        candidate_generation_input,
                    },
                ),
            ))
        }
    }
}

fn expected_stage_handoff_transport(
    run_spec: &CalibrationRunSpec,
    from_stage: CalibrationStage,
    to_stage: CalibrationStage,
) -> CalibrationStageHandoffTransport {
    run_spec
        .stage_handoffs
        .iter()
        .find(|handoff| handoff.from_stage == from_stage && handoff.to_stage == to_stage)
        .map(|handoff| handoff.transport())
        .unwrap_or(match run_spec.execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoffTransport::InMemoryOutput,
            CalibrationExecutionMode::Disk => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        })
}

fn expected_stage_handoff_artifact_reference(
    run_spec: &CalibrationRunSpec,
    from_stage: CalibrationStage,
    to_stage: CalibrationStage,
) -> String {
    if let Some(artifact_reference) = run_spec
        .stage_handoffs
        .iter()
        .find(|handoff| handoff.from_stage == from_stage && handoff.to_stage == to_stage)
        .and_then(|handoff| handoff.artifact_reference())
        && !artifact_reference.trim().is_empty()
    {
        return artifact_reference.to_string();
    }

    format!(
        "artifact://{}/{}",
        calibration_stage_token(from_stage),
        run_spec.run_id
    )
}

fn expected_stage_output_artifact_reference(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> String {
    let artifact_references = stage_output_artifact_references_for_stage(run_spec, stage);

    match artifact_references.as_slice() {
        [artifact_reference] => artifact_reference.clone(),
        _ => format!(
            "artifact://{}/{}",
            calibration_stage_token(stage),
            run_spec.run_id
        ),
    }
}

fn update_in_memory_payload_state(
    payload_state: &mut CalibrationInMemoryStagePayloadState,
    stage_report: &CalibrationStagedRunStageReport,
) {
    match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => {
            payload_state.seasonality_stage_payload = report.normalized_stage_payload.clone();
        }
        CalibrationStagedRunStageReport::Hawkes(report) => {
            payload_state.hawkes_stage_payload = report.fitted_stage_payload.clone();
        }
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            payload_state.return_components_stage_payload = report.fitted_stage_payload.clone();
        }
    }
}

fn build_stage_checkpoint_metadata(
    run_contract: &CalibrationRunContract,
    stage_report: &CalibrationStagedRunStageReport,
    stage_position: usize,
) -> Result<CalibrationStageCheckpointMetadata, CalibrationStagedRunFailure> {
    let stage = stage_report.stage();
    let Some(checkpoint_components) = stage_report.checkpoint_components() else {
        return Err(
            CalibrationStagedRunFailure::MissingCheckpointMetadataForSuccessfulStage { stage },
        );
    };

    Ok(CalibrationStageCheckpointMetadata {
        schema_version: CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION.to_string(),
        checkpoint_reference_policy: CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY.to_string(),
        disk_write_policy: CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY.to_string(),
        artifact_manifest_schema_version: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
        checkpoint_reference: calibration_checkpoint_reference_for_stage(
            run_contract.run_spec.run_id.as_str(),
            stage,
        ),
        run_id: run_contract.run_spec.run_id.clone(),
        execution_mode: run_contract.run_spec.execution_mode,
        run_contract_schema_version: run_contract.schema_version.clone(),
        run_spec_schema_version: run_contract.run_spec_schema_version.clone(),
        stage_graph_schema_version: run_contract.stage_graph_schema_version.clone(),
        execution_stage_order: run_contract.execution_stage_order.clone(),
        completed_stage: stage,
        completed_stage_position: stage_position,
        run_global_seed: checkpoint_components.run_global_seed,
        completed_stage_output_schema_version: checkpoint_components.stage_output_schema_version,
        completed_stage_seed: checkpoint_components.stage_seed,
        completed_stage_seed_source: checkpoint_components.stage_seed_source,
        stage_seed_derivation_policy: checkpoint_components
            .stage_seed_derivation_policy
            .to_string(),
        completed_stage_solver_settings: checkpoint_components.stage_solver_settings,
    })
}

fn build_staged_run_failure_report(
    run_contract: &CalibrationRunContract,
    stage_reports: Vec<CalibrationStagedRunStageReport>,
    emitted_checkpoints: Vec<CalibrationStageCheckpointMetadata>,
    resumed_from_checkpoint_reference: Option<String>,
    resumed_from_last_successful_stage: Option<CalibrationStage>,
    started_from_stage: Option<CalibrationStage>,
    failure: CalibrationStagedRunFailure,
) -> CalibrationStagedRunReport {
    CalibrationStagedRunReport {
        schema_version: CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
        failure_triage_policy: CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
        run_id: run_contract.run_spec.run_id.clone(),
        execution_mode: run_contract.run_spec.execution_mode,
        status: CalibrationStageExecutionStatus::Failure,
        stage_reports,
        emitted_checkpoints,
        resumed_from_checkpoint_reference,
        resumed_from_last_successful_stage,
        started_from_stage,
        failure: Some(failure),
    }
}
