use std::path::PathBuf;

use clap::ArgGroup;
use clap::Args;
use clap::Subcommand;

use crate::commands::options::OutputPathOptions;

#[derive(Debug, Args)]
pub(crate) struct HourlyFactorsCommand {
    #[command(subcommand)]
    pub(crate) command: HourlyFactorsSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum HourlyFactorsSubcommand {
    #[command(
        name = "materialize-seasonality",
        about = "Materialize hourly seasonality factors."
    )]
    MaterializeSeasonality(MaterializeSeasonalityCommand),

    #[command(
        name = "materialize-news-intensity",
        about = "Materialize hourly news-intensity factors."
    )]
    MaterializeNewsIntensity(MaterializeNewsIntensityCommand),

    #[command(
        name = "build-contract",
        about = "Build an hourly-factor contract document."
    )]
    BuildContract(BuildHourlyFactorContractCommand),
}

#[derive(Debug, Args)]
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
pub(crate) struct BuildHourlyFactorContractCommand {
    #[command(flatten)]
    pub(crate) common: HourlyFactorCommonOptions,

    #[arg(
        long,
        value_name = "PATH",
        help = "Explicit seasonality source JSON document."
    )]
    pub(crate) seasonality_explicit_source_json: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Materialized seasonality source JSON document."
    )]
    pub(crate) seasonality_materialized_source_json: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Explicit news-intensity source JSON document."
    )]
    pub(crate) news_intensity_explicit_source_json: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Materialized news-intensity source JSON document."
    )]
    pub(crate) news_intensity_materialized_source_json: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct HourlyFactorCommonOptions {
    #[arg(long, value_name = "PATH", help = "Calendar registry JSON document.")]
    pub(crate) calendar_registry_json: Option<PathBuf>,

    #[arg(long, value_name = "CODE", help = "Exchange code.")]
    pub(crate) exchange_code: Option<String>,

    #[arg(long, value_name = "TIMESTAMP", help = "Inclusive UTC range start.")]
    pub(crate) range_start_utc: Option<String>,

    #[arg(long, value_name = "TIMESTAMP", help = "Exclusive UTC range end.")]
    pub(crate) range_end_utc: Option<String>,

    #[arg(
        long,
        default_value = "1h",
        value_name = "DURATION",
        help = "Exact positive fixed-width UTC interval, for example 1h or 15min."
    )]
    pub(crate) interval: String,
}
