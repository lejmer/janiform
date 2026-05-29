use std::path::Path;

use super::super::{
    CalibrationInMemoryStagePayloadState, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStagePayloadSource, CalibrationStagedRunFailure,
    CalibrationStagedRunInputs, CalibrationStagedRunObserverEvent,
    CalibrationStagedRunStageProgressEvent, CalibrationStagedRunStageReport,
    ExchangeCalendarRegistry, JointRefinementStageExecutionInput,
    JointRefinementStageOptionPricingInput, JointRefinementStageProgressUpdate,
    JointRefinementStageRuntimeHandoff, JointRefinementStageSeasonalityFactorsInput,
    ReturnComponentsStageCandidateGenerationInput,
    run_finalization_calibration_stage_with_progress_observer_and_persistence,
    run_joint_refinement_calibration_stage_with_progress_observer_and_persistence,
    run_option_refinement_calibration_stage_with_progress_observer_and_persistence,
};
use super::handoffs::{
    cloned_in_memory_stage_payload, expected_stage_handoff_artifact_reference,
    expected_stage_handoff_transport,
};

pub(super) struct JointRefinementLikeStageRunInput<'a, 'observer> {
    pub(super) stage: CalibrationStage,
    pub(super) run_spec: &'a CalibrationRunSpec,
    pub(super) calendar_registry: &'a ExchangeCalendarRegistry,
    pub(super) stage_input: JointRefinementStageExecutionInput,
    pub(super) upstream_joint_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
    pub(super) upstream_option_refinement_handoff: Option<&'a JointRefinementStageRuntimeHandoff>,
    pub(super) progress_observer:
        Option<&'observer mut dyn FnMut(JointRefinementStageProgressUpdate)>,
    pub(super) disk_checkpoint_directory: Option<&'a Path>,
    pub(super) load_current_stage_resume_state: bool,
}

pub(super) fn run_joint_refinement_like_stage(
    input: JointRefinementLikeStageRunInput<'_, '_>,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    match input.stage {
        CalibrationStage::JointRefinement => {
            Ok(CalibrationStagedRunStageReport::JointRefinement(Box::new(
                run_joint_refinement_calibration_stage_with_progress_observer_and_persistence(
                    input.run_spec,
                    input.calendar_registry,
                    input.stage_input,
                    input.progress_observer,
                    input.disk_checkpoint_directory,
                    input.load_current_stage_resume_state,
                ),
            )))
        }
        CalibrationStage::OptionRefinement => {
            Ok(CalibrationStagedRunStageReport::OptionRefinement(Box::new(
                run_option_refinement_calibration_stage_with_progress_observer_and_persistence(
                    input.run_spec,
                    input.calendar_registry,
                    input.stage_input,
                    input.upstream_joint_refinement_handoff,
                    input.progress_observer,
                    input.disk_checkpoint_directory,
                    input.load_current_stage_resume_state,
                ),
            )))
        }
        CalibrationStage::Finalization => {
            Ok(CalibrationStagedRunStageReport::Finalization(Box::new(
                run_finalization_calibration_stage_with_progress_observer_and_persistence(
                    super::super::FinalizationCalibrationStageEntrypointInput {
                        run_spec: input.run_spec,
                        calendar_registry: input.calendar_registry,
                        stage_input: input.stage_input,
                        upstream_joint_refinement_handoff: input.upstream_joint_refinement_handoff,
                        upstream_option_refinement_handoff: input
                            .upstream_option_refinement_handoff,
                        disk_checkpoint_directory: input.disk_checkpoint_directory,
                        load_current_stage_resume_state: input.load_current_stage_resume_state,
                    },
                    input.progress_observer,
                ),
            )))
        }
        CalibrationStage::Seasonality
        | CalibrationStage::Hawkes
        | CalibrationStage::ReturnComponents
        | CalibrationStage::LongVariance
        | CalibrationStage::NewsShocks
        | CalibrationStage::MeanReversion => {
            Err(CalibrationStagedRunFailure::MissingStageInput { stage: input.stage })
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
        hawkes_input: joint_refinement_payload_source(
            run_spec,
            CalibrationStage::Hawkes,
            consuming_stage,
            &payload_state.hawkes_stage_payload,
        )?,
        return_components_input: joint_refinement_payload_source(
            run_spec,
            CalibrationStage::ReturnComponents,
            consuming_stage,
            &payload_state.return_components_stage_payload,
        )?,
        mean_reversion_input: joint_refinement_payload_source(
            run_spec,
            CalibrationStage::MeanReversion,
            consuming_stage,
            &payload_state.mean_reversion_stage_payload,
        )?,
        candidate_generation_input: joint_refinement_candidate_generation_input(
            consuming_stage,
            staged_run_inputs,
        )?,
        coarse_data_records: staged_run_inputs.coarse_data_records.clone(),
        tick_objective_data_usage_policy: staged_run_inputs.tick_objective_data_usage_policy,
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
        // Intentional copy because the stage input owns the diagnostic request while
        // staged-run inputs remain available for later stage dispatch.
        diagnostics: staged_run_inputs.diagnostics.clone(),
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

fn joint_refinement_payload_source<T: Clone>(
    run_spec: &CalibrationRunSpec,
    upstream_stage: CalibrationStage,
    consuming_stage: CalibrationStage,
    payload: &Option<T>,
) -> Result<CalibrationStagePayloadSource<T>, CalibrationStagedRunFailure> {
    let stage_payload = cloned_in_memory_stage_payload(payload, upstream_stage, consuming_stage)?;

    match expected_stage_handoff_transport(run_spec, upstream_stage, consuming_stage) {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            Ok(CalibrationStagePayloadSource::InMemoryOutput {
                payload: stage_payload,
            })
        }
        CalibrationStageHandoffTransport::PersistedArtifactReference => {
            Ok(CalibrationStagePayloadSource::PersistedArtifactReference {
                artifact_reference: expected_stage_handoff_artifact_reference(
                    run_spec,
                    upstream_stage,
                    consuming_stage,
                ),
                payload: Some(stage_payload),
            })
        }
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
                .contains(&CalibrationStage::OptionRefinement)
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
        | CalibrationStage::NewsShocks
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
