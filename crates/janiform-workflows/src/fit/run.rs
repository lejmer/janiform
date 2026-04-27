use std::path::Path;
use std::path::PathBuf;

use janiform_calibration_artifacts::CalibrationResumeSettings;
use janiform_calibration_runtime::CalibrationMarketDataPreparationRequest;

use crate::error::WorkflowError;

use super::execution::{
    FitExecutionObserver, FitExecutionOutput, FitExecutionRequest, FitWorkflowPaths,
    fit_workflow_paths, run_fit_execution,
};
use super::output::fit_preparation_start_summary;
use super::preparation::{
    FitPreparationObserver, FitPreparationRequest, prepare_fit_workflow_with_observer,
};

/// Request for a complete fit workflow run.
#[derive(Debug)]
pub struct FitWorkflowRequest {
    /// High-level preparation request.
    pub preparation_request: FitPreparationRequest,
    /// Optional output base directory. The run directory is created below it.
    pub output_base_directory: Option<PathBuf>,
    /// Optional resume settings applied before execution.
    pub resume_settings: Option<CalibrationResumeSettings>,
}

/// Run fit preparation and staged calibration through one shared workflow path.
pub fn run_fit_workflow(
    request: FitWorkflowRequest,
    mut observer: FitExecutionObserver<'_>,
) -> Result<FitExecutionOutput, WorkflowError> {
    let FitWorkflowRequest {
        preparation_request,
        output_base_directory,
        resume_settings,
    } = request;

    let preparation_working_directory =
        preparation_working_directory(output_base_directory.as_deref())?;
    let mut emitted_start_summary = false;
    let terminal_output = observer.terminal_output;
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
        let rendered =
            fit_preparation_start_summary(ready_request, &paths).render(&terminal_output.renderer);

        terminal_output
            .renderer
            .write_stdout(&format!("\n{rendered}"))
            .map_err(|source| WorkflowError::OutputWrite { source })?;

        emitted_start_summary = true;

        Ok(())
    };

    let prepared_workflow = prepare_fit_workflow_with_observer(
        preparation_request,
        preparation_working_directory.as_path(),
        FitPreparationObserver {
            preparation_request_ready: Some(&mut emit_start_summary),
        },
    )?;

    let paths = fit_paths_for_run(
        prepared_workflow.execution_input.request.run_id.as_str(),
        output_base_directory.as_deref(),
    )?;

    let mut execution_input = prepared_workflow.execution_input;
    execution_input.request.resume_settings = resume_settings;

    if emitted_start_summary {
        if let Some(mut terminal_output) = observer.terminal_output {
            terminal_output.emit_start_summary = false;
            observer.terminal_output = Some(terminal_output);
        }
    }

    run_fit_execution(
        FitExecutionRequest {
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
