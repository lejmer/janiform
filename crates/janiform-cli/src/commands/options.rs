use std::path::PathBuf;

use clap::Args;
use clap::ValueEnum;

use crate::commands::presets::PresetRegistryArgs;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ExecutionMode {
    #[value(help = "Keep supported intermediate results in memory.")]
    Memory,
    #[value(help = "Save run outputs and checkpoints to disk.")]
    Disk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum WorkflowScope {
    #[value(help = "Fit the full model using the inputs supplied on the command line.")]
    Full,
    #[value(help = "Prepare only the seasonality portion of the calibration workflow.")]
    SeasonalityOnly,
    #[value(help = "Fit the full model without an observed option surface.")]
    FullWithoutOptionSurface,
    #[value(help = "Fit the full model including the observed option surface.")]
    FullWithOptionSurface,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum CalibrationStageOption {
    #[value(help = "Resume from the seasonality stage.")]
    Seasonality,
    #[value(help = "Resume from the Hawkes activity stage.")]
    Hawkes,
    #[value(help = "Resume from the return-component stage.")]
    ReturnComponents,
    #[value(help = "Resume from the long-variance stage.")]
    LongVariance,
    #[value(help = "Resume from the news-shocks stage.")]
    NewsShocks,
    #[value(help = "Resume from the return-distribution backfit stage.")]
    ReturnBackfit,
    #[value(help = "Resume from the mean-reversion stage.")]
    MeanReversion,
    #[value(help = "Resume from the joint-refinement stage.")]
    JointRefinement,
    #[value(help = "Resume from the option-refinement stage.")]
    OptionRefinement,
    #[value(help = "Resume from the finalization stage.")]
    Finalization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum CalibrationDiagnosticOption {
    #[value(help = "Include option-surface node and tier shape diagnostics in the fit report.")]
    OptionSurfaceShape,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum InputFileFormat {
    #[value(help = "Read comma-separated values.")]
    Csv,
    #[value(help = "Read Apache Parquet.")]
    Parquet,
    #[value(help = "Read JSON.")]
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum TickPriceInputPolicy {
    #[value(help = "Use the explicit tick price column.")]
    ExplicitPriceColumn,
    #[value(help = "Use the midpoint of tick bid and ask columns.")]
    BidAskMidpoint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum OptionQuoteInputPolicy {
    #[value(help = "Use the explicit option mid-price column.")]
    MidPriceColumn,
    #[value(help = "Use the midpoint of option bid and ask columns.")]
    BidAskMidpoint,
    #[value(help = "Use the explicit option implied-volatility column.")]
    ImpliedVolatilityColumn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PriceCoordinatePolicyOption {
    #[value(help = "Infer separate coordinates when option data requires it.")]
    Auto,
    #[value(help = "Use one tick price coordinate for dynamics and option valuation.")]
    Single,
    #[value(help = "Require continuous dynamics with a raw option valuation anchor.")]
    ContinuousFitRawOptionAnchor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum TickObjectiveDataUsagePolicyOption {
    #[value(
        help = "Use all ticks for seasonality and Hawkes, and a short recent tick window for slower objectives."
    )]
    ShortHorizonWindow,
    #[value(help = "Use all available tick data in every tick-backed calibration objective.")]
    AllAvailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum CoarseTimestampSemanticsOption {
    #[value(help = "Treat coarse timestamps as bar-open times.")]
    BarOpenTime,
    #[value(help = "Treat coarse timestamps as bar-close times.")]
    BarCloseTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum DuplicateRecordPolicy {
    #[value(help = "Keep the first record for each duplicate key.")]
    KeepFirst,
    #[value(help = "Keep the last record for each duplicate key.")]
    KeepLast,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PresetReferenceRegistryScope {
    #[value(name = "built_in", help = "Resolve from built-in presets.")]
    BuiltIn,
    #[value(
        name = "user_registered",
        help = "Resolve from user-registered presets."
    )]
    UserRegistered,
}

impl PresetReferenceRegistryScope {
    pub(crate) fn registry_value(self) -> &'static str {
        match self {
            Self::BuiltIn => "built_in",
            Self::UserRegistered => "user_registered",
        }
    }
}

#[derive(Debug, Args)]
pub(crate) struct RunIdentityOptions {
    #[arg(long, value_name = "ID", help = "Stable run identifier.")]
    pub(crate) run_id: Option<String>,

    #[arg(
        long,
        value_name = "DIR",
        help = "Directory for command outputs.",
        long_help = "Directory for command outputs.

For fit runs this is the run directory. Disk execution writes the fitted model, fit report, progress log, latest status snapshot, and checkpoints under this directory."
    )]
    pub(crate) output_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct ModelPathOptions {
    #[arg(
        long,
        value_name = "PATH",
        help = "Saved Janiform model path.",
        long_help = "Saved Janiform model path.

Most commands read the model.jf file written by janiform fit."
    )]
    pub(crate) model_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct OutputPathOptions {
    #[arg(
        long,
        value_name = "PATH",
        help = "Primary output path.",
        long_help = "Primary output path.

When omitted, Janiform chooses a default output path for the command."
    )]
    pub(crate) output_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct TickDataOptions {
    #[arg(long, value_name = "PATH", help = "Tick data input file.")]
    pub(crate) tick_data_path: Option<PathBuf>,

    #[arg(
        long,
        value_name = "NAME",
        help = "Market profile preset name.",
        long_help = "Market profile preset name.

When omitted, Janiform may infer a bundled market profile from the tick-data file name if no explicit session hints are supplied."
    )]
    pub(crate) market_profile: Option<String>,

    #[arg(
        long,
        requires = "market_profile",
        value_enum,
        help = "Registry scope for --market-profile. Defaults to built-in."
    )]
    pub(crate) market_profile_scope: Option<PresetReferenceRegistryScope>,

    #[command(flatten)]
    pub(crate) preset_registry: PresetRegistryArgs,

    #[arg(long, value_enum, help = "Tick data file format.")]
    pub(crate) tick_data_format: Option<InputFileFormat>,

    #[arg(long, value_name = "LABEL", help = "Tick data provenance label.")]
    pub(crate) tick_provenance_label: Option<String>,

    #[arg(long, value_name = "PATH", help = "Optional coarse data file.")]
    pub(crate) coarse_data_path: Option<PathBuf>,

    #[arg(long, value_enum, help = "Coarse data file format.")]
    pub(crate) coarse_data_format: Option<InputFileFormat>,

    #[arg(long, value_name = "LABEL", help = "Coarse data provenance label.")]
    pub(crate) coarse_data_provenance_label: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Coarse data timestamp column.")]
    pub(crate) coarse_timestamp_column: Option<String>,

    #[arg(long, value_enum, help = "Coarse data timestamp semantics.")]
    pub(crate) coarse_timestamp_semantics: Option<CoarseTimestampSemanticsOption>,

    #[arg(
        long,
        value_name = "COLUMN",
        help = "Coarse data interval-start column."
    )]
    pub(crate) coarse_interval_start_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Coarse data interval-end column.")]
    pub(crate) coarse_interval_end_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Coarse data open-price column.")]
    pub(crate) coarse_open_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Coarse data high-price column.")]
    pub(crate) coarse_high_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Coarse data low-price column.")]
    pub(crate) coarse_low_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Coarse data close-price column.")]
    pub(crate) coarse_close_column: Option<String>,

    #[arg(long, value_name = "COLUMN", help = "Coarse data volume column.")]
    pub(crate) coarse_volume_column: Option<String>,

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

    #[arg(long, value_enum, help = "Tick price input policy.")]
    pub(crate) tick_price_input_policy: Option<TickPriceInputPolicy>,

    #[arg(long, value_name = "TZ", help = "Timezone for naive tick timestamps.")]
    pub(crate) timestamp_timezone: Option<String>,

    #[arg(long, value_enum, help = "Duplicate tick record policy.")]
    pub(crate) duplicate_record_policy: Option<DuplicateRecordPolicy>,

    #[arg(long, help = "Keep tick source order instead of sorting by timestamp.")]
    pub(crate) keep_tick_source_order: bool,

    #[arg(
        long,
        value_enum,
        help = "Tick data usage policy for slower calibration objectives."
    )]
    pub(crate) tick_objective_data_usage_policy: Option<TickObjectiveDataUsagePolicyOption>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Observed option surface input file."
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

    #[arg(long, value_enum, help = "Option quote input policy.")]
    pub(crate) option_quote_input_policy: Option<OptionQuoteInputPolicy>,

    #[arg(
        long,
        value_enum,
        help = "Price-coordinate policy for option workflows."
    )]
    pub(crate) price_coordinate_policy: Option<PriceCoordinatePolicyOption>,

    #[arg(
        long,
        value_name = "RELATIVE",
        help = "Relative put-call parity warning threshold for option valuation coordinates."
    )]
    pub(crate) option_forward_parity_warning_threshold: Option<f64>,

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

    #[arg(long, value_name = "CODE", help = "Market exchange code.")]
    pub(crate) exchange_code: Option<String>,

    #[arg(long, value_name = "TZ", help = "Market session timezone.")]
    pub(crate) session_timezone: Option<String>,

    #[arg(long, value_name = "PATH", help = "Session definition JSON path.")]
    pub(crate) session_definition_path: Option<PathBuf>,
}
