use std::path::PathBuf;

use janiform::calibration::{CalibrationResumeSettings, CalibrationStagedExecutionInput};
use janiform_workflows::fit::{
    FitPreparationRequest, FitWorkflowPaths, StagedFitExecutionWorkflowRequest,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(super) struct FitPreparedRunRequestPayload {
    paths: FitWorkflowPathsPayload,
    input_paths: Vec<String>,
    preparation_payload: Option<Value>,
    staged_execution_input_path: Option<String>,
    session_definition_json: Option<String>,
    resume_settings: Option<CalibrationResumeSettings>,
    progress_mode: janiform_workflows::terminal::TerminalProgressMode,
    emit_machine_events: bool,
}

impl FitPreparedRunRequestPayload {
    pub(super) fn take_resume_settings(&mut self) -> Option<CalibrationResumeSettings> {
        self.resume_settings.take()
    }

    pub(super) fn progress_mode(&self) -> janiform_workflows::terminal::TerminalProgressMode {
        self.progress_mode
    }

    pub(super) fn emit_machine_events(&self) -> bool {
        self.emit_machine_events
    }

    pub(super) fn into_workflow_request(
        self,
        execution_input: CalibrationStagedExecutionInput,
    ) -> StagedFitExecutionWorkflowRequest {
        StagedFitExecutionWorkflowRequest {
            execution_input,
            paths: self.paths.into_workflow_paths(),
            input_paths: self.input_paths,
            preparation_payload: self.preparation_payload,
            staged_execution_input_path: self.staged_execution_input_path.map(PathBuf::from),
            session_definition_json: self.session_definition_json,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct FitFullRunRequestPayload {
    pub(super) preparation_request: FitPreparationRequest,
    pub(super) output_dir: Option<String>,
    pub(super) resume_settings: Option<CalibrationResumeSettings>,
    pub(super) progress_mode: janiform_workflows::terminal::TerminalProgressMode,
    pub(super) emit_machine_events: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct FitPrepareRequestPayload {
    pub(super) preparation_request: FitPreparationRequest,
    pub(super) working_directory: String,
}

#[derive(Debug, Deserialize)]
struct FitWorkflowPathsPayload {
    output_directory: String,
    model_path: String,
    report_path: String,
    checkpoint_directory: String,
}

impl FitWorkflowPathsPayload {
    fn into_workflow_paths(self) -> FitWorkflowPaths {
        FitWorkflowPaths {
            output_directory: PathBuf::from(self.output_directory),
            model_path: PathBuf::from(self.model_path),
            report_path: PathBuf::from(self.report_path),
            checkpoint_directory: PathBuf::from(self.checkpoint_directory),
        }
    }
}
