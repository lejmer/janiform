use std::path::PathBuf;

use janiform::model::factors::hourly::{
    DEFAULT_SEASONALITY_AGGREGATION_METHOD, DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
    DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY, DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT,
    HourlyFactorContractBuildInput, HourlyNewsIntensityMaterializationInput,
    HourlySeasonalityMaterializationInput, HourlySeasonalityMaterializationPolicy,
    HourlySeasonalitySourceSignal, build_hourly_factor_contract,
    materialize_hourly_news_intensity_factors, materialize_hourly_seasonality_factors,
};

use super::args::{
    BuildHourlyFactorContractCommand, MaterializeNewsIntensityCommand,
    MaterializeSeasonalityCommand,
};
use super::inputs::{
    SourceKind, f64_array_field, f64_field, load_factor_source, object_document,
    optional_multiplier_entries, optional_string_field, optional_timestamp_nanos_array_field,
    optional_usize_field, resolve_common_options, timestamp_nanos_array_field,
};
use super::payloads::{
    hourly_factor_contract_payload, news_intensity_materialization_payload,
    seasonality_materialization_payload,
};
use super::rendering::{
    contract_rows, emit_hourly_output, news_intensity_materialization_rows,
    seasonality_materialization_rows,
};
use crate::commands::shared::artifacts::{load_calendar_registry_artifact, load_json_artifact};
use crate::commands::shared::inputs::required_input_file;
use crate::commands::shared::output::write_pretty_json_file;
use crate::commands::shared::progress::TerminalCommandProgress;
use crate::context::CommandContext;
use crate::error::CliError;

pub(crate) fn run_hourly_factor_materialize_seasonality(
    context: &CommandContext,
    command: MaterializeSeasonalityCommand,
) -> Result<(), CliError> {
    let resolved = resolve_common_options(context, &command.common)?;

    let input_path = required_input_file(
        context,
        "--materialization-input-json",
        &command.materialization_input_json,
    )?;

    let output_path = command
        .output_path
        .output_path
        .map(|path| context.paths().resolve_process_path(&path));

    let mut progress = TerminalCommandProgress::start(
        context,
        "hourly seasonality materialization",
        2,
        output_path.as_deref(),
        &[
            "load calendar registry",
            "load materialization input",
            "materialize seasonality",
        ],
    )?;

    progress.observe("load calendar registry", 1)?;

    let calendar_registry = load_calendar_registry_artifact(&resolved.calendar_registry_path)?;

    progress.observe("load materialization input", 2)?;

    let input_document = load_json_artifact(&input_path, "hourly seasonality input")?;

    let input_fields = object_document(&input_document, "hourly seasonality input", &input_path)?;

    let source_timestamps = timestamp_nanos_array_field(
        input_fields,
        "source_timestamps_utc",
        "hourly seasonality input",
        &input_path,
    )?;

    let source_factors = f64_array_field(
        input_fields,
        "source_seasonality_factors",
        "hourly seasonality input",
        &input_path,
    )?;

    let interpolation_method = optional_string_field(input_fields, "interpolation_method")
        .unwrap_or(DEFAULT_SEASONALITY_INTERPOLATION_METHOD);

    let aggregation_method = optional_string_field(input_fields, "aggregation_method")
        .unwrap_or(DEFAULT_SEASONALITY_AGGREGATION_METHOD);

    let sparse_fallback_policy = optional_string_field(input_fields, "sparse_fallback_policy")
        .unwrap_or(DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY);

    let sparse_hour_minimum_observation_count = optional_usize_field(
        input_fields,
        "sparse_hour_minimum_observation_count",
        DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT,
        "hourly seasonality input",
        &input_path,
    )?;

    progress.observe("materialize seasonality", 3)?;

    let output = materialize_hourly_seasonality_factors(HourlySeasonalityMaterializationInput {
        calendar_registry: &calendar_registry,
        exchange_code: &resolved.exchange_code,
        range_utc: resolved.range_start_utc..resolved.range_end_utc,
        interval_duration_nanos: resolved.interval_duration_nanos,
        source_signal: HourlySeasonalitySourceSignal {
            timestamp_utc_nanos: &source_timestamps,
            seasonality_factors: &source_factors,
        },
        policy: HourlySeasonalityMaterializationPolicy {
            interpolation_method,
            aggregation_method,
            sparse_fallback_policy,
            sparse_hour_minimum_observation_count,
        },
    })
    .map_err(|source| hourly_factor_argument_error("materialize seasonality", source))?;
    let payload = seasonality_materialization_payload(&output);

    if let Some(output_path) = output_path.as_deref() {
        write_pretty_json_file(output_path, &payload, hourly_factor_artifact_write_error)?;
    }

    progress.complete()?;

    emit_hourly_output(
        context,
        &payload,
        output_path.as_deref(),
        "Hourly seasonality materialized",
        seasonality_materialization_rows(&output, output_path.as_deref()),
    )
}

pub(crate) fn run_hourly_factor_materialize_news_intensity(
    context: &CommandContext,
    command: MaterializeNewsIntensityCommand,
) -> Result<(), CliError> {
    let resolved = resolve_common_options(context, &command.common)?;

    let input_path = required_input_file(
        context,
        "--materialization-input-json",
        &command.materialization_input_json,
    )?;

    let output_path = command
        .output_path
        .output_path
        .map(|path| context.paths().resolve_process_path(&path));

    let mut progress = TerminalCommandProgress::start(
        context,
        "hourly news-intensity materialization",
        2,
        output_path.as_deref(),
        &[
            "load calendar registry",
            "load materialization input",
            "materialize news intensity",
        ],
    )?;

    progress.observe("load calendar registry", 1)?;

    let calendar_registry = load_calendar_registry_artifact(&resolved.calendar_registry_path)?;

    progress.observe("load materialization input", 2)?;

    let input_document = load_json_artifact(&input_path, "hourly news-intensity input")?;

    let input_fields =
        object_document(&input_document, "hourly news-intensity input", &input_path)?;

    let known_timestamps = optional_timestamp_nanos_array_field(
        input_fields,
        "known_scheduled_event_timestamps_utc",
        "hourly news-intensity input",
        &input_path,
    )?;

    let surprise_baseline_intensity = f64_field(
        input_fields,
        "surprise_baseline_intensity",
        "hourly news-intensity input",
        &input_path,
    )?;

    let surprise_multipliers = optional_multiplier_entries(
        input_fields,
        "surprise_deterministic_time_multiplier_by_interval_index",
        "hourly news-intensity input",
        &input_path,
    )?;

    progress.observe("materialize news intensity", 3)?;

    let output =
        materialize_hourly_news_intensity_factors(HourlyNewsIntensityMaterializationInput {
            calendar_registry: &calendar_registry,
            exchange_code: &resolved.exchange_code,
            range_utc: resolved.range_start_utc..resolved.range_end_utc,
            interval_duration_nanos: resolved.interval_duration_nanos,
            known_scheduled_event_timestamps_utc_nanos: &known_timestamps,
            surprise_baseline_intensity,
            surprise_deterministic_time_multiplier_by_interval_index: &surprise_multipliers,
        })
        .map_err(|source| hourly_factor_argument_error("materialize news intensity", source))?;

    let payload = news_intensity_materialization_payload(&output);

    if let Some(output_path) = output_path.as_deref() {
        write_pretty_json_file(output_path, &payload, hourly_factor_artifact_write_error)?;
    }

    progress.complete()?;

    emit_hourly_output(
        context,
        &payload,
        output_path.as_deref(),
        "Hourly news intensity materialized",
        news_intensity_materialization_rows(&output, output_path.as_deref()),
    )
}

pub(crate) fn run_hourly_factor_build_contract(
    context: &CommandContext,
    command: BuildHourlyFactorContractCommand,
) -> Result<(), CliError> {
    let resolved = resolve_common_options(context, &command.common)?;

    let output_path = command
        .output_path
        .output_path
        .map(|path| context.paths().resolve_process_path(&path));

    let mut progress = TerminalCommandProgress::start(
        context,
        "hourly factor contract build",
        3,
        output_path.as_deref(),
        &[
            "load calendar registry",
            "load seasonality source",
            "load news-intensity source",
            "build contract",
        ],
    )?;

    progress.observe("load calendar registry", 1)?;

    let calendar_registry = load_calendar_registry_artifact(&resolved.calendar_registry_path)?;

    progress.observe("load seasonality source", 2)?;

    let (seasonality_factors, seasonality_method) = load_factor_source(
        context,
        command.seasonality_explicit_source_json.as_ref(),
        command.seasonality_materialized_source_json.as_ref(),
        SourceKind::Seasonality,
        &resolved,
    )?;

    progress.observe("load news-intensity source", 3)?;

    let (news_intensity_factors, news_intensity_method) = load_factor_source(
        context,
        command.news_intensity_explicit_source_json.as_ref(),
        command.news_intensity_materialized_source_json.as_ref(),
        SourceKind::NewsIntensity,
        &resolved,
    )?;

    progress.observe("build contract", 4)?;

    let output = build_hourly_factor_contract(HourlyFactorContractBuildInput {
        calendar_registry: &calendar_registry,
        exchange_code: &resolved.exchange_code,
        range_utc: resolved.range_start_utc..resolved.range_end_utc,
        interval_duration_nanos: resolved.interval_duration_nanos,
        seasonality_factors: &seasonality_factors,
        news_intensity_factors: &news_intensity_factors,
        seasonality_materialization_method: &seasonality_method,
        news_intensity_materialization_method: &news_intensity_method,
    })
    .map_err(|source| hourly_factor_argument_error("build contract", source))?;

    let payload = hourly_factor_contract_payload(&output);

    if let Some(output_path) = output_path.as_deref() {
        write_pretty_json_file(output_path, &payload, hourly_factor_artifact_write_error)?;
    }

    progress.complete()?;

    emit_hourly_output(
        context,
        &payload,
        output_path.as_deref(),
        "Hourly factor contract built",
        contract_rows(&output, output_path.as_deref()),
    )
}

fn hourly_factor_argument_error(
    command_name: &'static str,
    source: janiform::model::factors::hourly::HourlyFactorContractError,
) -> CliError {
    CliError::InvalidCommandArgument {
        argument_name: command_name,
        message: source.to_string(),
    }
}

fn hourly_factor_artifact_write_error(path: PathBuf, source: std::io::Error) -> CliError {
    CliError::ArtifactWrite {
        artifact_name: "hourly factor",
        path,
        source,
    }
}
