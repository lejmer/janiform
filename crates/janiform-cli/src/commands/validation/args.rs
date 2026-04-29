use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;

use super::rendering::StylizedFactCheckDisplay;
use crate::commands::options::DuplicateRecordPolicy;
use crate::commands::options::InputFileFormat;
use crate::commands::options::TickPriceInputPolicy;

#[derive(Debug, Args)]
pub(crate) struct ValidateCommand {
    #[command(subcommand)]
    pub(crate) command: ValidateSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ValidateSubcommand {
    #[command(
        name = "stylized-facts",
        about = "Validate stylized facts for a fitted model."
    )]
    StylizedFacts(ValidateStylizedFactsCommand),
}

#[derive(Debug, Args)]
pub(crate) struct ValidateStylizedFactsCommand {
    #[command(subcommand)]
    pub(crate) command: Option<ValidateStylizedFactsSubcommand>,

    #[command(flatten)]
    pub(crate) options: ValidateStylizedFactsOptions,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ValidateStylizedFactsSubcommand {
    #[command(about = "Inspect the stylized-facts validation computation plan.")]
    Plan(ValidateStylizedFactsPlanCommand),
}

#[derive(Debug, Args)]
pub(crate) struct ValidateStylizedFactsPlanCommand {
    #[command(flatten)]
    pub(crate) options: ValidateStylizedFactsOptions,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ValidateStylizedFactsOptions {
    #[arg(long, value_name = "PATH", help = "Persisted Janiform model path.")]
    pub(crate) model_path: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Empirical tick data input path for raw-data validation."
    )]
    pub(crate) empirical_data_path: Option<PathBuf>,

    #[arg(long, value_enum, help = "Empirical tick data file format.")]
    pub(crate) empirical_data_format: Option<InputFileFormat>,

    #[arg(
        long,
        value_name = "INTERVALS",
        value_delimiter = ',',
        help = "Comma-separated return intervals such as `5m,1h,1d`."
    )]
    pub(crate) intervals: Vec<String>,

    #[arg(
        long,
        value_name = "COUNT",
        default_value_t = 1,
        help = "Number of seeded simulation paths to generate."
    )]
    pub(crate) simulation_path_count: usize,

    #[arg(
        long,
        value_name = "SEED",
        default_value_t = 0,
        help = "Base simulation random seed."
    )]
    pub(crate) seed: u64,

    #[arg(
        long,
        value_name = "COUNT",
        help = "CPU count available to validation metric computation."
    )]
    pub(crate) cpu_count: Option<usize>,

    #[arg(long, value_name = "DIR", help = "Directory for validation artifacts.")]
    pub(crate) output_dir: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Explicit stylized-facts threshold policy JSON document."
    )]
    pub(crate) threshold_policy: Option<PathBuf>,

    #[arg(
        long,
        value_name = "VALUE",
        help = "Statistical significance level used when no threshold policy is supplied."
    )]
    pub(crate) significance_level: Option<f64>,

    #[arg(
        long,
        value_enum,
        num_args = 0..=1,
        default_missing_value = "derived",
        help = "Cache derived validation work products. Optional level: derived or full."
    )]
    pub(crate) cache: Option<StylizedFactsCacheLevel>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Precomputed stylized-facts comparison request JSON object."
    )]
    pub(crate) comparison_request: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Stylized-fact metric catalog JSON document. Defaults to the built-in catalog."
    )]
    pub(crate) metric_catalog: Option<PathBuf>,

    #[arg(long, value_name = "COLUMN", help = "Empirical tick timestamp column.")]
    pub(crate) empirical_timestamp_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Empirical tick price column.")]
    pub(crate) empirical_price_column: Option<String>,

    #[arg(
        long,
        value_name = "COLUMN",
        help = "Empirical tick trade sign column."
    )]
    pub(crate) empirical_trade_sign_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Empirical tick bid column.")]
    pub(crate) empirical_bid_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Empirical tick ask column.")]
    pub(crate) empirical_ask_column: Option<String>,

    #[arg(long, value_enum, help = "Empirical tick price normalization policy.")]
    pub(crate) empirical_price_input_policy: Option<TickPriceInputPolicy>,

    #[arg(
        long,
        value_name = "TZ",
        help = "Timezone for naive empirical timestamps."
    )]
    pub(crate) empirical_timestamp_timezone: Option<String>,

    #[arg(long, value_enum, help = "Duplicate empirical tick record policy.")]
    pub(crate) empirical_duplicate_record_policy: Option<DuplicateRecordPolicy>,

    #[arg(
        long,
        help = "Keep empirical source order instead of sorting by timestamp."
    )]
    pub(crate) keep_empirical_source_order: bool,

    #[arg(
        long,
        help = "Return a nonzero process exit when any metric check fails."
    )]
    pub(crate) fail_on_failed_checks: bool,

    #[arg(
        long,
        value_enum,
        default_value = "summary",
        help = "Controls human check detail."
    )]
    pub(crate) show_checks: StylizedFactCheckDisplay,

    #[arg(
        long,
        value_name = "N",
        default_value_t = 10,
        help = "Maximum number of checks to show in human output."
    )]
    pub(crate) max_checks: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum StylizedFactsCacheLevel {
    Derived,
    Full,
}
