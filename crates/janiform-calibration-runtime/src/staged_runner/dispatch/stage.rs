use std::path::Path;

use janiform_foundations::cancellation::CancellationToken;

use super::super::{
    CalibrationInMemoryStagePayloadState, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStagePayloadSource, CalibrationStageProgressPhase,
    CalibrationStageSelectedCandidateSource, CalibrationStagedRunFailure,
    CalibrationStagedRunInputs, CalibrationStagedRunObserverEvent,
    CalibrationStagedRunStageProgressEvent, CalibrationStagedRunStageReport,
    ExchangeCalendarRegistry, HawkesStageExecutionInput, HawkesStageFittedPayload,
    HawkesStageProgressUpdate, HawkesStageSeasonalityInput, JointRefinementStageProgressUpdate,
    LongVarianceStageExecutionInput, LongVarianceStageProgressUpdate,
    MeanReversionStageProgressUpdate, NewsShocksStageExecutionInput,
    ReturnBackfitStageExecutionInput, ReturnBackfitStageProgressUpdate,
    ReturnComponentsHawkesReturnSourceResponsibilityInput,
    ReturnComponentsStageCandidateGenerationInput, ReturnComponentsStageExecutionInput,
    ReturnComponentsStageProgressUpdate, run_hawkes_calibration_stage,
    run_hawkes_calibration_stage_with_progress_observer, run_long_variance_calibration_stage,
    run_long_variance_calibration_stage_with_progress_observer,
    run_mean_reversion_calibration_stage,
    run_mean_reversion_calibration_stage_with_progress_observer, run_news_shocks_calibration_stage,
    run_return_backfit_calibration_stage,
    run_return_backfit_calibration_stage_with_progress_observer,
    run_return_components_calibration_stage,
    run_return_components_calibration_stage_with_progress_observer,
    run_seasonality_calibration_stage,
};
use super::handoffs::{
    cloned_in_memory_stage_payload, expected_stage_handoff_artifact_reference,
    expected_stage_handoff_transport,
};
use super::joint_refinement::{
    JointRefinementLikeStageRunInput, forward_joint_refinement_like_stage_progress,
    joint_refinement_like_stage_input, run_joint_refinement_like_stage,
};
use crate::SeasonalityStageProgressUpdate;
use crate::stages::seasonality::run_seasonality_calibration_stage_with_progress_observer;

pub(in crate::staged_runner) fn run_stage_for_staged_execution(
    input: RunStageForStagedExecutionInput<'_, '_>,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    if input.cancellation_token.is_cancelled() {
        return Err(CalibrationStagedRunFailure::StageExecutionFailed {
            stage: input.stage,
            error_message: Some("operation cancelled".to_string()),
        });
    }

    match input.stage {
        CalibrationStage::Seasonality => run_seasonality_staged_execution_stage(input),
        CalibrationStage::Hawkes => run_hawkes_staged_execution_stage(input),
        CalibrationStage::ReturnComponents => run_return_components_staged_execution_stage(input),
        CalibrationStage::LongVariance => run_long_variance_staged_execution_stage(input),
        CalibrationStage::NewsShocks => run_news_shocks_staged_execution_stage(input),
        CalibrationStage::ReturnBackfit => run_return_backfit_staged_execution_stage(input),
        CalibrationStage::MeanReversion => run_mean_reversion_staged_execution_stage(input),
        CalibrationStage::JointRefinement
        | CalibrationStage::OptionRefinement
        | CalibrationStage::Finalization => run_joint_refinement_like_staged_execution_stage(input),
    }
}

fn run_seasonality_staged_execution_stage(
    input: RunStageForStagedExecutionInput<'_, '_>,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    let Some(seasonality_stage_input) = input.staged_run_inputs.seasonality_stage_input.take()
    else {
        return Err(CalibrationStagedRunFailure::MissingStageInput {
            stage: CalibrationStage::Seasonality,
        });
    };

    if input.observer.is_none() {
        return Ok(CalibrationStagedRunStageReport::Seasonality(Box::new(
            run_seasonality_calibration_stage(
                input.run_spec,
                input.calendar_registry,
                seasonality_stage_input,
            ),
        )));
    }

    let mut progress_forwarder = |progress_update: SeasonalityStageProgressUpdate| {
        forward_simple_stage_progress(
            input.observer,
            SimpleStageProgressForwardInput {
                run_spec: input.run_spec,
                stage: CalibrationStage::Seasonality,
                stage_index: input.stage_index,
                stage_count: input.stage_count,
                progress_phase: progress_update.phase,
                completed_units: progress_update.completed_units,
                total_units: progress_update.total_units,
                stage_progress_percent: Some(progress_update.stage_progress_percent()),
                aggregate_progress_percent: progress_update.aggregate_progress_percent,
                current_item_label: progress_update.current_item_label,
                current_evaluation_count: progress_update.current_evaluation_count,
                total_evaluation_budget: progress_update.total_evaluation_budget,
                evaluation_progress_percent: progress_update.evaluation_progress_percent,
                current_global_loss: progress_update.current_objective_value,
                best_global_loss: progress_update.best_objective_value,
                current_convergence_ratio: None,
            },
        );
    };

    Ok(CalibrationStagedRunStageReport::Seasonality(Box::new(
        run_seasonality_calibration_stage_with_progress_observer(
            input.run_spec,
            input.calendar_registry,
            seasonality_stage_input,
            Some(&mut progress_forwarder),
        ),
    )))
}

fn run_hawkes_staged_execution_stage(
    input: RunStageForStagedExecutionInput<'_, '_>,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    let Some(candidate_inputs) = input.staged_run_inputs.hawkes_candidate_inputs.take() else {
        return Err(CalibrationStagedRunFailure::MissingStageInput {
            stage: CalibrationStage::Hawkes,
        });
    };

    let seasonality_input = hawkes_seasonality_input(input.run_spec, input.payload_state)?;

    let event_side_codes = input
        .staged_run_inputs
        .hawkes_event_side_codes
        .clone()
        .unwrap_or_default();

    let stage_input = HawkesStageExecutionInput {
        seasonality_input,
        candidate_inputs,
        event_side_codes,
    };

    if input.observer.is_none() {
        return Ok(CalibrationStagedRunStageReport::Hawkes(Box::new(
            run_hawkes_calibration_stage(input.run_spec, input.calendar_registry, stage_input),
        )));
    }

    let mut progress_forwarder = |progress_update: HawkesStageProgressUpdate| {
        forward_simple_stage_progress(
            input.observer,
            SimpleStageProgressForwardInput {
                run_spec: input.run_spec,
                stage: CalibrationStage::Hawkes,
                stage_index: input.stage_index,
                stage_count: input.stage_count,
                progress_phase: progress_update.phase,
                completed_units: progress_update.completed_units,
                total_units: progress_update.total_units,
                stage_progress_percent: Some(progress_update.stage_progress_percent()),
                aggregate_progress_percent: progress_update.aggregate_progress_percent,
                current_item_label: progress_update.current_item_label,
                current_evaluation_count: progress_update.current_evaluation_count,
                total_evaluation_budget: progress_update.total_evaluation_budget,
                evaluation_progress_percent: progress_update.evaluation_progress_percent,
                current_global_loss: progress_update.current_weighted_objective,
                best_global_loss: progress_update.best_weighted_objective,
                current_convergence_ratio: progress_update.current_convergence_ratio,
            },
        );
    };

    Ok(CalibrationStagedRunStageReport::Hawkes(Box::new(
        run_hawkes_calibration_stage_with_progress_observer(
            input.run_spec,
            input.calendar_registry,
            stage_input,
            Some(&mut progress_forwarder),
        ),
    )))
}

fn hawkes_seasonality_input(
    run_spec: &CalibrationRunSpec,
    payload_state: &CalibrationInMemoryStagePayloadState,
) -> Result<HawkesStageSeasonalityInput, CalibrationStagedRunFailure> {
    let seasonality_stage_payload = cloned_in_memory_stage_payload(
        &payload_state.seasonality_stage_payload,
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
    )?;

    match expected_stage_handoff_transport(
        run_spec,
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
    ) {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            Ok(HawkesStageSeasonalityInput::InMemoryOutput {
                payload: seasonality_stage_payload,
            })
        }
        CalibrationStageHandoffTransport::PersistedArtifactReference => {
            Ok(HawkesStageSeasonalityInput::PersistedArtifactReference {
                artifact_reference: expected_stage_handoff_artifact_reference(
                    run_spec,
                    CalibrationStage::Seasonality,
                    CalibrationStage::Hawkes,
                ),
                payload: seasonality_stage_payload,
            })
        }
    }
}

fn run_return_components_staged_execution_stage(
    input: RunStageForStagedExecutionInput<'_, '_>,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    let candidate_generation_input = return_components_candidate_generation_input(
        input.staged_run_inputs,
        input.payload_state,
        CalibrationStage::ReturnComponents,
    )?;

    let stage_input = ReturnComponentsStageExecutionInput {
        activity_input: return_components_activity_input(input.run_spec, input.payload_state)?,
        hawkes_return_source_responsibility_input:
            return_components_hawkes_return_source_responsibility_input(&input)?,
        coarse_data_records: input.staged_run_inputs.coarse_data_records.clone(),
        candidate_generation_input,
    };

    if input.observer.is_none() {
        return Ok(CalibrationStagedRunStageReport::ReturnComponents(Box::new(
            run_return_components_calibration_stage(
                input.run_spec,
                input.calendar_registry,
                stage_input,
            ),
        )));
    }

    let mut progress_forwarder = |progress_update: ReturnComponentsStageProgressUpdate| {
        forward_return_components_stage_progress(
            input.observer,
            input.run_spec,
            input.stage_index,
            input.stage_count,
            progress_update,
        );
    };

    Ok(CalibrationStagedRunStageReport::ReturnComponents(Box::new(
        run_return_components_calibration_stage_with_progress_observer(
            input.run_spec,
            input.calendar_registry,
            stage_input,
            Some(&mut progress_forwarder),
        ),
    )))
}

fn return_components_activity_input(
    run_spec: &CalibrationRunSpec,
    payload_state: &CalibrationInMemoryStagePayloadState,
) -> Result<CalibrationStagePayloadSource<HawkesStageFittedPayload>, CalibrationStagedRunFailure> {
    match expected_stage_handoff_transport(
        run_spec,
        CalibrationStage::Hawkes,
        CalibrationStage::ReturnComponents,
    ) {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            let hawkes_stage_payload = cloned_in_memory_stage_payload(
                &payload_state.hawkes_stage_payload,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            )?;

            Ok(CalibrationStagePayloadSource::InMemoryOutput {
                payload: hawkes_stage_payload,
            })
        }
        CalibrationStageHandoffTransport::PersistedArtifactReference => {
            Ok(CalibrationStagePayloadSource::PersistedArtifactReference {
                artifact_reference: expected_stage_handoff_artifact_reference(
                    run_spec,
                    CalibrationStage::Hawkes,
                    CalibrationStage::ReturnComponents,
                ),
                // Persisted handoffs can carry an in-memory copy for same-process consumers.
                payload: payload_state.hawkes_stage_payload.clone(),
            })
        }
    }
}

fn return_components_hawkes_return_source_responsibility_input(
    input: &RunStageForStagedExecutionInput<'_, '_>,
) -> Result<
    Option<ReturnComponentsHawkesReturnSourceResponsibilityInput>,
    CalibrationStagedRunFailure,
> {
    let seasonality_payload = cloned_in_memory_stage_payload(
        &input.payload_state.seasonality_stage_payload,
        CalibrationStage::Seasonality,
        CalibrationStage::ReturnComponents,
    )?;

    let Some(event_side_codes) = input.staged_run_inputs.hawkes_event_side_codes.clone() else {
        return Err(CalibrationStagedRunFailure::MissingStageInput {
            stage: CalibrationStage::Hawkes,
        });
    };

    Ok(Some(
        ReturnComponentsHawkesReturnSourceResponsibilityInput {
            seasonality_payload,
            event_side_codes,
        },
    ))
}

pub(super) fn return_components_candidate_generation_input(
    staged_run_inputs: &CalibrationStagedRunInputs,
    _payload_state: &CalibrationInMemoryStagePayloadState,
    consuming_stage: CalibrationStage,
) -> Result<ReturnComponentsStageCandidateGenerationInput, CalibrationStagedRunFailure> {
    // Intentional copy because the candidate template is reused by return components
    // and later stages.
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

fn forward_return_components_stage_progress(
    observer: &mut Option<&mut dyn FnMut(CalibrationStagedRunObserverEvent)>,
    run_spec: &CalibrationRunSpec,
    stage_index: usize,
    stage_count: usize,
    progress_update: ReturnComponentsStageProgressUpdate,
) {
    if let Some(observer) = observer.as_deref_mut() {
        // Observer events own their payloads while the run spec stays borrowed by dispatch.
        observer(CalibrationStagedRunObserverEvent::StageProgress(Box::new(
            CalibrationStagedRunStageProgressEvent {
                run_id: run_spec.run_id.clone(),
                stage: CalibrationStage::ReturnComponents,
                stage_index,
                stage_count,
                progress_phase: progress_update.phase.into(),
                completed_units: progress_update.completed_units,
                total_units: progress_update.total_units,
                stage_progress_percent: None,
                aggregate_progress_percent: progress_update.aggregate_progress_percent,
                current_item_label: progress_update.current_item_label,
                current_restart_id: None,
                current_work_item_status: None,
                planned_work_item_labels: Vec::new(),
                active_restart_count: None,
                current_evaluation_count: progress_update.current_evaluation_count,
                total_evaluation_budget: progress_update.total_evaluation_budget,
                evaluation_progress_percent: progress_update.evaluation_progress_percent,
                current_pricing_interval_index: None,
                total_pricing_interval_count: None,
                completed_pricing_row_count: None,
                total_pricing_row_count: None,
                valid_completed_units: None,
                cumulative_local_evaluation_count: None,
                current_global_loss: progress_update.current_weighted_objective,
                current_restart_best_global_loss: None,
                current_final_global_loss: None,
                current_candidate_assessment: None,
                best_restart_id: None,
                best_global_loss: progress_update.best_weighted_objective,
                current_coarse_consistency_fit: None,
                best_coarse_consistency_fit: None,
                exact_closed_form_buy_count: None,
                exact_closed_form_sell_count: None,
                exact_tilted_mixed_count: None,
                restart_evaluation_timing_diagnostic: None,
                current_convergence_ratio: progress_update.current_convergence_ratio,
            },
        )));
    }
}

struct SimpleStageProgressForwardInput<'run_spec, Phase> {
    run_spec: &'run_spec CalibrationRunSpec,
    stage: CalibrationStage,
    stage_index: usize,
    stage_count: usize,
    progress_phase: Phase,
    completed_units: usize,
    total_units: usize,
    stage_progress_percent: Option<usize>,
    aggregate_progress_percent: Option<usize>,
    current_item_label: Option<String>,
    current_evaluation_count: Option<usize>,
    total_evaluation_budget: Option<usize>,
    evaluation_progress_percent: Option<usize>,
    current_global_loss: Option<f64>,
    best_global_loss: Option<f64>,
    current_convergence_ratio: Option<f64>,
}

fn forward_simple_stage_progress<Phase>(
    observer: &mut Option<&mut dyn FnMut(CalibrationStagedRunObserverEvent)>,
    input: SimpleStageProgressForwardInput<'_, Phase>,
) where
    Phase: Into<CalibrationStageProgressPhase>,
{
    if let Some(observer) = observer.as_deref_mut() {
        observer(CalibrationStagedRunObserverEvent::StageProgress(Box::new(
            CalibrationStagedRunStageProgressEvent {
                run_id: input.run_spec.run_id.clone(),
                stage: input.stage,
                stage_index: input.stage_index,
                stage_count: input.stage_count,
                progress_phase: input.progress_phase.into(),
                completed_units: input.completed_units,
                total_units: input.total_units,
                stage_progress_percent: input.stage_progress_percent,
                aggregate_progress_percent: input.aggregate_progress_percent,
                current_item_label: input.current_item_label,
                current_restart_id: None,
                current_work_item_status: None,
                planned_work_item_labels: Vec::new(),
                active_restart_count: None,
                current_evaluation_count: input.current_evaluation_count,
                total_evaluation_budget: input.total_evaluation_budget,
                evaluation_progress_percent: input.evaluation_progress_percent,
                current_pricing_interval_index: None,
                total_pricing_interval_count: None,
                completed_pricing_row_count: None,
                total_pricing_row_count: None,
                valid_completed_units: None,
                cumulative_local_evaluation_count: None,
                current_global_loss: input.current_global_loss,
                current_restart_best_global_loss: None,
                current_final_global_loss: None,
                current_candidate_assessment: None,
                best_restart_id: None,
                best_global_loss: input.best_global_loss,
                current_coarse_consistency_fit: None,
                best_coarse_consistency_fit: None,
                exact_closed_form_buy_count: None,
                exact_closed_form_sell_count: None,
                exact_tilted_mixed_count: None,
                restart_evaluation_timing_diagnostic: None,
                current_convergence_ratio: input.current_convergence_ratio,
            },
        )));
    }
}

fn run_long_variance_staged_execution_stage(
    input: RunStageForStagedExecutionInput<'_, '_>,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    let stage_input = LongVarianceStageExecutionInput {
        return_components_input: Some(return_components_selected_candidate_source(
            input.run_spec,
            CalibrationStage::ReturnComponents,
            CalibrationStage::LongVariance,
            input.payload_state,
        )?),
        hawkes_stage_payload: input.payload_state.hawkes_stage_payload.clone(),
        return_components_stage_payload: input
            .payload_state
            .return_components_stage_payload
            .clone(),
        candidate_generation_input: Some(return_components_candidate_generation_input(
            input.staged_run_inputs,
            input.payload_state,
            CalibrationStage::LongVariance,
        )?),
        coarse_data_records: input.staged_run_inputs.coarse_data_records.clone(),
        tick_objective_data_usage_policy: input.staged_run_inputs.tick_objective_data_usage_policy,
    };

    if input.observer.is_none() {
        return Ok(CalibrationStagedRunStageReport::LongVariance(Box::new(
            run_long_variance_calibration_stage(
                input.run_spec,
                input.calendar_registry,
                stage_input,
            ),
        )));
    }

    let mut progress_forwarder = |progress_update: LongVarianceStageProgressUpdate| {
        forward_simple_stage_progress(
            input.observer,
            SimpleStageProgressForwardInput {
                run_spec: input.run_spec,
                stage: CalibrationStage::LongVariance,
                stage_index: input.stage_index,
                stage_count: input.stage_count,
                progress_phase: progress_update.phase,
                completed_units: progress_update.completed_units,
                total_units: progress_update.total_units,
                stage_progress_percent: Some(progress_update.stage_progress_percent()),
                aggregate_progress_percent: progress_update.aggregate_progress_percent,
                current_item_label: progress_update.current_item_label,
                current_evaluation_count: progress_update.current_evaluation_count,
                total_evaluation_budget: progress_update.total_evaluation_budget,
                evaluation_progress_percent: progress_update.evaluation_progress_percent,
                current_global_loss: None,
                best_global_loss: None,
                current_convergence_ratio: None,
            },
        );
    };

    Ok(CalibrationStagedRunStageReport::LongVariance(Box::new(
        run_long_variance_calibration_stage_with_progress_observer(
            input.run_spec,
            input.calendar_registry,
            stage_input,
            Some(&mut progress_forwarder),
        ),
    )))
}

fn run_news_shocks_staged_execution_stage(
    input: RunStageForStagedExecutionInput<'_, '_>,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    let stage_input = NewsShocksStageExecutionInput {
        return_components_input: Some(return_components_selected_candidate_source(
            input.run_spec,
            CalibrationStage::ReturnComponents,
            CalibrationStage::NewsShocks,
            input.payload_state,
        )?),
        return_components_stage_payload: input
            .payload_state
            .return_components_stage_payload
            .clone(),
    };

    Ok(CalibrationStagedRunStageReport::NewsShocks(Box::new(
        run_news_shocks_calibration_stage(input.run_spec, stage_input),
    )))
}

fn run_mean_reversion_staged_execution_stage(
    input: RunStageForStagedExecutionInput<'_, '_>,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    let Some(mut stage_input) = input.staged_run_inputs.mean_reversion_stage_input.take() else {
        return Err(CalibrationStagedRunFailure::MissingStageInput {
            stage: CalibrationStage::MeanReversion,
        });
    };

    stage_input.return_components_input = Some(return_components_selected_candidate_source(
        input.run_spec,
        effective_return_components_upstream_stage(input.run_spec),
        CalibrationStage::MeanReversion,
        input.payload_state,
    )?);

    // Mean-reversion input owns optional upstream payload attachments.
    stage_input.seasonality_stage_input =
        input.payload_state.mean_reversion_seasonality_input.clone();
    stage_input.hawkes_stage_payload = input.payload_state.hawkes_stage_payload.clone();
    stage_input.return_components_stage_payload =
        input.payload_state.return_components_stage_payload.clone();

    if input.observer.is_none() {
        return Ok(CalibrationStagedRunStageReport::MeanReversion(Box::new(
            run_mean_reversion_calibration_stage(
                input.run_spec,
                input.calendar_registry,
                stage_input,
            ),
        )));
    }

    let mut progress_forwarder = |progress_update: MeanReversionStageProgressUpdate| {
        forward_simple_stage_progress(
            input.observer,
            SimpleStageProgressForwardInput {
                run_spec: input.run_spec,
                stage: CalibrationStage::MeanReversion,
                stage_index: input.stage_index,
                stage_count: input.stage_count,
                progress_phase: progress_update.phase,
                completed_units: progress_update.completed_units,
                total_units: progress_update.total_units,
                stage_progress_percent: Some(progress_update.stage_progress_percent()),
                aggregate_progress_percent: progress_update.aggregate_progress_percent,
                current_item_label: progress_update.current_item_label,
                current_evaluation_count: progress_update.current_evaluation_count,
                total_evaluation_budget: progress_update.total_evaluation_budget,
                evaluation_progress_percent: progress_update.evaluation_progress_percent,
                current_global_loss: None,
                best_global_loss: None,
                current_convergence_ratio: None,
            },
        );
    };

    Ok(CalibrationStagedRunStageReport::MeanReversion(Box::new(
        run_mean_reversion_calibration_stage_with_progress_observer(
            input.run_spec,
            input.calendar_registry,
            stage_input,
            Some(&mut progress_forwarder),
        ),
    )))
}

fn run_return_backfit_staged_execution_stage(
    input: RunStageForStagedExecutionInput<'_, '_>,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    let stage_input = ReturnBackfitStageExecutionInput {
        return_components_input: Some(return_components_selected_candidate_source(
            input.run_spec,
            CalibrationStage::ReturnComponents,
            CalibrationStage::ReturnBackfit,
            input.payload_state,
        )?),
        hawkes_stage_payload: input.payload_state.hawkes_stage_payload.clone(),
        candidate_generation_input: Some(return_components_candidate_generation_input(
            input.staged_run_inputs,
            input.payload_state,
            CalibrationStage::ReturnBackfit,
        )?),
        coarse_data_records: input.staged_run_inputs.coarse_data_records.clone(),
        return_components_stage_payload: input
            .payload_state
            .return_components_stage_payload
            .clone(),
        long_variance_stage_payload: input.payload_state.long_variance_stage_payload.clone(),
        news_shocks_stage_payload: input.payload_state.news_shocks_stage_payload.clone(),
    };

    if input.observer.is_none() {
        return Ok(CalibrationStagedRunStageReport::ReturnBackfit(Box::new(
            run_return_backfit_calibration_stage(input.run_spec, stage_input),
        )));
    }

    let mut progress_forwarder = |progress_update: ReturnBackfitStageProgressUpdate| {
        forward_simple_stage_progress(
            input.observer,
            SimpleStageProgressForwardInput {
                run_spec: input.run_spec,
                stage: CalibrationStage::ReturnBackfit,
                stage_index: input.stage_index,
                stage_count: input.stage_count,
                progress_phase: progress_update.phase,
                completed_units: progress_update.completed_units,
                total_units: progress_update.total_units,
                stage_progress_percent: Some(progress_update.stage_progress_percent()),
                aggregate_progress_percent: progress_update.aggregate_progress_percent,
                current_item_label: progress_update.current_item_label,
                current_evaluation_count: progress_update.current_evaluation_count,
                total_evaluation_budget: progress_update.total_evaluation_budget,
                evaluation_progress_percent: progress_update.evaluation_progress_percent,
                current_global_loss: None,
                best_global_loss: None,
                current_convergence_ratio: None,
            },
        );
    };

    Ok(CalibrationStagedRunStageReport::ReturnBackfit(Box::new(
        run_return_backfit_calibration_stage_with_progress_observer(
            input.run_spec,
            stage_input,
            Some(&mut progress_forwarder),
        ),
    )))
}

fn return_components_selected_candidate_source(
    run_spec: &CalibrationRunSpec,
    upstream_stage: CalibrationStage,
    consuming_stage: CalibrationStage,
    payload_state: &CalibrationInMemoryStagePayloadState,
) -> Result<CalibrationStageSelectedCandidateSource, CalibrationStagedRunFailure> {
    match expected_stage_handoff_transport(run_spec, upstream_stage, consuming_stage) {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            let Some(return_components_stage_payload) =
                payload_state.return_components_stage_payload.as_ref()
            else {
                return Err(
                    CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                        required_stage: upstream_stage,
                        consuming_stage,
                    },
                );
            };

            Ok(CalibrationStageSelectedCandidateSource::InMemoryOutput {
                selected_candidate_id: return_components_stage_payload
                    .selected_candidate_id
                    .clone(),
            })
        }
        CalibrationStageHandoffTransport::PersistedArtifactReference => Ok(
            CalibrationStageSelectedCandidateSource::PersistedArtifactReference {
                artifact_reference: expected_stage_handoff_artifact_reference(
                    run_spec,
                    upstream_stage,
                    consuming_stage,
                ),
            },
        ),
    }
}

pub(super) fn effective_return_components_upstream_stage(
    run_spec: &CalibrationRunSpec,
) -> CalibrationStage {
    if run_spec
        .selected_stages
        .contains(&CalibrationStage::ReturnBackfit)
    {
        CalibrationStage::ReturnBackfit
    } else {
        CalibrationStage::ReturnComponents
    }
}

fn run_joint_refinement_like_staged_execution_stage(
    input: RunStageForStagedExecutionInput<'_, '_>,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    let stage_input = joint_refinement_like_stage_input(
        input.run_spec,
        input.stage,
        input.staged_run_inputs,
        input.payload_state,
    )?;

    let upstream_joint_refinement_handoff = input
        .payload_state
        .joint_refinement_runtime_handoff
        .as_ref();

    let upstream_option_refinement_handoff = input
        .payload_state
        .option_refinement_runtime_handoff
        .as_ref();

    if input.observer.is_none() {
        return run_joint_refinement_like_stage(JointRefinementLikeStageRunInput {
            stage: input.stage,
            run_spec: input.run_spec,
            calendar_registry: input.calendar_registry,
            stage_input,
            upstream_joint_refinement_handoff,
            upstream_option_refinement_handoff,
            progress_observer: None,
            disk_checkpoint_directory: input.disk_checkpoint_directory,
            load_current_stage_resume_state: input.load_current_stage_resume_state,
            cancellation_token: input.cancellation_token.shared_handle(),
        });
    }

    let mut progress_forwarder = |progress_update: JointRefinementStageProgressUpdate| {
        forward_joint_refinement_like_stage_progress(
            input.observer,
            input.run_spec,
            input.stage,
            input.stage_index,
            input.stage_count,
            progress_update,
        );
    };

    run_joint_refinement_like_stage(JointRefinementLikeStageRunInput {
        stage: input.stage,
        run_spec: input.run_spec,
        calendar_registry: input.calendar_registry,
        stage_input,
        upstream_joint_refinement_handoff,
        upstream_option_refinement_handoff,
        progress_observer: Some(&mut progress_forwarder),
        disk_checkpoint_directory: input.disk_checkpoint_directory,
        load_current_stage_resume_state: input.load_current_stage_resume_state,
        cancellation_token: input.cancellation_token.shared_handle(),
    })
}

pub(in crate::staged_runner) struct RunStageForStagedExecutionInput<'a, 'observer> {
    pub(in crate::staged_runner) run_spec: &'a CalibrationRunSpec,
    pub(in crate::staged_runner) calendar_registry: &'a ExchangeCalendarRegistry,
    pub(in crate::staged_runner) disk_checkpoint_directory: Option<&'a Path>,
    pub(in crate::staged_runner) stage: CalibrationStage,
    pub(in crate::staged_runner) stage_index: usize,
    pub(in crate::staged_runner) stage_count: usize,
    pub(in crate::staged_runner) staged_run_inputs: &'a mut CalibrationStagedRunInputs,
    pub(in crate::staged_runner) payload_state: &'a CalibrationInMemoryStagePayloadState,
    pub(in crate::staged_runner) load_current_stage_resume_state: bool,
    pub(in crate::staged_runner) cancellation_token: CancellationToken,
    pub(in crate::staged_runner) observer:
        &'a mut Option<&'observer mut dyn FnMut(CalibrationStagedRunObserverEvent)>,
}
