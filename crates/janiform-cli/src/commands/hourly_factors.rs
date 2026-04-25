use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::CliBoundaryOptions;
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
    pub(crate) boundary: CliBoundaryOptions,

    #[command(flatten)]
    pub(crate) inputs: HourlyFactorInputOptions,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct MaterializeNewsIntensityCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[command(flatten)]
    pub(crate) inputs: HourlyFactorInputOptions,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct BuildHourlyFactorContractCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[command(flatten)]
    pub(crate) inputs: HourlyFactorInputOptions,

    #[arg(long, value_name = "PATH", help = "Hourly seasonality factor path.")]
    pub(crate) seasonality_factor_path: Option<PathBuf>,

    #[arg(long, value_name = "PATH", help = "Hourly news-intensity factor path.")]
    pub(crate) news_intensity_factor_path: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct HourlyFactorInputOptions {
    #[arg(long, value_name = "PATH", help = "Input data path.")]
    pub(crate) input_path: Option<PathBuf>,

    #[arg(long, value_name = "NAME", help = "Timestamp column name.")]
    pub(crate) timestamp_column: Option<String>,

    #[arg(long, value_name = "NAME", help = "Value column name.")]
    pub(crate) value_column: Option<String>,
}
