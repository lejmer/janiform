use super::{
    CalibrationInMemoryStagePayloadState, CalibrationStage, CalibrationStagedRunFailure,
    CalibrationStagedRunInputs, CalibrationStagedRunStageReport,
    ObservationModelsHawkesSourceResponsibilityInput, run_observation_models_calibration_stage,
    run_seasonality_calibration_stage,
};
use crate::staged_runner::payload_state::update_in_memory_payload_state;
use janiform_calibration_artifacts::CalibrationRunContract;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

pub(super) fn rebuild_recomputable_disk_resume_payloads(
    run_contract: &CalibrationRunContract,
    calendar_registry: &ExchangeCalendarRegistry,
    staged_run_inputs: &CalibrationStagedRunInputs,
    payload_state: &mut CalibrationInMemoryStagePayloadState,
    start_position: usize,
) -> Result<(), CalibrationStagedRunFailure> {
    let upcoming_stages = run_contract
        .execution_stage_order
        .get(start_position..)
        .unwrap_or(&[]);

    let needs_observation_source_activity_rebuild = needs_observation_source_activity_rebuild(
        upcoming_stages,
        staged_run_inputs,
        payload_state,
    );

    let needs_full_seasonality_payload = needs_observation_source_activity_rebuild
        || upcoming_stages.iter().any(|stage| {
            matches!(
                stage,
                CalibrationStage::Hawkes | CalibrationStage::ObservationModels
            )
        });

    if needs_full_seasonality_payload && payload_state.seasonality_stage_payload.is_none() {
        rebuild_seasonality_resume_payload(
            run_contract,
            calendar_registry,
            staged_run_inputs,
            payload_state,
        )?;
    }

    if needs_observation_source_activity_rebuild {
        rebuild_observation_models_resume_payload(run_contract, staged_run_inputs, payload_state)?;
    }

    Ok(())
}

fn needs_observation_source_activity_rebuild(
    upcoming_stages: &[CalibrationStage],
    staged_run_inputs: &CalibrationStagedRunInputs,
    payload_state: &CalibrationInMemoryStagePayloadState,
) -> bool {
    if upcoming_stages.contains(&CalibrationStage::ObservationModels) {
        return false;
    }

    if !upcoming_stages
        .iter()
        .any(|stage| stage_requires_return_candidate_generation(*stage))
    {
        return false;
    }

    let required_source_activity_count = staged_run_inputs
        .return_components_candidate_generation_input
        .as_ref()
        .map(|input| input.observed_open_session_returns.len())
        .unwrap_or(0);

    let restored_source_activity_count = payload_state
        .observation_models_stage_payload
        .as_ref()
        .map(|payload| payload.open_session_return_source_activities.len())
        .unwrap_or(0);

    restored_source_activity_count != required_source_activity_count
}

fn stage_requires_return_candidate_generation(stage: CalibrationStage) -> bool {
    matches!(
        stage,
        CalibrationStage::ReturnComponents
            | CalibrationStage::LongVariance
            | CalibrationStage::ReturnBackfit
            | CalibrationStage::JointRefinement
    )
}

fn rebuild_seasonality_resume_payload(
    run_contract: &CalibrationRunContract,
    calendar_registry: &ExchangeCalendarRegistry,
    staged_run_inputs: &CalibrationStagedRunInputs,
    payload_state: &mut CalibrationInMemoryStagePayloadState,
) -> Result<(), CalibrationStagedRunFailure> {
    // Intentional clone because warmup owns the stage input while staged inputs remain available
    // for the resumed execution sequence.
    let Some(stage_input) = staged_run_inputs.seasonality_stage_input.clone() else {
        return Err(CalibrationStagedRunFailure::MissingStageInput {
            stage: CalibrationStage::Seasonality,
        });
    };

    let report =
        run_seasonality_calibration_stage(&run_contract.run_spec, calendar_registry, stage_input);

    if !report.status.is_successful() {
        return Err(CalibrationStagedRunFailure::StageExecutionFailed {
            stage: CalibrationStage::Seasonality,
            error_message: report
                .failure
                .as_ref()
                .map(|failure| format!("{failure:?}")),
        });
    }

    update_in_memory_payload_state(
        payload_state,
        &CalibrationStagedRunStageReport::Seasonality(Box::new(report)),
    );

    Ok(())
}

fn rebuild_observation_models_resume_payload(
    run_contract: &CalibrationRunContract,
    staged_run_inputs: &CalibrationStagedRunInputs,
    payload_state: &mut CalibrationInMemoryStagePayloadState,
) -> Result<(), CalibrationStagedRunFailure> {
    // Intentional clone because the observation stage consumes its execution input.
    let Some(mut stage_input) = staged_run_inputs.observation_models_stage_input.clone() else {
        return Err(CalibrationStagedRunFailure::MissingStageInput {
            stage: CalibrationStage::ObservationModels,
        });
    };

    // Intentional clone because the source-responsibility input owns the event tape.
    let seasonality_payload = payload_state.seasonality_stage_payload.clone().ok_or(
        CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
            required_stage: CalibrationStage::Seasonality,
            consuming_stage: CalibrationStage::ObservationModels,
        },
    )?;

    let hawkes_payload = payload_state.hawkes_stage_payload.as_ref().ok_or(
        CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
            required_stage: CalibrationStage::Hawkes,
            consuming_stage: CalibrationStage::ObservationModels,
        },
    )?;

    // Intentional clone because event side labels are consumed by the recomputed observation stage.
    let Some(event_side_codes) = staged_run_inputs.hawkes_event_side_codes.clone() else {
        return Err(CalibrationStagedRunFailure::MissingStageInput {
            stage: CalibrationStage::Hawkes,
        });
    };

    stage_input.source_responsibility_input =
        Some(ObservationModelsHawkesSourceResponsibilityInput {
            seasonality_payload,
            event_side_codes,
            // Intentional clone to keep the restored Hawkes payload available to resumed stages.
            hawkes_fitted_parameters: hawkes_payload.fitted_parameters.clone(),
        });

    let report = run_observation_models_calibration_stage(&run_contract.run_spec, stage_input);

    if !report.status.is_successful() {
        return Err(CalibrationStagedRunFailure::StageExecutionFailed {
            stage: CalibrationStage::ObservationModels,
            error_message: report
                .failure
                .as_ref()
                .map(|failure| format!("{failure:?}")),
        });
    }

    update_in_memory_payload_state(
        payload_state,
        &CalibrationStagedRunStageReport::ObservationModels(Box::new(report)),
    );

    Ok(())
}
