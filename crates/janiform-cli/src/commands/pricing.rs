use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;

use crate::commands::options::ModelPathOptions;
use crate::commands::options::OutputPathOptions;

mod execution;
mod rendering;

pub(crate) use execution::run_price;
pub(crate) use execution::run_price_fitted_model;

#[derive(Debug, Args)]
pub(crate) struct PriceCommand {
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
    pub(crate) model: ModelPathOptions,

    #[command(flatten)]
    pub(crate) option: VanillaOptionPricingOptions,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct ObservedOptionPricingCommand {
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

    #[arg(long, value_name = "UTC", help = "Exact option expiry timestamp.")]
    pub(crate) expiry_timestamp_utc: Option<String>,

    #[arg(
        long,
        value_name = "DATE",
        help = "Option expiry date in YYYY-MM-DD. The valuation UTC time-of-day is preserved."
    )]
    pub(crate) expiry_date: Option<String>,

    #[arg(
        long,
        value_name = "TENOR",
        help = "Relative option tenor. Supported units: h, d, w, m, y."
    )]
    pub(crate) tenor: Option<String>,

    #[arg(
        long,
        value_enum,
        default_value_t = PricePayoffType::Call,
        help = "Vanilla option payoff type."
    )]
    pub(crate) payoff_type: PricePayoffType,

    #[arg(
        long,
        value_name = "RATE",
        default_value_t = 0.0,
        help = "Continuously compounded risk-free rate."
    )]
    pub(crate) risk_free_rate: f64,

    #[arg(
        long,
        value_name = "RATE",
        default_value_t = 0.0,
        help = "Continuously compounded dividend yield."
    )]
    pub(crate) dividend_yield: f64,

    #[arg(long, help = "Include transform diagnostics in pricing_report.json.")]
    pub(crate) include_transform_diagnostics: bool,

    #[arg(
        long,
        help = "Include integration quality diagnostics in pricing_report.json."
    )]
    pub(crate) include_integration_quality_diagnostics: bool,

    #[arg(long, help = "Include calibration hooks in pricing.json.")]
    pub(crate) include_calibration_hooks: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PricePayoffType {
    Call,
    Put,
}

impl Display for PricePayoffType {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Call => write!(formatter, "call"),
            Self::Put => write!(formatter, "put"),
        }
    }
}
