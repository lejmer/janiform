use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use miette::Diagnostic;
use thiserror::Error;

/// Stable CLI process exit codes.
///
/// `UnimplementedCommand` is reserved for recognized command surfaces that parse
/// successfully but have not yet been wired to their Rust runtime handler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum CliExitCode {
    Success = 0,
    CommandLineUsage = 2,
    UnimplementedCommand = 78,
}

impl CliExitCode {
    pub(crate) fn from_clap_exit_code(exit_code: i32) -> Self {
        match exit_code {
            0 => Self::Success,
            _ => Self::CommandLineUsage,
        }
    }
}

impl From<CliExitCode> for ExitCode {
    fn from(value: CliExitCode) -> Self {
        Self::from(value as u8)
    }
}

#[derive(Debug, Diagnostic, Error)]
pub enum CliError {
    #[error("{message}")]
    #[diagnostic(code(janiform_cli::argument_parse))]
    ArgumentParse { message: String },

    #[error(
        "command `janiform {command_path}` is recognized but is not implemented in the Rust CLI yet"
    )]
    #[diagnostic(code(janiform_cli::unimplemented_command))]
    UnimplementedCommand { command_path: &'static str },

    #[error("failed to read CLI configuration `{}`", path.display())]
    #[diagnostic(code(janiform_cli::config_read))]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to parse CLI configuration `{}`", path.display())]
    #[diagnostic(code(janiform_cli::config_parse))]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error(
        "CLI configuration `{}` has conflicting fields `{first_field}` and `{second_field}`",
        path.display()
    )]
    #[diagnostic(code(janiform_cli::config_conflict))]
    ConfigConflict {
        path: PathBuf,
        first_field: &'static str,
        second_field: &'static str,
    },

    #[error("failed to resolve the process working directory")]
    #[diagnostic(code(janiform_cli::current_directory))]
    CurrentDirectory {
        #[source]
        source: io::Error,
    },

    #[error("cannot resolve repository-relative path `{}` outside a repository", path.display())]
    #[diagnostic(code(janiform_cli::repository_root_unavailable))]
    RepositoryRootUnavailable { path: PathBuf },

    #[error("argument `{argument_name}` must reference an existing file: `{}`", path.display())]
    #[diagnostic(code(janiform_cli::missing_input_file))]
    MissingInputFile {
        argument_name: &'static str,
        path: PathBuf,
    },

    #[error("`--trace-events` requires an event stream selected with `--events jsonl`")]
    #[diagnostic(code(janiform_cli::trace_events_without_event_stream))]
    TraceEventsWithoutEventStream,

    #[error("failed to write command output")]
    #[diagnostic(code(janiform_cli::output_write))]
    OutputWrite {
        #[source]
        source: io::Error,
    },

    #[error("failed to serialize command output")]
    #[diagnostic(code(janiform_cli::output_serialize))]
    OutputSerialize {
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to inspect run directory `{}`", run_directory.display())]
    #[diagnostic(code(janiform_cli::run_inspection_failed))]
    RunInspectionFailed {
        run_directory: PathBuf,
        #[source]
        source: janiform_event_log::EventLogError,
    },
}

impl CliError {
    pub(crate) fn unimplemented(command_path: &'static str) -> Self {
        Self::UnimplementedCommand { command_path }
    }

    pub fn exit_code(&self) -> CliExitCode {
        match self {
            Self::ArgumentParse { .. } => CliExitCode::CommandLineUsage,
            Self::UnimplementedCommand { .. } => CliExitCode::UnimplementedCommand,
            Self::ConfigRead { .. }
            | Self::ConfigParse { .. }
            | Self::ConfigConflict { .. }
            | Self::CurrentDirectory { .. }
            | Self::RepositoryRootUnavailable { .. }
            | Self::MissingInputFile { .. }
            | Self::TraceEventsWithoutEventStream
            | Self::OutputWrite { .. }
            | Self::OutputSerialize { .. }
            | Self::RunInspectionFailed { .. } => CliExitCode::CommandLineUsage,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::ArgumentParse { .. } => "argument_parse",
            Self::UnimplementedCommand { .. } => "unimplemented_command",
            Self::ConfigRead { .. } => "config_read",
            Self::ConfigParse { .. } => "config_parse",
            Self::ConfigConflict { .. } => "config_conflict",
            Self::CurrentDirectory { .. } => "current_directory",
            Self::RepositoryRootUnavailable { .. } => "repository_root_unavailable",
            Self::MissingInputFile { .. } => "missing_input_file",
            Self::TraceEventsWithoutEventStream => "trace_events_without_event_stream",
            Self::OutputWrite { .. } => "output_write",
            Self::OutputSerialize { .. } => "output_serialize",
            Self::RunInspectionFailed { .. } => "run_inspection_failed",
        }
    }
}
