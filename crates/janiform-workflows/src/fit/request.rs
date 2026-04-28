use std::path::PathBuf;

use janiform_calibration_artifacts::CalibrationResumeSettings;
use janiform_calibration_runtime::CalibrationStagedExecutionInput;
use serde_json::Value;

use super::preparation::FitPreparationRequest;

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

/// Request for staged fit execution workflow.
#[derive(Debug)]
pub struct StagedFitExecutionWorkflowRequest {
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
