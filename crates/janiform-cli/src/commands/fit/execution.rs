use std::fs;
use std::path::PathBuf;

use janiform::calibration::staged::{
    CalibrationStageExecutionStatus, CalibrationStagedExecutionInput,
    CalibrationStagedExecutionRuntimeResources,
};
use janiform_event_log::RunEvent;
use janiform_terminal_ui::{ProgressAdapter, format_path};
use janiform_workflows::error::WorkflowError;
use janiform_workflows::fit::{
    FitWorkflowObserver, FitWorkflowOutput, FitWorkflowPaths, StagedFitExecutionWorkflowRequest,
    apply_resume_settings as apply_fit_resume_settings, failure_reason,
    fit_completion_json_payload,
    run_staged_fit_execution_workflow as run_shared_staged_fit_execution_workflow,
    staged_execution_paths as shared_staged_execution_paths,
};
use janiform_workflows::write_machine_event_to_stderr;
use serde_json::Value;
use serde_json::json;

use super::args::FitCommand;
use super::args::StagedExecutionOptions;
use super::preparation::{FitPreparationOverrides, PreparedFitPreview, prepare_fit_preview};
use crate::commands::options::CalibrationStageOption;
use crate::commands::shared::calibration::calibration_execution_mode;
use crate::commands::shared::calibration::calibration_stage;
use crate::commands::shared::output::write_json_payload;
use crate::commands::shared::output_paths::default_run_output_directory;
use crate::commands::shared::terminal::terminal_output_for_context;
use crate::context::CommandContext;
use crate::context::EventStreamFormat;
use crate::context::OutputMode;
use crate::error::CliError;

type CliMachineEventSink<'a> = &'a mut dyn FnMut(&RunEvent) -> Result<(), WorkflowError>;

pub(crate) fn run_fit_command(
    context: &CommandContext,
    command: FitCommand,
) -> Result<(), CliError> {
    let prepared = prepare_fit_preview(
        context,
        &command.inputs,
        &command.run,
        command.workflow_scope,
        command.execution_mode.map(calibration_execution_mode),
        command.cpu_count,
        FitPreparationOverrides {
            mean_reversion_grid_interval_hours: command.mean_reversion_grid_interval_hours,
            dynamics_search_restart_candidate_budget: command
                .dynamics_search_restart_candidate_budget,
        },
    )?;

    let execution_input = apply_resume_settings(
        prepared.plan.execution_input.clone(),
        command.resume_checkpoint_reference.as_deref(),
        command.resume_from_stage,
    );

    run_execution_input(
        context,
        execution_input,
        ExecutionArtifacts::PreparedFit(&prepared),
    )
}

pub(crate) fn run_fit_staged_run_command(
    context: &CommandContext,
    options: StagedExecutionOptions,
) -> Result<(), CliError> {
    let execution_input_path =
        options
            .execution_input_path
            .as_ref()
            .ok_or(CliError::MissingRequiredArgument {
                argument_name: "--execution-input-path",
            })?;

    let execution_input_path = context.paths().resolve_process_path(execution_input_path);

    let file = fs::File::open(&execution_input_path).map_err(|source| {
        CliError::StagedExecutionInputRead {
            path: execution_input_path.clone(),
            source,
        }
    })?;

    let mut execution_input: CalibrationStagedExecutionInput = serde_json::from_reader(file)
        .map_err(|source| CliError::StagedExecutionInputParse {
            path: execution_input_path.clone(),
            source: Box::new(source),
        })?;

    if let Some(persistence_root_directory) = options.disk_checkpoint_directory.as_ref() {
        let persistence_root_directory = context
            .paths()
            .resolve_process_path(persistence_root_directory);

        execution_input.runtime_resources = Some(CalibrationStagedExecutionRuntimeResources {
            disk_checkpoint_directory: Some(persistence_root_directory.display().to_string()),
        });
    }

    let execution_input = apply_resume_settings(
        execution_input,
        options.resume_checkpoint_reference.as_deref(),
        options.resume_from_stage,
    );

    run_execution_input(
        context,
        execution_input,
        ExecutionArtifacts::StagedInput {
            execution_input_path,
        },
    )
}

enum ExecutionArtifacts<'a> {
    PreparedFit(&'a PreparedFitPreview),
    StagedInput { execution_input_path: PathBuf },
}

impl ExecutionArtifacts<'_> {
    fn paths(
        &self,
        context: &CommandContext,
        execution_input: &CalibrationStagedExecutionInput,
    ) -> FitWorkflowPaths {
        match self {
            Self::PreparedFit(prepared) => prepared.paths.clone(),
            Self::StagedInput { .. } => shared_staged_execution_paths(
                execution_input,
                default_run_output_directory(context, &execution_input.request.run_id),
            ),
        }
    }

    fn input_paths(&self, execution_input: &CalibrationStagedExecutionInput) -> Vec<String> {
        match self {
            Self::PreparedFit(prepared) => {
                let mut paths = vec![format!(
                    "ticks: {}",
                    format_path(&prepared.inputs.tick_data_path)
                )];

                if let Some(option_surface_path) = &prepared.inputs.option_surface_path {
                    paths.push(format!("options: {}", format_path(option_surface_path)));
                }

                if let Some(known_news_path) = &prepared.inputs.known_news_path {
                    paths.push(format!("known news: {}", format_path(known_news_path)));
                }

                paths
            }
            Self::StagedInput {
                execution_input_path,
            } => vec![
                format!("staged input: {}", format_path(execution_input_path)),
                format!(
                    "ticks: {}",
                    execution_input
                        .request
                        .data_inputs
                        .trade_events_data_reference
                ),
            ],
        }
    }

    fn preparation_payload(&self) -> Option<Value> {
        match self {
            Self::PreparedFit(prepared) => Some(json!({
                "request": prepared.request,
                "diagnostics": prepared.plan.diagnostics,
                "provenance": prepared.plan.provenance,
                "session_resolution": prepared.plan.session_resolution,
                "reference_price": prepared.plan.reference_price,
                "fit_warning": prepared.plan.fit_warning,
            })),
            Self::StagedInput { .. } => None,
        }
    }

    fn staged_execution_input_path(&self) -> Option<PathBuf> {
        match self {
            Self::PreparedFit(_) => None,
            Self::StagedInput {
                execution_input_path,
            } => Some(execution_input_path.clone()),
        }
    }

    fn session_definition_json(&self) -> Option<String> {
        match self {
            Self::PreparedFit(prepared) => Some(
                prepared
                    .plan
                    .session_resolution
                    .resolved_session_definition_json
                    .clone(),
            ),
            Self::StagedInput { .. } => None,
        }
    }
}

fn run_execution_input(
    context: &CommandContext,
    execution_input: CalibrationStagedExecutionInput,
    artifacts: ExecutionArtifacts<'_>,
) -> Result<(), CliError> {
    let paths = artifacts.paths(context, &execution_input);
    let input_paths = artifacts.input_paths(&execution_input);

    let terminal_output = terminal_output_for_context(context);

    let mut progress_adapter = match context.output_mode() {
        OutputMode::Human | OutputMode::Quiet => Some(ProgressAdapter::stderr()),
        OutputMode::Json | OutputMode::Silent => None,
    };

    let mut machine_event_sink = write_machine_event_to_stderr;

    // Completion rendering happens after the shared runner consumes the request.
    // Keep boundary copies here rather than making the workflow API borrow CLI state.
    let completion_execution_input = execution_input.clone();
    let completion_paths = paths.clone();

    let output = run_shared_staged_fit_execution_workflow(
        StagedFitExecutionWorkflowRequest {
            execution_input,
            paths,
            input_paths,
            preparation_payload: artifacts.preparation_payload(),
            staged_execution_input_path: artifacts.staged_execution_input_path(),
            session_definition_json: artifacts.session_definition_json(),
        },
        FitWorkflowObserver {
            progress_adapter: progress_adapter.as_mut(),
            terminal_output,
            machine_event_sink: machine_event_sink_for_context(context, &mut machine_event_sink),
        },
    )?;

    emit_completion_output(
        context,
        &completion_execution_input,
        &completion_paths,
        &output,
    )?;

    if output.report.status != CalibrationStageExecutionStatus::Success {
        return Err(CliError::FitExecutionFailed {
            message: failure_reason(&output.report)?,
        });
    }

    Ok(())
}

fn emit_completion_output(
    context: &CommandContext,
    execution_input: &CalibrationStagedExecutionInput,
    paths: &FitWorkflowPaths,
    output: &FitWorkflowOutput,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => write_json_payload(&fit_completion_json_payload(
            execution_input,
            &output.report,
            paths,
            output.event_log_path.as_path(),
        )),
        OutputMode::Human | OutputMode::Quiet | OutputMode::Silent => Ok(()),
    }
}

fn machine_event_sink_for_context<'a>(
    context: &CommandContext,
    machine_event_sink: CliMachineEventSink<'a>,
) -> Option<CliMachineEventSink<'a>> {
    if context.event_settings().stream_format == Some(EventStreamFormat::Jsonl) {
        return Some(machine_event_sink);
    }

    None
}

fn apply_resume_settings(
    execution_input: CalibrationStagedExecutionInput,
    checkpoint_reference: Option<&str>,
    resume_from_stage: Option<CalibrationStageOption>,
) -> CalibrationStagedExecutionInput {
    apply_fit_resume_settings(
        execution_input,
        checkpoint_reference,
        resume_from_stage.map(calibration_stage),
    )
}
