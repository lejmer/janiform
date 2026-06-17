use std::path::{Path, PathBuf};

use super::super::super::super::contracts::CalibrationStage;
use super::super::super::shared::{
    CalibrationArtifactIoError, CalibrationArtifactPersistenceKind,
    calibration_committed_artifact_run_directory, load_json_artifact,
    persist_validated_json_artifact,
};
use super::super::super::stage_references::calibration_stage_token;
use super::artifact::CalibrationStageParameterArtifact;

pub fn calibration_stage_parameter_artifact_path(
    persistence_root_directory: &Path,
    run_id: &str,
    stage: CalibrationStage,
) -> PathBuf {
    let run_directory =
        calibration_committed_artifact_run_directory(persistence_root_directory, run_id);

    calibration_stage_parameter_artifact_path_in_run_directory(run_directory.as_path(), stage)
}

pub fn calibration_stage_parameter_artifact_path_in_run_directory(
    run_directory: &Path,
    stage: CalibrationStage,
) -> PathBuf {
    run_directory.join(format!(
        "{}.stage_parameter.artifact.json",
        calibration_stage_token(stage)
    ))
}

pub fn persist_calibration_stage_parameter_artifact_to_disk(
    persistence_root_directory: &Path,
    artifact: &CalibrationStageParameterArtifact,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_path = calibration_stage_parameter_artifact_path(
        persistence_root_directory,
        artifact.run_id.as_str(),
        artifact.stage,
    );

    persist_calibration_stage_parameter_artifact_to_path(artifact_path.as_path(), artifact)
}

pub fn persist_calibration_stage_parameter_artifact_to_run_directory(
    run_directory: &Path,
    artifact: &CalibrationStageParameterArtifact,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_path =
        calibration_stage_parameter_artifact_path_in_run_directory(run_directory, artifact.stage);

    persist_calibration_stage_parameter_artifact_to_path(artifact_path.as_path(), artifact)
}

fn persist_calibration_stage_parameter_artifact_to_path(
    artifact_path: &Path,
    artifact: &CalibrationStageParameterArtifact,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::StageParameter;

    artifact.validate().map_err(|source| {
        CalibrationArtifactIoError::validation_failed(artifact_kind, source.to_string())
    })?;

    persist_validated_json_artifact(artifact_kind, artifact_path, artifact, |payload| {
        payload.validate().map_err(|source| source.to_string())
    })
}

pub fn load_calibration_stage_parameter_artifact_from_disk(
    artifact_path: &Path,
) -> Result<CalibrationStageParameterArtifact, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::StageParameter;

    let artifact: CalibrationStageParameterArtifact =
        load_json_artifact(artifact_kind, artifact_path)?;

    artifact.validate().map_err(|source| {
        CalibrationArtifactIoError::validation_failed(artifact_kind, source.to_string())
    })?;

    Ok(artifact)
}

/// Build a user-facing load error for a stage parameter artifact.
pub fn calibration_stage_parameter_artifact_load_error_message(
    stage: CalibrationStage,
    source: &CalibrationArtifactIoError,
) -> String {
    match source {
        CalibrationArtifactIoError::DeserializationFailed { error_message, .. } => format!(
            "persisted `{}` stage parameter artifact cannot be read by the current calibration artifact contract: {error_message}. Recompute this stage before resuming from a later stage.",
            calibration_stage_token(stage)
        ),
        _ => source.to_string(),
    }
}
