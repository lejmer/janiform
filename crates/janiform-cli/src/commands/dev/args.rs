use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;
use janiform_governance::RegressionHarnessToleranceConfig;
use janiform_verification::OpenClosedGapLeakageRegressionHarnessToleranceConfig;

#[derive(Debug, Args)]
pub(crate) struct DevCommand {
    #[command(subcommand)]
    pub(crate) command: DevSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum DevSubcommand {
    #[command(about = "Developer regression harnesses.")]
    Regression(RegressionCommand),
}

#[derive(Debug, Args)]
pub(crate) struct RegressionCommand {
    #[command(subcommand)]
    pub(crate) command: RegressionSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum RegressionSubcommand {
    #[command(about = "Run deterministic regression checks.")]
    Deterministic(DeterministicRegressionCommand),

    #[command(
        name = "open-closed-gap-leakage",
        about = "Run open/closed gap leakage regression checks."
    )]
    OpenClosedGapLeakage(OpenClosedGapLeakageRegressionCommand),
}

#[derive(Debug, Args)]
pub(crate) struct DeterministicRegressionCommand {
    #[arg(long, value_name = "PATH", help = "Calendar registry JSON document.")]
    pub(crate) calendar_registry_json: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Calendar regression cases JSON document."
    )]
    pub(crate) calendar_cases_json: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Hourly-factor regression cases JSON document."
    )]
    pub(crate) seasonality_cases_json: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = RegressionHarnessToleranceConfig::default().seasonality_absolute_tolerance,
        help = "Absolute tolerance for seasonality factor mismatches."
    )]
    pub(crate) seasonality_absolute_tolerance: f64,

    #[arg(
        long,
        default_value_t = RegressionHarnessToleranceConfig::default().news_intensity_absolute_tolerance,
        help = "Absolute tolerance for news-intensity factor mismatches."
    )]
    pub(crate) news_intensity_absolute_tolerance: f64,
}

#[derive(Debug, Args)]
pub(crate) struct OpenClosedGapLeakageRegressionCommand {
    #[arg(
        long,
        value_name = "PATH",
        help = "Calendar regression fixture JSON document."
    )]
    pub(crate) calendar_fixture_path: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Open/closed gap behavior fixture JSON document."
    )]
    pub(crate) gap_behavior_fixture_path: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = OpenClosedGapLeakageRegressionHarnessToleranceConfig::default().component_absolute_tolerance,
        help = "Absolute tolerance for component-level semantic mismatches."
    )]
    pub(crate) component_absolute_tolerance: f64,
}
