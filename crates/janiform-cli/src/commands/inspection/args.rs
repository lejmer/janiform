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

    #[command(
        about = "Inspect fitted seasonality on a requested interval grid.",
        long_about = "Inspect fitted seasonality on a requested interval grid.

This command reads a saved model, materializes its Fourier business-time clock over the requested UTC range and interval, and reports the deterministic seasonality factors that downstream runtimes would consume."
    )]
    Seasonality(InspectSeasonalityCommand),
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

#[derive(Debug, Args)]
#[command(
    about = "Inspect fitted seasonality on a requested interval grid.",
    long_about = "Inspect fitted seasonality on a requested interval grid.

The model artifact must include a fitted Fourier seasonality clock and the resolved session definition used during calibration."
)]
pub(crate) struct InspectSeasonalityCommand {
    #[arg(long, value_name = "MODEL_PATH", help = "Saved model path to inspect.")]
    pub(crate) model: PathBuf,

    #[arg(
        long,
        value_name = "RFC3339_UTC",
        help = "Inclusive UTC range start, for example 2026-02-02T14:00:00Z."
    )]
    pub(crate) range_start_utc: String,

    #[arg(
        long,
        value_name = "RFC3339_UTC",
        help = "Exclusive UTC range end, for example 2026-02-03T14:00:00Z."
    )]
    pub(crate) range_end_utc: String,

    #[arg(
        long,
        default_value = "1h",
        value_name = "DURATION",
        help = "Fixed UTC interval duration such as 5min, 1h, or 1d."
    )]
    pub(crate) interval: String,
}
