use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::CliBoundaryOptions;
use crate::commands::options::OutputPathOptions;

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
    pub(crate) boundary: CliBoundaryOptions,

    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,

    #[arg(
        value_name = "RUN_OR_REPORT_PATH",
        help = "Run directory or fit report document."
    )]
    pub(crate) run_or_report_path: PathBuf,
}
