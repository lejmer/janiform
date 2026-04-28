use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::ModelPathOptions;
use crate::commands::options::OutputPathOptions;

mod execution;
mod model_artifact;
mod rendering;
mod requests;

pub(crate) use execution::run_simulate_coarse_interval;
pub(crate) use execution::run_simulate_seeded_ticks;
pub(crate) use model_artifact::run_simulate_from_model_artifact;

use janiform_workflows::simulation::DEFAULT_SIMULATION_EXPORT_BATCH_SIZE;

#[derive(Debug, Args)]
pub(crate) struct SimulateCommand {
    #[command(flatten)]
    pub(crate) model: ModelPathOptions,

    #[command(flatten)]
    pub(crate) simulation: SimulationHorizonOptions,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,

    #[command(flatten)]
    pub(crate) export_batch: SimulationExportBatchOptions,

    #[arg(
        long,
        value_name = "INTERVAL",
        default_value = "1h",
        help = "Horizon interval duration such as `1min`, `15min`, `1h`, or `1D`."
    )]
    pub(crate) interval: String,

    #[arg(
        long,
        help = "Generate a coarse interval path instead of the default seeded tick path."
    )]
    pub(crate) coarse: bool,

    #[arg(
        long,
        help = "Enable seeded-tick debug tracing for the seeded simulation path."
    )]
    pub(crate) debug_trace_enabled: bool,

    #[arg(
        long,
        value_name = "N",
        default_value_t = 1,
        help = "Keep every nth transition in the seeded-tick trace."
    )]
    pub(crate) sample_every_nth_transition: u64,

    #[arg(
        long,
        value_name = "ROWS",
        default_value_t = 1024,
        help = "Maximum seeded-tick records emitted per chunk."
    )]
    pub(crate) max_records_per_chunk: usize,

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
    #[arg(
        long,
        value_name = "PATH",
        help = "Seeded tick simulation request JSON document."
    )]
    pub(crate) request_json: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Calendar registry JSON document used to rehydrate hourly factors."
    )]
    pub(crate) calendar_registry_json: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,

    #[command(flatten)]
    pub(crate) export_batch: SimulationExportBatchOptions,

    #[arg(
        long,
        value_name = "PRICE",
        default_value_t = 1.0,
        help = "Reference price used to convert simulated log prices."
    )]
    pub(crate) reference_price: f64,

    #[arg(
        long,
        value_name = "COLUMN",
        default_value = "timestamp",
        help = "Output timestamp column name."
    )]
    pub(crate) timestamp_column: String,

    #[arg(
        long,
        value_name = "COLUMN",
        default_value = "price",
        help = "Output price column name."
    )]
    pub(crate) price_column: String,

    #[arg(
        long,
        value_name = "COLUMN",
        help = "Optional output trade-side column name."
    )]
    pub(crate) side_column: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct CoarseIntervalCommand {
    #[arg(
        long,
        value_name = "PATH",
        help = "Coarse interval simulation request JSON document."
    )]
    pub(crate) request_json: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,

    #[command(flatten)]
    pub(crate) export_batch: SimulationExportBatchOptions,

    #[arg(
        long,
        value_name = "PRICE",
        default_value_t = 1.0,
        help = "Reference price used to convert simulated log prices."
    )]
    pub(crate) reference_price: f64,

    #[arg(
        long,
        value_name = "COLUMN",
        default_value = "timestamp",
        help = "Output timestamp column name."
    )]
    pub(crate) timestamp_column: String,

    #[arg(
        long,
        value_name = "COLUMN",
        default_value = "price",
        help = "Output price column name."
    )]
    pub(crate) price_column: String,
}

#[derive(Debug, Args)]
pub(crate) struct SimulationExportBatchOptions {
    #[arg(
        long = "output-batch-size",
        value_name = "ROWS",
        default_value_t = DEFAULT_SIMULATION_EXPORT_BATCH_SIZE,
        help = "Number of simulation output rows per exported artifact batch."
    )]
    pub(crate) output_batch_size: usize,
}

#[derive(Debug, Args)]
pub(crate) struct SimulationHorizonOptions {
    #[arg(
        long,
        value_name = "SEED",
        help = "Simulation random seed. Default: 0."
    )]
    pub(crate) seed: Option<u64>,

    #[arg(
        long,
        value_name = "UTC",
        help = "Simulation start timestamp. Default: current UTC time."
    )]
    pub(crate) start_time_utc: Option<String>,

    #[arg(
        long,
        value_name = "COUNT",
        help = "Number of horizon intervals to simulate, using the duration from --interval."
    )]
    pub(crate) horizon_interval_count: Option<u64>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Optional simulation debug trace path."
    )]
    pub(crate) debug_trace_path: Option<PathBuf>,
}
