use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::CliBoundaryOptions;
use crate::commands::options::OutputPathOptions;

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
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[arg(long, value_name = "PATH", help = "Regression baseline path.")]
    pub(crate) baseline_path: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct OpenClosedGapLeakageRegressionCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[arg(long, value_name = "PATH", help = "Regression fixture path.")]
    pub(crate) fixture_path: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}
