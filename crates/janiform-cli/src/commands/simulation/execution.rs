use std::path::{Path, PathBuf};

use janiform::simulation::{CoarseSimulationPath, SeededTickSimulationRunResult};
use janiform_workflows::simulation::{
    CoarseIntervalExportSettings, DirectSimulationCompleted, DirectSimulationCompletion,
    DirectSimulationJob, DirectSimulationWorkflowRequest, SeededTickExportSettings,
    SimulationExportSummary, SimulationOutputColumns, SimulationWorkflowObserver,
    direct_simulation_scope_label, run_direct_simulation_workflow, simulation_workflow_label,
};
use serde_json::{Value, json};

use super::requests::{load_coarse_simulation_request, load_seeded_tick_simulation_request};
use super::{CoarseIntervalCommand, SeededTicksCommand};
use crate::commands::shared::output::{write_json_payload, write_pretty_json_file};
use crate::commands::shared::terminal::{
    terminal_output_for_context, terminal_progress_adapter_for_context,
};
use crate::context::{CommandContext, OutputMode};
use crate::error::CliError;

pub(crate) fn run_simulate_seeded_ticks_command(
    context: &CommandContext,
    command: SeededTicksCommand,
) -> Result<(), CliError> {
    let request_path = required_input_path(context, "--request-json", command.request_json)?;
    let calendar_registry_path = required_input_path(
        context,
        "--calendar-registry-json",
        command.calendar_registry_json,
    )?;
    let output_path = resolve_optional_output_path(context, command.output_path.output_path);

    let run_config = load_seeded_tick_simulation_request(&request_path, &calendar_registry_path)?;

    let columns = SimulationOutputColumns {
        timestamp_column: command.timestamp_column,
        price_column: command.price_column,
        side_column: command.side_column,
    };

    let reference_price = command.reference_price;
    let batch_size = command.export_batch.output_batch_size;

    execute_direct_simulation(
        context,
        &request_path,
        output_path,
        "seeded",
        "seeded_tick",
        DirectSimulationJob::SeededTick {
            run_config: Box::new(run_config),
            export: SeededTickExportSettings {
                columns,
                reference_price,
                batch_size,
            },
        },
    )
}

pub(crate) fn run_simulate_coarse_interval_command(
    context: &CommandContext,
    command: CoarseIntervalCommand,
) -> Result<(), CliError> {
    let request_path = required_input_path(context, "--request-json", command.request_json)?;
    let output_path = resolve_optional_output_path(context, command.output_path.output_path);

    let request = load_coarse_simulation_request(&request_path)?;

    let calendar_registry = request
        .model_inputs
        .load_calendar_registry()
        .map_err(|source| CliError::InvalidCommandArgument {
            argument_name: "--request-json",
            message: source.to_string(),
        })?;

    // The request is moved into the engine run, while export still needs the validated exchange code.
    let exchange_code = request.model_inputs.exchange_code.clone();

    let columns = SimulationOutputColumns {
        timestamp_column: command.timestamp_column,
        price_column: command.price_column,
        side_column: None,
    };

    let reference_price = command.reference_price;
    let batch_size = command.export_batch.output_batch_size;

    execute_direct_simulation(
        context,
        &request_path,
        output_path,
        "coarse",
        "coarse_interval",
        DirectSimulationJob::CoarseInterval {
            request: Box::new(request),
            export: CoarseIntervalExportSettings {
                columns,
                calendar_registry,
                exchange_code,
                reference_price,
                batch_size,
            },
        },
    )
}

fn execute_direct_simulation(
    context: &CommandContext,
    request_path: &Path,
    output_path: Option<PathBuf>,
    run_id_prefix: &str,
    report_kind: &str,
    job: DirectSimulationJob,
) -> Result<(), CliError> {
    let run_id = direct_simulation_run_id(run_id_prefix);
    let scope_label = direct_simulation_scope_label(&job);

    let mut progress_adapter = terminal_progress_adapter_for_context(context);

    let output = run_direct_simulation_workflow(
        DirectSimulationWorkflowRequest {
            job,
            run_id: run_id.clone(),
            request_path: Some(request_path.to_path_buf()),
            output_path: output_path.clone(),
        },
        SimulationWorkflowObserver {
            progress_adapter: progress_adapter.as_mut(),
            terminal_output: terminal_output_for_context(context),
        },
    )?;

    if let Some(output_path) = output_path.as_deref() {
        write_report_and_manifest(
            output_path,
            &run_id,
            report_kind,
            completed_diagnostics(&output.completed, output.export_summary.as_ref()),
        )?;
    }

    let completion = output.completed.completion();

    emit_completion(
        context,
        scope_label,
        output_path.as_deref(),
        output.export_summary.as_ref(),
        completion,
        || completed_raw_payload(&output.completed),
    )
}

pub(super) fn completed_diagnostics(
    completed: &DirectSimulationCompleted,
    export_summary: Option<&SimulationExportSummary>,
) -> Value {
    match completed {
        DirectSimulationCompleted::SeededTick { result, .. } => {
            seeded_tick_diagnostics_json(result, export_summary)
        }
        DirectSimulationCompleted::CoarseInterval { result, .. } => {
            coarse_diagnostics_json(result, export_summary)
        }
    }
}

pub(super) fn completed_raw_payload(
    completed: &DirectSimulationCompleted,
) -> Result<Value, CliError> {
    match completed {
        DirectSimulationCompleted::SeededTick { result, .. } => {
            Ok(seeded_tick_payload_json(result))
        }
        DirectSimulationCompleted::CoarseInterval { result, .. } => serde_json::to_value(result)
            .map_err(|source| CliError::OutputSerialize {
                source: Box::new(source),
            }),
    }
}

pub(super) fn required_input_path(
    context: &CommandContext,
    argument_name: &'static str,
    path: Option<PathBuf>,
) -> Result<PathBuf, CliError> {
    let path = path.ok_or(CliError::MissingRequiredArgument { argument_name })?;

    context.paths().input_file(argument_name, &path)
}

pub(super) fn required_horizon_interval_count(value: Option<u64>) -> Result<usize, CliError> {
    let value = required_value("--horizon-interval-count", value)?;
    if value == 0 {
        return Err(CliError::InvalidCommandArgument {
            argument_name: "--horizon-interval-count",
            message: "must be a positive integer".to_string(),
        });
    }

    usize::try_from(value).map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "--horizon-interval-count",
        message: format!("does not fit in memory on this platform: {source}"),
    })
}

fn required_value<T>(argument_name: &'static str, value: Option<T>) -> Result<T, CliError> {
    value.ok_or(CliError::MissingRequiredArgument { argument_name })
}

pub(super) fn resolve_optional_output_path(
    context: &CommandContext,
    path: Option<PathBuf>,
) -> Option<PathBuf> {
    path.map(|path| context.paths().resolve_process_path(&path))
}

pub(super) fn emit_completion(
    context: &CommandContext,
    simulation_kind: &str,
    output_path: Option<&Path>,
    export_summary: Option<&SimulationExportSummary>,
    completion: DirectSimulationCompletion,
    raw_payload: impl FnOnce() -> Result<Value, CliError>,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => {
            if let Some(output_path) = output_path {
                write_json_payload(&json!({
                    "schema_version": "janiform.simulation_run.v1",
                    "command_id": simulation_workflow_label(),
                    "status": "completed",
                    "simulation_kind": simulation_kind,
                    "seed": completion.seed,
                    "simulated_interval_count": completion.simulated_interval_count,
                    "output_path": output_path,
                    "export": export_summary.map(export_summary_json),
                }))
            } else {
                let raw_payload = raw_payload()?;
                write_json_payload(&raw_payload)
            }
        }
        OutputMode::Terminal | OutputMode::Quiet | OutputMode::Silent => Ok(()),
    }
}

pub(super) fn write_report_and_manifest(
    output_path: &Path,
    run_id: &str,
    simulation_kind: &str,
    diagnostics: Value,
) -> Result<(), CliError> {
    let run_directory = output_path.parent().unwrap_or_else(|| Path::new("."));

    let report_path = run_directory.join("simulation_report.json");
    let manifest_path = run_directory.join("run.json");

    write_pretty_json_file(
        &report_path,
        &json!({
            "schema_version": "janiform.workflow_report.v1",
            "workflow": simulation_workflow_label(),
            "simulation_kind": simulation_kind,
            "primary_output_path": output_path,
            "diagnostics": diagnostics,
        }),
        artifact_write_error,
    )?;

    write_pretty_json_file(
        &manifest_path,
        &json!({
            "schema_version": "janiform.run_manifest.v1",
            "workflow": simulation_workflow_label(),
            "run_id": run_id,
            "run_directory": run_directory,
            "primary_output_path": output_path,
            "report_path": report_path,
        }),
        artifact_write_error,
    )
}

fn artifact_write_error(path: PathBuf, source: std::io::Error) -> CliError {
    CliError::ArtifactWrite {
        artifact_name: "simulation artifact",
        path,
        source,
    }
}

fn seeded_tick_payload_json(result: &SeededTickSimulationRunResult) -> Value {
    json!({
        "schema_version": result.schema_version,
        "seed": result.seed,
        "start_time_utc": result.start_time_utc.to_rfc3339(),
        "end_time_utc": result.end_time_utc.to_rfc3339(),
        "simulated_interval_count": result.simulated_interval_count,
        "fine_path": result.fine_path_output(),
        "event_tape": result.event_tape_output(),
        "diagnostics": result.native_diagnostics(),
        "debug_trace_enabled": result.debug_trace_enabled,
        "debug_trace": &result.debug_trace_output,
    })
}

fn seeded_tick_diagnostics_json(
    result: &SeededTickSimulationRunResult,
    export_summary: Option<&SimulationExportSummary>,
) -> Value {
    json!({
        "native": result.native_diagnostics(),
        "export": export_summary.map(export_summary_json),
    })
}

fn coarse_diagnostics_json(
    result: &CoarseSimulationPath,
    export_summary: Option<&SimulationExportSummary>,
) -> Value {
    json!({
        "path": &result.diagnostics,
        "export": export_summary.map(export_summary_json),
    })
}

fn export_summary_json(summary: &SimulationExportSummary) -> Value {
    json!({
        "record_count": summary.record_count,
        "chunk_count": summary.chunk_count,
    })
}

pub(super) fn direct_simulation_run_id(prefix: &str) -> String {
    format!(
        "simulate_{prefix}_{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    )
}
