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
    EventPersistencePolicy, RunEventLogWriter, SnapshotWritePolicy, WorkflowRef,
    default_run_event_paths,
};
use janiform_market_time::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader};
use janiform_terminal_ui::{ProgressAdapter, ProgressEvent, ProgressMeasure, format_path};
use serde_json::Value;

use crate::error::WorkflowError;
use crate::progress::VolatileProgressGate;
use crate::terminal::{emit_terminal_completion_view, emit_terminal_start_view};

use super::artifacts::{build_fit_report_payload, build_model_payload, write_pretty_json};
use super::events::{
    append_artifact_event, progress_event_from_staged_event, run_events_from_staged_event,
};
use super::output::FitWorkflowOutput;
use super::output::{FitCompletionView, fit_completion_view, fit_start_summary};
use super::request::{FitWorkflowPaths, StagedFitExecutionWorkflowRequest};
use super::scope::{workflow_scope_id, workflow_scope_label_for_input};
use super::workflow::{FitRunEventSink, FitWorkflowObserver};

struct RunEventObservationInput<'a, 'sink> {
    event_writer: &'a mut RunEventLogWriter,
    workflow: &'a WorkflowRef,
    scope_id: &'a str,
    scope_label: &'a str,
    input_paths: &'a [String],
    output_directory: &'a str,
    machine_event_sink: &'a mut Option<FitRunEventSink<'sink>>,
    event: &'a CalibrationStagedRunObserverEvent,
}

struct ExecutionArtifactsWriteInput<'a, 'progress> {
    paths: &'a FitWorkflowPaths,
    execution_input: &'a CalibrationStagedExecutionInput,
    report: &'a CalibrationStagedRunReport,
    preparation_payload: Option<&'a Value>,
    staged_execution_input_path: Option<&'a Path>,
    event_writer: &'a mut RunEventLogWriter,
    workflow: &'a WorkflowRef,
    progress_adapter: &'a mut Option<&'progress mut ProgressAdapter>,
}

/// Run staged fit execution through the shared orchestration path.
pub fn run_staged_fit_execution_workflow(
    request: StagedFitExecutionWorkflowRequest,
    mut observer: FitWorkflowObserver<'_>,
) -> Result<FitWorkflowOutput, WorkflowError> {
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

    let (report_payload, model_payload) =
        write_execution_artifacts(ExecutionArtifactsWriteInput {
            paths: &request.paths,
            execution_input: &request.execution_input,
            report: &report,
            preparation_payload: request.preparation_payload.as_ref(),
            staged_execution_input_path: request.staged_execution_input_path.as_deref(),
            event_writer: &mut event_writer,
            workflow: &workflow,
            progress_adapter: &mut observer.progress_adapter,
        })?;

    event_writer
        .flush()
        .map_err(|source| WorkflowError::FitEventLog { source })?;

    let output = FitWorkflowOutput {
        report,
        report_payload,
        model_payload,
        event_log_path: event_paths.event_log_path,
        latest_state_snapshot_path: event_paths.latest_state_snapshot_path,
        execution_duration,
    };

    complete_progress(&mut observer.progress_adapter, output.report.status)?;
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
    let persistence_root_directory = execution_input
        .runtime_resources
        .as_ref()
        .and_then(|resources| resources.disk_checkpoint_directory.as_ref())
        .map(PathBuf::from);

    let output_directory = persistence_root_directory.unwrap_or(default_output_directory);

    fit_workflow_paths(output_directory)
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

fn emit_start_summary(
    observer: &FitWorkflowObserver<'_>,
    execution_input: &CalibrationStagedExecutionInput,
    input_paths: &[String],
    paths: &FitWorkflowPaths,
) -> Result<(), WorkflowError> {
    emit_terminal_start_view(&observer.terminal_output, |terminal_output| {
        Ok(
            fit_start_summary(execution_input, input_paths, paths)
                .render(&terminal_output.renderer),
        )
    })
}

fn emit_completion_summary(
    observer: &FitWorkflowObserver<'_>,
    paths: &FitWorkflowPaths,
    output: &FitWorkflowOutput,
) -> Result<(), WorkflowError> {
    emit_terminal_completion_view(&observer.terminal_output, |terminal_output| {
        let rendered = match fit_completion_view(
            &output.report,
            paths,
            output.event_log_path.as_path(),
            output.execution_duration,
        )? {
            FitCompletionView::Success(summary) => summary.render(&terminal_output.renderer),
            FitCompletionView::Failure(summary) => summary.render(&terminal_output.renderer),
        };

        Ok(rendered)
    })
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
    observer: &mut FitWorkflowObserver<'_>,
) -> Result<(CalibrationStagedRunReport, Duration), WorkflowError> {
    let scope_id = workflow_scope_id(execution_input);
    let scope_label = workflow_scope_label_for_input(execution_input);
    let mut observer_error: Option<WorkflowError> = None;
    let mut progress_gate = VolatileProgressGate::default();

    let execution_started_at = Instant::now();

    // The runtime consumes execution input, while observers and artifact code use
    // the validated request after the run completes.
    let run_result = {
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

            if let Err(error) = observe_run_events(RunEventObservationInput {
                event_writer,
                workflow,
                scope_id: scope_id.as_str(),
                scope_label,
                input_paths,
                output_directory,
                machine_event_sink: &mut observer.machine_event_sink,
                event: &event,
            }) {
                observer_error = Some(error);
            }
        };

        run_staged_calibration_from_execution_input_with_observer(
            execution_input.clone(),
            calendar_registry,
            None,
            &mut staged_observer,
        )
    };
    let execution_duration = execution_started_at.elapsed();

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
    let CalibrationStagedRunObserverEvent::StageProgress(progress) = event else {
        return true;
    };

    if progress.evaluation_progress_percent == Some(100) {
        return true;
    }

    if progress.restart_evaluation_timing_diagnostic.is_some() {
        return true;
    }

    if !progress.planned_work_item_labels.is_empty() {
        return true;
    }

    if progress.current_item_label.is_some()
        && progress.current_evaluation_count.is_none()
        && progress.evaluation_progress_percent.is_none()
    {
        return true;
    }

    if progress.current_item_label.is_some() && progress.evaluation_progress_percent == Some(0) {
        return true;
    }

    if progress.current_item_label.is_some()
        && progress.current_parameter_block.is_some()
        && progress.evaluation_progress_percent.is_none()
    {
        return true;
    }

    progress_gate.should_emit(
        Instant::now(),
        (progress.stage, progress.progress_phase),
        progress.completed_units,
        progress.total_units,
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

fn complete_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    status: CalibrationStageExecutionStatus,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunCompleted {
        status: terminal_progress_status(status).to_string(),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

const fn terminal_progress_status(status: CalibrationStageExecutionStatus) -> &'static str {
    match status {
        CalibrationStageExecutionStatus::Success => "completed",
        CalibrationStageExecutionStatus::SuccessWithWarnings => "completed with warnings",
        CalibrationStageExecutionStatus::Failure => "failed",
    }
}

fn observe_run_events(input: RunEventObservationInput<'_, '_>) -> Result<(), WorkflowError> {
    let run_events = run_events_from_staged_event(
        input.event,
        input.workflow.clone(),
        input.scope_id,
        input.scope_label,
        input.input_paths,
        input.output_directory,
    )?;

    for run_event in run_events {
        input
            .event_writer
            .append_event(&run_event)
            .map_err(|source| WorkflowError::FitEventLog { source })?;

        if let Some(machine_event_sink) = input.machine_event_sink.as_deref_mut() {
            machine_event_sink(&run_event)?;
        }
    }

    if let CalibrationStagedRunObserverEvent::StageProgress(progress) = input.event
        && progress.restart_evaluation_timing_diagnostic.is_some()
    {
        input
            .event_writer
            .flush()
            .map_err(|source| WorkflowError::FitEventLog { source })?;
    }

    Ok(())
}

fn write_execution_artifacts(
    input: ExecutionArtifactsWriteInput<'_, '_>,
) -> Result<(Value, Option<Value>), WorkflowError> {
    observe_output_materialization_progress(input.progress_adapter, "building fit report")?;

    let report_payload = build_fit_report_payload(
        input.execution_input,
        input.report,
        input.preparation_payload,
        input.staged_execution_input_path,
    )?;

    observe_output_materialization_progress(input.progress_adapter, "writing fit report")?;

    write_pretty_json(input.paths.report_path.as_path(), &report_payload)?;

    observe_output_materialization_progress(
        input.progress_adapter,
        "recording fit report artifact",
    )?;

    append_artifact_event(
        input.event_writer,
        input.workflow.clone(),
        input.execution_input.request.run_id.as_str(),
        "fit_report",
        input.paths.report_path.as_path(),
        None,
    )?;

    if !input.report.status.is_successful() {
        return Ok((report_payload, None));
    }

    observe_output_materialization_progress(input.progress_adapter, "building fitted model")?;

    let model_payload = build_model_payload(
        input.execution_input,
        input.report,
        &report_payload,
        input.preparation_payload,
    )?;

    observe_output_materialization_progress(input.progress_adapter, "writing fitted model")?;

    write_pretty_json(input.paths.model_path.as_path(), &model_payload)?;

    observe_output_materialization_progress(input.progress_adapter, "recording model artifact")?;

    append_artifact_event(
        input.event_writer,
        input.workflow.clone(),
        input.execution_input.request.run_id.as_str(),
        "model",
        input.paths.model_path.as_path(),
        None,
    )?;

    Ok((report_payload, Some(model_payload)))
}

#[cfg(test)]
mod tests {
    use janiform_calibration_runtime::JointRefinementStageProgressPhase;
    use janiform_calibration_runtime::ReturnComponentsStageProgressPhase;

    use super::*;

    #[test]
    fn progress_gate_observes_labelled_phase_start_without_eval_metrics() {
        let mut progress_gate = VolatileProgressGate::default();

        assert!(should_observe_staged_event(
            &mut progress_gate,
            &return_components_stage_progress_event(
                ReturnComponentsStageProgressPhase::ContinuousNigProxyEmSeedGeneration,
                1,
                1,
                Some("proxy EM search points ready"),
                None,
                None,
            )
        ));
        assert!(should_observe_staged_event(
            &mut progress_gate,
            &return_components_stage_progress_event(
                ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
                0,
                12,
                Some("score proxy EM search points"),
                None,
                None,
            )
        ));
    }

    #[test]
    fn progress_gate_observes_restart_block_start_without_eval_metrics() {
        let mut progress_gate = VolatileProgressGate::default();

        assert!(should_observe_staged_event(
            &mut progress_gate,
            &joint_refinement_progress_event(
                JointRefinementStageProgressPhase::RestartOptimization,
                Some("baseline"),
                None,
                Some(1),
                Some(1),
            )
        ));
        assert!(should_observe_staged_event(
            &mut progress_gate,
            &joint_refinement_progress_event(
                JointRefinementStageProgressPhase::RestartOptimization,
                Some("midpoint"),
                Some("nig"),
                Some(0),
                None,
            )
        ));
    }

    #[test]
    fn progress_gate_observes_active_option_work_item_start_after_phase_start() {
        let mut progress_gate = VolatileProgressGate::default();

        assert!(should_observe_staged_event(
            &mut progress_gate,
            &joint_refinement_progress_event(
                JointRefinementStageProgressPhase::OptionSearchTierRefinement,
                None,
                None,
                None,
                None,
            )
        ));
        assert!(should_observe_staged_event(
            &mut progress_gate,
            &joint_refinement_progress_event(
                JointRefinementStageProgressPhase::OptionSearchTierRefinement,
                Some("halton_02"),
                None,
                Some(0),
                Some(0),
            )
        ));
    }

    fn return_components_stage_progress_event(
        phase: ReturnComponentsStageProgressPhase,
        completed_units: usize,
        total_units: usize,
        current_item_label: Option<&str>,
        current_evaluation_count: Option<usize>,
        evaluation_progress_percent: Option<usize>,
    ) -> CalibrationStagedRunObserverEvent {
        CalibrationStagedRunObserverEvent::StageProgress(Box::new(
            janiform_calibration_runtime::CalibrationStagedRunStageProgressEvent {
                run_id: "test-run".to_string(),
                stage: CalibrationStage::ReturnComponents,
                stage_index: 3,
                stage_count: 5,
                progress_phase: CalibrationStageProgressPhase::ReturnComponents(phase),
                completed_units,
                total_units,
                stage_progress_percent: None,
                aggregate_progress_percent: None,
                current_item_label: current_item_label.map(str::to_string),
                current_restart_id: None,
                current_work_item_status: None,
                planned_work_item_labels: Vec::new(),
                active_restart_count: None,
                current_parameter_block: None,
                completed_outer_cycles: None,
                total_outer_cycles: None,
                current_evaluation_count,
                total_evaluation_budget: None,
                evaluation_progress_percent,
                current_pricing_interval_index: None,
                total_pricing_interval_count: None,
                completed_pricing_row_count: None,
                total_pricing_row_count: None,
                valid_completed_units: None,
                cumulative_local_evaluation_count: None,
                current_global_loss: None,
                current_block_best_global_loss: None,
                current_candidate_assessment: None,
                best_restart_id: None,
                best_global_loss: None,
                current_coarse_consistency_fit: None,
                best_coarse_consistency_fit: None,
                exact_closed_form_buy_count: None,
                exact_closed_form_sell_count: None,
                exact_tilted_mixed_count: None,
                restart_evaluation_timing_diagnostic: None,
                current_convergence_ratio: None,
            },
        ))
    }

    fn joint_refinement_progress_event(
        phase: JointRefinementStageProgressPhase,
        restart_id: Option<&str>,
        current_parameter_block: Option<&str>,
        current_evaluation_count: Option<usize>,
        evaluation_progress_percent: Option<usize>,
    ) -> CalibrationStagedRunObserverEvent {
        CalibrationStagedRunObserverEvent::StageProgress(Box::new(
            janiform_calibration_runtime::CalibrationStagedRunStageProgressEvent {
                run_id: "test-run".to_string(),
                stage: CalibrationStage::JointRefinement,
                stage_index: 5,
                stage_count: 5,
                progress_phase: CalibrationStageProgressPhase::JointRefinement(phase),
                completed_units: 0,
                total_units: 6,
                stage_progress_percent: None,
                aggregate_progress_percent: None,
                current_item_label: restart_id.map(str::to_string),
                current_restart_id: restart_id.map(str::to_string),
                current_work_item_status: None,
                planned_work_item_labels: Vec::new(),
                active_restart_count: Some(5),
                current_parameter_block: current_parameter_block.map(str::to_string),
                completed_outer_cycles: current_parameter_block.map(|_| 0),
                total_outer_cycles: Some(1),
                current_evaluation_count,
                total_evaluation_budget: Some(59),
                evaluation_progress_percent,
                current_pricing_interval_index: None,
                total_pricing_interval_count: None,
                completed_pricing_row_count: None,
                total_pricing_row_count: None,
                valid_completed_units: Some(0),
                cumulative_local_evaluation_count: Some(0),
                current_global_loss: None,
                current_block_best_global_loss: None,
                current_candidate_assessment: None,
                best_restart_id: None,
                best_global_loss: None,
                current_coarse_consistency_fit: None,
                best_coarse_consistency_fit: None,
                exact_closed_form_buy_count: None,
                exact_closed_form_sell_count: None,
                exact_tilted_mixed_count: None,
                restart_evaluation_timing_diagnostic: None,
                current_convergence_ratio: None,
            },
        ))
    }
}
