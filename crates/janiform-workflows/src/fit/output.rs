use std::path::Path;
use std::time::Duration;

use janiform_calibration_runtime::{
    CalibrationStageExecutionStatus, CalibrationStagedExecutionInput, CalibrationStagedRunReport,
};
use janiform_terminal_ui::{
    CompletionSummary, CompletionSummaryStatus, FailureBlock, KeyValueRow, StartSummary,
    format_artifact_path, format_duration_seconds, format_path,
};
use serde_json::Value;
use serde_json::json;

use crate::error::WorkflowError;

use super::events::{failed_stage_label, failure_reason, resume_availability};
use super::execution::FitWorkflowPaths;
use super::scope::workflow_scope_label_for_input;

const FIT_RUN_SCHEMA_VERSION: &str = "janiform.fit_run.v1";
const FIT_RUN_COMMAND_ID: &str = "fit";

/// Human completion view for a fit workflow.
#[derive(Debug)]
pub enum FitCompletionView {
    /// Successful completion summary.
    Success(CompletionSummary),
    /// Failed completion summary.
    Failure(FailureBlock),
}

/// Build the human start summary for a fit workflow.
pub fn fit_start_summary(
    execution_input: &CalibrationStagedExecutionInput,
    input_paths: &[String],
    paths: &FitWorkflowPaths,
) -> StartSummary {
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

    StartSummary {
        title: "Janiform fit".to_string(),
        rows,
    }
}

/// Build the machine completion payload for a fit workflow.
pub fn fit_completion_json_payload(
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    paths: &FitWorkflowPaths,
    event_log_path: &Path,
) -> Value {
    json!({
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
    })
}

/// Build the human completion view for a fit workflow.
pub fn fit_completion_view(
    report: &CalibrationStagedRunReport,
    paths: &FitWorkflowPaths,
    event_log_path: &Path,
    execution_duration: Duration,
) -> Result<FitCompletionView, WorkflowError> {
    if report.status == CalibrationStageExecutionStatus::Success {
        return Ok(FitCompletionView::Success(success_summary(
            report,
            paths,
            event_log_path,
            execution_duration,
        )));
    }

    Ok(FitCompletionView::Failure(failure_summary(
        report,
        paths,
        event_log_path,
    )?))
}

fn success_summary(
    report: &CalibrationStagedRunReport,
    paths: &FitWorkflowPaths,
    event_log_path: &Path,
    execution_duration: Duration,
) -> CompletionSummary {
    CompletionSummary {
        status: CompletionSummaryStatus::Succeeded,
        title: format!(
            "Fit completed in {}",
            format_duration_seconds(execution_duration)
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
    paths: &FitWorkflowPaths,
    event_log_path: &Path,
) -> Result<FailureBlock, WorkflowError> {
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
