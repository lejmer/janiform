use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;

use janiform_calibration_runtime::{
    CalibrationMarketDataPreparationRequest, MarketDataPreparationProgressEvent,
    MarketDataPreparationProgressPhase,
};
use janiform_event_log::WorkflowRef;
use janiform_terminal_ui::{ProgressAdapter, ProgressEvent, ProgressMeasure};

use crate::error::WorkflowError;
use crate::market_data_progress::{
    MarketDataLoadingProgressGate, market_data_loading_progress_measure,
    market_data_loading_progress_signal, market_data_loading_run_event,
    market_data_preparation_progress_signal, market_data_preparation_run_event,
    market_data_timestamp_batching_observation_progress_signal,
    market_data_timestamp_batching_observation_run_event, should_emit_market_data_loading_progress,
};

use super::execution::{fit_workflow_paths, run_staged_fit_execution_workflow};
use super::input_paths::fit_preparation_request_input_count;
use super::labels::{fit_preparation_progress_scope_label, fit_workflow_id, fit_workflow_label};
use super::output::FitWorkflowOutput;
use super::output::fit_preparation_start_summary;
use super::preparation::{
    FitPreparationObserver, FitPreparationProgress, FitPreparationRequest,
    prepare_fit_workflow_with_observer,
};
use super::request::{FitWorkflowPaths, FitWorkflowRequest, StagedFitExecutionWorkflowRequest};
use super::workflow::FitWorkflowObserver;

/// Run fit preparation and staged calibration through one shared workflow path.
pub fn run_fit_workflow(
    request: FitWorkflowRequest,
    mut observer: FitWorkflowObserver<'_>,
) -> Result<FitWorkflowOutput, WorkflowError> {
    let FitWorkflowRequest {
        preparation_request,
        output_base_directory,
        resume_settings,
    } = request;

    let preparation_working_directory =
        preparation_working_directory(output_base_directory.as_deref())?;

    let mut emitted_start_summary = false;
    let terminal_output = observer.terminal_output;
    let progress_adapter = RefCell::new(observer.progress_adapter.take());

    let mut machine_event_sink = observer.machine_event_sink.take();
    let mut loading_progress_gate = MarketDataLoadingProgressGate::default();

    observe_preparation_started(&mut progress_adapter.borrow_mut(), &preparation_request)?;

    let observe_market_data_progress =
        progress_adapter.borrow().is_some() || machine_event_sink.is_some();

    let prepared_workflow_result = {
        let mut emit_start_summary = |ready_request: &CalibrationMarketDataPreparationRequest| {
            let Some(terminal_output) = terminal_output else {
                return Ok(());
            };
            if !terminal_output.emit_start_summary {
                return Ok(());
            }

            let paths = fit_paths_for_run(
                ready_request.run_id.as_str(),
                output_base_directory.as_deref(),
            )?;

            let rendered = fit_preparation_start_summary(ready_request, &paths)
                .render(&terminal_output.renderer);

            suspend_preparation_progress_for_output(&mut progress_adapter.borrow_mut(), || {
                terminal_output
                    .renderer
                    .write_stdout(&format!("\n{rendered}"))
            })
            .map_err(|source| WorkflowError::OutputWrite { source })?;

            emitted_start_summary = true;

            Ok(())
        };

        let mut emit_preparation_progress = |progress: FitPreparationProgress| {
            render_preparation_progress(&mut progress_adapter.borrow_mut(), progress)
        };

        let mut emit_market_data_progress = |run_id: &str, event| match event {
            MarketDataPreparationProgressEvent::Loading(loading_event) => {
                if !should_emit_market_data_loading_progress(
                    &mut loading_progress_gate,
                    &loading_event,
                ) {
                    return Ok(());
                }

                render_market_data_loading_progress(
                    &mut progress_adapter.borrow_mut(),
                    &loading_event,
                )?;

                let Some(machine_event_sink) = machine_event_sink.as_mut() else {
                    return Ok(());
                };

                let run_event = market_data_loading_run_event(
                    run_id,
                    WorkflowRef::labeled(fit_workflow_id(), fit_workflow_label()),
                    &loading_event,
                );

                machine_event_sink(&run_event)
            }
            MarketDataPreparationProgressEvent::Preparing(phase) => {
                render_market_data_preparation_progress(&mut progress_adapter.borrow_mut(), phase)?;

                let Some(machine_event_sink) = machine_event_sink.as_mut() else {
                    return Ok(());
                };

                let run_event = market_data_preparation_run_event(
                    run_id,
                    WorkflowRef::labeled(fit_workflow_id(), fit_workflow_label()),
                    phase,
                );

                machine_event_sink(&run_event)
            }
            MarketDataPreparationProgressEvent::TimestampBatchingObservation(diagnostic) => {
                render_market_data_timestamp_batching_observation_progress(
                    &mut progress_adapter.borrow_mut(),
                    &diagnostic,
                )?;

                let Some(machine_event_sink) = machine_event_sink.as_mut() else {
                    return Ok(());
                };

                let run_event = market_data_timestamp_batching_observation_run_event(
                    run_id,
                    WorkflowRef::labeled(fit_workflow_id(), fit_workflow_label()),
                    &diagnostic,
                );

                machine_event_sink(&run_event)
            }
        };

        prepare_fit_workflow_with_observer(
            preparation_request,
            preparation_working_directory.as_path(),
            FitPreparationObserver {
                preparation_request_ready: Some(&mut emit_start_summary),
                preparation_progress: Some(&mut emit_preparation_progress),
                market_data_preparation_progress: if observe_market_data_progress {
                    Some(&mut emit_market_data_progress)
                } else {
                    None
                },
            },
        )
    };

    observer.progress_adapter = progress_adapter.into_inner();
    observer.machine_event_sink = machine_event_sink;

    let prepared_workflow = prepared_workflow_result?;

    let paths = fit_paths_for_run(
        prepared_workflow.execution_input.request.run_id.as_str(),
        output_base_directory.as_deref(),
    )?;

    let mut execution_input = prepared_workflow.execution_input;

    execution_input.request.resume_settings = resume_settings;

    if emitted_start_summary && let Some(mut terminal_output) = observer.terminal_output {
        terminal_output.emit_start_summary = false;
        observer.terminal_output = Some(terminal_output);
    }

    run_staged_fit_execution_workflow(
        StagedFitExecutionWorkflowRequest {
            execution_input,
            paths,
            input_paths: prepared_workflow.input_paths,
            preparation_payload: Some(prepared_workflow.preparation_payload),
            staged_execution_input_path: None,
            session_definition_json: Some(prepared_workflow.session_definition_json),
        },
        observer,
    )
}

fn observe_preparation_started(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    request: &FitPreparationRequest,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunStarted {
        run_id: preparation_progress_run_id(request),
        scope: fit_preparation_progress_scope_label().to_string(),
        input_count: fit_preparation_request_input_count(request),
        output_path: None,
        stage_sequence: Vec::new(),
        completed_stage_count: 0,
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

fn suspend_preparation_progress_for_output<T>(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    operation: impl FnOnce() -> T,
) -> T {
    let Some(progress_adapter) = progress_adapter.as_deref() else {
        return operation();
    };

    progress_adapter.suspend_for_output(operation)
}

fn render_preparation_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    progress: FitPreparationProgress,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunProgress {
        phase: progress.phase.to_string(),
        measure: ProgressMeasure::Unbounded,
        latest_signal: Some(progress.latest_signal.to_string()),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

fn render_market_data_loading_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    event: &janiform_calibration_runtime::MarketDataLoadingProgressEvent,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunProgress {
        phase: "Loading market data".to_string(),
        measure: market_data_loading_progress_measure(event)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        latest_signal: Some(market_data_loading_progress_signal(event)),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

fn render_market_data_preparation_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    phase: MarketDataPreparationProgressPhase,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunProgress {
        phase: "Preparing market data".to_string(),
        measure: ProgressMeasure::Unbounded,
        latest_signal: Some(market_data_preparation_progress_signal(phase)),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

fn render_market_data_timestamp_batching_observation_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    diagnostic: &janiform::calibration::market_data::CalibrationTimestampBatchingObservationDiagnostic,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunProgress {
        phase: "Preparing market data".to_string(),
        measure: ProgressMeasure::Unbounded,
        latest_signal: Some(market_data_timestamp_batching_observation_progress_signal(
            diagnostic,
        )),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

fn preparation_progress_run_id(request: &FitPreparationRequest) -> String {
    request
        .run_id
        .clone()
        .unwrap_or_else(|| "resolving run id".to_string())
}

fn preparation_working_directory(
    output_base_directory: Option<&Path>,
) -> Result<PathBuf, WorkflowError> {
    Ok(base_directory(output_base_directory)?.join("preparation"))
}

fn fit_paths_for_run(
    run_id: &str,
    output_base_directory: Option<&Path>,
) -> Result<FitWorkflowPaths, WorkflowError> {
    Ok(fit_workflow_paths(
        base_directory(output_base_directory)?.join(run_id),
    ))
}

fn base_directory(output_base_directory: Option<&Path>) -> Result<PathBuf, WorkflowError> {
    match output_base_directory {
        Some(output_base_directory) => Ok(output_base_directory.to_path_buf()),
        None => std::env::current_dir()
            .map(|path| path.join(".janiform"))
            .map_err(|source| WorkflowError::OutputWrite { source }),
    }
}
