use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use janiform_calibration_runtime::{
    CalibrationMarketDataPreparationRequest, CalibrationStageExecutionStatus,
    CalibrationStagedExecutionInput, CalibrationStagedRunReport, CalibrationStagedRunStageReport,
};
use janiform_terminal_ui::{
    CompletionSummary, CompletionSummaryStatus, FailureBlock, KeyValueRow, StartSummary,
    format_artifact_path, format_elapsed_label, format_path,
};
use serde_json::{Value, json};

use crate::error::WorkflowError;
use crate::price_coordinates::price_coordinate_resolution_summary;

use super::events::{failed_stage_label, failure_reason, resume_availability};
use super::input_paths::preparation_request_input_paths;
use super::labels::{fit_workflow_id, workflow_scope_label};
use super::request::FitWorkflowPaths;
use super::scope::workflow_scope_label_for_input;

const FIT_RUN_SCHEMA_VERSION: &str = "janiform.fit_run.v1";

/// Output produced by shared fit workflow execution.
#[derive(Debug)]
pub struct FitWorkflowOutput {
    /// Staged calibration report.
    pub report: CalibrationStagedRunReport,
    /// Fit report artifact payload.
    pub report_payload: Value,
    /// Model artifact payload when the fit succeeded.
    pub model_payload: Option<Value>,
    /// Durable event-log path.
    pub event_log_path: PathBuf,
    /// Latest-state snapshot path.
    pub latest_state_snapshot_path: PathBuf,
    /// Wall-clock execution duration.
    pub execution_duration: Duration,
}

/// Terminal completion view for a fit workflow.
#[derive(Debug)]
pub(super) enum FitCompletionView {
    /// Successful completion summary.
    Success(CompletionSummary),
    /// Failed completion summary.
    Failure(FailureBlock),
}

/// Build the terminal start summary for a fit workflow.
pub(super) fn fit_start_summary(
    execution_input: &CalibrationStagedExecutionInput,
    input_paths: &[String],
    paths: &FitWorkflowPaths,
) -> StartSummary {
    fit_start_summary_from_parts(
        execution_input.request.run_id.as_str(),
        workflow_scope_label_for_input(execution_input),
        input_paths,
        known_news_participation(execution_input),
        option_participation(execution_input),
        paths,
    )
}

/// Build the terminal start summary once preparation inputs are resolved.
pub fn fit_preparation_start_summary(
    preparation_request: &CalibrationMarketDataPreparationRequest,
    paths: &FitWorkflowPaths,
) -> StartSummary {
    let input_paths = preparation_request_input_paths(preparation_request);

    fit_start_summary_from_parts(
        preparation_request.run_id.as_str(),
        workflow_scope_label(preparation_request.workflow_scope),
        input_paths.as_slice(),
        preparation_known_news_participation(preparation_request),
        preparation_option_participation(preparation_request),
        paths,
    )
}

/// Build the machine completion payload for a fit workflow.
pub fn fit_completion_json_payload(
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    paths: &FitWorkflowPaths,
    event_log_path: &Path,
) -> Value {
    let warnings = fit_warning_messages(report);

    json!({
        "schema_version": FIT_RUN_SCHEMA_VERSION,
        "command_id": fit_workflow_id(),
        "run_id": execution_input.request.run_id,
        "status": report.status,
        "model_path": report.status.is_successful().then_some(&paths.model_path),
        "report_path": paths.report_path,
        "event_log_path": event_log_path,
        "checkpoint_count": report.emitted_checkpoints.len(),
        "resume": {
            "resumed_from_checkpoint_reference": report.resumed_from_checkpoint_reference,
            "resumed_from_last_successful_stage": report.resumed_from_last_successful_stage,
            "started_from_stage": report.started_from_stage,
        },
        "failure": report.failure,
        "warnings": warnings,
    })
}

/// Build the terminal completion view for a fit workflow.
pub(super) fn fit_completion_view(
    report: &CalibrationStagedRunReport,
    report_payload: &Value,
    paths: &FitWorkflowPaths,
    event_log_path: &Path,
    execution_duration: Duration,
) -> Result<FitCompletionView, WorkflowError> {
    if report.status.is_successful() {
        return Ok(FitCompletionView::Success(success_summary(
            report,
            report_payload,
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
    report_payload: &Value,
    paths: &FitWorkflowPaths,
    event_log_path: &Path,
    execution_duration: Duration,
) -> CompletionSummary {
    let warning_messages = combined_fit_warning_messages(report, report_payload);

    let mut rows = vec![
        KeyValueRow::new("Model", format_artifact_path(paths.model_path.as_path())),
        KeyValueRow::new("Report", format_artifact_path(paths.report_path.as_path())),
        KeyValueRow::new("Events", format_artifact_path(event_log_path)),
        KeyValueRow::new("Checkpoints", report.emitted_checkpoints.len().to_string()),
        KeyValueRow::new("Resume", resume_availability(report)),
    ];

    if let Some(price_coordinate_summary) = report_payload
        .get("preparation")
        .and_then(|preparation| preparation.get("price_coordinate_resolution"))
        .and_then(price_coordinate_resolution_summary)
    {
        rows.push(KeyValueRow::block(
            "Price coordinates",
            price_coordinate_summary,
        ));
    }

    if !warning_messages.is_empty() {
        rows.push(KeyValueRow::block("Warnings", warning_messages.join("\n")));
    }

    CompletionSummary {
        status: if report.status == CalibrationStageExecutionStatus::SuccessWithWarnings
            || !warning_messages.is_empty()
        {
            CompletionSummaryStatus::SucceededWithWarnings
        } else {
            CompletionSummaryStatus::Succeeded
        },
        title: format!(
            "Fit completed in {}",
            format_elapsed_label(execution_duration)
        ),
        rows,
    }
}

fn fit_warning_messages(report: &CalibrationStagedRunReport) -> Vec<String> {
    let mut warnings = Vec::new();

    for stage_report in &report.stage_reports {
        for warning in stage_report_warning_messages(stage_report) {
            if !warnings.contains(warning) {
                warnings.push(warning.clone());
            }
        }
    }

    warnings
}

fn combined_fit_warning_messages(
    report: &CalibrationStagedRunReport,
    report_payload: &Value,
) -> Vec<String> {
    let mut warnings = fit_warning_messages(report);

    if let Some(preparation_warnings) = report_payload
        .get("preparation")
        .and_then(|preparation| preparation.get("warnings"))
        .and_then(Value::as_array)
    {
        for warning in preparation_warnings {
            let Some(warning) = warning.as_str() else {
                continue;
            };
            let warning = warning.to_string();
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
    }

    warnings
}

fn stage_report_warning_messages(stage_report: &CalibrationStagedRunStageReport) -> &[String] {
    match stage_report {
        CalibrationStagedRunStageReport::JointRefinement(report)
        | CalibrationStagedRunStageReport::OptionRefinement(report)
        | CalibrationStagedRunStageReport::Finalization(report) => report.warnings.as_slice(),
        CalibrationStagedRunStageReport::ReturnBackfit(report) => report
            .fitted_stage_payload
            .as_ref()
            .map(|payload| payload.diagnostics.warnings.as_slice())
            .unwrap_or(&[]),
        CalibrationStagedRunStageReport::Seasonality(_)
        | CalibrationStagedRunStageReport::Hawkes(_)
        | CalibrationStagedRunStageReport::ObservationModels(_)
        | CalibrationStagedRunStageReport::ReturnComponents(_)
        | CalibrationStagedRunStageReport::LongVariance(_)
        | CalibrationStagedRunStageReport::NewsShocks(_)
        | CalibrationStagedRunStageReport::MeanReversion(_) => &[],
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

fn fit_start_summary_from_parts(
    run_id: &str,
    scope: &str,
    input_paths: &[String],
    known_news_participation: String,
    option_participation: String,
    paths: &FitWorkflowPaths,
) -> StartSummary {
    let mut rows = vec![
        KeyValueRow::new("Run", run_id),
        KeyValueRow::new("Scope", scope),
    ];

    if !input_paths.is_empty() {
        rows.push(KeyValueRow::block(
            "Inputs",
            input_paths_summary(input_paths),
        ));
    }

    rows.push(KeyValueRow::new("Known news", known_news_participation));
    rows.push(KeyValueRow::new("Options", option_participation));
    rows.push(KeyValueRow::new(
        "Output",
        format_path(paths.output_directory.as_path()),
    ));

    StartSummary {
        title: "Janiform fit".to_string(),
        rows,
    }
}

fn known_news_participation(execution_input: &CalibrationStagedExecutionInput) -> String {
    execution_input
        .return_components_candidate_generation_input
        .as_ref()
        .map(|input| {
            input
                .known_news_calendar
                .scheduled_event_timestamps_utc_nanos
                .len()
        })
        .filter(|count| *count > 0)
        .map(|count| format!("{count} events"))
        .unwrap_or_else(|| "none".to_string())
}

fn option_participation(execution_input: &CalibrationStagedExecutionInput) -> String {
    execution_input
        .joint_refinement_option_pricing_input
        .as_ref()
        .map(|input| format!("{} nodes", input.option_snapshot_nodes.len()))
        .unwrap_or_else(|| "none".to_string())
}

fn preparation_known_news_participation(
    preparation_request: &CalibrationMarketDataPreparationRequest,
) -> String {
    preparation_request
        .known_news_inputs
        .as_ref()
        .map(|_| "configured".to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn preparation_option_participation(
    preparation_request: &CalibrationMarketDataPreparationRequest,
) -> String {
    preparation_request
        .option_surface_source
        .as_ref()
        .map(|_| "configured".to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn input_paths_summary(input_paths: &[String]) -> String {
    let input_entries = input_paths
        .iter()
        .map(|input_path| input_path_parts(input_path))
        .collect::<Vec<_>>();
    let label_width = input_entries
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or_default();

    input_entries
        .into_iter()
        .map(|(label, path)| format!("{label:<label_width$}: {path}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn input_path_parts(input_path: &str) -> (&str, &str) {
    input_path.split_once(": ").unwrap_or(("input", input_path))
}

#[cfg(test)]
mod tests;
