use super::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY,
    CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY, CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION,
    CalibrationOptimizerSettings, CalibrationRunContract, CalibrationStageCheckpointMetadata,
    CalibrationStageParameterArtifactInput, CalibrationStageParameterBuildMetadata,
    CalibrationStageParameterOptimizerStateInput, CalibrationStageSeedSource,
    CalibrationStagedRunFailure, CalibrationStagedRunStageReport,
    calibration_checkpoint_reference_for_stage,
};

pub(super) fn build_stage_parameter_artifact_input(
    stage_report: &CalibrationStagedRunStageReport,
) -> Result<CalibrationStageParameterArtifactInput, String> {
    match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => {
            let payload = report
                .normalized_stage_payload
                .as_ref()
                .ok_or_else(|| "successful seasonality stage is missing its payload".to_string())?;
            let metadata = report
                .metadata
                .as_ref()
                .ok_or_else(|| "successful seasonality stage is missing metadata".to_string())?;

            Ok(CalibrationStageParameterArtifactInput::Seasonality(
                Box::new(
                    janiform_calibration_artifacts::SeasonalityStageParameterArtifactInput {
                        metadata: build_stage_parameter_metadata(
                            metadata.stage_output_schema_version,
                            metadata.run_global_seed,
                            metadata.stage_seed,
                            metadata.stage_seed_source,
                            metadata.stage_seed_derivation_policy,
                            metadata.solver_settings.clone(),
                        ),
                        dataset_id: payload.dataset_id.clone(),
                        exchange_code: payload.exchange_code.clone(),
                        trade_events_data_reference: payload.trade_events_data_reference.clone(),
                        estimator_version: metadata.estimator_version.clone(),
                        timestamp_event_input_schema_version: payload
                            .timestamp_event_input_schema_version
                            .clone(),
                        timestamp_event_preprocessed_schema_version: payload
                            .timestamp_event_preprocessed_schema_version
                            .clone(),
                        intraday_estimate_schema_version: payload
                            .intraday_estimate_schema_version
                            .clone(),
                        day_of_week_estimate_schema_version: payload
                            .day_of_week_estimate_schema_version
                            .clone(),
                        day_of_year_estimate_schema_version: payload
                            .day_of_year_estimate_schema_version
                            .clone(),
                        composition_schema_version: payload.composition_schema_version.clone(),
                        intraday_minutes_per_bin: payload.intraday_minutes_per_bin,
                        intraday_factor_by_bin: payload.intraday_factor_by_bin.clone(),
                        day_of_week_factor: payload.day_of_week_factor.clone(),
                        day_of_year_factor: payload.day_of_year_factor.clone(),
                        normalization_metadata: payload.normalization_metadata.clone(),
                    },
                ),
            ))
        }
        CalibrationStagedRunStageReport::Hawkes(report) => {
            let payload = report
                .fitted_stage_payload
                .as_ref()
                .ok_or_else(|| "successful hawkes stage is missing its payload".to_string())?;
            let metadata = report
                .metadata
                .as_ref()
                .ok_or_else(|| "successful hawkes stage is missing metadata".to_string())?;

            Ok(CalibrationStageParameterArtifactInput::Hawkes(Box::new(
                janiform_calibration_artifacts::HawkesStageParameterArtifactInput {
                    metadata: build_stage_parameter_metadata(
                        metadata.stage_output_schema_version,
                        metadata.run_global_seed,
                        metadata.stage_seed,
                        metadata.stage_seed_source,
                        metadata.stage_seed_derivation_policy,
                        metadata.solver_settings.clone(),
                    ),
                    optimizer_state: build_stage_parameter_optimizer_state_input(
                        StageParameterOptimizerStateInputBuildInput {
                            backend_id: metadata.optimizer_adapter_metadata.backend_id.clone(),
                            random_seed: metadata.optimizer_adapter_metadata.random_seed,
                            selection_policy: metadata.optimizer_adapter_metadata.selection_policy,
                            penalty_policy: metadata.optimizer_adapter_metadata.penalty_policy,
                            bounds_policy: metadata.optimizer_adapter_metadata.bounds_policy,
                            solver_settings: metadata
                                .optimizer_adapter_metadata
                                .solver_settings
                                .clone(),
                            input_candidate_count: metadata.input_candidate_count,
                            valid_candidate_count: metadata.valid_candidate_count,
                            selected_candidate_raw_objective: metadata
                                .selected_candidate_raw_objective,
                            selected_candidate_penalized_objective: metadata
                                .selected_candidate_penalized_objective,
                        },
                    ),
                    selected_candidate_id: payload.selected_candidate_id.clone(),
                    fitted_parameters: payload.fitted_parameters.clone(),
                    seasonality_input_transport: payload.seasonality_input_transport,
                    seasonality_dataset_id: payload.seasonality_dataset_id.clone(),
                    seasonality_artifact_reference: payload.seasonality_artifact_reference.clone(),
                },
            )))
        }
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            let payload = report.fitted_stage_payload.as_ref().ok_or_else(|| {
                "successful return-components stage is missing its payload".to_string()
            })?;
            let metadata = report.metadata.as_ref().ok_or_else(|| {
                "successful return-components stage is missing metadata".to_string()
            })?;

            Ok(CalibrationStageParameterArtifactInput::ReturnComponents(
                Box::new(
                    janiform_calibration_artifacts::ReturnComponentsStageParameterArtifactInput {
                        metadata: build_stage_parameter_metadata(
                            metadata.stage_output_schema_version,
                            metadata.run_global_seed,
                            metadata.stage_seed,
                            metadata.stage_seed_source,
                            metadata.stage_seed_derivation_policy,
                            metadata.solver_settings.clone(),
                        ),
                        optimizer_state: build_stage_parameter_optimizer_state_input(
                            StageParameterOptimizerStateInputBuildInput {
                                backend_id: metadata.optimizer_adapter_metadata.backend_id.clone(),
                                random_seed: metadata.optimizer_adapter_metadata.random_seed,
                                selection_policy: metadata
                                    .optimizer_adapter_metadata
                                    .selection_policy,
                                penalty_policy: metadata.optimizer_adapter_metadata.penalty_policy,
                                bounds_policy: metadata.optimizer_adapter_metadata.bounds_policy,
                                solver_settings: metadata
                                    .optimizer_adapter_metadata
                                    .solver_settings
                                    .clone(),
                                input_candidate_count: metadata.input_candidate_count,
                                valid_candidate_count: metadata.valid_candidate_count,
                                selected_candidate_raw_objective: metadata
                                    .selected_candidate_raw_objective,
                                selected_candidate_penalized_objective: metadata
                                    .selected_candidate_penalized_objective,
                            },
                        ),
                        selected_candidate_id: payload.selected_candidate_id.clone(),
                        fitted_parameters: payload.fitted_parameters.clone(),
                        candidate_generation_policy: payload
                            .candidate_generation_policy
                            .to_string(),
                        objective_weighting_policy: payload.objective_weighting_policy.to_string(),
                        objective_metric_bucket_policy: payload
                            .objective_metric_bucket_policy
                            .to_string(),
                        activity_input_transport: payload.activity_input_transport,
                        activity_selected_hawkes_candidate_id: payload
                            .activity_selected_hawkes_candidate_id
                            .clone(),
                        activity_artifact_reference: payload.activity_artifact_reference.clone(),
                    },
                ),
            ))
        }
        CalibrationStagedRunStageReport::MeanReversion(report) => {
            let payload = report.fitted_stage_payload.as_ref().ok_or_else(|| {
                "successful mean-reversion stage is missing its payload".to_string()
            })?;

            let metadata = report
                .metadata
                .as_ref()
                .ok_or_else(|| "successful mean-reversion stage is missing metadata".to_string())?;

            Ok(CalibrationStageParameterArtifactInput::MeanReversion(
                Box::new(
                    janiform_calibration_artifacts::MeanReversionStageParameterArtifactInput {
                        metadata: build_stage_parameter_metadata(
                            metadata.stage_output_schema_version,
                            metadata.run_global_seed,
                            metadata.stage_seed,
                            metadata.stage_seed_source,
                            metadata.stage_seed_derivation_policy,
                            metadata.solver_settings.clone(),
                        ),
                        fitted_parameters: payload.fitted_parameters.clone(),
                        diagnostics: payload.diagnostics.clone(),
                        return_components_input_transport: payload
                            .return_components_input_transport,
                        return_components_selected_candidate_id: payload
                            .return_components_selected_candidate_id
                            .clone(),
                        return_components_artifact_reference: payload
                            .return_components_artifact_reference
                            .clone(),
                    },
                ),
            ))
        }
        CalibrationStagedRunStageReport::JointRefinement(report) => {
            let payload = report.fitted_stage_payload.as_ref().ok_or_else(|| {
                "successful joint-refinement stage is missing its payload".to_string()
            })?;
            let metadata = report.metadata.as_ref().ok_or_else(|| {
                "successful joint-refinement stage is missing metadata".to_string()
            })?;

            Ok(CalibrationStageParameterArtifactInput::JointRefinement(
                Box::new(
                    janiform_calibration_artifacts::JointRefinementStageParameterArtifactInput {
                        metadata: build_stage_parameter_metadata(
                            metadata.stage_output_schema_version,
                            metadata.run_global_seed,
                            metadata.stage_seed,
                            metadata.stage_seed_source,
                            metadata.stage_seed_derivation_policy,
                            metadata.solver_settings.clone(),
                        ),
                        optimizer_state: build_stage_parameter_optimizer_state_input(
                            StageParameterOptimizerStateInputBuildInput {
                                backend_id: metadata.optimizer_adapter_metadata.backend_id.clone(),
                                random_seed: metadata.optimizer_adapter_metadata.random_seed,
                                selection_policy: metadata
                                    .optimizer_adapter_metadata
                                    .selection_policy,
                                penalty_policy: metadata.optimizer_adapter_metadata.penalty_policy,
                                bounds_policy: metadata.optimizer_adapter_metadata.bounds_policy,
                                solver_settings: metadata
                                    .optimizer_adapter_metadata
                                    .solver_settings
                                    .clone(),
                                input_candidate_count: metadata.input_candidate_count,
                                valid_candidate_count: metadata.valid_candidate_count,
                                selected_candidate_raw_objective: metadata
                                    .selected_candidate_raw_objective,
                                selected_candidate_penalized_objective: metadata
                                    .selected_candidate_penalized_objective,
                            },
                        ),
                        selected_candidate_id: payload.selected_candidate_id.clone(),
                        fitted_parameters: payload.fitted_parameters.clone(),
                        baseline_hawkes_fitted_parameters: payload
                            .baseline_hawkes_fitted_parameters
                            .clone(),
                        baseline_return_components_fitted_parameters: payload
                            .baseline_return_components_fitted_parameters
                            .clone(),
                        objective_breakdown: payload.objective_breakdown.clone(),
                        optimizer_diagnostics: payload.optimizer_diagnostics.clone(),
                    },
                ),
            ))
        }
    }
}

pub(super) fn build_stage_parameter_metadata(
    stage_output_schema_version: &str,
    run_global_seed: u64,
    stage_seed: u64,
    stage_seed_source: CalibrationStageSeedSource,
    stage_seed_derivation_policy: &str,
    solver_settings: CalibrationOptimizerSettings,
) -> CalibrationStageParameterBuildMetadata {
    CalibrationStageParameterBuildMetadata {
        stage_output_schema_version: stage_output_schema_version.to_string(),
        run_global_seed,
        stage_seed,
        stage_seed_source,
        stage_seed_derivation_policy: stage_seed_derivation_policy.to_string(),
        solver_settings,
    }
}

pub(super) fn build_stage_parameter_optimizer_state_input(
    input: StageParameterOptimizerStateInputBuildInput,
) -> CalibrationStageParameterOptimizerStateInput {
    let StageParameterOptimizerStateInputBuildInput {
        backend_id,
        random_seed,
        selection_policy,
        penalty_policy,
        bounds_policy,
        solver_settings,
        input_candidate_count,
        valid_candidate_count,
        selected_candidate_raw_objective,
        selected_candidate_penalized_objective,
    } = input;

    CalibrationStageParameterOptimizerStateInput {
        backend_id,
        random_seed,
        selection_policy: selection_policy.to_string(),
        penalty_policy: penalty_policy.to_string(),
        bounds_policy: bounds_policy.to_string(),
        solver_settings,
        input_candidate_count,
        valid_candidate_count,
        selected_candidate_raw_objective,
        selected_candidate_penalized_objective,
    }
}

pub(super) struct StageParameterOptimizerStateInputBuildInput {
    pub(super) backend_id: String,
    pub(super) random_seed: u64,
    pub(super) selection_policy: &'static str,
    pub(super) penalty_policy: &'static str,
    pub(super) bounds_policy: &'static str,
    pub(super) solver_settings: CalibrationOptimizerSettings,
    pub(super) input_candidate_count: usize,
    pub(super) valid_candidate_count: usize,
    pub(super) selected_candidate_raw_objective: f64,
    pub(super) selected_candidate_penalized_objective: f64,
}

pub(super) fn build_stage_checkpoint_metadata(
    run_contract: &CalibrationRunContract,
    stage_report: &CalibrationStagedRunStageReport,
    stage_position: usize,
) -> Result<CalibrationStageCheckpointMetadata, CalibrationStagedRunFailure> {
    let stage = stage_report.stage();
    let Some(checkpoint_components) = stage_report.checkpoint_components() else {
        return Err(
            CalibrationStagedRunFailure::MissingCheckpointMetadataForSuccessfulStage { stage },
        );
    };

    Ok(CalibrationStageCheckpointMetadata {
        schema_version: CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION.to_string(),
        checkpoint_reference_policy: CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY.to_string(),
        disk_write_policy: CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY.to_string(),
        artifact_manifest_schema_version: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
        checkpoint_reference: calibration_checkpoint_reference_for_stage(
            run_contract.run_spec.run_id.as_str(),
            stage,
        ),
        run_id: run_contract.run_spec.run_id.clone(),
        execution_mode: run_contract.run_spec.execution_mode,
        run_contract_schema_version: run_contract.schema_version.clone(),
        run_spec_schema_version: run_contract.run_spec_schema_version.clone(),
        stage_graph_schema_version: run_contract.stage_graph_schema_version.clone(),
        execution_stage_order: run_contract.execution_stage_order.clone(),
        completed_stage: stage,
        completed_stage_position: stage_position,
        run_global_seed: checkpoint_components.run_global_seed,
        completed_stage_output_schema_version: checkpoint_components.stage_output_schema_version,
        completed_stage_seed: checkpoint_components.stage_seed,
        completed_stage_seed_source: checkpoint_components.stage_seed_source,
        stage_seed_derivation_policy: checkpoint_components
            .stage_seed_derivation_policy
            .to_string(),
        completed_stage_solver_settings: checkpoint_components.stage_solver_settings,
    })
}
