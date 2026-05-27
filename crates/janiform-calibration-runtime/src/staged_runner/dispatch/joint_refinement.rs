use std::path::Path;

use super::super::{
    CalibrationInMemoryStagePayloadState, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStagedRunFailure, CalibrationStagedRunInputs,
    CalibrationStagedRunObserverEvent, CalibrationStagedRunStageProgressEvent,
    CalibrationStagedRunStageReport, ExchangeCalendarRegistry, JointRefinementStageExecutionInput,
    JointRefinementStageHawkesInput, JointRefinementStageMeanReversionInput,
    JointRefinementStageOptionPricingInput, JointRefinementStageProgressUpdate,
    JointRefinementStageReturnComponentsInput, JointRefinementStageRuntimeHandoff,
    JointRefinementStageSeasonalityFactorsInput, ReturnComponentsStageCandidateGenerationInput,
    run_finalization_calibration_stage_with_progress_observer_and_persistence,
    run_joint_refinement_calibration_stage_with_progress_observer_and_persistence,
    run_option_refinement_calibration_stage_with_progress_observer_and_persistence,
};
use super::handoffs::{
    cloned_in_memory_stage_payload, expected_stage_handoff_artifact_reference,
    expected_stage_handoff_transport,
};

pub(super) fn run_joint_refinement_like_stage(
    stage: CalibrationStage,
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: JointRefinementStageExecutionInput,
    upstream_joint_refinement_handoff: Option<&JointRefinementStageRuntimeHandoff>,
    upstream_option_refinement_handoff: Option<&JointRefinementStageRuntimeHandoff>,
    progress_observer: Option<&mut dyn FnMut(JointRefinementStageProgressUpdate)>,
    disk_checkpoint_directory: Option<&Path>,
    load_current_stage_resume_state: bool,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    match stage {
        CalibrationStage::JointRefinement => {
            Ok(CalibrationStagedRunStageReport::JointRefinement(Box::new(
                run_joint_refinement_calibration_stage_with_progress_observer_and_persistence(
                    run_spec,
                    calendar_registry,
                    stage_input,
                    progress_observer,
                    disk_checkpoint_directory,
                    load_current_stage_resume_state,
                ),
            )))
        }
        CalibrationStage::OptionRefinement => {
            Ok(CalibrationStagedRunStageReport::OptionRefinement(Box::new(
                run_option_refinement_calibration_stage_with_progress_observer_and_persistence(
                    run_spec,
                    calendar_registry,
                    stage_input,
                    upstream_joint_refinement_handoff,
                    progress_observer,
                    disk_checkpoint_directory,
                    load_current_stage_resume_state,
                ),
            )))
        }
        CalibrationStage::Finalization => {
            Ok(CalibrationStagedRunStageReport::Finalization(Box::new(
                run_finalization_calibration_stage_with_progress_observer_and_persistence(
                    run_spec,
                    calendar_registry,
                    stage_input,
                    upstream_joint_refinement_handoff,
                    upstream_option_refinement_handoff,
                    progress_observer,
                    disk_checkpoint_directory,
                    load_current_stage_resume_state,
                ),
            )))
        }
        CalibrationStage::Seasonality
        | CalibrationStage::Hawkes
        | CalibrationStage::ReturnComponents
        | CalibrationStage::LongVariance
        | CalibrationStage::MeanReversion => {
            Err(CalibrationStagedRunFailure::MissingStageInput { stage })
        }
    }
}

pub(super) fn forward_joint_refinement_like_stage_progress(
    observer: &mut Option<&mut dyn FnMut(CalibrationStagedRunObserverEvent)>,
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
    stage_index: usize,
    stage_count: usize,
    progress_update: JointRefinementStageProgressUpdate,
) {
    if let Some(observer) = observer.as_deref_mut() {
        // Observer events own their payloads while the run spec stays borrowed by dispatch.
        observer(CalibrationStagedRunObserverEvent::StageProgress(Box::new(
            CalibrationStagedRunStageProgressEvent {
                run_id: run_spec.run_id.clone(),
                stage,
                stage_index,
                stage_count,
                progress_phase: progress_update.phase.into(),
                completed_units: progress_update.completed_units,
                total_units: progress_update.total_units,
                stage_progress_percent: Some(progress_update.stage_progress_percent()),
                aggregate_progress_percent: progress_update.aggregate_progress_percent,
                current_item_label: progress_update.current_item_label,
                current_restart_id: progress_update.current_restart_id,
                current_work_item_status: progress_update.current_work_item_status,
                planned_work_item_labels: progress_update.planned_work_item_labels,
                active_restart_count: progress_update.active_restart_count,
                completed_outer_cycles: progress_update.completed_outer_cycles,
                total_outer_cycles: progress_update.total_outer_cycles,
                current_evaluation_count: progress_update.current_evaluation_count,
                total_evaluation_budget: progress_update.total_evaluation_budget,
                evaluation_progress_percent: progress_update.evaluation_progress_percent,
                current_pricing_interval_index: progress_update.current_pricing_interval_index,
                total_pricing_interval_count: progress_update.total_pricing_interval_count,
                completed_pricing_row_count: progress_update.completed_pricing_row_count,
                total_pricing_row_count: progress_update.total_pricing_row_count,
                valid_completed_units: progress_update.valid_completed_units,
                cumulative_local_evaluation_count: progress_update
                    .cumulative_local_evaluation_count,
                current_global_loss: progress_update.current_global_loss,
                current_restart_best_global_loss: progress_update.current_restart_best_global_loss,
                current_candidate_assessment: progress_update.current_candidate_assessment,
                best_restart_id: progress_update.best_restart_id,
                best_global_loss: progress_update.best_global_loss,
                current_coarse_consistency_fit: progress_update.current_coarse_consistency_fit,
                best_coarse_consistency_fit: progress_update.best_coarse_consistency_fit,
                exact_closed_form_buy_count: progress_update.exact_closed_form_buy_count,
                exact_closed_form_sell_count: progress_update.exact_closed_form_sell_count,
                exact_tilted_mixed_count: progress_update.exact_tilted_mixed_count,
                restart_evaluation_timing_diagnostic: progress_update
                    .restart_evaluation_timing_diagnostic,
                current_convergence_ratio: None,
            },
        )));
    }
}

pub(super) fn joint_refinement_like_stage_input(
    run_spec: &CalibrationRunSpec,
    consuming_stage: CalibrationStage,
    staged_run_inputs: &CalibrationStagedRunInputs,
    payload_state: &CalibrationInMemoryStagePayloadState,
) -> Result<JointRefinementStageExecutionInput, CalibrationStagedRunFailure> {
    let option_refinement_settings =
        option_refinement_dispatch_settings(run_spec, consuming_stage, staged_run_inputs)?;

    Ok(JointRefinementStageExecutionInput {
        seasonality_factors_input: joint_refinement_seasonality_factors_input(
            consuming_stage,
            payload_state,
        )?,
        hawkes_input: joint_refinement_hawkes_input(run_spec, consuming_stage, payload_state)?,
        return_components_input: joint_refinement_return_components_input(
            run_spec,
            consuming_stage,
            payload_state,
        )?,
        mean_reversion_input: joint_refinement_mean_reversion_input(
            run_spec,
            consuming_stage,
            payload_state,
        )?,
        candidate_generation_input: joint_refinement_candidate_generation_input(
            consuming_stage,
            staged_run_inputs,
        )?,
        calibration_effort: staged_run_inputs.joint_refinement_calibration_effort,
        restart_candidate_budget_override: staged_run_inputs
            .joint_refinement_restart_candidate_budget_override,
        treat_inadmissible_calibration_warning_as_error: staged_run_inputs
            .treat_inadmissible_calibration_warning_as_error,
        option_refinement_candidate_count_override: option_refinement_settings
            .candidate_count_override,
        option_refinement_wall_clock_budget_millis: option_refinement_settings
            .wall_clock_budget_millis,
        audit_option_pricing_enabled: option_refinement_settings.audit_option_pricing_enabled,
        option_pricing_input: option_refinement_settings.option_pricing_input,
    })
}

fn joint_refinement_seasonality_factors_input(
    consuming_stage: CalibrationStage,
    payload_state: &CalibrationInMemoryStagePayloadState,
) -> Result<JointRefinementStageSeasonalityFactorsInput, CalibrationStagedRunFailure> {
    cloned_in_memory_stage_payload(
        &payload_state.joint_refinement_seasonality_factors_input,
        CalibrationStage::Seasonality,
        consuming_stage,
    )
}

fn joint_refinement_candidate_generation_input(
    consuming_stage: CalibrationStage,
    staged_run_inputs: &CalibrationStagedRunInputs,
) -> Result<ReturnComponentsStageCandidateGenerationInput, CalibrationStagedRunFailure> {
    // Return-component candidate generation is reused by all joint-refinement-like stages.
    let Some(candidate_generation_input) = staged_run_inputs
        .return_components_candidate_generation_input
        .clone()
    else {
        return Err(CalibrationStagedRunFailure::MissingStageInput {
            stage: consuming_stage,
        });
    };

    Ok(candidate_generation_input)
}

fn joint_refinement_hawkes_input(
    run_spec: &CalibrationRunSpec,
    consuming_stage: CalibrationStage,
    payload_state: &CalibrationInMemoryStagePayloadState,
) -> Result<JointRefinementStageHawkesInput, CalibrationStagedRunFailure> {
    let hawkes_stage_payload = cloned_in_memory_stage_payload(
        &payload_state.hawkes_stage_payload,
        CalibrationStage::Hawkes,
        consuming_stage,
    )?;

    match expected_stage_handoff_transport(run_spec, CalibrationStage::Hawkes, consuming_stage) {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            Ok(JointRefinementStageHawkesInput::InMemoryOutput {
                payload: hawkes_stage_payload,
            })
        }
        CalibrationStageHandoffTransport::PersistedArtifactReference => Ok(
            JointRefinementStageHawkesInput::PersistedArtifactReference {
                artifact_reference: expected_stage_handoff_artifact_reference(
                    run_spec,
                    CalibrationStage::Hawkes,
                    consuming_stage,
                ),
                payload: Some(hawkes_stage_payload),
            },
        ),
    }
}

fn joint_refinement_return_components_input(
    run_spec: &CalibrationRunSpec,
    consuming_stage: CalibrationStage,
    payload_state: &CalibrationInMemoryStagePayloadState,
) -> Result<JointRefinementStageReturnComponentsInput, CalibrationStagedRunFailure> {
    let return_components_stage_payload = cloned_in_memory_stage_payload(
        &payload_state.return_components_stage_payload,
        CalibrationStage::ReturnComponents,
        consuming_stage,
    )?;

    match expected_stage_handoff_transport(
        run_spec,
        CalibrationStage::ReturnComponents,
        consuming_stage,
    ) {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            Ok(JointRefinementStageReturnComponentsInput::InMemoryOutput {
                payload: return_components_stage_payload,
            })
        }
        CalibrationStageHandoffTransport::PersistedArtifactReference => Ok(
            JointRefinementStageReturnComponentsInput::PersistedArtifactReference {
                artifact_reference: expected_stage_handoff_artifact_reference(
                    run_spec,
                    CalibrationStage::ReturnComponents,
                    consuming_stage,
                ),
                payload: Some(return_components_stage_payload),
            },
        ),
    }
}

fn joint_refinement_mean_reversion_input(
    run_spec: &CalibrationRunSpec,
    consuming_stage: CalibrationStage,
    payload_state: &CalibrationInMemoryStagePayloadState,
) -> Result<JointRefinementStageMeanReversionInput, CalibrationStagedRunFailure> {
    let mean_reversion_stage_payload = cloned_in_memory_stage_payload(
        &payload_state.mean_reversion_stage_payload,
        CalibrationStage::MeanReversion,
        consuming_stage,
    )?;

    match expected_stage_handoff_transport(
        run_spec,
        CalibrationStage::MeanReversion,
        consuming_stage,
    ) {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            Ok(JointRefinementStageMeanReversionInput::InMemoryOutput {
                payload: mean_reversion_stage_payload,
            })
        }
        CalibrationStageHandoffTransport::PersistedArtifactReference => Ok(
            JointRefinementStageMeanReversionInput::PersistedArtifactReference {
                artifact_reference: expected_stage_handoff_artifact_reference(
                    run_spec,
                    CalibrationStage::MeanReversion,
                    consuming_stage,
                ),
                payload: Some(mean_reversion_stage_payload),
            },
        ),
    }
}

fn option_refinement_dispatch_settings(
    run_spec: &CalibrationRunSpec,
    consuming_stage: CalibrationStage,
    staged_run_inputs: &CalibrationStagedRunInputs,
) -> Result<OptionRefinementDispatchSettings, CalibrationStagedRunFailure> {
    match consuming_stage {
        CalibrationStage::JointRefinement => {
            if run_spec
                .selected_stages
                .iter()
                .any(|stage| *stage == CalibrationStage::OptionRefinement)
            {
                // Option pricing input can be consumed by separate selected stages.
                return Ok(OptionRefinementDispatchSettings {
                    candidate_count_override: staged_run_inputs
                        .option_refinement_candidate_count_override,
                    wall_clock_budget_millis: None,
                    audit_option_pricing_enabled: false,
                    option_pricing_input: staged_run_inputs.option_refinement_pricing_input.clone(),
                });
            }

            Ok(OptionRefinementDispatchSettings {
                candidate_count_override: None,
                wall_clock_budget_millis: None,
                audit_option_pricing_enabled: false,
                option_pricing_input: None,
            })
        }
        CalibrationStage::OptionRefinement | CalibrationStage::Finalization => {
            // Option pricing input can be consumed by separate selected stages.
            Ok(OptionRefinementDispatchSettings {
                candidate_count_override: staged_run_inputs
                    .option_refinement_candidate_count_override,
                wall_clock_budget_millis: staged_run_inputs
                    .option_refinement_wall_clock_budget_millis,
                audit_option_pricing_enabled: staged_run_inputs.audit_option_pricing_enabled,
                option_pricing_input: staged_run_inputs.option_refinement_pricing_input.clone(),
            })
        }
        CalibrationStage::Seasonality
        | CalibrationStage::Hawkes
        | CalibrationStage::ReturnComponents
        | CalibrationStage::LongVariance
        | CalibrationStage::MeanReversion => Err(CalibrationStagedRunFailure::MissingStageInput {
            stage: consuming_stage,
        }),
    }
}

struct OptionRefinementDispatchSettings {
    candidate_count_override: Option<usize>,
    wall_clock_budget_millis: Option<u64>,
    audit_option_pricing_enabled: bool,
    option_pricing_input: Option<JointRefinementStageOptionPricingInput>,
}
