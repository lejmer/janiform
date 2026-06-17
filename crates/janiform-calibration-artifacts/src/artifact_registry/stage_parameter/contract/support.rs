use janiform_foundations::strings::is_non_empty_string;

use super::super::super::super::contracts::{
    CalibrationRunSpec, CalibrationStage, CalibrationStageHandoffTransport,
};
use super::super::super::stage_references::{
    calibration_stage_artifact_reference, stage_output_artifact_references_for_stage,
};
use super::errors::{
    CalibrationStageParameterArtifactBuildError, CalibrationStageParameterArtifactValidationError,
};
use super::optimizer::CalibrationStageParameterOptimizerState;

pub(super) fn resolve_stage_output_artifact_reference(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> Result<String, CalibrationStageParameterArtifactBuildError> {
    let artifact_references = stage_output_artifact_references_for_stage(run_spec, stage);

    match artifact_references.as_slice() {
        [] => Ok(calibration_stage_artifact_reference(
            run_spec.run_id.as_str(),
            stage,
        )),
        [artifact_reference] => Ok(artifact_reference.clone()),
        _ => Err(
            CalibrationStageParameterArtifactBuildError::ConflictingArtifactReferences {
                stage,
                artifact_references,
            },
        ),
    }
}

pub(super) fn validate_stage_parameter_metadata_field(
    field: &'static str,
    expected: &'static str,
    found: &str,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    if found != expected {
        return Err(
            CalibrationStageParameterArtifactValidationError::MetadataFieldMismatch {
                field,
                expected,
                found: found.to_string(),
            },
        );
    }

    Ok(())
}

pub(super) fn validate_stage_parameter_required_string_list(
    field: &'static str,
    expected: &[&str],
    found: &[String],
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    let expected_fields: Vec<String> = expected.iter().map(|value| (*value).to_string()).collect();
    if found != expected_fields.as_slice() {
        return Err(
            CalibrationStageParameterArtifactValidationError::RequiredFieldListMismatch {
                field,
                expected: expected_fields,
                found: found.to_vec(),
            },
        );
    }

    Ok(())
}

pub(super) fn validate_non_empty_artifact_value<E>(value: &str, error: E) -> Result<(), E> {
    if !is_non_empty_string(value) {
        return Err(error);
    }

    Ok(())
}

pub(super) fn validate_optimizer_state_presence(
    stage: CalibrationStage,
    optimizer_state: Option<&CalibrationStageParameterOptimizerState>,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    if optimizer_state.is_none() {
        return Err(
            CalibrationStageParameterArtifactValidationError::MissingOptimizerState { stage },
        );
    }

    Ok(())
}

pub(super) fn validate_optimizer_state_absence(
    stage: CalibrationStage,
    optimizer_state: Option<&CalibrationStageParameterOptimizerState>,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    if optimizer_state.is_some() {
        return Err(
            CalibrationStageParameterArtifactValidationError::UnexpectedOptimizerState { stage },
        );
    }

    Ok(())
}

pub(super) fn validate_return_components_input_boundary(
    transport: CalibrationStageHandoffTransport,
    selected_candidate_id: Option<&str>,
    artifact_reference: Option<&str>,
    boundary_mismatch_error: impl FnOnce(
        &'static str,
    ) -> CalibrationStageParameterArtifactValidationError,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    match transport {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            if selected_candidate_id.is_none_or(|value| !is_non_empty_string(value))
                || artifact_reference.is_some()
            {
                return Err(boundary_mismatch_error("in_memory_output"));
            }
        }
        CalibrationStageHandoffTransport::PersistedArtifactReference => {
            if artifact_reference.is_none_or(|value| !is_non_empty_string(value))
                || selected_candidate_id.is_some()
            {
                return Err(boundary_mismatch_error("persisted_artifact_reference"));
            }
        }
    }

    Ok(())
}
