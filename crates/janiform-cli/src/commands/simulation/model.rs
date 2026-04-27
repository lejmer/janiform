use chrono::{DateTime, Utc};
use janiform::hourly_factors::ONE_HOUR_NANOS;
use janiform::simulation::SeededTickEventStateSnapshotCapture;
use janiform_foundations::time::parse_duration_spec_nanos;
use janiform_workflows::simulation::{
    ModelSimulationKind, ModelSimulationWorkflowRequest, run_model_simulation_workflow,
    simulation_output_preferences_from_model_payload,
};

use super::SimulateCommand;
use super::execution::{
    complete_progress, completed_diagnostics, completed_raw_payload, direct_simulation_run_id,
    emit_completion, emit_start_summary, progress_adapter_for_context,
    required_horizon_interval_count, required_input_path, resolve_optional_output_path,
    simulation_progress_observer, write_report_and_manifest,
};
use crate::commands::shared::artifact_documents::load_json_artifact;
use crate::context::CommandContext;
use crate::error::CliError;

const MODEL_SEEDED_TICK_SCOPE_LABEL: &str = "model seeded tick simulation";
const MODEL_COARSE_INTERVAL_SCOPE_LABEL: &str = "model coarse interval simulation";

pub(crate) fn run_simulate_model(
    context: &CommandContext,
    command: SimulateCommand,
) -> Result<(), CliError> {
    let seed = required_value("--seed", command.simulation.seed)?;

    let start_time_utc =
        parse_start_time_utc(
            command.simulation.start_time_utc.as_deref().ok_or(
                CliError::MissingRequiredArgument {
                    argument_name: "--start-time-utc",
                },
            )?,
            "--start-time-utc",
        )?;

    let horizon_interval_count =
        required_horizon_interval_count(command.simulation.horizon_interval_count)?;

    let interval_duration_nanos = model_interval_duration_nanos(command.interval.as_deref())?;

    let simulation_kind = if command.interval.is_some() {
        ModelSimulationKind::CoarseInterval
    } else {
        ModelSimulationKind::SeededTick
    };

    let model_path = required_input_path(context, "--model-path", command.model.model_path)?;
    let output_path = resolve_optional_output_path(context, command.output_path.output_path);

    let model_payload = load_json_artifact(&model_path, "model")?;

    let output_preferences = simulation_output_preferences_from_model_payload(&model_payload)?;

    let scope_label = model_simulation_scope_label(simulation_kind);

    let run_id = direct_simulation_run_id("model");

    emit_start_summary(
        context,
        &run_id,
        scope_label,
        &model_path,
        output_path.as_deref(),
    )?;

    let mut progress_adapter =
        progress_adapter_for_context(context, &run_id, output_path.as_deref())?;

    let mut progress_error = None;

    let mut observer = simulation_progress_observer(progress_adapter.as_mut(), &mut progress_error);

    let output = run_model_simulation_workflow(
        ModelSimulationWorkflowRequest {
            model_payload,
            simulation_kind,
            seed,
            start_time_utc,
            horizon_interval_count,
            interval_duration_nanos,
            output_path: output_path.clone(),
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
        },
        &mut observer,
    )?;

    drop(observer);

    if let Some(progress_error) = progress_error {
        return Err(progress_error);
    }

    complete_progress(progress_adapter.as_mut())?;

    if let Some(output_path) = output_path.as_deref() {
        write_report_and_manifest(
            output_path,
            &run_id,
            model_simulation_report_kind(simulation_kind),
            completed_diagnostics(&output.completed, output.export_summary.as_ref()),
        )?;
    }

    let completion = output.completed.completion();
    let elapsed = output.execution_duration;

    emit_completion(
        context,
        scope_label,
        output_path.as_deref(),
        output.export_summary.as_ref(),
        elapsed,
        completion,
        || completed_raw_payload(&output.completed),
    )
}

fn required_value<T>(argument_name: &'static str, value: Option<T>) -> Result<T, CliError> {
    value.ok_or(CliError::MissingRequiredArgument { argument_name })
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

fn model_interval_duration_nanos(interval: Option<&str>) -> Result<i64, CliError> {
    let Some(interval) = interval else {
        return Ok(ONE_HOUR_NANOS);
    };

    parse_duration_spec_nanos(interval).map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "--interval",
        message: source.to_string(),
    })
}

fn model_simulation_scope_label(simulation_kind: ModelSimulationKind) -> &'static str {
    match simulation_kind {
        ModelSimulationKind::SeededTick => MODEL_SEEDED_TICK_SCOPE_LABEL,
        ModelSimulationKind::CoarseInterval => MODEL_COARSE_INTERVAL_SCOPE_LABEL,
    }
}

fn model_simulation_report_kind(simulation_kind: ModelSimulationKind) -> &'static str {
    match simulation_kind {
        ModelSimulationKind::SeededTick => "seeded_tick",
        ModelSimulationKind::CoarseInterval => "coarse_interval",
    }
}
