use clap::{Args, ValueEnum};
use janiform_workflows::forecast::DEFAULT_FORECAST_DISTRIBUTION_NODE_COUNT;

use crate::commands::options::{ModelPathOptions, OutputPathOptions};

#[derive(Debug, Args)]
#[command(
    about = "Forecast expected path and terminal distribution from a fitted Janiform model.",
    long_about = "Forecast expected path and terminal distribution from a fitted Janiform model.

The forecast command uses a saved affine model directly. It writes a forecast bundle containing a forecast summary, expected path, and terminal distribution tables."
)]
pub(crate) struct ForecastCommand {
    #[command(flatten)]
    pub(crate) model: ModelPathOptions,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,

    #[arg(
        long,
        value_name = "UTC",
        help = "Forecast start timestamp. Defaults to current UTC time."
    )]
    pub(crate) start_date_utc: Option<String>,

    #[arg(
        long,
        value_name = "PRICE",
        conflicts_with = "state_path",
        help = "Initial observed price. Defaults to the model-only forecast price when no state is supplied."
    )]
    pub(crate) initial_price: Option<f64>,

    #[arg(
        long,
        value_name = "PATH",
        conflicts_with = "initial_price",
        help = "Current-state JSON artifact used as the forecast boundary condition."
    )]
    pub(crate) state_path: Option<std::path::PathBuf>,

    #[arg(
        long,
        value_name = "INTERVAL",
        default_value = "1h",
        help = "Forecast interval duration such as 1min, 15min, 1h, or 1D."
    )]
    pub(crate) interval: String,

    #[arg(
        long,
        value_name = "COUNT",
        conflicts_with = "end_date_utc",
        help = "Number of forecast intervals."
    )]
    pub(crate) horizon_interval_count: Option<usize>,

    #[arg(
        long,
        value_name = "UTC",
        conflicts_with = "horizon_interval_count",
        help = "Forecast end timestamp. Janiform resolves to the smallest interval count covering it."
    )]
    pub(crate) end_date_utc: Option<String>,

    #[arg(long, value_enum, default_value_t = ForecastMeasureOption::Physical)]
    pub(crate) measure: ForecastMeasureOption,

    #[arg(
        long,
        value_name = "RATE",
        default_value_t = 0.0,
        help = "Risk-free rate used only for risk-neutral forecasts."
    )]
    pub(crate) risk_free_rate: f64,

    #[arg(
        long,
        value_name = "RATE",
        default_value_t = 0.0,
        help = "Dividend yield used only for risk-neutral forecasts."
    )]
    pub(crate) dividend_yield: f64,

    #[arg(long, value_enum, default_value_t = ForecastTableFormatOption::Parquet)]
    pub(crate) table_format: ForecastTableFormatOption,

    #[arg(
        long,
        value_name = "N",
        default_value_t = DEFAULT_FORECAST_DISTRIBUTION_NODE_COUNT,
        help = "Terminal distribution node count."
    )]
    pub(crate) distribution_node_count: usize,

    #[arg(
        long,
        value_name = "X",
        requires = "distribution_log_return_upper",
        help = "Explicit terminal log-return support lower bound."
    )]
    pub(crate) distribution_log_return_lower: Option<f64>,

    #[arg(
        long,
        value_name = "X",
        requires = "distribution_log_return_lower",
        help = "Explicit terminal log-return support upper bound."
    )]
    pub(crate) distribution_log_return_upper: Option<f64>,

    #[arg(
        long,
        value_name = "COUNT",
        help = "CPU count for parallel forecast work. Use 0 for all available CPUs."
    )]
    pub(crate) cpu_count: Option<usize>,

    #[arg(
        long,
        help = "Fail before writing artifacts when forecast warnings are emitted."
    )]
    pub(crate) treat_forecast_warnings_as_errors: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ForecastMeasureOption {
    #[value(help = "Use fitted physical dynamics.")]
    Physical,
    #[value(name = "risk_neutral", help = "Use risk-neutral martingale dynamics.")]
    RiskNeutral,
}

impl ForecastMeasureOption {
    pub(crate) const fn workflow_measure(self) -> janiform_workflows::forecast::ForecastMeasure {
        match self {
            Self::Physical => janiform_workflows::forecast::ForecastMeasure::Physical,
            Self::RiskNeutral => janiform_workflows::forecast::ForecastMeasure::RiskNeutral,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ForecastTableFormatOption {
    #[value(help = "Write Apache Parquet tables.")]
    Parquet,
    #[value(help = "Write CSV tables.")]
    Csv,
    #[value(help = "Write row-array JSON tables.")]
    Json,
}

impl ForecastTableFormatOption {
    pub(crate) const fn workflow_table_format(
        self,
    ) -> janiform_workflows::forecast::ForecastTableFormat {
        match self {
            Self::Parquet => janiform_workflows::forecast::ForecastTableFormat::Parquet,
            Self::Csv => janiform_workflows::forecast::ForecastTableFormat::Csv,
            Self::Json => janiform_workflows::forecast::ForecastTableFormat::Json,
        }
    }
}
