use chrono::{DateTime, Utc};
use janiform::simulation::SeededTickEventStateSnapshotCapture;
use janiform_foundations::time::parse_duration_spec_nanos;
use janiform_workflows::simulation::{
    ModelArtifactSimulationKind, ModelArtifactSimulationWorkflowRequest,
    SimulationWorkflowObserver, run_simulation_from_model_artifact_workflow,
    simulation_output_preferences_from_model_payload,
};

use super::SimulateCommand;
use super::execution::{
    completed_diagnostics, completed_raw_payload, direct_simulation_run_id, emit_completion,
    required_horizon_interval_count, required_input_path, write_report_and_manifest,
};
use crate::commands::shared::artifact_documents::load_json_artifact;
use crate::commands::shared::output_paths::default_run_output_directory;
use crate::commands::shared::terminal::{
    terminal_output_for_context, terminal_progress_adapter_for_context,
};
use crate::context::CommandContext;
use crate::error::CliError;

const DEFAULT_MODEL_ARTIFACT_SIMULATION_SEED: u64 = 0;
const MODEL_ARTIFACT_SEEDED_TICK_SCOPE_LABEL: &str = "model artifact seeded tick simulation";
const MODEL_ARTIFACT_COARSE_INTERVAL_SCOPE_LABEL: &str =
    "model artifact coarse interval simulation";

pub(crate) fn run_simulate_from_model_artifact_command(
    context: &CommandContext,
    command: SimulateCommand,
) -> Result<(), CliError> {
    let seed = command
        .simulation
        .seed
        .unwrap_or(DEFAULT_MODEL_ARTIFACT_SIMULATION_SEED);

    let start_time_utc = match command.simulation.start_time_utc.as_deref() {
        Some(start_time_utc) => parse_start_time_utc(start_time_utc, "--start-time-utc")?,
        None => Utc::now(),
    };

    let horizon_interval_count =
        required_horizon_interval_count(command.simulation.horizon_interval_count)?;

    let interval_duration_nanos = model_interval_duration_nanos(command.interval.as_str())?;

    let simulation_kind = if command.coarse {
        ModelArtifactSimulationKind::CoarseInterval
    } else {
        ModelArtifactSimulationKind::SeededTick
    };

    let run_id = direct_simulation_run_id("model");

    let model_path = required_input_path(context, "--model-path", command.model.model_path)?;

    let model_payload = load_json_artifact(&model_path, "model")?;

    let output_preferences = simulation_output_preferences_from_model_payload(&model_payload)?;

    let output_path = resolve_model_output_path(
        context,
        command.output_path.output_path,
        &run_id,
        output_preferences.default_file_format,
    );

    let scope_label = model_artifact_simulation_scope_label(simulation_kind);

    let workflow_request = ModelArtifactSimulationWorkflowRequest {
        model_payload,
        run_id: Some(run_id.clone()),
        scope_label: Some(scope_label.to_string()),
        request_path: Some(model_path),
        simulation_kind,
        seed,
        start_time_utc,
        horizon_interval_count,
        interval_duration_nanos,
        output_path: Some(output_path.clone()),
        output_columns: output_preferences.output_columns,
        reference_price: output_preferences.reference_price,
        export_batch_size: command.export_batch.output_batch_size,
        continuation_data: None,
        initial_excitation_state: None,
        initial_mean_reversion_boundary_state: None,
        debug_trace_enabled: command.debug_trace_enabled,
        sample_every_nth_transition: command.sample_every_nth_transition,
        max_records_per_chunk: command.max_records_per_chunk,
        state_snapshot_capture: SeededTickEventStateSnapshotCapture::default(),
    };

    let mut progress_adapter = terminal_progress_adapter_for_context(context);

    let output = run_simulation_from_model_artifact_workflow(
        workflow_request,
        SimulationWorkflowObserver {
            progress_adapter: progress_adapter.as_mut(),
            terminal_output: terminal_output_for_context(context),
        },
    )?;

    write_report_and_manifest(
        output_path.as_path(),
        &run_id,
        model_artifact_simulation_report_kind(simulation_kind),
        completed_diagnostics(&output.completed, output.export_summary.as_ref()),
    )?;

    let completion = output.completed.completion();

    emit_completion(
        context,
        scope_label,
        Some(output_path.as_path()),
        output.export_summary.as_ref(),
        completion,
        || completed_raw_payload(&output.completed),
    )
}

fn parse_start_time_utc(
    value: &str,
    argument_name: &'static str,
) -> Result<DateTime<Utc>, CliError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|source| CliError::InvalidCommandArgument {
            argument_name,
            message: format!("must be an RFC 3339 timestamp: {source}"),
        })
}

fn resolve_model_output_path(
    context: &CommandContext,
    output_path: Option<std::path::PathBuf>,
    run_id: &str,
    default_file_format: &str,
) -> std::path::PathBuf {
    let Some(output_path) = output_path else {
        return default_run_output_directory(context, run_id)
            .join(format!("simulation.{default_file_format}"));
    };

    let mut resolved_path = context.paths().resolve_process_path(&output_path);
    if resolved_path.extension().is_none() {
        resolved_path.set_extension(default_file_format);
    }

    resolved_path
}

fn model_interval_duration_nanos(interval: &str) -> Result<i64, CliError> {
    parse_duration_spec_nanos(interval).map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "--interval",
        message: source.to_string(),
    })
}

fn model_artifact_simulation_scope_label(
    simulation_kind: ModelArtifactSimulationKind,
) -> &'static str {
    match simulation_kind {
        ModelArtifactSimulationKind::SeededTick => MODEL_ARTIFACT_SEEDED_TICK_SCOPE_LABEL,
        ModelArtifactSimulationKind::CoarseInterval => MODEL_ARTIFACT_COARSE_INTERVAL_SCOPE_LABEL,
    }
}

fn model_artifact_simulation_report_kind(
    simulation_kind: ModelArtifactSimulationKind,
) -> &'static str {
    match simulation_kind {
        ModelArtifactSimulationKind::SeededTick => "seeded_tick",
        ModelArtifactSimulationKind::CoarseInterval => "coarse_interval",
    }
}
