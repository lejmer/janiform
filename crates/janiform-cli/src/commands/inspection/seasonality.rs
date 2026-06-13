use std::path::Path;

use chrono::DateTime;
use chrono::Utc;
use janiform::calibration::staged::{
    RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
    RuntimeStructuralSeasonalityMaterializationInput, materialize_runtime_structural_seasonality,
};
use janiform::model::factors::interval_factor_grid::FourierBusinessTimeClockModel;
use janiform::time::calendar::ExchangeCalendarRegistryLoader;
use janiform_terminal_ui::KeyValueRow;
use janiform_terminal_ui::StartSummary;
use janiform_terminal_ui::TerminalRenderer;
use serde::Serialize;
use serde_json::Value;

use crate::commands::inspection::InspectSeasonalityCommand;
use crate::commands::shared::artifacts::load_json_artifact;
use crate::commands::shared::artifacts::require_json_object;
use crate::commands::shared::inputs::parse_exact_positive_interval_duration_nanos;
use crate::commands::shared::inputs::parse_utc_timestamp_argument;
use crate::commands::shared::output::write_json_payload;
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;
use janiform_foundations::json::query::nested_string;
use janiform_foundations::json::query::nested_value;

const INSPECT_SEASONALITY_SCHEMA_VERSION: &str = "inspection.seasonality_interval_grid.v1";

#[derive(Debug, Clone, PartialEq, Serialize)]
struct InspectSeasonalityOutput {
    schema_version: &'static str,
    model_path: String,
    exchange_code: String,
    range_start_utc_nanos: i64,
    range_end_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_start_utc_nanos: Vec<i64>,
    interval_end_utc_nanos: Vec<i64>,
    seasonality_factor: Vec<f64>,
    materialization_method: &'static str,
}

pub(crate) fn run_inspect_seasonality(
    context: &CommandContext,
    command: InspectSeasonalityCommand,
) -> Result<(), CliError> {
    let model_path = context.paths().resolve_process_path(&command.model);
    let model = load_json_artifact(&model_path, "model")?;

    require_json_object(&model, "model", &model_path)?;

    let range_start_utc =
        parse_utc_timestamp_argument("--range-start-utc", &command.range_start_utc)?;

    let range_end_utc = parse_utc_timestamp_argument("--range-end-utc", &command.range_end_utc)?;

    let interval_duration_nanos =
        parse_exact_positive_interval_duration_nanos("--interval", &command.interval)?;

    let range_start_utc_nanos = timestamp_nanos("--range-start-utc", range_start_utc)?;
    let range_end_utc_nanos = timestamp_nanos("--range-end-utc", range_end_utc)?;

    validate_interval_range(
        range_start_utc_nanos,
        range_end_utc_nanos,
        interval_duration_nanos,
    )?;

    let clock_model = load_clock_model(&model, &model_path)?;
    let exchange_code = model_exchange_code(&model, &model_path)?;
    let calendar_registry = model_calendar_registry(&model, &model_path)?;

    let materialized = materialize_runtime_structural_seasonality(
        &calendar_registry,
        RuntimeStructuralSeasonalityMaterializationInput {
            exchange_code: exchange_code.clone(),
            clock_model,
            range_start_utc_nanos,
            range_end_utc_nanos,
            interval_duration_nanos,
        },
    )
    .map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "--model",
        message: source.to_string(),
    })?;

    let output = InspectSeasonalityOutput {
        schema_version: INSPECT_SEASONALITY_SCHEMA_VERSION,
        model_path: model_path.display().to_string(),
        exchange_code,
        range_start_utc_nanos,
        range_end_utc_nanos,
        interval_duration_nanos,
        interval_end_utc_nanos: materialized
            .interval_start_utc_nanos
            .iter()
            .map(|start_utc_nanos| start_utc_nanos + interval_duration_nanos)
            .collect(),
        interval_start_utc_nanos: materialized.interval_start_utc_nanos,
        seasonality_factor: materialized.seasonality_factors,
        materialization_method: RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
    };

    match context.output_mode() {
        OutputMode::Json => write_json_payload(&output),
        OutputMode::Silent => Ok(()),
        OutputMode::Terminal | OutputMode::Quiet => {
            let rendered = render_seasonality_output(&output, &context.stdout_renderer());
            context
                .stdout_renderer()
                .write_stdout(&rendered)
                .map_err(|source| CliError::OutputWrite { source })
        }
    }
}

fn load_clock_model(
    model: &Value,
    model_path: &Path,
) -> Result<FourierBusinessTimeClockModel, CliError> {
    let clock_model = nested_value(model, &["parameters", "seasonality", "clock_model"])
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name: "model",
            path: model_path.to_path_buf(),
            message: "model is missing parameters.seasonality.clock_model".to_string(),
        })?;

    serde_json::from_value(clock_model.clone()).map_err(|source| CliError::ArtifactMalformed {
        artifact_name: "model",
        path: model_path.to_path_buf(),
        message: format!("parameters.seasonality.clock_model is invalid: {source}"),
    })
}

fn model_exchange_code(model: &Value, model_path: &Path) -> Result<String, CliError> {
    nested_string(model, &["parameters", "seasonality", "exchange_code"])
        .or_else(|| nested_string(model, &["session_resolution", "exchange_code"]))
        .or_else(|| nested_string(model, &["metadata", "session_resolution", "exchange_code"]))
        .map(ToOwned::to_owned)
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name: "model",
            path: model_path.to_path_buf(),
            message: "model is missing a seasonality exchange_code".to_string(),
        })
}

fn model_calendar_registry(
    model: &Value,
    model_path: &Path,
) -> Result<janiform::time::calendar::ExchangeCalendarRegistry, CliError> {
    let registry_json = nested_string(
        model,
        &["session_resolution", "resolved_session_definition_json"],
    )
    .or_else(|| {
        nested_string(
            model,
            &[
                "metadata",
                "session_resolution",
                "resolved_session_definition_json",
            ],
        )
    })
    .ok_or_else(|| CliError::ArtifactMalformed {
        artifact_name: "model",
        path: model_path.to_path_buf(),
        message: "model is missing session_resolution.resolved_session_definition_json".to_string(),
    })?;

    ExchangeCalendarRegistryLoader::load_from_json(registry_json).map_err(|source| {
        CliError::ArtifactMalformed {
            artifact_name: "model",
            path: model_path.to_path_buf(),
            message: format!("resolved session definition is invalid: {source}"),
        }
    })
}

fn timestamp_nanos(
    argument_name: &'static str,
    timestamp_utc: DateTime<Utc>,
) -> Result<i64, CliError> {
    timestamp_utc
        .timestamp_nanos_opt()
        .ok_or_else(|| CliError::InvalidCommandArgument {
            argument_name,
            message: "timestamp is outside the UTC nanosecond range".to_string(),
        })
}

fn validate_interval_range(
    range_start_utc_nanos: i64,
    range_end_utc_nanos: i64,
    interval_duration_nanos: i64,
) -> Result<(), CliError> {
    if range_start_utc_nanos >= range_end_utc_nanos {
        return Err(CliError::InvalidCommandArgument {
            argument_name: "--range-end-utc",
            message: "must be strictly after --range-start-utc".to_string(),
        });
    }

    let duration_nanos = range_end_utc_nanos - range_start_utc_nanos;
    if duration_nanos.rem_euclid(interval_duration_nanos) != 0 {
        return Err(CliError::InvalidCommandArgument {
            argument_name: "--interval",
            message: "must divide the requested UTC range exactly".to_string(),
        });
    }

    Ok(())
}

fn render_seasonality_output(
    output: &InspectSeasonalityOutput,
    renderer: &TerminalRenderer,
) -> String {
    let rows = vec![
        KeyValueRow::new("Model", output.model_path.clone()),
        KeyValueRow::new("Exchange", output.exchange_code.clone()),
        KeyValueRow::new("Intervals", output.seasonality_factor.len().to_string()),
        KeyValueRow::new("Interval nanos", output.interval_duration_nanos.to_string()),
        KeyValueRow::new("Factor min", format!("{:.6}", seasonality_min(output))),
        KeyValueRow::new("Factor max", format!("{:.6}", seasonality_max(output))),
        KeyValueRow::new("Factor mean", format!("{:.6}", seasonality_mean(output))),
    ];

    StartSummary {
        title: "Seasonality".to_string(),
        rows,
    }
    .render(renderer)
}

fn seasonality_min(output: &InspectSeasonalityOutput) -> f64 {
    output
        .seasonality_factor
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min)
}

fn seasonality_max(output: &InspectSeasonalityOutput) -> f64 {
    output
        .seasonality_factor
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max)
}

fn seasonality_mean(output: &InspectSeasonalityOutput) -> f64 {
    let factor_sum: f64 = output.seasonality_factor.iter().sum();

    factor_sum / output.seasonality_factor.len() as f64
}
