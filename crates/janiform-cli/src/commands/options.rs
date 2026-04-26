use std::path::PathBuf;

use clap::Args;
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ExecutionMode {
    Memory,
    Disk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum WorkflowScope {
    Full,
    SeasonalityOnly,
    FullWithoutOptionSurface,
    FullWithOptionSurface,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum CalibrationStageOption {
    Seasonality,
    Hawkes,
    ReturnComponents,
    MeanReversion,
    JointRefinement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum InputFileFormat {
    Csv,
    Parquet,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum TickPriceInputPolicy {
    ExplicitPriceColumn,
    BidAskMidpoint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum OptionQuoteInputPolicy {
    MidPriceColumn,
    BidAskMidpoint,
    ImpliedVolatilityColumn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum DuplicateRecordPolicy {
    KeepFirst,
    KeepLast,
}

#[derive(Debug, Args)]
pub(crate) struct RunIdentityOptions {
    #[arg(long, value_name = "ID", help = "Stable run identifier.")]
    pub(crate) run_id: Option<String>,

    #[arg(long, value_name = "DIR", help = "Directory for command artifacts.")]
    pub(crate) output_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct ModelPathOptions {
    #[arg(long, value_name = "PATH", help = "Persisted Janiform model path.")]
    pub(crate) model_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct OutputPathOptions {
    #[arg(long, value_name = "PATH", help = "Primary command output path.")]
    pub(crate) output_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct TickDataOptions {
    #[arg(long, value_name = "PATH", help = "Tick data input path.")]
    pub(crate) tick_data_path: Option<PathBuf>,

    #[arg(long, value_enum, help = "Tick data file format.")]
    pub(crate) tick_data_format: Option<InputFileFormat>,

    #[arg(long, value_name = "LABEL", help = "Tick data provenance label.")]
    pub(crate) tick_provenance_label: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Tick timestamp column.")]
    pub(crate) tick_timestamp_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Tick price column.")]
    pub(crate) tick_price_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Tick trade sign column.")]
    pub(crate) tick_trade_sign_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Tick bid column.")]
    pub(crate) tick_bid_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Tick ask column.")]
    pub(crate) tick_ask_column: Option<String>,

    #[arg(long, value_enum, help = "Tick price normalization policy.")]
    pub(crate) tick_price_input_policy: Option<TickPriceInputPolicy>,

    #[arg(long, value_name = "TZ", help = "Timezone for naive tick timestamps.")]
    pub(crate) timestamp_timezone: Option<String>,

    #[arg(long, value_enum, help = "Duplicate tick record policy.")]
    pub(crate) duplicate_record_policy: Option<DuplicateRecordPolicy>,

    #[arg(long, help = "Keep tick source order instead of sorting by timestamp.")]
    pub(crate) keep_tick_source_order: bool,

    #[arg(
        long,
        value_name = "PATH",
        help = "Observed option surface input path."
    )]
    pub(crate) option_surface_path: Option<PathBuf>,

    #[arg(long, value_enum, help = "Option surface file format.")]
    pub(crate) option_surface_format: Option<InputFileFormat>,

    #[arg(long, value_name = "LABEL", help = "Option surface provenance label.")]
    pub(crate) option_surface_provenance_label: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Option quote timestamp column.")]
    pub(crate) option_quote_timestamp_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Option expiry timestamp column.")]
    pub(crate) option_expiry_timestamp_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Option strike column.")]
    pub(crate) option_strike_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Option type column.")]
    pub(crate) option_type_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Option mid price column.")]
    pub(crate) option_mid_price_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Option bid column.")]
    pub(crate) option_bid_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Option ask column.")]
    pub(crate) option_ask_column: Option<String>,

    #[arg(
        long,
        value_name = "COLUMN",
        help = "Option implied-volatility column."
    )]
    pub(crate) option_implied_volatility_column: Option<String>,

    #[arg(long, value_enum, help = "Option quote normalization policy.")]
    pub(crate) option_quote_input_policy: Option<OptionQuoteInputPolicy>,

    #[arg(
        long,
        value_name = "UTC",
        help = "Option valuation time as an RFC 3339 UTC timestamp."
    )]
    pub(crate) option_surface_valuation_time_utc: Option<String>,

    #[arg(long, value_name = "PATH", help = "Known-news input path.")]
    pub(crate) known_news_path: Option<PathBuf>,

    #[arg(long, value_enum, help = "Known-news file format.")]
    pub(crate) known_news_format: Option<InputFileFormat>,

    #[arg(long, value_name = "LABEL", help = "Known-news provenance label.")]
    pub(crate) known_news_provenance_label: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Known-news timestamp column.")]
    pub(crate) known_news_timestamp_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Known-news identifier column.")]
    pub(crate) known_news_identifier_column: Option<String>,

    #[arg(long, value_name = "TZ", help = "Known-news calendar timezone.")]
    pub(crate) known_news_timezone: Option<String>,

    #[arg(long, value_name = "EXCHANGE", help = "Market exchange code.")]
    pub(crate) exchange_code: Option<String>,

    #[arg(long, value_name = "TZ", help = "Market session timezone.")]
    pub(crate) session_timezone: Option<String>,

    #[arg(long, value_name = "PATH", help = "Session definition JSON path.")]
    pub(crate) session_definition_path: Option<PathBuf>,
}
