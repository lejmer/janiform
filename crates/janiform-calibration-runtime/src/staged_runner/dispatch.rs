use std::path::Path;

use super::payload_state::joint_refinement_seasonality_factors_from_payload;
use super::{
    CalibrationExecutionMode, CalibrationInMemoryStagePayloadState, CalibrationRunSpec,
    CalibrationStage, CalibrationStageHandoffTransport, CalibrationStagedRunFailure,
    CalibrationStagedRunInputs, CalibrationStagedRunObserverEvent, CalibrationStagedRunStageReport,
    ExchangeCalendarRegistry, HawkesStageExecutionInput, HawkesStageSeasonalityInput,
    JointRefinementStageExecutionInput, JointRefinementStageHawkesInput,
    JointRefinementStageReturnComponentsInput, MeanReversionStageReturnComponentsInput,
    ReturnComponentsStageActivityInput, ReturnComponentsStageExecutionInput,
    run_hawkes_calibration_stage,
    run_joint_refinement_calibration_stage_with_progress_observer_and_persistence,
    run_mean_reversion_calibration_stage, run_return_components_calibration_stage,
    run_return_components_calibration_stage_with_progress_observer,
    run_seasonality_calibration_stage, stage_output_artifact_references_for_stage,
};
use crate::JointRefinementStageMeanReversionInput;

pub(super) fn run_stage_for_staged_execution(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    disk_checkpoint_directory: Option<&Path>,
    stage: CalibrationStage,
    stage_index: usize,
    stage_count: usize,
    staged_run_inputs: &mut CalibrationStagedRunInputs,
    payload_state: &CalibrationInMemoryStagePayloadState,
    observer: &mut Option<&mut dyn FnMut(CalibrationStagedRunObserverEvent)>,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    match stage {
        CalibrationStage::Seasonality => {
            let Some(seasonality_stage_input) = staged_run_inputs.seasonality_stage_input.take()
            else {
                return Err(CalibrationStagedRunFailure::MissingStageInput {
                    stage: CalibrationStage::Seasonality,
                });
            };

            Ok(CalibrationStagedRunStageReport::Seasonality(
                run_seasonality_calibration_stage(
                    run_spec,
                    calendar_registry,
                    seasonality_stage_input,
                ),
            ))
        }
        CalibrationStage::Hawkes => {
            let Some(candidate_inputs) = staged_run_inputs.hawkes_candidate_inputs.take() else {
                return Err(CalibrationStagedRunFailure::MissingStageInput {
                    stage: CalibrationStage::Hawkes,
                });
            };

            let seasonality_input_transport = expected_stage_handoff_transport(
                run_spec,
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
            );

            let seasonality_input = match seasonality_input_transport {
                CalibrationStageHandoffTransport::InMemoryOutput => {
                    let Some(seasonality_stage_payload) =
                        payload_state.seasonality_stage_payload.clone()
                    else {
                        return Err(
                            CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                                required_stage: CalibrationStage::Seasonality,
                                consuming_stage: CalibrationStage::Hawkes,
                            },
                        );
                    };

                    HawkesStageSeasonalityInput::InMemoryOutput {
                        payload: seasonality_stage_payload,
                    }
                }
                CalibrationStageHandoffTransport::PersistedArtifactReference => {
                    let Some(seasonality_stage_payload) =
                        payload_state.seasonality_stage_payload.clone()
                    else {
                        return Err(
                            CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                                required_stage: CalibrationStage::Seasonality,
                                consuming_stage: CalibrationStage::Hawkes,
                            },
                        );
                    };

                    HawkesStageSeasonalityInput::PersistedArtifactReference {
                        artifact_reference: expected_stage_handoff_artifact_reference(
                            run_spec,
                            CalibrationStage::Seasonality,
                            CalibrationStage::Hawkes,
                        ),
                        payload: seasonality_stage_payload,
                    }
                }
            };

            Ok(CalibrationStagedRunStageReport::Hawkes(
                run_hawkes_calibration_stage(
                    run_spec,
                    calendar_registry,
                    HawkesStageExecutionInput {
                        seasonality_input,
                        candidate_inputs,
                    },
                ),
            ))
        }
        CalibrationStage::ReturnComponents => {
            let Some(candidate_generation_input) = staged_run_inputs
                .return_components_candidate_generation_input
                .clone()
            else {
                return Err(CalibrationStagedRunFailure::MissingStageInput {
                    stage: CalibrationStage::ReturnComponents,
                });
            };

            let activity_input_transport = expected_stage_handoff_transport(
                run_spec,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            );

            let activity_input = match activity_input_transport {
                CalibrationStageHandoffTransport::InMemoryOutput => {
                    let Some(hawkes_stage_payload) = payload_state.hawkes_stage_payload.clone()
                    else {
                        return Err(
                            CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                                required_stage: CalibrationStage::Hawkes,
                                consuming_stage: CalibrationStage::ReturnComponents,
                            },
                        );
                    };

                    ReturnComponentsStageActivityInput::InMemoryOutput {
                        payload: hawkes_stage_payload,
                    }
                }
                CalibrationStageHandoffTransport::PersistedArtifactReference => {
                    ReturnComponentsStageActivityInput::PersistedArtifactReference {
                        artifact_reference: expected_stage_handoff_artifact_reference(
                            run_spec,
                            CalibrationStage::Hawkes,
                            CalibrationStage::ReturnComponents,
                        ),
                        payload: payload_state.hawkes_stage_payload.clone(),
                    }
                }
            };

            let stage_input = ReturnComponentsStageExecutionInput {
                activity_input,
                candidate_generation_input,
            };

            if observer.is_none() {
                return Ok(CalibrationStagedRunStageReport::ReturnComponents(
                    run_return_components_calibration_stage(
                        run_spec,
                        calendar_registry,
                        stage_input,
                    ),
                ));
            }

            let mut progress_forwarder =
                |progress_update: super::ReturnComponentsStageProgressUpdate| {
                    if let Some(observer) = observer.as_deref_mut() {
                        observer(CalibrationStagedRunObserverEvent::StageProgress {
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
                            active_restart_count: None,
                            current_parameter_block: None,
                            completed_outer_cycles: None,
                            total_outer_cycles: None,
                            current_evaluation_count: progress_update.current_evaluation_count,
                            total_evaluation_budget: progress_update.total_evaluation_budget,
                            evaluation_progress_percent: progress_update
                                .evaluation_progress_percent,
                            valid_completed_units: None,
                            cumulative_local_evaluation_count: None,
                            current_weighted_objective: None,
                            current_restart_best_weighted_objective: None,
                            best_restart_id: None,
                            best_weighted_objective: None,
                            current_coarse_consistency_fit: None,
                            best_coarse_consistency_fit: None,
                            current_convergence_ratio: progress_update.current_convergence_ratio,
                        });
                    }
                };

            Ok(CalibrationStagedRunStageReport::ReturnComponents(
                run_return_components_calibration_stage_with_progress_observer(
                    run_spec,
                    calendar_registry,
                    stage_input,
                    Some(&mut progress_forwarder),
                ),
            ))
        }
        CalibrationStage::MeanReversion => {
            let Some(mut stage_input) = staged_run_inputs.mean_reversion_stage_input.take() else {
                return Err(CalibrationStagedRunFailure::MissingStageInput {
                    stage: CalibrationStage::MeanReversion,
                });
            };

            let return_components_input_transport = expected_stage_handoff_transport(
                run_spec,
                CalibrationStage::ReturnComponents,
                CalibrationStage::MeanReversion,
            );

            stage_input.return_components_input = Some(match return_components_input_transport {
                CalibrationStageHandoffTransport::InMemoryOutput => {
                    let Some(return_components_stage_payload) =
                        payload_state.return_components_stage_payload.as_ref()
                    else {
                        return Err(
                            CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                                required_stage: CalibrationStage::ReturnComponents,
                                consuming_stage: CalibrationStage::MeanReversion,
                            },
                        );
                    };

                    MeanReversionStageReturnComponentsInput::InMemoryOutput {
                        selected_candidate_id: return_components_stage_payload
                            .selected_candidate_id
                            .clone(),
                    }
                }
                CalibrationStageHandoffTransport::PersistedArtifactReference => {
                    MeanReversionStageReturnComponentsInput::PersistedArtifactReference {
                        artifact_reference: expected_stage_handoff_artifact_reference(
                            run_spec,
                            CalibrationStage::ReturnComponents,
                            CalibrationStage::MeanReversion,
                        ),
                    }
                }
            });

            stage_input.seasonality_stage_input =
                payload_state.mean_reversion_seasonality_input.clone();
            stage_input.hawkes_stage_payload = payload_state.hawkes_stage_payload.clone();
            stage_input.return_components_stage_payload =
                payload_state.return_components_stage_payload.clone();

            Ok(CalibrationStagedRunStageReport::MeanReversion(
                run_mean_reversion_calibration_stage(run_spec, calendar_registry, stage_input),
            ))
        }
        CalibrationStage::JointRefinement => {
            let Some(seasonality_factors_input) = payload_state
                .joint_refinement_seasonality_factors_input
                .clone()
                .or_else(|| {
                    payload_state
                        .seasonality_stage_payload
                        .as_ref()
                        .map(joint_refinement_seasonality_factors_from_payload)
                })
            else {
                return Err(
                    CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                        required_stage: CalibrationStage::Seasonality,
                        consuming_stage: CalibrationStage::JointRefinement,
                    },
                );
            };

            let Some(candidate_generation_input) = staged_run_inputs
                .return_components_candidate_generation_input
                .clone()
            else {
                return Err(CalibrationStagedRunFailure::MissingStageInput {
                    stage: CalibrationStage::JointRefinement,
                });
            };

            let hawkes_input_transport = expected_stage_handoff_transport(
                run_spec,
                CalibrationStage::Hawkes,
                CalibrationStage::JointRefinement,
            );

            let Some(hawkes_stage_payload) = payload_state.hawkes_stage_payload.clone() else {
                return Err(
                    CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                        required_stage: CalibrationStage::Hawkes,
                        consuming_stage: CalibrationStage::JointRefinement,
                    },
                );
            };

            let hawkes_input = match hawkes_input_transport {
                CalibrationStageHandoffTransport::InMemoryOutput => {
                    JointRefinementStageHawkesInput::InMemoryOutput {
                        payload: hawkes_stage_payload,
                    }
                }
                CalibrationStageHandoffTransport::PersistedArtifactReference => {
                    JointRefinementStageHawkesInput::PersistedArtifactReference {
                        artifact_reference: expected_stage_handoff_artifact_reference(
                            run_spec,
                            CalibrationStage::Hawkes,
                            CalibrationStage::JointRefinement,
                        ),
                        payload: Some(hawkes_stage_payload),
                    }
                }
            };

            let return_components_input_transport = expected_stage_handoff_transport(
                run_spec,
                CalibrationStage::ReturnComponents,
                CalibrationStage::JointRefinement,
            );

            let Some(return_components_stage_payload) =
                payload_state.return_components_stage_payload.clone()
            else {
                return Err(
                    CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                        required_stage: CalibrationStage::ReturnComponents,
                        consuming_stage: CalibrationStage::JointRefinement,
                    },
                );
            };

            let return_components_input = match return_components_input_transport {
                CalibrationStageHandoffTransport::InMemoryOutput => {
                    JointRefinementStageReturnComponentsInput::InMemoryOutput {
                        payload: return_components_stage_payload,
                    }
                }
                CalibrationStageHandoffTransport::PersistedArtifactReference => {
                    JointRefinementStageReturnComponentsInput::PersistedArtifactReference {
                        artifact_reference: expected_stage_handoff_artifact_reference(
                            run_spec,
                            CalibrationStage::ReturnComponents,
                            CalibrationStage::JointRefinement,
                        ),
                        payload: Some(return_components_stage_payload),
                    }
                }
            };

            let mean_reversion_input_transport = expected_stage_handoff_transport(
                run_spec,
                CalibrationStage::MeanReversion,
                CalibrationStage::JointRefinement,
            );

            let Some(mean_reversion_stage_payload) =
                payload_state.mean_reversion_stage_payload.clone()
            else {
                return Err(
                    CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                        required_stage: CalibrationStage::MeanReversion,
                        consuming_stage: CalibrationStage::JointRefinement,
                    },
                );
            };

            let mean_reversion_input = match mean_reversion_input_transport {
                CalibrationStageHandoffTransport::InMemoryOutput => {
                    JointRefinementStageMeanReversionInput::InMemoryOutput {
                        payload: mean_reversion_stage_payload,
                    }
                }
                CalibrationStageHandoffTransport::PersistedArtifactReference => {
                    JointRefinementStageMeanReversionInput::PersistedArtifactReference {
                        artifact_reference: expected_stage_handoff_artifact_reference(
                            run_spec,
                            CalibrationStage::MeanReversion,
                            CalibrationStage::JointRefinement,
                        ),
                        payload: Some(mean_reversion_stage_payload),
                    }
                }
            };

            let stage_input = JointRefinementStageExecutionInput {
                seasonality_factors_input,
                hawkes_input,
                return_components_input,
                mean_reversion_input,
                candidate_generation_input,
                restart_candidate_budget_override: staged_run_inputs
                    .joint_refinement_restart_candidate_budget_override,
                option_refinement_candidate_count_override: staged_run_inputs
                    .joint_refinement_option_refinement_candidate_count_override,
                audit_option_pricing_enabled: staged_run_inputs
                    .joint_refinement_audit_option_pricing_enabled,
                option_pricing_input: staged_run_inputs
                    .joint_refinement_option_pricing_input
                    .clone(),
            };

            if observer.is_none() {
                return Ok(CalibrationStagedRunStageReport::JointRefinement(
                    run_joint_refinement_calibration_stage_with_progress_observer_and_persistence(
                        run_spec,
                        calendar_registry,
                        stage_input,
                        None,
                        disk_checkpoint_directory,
                    ),
                ));
            }

            let mut progress_forwarder =
                |progress_update: super::JointRefinementStageProgressUpdate| {
                    if let Some(observer) = observer.as_deref_mut() {
                        observer(CalibrationStagedRunObserverEvent::StageProgress {
                            run_id: run_spec.run_id.clone(),
                            stage: CalibrationStage::JointRefinement,
                            stage_index,
                            stage_count,
                            progress_phase: progress_update.phase.into(),
                            completed_units: progress_update.completed_units,
                            total_units: progress_update.total_units,
                            stage_progress_percent: Some(progress_update.stage_progress_percent()),
                            aggregate_progress_percent: progress_update.aggregate_progress_percent,
                            current_item_label: progress_update.current_item_label,
                            current_restart_id: progress_update.current_restart_id,
                            active_restart_count: progress_update.active_restart_count,
                            current_parameter_block: progress_update.current_parameter_block,
                            completed_outer_cycles: progress_update.completed_outer_cycles,
                            total_outer_cycles: progress_update.total_outer_cycles,
                            current_evaluation_count: progress_update.current_evaluation_count,
                            total_evaluation_budget: progress_update.total_evaluation_budget,
                            evaluation_progress_percent: progress_update
                                .evaluation_progress_percent,
                            valid_completed_units: progress_update.valid_completed_units,
                            cumulative_local_evaluation_count: progress_update
                                .cumulative_local_evaluation_count,
                            current_weighted_objective: progress_update.current_weighted_objective,
                            current_restart_best_weighted_objective: progress_update
                                .current_restart_best_weighted_objective,
                            best_restart_id: progress_update.best_restart_id,
                            best_weighted_objective: progress_update.best_weighted_objective,
                            current_coarse_consistency_fit: progress_update
                                .current_coarse_consistency_fit,
                            best_coarse_consistency_fit: progress_update
                                .best_coarse_consistency_fit,
                            current_convergence_ratio: None,
                        });
                    }
                };

            Ok(CalibrationStagedRunStageReport::JointRefinement(
                run_joint_refinement_calibration_stage_with_progress_observer_and_persistence(
                    run_spec,
                    calendar_registry,
                    stage_input,
                    Some(&mut progress_forwarder),
                    disk_checkpoint_directory,
                ),
            ))
        }
    }
}

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

    format!(
        "artifact://{}/{}",
        calibration_stage_token(from_stage),
        run_spec.run_id
    )
}

pub(super) fn expected_stage_output_artifact_reference(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> String {
    let artifact_references = stage_output_artifact_references_for_stage(run_spec, stage);

    match artifact_references.as_slice() {
        [artifact_reference] => artifact_reference.clone(),
        _ => format!(
            "artifact://{}/{}",
            calibration_stage_token(stage),
            run_spec.run_id
        ),
    }
}

pub(super) fn calibration_stage_token(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "seasonality",
        CalibrationStage::Hawkes => "hawkes",
        CalibrationStage::ReturnComponents => "return_components",
        CalibrationStage::MeanReversion => "mean_reversion",
        CalibrationStage::JointRefinement => "joint_refinement",
    }
}
