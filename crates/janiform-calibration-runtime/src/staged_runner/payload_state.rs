use super::{
    CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_LONG_VARIANCE_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_NEWS_SHOCKS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_OBSERVATION_MODELS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
    CalibrationInMemoryStagePayloadState, CalibrationStageParameterArtifact,
    CalibrationStageParameterPayload, CalibrationStagedRunStageReport,
    JointRefinementStageSeasonalityFactorsInput,
};
use crate::stages::return_components::{
    RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY,
};
use janiform_calibration_artifacts::CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY;

pub(super) fn joint_refinement_seasonality_factors_from_payload(
    payload: &crate::stages::seasonality::SeasonalityStageNormalizedPayload,
) -> JointRefinementStageSeasonalityFactorsInput {
    JointRefinementStageSeasonalityFactorsInput {
        exchange_code: payload.exchange_code.clone(),
        clock_model: payload.clock_model.clone(),
    }
}

pub(super) fn restore_in_memory_payload_state_from_stage_parameter_artifact(
    payload_state: &mut CalibrationInMemoryStagePayloadState,
    artifact: &CalibrationStageParameterArtifact,
) {
    match &artifact.stage_parameters {
        CalibrationStageParameterPayload::Seasonality(payload) => {
            payload_state.mean_reversion_seasonality_input = Some(
                crate::stages::mean_reversion::MeanReversionStageSeasonalityInput {
                    exchange_code: payload.exchange_code.clone(),
                    clock_model: payload.clock_model.clone(),
                },
            );
            payload_state.joint_refinement_seasonality_factors_input =
                Some(JointRefinementStageSeasonalityFactorsInput {
                    exchange_code: payload.exchange_code.clone(),
                    clock_model: payload.clock_model.clone(),
                });
        }
        CalibrationStageParameterPayload::Hawkes(payload) => {
            payload_state.hawkes_stage_payload =
                Some(crate::stages::hawkes::HawkesStageFittedPayload {
                    schema_version: CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
                    selected_candidate_id: payload.selected_candidate_id.clone(),
                    fitted_parameters: payload.fitted_parameters.clone(),
                    candidate_diagnostics: Vec::new(),
                    seasonality_input_transport: payload.seasonality_input_transport,
                    seasonality_dataset_id: payload.seasonality_dataset_id.clone(),
                    seasonality_artifact_reference: payload.seasonality_artifact_reference.clone(),
                });
        }
        CalibrationStageParameterPayload::ObservationModels(payload) => {
            payload_state.observation_models_stage_payload = Some(
                crate::stages::observation_models::ObservationModelsStageFittedPayload {
                    schema_version: CALIBRATION_OBSERVATION_MODELS_STAGE_OUTPUT_SCHEMA_VERSION,
                    timestamp_batching_observation_model: payload
                        .timestamp_batching_observation_model
                        .clone(),
                    open_session_return_source_activities: Vec::new(),
                    diagnostics: payload.diagnostics.clone(),
                },
            );
        }
        CalibrationStageParameterPayload::ReturnComponents(payload) => {
            payload_state.return_components_stage_payload = Some(
                crate::stages::return_components::ReturnComponentsStageFittedPayload {
                    schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
                    selected_candidate_id: payload.selected_candidate_id.clone(),
                    fitted_parameters: payload.fitted_parameters.clone(),
                    candidate_diagnostics: Vec::new(),
                    candidate_generation_policy: RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY,
                    objective_weighting_policy:
                        CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
                    objective_metric_bucket_policy:
                        RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY,
                    candidate_family_counts: crate::stages::return_components::ReturnComponentsStageCandidateFamilyCounts {
                        nig_candidate_count: 1,
                        known_news_arrival_family_count: 1,
                        surprise_news_arrival_family_count: 1,
                        return_jump_mark_scale_family_count: 1,
                        activity_coupling_family_count: 1,
                    },
                    activity_input_transport: payload.activity_input_transport,
                    activity_selected_hawkes_candidate_id: payload
                        .activity_selected_hawkes_candidate_id
                        .clone(),
                    activity_artifact_reference: payload.activity_artifact_reference.clone(),
                },
            );
        }
        CalibrationStageParameterPayload::LongVariance(payload) => {
            payload_state.long_variance_stage_payload = Some(
                crate::stages::long_variance::LongVarianceStageFittedPayload {
                    schema_version: CALIBRATION_LONG_VARIANCE_STAGE_OUTPUT_SCHEMA_VERSION,
                    fitted_parameters: payload.fitted_parameters,
                    diagnostics: payload.diagnostics.clone(),
                    return_components_input_transport: payload.return_components_input_transport,
                    return_components_selected_candidate_id: payload
                        .return_components_selected_candidate_id
                        .clone(),
                    return_components_artifact_reference: payload
                        .return_components_artifact_reference
                        .clone(),
                },
            );
            apply_long_variance_to_return_components_state(payload_state);
        }
        CalibrationStageParameterPayload::NewsShocks(payload) => {
            payload_state.news_shocks_stage_payload =
                Some(crate::stages::news_shocks::NewsShocksStageFittedPayload {
                    schema_version: CALIBRATION_NEWS_SHOCKS_STAGE_OUTPUT_SCHEMA_VERSION,
                    news_process_configuration: payload.news_process_configuration.clone(),
                    surprise_deterministic_intensity_profile: payload
                        .surprise_deterministic_intensity_profile
                        .clone(),
                    diagnostics: payload.diagnostics.clone(),
                    return_components_input_transport: payload.return_components_input_transport,
                    return_components_selected_candidate_id: payload
                        .return_components_selected_candidate_id
                        .clone(),
                    return_components_artifact_reference: payload
                        .return_components_artifact_reference
                        .clone(),
                });
            apply_news_shocks_to_return_components_state(payload_state);
        }
        CalibrationStageParameterPayload::ReturnBackfit(payload) => {
            let effective_return_components_payload =
                crate::stages::return_components::ReturnComponentsStageFittedPayload {
                    schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
                    selected_candidate_id: payload
                        .effective_return_components_selected_candidate_id
                        .clone(),
                    fitted_parameters: payload.effective_fitted_parameters.clone(),
                    candidate_diagnostics: Vec::new(),
                    candidate_generation_policy: RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY,
                    objective_weighting_policy:
                        CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
                    objective_metric_bucket_policy:
                        RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY,
                    candidate_family_counts:
                        crate::stages::return_components::ReturnComponentsStageCandidateFamilyCounts {
                            nig_candidate_count: 1,
                            known_news_arrival_family_count: 1,
                            surprise_news_arrival_family_count: 1,
                            return_jump_mark_scale_family_count: 1,
                            activity_coupling_family_count: 1,
                        },
                    activity_input_transport:
                        janiform_calibration_artifacts::CalibrationStageHandoffTransport::InMemoryOutput,
                    activity_selected_hawkes_candidate_id: None,
                    activity_artifact_reference: None,
                };

            payload_state.return_backfit_stage_payload = Some(
                crate::stages::return_backfit::ReturnBackfitStageFittedPayload {
                    schema_version: CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION,
                    selected_candidate_id: payload.selected_candidate_id.clone(),
                    effective_return_components_payload: effective_return_components_payload
                        .clone(),
                    diagnostics: payload.diagnostics.clone(),
                    return_components_input_transport: payload.return_components_input_transport,
                    return_components_selected_candidate_id: payload
                        .return_components_selected_candidate_id
                        .clone(),
                    return_components_artifact_reference: payload
                        .return_components_artifact_reference
                        .clone(),
                },
            );

            payload_state.return_components_stage_payload =
                Some(effective_return_components_payload);
        }
        CalibrationStageParameterPayload::MeanReversion(payload) => {
            payload_state.mean_reversion_stage_payload = Some(
                crate::stages::mean_reversion::MeanReversionStageFittedPayload {
                    schema_version: CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
                    fitted_parameters: payload.fitted_parameters.clone(),
                    diagnostics: payload.diagnostics.clone(),
                    return_components_input_transport: payload.return_components_input_transport,
                    return_components_selected_candidate_id: payload
                        .return_components_selected_candidate_id
                        .clone(),
                    return_components_artifact_reference: payload
                        .return_components_artifact_reference
                        .clone(),
                },
            );
        }
        CalibrationStageParameterPayload::JointRefinement(payload) => {
            payload_state.joint_refinement_stage_payload =
                Some(joint_refinement_payload_from_parameter_payload(payload));
            payload_state.joint_refinement_runtime_handoff = None;
        }
        CalibrationStageParameterPayload::OptionRefinement(payload) => {
            payload_state.option_refinement_stage_payload =
                Some(joint_refinement_payload_from_parameter_payload(payload));
            payload_state.option_refinement_runtime_handoff = None;
        }
        CalibrationStageParameterPayload::Finalization(payload) => {
            payload_state.finalization_stage_payload =
                Some(joint_refinement_payload_from_parameter_payload(payload));
        }
    }
}

pub(super) fn update_in_memory_payload_state(
    payload_state: &mut CalibrationInMemoryStagePayloadState,
    stage_report: &CalibrationStagedRunStageReport,
) {
    match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => {
            payload_state.seasonality_stage_payload = report.normalized_stage_payload.clone();
            payload_state.mean_reversion_seasonality_input = report
                .normalized_stage_payload
                .as_ref()
                .map(
                    crate::stages::mean_reversion::MeanReversionStageSeasonalityInput::from_seasonality_stage_payload,
                );
            payload_state.joint_refinement_seasonality_factors_input = report
                .normalized_stage_payload
                .as_ref()
                .map(joint_refinement_seasonality_factors_from_payload);
        }
        CalibrationStagedRunStageReport::Hawkes(report) => {
            payload_state.hawkes_stage_payload = report.fitted_stage_payload.clone();
        }
        CalibrationStagedRunStageReport::ObservationModels(report) => {
            payload_state.observation_models_stage_payload = report.fitted_stage_payload.clone();
        }
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            payload_state.return_components_stage_payload = report.fitted_stage_payload.clone();
        }
        CalibrationStagedRunStageReport::LongVariance(report) => {
            payload_state.long_variance_stage_payload = report.fitted_stage_payload.clone();
            apply_long_variance_to_return_components_state(payload_state);
        }
        CalibrationStagedRunStageReport::NewsShocks(report) => {
            payload_state.news_shocks_stage_payload = report.fitted_stage_payload.clone();
            apply_news_shocks_to_return_components_state(payload_state);
        }
        CalibrationStagedRunStageReport::ReturnBackfit(report) => {
            payload_state.return_backfit_stage_payload = report.fitted_stage_payload.clone();
            payload_state.return_components_stage_payload = report
                .fitted_stage_payload
                .as_ref()
                .map(|payload| payload.effective_return_components_payload.clone());
        }
        CalibrationStagedRunStageReport::MeanReversion(report) => {
            payload_state.mean_reversion_stage_payload = report.fitted_stage_payload.clone();
        }
        CalibrationStagedRunStageReport::JointRefinement(report) => {
            payload_state.joint_refinement_stage_payload = report.fitted_stage_payload.clone();
            payload_state.joint_refinement_runtime_handoff = report.runtime_handoff.clone();
        }
        CalibrationStagedRunStageReport::OptionRefinement(report) => {
            payload_state.option_refinement_stage_payload = report.fitted_stage_payload.clone();
            payload_state.option_refinement_runtime_handoff = report.runtime_handoff.clone();
        }
        CalibrationStagedRunStageReport::Finalization(report) => {
            payload_state.finalization_stage_payload = report.fitted_stage_payload.clone();
        }
    }
}

fn apply_long_variance_to_return_components_state(
    payload_state: &mut CalibrationInMemoryStagePayloadState,
) {
    let Some(long_variance_payload) = payload_state.long_variance_stage_payload.as_ref() else {
        return;
    };

    let Some(return_components_payload) = payload_state.return_components_stage_payload.as_mut()
    else {
        return;
    };

    return_components_payload
        .fitted_parameters
        .long_variance_parameters = long_variance_payload.fitted_parameters;
}

fn apply_news_shocks_to_return_components_state(
    payload_state: &mut CalibrationInMemoryStagePayloadState,
) {
    let Some(news_shocks_payload) = payload_state.news_shocks_stage_payload.as_ref() else {
        return;
    };

    let Some(return_components_payload) = payload_state.return_components_stage_payload.as_mut()
    else {
        return;
    };

    return_components_payload
        .fitted_parameters
        .news_process_configuration = news_shocks_payload.news_process_configuration.clone();

    return_components_payload
        .fitted_parameters
        .surprise_deterministic_intensity_profile = news_shocks_payload
        .surprise_deterministic_intensity_profile
        .clone();
}

fn joint_refinement_payload_from_parameter_payload(
    payload: &janiform_calibration_artifacts::JointRefinementStageParameterPayload,
) -> crate::stages::joint_refinement::JointRefinementStageFittedPayload {
    crate::stages::joint_refinement::JointRefinementStageFittedPayload {
        schema_version:
            janiform_calibration_artifacts::CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: payload.selected_candidate_id.clone(),
        fitted_parameters: payload.fitted_parameters.clone(),
        baseline_hawkes_fitted_parameters: payload.baseline_hawkes_fitted_parameters.clone(),
        baseline_return_components_fitted_parameters: payload
            .baseline_return_components_fitted_parameters
            .clone(),
        objective_breakdown: payload.objective_breakdown.clone(),
        optimizer_diagnostics: payload.optimizer_diagnostics.clone(),
        candidate_frontier: Vec::new(),
        simulation_summaries: Vec::new(),
        coarse_tick_consistency: Vec::new(),
        option_calibration_diagnostics: None,
    }
}
