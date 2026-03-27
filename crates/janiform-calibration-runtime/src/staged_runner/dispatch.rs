use super::payload_state::joint_refinement_seasonality_factors_from_payload;
use super::{
    CalibrationExecutionMode, CalibrationInMemoryStagePayloadState, CalibrationRunSpec,
    CalibrationStage, CalibrationStageHandoffTransport, CalibrationStagedRunFailure,
    CalibrationStagedRunInputs, CalibrationStagedRunStageReport, ExchangeCalendarRegistry,
    HawkesStageExecutionInput, HawkesStageSeasonalityInput, JointRefinementStageExecutionInput,
    JointRefinementStageHawkesInput, JointRefinementStageReturnComponentsInput,
    MeanReversionStageReturnComponentsInput, ReturnComponentsStageActivityInput,
    ReturnComponentsStageExecutionInput, run_hawkes_calibration_stage,
    run_joint_refinement_calibration_stage, run_mean_reversion_calibration_stage,
    run_return_components_calibration_stage, run_seasonality_calibration_stage,
    stage_output_artifact_references_for_stage,
};

pub(super) fn run_stage_for_staged_execution(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage: CalibrationStage,
    staged_run_inputs: &mut CalibrationStagedRunInputs,
    payload_state: &CalibrationInMemoryStagePayloadState,
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

            Ok(CalibrationStagedRunStageReport::ReturnComponents(
                run_return_components_calibration_stage(
                    run_spec,
                    calendar_registry,
                    ReturnComponentsStageExecutionInput {
                        activity_input,
                        candidate_generation_input,
                    },
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

            Ok(CalibrationStagedRunStageReport::JointRefinement(
                run_joint_refinement_calibration_stage(
                    run_spec,
                    calendar_registry,
                    JointRefinementStageExecutionInput {
                        seasonality_factors_input,
                        hawkes_input,
                        return_components_input,
                        candidate_generation_input,
                    },
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
