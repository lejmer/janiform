use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;

use crate::commands::options::OutputPathOptions;

#[derive(Debug, Args)]
#[command(
    about = "Render reports from saved Janiform outputs.",
    long_about = "Render reports from saved Janiform outputs.

Reports are read-only views over completed or in-progress Janiform runs. Use inspect for status-oriented questions and report for outcome-oriented summaries."
)]
pub(crate) struct ReportCommand {
    #[command(subcommand)]
    pub(crate) command: ReportSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ReportSubcommand {
    #[command(
        about = "Report fit outcomes, stage metrics, durations, warnings, and outputs.",
        long_about = "Report fit outcomes, stage metrics, durations, warnings, and outputs.

The command accepts either a run directory or a fit report file. It can render a concise terminal report, write one JSON summary with --json, or write an HTML report with --html."
    )]
    Fit(ReportFitCommand),
}

#[derive(Debug, Args)]
#[command(
    about = "Report fit outcomes, stage metrics, durations, warnings, and outputs.",
    long_about = "Report fit outcomes, stage metrics, durations, warnings, and outputs.

When given a run directory, the command reads the fit report and uses the saved model and latest status snapshot when available. With --html, --output-path selects the HTML report path."
)]
pub(crate) struct ReportFitCommand {
    #[command(flatten)]
    pub(crate) output_path: OutputPathOptions,

    #[arg(long, help = "Write a rendered HTML report.")]
    pub(crate) html: bool,

    #[arg(
        value_name = "RUN_OR_REPORT_PATH",
        help = "Run directory or fit report document."
    )]
    pub(crate) run_or_report_path: PathBuf,
}
