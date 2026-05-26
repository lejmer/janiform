use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;

use super::rendering::StylizedFactCheckDisplay;
use crate::commands::options::DuplicateRecordPolicy;
use crate::commands::options::InputFileFormat;
use crate::commands::options::TickPriceInputPolicy;

#[derive(Debug, Args)]
#[command(
    about = "Validate fitted models against empirical behavior.",
    long_about = "Validate fitted models against empirical behavior.

Validation commands compare fitted-model behavior with empirical data and write validation reports."
)]
pub(crate) struct ValidateCommand {
    #[command(subcommand)]
    pub(crate) command: ValidateSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ValidateSubcommand {
    #[command(
        name = "stylized-facts",
        about = "Validate stylized facts for a fitted model.",
        long_about = "Validate stylized facts for a fitted model.

The workflow compares empirical tick data with synthetic paths generated from a fitted Janiform model, computes return-based stylized-fact metrics, and writes a validation report.

Delta definitions:
  absolute delta is abs(empirical metric value - simulated metric value)
  standardized delta is absolute delta divided by the pooled confidence-interval half-width
  pooled half-width is sqrt(empirical_half_width^2 + simulated_half_width^2)

Standardized delta is a scale-normalized comparison against available confidence intervals. It is not a z-score and is not reported as a statistical significance test."
    )]
    StylizedFacts(ValidateStylizedFactsCommand),
}

#[derive(Debug, Args)]
#[command(
    about = "Validate stylized facts for a fitted model.",
    long_about = "Validate stylized facts for a fitted model.

When validating from tick data, provide --model-path and --intervals. If --empirical-data-path is omitted, Janiform uses the tick data recorded with the fitted model when available. Use --comparison-request for an advanced precomputed comparison file that skips empirical loading, simulation, and metric computation.

The output directory is populated with lineage.json and the validation report. --cache retains intermediate validation results, and --cache=full also writes generated seeded tick paths."
)]
pub(crate) struct ValidateStylizedFactsCommand {
    #[command(subcommand)]
    pub(crate) command: Option<ValidateStylizedFactsSubcommand>,

    #[command(flatten)]
    pub(crate) options: ValidateStylizedFactsOptions,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ValidateStylizedFactsSubcommand {
    #[command(
        about = "Inspect the stylized-facts validation computation plan.",
        long_about = "Inspect the stylized-facts validation computation plan.

The plan command resolves model, empirical data, intervals, output paths, cache policy, and metric catalog settings without running simulation or metric computation."
    )]
    Plan(ValidateStylizedFactsPlanCommand),
}

#[derive(Debug, Args)]
#[command(
    about = "Inspect the stylized-facts validation computation plan.",
    long_about = "Inspect the stylized-facts validation computation plan.

This command does not simulate paths, compute metrics, or write the final report. Use it to verify validation inputs and output paths before a long-running validation run."
)]
pub(crate) struct ValidateStylizedFactsPlanCommand {
    #[command(flatten)]
    pub(crate) options: ValidateStylizedFactsOptions,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ValidateStylizedFactsOptions {
    #[arg(long, value_name = "PATH", help = "Saved Janiform model path.")]
    pub(crate) model_path: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Empirical tick data input file for raw-data validation."
    )]
    pub(crate) empirical_data_path: Option<PathBuf>,

    #[arg(long, value_enum, help = "Empirical tick data file format.")]
    pub(crate) empirical_data_format: Option<InputFileFormat>,

    #[arg(
        long,
        value_name = "INTERVALS",
        value_delimiter = ',',
        help = "Comma-separated return intervals such as 5min,1h,1D."
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

    #[arg(
        long,
        value_name = "DIR",
        help = "Directory for validation outputs.",
        long_help = "Directory for validation outputs.

The validation workflow writes lineage.json and the report under this directory. --cache retains intermediate comparison inputs and metric observations. --cache=full also writes generated seeded tick paths."
    )]
    pub(crate) output_dir: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Explicit stylized-facts threshold policy JSON document.",
        long_help = "Explicit stylized-facts threshold policy JSON document.

Threshold policies can set maximum absolute delta and maximum standardized delta. Absolute delta is abs(empirical metric value - simulated metric value). Standardized delta is absolute delta divided by the pooled confidence-interval half-width."
    )]
    pub(crate) threshold_policy: Option<PathBuf>,

    #[arg(
        long,
        value_name = "VALUE",
        help = "Significance level for built-in bootstrap confidence intervals.",
        long_help = "Significance level for built-in bootstrap confidence intervals.

The confidence level is 1 minus this value. This option is only valid when Janiform generates the threshold policy. It does not make standardized delta a z-score or a statistical significance test."
    )]
    pub(crate) significance_level: Option<f64>,

    #[arg(
        long,
        value_enum,
        num_args = 0..=1,
        default_missing_value = "derived",
        help = "Cache intermediate validation results. Optional level: derived or full.",
        long_help = "Cache intermediate validation results.

Use --cache or --cache=derived to keep intermediate comparison data and metric observations. Use --cache=full to also keep generated seeded tick paths."
    )]
    pub(crate) cache: Option<StylizedFactsCacheLevel>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Precomputed stylized-facts comparison file.",
        long_help = "Precomputed stylized-facts comparison file.

When supplied, validation skips empirical loading, simulation, and metric computation, then runs only comparison and report generation."
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
        help = "Control terminal check detail."
    )]
    pub(crate) show_checks: StylizedFactCheckDisplay,

    #[arg(
        long,
        value_name = "N",
        default_value_t = 10,
        help = "Maximum number of checks to show in terminal output."
    )]
    pub(crate) max_checks: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum StylizedFactsCacheLevel {
    #[value(help = "Cache intermediate comparison data and metric observations.")]
    Derived,
    #[value(help = "Cache intermediate outputs and generated seeded tick paths.")]
    Full,
}
