use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;
use janiform_governance::RegressionHarnessToleranceConfig;
use janiform_verification::OpenClosedGapLeakageRegressionHarnessToleranceConfig;

#[derive(Debug, Args)]
#[command(
    about = "Developer-only Janiform workflows.",
    long_about = "Developer-only Janiform workflows.

These commands are intended for repository diagnostics and regression fixture checks. They are not part of the normal fit, simulate, price, inspect, report, or validate workflow."
)]
pub(crate) struct DevCommand {
    #[command(subcommand)]
    pub(crate) command: DevSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum DevSubcommand {
    #[command(
        about = "Developer regression harnesses.",
        long_about = "Developer regression harnesses.

Regression commands compare deterministic fixtures and semantic validation fixtures against native Rust implementations."
    )]
    Regression(RegressionCommand),
}

#[derive(Debug, Args)]
#[command(
    about = "Developer regression harnesses.",
    long_about = "Developer regression harnesses.

Use these commands to validate checked-in regression fixtures for calendar behavior, hourly factors, deterministic outputs, and open/closed gap leakage behavior."
)]
pub(crate) struct RegressionCommand {
    #[command(subcommand)]
    pub(crate) command: RegressionSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum RegressionSubcommand {
    #[command(
        about = "Run deterministic regression checks.",
        long_about = "Run deterministic regression checks.

This command checks calendar and hourly-factor regression cases against the native deterministic harness."
    )]
    Deterministic(DeterministicRegressionCommand),

    #[command(
        name = "open-closed-gap-leakage",
        about = "Run open/closed gap leakage regression checks.",
        long_about = "Run open/closed gap leakage regression checks.

This command checks calendar fixtures and open/closed gap behavior fixtures against the native semantic validation harness."
    )]
    OpenClosedGapLeakage(OpenClosedGapLeakageRegressionCommand),
}

#[derive(Debug, Args)]
#[command(
    about = "Run deterministic regression checks.",
    long_about = "Run deterministic regression checks.

The command consumes a calendar registry, calendar regression cases, and hourly-factor regression cases. Tolerance options are fixture validation thresholds for this developer harness."
)]
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
#[command(
    about = "Run open/closed gap leakage regression checks.",
    long_about = "Run open/closed gap leakage regression checks.

The command consumes a calendar fixture and an open/closed gap behavior fixture. The component tolerance applies to semantic component comparisons in this developer harness."
)]
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
