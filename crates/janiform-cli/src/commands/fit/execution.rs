use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use janiform::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader};
use janiform::calibration::{
    CalibrationResumeSettings, CalibrationStageExecutionStatus, CalibrationStagedExecutionInput,
    CalibrationStagedExecutionRuntimeResources, CalibrationStagedRunObserverEvent,
    CalibrationStagedRunReport, MarketDataSessionConfiguration,
    resolve_market_data_session_configuration,
    run_staged_calibration_from_execution_input_with_observer,
};
use janiform_cli_ui::{ProgressAdapter, format_path};
use serde_json::Value;
use serde_json::json;

mod artifacts;
mod events;
mod output;
mod scope;

use artifacts::{build_fit_report_payload, build_model_payload, write_pretty_json};
use events::{
    append_artifact_event, failure_reason, progress_event_from_staged_event,
    run_events_from_staged_event,
};
use janiform_event_log::{RunEventLogWriter, WorkflowRef, default_run_event_paths};
use output::{emit_completion_output, emit_start_summary};
use scope::workflow_scope_label_for_input;

use super::args::FitCommand;
use super::args::StagedExecutionOptions;
use super::preparation::{
    FitPreparationOverrides, FitPreviewPaths, PreparedFitPreview, prepare_fit_preview,
};
use crate::commands::options::CalibrationStageOption;
use crate::commands::shared::calibration::calibration_execution_mode;
use crate::commands::shared::calibration::calibration_stage;
use crate::commands::shared::output::write_json_event;
use crate::commands::shared::output_paths::default_run_output_directory;
use crate::context::CommandContext;
use crate::context::EventStreamFormat;
use crate::context::OutputMode;
use crate::error::CliError;

pub(crate) fn run_fit_execution(
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

pub(crate) fn run_fit_staged_execution(
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
            source,
        })?;

    if let Some(checkpoint_directory) = options.disk_checkpoint_directory.as_ref() {
        let checkpoint_directory = context.paths().resolve_process_path(checkpoint_directory);
        execution_input.runtime_resources = Some(CalibrationStagedExecutionRuntimeResources {
            disk_checkpoint_directory: Some(checkpoint_directory.display().to_string()),
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
    ) -> FitPreviewPaths {
        match self {
            Self::PreparedFit(prepared) => FitPreviewPaths {
                output_directory: prepared.paths.output_directory.clone(),
                model_path: prepared.paths.model_path.clone(),
                report_path: prepared.paths.report_path.clone(),
                checkpoint_directory: prepared.paths.checkpoint_directory.clone(),
            },
            Self::StagedInput { .. } => staged_execution_paths(context, execution_input),
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

    fn staged_execution_input_path(&self) -> Option<&Path> {
        match self {
            Self::PreparedFit(_) => None,
            Self::StagedInput {
                execution_input_path,
            } => Some(execution_input_path.as_path()),
        }
    }

    fn session_definition_json(
        &self,
        execution_input: &CalibrationStagedExecutionInput,
    ) -> Result<String, CliError> {
        match self {
            Self::PreparedFit(prepared) => Ok(prepared
                .plan
                .session_resolution
                .resolved_session_definition_json
                .clone()),
            Self::StagedInput { .. } => {
                let session_resolution = resolve_market_data_session_configuration(
                    MarketDataSessionConfiguration {
                        exchange_code: execution_input.request.data_inputs.exchange_code.clone(),
                        session_definition_source: None,
                        session_timezone: None,
                    },
                    None,
                )
                .map_err(|source| CliError::FitExecutionFailed {
                    message: format!("failed to resolve staged calendar registry: {source}"),
                })?;

                Ok(session_resolution.resolved_session_definition_json)
            }
        }
    }
}

fn run_execution_input(
    context: &CommandContext,
    execution_input: CalibrationStagedExecutionInput,
    artifacts: ExecutionArtifacts<'_>,
) -> Result<(), CliError> {
    let _execution_plan = execution_input
        .clone()
        .build_execution_plan()
        .map_err(|source| CliError::StagedExecutionPlanFailed { source })?;

    let session_definition_json = artifacts.session_definition_json(&execution_input)?;

    let calendar_registry = ExchangeCalendarRegistryLoader::load_from_json(
        session_definition_json.as_str(),
    )
    .map_err(|source| CliError::FitExecutionFailed {
        message: format!("failed to load resolved calendar registry: {source}"),
    })?;

    let paths = artifacts.paths(context, &execution_input);

    fs::create_dir_all(paths.output_directory.as_path()).map_err(|source| {
        CliError::FitArtifactWrite {
            path: paths.output_directory.clone(),
            source,
        }
    })?;

    let event_paths = default_run_event_paths(paths.output_directory.as_path());

    let workflow = WorkflowRef::labeled("fit", "Fit");

    let mut event_writer = RunEventLogWriter::create(
        &event_paths.event_log_path,
        Some(&event_paths.latest_state_snapshot_path),
        execution_input.request.run_id.clone(),
        workflow.clone(),
    )
    .map_err(|source| CliError::FitEventLog { source })?;

    let input_paths = artifacts.input_paths(&execution_input);

    emit_start_summary(context, &execution_input, input_paths.as_slice(), &paths)?;

    let output_directory = format_path(paths.output_directory.as_path());

    let (report, execution_duration) = run_staged_with_observer(
        context,
        &execution_input,
        &calendar_registry,
        &mut event_writer,
        &workflow,
        input_paths.as_slice(),
        output_directory.as_str(),
    )?;

    write_execution_artifacts(
        &paths,
        &execution_input,
        &report,
        &artifacts,
        &mut event_writer,
        &workflow,
    )?;

    event_writer
        .flush()
        .map_err(|source| CliError::FitEventLog { source })?;

    emit_completion_output(
        context,
        &execution_input,
        &report,
        &paths,
        event_paths.event_log_path.as_path(),
        execution_duration,
    )?;

    if report.status != CalibrationStageExecutionStatus::Success {
        return Err(CliError::FitExecutionFailed {
            message: failure_reason(&report)?,
        });
    }

    Ok(())
}

fn run_staged_with_observer(
    context: &CommandContext,
    execution_input: &CalibrationStagedExecutionInput,
    calendar_registry: &ExchangeCalendarRegistry,
    event_writer: &mut RunEventLogWriter,
    workflow: &WorkflowRef,
    input_paths: &[String],
    output_directory: &str,
) -> Result<(CalibrationStagedRunReport, Duration), CliError> {
    let mut progress_adapter = match context.output_mode() {
        OutputMode::Human | OutputMode::Quiet => Some(ProgressAdapter::stderr()),
        OutputMode::Json | OutputMode::Silent => None,
    };

    let emit_machine_events =
        context.event_settings().stream_format == Some(EventStreamFormat::Jsonl);

    let scope_id = scope::workflow_scope_id(execution_input);

    let scope_label = workflow_scope_label_for_input(execution_input);

    let mut observer_error: Option<CliError> = None;

    let mut observer = |event: CalibrationStagedRunObserverEvent| {
        if observer_error.is_some() {
            return;
        }

        if let Err(error) = observe_progress(&mut progress_adapter, &event) {
            observer_error = Some(error);
            return;
        }

        if let Err(error) = observe_run_events(
            event_writer,
            workflow,
            scope_id.as_str(),
            scope_label,
            input_paths,
            output_directory,
            emit_machine_events,
            &event,
        ) {
            observer_error = Some(error);
        }
    };

    let execution_started_at = Instant::now();

    let run_result = run_staged_calibration_from_execution_input_with_observer(
        execution_input.clone(),
        calendar_registry,
        None,
        &mut observer,
    );

    let execution_duration = execution_started_at.elapsed();

    drop(observer);

    if let Some(observer_error) = observer_error {
        return Err(observer_error);
    }

    let report = run_result.map_err(|source| CliError::StagedExecutionPlanFailed { source })?;

    Ok((report, execution_duration))
}

fn observe_progress(
    progress_adapter: &mut Option<ProgressAdapter>,
    event: &CalibrationStagedRunObserverEvent,
) -> Result<(), CliError> {
    let Some(progress_event) = progress_event_from_staged_event(event)? else {
        return Ok(());
    };

    let Some(progress_adapter) = progress_adapter.as_mut() else {
        return Ok(());
    };

    progress_adapter.observe(progress_event);
    progress_adapter
        .draw_latest()
        .map_err(|source| CliError::OutputProgress { source })
}

fn observe_run_events(
    event_writer: &mut RunEventLogWriter,
    workflow: &WorkflowRef,
    scope_id: &str,
    scope_label: &str,
    input_paths: &[String],
    output_directory: &str,
    emit_machine_events: bool,
    event: &CalibrationStagedRunObserverEvent,
) -> Result<(), CliError> {
    let run_events = run_events_from_staged_event(
        event,
        workflow.clone(),
        scope_id,
        scope_label,
        input_paths,
        output_directory,
    )?;

    for run_event in run_events {
        event_writer
            .append_event(&run_event)
            .map_err(|source| CliError::FitEventLog { source })?;
        if emit_machine_events {
            write_json_event(&run_event)?;
        }
    }

    Ok(())
}

fn write_execution_artifacts(
    paths: &FitPreviewPaths,
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    artifacts: &ExecutionArtifacts<'_>,
    event_writer: &mut RunEventLogWriter,
    workflow: &WorkflowRef,
) -> Result<(), CliError> {
    let report_payload = build_fit_report_payload(
        execution_input,
        report,
        artifacts.preparation_payload(),
        artifacts.staged_execution_input_path(),
    )?;

    write_pretty_json(paths.report_path.as_path(), &report_payload)?;

    append_artifact_event(
        event_writer,
        workflow.clone(),
        execution_input.request.run_id.as_str(),
        "fit_report",
        paths.report_path.as_path(),
        None,
    )?;

    if report.status != CalibrationStageExecutionStatus::Success {
        return Ok(());
    }

    let model_payload = build_model_payload(execution_input, report, &report_payload)?;

    write_pretty_json(paths.model_path.as_path(), &model_payload)?;

    append_artifact_event(
        event_writer,
        workflow.clone(),
        execution_input.request.run_id.as_str(),
        "model",
        paths.model_path.as_path(),
        None,
    )
}

fn staged_execution_paths(
    context: &CommandContext,
    execution_input: &CalibrationStagedExecutionInput,
) -> FitPreviewPaths {
    let checkpoint_directory = execution_input
        .runtime_resources
        .as_ref()
        .and_then(|resources| resources.disk_checkpoint_directory.as_ref())
        .map(PathBuf::from);

    let output_directory = checkpoint_directory
        .as_ref()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| default_run_output_directory(context, &execution_input.request.run_id));

    let checkpoint_directory =
        checkpoint_directory.unwrap_or_else(|| output_directory.join("checkpoints"));

    FitPreviewPaths {
        model_path: output_directory.join("artifacts").join("model.jf"),
        report_path: output_directory.join("artifacts").join("report.json"),
        checkpoint_directory,
        output_directory,
    }
}

fn apply_resume_settings(
    mut execution_input: CalibrationStagedExecutionInput,
    checkpoint_reference: Option<&str>,
    resume_from_stage: Option<CalibrationStageOption>,
) -> CalibrationStagedExecutionInput {
    if let Some(checkpoint_reference) = checkpoint_reference {
        execution_input.request.resume_settings =
            Some(CalibrationResumeSettings::persisted_checkpoint_reference(
                checkpoint_reference.to_string(),
                resume_from_stage.map(calibration_stage),
            ));
    }

    execution_input
}
