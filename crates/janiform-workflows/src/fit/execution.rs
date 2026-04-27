use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use janiform_calibration_artifacts::CalibrationResumeSettings;
use janiform_calibration_artifacts::CalibrationStage;
use janiform_calibration_runtime::{
    CalibrationStageExecutionStatus, CalibrationStageProgressPhase,
    CalibrationStagedExecutionInput, CalibrationStagedRunObserverEvent, CalibrationStagedRunReport,
    MarketDataSessionConfiguration, resolve_market_data_session_configuration,
    run_staged_calibration_from_execution_input_with_observer,
};
use janiform_event_log::{
    EventPersistencePolicy, RunEvent, RunEventLogWriter, SnapshotWritePolicy, WorkflowRef,
    default_run_event_paths,
};
use janiform_market_time::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader};
use janiform_terminal_ui::{
    ProgressAdapter, ProgressEvent, ProgressMeasure, TerminalRenderer, format_path,
};
use serde_json::Value;

use crate::error::WorkflowError;
use crate::progress::VolatileProgressGate;

use super::artifacts::{build_fit_report_payload, build_model_payload, write_pretty_json};
use super::events::{
    append_artifact_event, progress_event_from_staged_event, run_events_from_staged_event,
};
use super::output::{FitCompletionView, fit_completion_view, fit_start_summary};
use super::scope::{workflow_scope_id, workflow_scope_label_for_input};

/// Filesystem paths used by fit workflow execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FitWorkflowPaths {
    /// Run output directory.
    pub output_directory: PathBuf,
    /// Fitted model artifact path.
    pub model_path: PathBuf,
    /// Fit report artifact path.
    pub report_path: PathBuf,
    /// Checkpoint directory.
    pub checkpoint_directory: PathBuf,
}

/// Request for shared fit workflow execution.
#[derive(Debug)]
pub struct FitExecutionRequest {
    /// Prepared staged execution input.
    pub execution_input: CalibrationStagedExecutionInput,
    /// Output artifact paths.
    pub paths: FitWorkflowPaths,
    /// Human-readable input path labels.
    pub input_paths: Vec<String>,
    /// Optional preparation payload included in the fit report.
    pub preparation_payload: Option<Value>,
    /// Optional staged execution input artifact path.
    pub staged_execution_input_path: Option<PathBuf>,
    /// Optional already-resolved session definition JSON.
    pub session_definition_json: Option<String>,
}

/// Optional observers for shared fit workflow execution.
#[derive(Default)]
pub struct FitExecutionObserver<'a> {
    /// Terminal progress adapter.
    pub progress_adapter: Option<&'a mut ProgressAdapter>,
    /// Terminal output for run start and completion summaries.
    pub terminal_output: Option<FitTerminalOutput>,
    /// Sink for machine-readable run events.
    pub machine_event_sink: Option<&'a mut dyn FnMut(&RunEvent) -> Result<(), WorkflowError>>,
}

/// Terminal output policy for a shared fit workflow execution.
#[derive(Clone, Copy, Debug)]
pub struct FitTerminalOutput {
    /// Renderer used for stdout summaries.
    pub renderer: TerminalRenderer,
    /// Whether to emit the start summary.
    pub emit_start_summary: bool,
    /// Whether to emit the completion summary.
    pub emit_completion_summary: bool,
}

impl FitTerminalOutput {
    /// Emit both start and completion summaries through the provided renderer.
    pub const fn start_and_completion(renderer: TerminalRenderer) -> Self {
        Self {
            renderer,
            emit_start_summary: true,
            emit_completion_summary: true,
        }
    }

    /// Emit only the completion summary through the provided renderer.
    pub const fn completion_only(renderer: TerminalRenderer) -> Self {
        Self {
            renderer,
            emit_start_summary: false,
            emit_completion_summary: true,
        }
    }
}

/// Output produced by shared fit workflow execution.
#[derive(Debug)]
pub struct FitExecutionOutput {
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

/// Run a prepared fit workflow through the shared orchestration path.
pub fn run_fit_execution(
    request: FitExecutionRequest,
    mut observer: FitExecutionObserver<'_>,
) -> Result<FitExecutionOutput, WorkflowError> {
    emit_start_summary(
        &observer,
        &request.execution_input,
        &request.input_paths,
        &request.paths,
    )?;

    // Intentional clone because build_execution_plan consumes the execution input,
    // while orchestration still needs the validated input for execution and artifacts.
    let _execution_plan = request
        .execution_input
        .clone()
        .build_execution_plan()
        .map_err(|source| WorkflowError::StagedExecutionPlanFailed { source })?;

    let session_definition_json = resolve_session_definition_json(
        &request.execution_input,
        request.session_definition_json.as_deref(),
    )?;

    let calendar_registry = ExchangeCalendarRegistryLoader::load_from_json(
        session_definition_json.as_str(),
    )
    .map_err(|source| WorkflowError::CalendarRegistryLoadFailed {
        message: source.to_string(),
    })?;

    fs::create_dir_all(request.paths.output_directory.as_path()).map_err(|source| {
        WorkflowError::FitArtifactWrite {
            path: request.paths.output_directory.clone(),
            source,
        }
    })?;

    let event_paths = default_run_event_paths(request.paths.output_directory.as_path());
    let workflow = WorkflowRef::labeled("fit", "Fit");

    let mut event_writer = RunEventLogWriter::create_with_policy(
        &event_paths.event_log_path,
        Some(&event_paths.latest_state_snapshot_path),
        request.execution_input.request.run_id.clone(),
        workflow.clone(),
        fit_event_persistence_policy(),
    )
    .map_err(|source| WorkflowError::FitEventLog { source })?;

    let output_directory = format_path(request.paths.output_directory.as_path());

    let (report, execution_duration) = run_staged_with_observer(
        &request.execution_input,
        &calendar_registry,
        &mut event_writer,
        &workflow,
        request.input_paths.as_slice(),
        output_directory.as_str(),
        &mut observer,
    )?;

    let (report_payload, model_payload) = write_execution_artifacts(
        &request.paths,
        &request.execution_input,
        &report,
        request.preparation_payload.as_ref(),
        request.staged_execution_input_path.as_deref(),
        &mut event_writer,
        &workflow,
        &mut observer.progress_adapter,
    )?;

    event_writer
        .flush()
        .map_err(|source| WorkflowError::FitEventLog { source })?;

    let output = FitExecutionOutput {
        report,
        report_payload,
        model_payload,
        event_log_path: event_paths.event_log_path,
        latest_state_snapshot_path: event_paths.latest_state_snapshot_path,
        execution_duration,
    };

    finish_progress(&mut observer.progress_adapter)?;

    emit_completion_summary(&observer, &request.paths, &output)?;

    Ok(output)
}

/// Resolve standard fit output paths from an output directory.
pub fn fit_workflow_paths(output_directory: PathBuf) -> FitWorkflowPaths {
    FitWorkflowPaths {
        model_path: output_directory.join("artifacts").join("model.jf"),
        report_path: output_directory.join("artifacts").join("report.json"),
        checkpoint_directory: output_directory.join("checkpoints"),
        output_directory,
    }
}

/// Resolve fit output paths for a staged execution input.
pub fn staged_execution_paths(
    execution_input: &CalibrationStagedExecutionInput,
    default_output_directory: PathBuf,
) -> FitWorkflowPaths {
    let checkpoint_directory = execution_input
        .runtime_resources
        .as_ref()
        .and_then(|resources| resources.disk_checkpoint_directory.as_ref())
        .map(PathBuf::from);

    let output_directory = checkpoint_directory
        .as_ref()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or(default_output_directory);

    let checkpoint_directory =
        checkpoint_directory.unwrap_or_else(|| output_directory.join("checkpoints"));

    fit_workflow_paths_with_checkpoint_directory(output_directory, checkpoint_directory)
}

/// Apply staged resume settings to an execution input.
pub fn apply_resume_settings(
    mut execution_input: CalibrationStagedExecutionInput,
    checkpoint_reference: Option<&str>,
    resume_stage: Option<CalibrationStage>,
) -> CalibrationStagedExecutionInput {
    if let Some(checkpoint_reference) = checkpoint_reference {
        execution_input.request.resume_settings =
            Some(CalibrationResumeSettings::persisted_checkpoint_reference(
                checkpoint_reference.to_string(),
                resume_stage,
            ));
    }

    execution_input
}

fn fit_workflow_paths_with_checkpoint_directory(
    output_directory: PathBuf,
    checkpoint_directory: PathBuf,
) -> FitWorkflowPaths {
    FitWorkflowPaths {
        model_path: output_directory.join("artifacts").join("model.jf"),
        report_path: output_directory.join("artifacts").join("report.json"),
        checkpoint_directory,
        output_directory,
    }
}

fn emit_start_summary(
    observer: &FitExecutionObserver<'_>,
    execution_input: &CalibrationStagedExecutionInput,
    input_paths: &[String],
    paths: &FitWorkflowPaths,
) -> Result<(), WorkflowError> {
    let Some(terminal_output) = observer.terminal_output else {
        return Ok(());
    };
    if !terminal_output.emit_start_summary {
        return Ok(());
    }

    let rendered =
        fit_start_summary(execution_input, input_paths, paths).render(&terminal_output.renderer);

    terminal_output
        .renderer
        .write_stdout(&format!("\n{rendered}"))
        .map_err(|source| WorkflowError::OutputWrite { source })
}

fn emit_completion_summary(
    observer: &FitExecutionObserver<'_>,
    paths: &FitWorkflowPaths,
    output: &FitExecutionOutput,
) -> Result<(), WorkflowError> {
    let Some(terminal_output) = observer.terminal_output else {
        return Ok(());
    };
    if !terminal_output.emit_completion_summary {
        return Ok(());
    }

    let rendered = match fit_completion_view(
        &output.report,
        paths,
        output.event_log_path.as_path(),
        output.execution_duration,
    )? {
        FitCompletionView::Success(summary) => summary.render(&terminal_output.renderer),
        FitCompletionView::Failure(summary) => summary.render(&terminal_output.renderer),
    };

    terminal_output
        .renderer
        .write_stdout(&format!("\n{rendered}"))
        .map_err(|source| WorkflowError::OutputWrite { source })
}

fn resolve_session_definition_json(
    execution_input: &CalibrationStagedExecutionInput,
    session_definition_json: Option<&str>,
) -> Result<String, WorkflowError> {
    if let Some(session_definition_json) = session_definition_json {
        return Ok(session_definition_json.to_string());
    }

    let session_resolution = resolve_market_data_session_configuration(
        MarketDataSessionConfiguration {
            exchange_code: execution_input.request.data_inputs.exchange_code.clone(),
            session_definition_source: None,
            session_timezone: None,
        },
        None,
    )
    .map_err(|source| WorkflowError::FitExecutionFailed {
        message: format!("failed to resolve staged calendar registry: {source}"),
    })?;

    Ok(session_resolution.resolved_session_definition_json)
}

fn run_staged_with_observer(
    execution_input: &CalibrationStagedExecutionInput,
    calendar_registry: &ExchangeCalendarRegistry,
    event_writer: &mut RunEventLogWriter,
    workflow: &WorkflowRef,
    input_paths: &[String],
    output_directory: &str,
    observer: &mut FitExecutionObserver<'_>,
) -> Result<(CalibrationStagedRunReport, Duration), WorkflowError> {
    let scope_id = workflow_scope_id(execution_input);
    let scope_label = workflow_scope_label_for_input(execution_input);
    let mut observer_error: Option<WorkflowError> = None;
    let mut progress_gate = VolatileProgressGate::default();

    let mut staged_observer = |event: CalibrationStagedRunObserverEvent| {
        if observer_error.is_some() {
            return;
        }

        if !should_observe_staged_event(&mut progress_gate, &event) {
            return;
        }

        if let Err(error) = observe_progress(&mut observer.progress_adapter, &event) {
            observer_error = Some(error);
            return;
        }

        if let Err(error) = observe_run_events(
            event_writer,
            workflow,
            scope_id.as_str(),
            scope_label,
            input_paths,
            output_directory,
            &mut observer.machine_event_sink,
            &event,
        ) {
            observer_error = Some(error);
        }
    };

    let execution_started_at = Instant::now();

    // The runtime consumes execution input, while observers and artifact code use
    // the validated request after the run completes.
    let run_result = run_staged_calibration_from_execution_input_with_observer(
        execution_input.clone(),
        calendar_registry,
        None,
        &mut staged_observer,
    );
    let execution_duration = execution_started_at.elapsed();

    drop(staged_observer);

    if let Some(observer_error) = observer_error {
        return Err(observer_error);
    }

    let report =
        run_result.map_err(|source| WorkflowError::StagedExecutionPlanFailed { source })?;

    Ok((report, execution_duration))
}

fn fit_event_persistence_policy() -> EventPersistencePolicy {
    EventPersistencePolicy {
        snapshot_policy: SnapshotWritePolicy::DurableBoundariesOnly,
    }
}

fn should_observe_staged_event(
    progress_gate: &mut VolatileProgressGate<(CalibrationStage, CalibrationStageProgressPhase)>,
    event: &CalibrationStagedRunObserverEvent,
) -> bool {
    let CalibrationStagedRunObserverEvent::StageProgress {
        stage,
        progress_phase,
        completed_units,
        total_units,
        current_item_label,
        planned_work_item_labels,
        current_evaluation_count,
        evaluation_progress_percent,
        ..
    } = event
    else {
        return true;
    };

    if *evaluation_progress_percent == Some(100) {
        return true;
    }
    if !planned_work_item_labels.is_empty() {
        return true;
    }
    if current_item_label.is_some()
        && *current_evaluation_count == Some(0)
        && evaluation_progress_percent.is_none()
    {
        return true;
    }

    progress_gate.should_emit(
        Instant::now(),
        (*stage, *progress_phase),
        *completed_units,
        *total_units,
    )
}

fn observe_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    event: &CalibrationStagedRunObserverEvent,
) -> Result<(), WorkflowError> {
    let Some(progress_event) = progress_event_from_staged_event(event)? else {
        return Ok(());
    };

    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(progress_event);
    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

fn observe_output_materialization_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    latest_signal: &str,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunProgress {
        phase: "finalizing outputs".to_string(),
        measure: ProgressMeasure::Unbounded,
        latest_signal: Some(latest_signal.to_string()),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

fn finish_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<(), WorkflowError> {
    if let Some(progress_adapter) = progress_adapter.as_deref_mut() {
        progress_adapter
            .finish_and_keep()
            .map_err(|source| WorkflowError::OutputWrite { source })?;
    }

    Ok(())
}

fn observe_run_events(
    event_writer: &mut RunEventLogWriter,
    workflow: &WorkflowRef,
    scope_id: &str,
    scope_label: &str,
    input_paths: &[String],
    output_directory: &str,
    machine_event_sink: &mut Option<&mut dyn FnMut(&RunEvent) -> Result<(), WorkflowError>>,
    event: &CalibrationStagedRunObserverEvent,
) -> Result<(), WorkflowError> {
    let run_events = run_events_from_staged_event(
        event,
        workflow.clone(),
        scope_id,
        scope_label,
        input_paths,
        output_directory,
    )?;

    for run_event in run_events {
        event_writer
            .append_event(&run_event)
            .map_err(|source| WorkflowError::FitEventLog { source })?;

        if let Some(machine_event_sink) = machine_event_sink.as_deref_mut() {
            machine_event_sink(&run_event)?;
        }
    }

    Ok(())
}

fn write_execution_artifacts(
    paths: &FitWorkflowPaths,
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    preparation_payload: Option<&Value>,
    staged_execution_input_path: Option<&Path>,
    event_writer: &mut RunEventLogWriter,
    workflow: &WorkflowRef,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<(Value, Option<Value>), WorkflowError> {
    observe_output_materialization_progress(progress_adapter, "building fit report")?;

    let report_payload = build_fit_report_payload(
        execution_input,
        report,
        preparation_payload,
        staged_execution_input_path,
    )?;

    observe_output_materialization_progress(progress_adapter, "writing fit report")?;

    write_pretty_json(paths.report_path.as_path(), &report_payload)?;

    observe_output_materialization_progress(progress_adapter, "recording fit report artifact")?;

    append_artifact_event(
        event_writer,
        workflow.clone(),
        execution_input.request.run_id.as_str(),
        "fit_report",
        paths.report_path.as_path(),
        None,
    )?;

    if report.status != CalibrationStageExecutionStatus::Success {
        return Ok((report_payload, None));
    }

    observe_output_materialization_progress(progress_adapter, "building fitted model")?;

    let model_payload = build_model_payload(
        execution_input,
        report,
        &report_payload,
        preparation_payload,
    )?;

    observe_output_materialization_progress(progress_adapter, "writing fitted model")?;

    write_pretty_json(paths.model_path.as_path(), &model_payload)?;

    observe_output_materialization_progress(progress_adapter, "recording model artifact")?;

    append_artifact_event(
        event_writer,
        workflow.clone(),
        execution_input.request.run_id.as_str(),
        "model",
        paths.model_path.as_path(),
        None,
    )?;

    Ok((report_payload, Some(model_payload)))
}
