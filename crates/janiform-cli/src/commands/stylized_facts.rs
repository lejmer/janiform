use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::ExecutionMode;
use crate::commands::options::OutputPathOptions;

#[derive(Debug, Args)]
pub(crate) struct StylizedFactsCommand {
    #[command(subcommand)]
    pub(crate) command: StylizedFactsSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum StylizedFactsSubcommand {
    #[command(
        name = "horizon-comparison",
        about = "Compare stylized facts across empirical and simulated horizons."
    )]
    HorizonComparison(HorizonComparisonCommand),
}

#[derive(Debug, Args)]
pub(crate) struct HorizonComparisonCommand {
    #[arg(
        long,
        value_name = "PATH",
        help = "Empirical return series input path."
    )]
    pub(crate) empirical_path: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Simulated return series input path."
    )]
    pub(crate) simulated_path: Option<PathBuf>,

    #[arg(long, value_enum, help = "Stylized-fact execution mode.")]
    pub(crate) execution_mode: Option<ExecutionMode>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}
