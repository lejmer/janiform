use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;

use crate::commands::options::OutputPathOptions;

#[derive(Debug, Args)]
pub(crate) struct SeasonalityCommand {
    #[command(subcommand)]
    pub(crate) command: SeasonalitySubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SeasonalitySubcommand {
    #[command(
        name = "preprocess-timestamps",
        about = "Preprocess timestamp columns for seasonality workflows."
    )]
    PreprocessTimestamps(PreprocessTimestampsCommand),

    #[command(about = "Build seasonality diagnostics from timestamp data.")]
    Diagnostics(SeasonalityDiagnosticsCommand),
}

#[derive(Debug, Args)]
pub(crate) struct PreprocessTimestampsCommand {
    #[arg(
        long,
        value_name = "PATH",
        help = "Event-record JSON document with a top-level event_records list."
    )]
    pub(crate) event_records_json: Option<PathBuf>,

    #[arg(long, value_name = "PATH", help = "Calendar registry JSON document.")]
    pub(crate) calendar_registry_json: Option<PathBuf>,

    #[arg(long, value_name = "CODE", help = "Exchange code.")]
    pub(crate) exchange_code: Option<String>,

    #[arg(
        long,
        default_value = "timestamp",
        value_name = "NAME",
        help = "Timestamp field name inside each event record."
    )]
    pub(crate) timestamp_field_name: String,

    #[arg(
        long,
        default_value = "open_only",
        value_name = "FILTER",
        help = "Session filter: all, open_only, or closed_only."
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
pub(crate) struct SeasonalityDiagnosticsCommand {
    #[arg(
        long,
        value_name = "PATH",
        help = "Event-record JSON document with a top-level event_records list."
    )]
    pub(crate) event_records_json: Option<PathBuf>,

    #[arg(long, value_name = "PATH", help = "Calendar registry JSON document.")]
    pub(crate) calendar_registry_json: Option<PathBuf>,

    #[arg(long, value_name = "ID", help = "Stable dataset identifier.")]
    pub(crate) dataset_id: Option<String>,

    #[arg(long, value_name = "CODE", help = "Exchange code.")]
    pub(crate) exchange_code: Option<String>,

    #[arg(
        long,
        default_value = "timestamp",
        value_name = "NAME",
        help = "Timestamp field name inside each event record."
    )]
    pub(crate) timestamp_field_name: String,

    #[arg(
        long,
        default_value = "open_only",
        value_name = "FILTER",
        help = "Session filter: all, open_only, or closed_only."
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
    Gaussian,
    Uniform,
}
