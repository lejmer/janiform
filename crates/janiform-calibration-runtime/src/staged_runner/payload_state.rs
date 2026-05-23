use super::{
    CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
    CalibrationInMemoryStagePayloadState, CalibrationStageParameterArtifact,
    CalibrationStageParameterPayload, CalibrationStagedRunStageReport,
    JointRefinementStageSeasonalityFactorsInput,
};

pub(super) fn joint_refinement_seasonality_factors_from_payload(
    payload: &crate::stages::seasonality::SeasonalityStageNormalizedPayload,
) -> JointRefinementStageSeasonalityFactorsInput {
    JointRefinementStageSeasonalityFactorsInput {
        intraday_minutes_per_bin: payload.intraday_minutes_per_bin,
        intraday_factor_by_bin: payload.intraday_factor_by_bin.clone(),
        day_of_week_factor: payload.day_of_week_factor.clone(),
        hour_of_week_factor: payload.hour_of_week_factor.clone(),
        day_of_year_factor: payload.day_of_year_factor.clone(),
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
                    intraday_minutes_per_bin: payload.intraday_minutes_per_bin,
                    intraday_factor_by_bin: payload.intraday_factor_by_bin.clone(),
                    day_of_week_factor: payload.day_of_week_factor.clone(),
                    hour_of_week_factor: payload.hour_of_week_factor.clone(),
                    day_of_year_factor: payload.day_of_year_factor.clone(),
                },
            );
            payload_state.joint_refinement_seasonality_factors_input =
                Some(JointRefinementStageSeasonalityFactorsInput {
                    intraday_minutes_per_bin: payload.intraday_minutes_per_bin,
                    intraday_factor_by_bin: payload.intraday_factor_by_bin.clone(),
                    day_of_week_factor: payload.day_of_week_factor.clone(),
                    hour_of_week_factor: payload.hour_of_week_factor.clone(),
                    day_of_year_factor: payload.day_of_year_factor.clone(),
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
        CalibrationStageParameterPayload::ReturnComponents(payload) => {
            payload_state.return_components_stage_payload = Some(
                crate::stages::return_components::ReturnComponentsStageFittedPayload {
                    schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
                    selected_candidate_id: payload.selected_candidate_id.clone(),
                    fitted_parameters: payload.fitted_parameters.clone(),
                    candidate_diagnostics: Vec::new(),
                    candidate_generation_policy: Box::leak(
                        payload.candidate_generation_policy.clone().into_boxed_str(),
                    ),
                    objective_weighting_policy: Box::leak(
                        payload.objective_weighting_policy.clone().into_boxed_str(),
                    ),
                    objective_metric_bucket_policy: Box::leak(
                        payload.objective_metric_bucket_policy.clone().into_boxed_str(),
                    ),
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
            payload_state.seasonality_stage_payload = None;
        }
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            payload_state.return_components_stage_payload = report.fitted_stage_payload.clone();
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
