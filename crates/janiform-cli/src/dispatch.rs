use crate::cli::RootCommand;
use crate::commands::dev::DevSubcommand;
use crate::commands::dev::RegressionSubcommand;
use crate::commands::fit::FitStagedSubcommand;
use crate::commands::fit::FitSubcommand;
use crate::commands::fit::execution::run_fit_execution;
use crate::commands::fit::execution::run_fit_staged_execution;
use crate::commands::fit::planning::run_fit_inspect;
use crate::commands::fit::planning::run_fit_plan;
use crate::commands::fit::planning::run_fit_staged_plan;
use crate::commands::hourly_factors::HourlyFactorsSubcommand;
use crate::commands::inspection::InspectSubcommand;
use crate::commands::inspection::run_inspect_model;
use crate::commands::inspection::run_inspect_run;
use crate::commands::presets::PresetsSubcommand;
use crate::commands::pricing::PriceSubcommand;
use crate::commands::pricing::run_price;
use crate::commands::pricing::run_price_fitted_model;
use crate::commands::reporting::ReportSubcommand;
use crate::commands::reporting::run_report_fit;
use crate::commands::seasonality::SeasonalitySubcommand;
use crate::commands::simulation::SimulateSubcommand;
use crate::commands::stylized_facts::StylizedFactsSubcommand;
use crate::context::CommandContext;
use crate::error::CliError;

pub(crate) fn dispatch(context: &CommandContext, command: RootCommand) -> Result<(), CliError> {
    tracing::debug!(
        output_mode = ?context.output_mode(),
        event_stream = ?context.event_settings().stream_format,
        "dispatching parsed CLI command"
    );

    match command {
        RootCommand::Fit(command) => dispatch_fit(context, command),
        RootCommand::Inspect(command) => match command.command {
            InspectSubcommand::Run(command) => run_inspect_run(context, command),
            InspectSubcommand::Model(command) => run_inspect_model(context, command),
        },
        RootCommand::Report(command) => match command.command {
            ReportSubcommand::Fit(command) => run_report_fit(context, command),
        },
        RootCommand::Price(command) => dispatch_price(context, command),
        RootCommand::Simulate(command) => match command.command {
            Some(SimulateSubcommand::SeededTicks(command)) => {
                crate::commands::simulation::run_simulate_seeded_ticks(context, command)
            }
            Some(SimulateSubcommand::CoarseInterval(command)) => {
                crate::commands::simulation::run_simulate_coarse_interval(context, command)
            }
            None => crate::commands::simulation::run_simulate_from_model_artifact(context, command),
        },
        RootCommand::Presets(command) => match command.command {
            PresetsSubcommand::List(_) => not_implemented("presets list"),
            PresetsSubcommand::Show(_) => not_implemented("presets show"),
            PresetsSubcommand::Export(_) => not_implemented("presets export"),
        },
        RootCommand::Seasonality(command) => match command.command {
            SeasonalitySubcommand::PreprocessTimestamps(_) => {
                not_implemented("seasonality preprocess-timestamps")
            }
            SeasonalitySubcommand::Diagnostics(_) => not_implemented("seasonality diagnostics"),
        },
        RootCommand::HourlyFactors(command) => match command.command {
            HourlyFactorsSubcommand::MaterializeSeasonality(_) => {
                not_implemented("hourly-factors materialize-seasonality")
            }
            HourlyFactorsSubcommand::MaterializeNewsIntensity(_) => {
                not_implemented("hourly-factors materialize-news-intensity")
            }
            HourlyFactorsSubcommand::BuildContract(_) => {
                not_implemented("hourly-factors build-contract")
            }
        },
        RootCommand::StylizedFacts(command) => match command.command {
            StylizedFactsSubcommand::HorizonComparison(_) => {
                not_implemented("stylized-facts horizon-comparison")
            }
        },
        RootCommand::Dev(command) => match command.command {
            DevSubcommand::Regression(command) => match command.command {
                RegressionSubcommand::Deterministic(_) => {
                    not_implemented("dev regression deterministic")
                }
                RegressionSubcommand::OpenClosedGapLeakage(_) => {
                    not_implemented("dev regression open-closed-gap-leakage")
                }
            },
        },
    }
}

fn dispatch_fit(
    context: &CommandContext,
    mut command: crate::commands::fit::FitCommand,
) -> Result<(), CliError> {
    match command.command.take() {
        Some(FitSubcommand::Inspect(command)) => run_fit_inspect(context, command),
        Some(FitSubcommand::Plan(command)) => run_fit_plan(context, command),
        Some(FitSubcommand::Staged(command)) => match command.command {
            Some(FitStagedSubcommand::Plan(command)) => run_fit_staged_plan(context, command),
            Some(FitStagedSubcommand::Run(command)) => {
                run_fit_staged_execution(context, command.staged)
            }
            None => run_fit_staged_execution(context, command.staged),
        },
        None => run_fit_execution(context, command),
    }
}

fn dispatch_price(
    context: &CommandContext,
    mut command: crate::commands::pricing::PriceCommand,
) -> Result<(), CliError> {
    match command.command.take() {
        Some(PriceSubcommand::FittedModel(command)) => run_price_fitted_model(context, command),
        Some(PriceSubcommand::ObservedOptions(_)) => not_implemented("price observed-options"),
        None => run_price(context, command),
    }
}

fn not_implemented(command_path: &'static str) -> Result<(), CliError> {
    Err(CliError::unimplemented(command_path))
}
