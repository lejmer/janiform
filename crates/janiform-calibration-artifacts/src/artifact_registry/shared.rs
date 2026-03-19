use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::super::contracts::{CalibrationExecutionMode, CalibrationRunSpec, CalibrationStage};

pub const CALIBRATION_ARTIFACT_STORAGE_LAYOUT_POLICY: &str =
    "root_directory_separates_committed_artifacts_pending_artifacts_quarantine_and_checkpoints.v1";
pub const CALIBRATION_ARTIFACT_ROOT_CONFIGURATION_POLICY: &str =
    "caller_supplied_root_directory_supports_local_and_ci_filesystem_contexts.v1";

pub fn calibration_committed_artifact_runs_root_directory(
    persistence_root_directory: &Path,
) -> PathBuf {
    persistence_root_directory.join("artifacts").join("runs")
}

pub fn calibration_experiment_index_root_directory(persistence_root_directory: &Path) -> PathBuf {
    persistence_root_directory.join("artifacts").join("index")
}

pub fn calibration_experiment_index_path(persistence_root_directory: &Path) -> PathBuf {
    calibration_experiment_index_root_directory(persistence_root_directory)
        .join("calibration_experiment_index.json")
}

pub(crate) fn calibration_pending_artifact_runs_root_directory(
    persistence_root_directory: &Path,
) -> PathBuf {
    persistence_root_directory.join("artifacts").join("pending")
}

pub(crate) fn calibration_quarantine_artifact_runs_root_directory(
    persistence_root_directory: &Path,
) -> PathBuf {
    persistence_root_directory
        .join("artifacts")
        .join("quarantine")
}

pub fn calibration_committed_artifact_run_directory(
    persistence_root_directory: &Path,
    run_id: &str,
) -> PathBuf {
    calibration_committed_artifact_runs_root_directory(persistence_root_directory).join(run_id)
}

pub(crate) fn calibration_pending_artifact_run_directory(
    persistence_root_directory: &Path,
    run_id: &str,
) -> PathBuf {
    calibration_pending_artifact_runs_root_directory(persistence_root_directory).join(run_id)
}

pub fn calibration_checkpoint_run_directory(
    persistence_root_directory: &Path,
    run_id: &str,
) -> PathBuf {
    persistence_root_directory.join("checkpoints").join(run_id)
}

pub(crate) fn calibration_execution_mode_token(
    execution_mode: CalibrationExecutionMode,
) -> &'static str {
    match execution_mode {
        CalibrationExecutionMode::Memory => "memory",
        CalibrationExecutionMode::Disk => "disk",
    }
}

pub fn calibration_stage_token(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "seasonality",
        CalibrationStage::Hawkes => "hawkes",
        CalibrationStage::ReturnComponents => "return_components",
        CalibrationStage::JointRefinement => "joint_refinement",
    }
}

pub fn stage_output_artifact_references_for_stage(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> Vec<String> {
    let mut artifact_references = run_spec
        .stage_handoffs
        .iter()
        .filter(|handoff| handoff.from_stage == stage)
        .filter_map(|handoff| handoff.artifact_reference())
        .filter(|artifact_reference| !artifact_reference.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    artifact_references.sort();
    artifact_references.dedup();
    artifact_references
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationArtifactPersistenceKind {
    Manifest,
    DatasetSnapshot,
    StageParameter,
    ValidationMetric,
    ExperimentIndex,
}

impl CalibrationArtifactPersistenceKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Manifest => "artifact manifest",
            Self::DatasetSnapshot => "dataset snapshot artifact",
            Self::StageParameter => "stage parameter artifact",
            Self::ValidationMetric => "validation metric artifact",
            Self::ExperimentIndex => "experiment index artifact",
        }
    }

    const fn directory_label(self) -> &'static str {
        match self {
            Self::Manifest => "artifact manifest directory",
            Self::DatasetSnapshot => "dataset snapshot artifact directory",
            Self::StageParameter => "stage parameter artifact directory",
            Self::ValidationMetric => "validation metric artifact directory",
            Self::ExperimentIndex => "experiment index artifact directory",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactIoError {
    CreateDirectoryFailed {
        artifact_kind: CalibrationArtifactPersistenceKind,
        path: String,
        error_message: String,
    },
    SerializationFailed {
        artifact_kind: CalibrationArtifactPersistenceKind,
        error_message: String,
    },
    WriteFailed {
        artifact_kind: CalibrationArtifactPersistenceKind,
        path: String,
        error_message: String,
    },
    RenameFailed {
        artifact_kind: CalibrationArtifactPersistenceKind,
        source_path: String,
        target_path: String,
        error_message: String,
    },
    ReadFailed {
        artifact_kind: CalibrationArtifactPersistenceKind,
        path: String,
        error_message: String,
    },
    DeserializationFailed {
        artifact_kind: CalibrationArtifactPersistenceKind,
        path: String,
        error_message: String,
    },
    ValidationFailed {
        artifact_kind: CalibrationArtifactPersistenceKind,
        error_message: String,
    },
}

impl CalibrationArtifactIoError {
    pub(crate) fn validation_failed(
        artifact_kind: CalibrationArtifactPersistenceKind,
        error_message: String,
    ) -> Self {
        Self::ValidationFailed {
            artifact_kind,
            error_message,
        }
    }

    pub(crate) fn create_directory_failed(
        artifact_kind: CalibrationArtifactPersistenceKind,
        path: String,
        error_message: String,
    ) -> Self {
        Self::CreateDirectoryFailed {
            artifact_kind,
            path,
            error_message,
        }
    }

    pub(crate) fn serialization_failed(
        artifact_kind: CalibrationArtifactPersistenceKind,
        error_message: String,
    ) -> Self {
        Self::SerializationFailed {
            artifact_kind,
            error_message,
        }
    }

    pub(crate) fn write_failed(
        artifact_kind: CalibrationArtifactPersistenceKind,
        path: String,
        error_message: String,
    ) -> Self {
        Self::WriteFailed {
            artifact_kind,
            path,
            error_message,
        }
    }

    pub(crate) fn rename_failed(
        artifact_kind: CalibrationArtifactPersistenceKind,
        source_path: String,
        target_path: String,
        error_message: String,
    ) -> Self {
        Self::RenameFailed {
            artifact_kind,
            source_path,
            target_path,
            error_message,
        }
    }

    pub(crate) fn read_failed(
        artifact_kind: CalibrationArtifactPersistenceKind,
        path: String,
        error_message: String,
    ) -> Self {
        Self::ReadFailed {
            artifact_kind,
            path,
            error_message,
        }
    }

    pub(crate) fn deserialization_failed(
        artifact_kind: CalibrationArtifactPersistenceKind,
        path: String,
        error_message: String,
    ) -> Self {
        Self::DeserializationFailed {
            artifact_kind,
            path,
            error_message,
        }
    }
}

impl Display for CalibrationArtifactIoError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirectoryFailed {
                artifact_kind,
                path,
                error_message,
            } => write!(
                formatter,
                "failed to create {} `{path}`: {error_message}",
                artifact_kind.directory_label()
            ),
            Self::SerializationFailed {
                artifact_kind,
                error_message,
            } => write!(
                formatter,
                "failed to serialize {}: {error_message}",
                artifact_kind.label()
            ),
            Self::WriteFailed {
                artifact_kind,
                path,
                error_message,
            } => write!(
                formatter,
                "failed to write {} `{path}`: {error_message}",
                artifact_kind.label()
            ),
            Self::RenameFailed {
                artifact_kind,
                source_path,
                target_path,
                error_message,
            } => write!(
                formatter,
                "failed to rename {} `{source_path}` -> `{target_path}`: {error_message}",
                artifact_kind.label()
            ),
            Self::ReadFailed {
                artifact_kind,
                path,
                error_message,
            } => write!(
                formatter,
                "failed to read {} `{path}`: {error_message}",
                artifact_kind.label()
            ),
            Self::DeserializationFailed {
                artifact_kind,
                path,
                error_message,
            } => write!(
                formatter,
                "failed to deserialize {} `{path}`: {error_message}",
                artifact_kind.label()
            ),
            Self::ValidationFailed {
                artifact_kind,
                error_message,
            } => write!(
                formatter,
                "{} validation failed: {error_message}",
                artifact_kind.label()
            ),
        }
    }
}

impl Error for CalibrationArtifactIoError {}

pub(crate) fn compute_reference_checksum_hex(data_reference: &str) -> String {
    format!("{:016x}", fnv1a_64(data_reference.as_bytes()))
}

fn fnv1a_64(payload: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = FNV_OFFSET;
    for byte in payload {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

pub(crate) fn validate_checksum_hex<E>(checksum_hex: &str, error: E) -> Result<(), E> {
    if checksum_hex.len() != 16
        || checksum_hex
            .chars()
            .any(|character| !matches!(character, '0'..='9' | 'a'..='f'))
    {
        return Err(error);
    }

    Ok(())
}

pub(crate) fn write_json_file_atomically<T, E, SerializationFailed, WriteFailed, RenameFailed>(
    target_path: &Path,
    payload: &T,
    serialization_failed: SerializationFailed,
    write_failed: WriteFailed,
    rename_failed: RenameFailed,
) -> Result<(), E>
where
    T: Serialize,
    SerializationFailed: FnOnce(String) -> E,
    WriteFailed: Fn(&Path, String) -> E,
    RenameFailed: Fn(&Path, &Path, String) -> E,
{
    let serialized = serde_json::to_vec_pretty(payload)
        .map_err(|source| serialization_failed(source.to_string()))?;

    let temporary_path = target_path.with_extension("json.tmp");
    let mut temporary_file = File::create(&temporary_path)
        .map_err(|source| write_failed(&temporary_path, source.to_string()))?;
    temporary_file
        .write_all(serialized.as_slice())
        .map_err(|source| write_failed(&temporary_path, source.to_string()))?;
    temporary_file
        .sync_all()
        .map_err(|source| write_failed(&temporary_path, source.to_string()))?;

    fs::rename(&temporary_path, target_path)
        .map_err(|source| rename_failed(&temporary_path, target_path, source.to_string()))?;

    Ok(())
}

pub(crate) fn persist_validated_json_artifact<T, Validate>(
    artifact_kind: CalibrationArtifactPersistenceKind,
    artifact_path: &Path,
    payload: &T,
    validate: Validate,
) -> Result<PathBuf, CalibrationArtifactIoError>
where
    T: Serialize,
    Validate: FnOnce(&T) -> Result<(), String>,
{
    validate(payload).map_err(|error_message| {
        CalibrationArtifactIoError::validation_failed(artifact_kind, error_message)
    })?;

    let run_directory = artifact_path
        .parent()
        .expect("artifact path should always have a parent directory");
    fs::create_dir_all(run_directory).map_err(|source| {
        CalibrationArtifactIoError::create_directory_failed(
            artifact_kind,
            run_directory.display().to_string(),
            source.to_string(),
        )
    })?;

    write_json_file_atomically(
        artifact_path,
        payload,
        |error_message| {
            CalibrationArtifactIoError::serialization_failed(artifact_kind, error_message)
        },
        |path, error_message| {
            CalibrationArtifactIoError::write_failed(
                artifact_kind,
                path.display().to_string(),
                error_message,
            )
        },
        |source_path, target_path, error_message| {
            CalibrationArtifactIoError::rename_failed(
                artifact_kind,
                source_path.display().to_string(),
                target_path.display().to_string(),
                error_message,
            )
        },
    )?;

    Ok(artifact_path.to_path_buf())
}

pub(crate) fn load_validated_json_artifact<T, Validate>(
    artifact_kind: CalibrationArtifactPersistenceKind,
    artifact_path: &Path,
    validate: Validate,
) -> Result<T, CalibrationArtifactIoError>
where
    T: DeserializeOwned,
    Validate: FnOnce(&T) -> Result<(), String>,
{
    let content = fs::read_to_string(artifact_path).map_err(|source| {
        CalibrationArtifactIoError::read_failed(
            artifact_kind,
            artifact_path.display().to_string(),
            source.to_string(),
        )
    })?;

    let payload: T = serde_json::from_str(content.as_str()).map_err(|source| {
        CalibrationArtifactIoError::deserialization_failed(
            artifact_kind,
            artifact_path.display().to_string(),
            source.to_string(),
        )
    })?;

    validate(&payload).map_err(|error_message| {
        CalibrationArtifactIoError::validation_failed(artifact_kind, error_message)
    })?;

    Ok(payload)
}
