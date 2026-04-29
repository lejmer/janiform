use std::env;
use std::path::Path;
use std::path::PathBuf;

use crate::error::CliError;

/// Portable path resolver for CLI boundary paths.
#[derive(Debug)]
pub struct PathResolver {
    process_cwd: PathBuf,
    repository_root: Option<PathBuf>,
}

impl PathResolver {
    pub(crate) fn from_process() -> Result<Self, CliError> {
        let process_cwd =
            env::current_dir().map_err(|source| CliError::CurrentDirectory { source })?;
        let repository_root = discover_repository_root(&process_cwd);

        Ok(Self {
            process_cwd,
            repository_root,
        })
    }

    /// Current process working directory used for process-relative CLI paths.
    pub fn process_cwd(&self) -> &Path {
        &self.process_cwd
    }

    /// Discovered repository root, if the command was invoked inside a repository.
    pub fn repository_root(&self) -> Option<&Path> {
        self.repository_root.as_deref()
    }

    /// Resolve a path relative to the process working directory unless it is already absolute.
    pub fn resolve_process_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            return PathBuf::from(path);
        }

        self.process_cwd.join(path)
    }

    /// Resolve a path relative to the repository root unless it is already absolute.
    pub fn resolve_repository_path(&self, path: &Path) -> Result<PathBuf, CliError> {
        if path.is_absolute() {
            return Ok(PathBuf::from(path));
        }

        let Some(repository_root) = &self.repository_root else {
            return Err(CliError::RepositoryRootUnavailable {
                path: PathBuf::from(path),
            });
        };

        Ok(repository_root.join(path))
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

    /// Resolve a CLI output directory without creating it.
    pub fn output_directory(&self, path: &Path) -> PathBuf {
        self.resolve_process_path(path)
    }

    /// Resolve a user preset registry manifest path as a repository-relative input file.
    pub fn user_preset_registry_file(&self, path: &Path) -> Result<PathBuf, CliError> {
        let resolved_path = self.resolve_repository_path(path)?;
        if resolved_path.is_file() {
            return Ok(resolved_path);
        }

        Err(CliError::MissingInputFile {
            argument_name: "--preset-registry",
            path: resolved_path,
        })
    }

    /// Resolve a user preset artifact path as a repository-relative input file.
    pub fn user_preset_file(&self, path: &Path) -> Result<PathBuf, CliError> {
        let resolved_path = self.resolve_repository_path(path)?;
        if resolved_path.is_file() {
            return Ok(resolved_path);
        }

        Err(CliError::MissingInputFile {
            argument_name: "--user-preset",
            path: resolved_path,
        })
    }
}

fn discover_repository_root(start: &Path) -> Option<PathBuf> {
    for candidate in start.ancestors() {
        if candidate.join(".git").exists() || candidate.join("Cargo.toml").is_file() {
            return Some(PathBuf::from(candidate));
        }
    }

    None
}
