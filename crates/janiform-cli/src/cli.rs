use clap::Parser;
use clap::Subcommand;

use crate::commands::dev::DevCommand;
use crate::commands::fit::FitCommand;
use crate::commands::forecast::ForecastCommand;
use crate::commands::inspection::InspectCommand;
use crate::commands::presets::PresetsCommand;
use crate::commands::pricing::PriceCommand;
use crate::commands::reporting::ReportCommand;
use crate::commands::simulation::SimulateCommand;
use crate::commands::validation::ValidateCommand;
use crate::context::GlobalCliOptions;

#[derive(Debug, Parser)]
#[command(
    name = "janiform",
    version,
    about = "Command-line interface for the Janiform high-frequency return model.",
    long_about = "Janiform is a high-frequency return model for realistic simulation of market prices and derivative valuation.

This command-line interface fits the model from market data, then uses fitted models for simulation, derivative valuation, reporting, and validation. Use command help for details on each task.

Output modes:
  default terminal output writes summaries to stdout and progress to stderr
  --quiet limits terminal output to final status and output paths
  --silent suppresses normal output while preserving warnings and errors
  --json writes one final JSON object to stdout
  --events jsonl writes machine events to stderr
  --trace-events adds trace-level records to the event stream

See also:
  janiform fit --help
  janiform simulate --help
  janiform price --help
  janiform validate --help
  janiform inspect --help
  janiform report --help",
    arg_required_else_help = true
)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub(crate) global: GlobalCliOptions,

    #[command(subcommand)]
    pub(crate) command: RootCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum RootCommand {
    #[command(about = "Fit Janiform models from market data.")]
    Fit(Box<FitCommand>),

    #[command(about = "Inspect saved runs and fitted models.")]
    Inspect(Box<InspectCommand>),

    #[command(about = "Render reports from saved Janiform outputs.")]
    Report(Box<ReportCommand>),

    #[command(
        about = "Price derivatives with a fitted Janiform model.",
        alias = "pricing"
    )]
    Price(Box<PriceCommand>),

    #[command(about = "Simulate paths from a fitted Janiform model.")]
    Simulate(Box<SimulateCommand>),

    #[command(
        about = "Forecast expected path and terminal distribution from a fitted Janiform model."
    )]
    Forecast(Box<ForecastCommand>),

    #[command(about = "Inspect bundled and user-registered presets.")]
    Presets(Box<PresetsCommand>),

    #[command(about = "Validate fitted models against empirical behavior.")]
    Validate(Box<ValidateCommand>),

    #[command(about = "Janiform workflows for developers.")]
    Dev(Box<DevCommand>),
}
