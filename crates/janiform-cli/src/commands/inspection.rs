use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::CliBoundaryOptions;

#[derive(Debug, Args)]
pub(crate) struct InspectCommand {
    #[command(subcommand)]
    pub(crate) command: InspectSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum InspectSubcommand {
    #[command(about = "Inspect run status, artifacts, checkpoints, and event-log health.")]
    Run(InspectRunCommand),

    #[command(about = "Inspect model identity, provenance, fitted components, and capabilities.")]
    Model(InspectModelCommand),
}

#[derive(Debug, Args)]
pub(crate) struct InspectRunCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[arg(value_name = "RUN_DIR", help = "Run directory to inspect.")]
    pub(crate) run_directory: PathBuf,

    #[arg(long, help = "Include event-log and artifact metadata.")]
    pub(crate) verbose: bool,
}

#[derive(Debug, Args)]
pub(crate) struct InspectModelCommand {
    #[command(flatten)]
    pub(crate) boundary: CliBoundaryOptions,

    #[arg(value_name = "MODEL_PATH", help = "Persisted model path to inspect.")]
    pub(crate) model_path: PathBuf,

    #[arg(long, help = "Include detailed parameter summaries.")]
    pub(crate) parameters: bool,
}
