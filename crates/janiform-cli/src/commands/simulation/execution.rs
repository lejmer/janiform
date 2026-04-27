use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use janiform::simulation::{
    CoarseSimulationPath, SeededTickSimulationRunResult, SimulationProgressEvent,
    SimulationProgressPhase,
};
use janiform_terminal_ui::{ProgressAdapter, ProgressEvent, ProgressMeasure, format_path};
use serde_json::{Value, json};

use super::artifacts::{SimulationExportSummary, SimulationOutputColumns};
use super::direct::{
    CoarseIntervalExportSettings, DirectSimulationCompleted, DirectSimulationCompletion,
    DirectSimulationJob, SeededTickExportSettings,
};
use super::rendering::{simulation_completion_summary, simulation_start_summary};
use super::requests::{load_coarse_simulation_request, load_seeded_tick_simulation_request};
use super::{CoarseIntervalCommand, SeededTicksCommand};
use crate::commands::shared::output::{write_json_payload, write_pretty_json_file};
use crate::context::{CommandContext, OutputMode};
use crate::error::CliError;

const SEEDED_TICK_SCOPE_LABEL: &str = "seeded tick simulation";
const COARSE_INTERVAL_SCOPE_LABEL: &str = "coarse interval simulation";

pub(crate) fn run_simulate_seeded_ticks(
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
        SEEDED_TICK_SCOPE_LABEL,
        "seeded_tick",
        DirectSimulationJob::SeededTick {
            run_config,
            export: SeededTickExportSettings {
                columns,
                reference_price,
                batch_size,
            },
        },
    )
}

pub(crate) fn run_simulate_coarse_interval(
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
        COARSE_INTERVAL_SCOPE_LABEL,
        "coarse_interval",
        DirectSimulationJob::CoarseInterval {
            request,
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
    scope_label: &str,
    report_kind: &str,
    job: DirectSimulationJob,
) -> Result<(), CliError> {
    let run_id = direct_simulation_run_id(run_id_prefix);

    emit_start_summary(
        context,
        &run_id,
        scope_label,
        request_path,
        output_path.as_deref(),
    )?;

    let mut progress_adapter =
        progress_adapter_for_context(context, &run_id, output_path.as_deref())?;

    let started_at = Instant::now();

    let mut progress_error = None;
    let mut observer = simulation_progress_observer(progress_adapter.as_mut(), &mut progress_error);

    let completed = job.run(&mut observer)?;

    drop(observer);

    if let Some(progress_error) = progress_error {
        return Err(progress_error);
    }

    let elapsed = started_at.elapsed();
    let export_summary = completed.export(output_path.as_deref())?;

    complete_progress(progress_adapter.as_mut())?;

    if let Some(output_path) = output_path.as_deref() {
        write_report_and_manifest(
            output_path,
            &run_id,
            report_kind,
            completed_diagnostics(&completed, export_summary.as_ref()),
        )?;
    }

    let completion = completed.completion();

    emit_completion(
        context,
        scope_label,
        output_path.as_deref(),
        export_summary.as_ref(),
        elapsed,
        completion,
        || completed_raw_payload(&completed),
    )
}

fn completed_diagnostics(
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

fn completed_raw_payload(completed: &DirectSimulationCompleted) -> Result<Value, CliError> {
    match completed {
        DirectSimulationCompleted::SeededTick { result, .. } => {
            Ok(seeded_tick_payload_json(result))
        }
        DirectSimulationCompleted::CoarseInterval { result, .. } => {
            serde_json::to_value(result).map_err(|source| CliError::OutputSerialize { source })
        }
    }
}

fn required_input_path(
    context: &CommandContext,
    argument_name: &'static str,
    path: Option<PathBuf>,
) -> Result<PathBuf, CliError> {
    let path = path.ok_or(CliError::MissingRequiredArgument { argument_name })?;

    context.paths().input_file(argument_name, &path)
}

fn resolve_optional_output_path(
    context: &CommandContext,
    path: Option<PathBuf>,
) -> Option<PathBuf> {
    path.map(|path| context.paths().resolve_process_path(&path))
}

fn progress_adapter_for_context(
    context: &CommandContext,
    run_id: &str,
    output_path: Option<&Path>,
) -> Result<Option<ProgressAdapter>, CliError> {
    match context.output_mode() {
        OutputMode::Human | OutputMode::Quiet => {
            let mut adapter = ProgressAdapter::stderr();

            adapter.observe(ProgressEvent::RunStarted {
                run_id: run_id.to_string(),
                scope: "Simulation".to_string(),
                input_count: 1,
                output_path: output_path.map(format_path),
                stage_sequence: Vec::new(),
            });

            adapter
                .draw_latest()
                .map_err(|source| CliError::OutputProgress { source })?;

            Ok(Some(adapter))
        }
        OutputMode::Json | OutputMode::Silent => Ok(None),
    }
}

fn simulation_progress_observer<'a>(
    progress_adapter: Option<&'a mut ProgressAdapter>,
    progress_error: &'a mut Option<CliError>,
) -> impl FnMut(SimulationProgressEvent) + 'a {
    let mut progress_adapter = progress_adapter;

    move |event| {
        if progress_error.is_some() {
            return;
        }

        let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
            return;
        };

        let phase = match event.phase {
            SimulationProgressPhase::SeededTickIntervals => "seeded tick path generation",
            SimulationProgressPhase::CoarseIntervals => "coarse interval path generation",
        };

        if let Ok(measure) = ProgressMeasure::bounded(event.completed_units, event.total_units) {
            progress_adapter.observe(ProgressEvent::RunProgress {
                phase: phase.to_string(),
                measure,
                latest_signal: Some(format!(
                    "{} of {} intervals completed",
                    event.completed_units, event.total_units
                )),
            });

            if let Err(source) = progress_adapter.draw_latest() {
                *progress_error = Some(CliError::OutputProgress { source });
            }
        }
    }
}

fn complete_progress(progress_adapter: Option<&mut ProgressAdapter>) -> Result<(), CliError> {
    let Some(progress_adapter) = progress_adapter else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunCompleted {
        status: "completed".to_string(),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| CliError::OutputProgress { source })?;

    progress_adapter
        .finish_and_keep()
        .map_err(|source| CliError::OutputWrite { source })
}

fn emit_start_summary(
    context: &CommandContext,
    run_id: &str,
    simulation_kind: &str,
    request_path: &Path,
    output_path: Option<&Path>,
) -> Result<(), CliError> {
    if context.output_mode() != OutputMode::Human {
        return Ok(());
    }

    let rendered = simulation_start_summary(run_id, simulation_kind, request_path, output_path)
        .render(&context.stdout_renderer());

    context
        .stdout_renderer()
        .write_stdout(&format!("\n{rendered}"))
        .map_err(|source| CliError::OutputWrite { source })
}

fn emit_completion(
    context: &CommandContext,
    simulation_kind: &str,
    output_path: Option<&Path>,
    export_summary: Option<&SimulationExportSummary>,
    elapsed: Duration,
    completion: DirectSimulationCompletion,
    raw_payload: impl FnOnce() -> Result<Value, CliError>,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => {
            if let Some(output_path) = output_path {
                write_json_payload(&json!({
                    "schema_version": "janiform.simulation_run.v1",
                    "command_id": "simulate",
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
        OutputMode::Human | OutputMode::Quiet => {
            let rendered = simulation_completion_summary(
                simulation_kind,
                completion.seed,
                output_path,
                export_summary,
                elapsed,
                completion.simulated_interval_count,
            )
            .render(&context.stdout_renderer());

            context
                .stdout_renderer()
                .write_stdout(&rendered)
                .map_err(|source| CliError::OutputWrite { source })
        }
        OutputMode::Silent => Ok(()),
    }
}

fn write_report_and_manifest(
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
            "workflow": "simulate",
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
            "workflow": "simulate",
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

fn direct_simulation_run_id(prefix: &str) -> String {
    format!(
        "simulate_{prefix}_{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    )
}
