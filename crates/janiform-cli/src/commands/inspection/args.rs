use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

#[derive(Debug, Args)]
#[command(
    about = "Inspect saved runs and fitted models.",
    long_about = "Inspect saved runs and fitted models.

Use inspect run to read run status, calibration stages, output paths, checkpoint availability, warnings, and progress-log health. Use inspect model to read model identity, provenance, fitted components, capabilities, and compact parameter summaries."
)]
pub(crate) struct InspectCommand {
    #[command(subcommand)]
    pub(crate) command: InspectSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum InspectSubcommand {
    #[command(
        about = "Inspect run status, outputs, checkpoints, and progress-log health.",
        long_about = "Inspect run status, outputs, checkpoints, and progress-log health.

The command reads the latest status snapshot first, then falls back to the progress log when needed. It is safe to use while a long-running disk fit is still writing outputs."
    )]
    Run(InspectRunCommand),

    #[command(
        about = "Inspect model identity, provenance, fitted components, and capabilities.",
        long_about = "Inspect model identity, provenance, fitted components, and capabilities.

Use this command when you have a saved model and need to understand what data produced it, which components were fit, and which downstream tasks it supports."
    )]
    Model(InspectModelCommand),
}

#[derive(Debug, Args)]
#[command(
    about = "Inspect run status, outputs, checkpoints, and progress-log health.",
    long_about = "Inspect run status, outputs, checkpoints, and progress-log health.

The run directory normally contains the fitted model, fit report, progress log, latest status snapshot, and checkpoint files for disk execution."
)]
pub(crate) struct InspectRunCommand {
    #[arg(value_name = "RUN_DIR", help = "Run directory to inspect.")]
    pub(crate) run_directory: PathBuf,

    #[arg(long, help = "Include progress-log and output metadata.")]
    pub(crate) include_events: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Inspect model identity, provenance, fitted components, and capabilities.",
    long_about = "Inspect model identity, provenance, fitted components, and capabilities.

The default view stays compact for terminal use. Add --parameters for fuller fitted-parameter summaries."
)]
pub(crate) struct InspectModelCommand {
    #[arg(value_name = "MODEL_PATH", help = "Saved model path to inspect.")]
    pub(crate) model_path: PathBuf,

    #[arg(long, help = "Include detailed parameter summaries.")]
    pub(crate) parameters: bool,
}
