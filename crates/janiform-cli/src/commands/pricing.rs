use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::CliBoundaryOptions;
use crate::commands::options::ModelPathOptions;
use crate::commands::options::OutputPathOptions;

#[derive(Debug, Args)]
pub(crate) struct PriceCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[command(flatten)]
    pub(crate) model: ModelPathOptions,

    #[command(flatten)]
    pub(crate) option: VanillaOptionPricingOptions,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,

    #[command(subcommand)]
    pub(crate) command: Option<PriceSubcommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PriceSubcommand {
    #[command(
        name = "fitted-model",
        about = "Price explicit vanilla contracts from a fitted model."
    )]
    FittedModel(FittedModelPricingCommand),

    #[command(
        name = "observed-options",
        about = "Price an observed option snapshot from a fitted model."
    )]
    ObservedOptions(ObservedOptionPricingCommand),
}

#[derive(Debug, Args)]
pub(crate) struct FittedModelPricingCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[command(flatten)]
    pub(crate) model: ModelPathOptions,

    #[command(flatten)]
    pub(crate) option: VanillaOptionPricingOptions,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct ObservedOptionPricingCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[command(flatten)]
    pub(crate) model: ModelPathOptions,

    #[arg(
        long,
        value_name = "PATH",
        help = "Observed option snapshot input path."
    )]
    pub(crate) option_surface_path: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct VanillaOptionPricingOptions {
    #[arg(long, value_name = "UTC", help = "Pricing valuation timestamp.")]
    pub(crate) valuation_time_utc: Option<String>,

    #[arg(long, value_name = "PRICE", help = "Current spot price.")]
    pub(crate) spot: Option<f64>,

    #[arg(long, value_name = "PRICE", help = "Vanilla option strike.")]
    pub(crate) strike: Option<f64>,

    #[arg(long, value_name = "UTC", help = "Vanilla option maturity timestamp.")]
    pub(crate) maturity_time_utc: Option<String>,
}
