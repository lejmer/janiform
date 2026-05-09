use std::fmt::{Display, Formatter};

use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;

use crate::commands::options::ModelPathOptions;
use crate::commands::options::OutputPathOptions;
use crate::commands::options::TickDataOptions;

#[derive(Debug, Args)]
#[command(
    about = "Price derivatives with a fitted Janiform model.",
    long_about = "Price derivatives with a fitted Janiform model.

The default price command values an explicit vanilla option using a saved Janiform model, valuation timestamp, spot, strike, expiry, payoff type, risk-free rate, and dividend yield.

Use price fitted-model for explicit vanilla option terms. Use price observed-options when you want to value an observed option snapshot. Pricing writes pricing.json and pricing_report.json when an output path is supplied or a default output path is available."
)]
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
        about = "Price vanilla options with a fitted model.",
        long_about = "Price vanilla options with a fitted model.

This command reads a saved Janiform model and the supplied vanilla option terms. It writes pricing.json as the primary pricing result and pricing_report.json for optional diagnostics."
    )]
    FittedModel(FittedModelPricingCommand),

    #[command(
        name = "observed-options",
        about = "Price an observed option snapshot with a fitted model.",
        long_about = "Price an observed option snapshot with a fitted model.

This command reads a saved Janiform model and an observed option snapshot, then writes pricing results for that snapshot."
    )]
    ObservedOptions(Box<ObservedOptionPricingCommand>),
}

#[derive(Debug, Args)]
#[command(
    about = "Price vanilla options with a fitted model.",
    long_about = "Price vanilla options with a fitted model.

Supply either --expiry-timestamp-utc, --expiry-date, or --tenor to define expiry. The valuation timestamp, spot, strike, payoff type, rates, and saved model define the pricing state."
)]
pub(crate) struct FittedModelPricingCommand {
    #[command(flatten)]
    pub(crate) model: ModelPathOptions,

    #[command(flatten)]
    pub(crate) option: VanillaOptionPricingOptions,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
#[command(
    about = "Price an observed option snapshot with a fitted model.",
    long_about = "Price an observed option snapshot with a fitted model.

The option surface input is interpreted as the observed pricing snapshot for the fitted model. This command is useful for pricing or reporting a whole snapshot rather than one manually specified vanilla option."
)]
pub(crate) struct ObservedOptionPricingCommand {
    #[command(flatten)]
    pub(crate) model: ModelPathOptions,

    #[command(flatten)]
    pub(crate) market_data: TickDataOptions,

    #[arg(
        long,
        value_name = "UTC",
        help = "Observed option valuation timestamp as RFC 3339 UTC."
    )]
    pub(crate) valuation_time_utc: Option<String>,

    #[arg(long, value_enum, help = "Observed option type filter.")]
    pub(crate) option_type: Option<PricePayoffType>,

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

    #[arg(
        long,
        help = "Price a single-expiry surface panel instead of one snapshot."
    )]
    pub(crate) surface: bool,

    #[arg(
        long,
        value_name = "COUNT",
        help = "Number of surface valuation times to retain."
    )]
    pub(crate) valuation_count: Option<usize>,

    #[arg(
        long,
        help = "Use every retained quote timestamp for single-expiry surface pricing."
    )]
    pub(crate) use_all_valuation_times: bool,

    #[arg(long, value_name = "COUNT", help = "CPU count for native pricing.")]
    pub(crate) cpu_count: Option<usize>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct VanillaOptionPricingOptions {
    #[arg(
        long,
        value_name = "UTC",
        help = "Pricing valuation timestamp as RFC 3339 UTC."
    )]
    pub(crate) valuation_time_utc: Option<String>,

    #[arg(long, value_name = "PRICE", help = "Current spot price.")]
    pub(crate) spot: Option<f64>,

    #[arg(long, value_name = "PRICE", help = "Vanilla option strike.")]
    pub(crate) strike: Option<f64>,

    #[arg(
        long,
        value_name = "UTC",
        help = "Exact option expiry timestamp as RFC 3339 UTC."
    )]
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
        help = "Relative option tenor. Supported units are h, d, w, m, and y."
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

    #[arg(long, help = "Include calibration details in pricing.json.")]
    pub(crate) include_calibration_hooks: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PricePayoffType {
    #[value(help = "Call payoff.")]
    Call,
    #[value(help = "Put payoff.")]
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
