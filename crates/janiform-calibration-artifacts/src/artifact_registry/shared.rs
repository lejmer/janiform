use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use janiform_foundations::hashing::fnv1a_64_hex_lowercase;
use janiform_foundations::paths::format_path;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::super::contracts::CalibrationExecutionMode;

pub const CALIBRATION_ARTIFACT_STORAGE_LAYOUT_POLICY: &str = "run_directory_separates_committed_calibration_artifacts_pending_artifacts_quarantine_and_checkpoints.v1";
pub const CALIBRATION_ARTIFACT_ROOT_CONFIGURATION_POLICY: &str =
    "caller_supplied_root_directory_supports_local_and_ci_filesystem_contexts.v1";

pub fn calibration_committed_artifact_runs_root_directory(
    persistence_root_directory: &Path,
) -> PathBuf {
    persistence_root_directory.join("artifacts")
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
    _run_id: &str,
) -> PathBuf {
    calibration_committed_artifact_runs_root_directory(persistence_root_directory)
        .join("calibration")
}

pub(crate) fn calibration_pending_artifact_run_directory(
    persistence_root_directory: &Path,
    _run_id: &str,
) -> PathBuf {
    calibration_pending_artifact_runs_root_directory(persistence_root_directory).join("calibration")
}

pub fn calibration_checkpoint_run_directory(
    persistence_root_directory: &Path,
    _run_id: &str,
) -> PathBuf {
    persistence_root_directory.join("checkpoints")
}

pub(crate) fn calibration_execution_mode_token(
    execution_mode: CalibrationExecutionMode,
) -> &'static str {
    match execution_mode {
        CalibrationExecutionMode::Memory => "memory",
        CalibrationExecutionMode::Disk => "disk",
    }
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
    fnv1a_64_hex_lowercase(data_reference.as_bytes())
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

    if target_path.exists() {
        fs::remove_file(target_path)
            .map_err(|source| write_failed(target_path, source.to_string()))?;
    }

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

    let Some(run_directory) = artifact_path.parent() else {
        return Err(CalibrationArtifactIoError::validation_failed(
            artifact_kind,
            format!(
                "artifact path `{}` must include a parent directory",
                format_path(artifact_path)
            ),
        ));
    };

    fs::create_dir_all(run_directory).map_err(|source| {
        CalibrationArtifactIoError::create_directory_failed(
            artifact_kind,
            format_path(run_directory),
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
                format_path(path),
                error_message,
            )
        },
        |source_path, target_path, error_message| {
            CalibrationArtifactIoError::rename_failed(
                artifact_kind,
                format_path(source_path),
                format_path(target_path),
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
    let payload = load_json_artifact(artifact_kind, artifact_path)?;

    validate(&payload).map_err(|error_message| {
        CalibrationArtifactIoError::validation_failed(artifact_kind, error_message)
    })?;

    Ok(payload)
}

pub(crate) fn load_json_artifact<T>(
    artifact_kind: CalibrationArtifactPersistenceKind,
    artifact_path: &Path,
) -> Result<T, CalibrationArtifactIoError>
where
    T: DeserializeOwned,
{
    let content = fs::read_to_string(artifact_path).map_err(|source| {
        CalibrationArtifactIoError::read_failed(
            artifact_kind,
            format_path(artifact_path),
            source.to_string(),
        )
    })?;

    serde_json::from_str(content.as_str()).map_err(|source| {
        CalibrationArtifactIoError::deserialization_failed(
            artifact_kind,
            format_path(artifact_path),
            source.to_string(),
        )
    })
}
