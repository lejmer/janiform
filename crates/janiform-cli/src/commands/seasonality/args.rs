use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;

use crate::commands::options::OutputPathOptions;

#[derive(Debug, Args)]
#[command(
    about = "Prepare and diagnose seasonality inputs.",
    long_about = "Prepare and diagnose seasonality inputs.

Seasonality commands read timestamp-event data and an exchange calendar, then produce preprocessing diagnostics or a full seasonality diagnostics report. These commands are useful before fitting and for auditing calendar-aligned event density."
)]
pub(crate) struct SeasonalityCommand {
    #[command(subcommand)]
    pub(crate) command: SeasonalitySubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SeasonalitySubcommand {
    #[command(
        name = "preprocess-timestamps",
        about = "Preprocess timestamp data for seasonality workflows.",
        long_about = "Preprocess timestamp data for seasonality workflows.

This command loads CSV, Parquet, or JSONL timestamp events, resolves market sessions from the exchange calendar, applies the requested session filter, and writes preprocessed timestamp diagnostics."
    )]
    PreprocessTimestamps(PreprocessTimestampsCommand),

    #[command(
        about = "Build seasonality diagnostics from timestamp data.",
        long_about = "Build seasonality diagnostics from timestamp data.

This command runs timestamp preprocessing, intraday estimation, day-of-week estimation, day-of-year estimation, deterministic signal composition, and diagnostics report generation."
    )]
    Diagnostics(SeasonalityDiagnosticsCommand),
}

#[derive(Debug, Args)]
#[command(
    about = "Preprocess timestamp data for seasonality workflows.",
    long_about = "Preprocess timestamp data for seasonality workflows.

The output records parsed timestamps, market-session classification, sparse-hour diagnostics, and reproducibility metadata for downstream seasonality or calibration workflows."
)]
pub(crate) struct PreprocessTimestampsCommand {
    #[arg(long, value_name = "PATH", help = "Timestamp-event source file.")]
    pub(crate) event_records: Option<PathBuf>,

    #[arg(
        long,
        value_enum,
        value_name = "FORMAT",
        help = "Timestamp-event source format."
    )]
    pub(crate) event_records_format: Option<TimestampEventRecordsFormatOption>,

    #[arg(long, value_name = "PATH", help = "Exchange calendar JSON file.")]
    pub(crate) calendar_registry_json: Option<PathBuf>,

    #[arg(long, value_name = "CODE", help = "Exchange code.")]
    pub(crate) exchange_code: Option<String>,

    #[arg(
        long,
        default_value = "timestamp",
        value_name = "NAME",
        help = "Timestamp column or JSONL field name inside the timestamp-event source."
    )]
    pub(crate) timestamp_column: String,

    #[arg(
        long,
        value_enum,
        default_value_t = TimestampEventTimestampUnitOption::Rfc3339,
        help = "Timestamp value unit."
    )]
    pub(crate) timestamp_unit: TimestampEventTimestampUnitOption,

    #[arg(
        long,
        default_value = "open_only",
        value_name = "FILTER",
        help = "Session filter. Use all, open_only, or closed_only."
    )]
    pub(crate) session_filter: String,

    #[arg(
        long,
        default_value_t = 1,
        value_name = "COUNT",
        help = "Minimum accepted events per hour before a bucket is flagged as sparse."
    )]
    pub(crate) sparse_hour_event_threshold: usize,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
#[command(
    about = "Build seasonality diagnostics from timestamp data.",
    long_about = "Build seasonality diagnostics from timestamp data.

The output includes preprocessing diagnostics, estimated intraday factors, day-of-week factors, day-of-year factors, deterministic signal composition, quality diagnostics, and reproducibility metadata."
)]
pub(crate) struct SeasonalityDiagnosticsCommand {
    #[arg(long, value_name = "PATH", help = "Timestamp-event source file.")]
    pub(crate) event_records: Option<PathBuf>,

    #[arg(
        long,
        value_enum,
        value_name = "FORMAT",
        help = "Timestamp-event source format."
    )]
    pub(crate) event_records_format: Option<TimestampEventRecordsFormatOption>,

    #[arg(long, value_name = "PATH", help = "Exchange calendar JSON file.")]
    pub(crate) calendar_registry_json: Option<PathBuf>,

    #[arg(long, value_name = "ID", help = "Stable dataset identifier.")]
    pub(crate) dataset_id: Option<String>,

    #[arg(long, value_name = "CODE", help = "Exchange code.")]
    pub(crate) exchange_code: Option<String>,

    #[arg(
        long,
        default_value = "timestamp",
        value_name = "NAME",
        help = "Timestamp column or JSONL field name inside the timestamp-event source."
    )]
    pub(crate) timestamp_column: String,

    #[arg(
        long,
        value_enum,
        default_value_t = TimestampEventTimestampUnitOption::Rfc3339,
        help = "Timestamp value unit."
    )]
    pub(crate) timestamp_unit: TimestampEventTimestampUnitOption,

    #[arg(
        long,
        default_value = "open_only",
        value_name = "FILTER",
        help = "Session filter. Use all, open_only, or closed_only."
    )]
    pub(crate) session_filter: String,

    #[arg(
        long,
        default_value_t = 1,
        value_name = "COUNT",
        help = "Minimum accepted events per hour before a bucket is flagged as sparse."
    )]
    pub(crate) sparse_hour_event_threshold: usize,

    #[arg(
        long,
        default_value_t = 1,
        value_name = "MINUTES",
        help = "Intraday estimator bin size in minutes."
    )]
    pub(crate) intraday_minutes_per_bin: u16,

    #[arg(
        long,
        value_enum,
        default_value_t = IntradaySmoothingKernelOption::Gaussian,
        help = "Intraday smoothing kernel."
    )]
    pub(crate) intraday_smoothing_kernel: IntradaySmoothingKernelOption,

    #[arg(
        long,
        default_value_t = 3.0,
        value_name = "BINS",
        help = "Intraday smoothing bandwidth measured in bins."
    )]
    pub(crate) intraday_smoothing_bandwidth_bins: f64,

    #[arg(
        long,
        default_value_t = 1,
        value_name = "COUNT",
        help = "Minimum events per intraday bin before the bin is flagged as sparse."
    )]
    pub(crate) intraday_minimum_bin_events: usize,

    #[arg(
        long,
        default_value_t = 1e-6,
        value_name = "VALUE",
        help = "Intraday estimator pseudocount added before normalization."
    )]
    pub(crate) intraday_pseudocount: f64,

    #[arg(
        long,
        default_value_t = 1,
        value_name = "COUNT",
        help = "Minimum events per day-of-week bucket before the bucket is flagged as sparse."
    )]
    pub(crate) day_of_week_minimum_bucket_events: usize,

    #[arg(
        long,
        default_value_t = 1e-6,
        value_name = "VALUE",
        help = "Day-of-week estimator pseudocount added before normalization."
    )]
    pub(crate) day_of_week_pseudocount: f64,

    #[arg(
        long,
        default_value_t = 1,
        value_name = "COUNT",
        help = "Minimum events per day-of-year bucket before the bucket is flagged as sparse."
    )]
    pub(crate) day_of_year_minimum_bucket_events: usize,

    #[arg(
        long,
        default_value_t = 1e-6,
        value_name = "VALUE",
        help = "Day-of-year estimator pseudocount added before normalization."
    )]
    pub(crate) day_of_year_pseudocount: f64,

    #[arg(
        long,
        default_value = "seasonality_estimator.v1",
        value_name = "VERSION",
        help = "Estimator version label recorded in reproducibility metadata."
    )]
    pub(crate) estimator_version: String,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum IntradaySmoothingKernelOption {
    #[value(help = "Use Gaussian smoothing across intraday bins.")]
    Gaussian,
    #[value(help = "Use uniform smoothing across intraday bins.")]
    Uniform,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum TimestampEventRecordsFormatOption {
    #[value(help = "Read comma-separated values.")]
    Csv,
    #[value(help = "Read Apache Parquet.")]
    Parquet,
    #[value(help = "Read newline-delimited JSON records.")]
    Jsonl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum TimestampEventTimestampUnitOption {
    #[value(help = "Parse RFC 3339 timestamps.")]
    Rfc3339,
    #[value(help = "Parse Unix timestamps in seconds.")]
    Seconds,
    #[value(help = "Parse Unix timestamps in milliseconds.")]
    Milliseconds,
    #[value(help = "Parse Unix timestamps in microseconds.")]
    Microseconds,
    #[value(help = "Parse Unix timestamps in nanoseconds.")]
    Nanoseconds,
}
