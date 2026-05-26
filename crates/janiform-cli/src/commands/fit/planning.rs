use std::path::Path;

use janiform::calibration::artifacts::CalibrationExecutionMode;

use super::payloads::{inspect_json, plan_json, staged_plan_json};
use super::planning_rendering::{
    render_inspect_human, render_plan_human, render_staged_plan_human,
};
use super::preparation::{
    FitPreparationOverrides, FitPreviewOptions, PreparedFitPreview, prepare_fit_preview,
    write_staged_execution_input,
};
use crate::commands::fit::FitInspectCommand;
use crate::commands::fit::FitPlanCommand;
use crate::commands::fit::FitStagedPlanCommand;
use crate::commands::shared::calibration::calibration_execution_mode;
use crate::commands::shared::output::write_json_payload;
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;

pub(crate) fn run_fit_plan(
    context: &CommandContext,
    command: FitPlanCommand,
) -> Result<(), CliError> {
    emit_progress(context, "Plan fit: prepare market data")?;

    let prepared = prepare_fit_preview(
        context,
        &command.inputs,
        &command.run,
        FitPreviewOptions {
            workflow_scope: command.workflow_scope,
            execution_mode: command.execution_mode.map(calibration_execution_mode),
            cpu_count: command.cpu_count,
            overrides: FitPreparationOverrides {
                mean_reversion_grid_interval_hours: command.mean_reversion_grid_interval_hours,
                dynamics_search_restart_candidate_budget: command
                    .dynamics_search_restart_candidate_budget,
                treat_calibration_warnings_as_errors: command.treat_calibration_warnings_as_errors,
                stylized_fact_huber_cutoff: command.stylized_fact_huber_cutoff,
                stylized_fact_admissibility_z_threshold: command
                    .stylized_fact_admissibility_z_threshold,
            },
            no_persist: false,
        },
    )?;

    let staged_artifact_path = write_staged_execution_input(
        context,
        command.staged_execution_input_output_path.as_deref(),
        &prepared.plan,
    )?;

    emit_plan_output(context, &prepared, staged_artifact_path.as_deref())
}

pub(crate) fn run_fit_inspect(
    context: &CommandContext,
    command: FitInspectCommand,
) -> Result<(), CliError> {
    emit_progress(context, "Inspect fit: prepare market data")?;

    let prepared = prepare_fit_preview(
        context,
        &command.inputs,
        &command.run,
        FitPreviewOptions {
            workflow_scope: command.workflow_scope,
            execution_mode: None,
            cpu_count: command.cpu_count,
            overrides: FitPreparationOverrides::default(),
            no_persist: false,
        },
    )?;

    emit_inspect_output(context, &prepared)
}

pub(crate) fn run_fit_staged_plan(
    context: &CommandContext,
    command: FitStagedPlanCommand,
) -> Result<(), CliError> {
    emit_progress(context, "Plan staged fit: prepare stage inputs")?;

    let prepared = prepare_fit_preview(
        context,
        &command.inputs,
        &command.run,
        FitPreviewOptions {
            workflow_scope: command.workflow_scope,
            execution_mode: Some(CalibrationExecutionMode::Disk),
            cpu_count: command.cpu_count,
            overrides: FitPreparationOverrides::default(),
            no_persist: false,
        },
    )?;

    let staged_artifact_path = write_staged_execution_input(
        context,
        command.execution_input_output_path.as_deref(),
        &prepared.plan,
    )?;

    emit_staged_plan_output(context, &prepared, staged_artifact_path.as_deref())
}

fn emit_plan_output(
    context: &CommandContext,
    prepared: &PreparedFitPreview,
    staged_artifact_path: Option<&Path>,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => write_json_payload(&plan_json(prepared, staged_artifact_path)),
        OutputMode::Silent => Ok(()),
        OutputMode::Terminal | OutputMode::Quiet => {
            let rendered = render_plan_human(prepared, staged_artifact_path, context);

            context
                .stdout_renderer()
                .write_stdout(&rendered)
                .map_err(|source| CliError::OutputWrite { source })
        }
    }
}

fn emit_inspect_output(
    context: &CommandContext,
    prepared: &PreparedFitPreview,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => write_json_payload(&inspect_json(prepared)),
        OutputMode::Silent => Ok(()),
        OutputMode::Terminal | OutputMode::Quiet => {
            let rendered = render_inspect_human(prepared, context);

            context
                .stdout_renderer()
                .write_stdout(&rendered)
                .map_err(|source| CliError::OutputWrite { source })
        }
    }
}

fn emit_staged_plan_output(
    context: &CommandContext,
    prepared: &PreparedFitPreview,
    staged_artifact_path: Option<&Path>,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => write_json_payload(&staged_plan_json(prepared, staged_artifact_path)),
        OutputMode::Silent => Ok(()),
        OutputMode::Terminal | OutputMode::Quiet => {
            let rendered = render_staged_plan_human(prepared, staged_artifact_path, context);

            context
                .stdout_renderer()
                .write_stdout(&rendered)
                .map_err(|source| CliError::OutputWrite { source })
        }
    }
}

fn emit_progress(context: &CommandContext, message: &str) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Terminal | OutputMode::Quiet => context
            .stderr_renderer()
            .write_stderr(message)
            .map_err(|source| CliError::OutputWrite { source }),
        OutputMode::Json | OutputMode::Silent => Ok(()),
    }
}
