use clap::Parser;
use clap::Subcommand;

use crate::commands::dev::DevCommand;
use crate::commands::fit::FitCommand;
use crate::commands::hourly_factors::HourlyFactorsCommand;
use crate::commands::inspection::InspectCommand;
use crate::commands::presets::PresetsCommand;
use crate::commands::pricing::PriceCommand;
use crate::commands::reporting::ReportCommand;
use crate::commands::seasonality::SeasonalityCommand;
use crate::commands::simulation::SimulateCommand;
use crate::commands::stylized_facts::StylizedFactsCommand;
use crate::context::GlobalCliOptions;

#[derive(Debug, Parser)]
#[command(
    name = "janiform",
    version,
    about = "Native Janiform command shell.",
    long_about = "Primary workflows are the fit, simulate, and price commands.

Use the fit command to fit a Janiform model from market data.
Use the simulate command to simulate from a persisted Janiform model.
Use the price command to price options from a persisted Janiform model.

See also:
  janiform fit --help
  janiform simulate --help
  janiform price --help",
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
    #[command(about = "Fit a Janiform model from market data.")]
    Fit(FitCommand),

    #[command(about = "Inspect persisted runs and fitted models.")]
    Inspect(InspectCommand),

    #[command(about = "Build reports from persisted Janiform artifacts.")]
    Report(ReportCommand),

    #[command(
        about = "Price options from a persisted Janiform model.",
        alias = "pricing"
    )]
    Price(PriceCommand),

    #[command(about = "Simulate paths from a persisted Janiform model.")]
    Simulate(SimulateCommand),

    #[command(about = "Inspect bundled and user-registered presets.")]
    Presets(PresetsCommand),

    #[command(about = "Seasonality preprocessing workflows.")]
    Seasonality(SeasonalityCommand),

    #[command(
        name = "hourly-factors",
        about = "Hourly factor materialization and contract-building workflows."
    )]
    HourlyFactors(HourlyFactorsCommand),

    #[command(name = "stylized-facts", about = "Stylized-fact reporting workflows.")]
    StylizedFacts(StylizedFactsCommand),

    #[command(about = "Developer-only Janiform workflows.")]
    Dev(DevCommand),
}
