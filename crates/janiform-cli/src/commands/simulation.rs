use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::CliBoundaryOptions;
use crate::commands::options::ModelPathOptions;
use crate::commands::options::OutputPathOptions;

#[derive(Debug, Args)]
pub(crate) struct SimulateCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[command(flatten)]
    pub(crate) model: ModelPathOptions,

    #[command(flatten)]
    pub(crate) simulation: SimulationHorizonOptions,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,

    #[command(subcommand)]
    pub(crate) command: Option<SimulateSubcommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SimulateSubcommand {
    #[command(name = "seeded-ticks", about = "Simulate a seeded tick path.")]
    SeededTicks(SeededTicksCommand),

    #[command(name = "coarse-interval", about = "Simulate a coarse interval path.")]
    CoarseInterval(CoarseIntervalCommand),
}

#[derive(Debug, Args)]
pub(crate) struct SeededTicksCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[command(flatten)]
    pub(crate) model: ModelPathOptions,

    #[command(flatten)]
    pub(crate) simulation: SimulationHorizonOptions,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct CoarseIntervalCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[command(flatten)]
    pub(crate) model: ModelPathOptions,

    #[command(flatten)]
    pub(crate) simulation: SimulationHorizonOptions,

    #[arg(long, value_name = "INTERVAL", help = "Coarse interval specification.")]
    pub(crate) interval: Option<String>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,
}

#[derive(Debug, Args)]
pub(crate) struct SimulationHorizonOptions {
    #[arg(long, value_name = "SEED", help = "Simulation random seed.")]
    pub(crate) seed: Option<u64>,

    #[arg(long, value_name = "UTC", help = "Simulation start timestamp.")]
    pub(crate) start_time_utc: Option<String>,

    #[arg(
        long,
        value_name = "COUNT",
        help = "Number of horizon intervals to simulate."
    )]
    pub(crate) horizon_interval_count: Option<u64>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Optional simulation debug trace path."
    )]
    pub(crate) debug_trace_path: Option<PathBuf>,
}
