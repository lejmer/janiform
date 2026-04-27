use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::OutputPathOptions;

mod fit;

pub(crate) use fit::run_report_fit;

#[derive(Debug, Args)]
pub(crate) struct ReportCommand {
    #[command(subcommand)]
    pub(crate) command: ReportSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ReportSubcommand {
    #[command(about = "Report fit outcomes, stage metrics, durations, warnings, and artifacts.")]
    Fit(ReportFitCommand),
}

#[derive(Debug, Args)]
pub(crate) struct ReportFitCommand {
    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,

    #[arg(long, help = "Write a rendered HTML report artifact.")]
    pub(crate) html: bool,

    #[arg(
        value_name = "RUN_OR_REPORT_PATH",
        help = "Run directory or fit report document."
    )]
    pub(crate) run_or_report_path: PathBuf,
}
