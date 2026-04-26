use std::path::Path;
use std::time::Duration;

use janiform::calibration::{
    CalibrationStageExecutionStatus, CalibrationStagedExecutionInput, CalibrationStagedRunReport,
};
use janiform_cli_ui::{
    CompletionSummary, CompletionSummaryStatus, FailureBlock, KeyValueRow, StartSummary,
    format_artifact_path, format_path,
};
use serde_json::json;

use super::events::{failed_stage_label, failure_reason, resume_availability};
use super::scope::workflow_scope_label_for_input;
use crate::commands::fit::payloads::FIT_RUN_COMMAND_ID;
use crate::commands::fit::payloads::FIT_RUN_SCHEMA_VERSION;
use crate::commands::fit::preparation::FitPreviewPaths;
use crate::commands::shared::output::format_elapsed_duration;
use crate::commands::shared::output::write_json_payload;
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;

pub(super) fn emit_start_summary(
    context: &CommandContext,
    execution_input: &CalibrationStagedExecutionInput,
    input_paths: &[String],
    paths: &FitPreviewPaths,
) -> Result<(), CliError> {
    if context.output_mode() != OutputMode::Human {
        return Ok(());
    }

    let mut rows = vec![
        KeyValueRow::new("Run", execution_input.request.run_id.clone()),
        KeyValueRow::new("Scope", workflow_scope_label_for_input(execution_input)),
    ];
    for input in input_paths {
        rows.push(KeyValueRow::new("Input", input.clone()));
    }
    rows.push(KeyValueRow::new(
        "Known news",
        known_news_participation(execution_input),
    ));
    rows.push(KeyValueRow::new(
        "Options",
        option_participation(execution_input),
    ));
    rows.push(KeyValueRow::new(
        "Output",
        format_path(paths.output_directory.as_path()),
    ));

    let rendered = StartSummary {
        title: "Janiform fit".to_string(),
        rows,
    }
    .render(&context.stdout_renderer());

    context
        .stdout_renderer()
        .write_stdout(&rendered)
        .map_err(|source| CliError::OutputWrite { source })
}

pub(super) fn emit_completion_output(
    context: &CommandContext,
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    paths: &FitPreviewPaths,
    event_log_path: &Path,
    execution_duration: Duration,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => write_json_payload(&json!({
            "schema_version": FIT_RUN_SCHEMA_VERSION,
            "command_id": FIT_RUN_COMMAND_ID,
            "run_id": execution_input.request.run_id,
            "status": report.status,
            "model_path": (report.status == CalibrationStageExecutionStatus::Success).then_some(&paths.model_path),
            "report_path": paths.report_path,
            "event_log_path": event_log_path,
            "checkpoint_count": report.emitted_checkpoints.len(),
            "resume": {
                "resumed_from_checkpoint_reference": report.resumed_from_checkpoint_reference,
                "resumed_from_last_successful_stage": report.resumed_from_last_successful_stage,
                "started_from_stage": report.started_from_stage,
            },
            "failure": report.failure,
        })),
        OutputMode::Silent => Ok(()),
        OutputMode::Human | OutputMode::Quiet => {
            let rendered = if report.status == CalibrationStageExecutionStatus::Success {
                success_summary(report, paths, event_log_path, execution_duration)
                    .render(&context.stdout_renderer())
            } else {
                failure_summary(report, paths, event_log_path)?.render(&context.stdout_renderer())
            };

            context
                .stdout_renderer()
                .write_stdout(&rendered)
                .map_err(|source| CliError::OutputWrite { source })
        }
    }
}

fn success_summary(
    report: &CalibrationStagedRunReport,
    paths: &FitPreviewPaths,
    event_log_path: &Path,
    execution_duration: Duration,
) -> CompletionSummary {
    CompletionSummary {
        status: CompletionSummaryStatus::Succeeded,
        title: format!(
            "Fit completed in {}",
            format_elapsed_duration(execution_duration)
        ),
        rows: vec![
            KeyValueRow::new("Model", format_artifact_path(paths.model_path.as_path())),
            KeyValueRow::new("Report", format_artifact_path(paths.report_path.as_path())),
            KeyValueRow::new("Events", format_artifact_path(event_log_path)),
            KeyValueRow::new("Checkpoints", report.emitted_checkpoints.len().to_string()),
            KeyValueRow::new(
                "Resume",
                report
                    .resumed_from_checkpoint_reference
                    .as_deref()
                    .unwrap_or("not used")
                    .to_string(),
            ),
        ],
    }
}

fn failure_summary(
    report: &CalibrationStagedRunReport,
    paths: &FitPreviewPaths,
    event_log_path: &Path,
) -> Result<FailureBlock, CliError> {
    Ok(FailureBlock {
        title: "Fit failed".to_string(),
        reason: failure_reason(report)?,
        details: vec![
            KeyValueRow::new("Stage", failed_stage_label(report)),
            KeyValueRow::new("Report", format_artifact_path(paths.report_path.as_path())),
            KeyValueRow::new("Events", format_artifact_path(event_log_path)),
            KeyValueRow::new("Checkpoints", report.emitted_checkpoints.len().to_string()),
            KeyValueRow::new("Resume", resume_availability(report)),
        ],
    })
}

fn known_news_participation(execution_input: &CalibrationStagedExecutionInput) -> String {
    execution_input
        .seasonality_stage_input
        .as_ref()
        .map(|input| input.scheduled_event_overlays.len())
        .filter(|count| *count > 0)
        .map(|count| format!("{count} overlays"))
        .unwrap_or_else(|| "none".to_string())
}

fn option_participation(execution_input: &CalibrationStagedExecutionInput) -> String {
    execution_input
        .joint_refinement_option_pricing_input
        .as_ref()
        .map(|input| format!("{} nodes", input.option_snapshot_nodes.len()))
        .unwrap_or_else(|| "none".to_string())
}
