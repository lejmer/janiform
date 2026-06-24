use std::path::Path;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    ReturnComponentsStageFittedParameters, resolve_calibration_stage_seed_metadata,
    stage_solver_settings_for_stage,
};
use janiform_foundations::splitmix64_output_mixer;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use crate::stages::hawkes::stationary_open_event_rate_per_hour;
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionInput,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use crate::stages::mean_reversion::MeanReversionStageFittedPayload;
use crate::stages::return_components::{
    ReturnComponentsStageFittedPayload, prepare_observed_signal_inputs_for_joint_refinement,
    resolve_activity_calibration_context,
};

use super::super::super::super::setup::{
    joint_refinement_final_simulation_seed_count, stage_input_to_activity_input,
};
use super::super::super::JointRefinementResumeStore;
use super::super::progress::run_single_pass_joint_refinement_progress_phase;
use super::super::types::{
    InitialPipelineState, JointRefinementPipelineStage, ResolvedStagePayloads,
};

const PREPARATION_STAGE_INPUT_RESOLUTION_LABEL: &str = "resolve stage inputs";
const PREPARATION_RESUME_STATE_RESOLUTION_LABEL: &str = "resolve resume state";

pub(in crate::stages::joint_refinement::execution::pipeline) fn resolve_initial_pipeline_state<
    'a,
>(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &'a JointRefinementStageExecutionInput,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    disk_checkpoint_directory: Option<&Path>,
    load_current_stage_resume_state: bool,
    pipeline_stage: JointRefinementPipelineStage,
) -> Result<InitialPipelineState<'a>, JointRefinementStageExecutionFailure> {
    let resolved_stage_payloads = run_single_pass_joint_refinement_progress_phase(
        progress_sender,
        JointRefinementStageProgressPhase::StageInputResolution,
        PREPARATION_STAGE_INPUT_RESOLUTION_LABEL,
        || resolve_stage_payloads(run_spec, calendar_registry, stage_input, pipeline_stage),
    )?;

    let resume_store = run_single_pass_joint_refinement_progress_phase(
        progress_sender,
        JointRefinementStageProgressPhase::ResumeStateResolution,
        PREPARATION_RESUME_STATE_RESOLUTION_LABEL,
        || {
            Ok(if load_current_stage_resume_state {
                JointRefinementResumeStore::load(
                    disk_checkpoint_directory,
                    run_spec.run_id.as_str(),
                    pipeline_stage.calibration_stage(),
                    resolved_stage_payloads.resolved_stage_seed.stage_seed,
                )
            } else {
                JointRefinementResumeStore::fresh(
                    disk_checkpoint_directory,
                    run_spec.run_id.as_str(),
                    pipeline_stage.calibration_stage(),
                    resolved_stage_payloads.resolved_stage_seed.stage_seed,
                )
            })
        },
    )?;

    Ok(InitialPipelineState {
        resolved_stage_payloads,
        resume_store,
    })
}

fn resolve_stage_payloads<'a>(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &'a JointRefinementStageExecutionInput,
    pipeline_stage: JointRefinementPipelineStage,
) -> Result<ResolvedStagePayloads<'a>, JointRefinementStageExecutionFailure> {
    validate_joint_refinement_stage_request(run_spec, pipeline_stage.calibration_stage())?;

    let stage_solver_settings =
        stage_solver_settings_for_stage(run_spec, pipeline_stage.calibration_stage())
            .ok_or(JointRefinementStageExecutionFailure::MissingStageSolverSettings)?;

    let hawkes_payload = stage_input
        .hawkes_input
        .payload()
        .ok_or(JointRefinementStageExecutionFailure::MissingHawkesPayload)?;

    let return_components_payload = stage_input
        .return_components_input
        .payload()
        .ok_or(JointRefinementStageExecutionFailure::MissingReturnComponentsPayload)?;

    let mean_reversion_payload = stage_input
        .mean_reversion_input
        .payload()
        .ok_or(JointRefinementStageExecutionFailure::MissingMeanReversionPayload)?;

    let resolved_stage_seed =
        resolve_calibration_stage_seed_metadata(run_spec, pipeline_stage.calibration_stage());

    let activity_context = resolve_activity_calibration_context(&stage_input_to_activity_input(
        &stage_input.hawkes_input,
    ));

    let observed_signal_preparation = prepare_observed_signal_inputs_for_joint_refinement(
        run_spec.data_inputs.exchange_code.as_str(),
        calendar_registry,
        stage_input.candidate_generation_input.clone(),
        activity_context,
    )
    .map_err(|source| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: source.to_string(),
        }
    })?;

    let baseline_modeled_open_event_rate_per_hour =
        stationary_open_event_rate_per_hour(&hawkes_payload.fitted_parameters)
            .ok_or(JointRefinementStageExecutionFailure::MissingHawkesPayload)?;

    Ok(ResolvedStagePayloads {
        stage_solver_settings: stage_solver_settings.clone(),
        hawkes_payload,
        return_components_payload,
        mean_reversion_payload,
        resolved_stage_seed,
        observed_signal_preparation,
        baseline_modeled_open_event_rate_per_hour,
        final_simulation_seeds: build_final_simulation_seeds(
            resolved_stage_seed.stage_seed,
            &stage_solver_settings,
        ),
    })
}

fn validate_joint_refinement_stage_request(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> Result<(), JointRefinementStageExecutionFailure> {
    run_spec.validate().map_err(
        |source| JointRefinementStageExecutionFailure::InvalidRunSpec {
            error_message: format!("{source} (classification={:?})", source.classification()),
        },
    )?;

    if run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == stage)
    {
        return Ok(());
    }

    Err(JointRefinementStageExecutionFailure::StageIsNotSelectedInRunSpec)
}

fn build_final_simulation_seeds(
    stage_seed: u64,
    stage_solver_settings: &CalibrationOptimizerSettings,
) -> Vec<u64> {
    let all_final_simulation_seeds = [
        stage_seed,
        splitmix64_output_mixer(stage_seed ^ 0xA5A5_A5A5_A5A5_A5A5),
        splitmix64_output_mixer(stage_seed ^ 0x5A5A_5A5A_5A5A_5A5A),
    ];

    all_final_simulation_seeds
        .into_iter()
        .take(joint_refinement_final_simulation_seed_count(
            stage_solver_settings,
        ))
        .collect()
}

pub(in crate::stages::joint_refinement::execution::pipeline) fn return_components_fitted_parameters_with_mean_reversion(
    return_components_payload: &ReturnComponentsStageFittedPayload,
    mean_reversion_payload: &MeanReversionStageFittedPayload,
) -> ReturnComponentsStageFittedParameters {
    let mut fitted_parameters = return_components_payload.fitted_parameters.clone();

    fitted_parameters.mean_reversion_fitted_parameters = mean_reversion_payload.fitted_parameters;

    fitted_parameters
}
