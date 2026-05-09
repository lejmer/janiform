use crate::cli::RootCommand;
use crate::commands::dev::DevSubcommand;
use crate::commands::dev::RegressionSubcommand;
use crate::commands::dev::run_deterministic_regression;
use crate::commands::dev::run_open_closed_gap_leakage_regression;
use crate::commands::fit::FitStagedSubcommand;
use crate::commands::fit::FitSubcommand;
use crate::commands::fit::execution::run_fit_command;
use crate::commands::fit::execution::run_fit_staged_run_command;
use crate::commands::fit::planning::run_fit_inspect;
use crate::commands::fit::planning::run_fit_plan;
use crate::commands::fit::planning::run_fit_staged_plan;
use crate::commands::hourly_factors::HourlyFactorsSubcommand;
use crate::commands::hourly_factors::run_hourly_factor_build_contract;
use crate::commands::hourly_factors::run_hourly_factor_materialize_news_intensity;
use crate::commands::hourly_factors::run_hourly_factor_materialize_seasonality;
use crate::commands::inspection::InspectSubcommand;
use crate::commands::inspection::run_inspect_model;
use crate::commands::inspection::run_inspect_run;
use crate::commands::presets::PresetsSubcommand;
use crate::commands::presets::run_presets_export;
use crate::commands::presets::run_presets_list;
use crate::commands::presets::run_presets_show;
use crate::commands::pricing::PriceSubcommand;
use crate::commands::pricing::run_price_command;
use crate::commands::pricing::run_price_fitted_model_command;
use crate::commands::pricing::run_price_observed_options_command;
use crate::commands::reporting::ReportSubcommand;
use crate::commands::reporting::run_report_fit;
use crate::commands::seasonality::SeasonalitySubcommand;
use crate::commands::seasonality::run_seasonality_diagnostics;
use crate::commands::seasonality::run_seasonality_preprocess_timestamps;
use crate::commands::simulation::SimulateSubcommand;
use crate::commands::validation::ValidateSubcommand;
use crate::commands::validation::run_validate_stylized_facts;
use crate::context::CommandContext;
use crate::error::CliError;
use crate::error::CliExitCode;

pub(crate) fn dispatch(
    context: &CommandContext,
    command: RootCommand,
) -> Result<CliExitCode, CliError> {
    tracing::debug!(
        output_mode = ?context.output_mode(),
        event_stream = ?context.event_settings().stream_format,
        "dispatching parsed CLI command"
    );

    let result = match command {
        RootCommand::Fit(command) => dispatch_fit(context, *command),
        RootCommand::Inspect(command) => match command.command {
            InspectSubcommand::Run(command) => run_inspect_run(context, command),
            InspectSubcommand::Model(command) => run_inspect_model(context, command),
        },
        RootCommand::Report(command) => match command.command {
            ReportSubcommand::Fit(command) => run_report_fit(context, command),
        },
        RootCommand::Price(command) => dispatch_price(context, *command),
        RootCommand::Simulate(command) => {
            let mut command = *command;

            match command.command.take() {
                Some(SimulateSubcommand::SeededTicks(command)) => {
                    crate::commands::simulation::run_simulate_seeded_ticks_command(context, command)
                }
                Some(SimulateSubcommand::CoarseInterval(command)) => {
                    crate::commands::simulation::run_simulate_coarse_interval_command(
                        context, command,
                    )
                }
                None => crate::commands::simulation::run_simulate_from_model_artifact_command(
                    context, command,
                ),
            }
        }
        RootCommand::Presets(command) => match command.command {
            PresetsSubcommand::List(command) => run_presets_list(context, command),
            PresetsSubcommand::Show(command) => run_presets_show(context, command),
            PresetsSubcommand::Export(command) => run_presets_export(context, command),
        },
        RootCommand::Seasonality(command) => match command.command {
            SeasonalitySubcommand::PreprocessTimestamps(command) => {
                run_seasonality_preprocess_timestamps(context, command)
            }
            SeasonalitySubcommand::Diagnostics(command) => {
                run_seasonality_diagnostics(context, command)
            }
        },
        RootCommand::HourlyFactors(command) => match command.command {
            HourlyFactorsSubcommand::MaterializeSeasonality(command) => {
                run_hourly_factor_materialize_seasonality(context, command)
            }
            HourlyFactorsSubcommand::MaterializeNewsIntensity(command) => {
                run_hourly_factor_materialize_news_intensity(context, command)
            }
            HourlyFactorsSubcommand::BuildContract(command) => {
                run_hourly_factor_build_contract(context, command)
            }
        },
        RootCommand::Validate(command) => match command.command {
            ValidateSubcommand::StylizedFacts(command) => {
                return run_validate_stylized_facts(context, command);
            }
        },
        RootCommand::Dev(command) => match command.command {
            DevSubcommand::Regression(command) => match command.command {
                RegressionSubcommand::Deterministic(command) => {
                    return run_deterministic_regression(context, command);
                }
                RegressionSubcommand::OpenClosedGapLeakage(command) => {
                    return run_open_closed_gap_leakage_regression(context, command);
                }
            },
        },
    };

    result.map(|()| CliExitCode::Success)
}

fn dispatch_fit(
    context: &CommandContext,
    mut command: crate::commands::fit::FitCommand,
) -> Result<(), CliError> {
    match command.command.take() {
        Some(FitSubcommand::Inspect(command)) => run_fit_inspect(context, command),
        Some(FitSubcommand::Plan(command)) => run_fit_plan(context, command),
        Some(FitSubcommand::Staged(command)) => match command.command {
            Some(FitStagedSubcommand::Plan(command)) => run_fit_staged_plan(context, *command),
            Some(FitStagedSubcommand::Run(command)) => {
                run_fit_staged_run_command(context, command.staged)
            }
            None => run_fit_staged_run_command(context, command.staged),
        },
        None => run_fit_command(context, command),
    }
}

fn dispatch_price(
    context: &CommandContext,
    mut command: crate::commands::pricing::PriceCommand,
) -> Result<(), CliError> {
    match command.command.take() {
        Some(PriceSubcommand::FittedModel(command)) => {
            run_price_fitted_model_command(context, command)
        }
        Some(PriceSubcommand::ObservedOptions(command)) => {
            run_price_observed_options_command(context, *command)
        }
        None => run_price_command(context, command),
    }
}
