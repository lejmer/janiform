use super::super::super::super::contracts::{CalibrationExecutionMode, CalibrationRunContract};
use super::artifact::CalibrationStageParameterArtifact;
use super::errors::CalibrationStageParameterArtifactBuildError;
use super::input::IntoCalibrationStageParameterArtifactInput;
use super::support::resolve_stage_output_artifact_reference;

pub fn build_calibration_stage_parameter_artifact<I>(
    run_contract: &CalibrationRunContract,
    stage_input: I,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError>
where
    I: IntoCalibrationStageParameterArtifactInput,
{
    if run_contract.run_spec.execution_mode != CalibrationExecutionMode::Disk {
        return Err(
            CalibrationStageParameterArtifactBuildError::UnsupportedExecutionMode {
                expected: CalibrationExecutionMode::Disk,
                found: run_contract.run_spec.execution_mode,
            },
        );
    }

    let stage_input = stage_input.into_stage_parameter_artifact_input();
    let stage = stage_input.stage();

    let artifact_reference =
        resolve_stage_output_artifact_reference(&run_contract.run_spec, stage)?;

    let artifact = stage_input.build_stage_parameter_artifact(
        run_contract.run_spec.run_id.as_str(),
        run_contract.run_spec.execution_mode,
        artifact_reference,
    )?;

    artifact.validate().map_err(|source| {
        CalibrationStageParameterArtifactBuildError::ValidationFailed {
            stage,
            error_message: source.to_string(),
        }
    })?;

    Ok(artifact)
}
