use super::{
    CalibrationInMemoryStagePayloadState, CalibrationStage, CalibrationStagedRunFailure,
    CalibrationStagedRunInputs, CalibrationStagedRunStageReport, run_seasonality_calibration_stage,
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

    let needs_full_seasonality_payload = upcoming_stages.iter().any(|stage| {
        matches!(
            stage,
            CalibrationStage::Hawkes | CalibrationStage::ReturnComponents
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

    Ok(())
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
