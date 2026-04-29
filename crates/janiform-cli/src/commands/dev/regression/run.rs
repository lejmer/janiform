use std::path::Path;
use std::path::PathBuf;

use janiform_foundations::numeric::validate_non_negative_number;
use janiform_governance::{
    RegressionHarnessError, RegressionHarnessToleranceConfig, run_deterministic_regression_harness,
};
use janiform_verification::{
    OpenClosedGapLeakageRegressionHarnessError,
    OpenClosedGapLeakageRegressionHarnessToleranceConfig,
    run_open_closed_gap_leakage_regression_harness,
};

use super::fixtures::{
    extract_calendar_cases, extract_gap_behavior_cases, extract_seasonality_cases,
};
use super::rendering::{emit_deterministic_regression_output, emit_open_closed_gap_leakage_output};
use crate::commands::dev::args::{
    DeterministicRegressionCommand, OpenClosedGapLeakageRegressionCommand,
};
use crate::commands::shared::artifacts::{load_calendar_registry_artifact, load_json_artifact};
use crate::commands::shared::inputs::required_input_file;
use crate::context::CommandContext;
use crate::error::{CliError, CliExitCode};

const DEFAULT_CALENDAR_FIXTURE_PATH: &str =
    "tests/fixtures/calendar/calendar_service_regression_matrix.json";
const DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH: &str =
    "tests/fixtures/gap_behavior/open_closed_gap_behavior_scenarios.json";

pub(crate) fn run_deterministic_regression(
    context: &CommandContext,
    command: DeterministicRegressionCommand,
) -> Result<CliExitCode, CliError> {
    validate_non_negative_finite_tolerance(
        "--seasonality-absolute-tolerance",
        command.seasonality_absolute_tolerance,
    )?;
    validate_non_negative_finite_tolerance(
        "--news-intensity-absolute-tolerance",
        command.news_intensity_absolute_tolerance,
    )?;

    let calendar_registry_path = required_input_file(
        context,
        "--calendar-registry-json",
        &command.calendar_registry_json,
    )?;
    let calendar_cases_path = required_input_file(
        context,
        "--calendar-cases-json",
        &command.calendar_cases_json,
    )?;
    let seasonality_cases_path = required_input_file(
        context,
        "--seasonality-cases-json",
        &command.seasonality_cases_json,
    )?;

    let calendar_cases_document = load_json_artifact(&calendar_cases_path, "calendar cases")?;
    let seasonality_cases_document =
        load_json_artifact(&seasonality_cases_path, "seasonality cases")?;

    let calendar_registry = load_calendar_registry_artifact(&calendar_registry_path)?;
    let calendar_cases = extract_calendar_cases(&calendar_cases_path, &calendar_cases_document)?;
    let seasonality_cases =
        extract_seasonality_cases(&seasonality_cases_path, &seasonality_cases_document)?;

    let report = run_deterministic_regression_harness(
        &calendar_registry,
        &calendar_cases,
        &seasonality_cases,
        RegressionHarnessToleranceConfig {
            seasonality_absolute_tolerance: command.seasonality_absolute_tolerance,
            news_intensity_absolute_tolerance: command.news_intensity_absolute_tolerance,
        },
    )
    .map_err(deterministic_harness_error)?;

    let failed_case_count = report.summary.failed_case_count;

    emit_deterministic_regression_output(context, &report)?;

    Ok(if failed_case_count == 0 {
        CliExitCode::Success
    } else {
        CliExitCode::ReportFailedChecks
    })
}

pub(crate) fn run_open_closed_gap_leakage_regression(
    context: &CommandContext,
    command: OpenClosedGapLeakageRegressionCommand,
) -> Result<CliExitCode, CliError> {
    validate_non_negative_finite_tolerance(
        "--component-absolute-tolerance",
        command.component_absolute_tolerance,
    )?;

    let calendar_fixture_path = optional_or_default_input_file(
        context,
        "--calendar-fixture-path",
        command.calendar_fixture_path,
        DEFAULT_CALENDAR_FIXTURE_PATH,
    )?;
    let gap_behavior_fixture_path = optional_or_default_input_file(
        context,
        "--gap-behavior-fixture-path",
        command.gap_behavior_fixture_path,
        DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH,
    )?;

    let gap_behavior_fixture_document = load_json_artifact(
        &gap_behavior_fixture_path,
        "open/closed gap behavior fixture",
    )?;

    let calendar_registry = load_calendar_registry_artifact(&calendar_fixture_path)?;
    let gap_behavior_cases =
        extract_gap_behavior_cases(&gap_behavior_fixture_path, &gap_behavior_fixture_document)?;

    let report = run_open_closed_gap_leakage_regression_harness(
        &calendar_registry,
        &gap_behavior_cases,
        OpenClosedGapLeakageRegressionHarnessToleranceConfig {
            component_absolute_tolerance: command.component_absolute_tolerance,
        },
    )
    .map_err(open_closed_gap_harness_error)?;

    let failed_case_count = report.summary.failed_case_count;

    emit_open_closed_gap_leakage_output(context, &report)?;

    Ok(if failed_case_count == 0 {
        CliExitCode::Success
    } else {
        CliExitCode::ReportFailedChecks
    })
}

fn validate_non_negative_finite_tolerance(
    argument_name: &'static str,
    value: f64,
) -> Result<(), CliError> {
    validate_non_negative_number(value, argument_name).map_err(|source| {
        CliError::InvalidCommandArgument {
            argument_name,
            message: source.to_string(),
        }
    })
}

fn optional_or_default_input_file(
    context: &CommandContext,
    argument_name: &'static str,
    path: Option<PathBuf>,
    default_fixture_path: &str,
) -> Result<PathBuf, CliError> {
    match path {
        Some(path) => context.paths().input_file(argument_name, &path),
        None => {
            let path = PathBuf::from(default_fixture_path);
            let Some(development_fixture_root) =
                discover_development_fixture_root(context.paths().process_cwd(), &path)
            else {
                return Err(CliError::InvalidCommandArgument {
                    argument_name,
                    message: format!(
                        "default fixture `{default_fixture_path}` is only available when running from the Janiform source tree; pass `{argument_name}` explicitly"
                    ),
                });
            };
            let resolved_path = development_fixture_root.join(&path);

            if resolved_path.is_file() {
                return Ok(resolved_path);
            }

            Err(CliError::MissingInputFile {
                argument_name,
                path: resolved_path,
            })
        }
    }
}

fn discover_development_fixture_root(start: &Path, fixture_path: &Path) -> Option<PathBuf> {
    for candidate in start.ancestors() {
        if candidate.join("crates").join("janiform-cli").is_dir()
            && candidate.join(fixture_path).is_file()
        {
            return Some(PathBuf::from(candidate));
        }
    }

    None
}

fn deterministic_harness_error(error: RegressionHarnessError) -> CliError {
    let argument_name = match error {
        RegressionHarnessError::InvalidSeasonalityAbsoluteTolerance { .. } => {
            "--seasonality-absolute-tolerance"
        }
        RegressionHarnessError::InvalidNewsIntensityAbsoluteTolerance { .. } => {
            "--news-intensity-absolute-tolerance"
        }
    };

    CliError::InvalidCommandArgument {
        argument_name,
        message: error.to_string(),
    }
}

fn open_closed_gap_harness_error(error: OpenClosedGapLeakageRegressionHarnessError) -> CliError {
    match error {
        OpenClosedGapLeakageRegressionHarnessError::InvalidComponentAbsoluteTolerance {
            ..
        } => CliError::InvalidCommandArgument {
            argument_name: "--component-absolute-tolerance",
            message: error.to_string(),
        },
        OpenClosedGapLeakageRegressionHarnessError::MissingScenarioClassCoverage { .. } => {
            CliError::InvalidCommandArgument {
                argument_name: "--gap-behavior-fixture-path",
                message: error.to_string(),
            }
        }
    }
}
