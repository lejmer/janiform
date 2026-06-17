use super::super::super::super::contracts::{CalibrationExecutionMode, CalibrationStage};
use super::artifact::CalibrationStageParameterArtifact;
use super::errors::CalibrationStageParameterArtifactBuildError;

/// Input contract implemented by each stage-specific stage-parameter artifact builder.
///
/// The serialized stage-parameter payload remains the only enum taxonomy. Build inputs are
/// concrete stage contracts so the stage mapping lives with the input type that owns it.
pub trait CalibrationStageParameterArtifactInput: std::fmt::Debug {
    fn stage(&self) -> CalibrationStage;

    fn build_stage_parameter_artifact(
        self: Box<Self>,
        run_id: &str,
        execution_mode: CalibrationExecutionMode,
        artifact_reference: String,
    ) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError>;
}

/// Converts concrete or erased stage-parameter inputs into the artifact builder boundary.
pub trait IntoCalibrationStageParameterArtifactInput {
    fn into_stage_parameter_artifact_input(self)
    -> Box<dyn CalibrationStageParameterArtifactInput>;
}

impl<T> IntoCalibrationStageParameterArtifactInput for T
where
    T: CalibrationStageParameterArtifactInput + 'static,
{
    fn into_stage_parameter_artifact_input(
        self,
    ) -> Box<dyn CalibrationStageParameterArtifactInput> {
        Box::new(self)
    }
}

impl IntoCalibrationStageParameterArtifactInput
    for Box<dyn CalibrationStageParameterArtifactInput>
{
    fn into_stage_parameter_artifact_input(
        self,
    ) -> Box<dyn CalibrationStageParameterArtifactInput> {
        self
    }
}
