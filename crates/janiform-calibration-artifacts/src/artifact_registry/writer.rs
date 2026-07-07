use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use janiform_foundations::paths::format_path;

use super::experiment_index::update_calibration_experiment_index_for_run;
use super::manifest::calibration_artifact_manifest_path_in_run_directory;
use super::shared::{
    calibration_committed_artifact_run_directory,
    calibration_committed_artifact_runs_root_directory, calibration_pending_artifact_run_directory,
    calibration_quarantine_artifact_runs_root_directory,
};

pub const CALIBRATION_ARTIFACT_ATOMIC_COMMIT_POLICY: &str =
    "stage_all_run_artifacts_under_pending_then_publish_with_single_directory_rename.v1";
pub const CALIBRATION_ARTIFACT_PARTIAL_WRITE_RECOVERY_POLICY: &str =
    "fresh_runs_quarantine_stale_pending_or_incomplete_run_directories_before_restarting.v1";
pub const CALIBRATION_ARTIFACT_RESUME_REPLACEMENT_POLICY: &str =
    "resume_runs_seed_pending_artifacts_from_committed_run_and_replace_committed_on_success.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactRunPreparationMode {
    FreshRun,
    ResumeRun,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalibrationArtifactRunWriter {
    persistence_root_directory: PathBuf,
    run_id: String,
    pending_run_directory: PathBuf,
    committed_run_directory: PathBuf,
    replace_committed_on_commit: bool,
}

impl CalibrationArtifactRunWriter {
    pub fn prepare(
        persistence_root_directory: &Path,
        run_id: &str,
        mode: CalibrationArtifactRunPreparationMode,
    ) -> Result<Self, CalibrationArtifactRunWriterError> {
        let committed_runs_root_directory =
            calibration_committed_artifact_runs_root_directory(persistence_root_directory);

        let quarantine_runs_root_directory =
            calibration_quarantine_artifact_runs_root_directory(persistence_root_directory);

        fs::create_dir_all(&committed_runs_root_directory).map_err(|source| {
            CalibrationArtifactRunWriterError::CreateDirectoryFailed {
                path: format_path(&committed_runs_root_directory),
                error_message: source.to_string(),
            }
        })?;

        let committed_run_directory =
            calibration_committed_artifact_run_directory(persistence_root_directory, run_id);

        let pending_run_directory =
            calibration_pending_artifact_run_directory(persistence_root_directory, run_id);

        match mode {
            CalibrationArtifactRunPreparationMode::FreshRun => {
                if committed_run_directory.exists() {
                    let manifest_path = calibration_artifact_manifest_path_in_run_directory(
                        &committed_run_directory,
                    );
                    if manifest_path.exists() {
                        return Err(
                            CalibrationArtifactRunWriterError::CommittedRunAlreadyExists {
                                run_id: run_id.to_string(),
                                path: format_path(&committed_run_directory),
                            },
                        );
                    }

                    quarantine_run_directory(
                        &committed_run_directory,
                        &quarantine_runs_root_directory,
                        run_id,
                        "incomplete_committed",
                    )?;
                }

                if pending_run_directory.exists() {
                    quarantine_run_directory(
                        &pending_run_directory,
                        &quarantine_runs_root_directory,
                        run_id,
                        "stale_pending",
                    )?;
                }

                fs::create_dir_all(&pending_run_directory).map_err(|source| {
                    CalibrationArtifactRunWriterError::CreateDirectoryFailed {
                        path: format_path(&pending_run_directory),
                        error_message: source.to_string(),
                    }
                })?;
            }
            CalibrationArtifactRunPreparationMode::ResumeRun => {
                if committed_run_directory.exists() {
                    if pending_run_directory.exists() {
                        quarantine_run_directory(
                            &pending_run_directory,
                            &quarantine_runs_root_directory,
                            run_id,
                            "stale_pending_resume",
                        )?;
                    }

                    copy_run_directory(&committed_run_directory, &pending_run_directory)?;
                } else if pending_run_directory.exists() {
                    // Resume after an incomplete disk run continues from the existing
                    // pending artifact set because no committed artifact set exists yet.
                } else {
                    fs::create_dir_all(&pending_run_directory).map_err(|source| {
                        CalibrationArtifactRunWriterError::CreateDirectoryFailed {
                            path: format_path(&pending_run_directory),
                            error_message: source.to_string(),
                        }
                    })?;
                }
            }
        }

        Ok(Self {
            persistence_root_directory: persistence_root_directory.to_path_buf(),
            run_id: run_id.to_string(),
            pending_run_directory,
            committed_run_directory,
            replace_committed_on_commit: mode == CalibrationArtifactRunPreparationMode::ResumeRun,
        })
    }

    pub fn run_id(&self) -> &str {
        self.run_id.as_str()
    }

    pub fn pending_run_directory(&self) -> &Path {
        self.pending_run_directory.as_path()
    }

    pub fn committed_run_directory(&self) -> &Path {
        self.committed_run_directory.as_path()
    }

    pub fn commit(self) -> Result<PathBuf, CalibrationArtifactRunWriterError> {
        if !self.pending_run_directory.exists() {
            return Err(
                CalibrationArtifactRunWriterError::PendingRunDirectoryMissing {
                    run_id: self.run_id,
                    path: format_path(&self.pending_run_directory),
                },
            );
        }

        let replaced_committed_directory =
            if self.committed_run_directory.exists() && self.replace_committed_on_commit {
                let quarantine_root_directory = calibration_quarantine_artifact_runs_root_directory(
                    self.persistence_root_directory.as_path(),
                );

                Some(quarantine_run_directory(
                    &self.committed_run_directory,
                    &quarantine_root_directory,
                    self.run_id.as_str(),
                    "replaced_committed",
                )?)
            } else {
                None
            };

        if self.committed_run_directory.exists() {
            return Err(
                CalibrationArtifactRunWriterError::CommittedRunAlreadyExists {
                    run_id: self.run_id,
                    path: format_path(&self.committed_run_directory),
                },
            );
        }

        fs::rename(&self.pending_run_directory, &self.committed_run_directory).map_err(
            |source| {
                if let Some(replaced_directory) = replaced_committed_directory.as_ref() {
                    let _ = fs::rename(replaced_directory, &self.committed_run_directory);
                }

                CalibrationArtifactRunWriterError::CommitRenameFailed {
                    run_id: self.run_id.clone(),
                    source_path: format_path(&self.pending_run_directory),
                    target_path: format_path(&self.committed_run_directory),
                    error_message: source.to_string(),
                }
            },
        )?;

        update_calibration_experiment_index_for_run(
            self.persistence_root_directory.as_path(),
            self.run_id.as_str(),
        )
        .map_err(|source| {
            CalibrationArtifactRunWriterError::ExperimentIndexUpdateFailed {
                run_id: self.run_id.clone(),
                path: format_path(&self.committed_run_directory),
                error_message: source.to_string(),
            }
        })?;

        Ok(self.committed_run_directory)
    }
}

fn copy_run_directory(
    source_directory: &Path,
    target_directory: &Path,
) -> Result<(), CalibrationArtifactRunWriterError> {
    fs::create_dir_all(target_directory).map_err(|source| {
        CalibrationArtifactRunWriterError::CreateDirectoryFailed {
            path: format_path(target_directory),
            error_message: source.to_string(),
        }
    })?;

    for entry in fs::read_dir(source_directory).map_err(|source| {
        CalibrationArtifactRunWriterError::ReadDirectoryFailed {
            path: format_path(source_directory),
            error_message: source.to_string(),
        }
    })? {
        let entry =
            entry.map_err(
                |source| CalibrationArtifactRunWriterError::ReadDirectoryFailed {
                    path: format_path(source_directory),
                    error_message: source.to_string(),
                },
            )?;

        let source_path = entry.path();
        let target_path = target_directory.join(entry.file_name());

        let file_type = entry.file_type().map_err(|source| {
            CalibrationArtifactRunWriterError::ReadDirectoryFailed {
                path: format_path(&source_path),
                error_message: source.to_string(),
            }
        })?;

        if file_type.is_dir() {
            copy_run_directory(source_path.as_path(), target_path.as_path())?;
        } else if file_type.is_file() {
            fs::copy(source_path.as_path(), target_path.as_path()).map_err(|source| {
                CalibrationArtifactRunWriterError::CopyFileFailed {
                    source_path: format_path(&source_path),
                    target_path: format_path(&target_path),
                    error_message: source.to_string(),
                }
            })?;
        }
    }

    Ok(())
}

fn quarantine_run_directory(
    source_directory: &Path,
    quarantine_root_directory: &Path,
    run_id: &str,
    reason_token: &str,
) -> Result<PathBuf, CalibrationArtifactRunWriterError> {
    fs::create_dir_all(quarantine_root_directory).map_err(|source| {
        CalibrationArtifactRunWriterError::CreateDirectoryFailed {
            path: format_path(quarantine_root_directory),
            error_message: source.to_string(),
        }
    })?;

    let target_directory =
        next_quarantine_run_directory(quarantine_root_directory, run_id, reason_token)?;

    fs::rename(source_directory, &target_directory).map_err(|source| {
        CalibrationArtifactRunWriterError::QuarantineRenameFailed {
            run_id: run_id.to_string(),
            source_path: format_path(source_directory),
            target_path: format_path(&target_directory),
            error_message: source.to_string(),
        }
    })?;

    Ok(target_directory)
}

fn next_quarantine_run_directory(
    quarantine_root_directory: &Path,
    run_id: &str,
    reason_token: &str,
) -> Result<PathBuf, CalibrationArtifactRunWriterError> {
    for sequence_number in 1.. {
        let candidate_directory =
            quarantine_root_directory.join(format!("{run_id}.{reason_token}.{sequence_number:04}"));
        if !candidate_directory.exists() {
            return Ok(candidate_directory);
        }
    }

    Err(CalibrationArtifactRunWriterError::QuarantineNameExhausted {
        run_id: run_id.to_string(),
        quarantine_root: format_path(quarantine_root_directory),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationArtifactRunWriterError {
    CreateDirectoryFailed {
        path: String,
        error_message: String,
    },
    ReadDirectoryFailed {
        path: String,
        error_message: String,
    },
    CopyFileFailed {
        source_path: String,
        target_path: String,
        error_message: String,
    },
    CommittedRunAlreadyExists {
        run_id: String,
        path: String,
    },
    PendingRunDirectoryMissing {
        run_id: String,
        path: String,
    },
    CommitRenameFailed {
        run_id: String,
        source_path: String,
        target_path: String,
        error_message: String,
    },
    ExperimentIndexUpdateFailed {
        run_id: String,
        path: String,
        error_message: String,
    },
    QuarantineRenameFailed {
        run_id: String,
        source_path: String,
        target_path: String,
        error_message: String,
    },
    QuarantineNameExhausted {
        run_id: String,
        quarantine_root: String,
    },
}

impl Display for CalibrationArtifactRunWriterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirectoryFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to create persistence directory `{path}`: {error_message}"
            ),
            Self::ReadDirectoryFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to read artifact directory `{path}`: {error_message}"
            ),
            Self::CopyFileFailed {
                source_path,
                target_path,
                error_message,
            } => write!(
                formatter,
                "failed to copy artifact from `{source_path}` to `{target_path}`: {error_message}"
            ),
            Self::CommittedRunAlreadyExists { run_id, path } => write!(
                formatter,
                "committed artifact set already exists for run `{run_id}` in `{path}`"
            ),
            Self::PendingRunDirectoryMissing { run_id, path } => write!(
                formatter,
                "pending artifact set for run `{run_id}` does not exist in `{path}`"
            ),
            Self::CommitRenameFailed {
                run_id,
                source_path,
                target_path,
                error_message,
            } => write!(
                formatter,
                "failed to atomically commit run `{run_id}` from `{source_path}` to `{target_path}`: {error_message}"
            ),
            Self::ExperimentIndexUpdateFailed {
                run_id,
                path,
                error_message,
            } => write!(
                formatter,
                "run `{run_id}` committed in `{path}` but experiment index update failed: {error_message}"
            ),
            Self::QuarantineRenameFailed {
                run_id,
                source_path,
                target_path,
                error_message,
            } => write!(
                formatter,
                "failed to quarantine partial artifacts for run `{run_id}` from `{source_path}` to `{target_path}`: {error_message}"
            ),
            Self::QuarantineNameExhausted {
                run_id,
                quarantine_root,
            } => write!(
                formatter,
                "failed to allocate a quarantine directory for run `{run_id}` under `{quarantine_root}`"
            ),
        }
    }
}

impl Error for CalibrationArtifactRunWriterError {}
