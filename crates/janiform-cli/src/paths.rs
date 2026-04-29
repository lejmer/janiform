use std::env;
use std::path::Path;
use std::path::PathBuf;

use crate::error::CliError;

/// Portable path resolver for CLI boundary paths.
#[derive(Debug)]
pub struct PathResolver {
    process_cwd: PathBuf,
}

impl PathResolver {
    pub(crate) fn from_process() -> Result<Self, CliError> {
        let process_cwd =
            env::current_dir().map_err(|source| CliError::CurrentDirectory { source })?;

        Ok(Self { process_cwd })
    }

    /// Current process working directory used for process-relative CLI paths.
    pub fn process_cwd(&self) -> &Path {
        &self.process_cwd
    }

    /// Resolve a path relative to the process working directory unless it is already absolute.
    pub fn resolve_process_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            return PathBuf::from(path);
        }

        self.process_cwd.join(path)
    }

    /// Resolve and validate an input file path supplied through a named CLI argument.
    pub fn input_file(
        &self,
        argument_name: &'static str,
        path: &Path,
    ) -> Result<PathBuf, CliError> {
        let resolved_path = self.resolve_process_path(path);
        if resolved_path.is_file() {
            return Ok(resolved_path);
        }

        Err(CliError::MissingInputFile {
            argument_name,
            path: resolved_path,
        })
    }

    /// Resolve and validate an input file path relative to a known base directory.
    pub fn input_file_relative_to(
        &self,
        argument_name: &'static str,
        base_directory: &Path,
        path: &Path,
    ) -> Result<PathBuf, CliError> {
        let resolved_path = if path.is_absolute() {
            PathBuf::from(path)
        } else {
            base_directory.join(path)
        };

        if resolved_path.is_file() {
            return Ok(resolved_path);
        }

        Err(CliError::MissingInputFile {
            argument_name,
            path: resolved_path,
        })
    }

    /// Resolve a CLI output path without creating it.
    pub fn output_path(&self, path: &Path) -> PathBuf {
        self.resolve_process_path(path)
    }

    /// Resolve a user preset registry manifest path as a process-relative input file.
    pub fn user_preset_registry_file(&self, path: &Path) -> Result<PathBuf, CliError> {
        self.input_file("--preset-registry", path)
    }

    /// Resolve a user preset artifact path as a process-relative input file.
    pub fn user_preset_file(&self, path: &Path) -> Result<PathBuf, CliError> {
        self.input_file("--user-preset", path)
    }
}
