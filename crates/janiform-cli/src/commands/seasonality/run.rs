use std::path::{Path, PathBuf};

use janiform::calendar::{
    PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION, TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION,
    TimestampEventPreprocessingOutput, TimestampEventSessionFilter,
};
use janiform::seasonality::{
    DayOfWeekSeasonalityEstimatorSettings, DayOfYearSeasonalityEstimatorSettings,
    IntradaySeasonalityEstimatorSettings, IntradaySmoothingKernel,
    SeasonalityDiagnosticsReportInput, SeasonalityDiagnosticsReportOutput,
    compose_deterministic_seasonality_signal, estimate_day_of_week_seasonality,
    estimate_day_of_year_seasonality, estimate_intraday_seasonality,
    generate_seasonality_diagnostics_report,
};
use janiform_foundations::time::format_utc_nanos;
use janiform_terminal_ui::KeyValueRow;
use janiform_workflows::seasonality::{
    TimestampEventSource, TimestampEventSourceFormat, TimestampEventSourceLoadError,
    TimestampEventTimestampUnit, load_timestamp_event_records,
};
use serde_json::{Value, json};

use super::args::{
    IntradaySmoothingKernelOption, PreprocessTimestampsCommand, SeasonalityDiagnosticsCommand,
    TimestampEventRecordsFormatOption, TimestampEventTimestampUnitOption,
};
use crate::commands::shared::artifacts::load_calendar_registry_artifact;
use crate::commands::shared::inputs::{
    require_non_empty_string, required_input_file, required_non_empty_argument,
};
use crate::commands::shared::output::write_pretty_json_file;
use crate::commands::shared::progress::TerminalCommandProgress;
use crate::commands::shared::terminal::{
    append_output_path_row, emit_json_or_success_summary, output_path_rows,
};
use crate::context::CommandContext;
use crate::error::CliError;

pub(crate) fn run_seasonality_preprocess_timestamps(
    context: &CommandContext,
    command: PreprocessTimestampsCommand,
) -> Result<(), CliError> {
    let event_records_path =
        required_input_file(context, "--event-records", &command.event_records)?;

    let calendar_registry_path = required_input_file(
        context,
        "--calendar-registry-json",
        &command.calendar_registry_json,
    )?;

    let exchange_code = required_non_empty_argument("--exchange-code", command.exchange_code)?;

    let timestamp_column =
        require_non_empty_string("--timestamp-column", &command.timestamp_column)?;

    let session_filter = parse_session_filter(&command.session_filter)?;

    let event_source = timestamp_event_source(
        &event_records_path,
        command.event_records_format,
        timestamp_column,
        command.timestamp_unit,
    )?;

    let output_path = command
        .output_path
        .output_path
        .map(|path| context.paths().resolve_process_path(&path));

    let mut progress = TerminalCommandProgress::start(
        context,
        "seasonality timestamp preprocessing",
        2,
        output_path.as_deref(),
        &[
            "load timestamp events",
            "load calendar registry",
            "preprocess timestamps",
        ],
    )?;

    progress.observe("load timestamp events", 1)?;

    let event_records = load_timestamp_event_records(event_source)
        .map_err(timestamp_event_source_load_error)?
        .records;

    progress.observe("load calendar registry", 2)?;

    let calendar_registry = load_calendar_registry_artifact(&calendar_registry_path)?;

    progress.observe("preprocess timestamps", 3)?;

    let output = calendar_registry
        .preprocess_timestamp_events(
            &exchange_code,
            event_records,
            session_filter,
            command.sparse_hour_event_threshold,
        )
        .map_err(|source| CliError::InvalidCommandArgument {
            argument_name: "seasonality preprocess-timestamps",
            message: source.to_string(),
        })?;

    let payload = timestamp_preprocessing_payload(&output);

    if let Some(output_path) = output_path.as_deref() {
        write_pretty_json_file(output_path, &payload, seasonality_artifact_write_error)?;
    }

    progress.complete()?;

    emit_preprocess_output(context, &output, output_path.as_deref(), &payload)
}

pub(crate) fn run_seasonality_diagnostics(
    context: &CommandContext,
    command: SeasonalityDiagnosticsCommand,
) -> Result<(), CliError> {
    let event_records_path =
        required_input_file(context, "--event-records", &command.event_records)?;

    let calendar_registry_path = required_input_file(
        context,
        "--calendar-registry-json",
        &command.calendar_registry_json,
    )?;

    let dataset_id = required_non_empty_argument("--dataset-id", command.dataset_id)?;

    let exchange_code = required_non_empty_argument("--exchange-code", command.exchange_code)?;

    let timestamp_column =
        require_non_empty_string("--timestamp-column", &command.timestamp_column)?;

    let estimator_version =
        require_non_empty_string("--estimator-version", &command.estimator_version)?;

    let session_filter = parse_session_filter(&command.session_filter)?;

    let event_source = timestamp_event_source(
        &event_records_path,
        command.event_records_format,
        timestamp_column,
        command.timestamp_unit,
    )?;

    let output_path = command
        .output_path
        .output_path
        .map(|path| context.paths().resolve_process_path(&path));

    let mut progress = TerminalCommandProgress::start(
        context,
        "seasonality diagnostics",
        2,
        output_path.as_deref(),
        &[
            "load timestamp events",
            "load calendar registry",
            "preprocess timestamps",
            "estimate intraday",
            "estimate day of week",
            "estimate day of year",
            "compose deterministic signal",
            "generate report",
        ],
    )?;

    progress.observe("load timestamp events", 1)?;

    let event_records = load_timestamp_event_records(event_source)
        .map_err(timestamp_event_source_load_error)?
        .records;

    progress.observe("load calendar registry", 2)?;

    let calendar_registry = load_calendar_registry_artifact(&calendar_registry_path)?;

    progress.observe("preprocess timestamps", 3)?;

    let preprocessed = calendar_registry
        .preprocess_timestamp_events(
            &exchange_code,
            event_records,
            session_filter,
            command.sparse_hour_event_threshold,
        )
        .map_err(|source| CliError::InvalidCommandArgument {
            argument_name: "seasonality diagnostics",
            message: source.to_string(),
        })?;

    progress.observe("estimate intraday", 4)?;

    let intraday_estimate = estimate_intraday_seasonality(
        &preprocessed.exchange_local_minute_of_day,
        IntradaySeasonalityEstimatorSettings {
            minutes_per_bin: command.intraday_minutes_per_bin,
            smoothing_kernel: match command.intraday_smoothing_kernel {
                IntradaySmoothingKernelOption::Gaussian => IntradaySmoothingKernel::Gaussian,
                IntradaySmoothingKernelOption::Uniform => IntradaySmoothingKernel::Uniform,
            },
            smoothing_bandwidth_bins: command.intraday_smoothing_bandwidth_bins,
            minimum_bin_events: command.intraday_minimum_bin_events,
            pseudocount: command.intraday_pseudocount,
        },
    )
    .map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "seasonality diagnostics",
        message: source.to_string(),
    })?;

    progress.observe("estimate day of week", 5)?;

    let day_of_week_estimate = estimate_day_of_week_seasonality(
        &preprocessed.exchange_local_day_of_week,
        DayOfWeekSeasonalityEstimatorSettings {
            minimum_bucket_events: command.day_of_week_minimum_bucket_events,
            pseudocount: command.day_of_week_pseudocount,
        },
    )
    .map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "seasonality diagnostics",
        message: source.to_string(),
    })?;

    progress.observe("estimate day of year", 6)?;

    let day_of_year_estimate = estimate_day_of_year_seasonality(
        &preprocessed.exchange_local_day_of_year,
        DayOfYearSeasonalityEstimatorSettings {
            minimum_bucket_events: command.day_of_year_minimum_bucket_events,
            pseudocount: command.day_of_year_pseudocount,
        },
    )
    .map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "seasonality diagnostics",
        message: source.to_string(),
    })?;

    progress.observe("compose deterministic signal", 7)?;

    let composition = compose_deterministic_seasonality_signal(
        &preprocessed.timestamp_utc_nanos,
        &preprocessed.exchange_local_minute_of_day,
        &preprocessed.exchange_local_day_of_week,
        &preprocessed.exchange_local_day_of_year,
        intraday_estimate.minutes_per_bin,
        &intraday_estimate.intraday_log_component_by_bin,
        &day_of_week_estimate.day_of_week_log_component,
        &day_of_year_estimate.day_of_year_log_component,
        &[],
    )
    .map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "seasonality diagnostics",
        message: source.to_string(),
    })?;

    progress.observe("generate report", 8)?;

    let report = generate_seasonality_diagnostics_report(SeasonalityDiagnosticsReportInput {
        dataset_id: &dataset_id,
        input_schema_version: TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION,
        preprocessed_schema_version: PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION,
        exchange_code: &preprocessed.exchange_code,
        exchange_timezone_name: &preprocessed.exchange_timezone_name,
        source_record_positions: &preprocessed.source_record_positions,
        timestamp_utc_nanos: &preprocessed.timestamp_utc_nanos,
        exchange_local_minute_of_day: &preprocessed.exchange_local_minute_of_day,
        exchange_local_day_of_week: &preprocessed.exchange_local_day_of_week,
        exchange_local_day_of_year: &preprocessed.exchange_local_day_of_year,
        market_state_codes: &preprocessed.market_state_codes,
        intraday_estimate: &intraday_estimate,
        day_of_week_estimate: &day_of_week_estimate,
        day_of_year_estimate: &day_of_year_estimate,
        composition: &composition,
        estimator_version,
    })
    .map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "seasonality diagnostics",
        message: source.to_string(),
    })?;

    if let Some(output_path) = output_path.as_deref() {
        write_pretty_json_file(output_path, &report, seasonality_artifact_write_error)?;
    }

    progress.complete()?;

    emit_diagnostics_output(context, &report, &preprocessed, output_path.as_deref())
}

fn timestamp_preprocessing_payload(output: &TimestampEventPreprocessingOutput) -> Value {
    json!({
        "schema_version": output.schema_version,
        "input_schema_version": output.input_schema_version,
        "exchange_code": output.exchange_code,
        "exchange_timezone_name": output.exchange_timezone_name,
        "source_record_positions": output.source_record_positions,
        "timestamp_utc_nanos": output.timestamp_utc_nanos,
        "exchange_local_minute_of_day": output.exchange_local_minute_of_day,
        "exchange_local_day_of_week": output.exchange_local_day_of_week,
        "exchange_local_day_of_year": output.exchange_local_day_of_year,
        "market_state_codes": output.market_state_codes,
        "missing_hour_bucket_utc_nanos": output.missing_hour_bucket_utc_nanos,
        "sparse_hour_bucket_utc_nanos": output.sparse_hour_bucket_utc_nanos,
        "input_record_count": output.diagnostics.input_record_count,
        "validated_record_count": output.diagnostics.validated_record_count,
        "accepted_record_count": output.diagnostics.accepted_record_count,
        "filtered_record_count": output.diagnostics.filtered_record_count,
        "first_event_timestamp_utc_nanos": output.diagnostics.first_event_timestamp_utc_nanos,
        "last_event_timestamp_utc_nanos": output.diagnostics.last_event_timestamp_utc_nanos,
        "hour_bucket_count": output.diagnostics.hour_bucket_count,
        "missing_hour_bucket_count": output.diagnostics.missing_hour_bucket_count,
        "sparse_hour_bucket_count": output.diagnostics.sparse_hour_bucket_count,
    })
}

fn emit_preprocess_output(
    context: &CommandContext,
    output: &TimestampEventPreprocessingOutput,
    output_path: Option<&Path>,
    payload: &Value,
) -> Result<(), CliError> {
    emit_json_or_success_summary(
        context,
        payload,
        "Seasonality timestamps preprocessed",
        preprocessing_rows(output, output_path),
        output_path_rows(output_path),
    )
}

fn emit_diagnostics_output(
    context: &CommandContext,
    report: &SeasonalityDiagnosticsReportOutput,
    preprocessed: &TimestampEventPreprocessingOutput,
    output_path: Option<&Path>,
) -> Result<(), CliError> {
    emit_json_or_success_summary(
        context,
        report,
        "Seasonality diagnostics generated",
        diagnostics_rows(report, preprocessed, output_path),
        output_path_rows(output_path),
    )
}

fn preprocessing_rows(
    output: &TimestampEventPreprocessingOutput,
    output_path: Option<&Path>,
) -> Vec<KeyValueRow> {
    let mut rows = vec![
        KeyValueRow::new("Exchange", output.exchange_code.clone()),
        KeyValueRow::new("Timezone", output.exchange_timezone_name.clone()),
        KeyValueRow::new(
            "Accepted records",
            output.diagnostics.accepted_record_count.to_string(),
        ),
        KeyValueRow::new(
            "Filtered records",
            output.diagnostics.filtered_record_count.to_string(),
        ),
        KeyValueRow::new(
            "Coverage hours",
            output.diagnostics.hour_bucket_count.to_string(),
        ),
        KeyValueRow::new(
            "Missing hours",
            output.diagnostics.missing_hour_bucket_count.to_string(),
        ),
        KeyValueRow::new(
            "Sparse hours",
            output.diagnostics.sparse_hour_bucket_count.to_string(),
        ),
        KeyValueRow::new(
            "Accepted UTC range",
            accepted_utc_range(
                output.diagnostics.first_event_timestamp_utc_nanos,
                output.diagnostics.last_event_timestamp_utc_nanos,
            ),
        ),
    ];

    append_output_path_row(&mut rows, output_path);

    rows
}

fn diagnostics_rows(
    report: &SeasonalityDiagnosticsReportOutput,
    preprocessed: &TimestampEventPreprocessingOutput,
    output_path: Option<&Path>,
) -> Vec<KeyValueRow> {
    let overlay_event_count = report
        .overlay_component_summary
        .as_ref()
        .map(|summary| summary.active_overlay_event_count)
        .unwrap_or(0);

    let reproducibility = &report.reproducibility_metadata;
    let snapshot = &reproducibility.input_snapshot;

    let mut rows = vec![
        KeyValueRow::new("Dataset", report.dataset_id.clone()),
        KeyValueRow::new(
            "Accepted records",
            preprocessed.diagnostics.accepted_record_count.to_string(),
        ),
        KeyValueRow::new(
            "Filtered records",
            preprocessed.diagnostics.filtered_record_count.to_string(),
        ),
        KeyValueRow::new(
            "Fit quality",
            format!(
                "mae={:.6}, rmse={:.6}",
                report
                    .fit_quality_metrics
                    .weighted_mean_absolute_factor_error,
                report
                    .fit_quality_metrics
                    .weighted_root_mean_squared_factor_error
            ),
        ),
        KeyValueRow::new(
            "Normalization anchor error",
            format!(
                "{:.6}",
                report.fit_quality_metrics.normalization_anchor_error
            ),
        ),
        KeyValueRow::new(
            "Sparse buckets",
            format!(
                "intraday={}, day_of_week={}, day_of_year={}",
                report.intraday_component_summary.sparse_bucket_count,
                report.day_of_week_component_summary.sparse_bucket_count,
                report.day_of_year_component_summary.sparse_bucket_count
            ),
        ),
        KeyValueRow::new("Active overlays", overlay_event_count.to_string()),
        KeyValueRow::new("Estimator", reproducibility.estimator_version.clone()),
        KeyValueRow::new("Fingerprint", snapshot.snapshot_fingerprint_hex.clone()),
    ];

    append_output_path_row(&mut rows, output_path);

    rows
}

fn accepted_utc_range(first_nanos: Option<i64>, last_nanos: Option<i64>) -> String {
    match (first_nanos, last_nanos) {
        (Some(first_nanos), Some(last_nanos)) => {
            format!(
                "{} to {}",
                format_utc_nanos(first_nanos),
                format_utc_nanos(last_nanos)
            )
        }
        _ => "none".to_string(),
    }
}

fn timestamp_event_source(
    path: &Path,
    explicit_format: Option<TimestampEventRecordsFormatOption>,
    timestamp_column: &str,
    timestamp_unit: TimestampEventTimestampUnitOption,
) -> Result<TimestampEventSource, CliError> {
    Ok(TimestampEventSource {
        path: path.to_path_buf(),
        file_format: timestamp_event_source_format(path, explicit_format)?,
        timestamp_column: timestamp_column.to_string(),
        timestamp_unit: timestamp_event_timestamp_unit(timestamp_unit),
    })
}

fn timestamp_event_source_format(
    path: &Path,
    explicit_format: Option<TimestampEventRecordsFormatOption>,
) -> Result<TimestampEventSourceFormat, CliError> {
    match explicit_format {
        Some(TimestampEventRecordsFormatOption::Csv) => Ok(TimestampEventSourceFormat::Csv),
        Some(TimestampEventRecordsFormatOption::Parquet) => Ok(TimestampEventSourceFormat::Parquet),
        Some(TimestampEventRecordsFormatOption::Jsonl) => Ok(TimestampEventSourceFormat::Jsonl),
        None => infer_timestamp_event_source_format(path),
    }
}

fn infer_timestamp_event_source_format(
    path: &Path,
) -> Result<TimestampEventSourceFormat, CliError> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("csv") => {
            Ok(TimestampEventSourceFormat::Csv)
        }
        Some(extension) if extension.eq_ignore_ascii_case("parquet") => {
            Ok(TimestampEventSourceFormat::Parquet)
        }
        Some(extension) if extension.eq_ignore_ascii_case("jsonl") => {
            Ok(TimestampEventSourceFormat::Jsonl)
        }
        Some(extension) if extension.eq_ignore_ascii_case("json") => {
            Err(CliError::InvalidCommandArgument {
                argument_name: "--event-records-format",
                message:
                    "JSON document event sources are not supported; use jsonl, csv, or parquet"
                        .to_string(),
            })
        }
        _ => Err(CliError::InvalidCommandArgument {
            argument_name: "--event-records-format",
            message: "file format could not be inferred from the extension".to_string(),
        }),
    }
}

fn timestamp_event_timestamp_unit(
    timestamp_unit: TimestampEventTimestampUnitOption,
) -> TimestampEventTimestampUnit {
    match timestamp_unit {
        TimestampEventTimestampUnitOption::Rfc3339 => TimestampEventTimestampUnit::Rfc3339,
        TimestampEventTimestampUnitOption::Seconds => TimestampEventTimestampUnit::Seconds,
        TimestampEventTimestampUnitOption::Milliseconds => {
            TimestampEventTimestampUnit::Milliseconds
        }
        TimestampEventTimestampUnitOption::Microseconds => {
            TimestampEventTimestampUnit::Microseconds
        }
        TimestampEventTimestampUnitOption::Nanoseconds => TimestampEventTimestampUnit::Nanoseconds,
    }
}

fn timestamp_event_source_load_error(source: TimestampEventSourceLoadError) -> CliError {
    CliError::InvalidCommandArgument {
        argument_name: "--event-records",
        message: source.to_string(),
    }
}

fn parse_session_filter(value: &str) -> Result<TimestampEventSessionFilter, CliError> {
    match value {
        "all" => Ok(TimestampEventSessionFilter::All),
        "open_only" | "open-only" => Ok(TimestampEventSessionFilter::OpenOnly),
        "closed_only" | "closed-only" => Ok(TimestampEventSessionFilter::ClosedOnly),
        _ => Err(CliError::InvalidCommandArgument {
            argument_name: "--session-filter",
            message: "must be one of all, open_only, or closed_only".to_string(),
        }),
    }
}

fn seasonality_artifact_write_error(path: PathBuf, source: std::io::Error) -> CliError {
    CliError::ArtifactWrite {
        artifact_name: "seasonality",
        path,
        source,
    }
}
