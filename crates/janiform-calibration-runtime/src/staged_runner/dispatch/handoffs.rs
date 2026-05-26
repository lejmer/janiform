use super::super::{
    CalibrationExecutionMode, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStagedRunFailure,
    calibration_stage_artifact_reference, stage_output_artifact_references_for_stage,
};

pub(super) fn expected_stage_handoff_transport(
    run_spec: &CalibrationRunSpec,
    from_stage: CalibrationStage,
    to_stage: CalibrationStage,
) -> CalibrationStageHandoffTransport {
    run_spec
        .stage_handoffs
        .iter()
        .find(|handoff| handoff.from_stage == from_stage && handoff.to_stage == to_stage)
        .map(|handoff| handoff.transport())
        .unwrap_or(match run_spec.execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoffTransport::InMemoryOutput,
            CalibrationExecutionMode::Disk => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        })
}

pub(super) fn expected_stage_handoff_artifact_reference(
    run_spec: &CalibrationRunSpec,
    from_stage: CalibrationStage,
    to_stage: CalibrationStage,
) -> String {
    if let Some(artifact_reference) = run_spec
        .stage_handoffs
        .iter()
        .find(|handoff| handoff.from_stage == from_stage && handoff.to_stage == to_stage)
        .and_then(|handoff| handoff.artifact_reference())
        && !artifact_reference.trim().is_empty()
    {
        return artifact_reference.to_string();
    }

    calibration_stage_artifact_reference(run_spec.run_id.as_str(), from_stage)
}

pub(in crate::staged_runner) fn expected_stage_output_artifact_reference(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> String {
    let mut artifact_references = stage_output_artifact_references_for_stage(run_spec, stage);

    if artifact_references.len() == 1 {
        return artifact_references.remove(0);
    }

    calibration_stage_artifact_reference(run_spec.run_id.as_str(), stage)
}

pub(super) fn cloned_in_memory_stage_payload<T: Clone>(
    payload: &Option<T>,
    required_stage: CalibrationStage,
    consuming_stage: CalibrationStage,
) -> Result<T, CalibrationStagedRunFailure> {
    // Stage input contracts own their payloads while the payload state remains available
    // to later consumers.
    payload.clone().ok_or(
        CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
            required_stage,
            consuming_stage,
        },
    )
}
