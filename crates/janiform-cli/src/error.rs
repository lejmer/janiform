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
    ReportFailedChecks = 1,
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

    #[error("argument `{argument_name}` must reference an existing file: `{}`", path.display())]
    #[diagnostic(code(janiform_cli::missing_input_file))]
    MissingInputFile {
        argument_name: &'static str,
        path: PathBuf,
    },

    #[error("missing required argument `{argument_name}`")]
    #[diagnostic(code(janiform_cli::missing_required_argument))]
    MissingRequiredArgument { argument_name: &'static str },

    #[error("invalid argument `{argument_name}`: {message}")]
    #[diagnostic(code(janiform_cli::invalid_command_argument))]
    InvalidCommandArgument {
        argument_name: &'static str,
        message: String,
    },

    #[error("conflicting arguments `{first_argument}` and `{second_argument}`: {message}")]
    #[diagnostic(code(janiform_cli::conflicting_command_arguments))]
    ConflictingCommandArguments {
        first_argument: &'static str,
        second_argument: &'static str,
        message: String,
    },

    #[error("failed to prepare fit plan")]
    #[diagnostic(code(janiform_cli::fit_preparation_failed))]
    FitPreparationFailed {
        #[source]
        source: janiform::calibration::staged::CalibrationMarketDataPreparationError,
    },

    #[error("failed to write staged execution input `{}`", path.display())]
    #[diagnostic(code(janiform_cli::staged_execution_input_write))]
    StagedExecutionInputWrite {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to read staged execution input `{}`", path.display())]
    #[diagnostic(code(janiform_cli::staged_execution_input_read))]
    StagedExecutionInputRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to parse staged execution input `{}`", path.display())]
    #[diagnostic(code(janiform_cli::staged_execution_input_parse))]
    StagedExecutionInputParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to build staged execution plan")]
    #[diagnostic(code(janiform_cli::staged_execution_plan_failed))]
    StagedExecutionPlanFailed {
        #[source]
        source: janiform::calibration::staged::CalibrationStagedExecutionBridgeError,
    },

    #[error("fit execution failed: {message}")]
    #[diagnostic(code(janiform_cli::fit_execution_failed))]
    FitExecutionFailed { message: String },

    #[error("failed to write fit artifact `{}`", path.display())]
    #[diagnostic(code(janiform_cli::fit_artifact_write))]
    FitArtifactWrite {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to write fit event log")]
    #[diagnostic(code(janiform_cli::fit_event_log))]
    FitEventLog {
        #[source]
        source: janiform_event_log::EventLogError,
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

    #[error("failed to render command progress")]
    #[diagnostic(code(janiform_cli::output_progress))]
    OutputProgress {
        #[source]
        source: janiform_terminal_ui::ProgressContractError,
    },

    #[error("failed to read {artifact_name} artifact `{}`", path.display())]
    #[diagnostic(code(janiform_cli::artifact_read))]
    ArtifactRead {
        artifact_name: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to parse {artifact_name} artifact `{}`", path.display())]
    #[diagnostic(code(janiform_cli::artifact_parse))]
    ArtifactParse {
        artifact_name: &'static str,
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("malformed {artifact_name} artifact `{}`: {message}", path.display())]
    #[diagnostic(code(janiform_cli::artifact_malformed))]
    ArtifactMalformed {
        artifact_name: &'static str,
        path: PathBuf,
        message: String,
    },

    #[error("failed to write {artifact_name} artifact `{}`", path.display())]
    #[diagnostic(code(janiform_cli::artifact_write))]
    ArtifactWrite {
        artifact_name: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to read user preset `{}`", path.display())]
    #[diagnostic(code(janiform_cli::preset_user_file_read))]
    PresetUserFileRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("invalid user preset `{}`", path.display())]
    #[diagnostic(code(janiform_cli::preset_user_file_invalid))]
    PresetUserFileInvalid {
        path: PathBuf,
        #[source]
        source: janiform::calibration::presets::PresetWorkflowResolutionError,
    },

    #[error(
        "preset workflow `{command_path}` failed for category={category} scope={scope} preset={preset_name}: {source}"
    )]
    #[diagnostic(code(janiform_cli::preset_workflow_failed))]
    PresetWorkflowFailed {
        command_path: &'static str,
        category: String,
        scope: String,
        preset_name: String,
        #[source]
        source: janiform::calibration::presets::PresetWorkflowResolutionError,
    },

    #[error("preset runtime payload for workflow `{command_path}` is malformed: {message}")]
    #[diagnostic(code(janiform_cli::preset_runtime_payload_malformed))]
    PresetRuntimePayloadMalformed {
        command_path: &'static str,
        message: String,
    },

    #[error(
        "failed to inspect run directory `{}`: {message}. latest-state snapshot `{}`. event log `{}`. replay fallback attempted: {replay_fallback_attempted}",
        run_directory.display(),
        snapshot_path.display(),
        event_log_path.display()
    )]
    #[diagnostic(code(janiform_cli::run_inspection_failed))]
    RunInspectionFailed {
        run_directory: PathBuf,
        snapshot_path: PathBuf,
        event_log_path: PathBuf,
        replay_fallback_attempted: bool,
        message: String,
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
            | Self::MissingInputFile { .. }
            | Self::MissingRequiredArgument { .. }
            | Self::InvalidCommandArgument { .. }
            | Self::ConflictingCommandArguments { .. }
            | Self::FitPreparationFailed { .. }
            | Self::StagedExecutionInputWrite { .. }
            | Self::StagedExecutionInputRead { .. }
            | Self::StagedExecutionInputParse { .. }
            | Self::StagedExecutionPlanFailed { .. }
            | Self::FitExecutionFailed { .. }
            | Self::FitArtifactWrite { .. }
            | Self::FitEventLog { .. }
            | Self::TraceEventsWithoutEventStream
            | Self::OutputWrite { .. }
            | Self::OutputSerialize { .. }
            | Self::OutputProgress { .. }
            | Self::ArtifactRead { .. }
            | Self::ArtifactParse { .. }
            | Self::ArtifactMalformed { .. }
            | Self::ArtifactWrite { .. }
            | Self::PresetUserFileRead { .. }
            | Self::PresetUserFileInvalid { .. }
            | Self::PresetWorkflowFailed { .. }
            | Self::PresetRuntimePayloadMalformed { .. }
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
            Self::MissingInputFile { .. } => "missing_input_file",
            Self::MissingRequiredArgument { .. } => "missing_required_argument",
            Self::InvalidCommandArgument { .. } => "invalid_command_argument",
            Self::ConflictingCommandArguments { .. } => "conflicting_command_arguments",
            Self::FitPreparationFailed { .. } => "fit_preparation_failed",
            Self::StagedExecutionInputWrite { .. } => "staged_execution_input_write",
            Self::StagedExecutionInputRead { .. } => "staged_execution_input_read",
            Self::StagedExecutionInputParse { .. } => "staged_execution_input_parse",
            Self::StagedExecutionPlanFailed { .. } => "staged_execution_plan_failed",
            Self::FitExecutionFailed { .. } => "fit_execution_failed",
            Self::FitArtifactWrite { .. } => "fit_artifact_write",
            Self::FitEventLog { .. } => "fit_event_log",
            Self::TraceEventsWithoutEventStream => "trace_events_without_event_stream",
            Self::OutputWrite { .. } => "output_write",
            Self::OutputSerialize { .. } => "output_serialize",
            Self::OutputProgress { .. } => "output_progress",
            Self::ArtifactRead { .. } => "artifact_read",
            Self::ArtifactParse { .. } => "artifact_parse",
            Self::ArtifactMalformed { .. } => "artifact_malformed",
            Self::ArtifactWrite { .. } => "artifact_write",
            Self::PresetUserFileRead { .. } => "preset_user_file_read",
            Self::PresetUserFileInvalid { .. } => "preset_user_file_invalid",
            Self::PresetWorkflowFailed { .. } => "preset_workflow_failed",
            Self::PresetRuntimePayloadMalformed { .. } => "preset_runtime_payload_malformed",
            Self::RunInspectionFailed { .. } => "run_inspection_failed",
        }
    }
}

impl From<janiform_workflows::error::WorkflowError> for CliError {
    fn from(value: janiform_workflows::error::WorkflowError) -> Self {
        match value {
            janiform_workflows::error::WorkflowError::StagedExecutionPlanFailed { source } => {
                Self::StagedExecutionPlanFailed { source }
            }
            janiform_workflows::error::WorkflowError::CalendarRegistryLoadFailed { message }
            | janiform_workflows::error::WorkflowError::FitExecutionFailed { message }
            | janiform_workflows::error::WorkflowError::OutputPayloadInvalid { message } => {
                Self::FitExecutionFailed { message }
            }
            janiform_workflows::error::WorkflowError::FitValidationFailed { field, message } => {
                Self::InvalidCommandArgument {
                    argument_name: field,
                    message,
                }
            }
            janiform_workflows::error::WorkflowError::ModelValidationFailed { field, message } => {
                Self::InvalidCommandArgument {
                    argument_name: field,
                    message,
                }
            }
            janiform_workflows::error::WorkflowError::PricingValidationFailed {
                field,
                message,
            } => Self::InvalidCommandArgument {
                argument_name: field,
                message,
            },
            janiform_workflows::error::WorkflowError::ValidationInputFailed { field, message } => {
                Self::InvalidCommandArgument {
                    argument_name: field,
                    message,
                }
            }
            janiform_workflows::error::WorkflowError::ValidationMissingRequired { field } => {
                Self::MissingRequiredArgument {
                    argument_name: field,
                }
            }
            janiform_workflows::error::WorkflowError::SimulationValidationFailed {
                field,
                message,
            } => Self::InvalidCommandArgument {
                argument_name: field,
                message,
            },
            janiform_workflows::error::WorkflowError::FitArtifactWrite { path, source } => {
                Self::FitArtifactWrite { path, source }
            }
            janiform_workflows::error::WorkflowError::SimulationArtifactWrite { path, source } => {
                Self::ArtifactWrite {
                    artifact_name: "simulation artifact",
                    path,
                    source,
                }
            }
            janiform_workflows::error::WorkflowError::PricingArtifactWrite { path, source } => {
                Self::ArtifactWrite {
                    artifact_name: "pricing artifact",
                    path,
                    source,
                }
            }
            janiform_workflows::error::WorkflowError::ValidationArtifactRead { path, source } => {
                Self::ArtifactRead {
                    artifact_name: "validation",
                    path,
                    source,
                }
            }
            janiform_workflows::error::WorkflowError::ValidationArtifactParse { path, source } => {
                Self::ArtifactParse {
                    artifact_name: "validation",
                    path,
                    source,
                }
            }
            janiform_workflows::error::WorkflowError::ValidationArtifactMalformed {
                path,
                message,
            } => Self::ArtifactMalformed {
                artifact_name: "validation",
                path,
                message,
            },
            janiform_workflows::error::WorkflowError::ValidationArtifactWrite { path, source } => {
                Self::ArtifactWrite {
                    artifact_name: "validation",
                    path,
                    source,
                }
            }
            janiform_workflows::error::WorkflowError::PricingExecutionFailed { message } => {
                Self::InvalidCommandArgument {
                    argument_name: "price",
                    message,
                }
            }
            janiform_workflows::error::WorkflowError::FitEventLog { source } => {
                Self::FitEventLog { source }
            }
            janiform_workflows::error::WorkflowError::OutputSerialize { source } => {
                Self::OutputSerialize { source }
            }
            janiform_workflows::error::WorkflowError::OutputWrite { source } => {
                Self::OutputWrite { source }
            }
            janiform_workflows::error::WorkflowError::OutputProgress { source } => {
                Self::OutputProgress { source }
            }
        }
    }
}
