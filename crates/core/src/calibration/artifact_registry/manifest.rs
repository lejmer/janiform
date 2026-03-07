use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use super::super::checkpointing::CalibrationStageCheckpointMetadata;
use super::super::contracts::{
    CalibrationArtifactKind, CalibrationArtifactManifest, CalibrationArtifactReference,
    CalibrationExecutionMode, CalibrationRunContract, CalibrationStage,
};
use super::dataset_snapshot::calibration_dataset_snapshot_artifact_reference;
use super::shared::{
    CalibrationArtifactIoError, CalibrationArtifactPersistenceKind, calibration_stage_token,
    load_validated_json_artifact, persist_validated_json_artifact,
};
use super::stage_parameter::CalibrationStageParameterArtifact;

pub fn calibration_artifact_manifest_path(
    disk_checkpoint_directory: &Path,
    run_id: &str,
) -> PathBuf {
    disk_checkpoint_directory
        .join(run_id)
        .join("artifact_manifest.json")
}

pub fn calibration_artifact_manifest_reference(run_id: &str) -> String {
    format!("artifact://manifest/{run_id}")
}

pub fn build_calibration_artifact_manifest(
    run_contract: &CalibrationRunContract,
    stage_parameter_artifacts: &[CalibrationStageParameterArtifact],
    emitted_checkpoints: &[CalibrationStageCheckpointMetadata],
    additional_artifact_references: &[CalibrationArtifactReference],
) -> Result<CalibrationArtifactManifest, CalibrationArtifactManifestBuildError> {
    if run_contract.run_spec.execution_mode != CalibrationExecutionMode::Disk {
        return Err(
            CalibrationArtifactManifestBuildError::UnsupportedExecutionMode {
                expected: CalibrationExecutionMode::Disk,
                found: run_contract.run_spec.execution_mode,
            },
        );
    }

    let mut artifact_references = vec![CalibrationArtifactReference {
        artifact_id: "dataset_snapshot".to_string(),
        artifact_kind: CalibrationArtifactKind::DatasetSnapshot,
        artifact_reference: calibration_dataset_snapshot_artifact_reference(
            run_contract.run_spec.run_id.as_str(),
        ),
        stage: None,
    }];

    let mut stage_artifacts_by_stage = BTreeMap::new();
    for artifact in stage_parameter_artifacts {
        artifact.validate().map_err(|source| {
            CalibrationArtifactManifestBuildError::InvalidStageOutputArtifact {
                stage: artifact.stage,
                error_message: source.to_string(),
            }
        })?;

        if artifact.run_id != run_contract.run_spec.run_id {
            return Err(
                CalibrationArtifactManifestBuildError::StageOutputRunIdMismatch {
                    stage: artifact.stage,
                    expected: run_contract.run_spec.run_id.clone(),
                    found: artifact.run_id.clone(),
                },
            );
        }

        if stage_artifacts_by_stage
            .insert(artifact.stage, artifact)
            .is_some()
        {
            return Err(
                CalibrationArtifactManifestBuildError::DuplicateStageOutputArtifact {
                    stage: artifact.stage,
                },
            );
        }
    }

    for stage in &run_contract.execution_stage_order {
        let Some(artifact) = stage_artifacts_by_stage.get(stage) else {
            return Err(
                CalibrationArtifactManifestBuildError::MissingStageOutputArtifact { stage: *stage },
            );
        };
        artifact_references.push(CalibrationArtifactReference {
            artifact_id: format!("{}_stage_output", calibration_stage_token(*stage)),
            artifact_kind: CalibrationArtifactKind::StageOutput,
            artifact_reference: artifact.artifact_reference.clone(),
            stage: Some(*stage),
        });
    }

    let mut seen_checkpoint_stages = BTreeSet::new();
    for checkpoint in emitted_checkpoints {
        if checkpoint.run_id != run_contract.run_spec.run_id {
            return Err(
                CalibrationArtifactManifestBuildError::CheckpointRunIdMismatch {
                    stage: checkpoint.completed_stage,
                    expected: run_contract.run_spec.run_id.clone(),
                    found: checkpoint.run_id.clone(),
                },
            );
        }

        if !seen_checkpoint_stages.insert(checkpoint.completed_stage) {
            return Err(
                CalibrationArtifactManifestBuildError::DuplicateCheckpointArtifact {
                    stage: checkpoint.completed_stage,
                },
            );
        }

        artifact_references.push(CalibrationArtifactReference {
            artifact_id: format!(
                "{}_checkpoint",
                calibration_stage_token(checkpoint.completed_stage)
            ),
            artifact_kind: CalibrationArtifactKind::StageCheckpoint,
            artifact_reference: checkpoint.checkpoint_reference.clone(),
            stage: Some(checkpoint.completed_stage),
        });
    }

    artifact_references.extend_from_slice(additional_artifact_references);

    CalibrationArtifactManifest::try_new(run_contract.run_spec.run_id.clone(), artifact_references)
        .map_err(
            |source| CalibrationArtifactManifestBuildError::ManifestValidationFailed {
                error_message: source.to_string(),
            },
        )
}

pub fn persist_calibration_artifact_manifest_to_disk(
    disk_checkpoint_directory: &Path,
    manifest: &CalibrationArtifactManifest,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::Manifest;
    let manifest_path =
        calibration_artifact_manifest_path(disk_checkpoint_directory, manifest.run_id.as_str());
    persist_validated_json_artifact(
        artifact_kind,
        manifest_path.as_path(),
        manifest,
        |payload| payload.validate().map_err(|source| source.to_string()),
    )
}

pub fn load_calibration_artifact_manifest_from_disk(
    manifest_path: &Path,
) -> Result<CalibrationArtifactManifest, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::Manifest;
    load_validated_json_artifact(
        artifact_kind,
        manifest_path,
        |payload: &CalibrationArtifactManifest| {
            payload.validate().map_err(|source| source.to_string())
        },
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactManifestBuildError {
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    MissingStageOutputArtifact {
        stage: CalibrationStage,
    },
    DuplicateStageOutputArtifact {
        stage: CalibrationStage,
    },
    InvalidStageOutputArtifact {
        stage: CalibrationStage,
        error_message: String,
    },
    StageOutputRunIdMismatch {
        stage: CalibrationStage,
        expected: String,
        found: String,
    },
    DuplicateCheckpointArtifact {
        stage: CalibrationStage,
    },
    CheckpointRunIdMismatch {
        stage: CalibrationStage,
        expected: String,
        found: String,
    },
    ManifestValidationFailed {
        error_message: String,
    },
}

impl Display for CalibrationArtifactManifestBuildError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedExecutionMode { expected, found } => write!(
                formatter,
                "artifact manifest requires execution_mode={expected:?}, found `{found:?}`"
            ),
            Self::MissingStageOutputArtifact { stage } => write!(
                formatter,
                "artifact manifest is missing stage output artifact for `{stage:?}`"
            ),
            Self::DuplicateStageOutputArtifact { stage } => write!(
                formatter,
                "artifact manifest received duplicate stage output artifact for `{stage:?}`"
            ),
            Self::InvalidStageOutputArtifact {
                stage,
                error_message,
            } => write!(
                formatter,
                "artifact manifest received invalid stage output artifact for `{stage:?}`: {error_message}"
            ),
            Self::StageOutputRunIdMismatch {
                stage,
                expected,
                found,
            } => write!(
                formatter,
                "stage output artifact for `{stage:?}` must use run_id `{expected}`, found `{found}`"
            ),
            Self::DuplicateCheckpointArtifact { stage } => write!(
                formatter,
                "artifact manifest received duplicate checkpoint for `{stage:?}`"
            ),
            Self::CheckpointRunIdMismatch {
                stage,
                expected,
                found,
            } => write!(
                formatter,
                "checkpoint artifact for `{stage:?}` must use run_id `{expected}`, found `{found}`"
            ),
            Self::ManifestValidationFailed { error_message } => {
                write!(
                    formatter,
                    "artifact manifest validation failed: {error_message}"
                )
            }
        }
    }
}

impl Error for CalibrationArtifactManifestBuildError {}
