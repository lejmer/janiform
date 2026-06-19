use chrono::{DateTime, Utc};
use janiform_foundations::time::parse_utc_datetime;
use janiform_workflows::forecast::{
    ForecastInitialState, ForecastWorkflowMetadata, ForecastWorkflowObserver,
    ForecastWorkflowRequest, run_forecast,
};

use super::ForecastCommand;
use crate::commands::shared::artifacts::{load_json_artifact, load_typed_json_artifact};
use crate::commands::shared::inputs::parse_exact_positive_interval_duration_nanos;
use crate::commands::shared::output::write_json_payload;
use crate::commands::shared::output_paths::default_run_output_directory;
use crate::commands::shared::terminal::{
    terminal_output_for_context, terminal_progress_adapter_for_context,
};
use crate::context::{CommandContext, OutputMode};
use crate::error::CliError;

pub(crate) fn run_forecast_command(
    context: &CommandContext,
    command: ForecastCommand,
) -> Result<(), CliError> {
    let run_id = forecast_run_id();
    let model_path = required_model_path(context, command.model.model_path)?;
    let model_payload = load_json_artifact(&model_path, "model")?;

    let start_date_utc = match command.start_date_utc.as_deref() {
        Some(value) => parse_timestamp(value, "--start-date-utc")?,
        None => Utc::now(),
    };

    let end_date_utc = match command.end_date_utc.as_deref() {
        Some(value) => Some(parse_timestamp(value, "--end-date-utc")?),
        None => None,
    };

    let state = match command.state_path.as_deref() {
        Some(path) => Some(load_state_artifact(context, path)?),
        None => None,
    };

    let interval_duration_nanos =
        parse_exact_positive_interval_duration_nanos("--interval", command.interval.as_str())?;

    let output_path =
        resolve_output_path(context, command.output_path.output_path, run_id.as_str());

    let request = ForecastWorkflowRequest {
        metadata: ForecastWorkflowMetadata {
            run_id: Some(run_id),
            model_path: Some(model_path),
            output_path: Some(output_path),
        },
        model_payload,
        start_date_utc,
        initial_price: command.initial_price,
        state,
        interval_duration_nanos,
        horizon_interval_count: command.horizon_interval_count,
        end_date_utc,
        measure: command.measure.workflow_measure(),
        risk_free_rate: command.risk_free_rate,
        dividend_yield: command.dividend_yield,
        table_format: command.table_format.workflow_table_format(),
        distribution_node_count: command.distribution_node_count,
        distribution_log_return_lower: command.distribution_log_return_lower,
        distribution_log_return_upper: command.distribution_log_return_upper,
        cpu_count: command.cpu_count,
        treat_forecast_warnings_as_errors: command.treat_forecast_warnings_as_errors,
    };

    let mut progress_adapter = terminal_progress_adapter_for_context(context);

    let output = run_forecast(
        request,
        ForecastWorkflowObserver {
            cancellation_token: context.cancellation_token(),
            progress_adapter: progress_adapter.as_mut(),
            terminal_output: terminal_output_for_context(context),
            progress_event_observer: None,
        },
    )?;

    match context.output_mode() {
        OutputMode::Json => write_json_payload(&output.response_payload()),
        OutputMode::Terminal | OutputMode::Quiet | OutputMode::Silent => Ok(()),
    }
}

fn load_state_artifact(
    context: &CommandContext,
    state_path: &std::path::Path,
) -> Result<ForecastInitialState, CliError> {
    let resolved_path = context.paths().resolve_process_path(state_path);

    if !resolved_path.is_file() {
        return Err(CliError::MissingInputFile {
            argument_name: "--state-path",
            path: resolved_path,
        });
    }

    load_typed_json_artifact(&resolved_path, "forecast state")
}

fn required_model_path(
    context: &CommandContext,
    model_path: Option<std::path::PathBuf>,
) -> Result<std::path::PathBuf, CliError> {
    let model_path = model_path.ok_or(CliError::MissingRequiredArgument {
        argument_name: "--model-path",
    })?;

    let resolved_path = context.paths().resolve_process_path(&model_path);

    if !resolved_path.is_file() {
        return Err(CliError::MissingInputFile {
            argument_name: "--model-path",
            path: resolved_path,
        });
    }

    Ok(resolved_path)
}

fn parse_timestamp(value: &str, argument_name: &'static str) -> Result<DateTime<Utc>, CliError> {
    parse_utc_datetime(value).map_err(|source| CliError::InvalidCommandArgument {
        argument_name,
        message: format!("must be an RFC 3339 timestamp: {source}"),
    })
}

fn resolve_output_path(
    context: &CommandContext,
    output_path: Option<std::path::PathBuf>,
    run_id: &str,
) -> std::path::PathBuf {
    match output_path {
        Some(output_path) => context.paths().resolve_process_path(&output_path),
        None => default_run_output_directory(context, run_id).join("forecast"),
    }
}

fn forecast_run_id() -> String {
    format!("forecast_{}", Utc::now().format("%Y%m%dT%H%M%SZ"))
}
