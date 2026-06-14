use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;
use janiform_workflows::simulation::DEFAULT_SIMULATION_EXPORT_BATCH_SIZE;

use crate::commands::options::ModelPathOptions;
use crate::commands::options::OutputPathOptions;

#[derive(Debug, Args)]
#[command(
    about = "Simulate paths from a fitted Janiform model.",
    long_about = "Simulate paths from a fitted Janiform model.

The simulate command uses a saved Janiform model to generate synthetic market paths over the requested horizon. It defaults to seeded tick paths. Add --coarse when you want interval-level paths instead of tick-level output.

Advanced input-file commands are also available:
  janiform simulate seeded-ticks     run from a prepared seeded-tick input file
  janiform simulate coarse-interval  run from a prepared interval input file

Simulation writes the exported path data and a simulation report. Use --json for one final machine-readable completion summary."
)]
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
        help = "Horizon interval duration such as 1min, 15min, 1h, or 1D."
    )]
    pub(crate) interval: String,

    #[arg(
        long,
        help = "Generate interval-level paths instead of seeded tick paths."
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
    #[command(
        name = "seeded-ticks",
        about = "Run from a prepared seeded-tick simulation input.",
        long_about = "Run from a prepared seeded-tick simulation input.

Use this when another tool has already prepared the seeded-tick simulation input and calendar file. To simulate from a saved Janiform model, use janiform simulate --model-path instead."
    )]
    SeededTicks(SeededTicksCommand),

    #[command(
        name = "coarse-interval",
        about = "Run from a prepared interval simulation input.",
        long_about = "Run from a prepared interval simulation input.

Use this when another tool has already prepared the interval-level simulation input. To simulate interval paths from a saved Janiform model, use janiform simulate --model-path --coarse instead."
    )]
    CoarseInterval(CoarseIntervalCommand),
}

#[derive(Debug, Args)]
#[command(
    about = "Run from a prepared seeded-tick simulation input.",
    long_about = "Run from a prepared seeded-tick simulation input.

The command exports synthetic tick records in batches and writes a simulation_report.json file next to the output path. The calendar file is required to recover interval factor grids."
)]
pub(crate) struct SeededTicksCommand {
    #[arg(
        long,
        value_name = "PATH",
        help = "Prepared seeded-tick simulation input file."
    )]
    pub(crate) request_json: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Calendar file used to recover interval factor grids."
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
#[command(
    about = "Run from a prepared interval simulation input.",
    long_about = "Run from a prepared interval simulation input.

The command exports interval records in batches and writes a simulation_report.json file next to the output path. Use seeded-ticks when you need tick-level output."
)]
pub(crate) struct CoarseIntervalCommand {
    #[arg(
        long,
        value_name = "PATH",
        help = "Prepared interval simulation input file."
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
        help = "Number of simulation output rows per exported batch."
    )]
    pub(crate) output_batch_size: usize,
}

#[derive(Debug, Args)]
pub(crate) struct SimulationHorizonOptions {
    #[arg(
        long,
        value_name = "SEED",
        help = "Simulation random seed. Defaults to 0."
    )]
    pub(crate) seed: Option<u64>,

    #[arg(
        long,
        value_name = "UTC",
        help = "Simulation start timestamp. Defaults to the current UTC time."
    )]
    pub(crate) start_time_utc: Option<String>,

    #[arg(
        long,
        value_name = "COUNT",
        help = "Number of horizon intervals to simulate using --interval."
    )]
    pub(crate) horizon_interval_count: Option<u64>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Optional seeded-tick debug trace path."
    )]
    pub(crate) debug_trace_path: Option<PathBuf>,
}
