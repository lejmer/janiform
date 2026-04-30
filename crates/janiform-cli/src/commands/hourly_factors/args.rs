use std::path::PathBuf;

use clap::ArgGroup;
use clap::Args;
use clap::Subcommand;

use crate::commands::options::OutputPathOptions;

#[derive(Debug, Args)]
#[command(
    about = "Prepare hourly seasonality and news-intensity factors.",
    long_about = "Prepare hourly seasonality and news-intensity factors.

These commands align seasonality and scheduled-news effects to the interval grid used by Janiform simulations. They read a calendar, exchange code, UTC range, interval, and factor inputs."
)]
pub(crate) struct HourlyFactorsCommand {
    #[command(subcommand)]
    pub(crate) command: HourlyFactorsSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum HourlyFactorsSubcommand {
    #[command(
        name = "materialize-seasonality",
        about = "Build hourly seasonality factors.",
        long_about = "Build hourly seasonality factors.

This command converts seasonality observations into interval-aligned s_h factors over the requested UTC range. It writes a factor report that can be passed to build-contract."
    )]
    MaterializeSeasonality(MaterializeSeasonalityCommand),

    #[command(
        name = "materialize-news-intensity",
        about = "Build hourly news-intensity factors.",
        long_about = "Build hourly news-intensity factors.

This command converts scheduled news timestamps and surprise-intensity settings into interval-aligned nu_h factors over the requested UTC range. It writes a factor report that can be passed to build-contract."
    )]
    MaterializeNewsIntensity(MaterializeNewsIntensityCommand),

    #[command(
        name = "build-contract",
        about = "Combine hourly factors for model use.",
        long_about = "Combine hourly factors for model use.

This command combines one seasonality input and one news-intensity input into the hourly factors used by downstream model workflows. Each input may be explicit JSON or a materialization report."
    )]
    BuildContract(BuildHourlyFactorContractCommand),
}

#[derive(Debug, Args)]
#[command(
    about = "Build hourly seasonality factors.",
    long_about = "Build hourly seasonality factors.

The output records interval-aligned s_h factors, calendar resolution, range coverage, and materialization diagnostics."
)]
pub(crate) struct MaterializeSeasonalityCommand {
    #[command(flatten)]
    pub(crate) common: HourlyFactorCommonOptions,

    #[arg(
        long,
        value_name = "PATH",
        help = "Hourly seasonality materialization input JSON document."
    )]
    pub(crate) materialization_input_json: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
#[command(
    about = "Build hourly news-intensity factors.",
    long_about = "Build hourly news-intensity factors.

The output records interval-aligned nu_h factors, calendar resolution, range coverage, and materialization diagnostics."
)]
pub(crate) struct MaterializeNewsIntensityCommand {
    #[command(flatten)]
    pub(crate) common: HourlyFactorCommonOptions,

    #[arg(
        long,
        value_name = "PATH",
        help = "Hourly news-intensity materialization input JSON document."
    )]
    pub(crate) materialization_input_json: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("seasonality_source")
        .args(["seasonality_explicit_source_json", "seasonality_materialized_source_json"])
        .required(true)
))]
#[command(group(
    ArgGroup::new("news_intensity_source")
        .args(["news_intensity_explicit_source_json", "news_intensity_materialized_source_json"])
        .required(true)
))]
#[command(
    about = "Combine hourly factors for model use.",
    long_about = "Combine hourly factors for model use.

Provide exactly one seasonality input and exactly one news-intensity input. The command writes combined hourly factors over the requested exchange, UTC range, and interval."
)]
pub(crate) struct BuildHourlyFactorContractCommand {
    #[command(flatten)]
    pub(crate) common: HourlyFactorCommonOptions,

    #[arg(long, value_name = "PATH", help = "Explicit seasonality JSON file.")]
    pub(crate) seasonality_explicit_source_json: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Materialized seasonality JSON file."
    )]
    pub(crate) seasonality_materialized_source_json: Option<PathBuf>,

    #[arg(long, value_name = "PATH", help = "Explicit news-intensity JSON file.")]
    pub(crate) news_intensity_explicit_source_json: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Materialized news-intensity JSON file."
    )]
    pub(crate) news_intensity_materialized_source_json: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct HourlyFactorCommonOptions {
    #[arg(long, value_name = "PATH", help = "Exchange calendar JSON file.")]
    pub(crate) calendar_registry_json: Option<PathBuf>,

    #[arg(long, value_name = "CODE", help = "Exchange code.")]
    pub(crate) exchange_code: Option<String>,

    #[arg(long, value_name = "UTC", help = "Inclusive UTC range start.")]
    pub(crate) range_start_utc: Option<String>,

    #[arg(long, value_name = "UTC", help = "Exclusive UTC range end.")]
    pub(crate) range_end_utc: Option<String>,

    #[arg(
        long,
        default_value = "1h",
        value_name = "DURATION",
        help = "Exact positive fixed-width UTC interval, for example 1h or 15min."
    )]
    pub(crate) interval: String,
}
