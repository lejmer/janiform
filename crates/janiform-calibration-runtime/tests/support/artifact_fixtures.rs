#![allow(dead_code)]

use janiform_calibration_artifacts::{
    CalibrationLossAdmissibility, CalibrationObjectivePolicy, CalibrationRunContract,
    CalibrationStage, CalibrationStageHandoffTransport, CalibrationStageParameterArtifact,
    CalibrationStageParameterArtifactInput, CalibrationStageParameterBuildMetadata,
    CalibrationStageParameterOptimizerStateInput, HawkesStageFittedParameters,
    HawkesStageParameterArtifactInput, JointRefinementStageFittedParameters,
    JointRefinementStageObjectiveBreakdown, JointRefinementStageOptimizerDiagnostics,
    JointRefinementStageParameterArtifactInput, JointRefinementStageRestartDiagnostics,
    JointRefinementStageRuntimeProfile, MeanReversionStageCalibrationGrid,
    MeanReversionStageDiagnostics, MeanReversionStageParameterArtifactInput,
    ReturnComponentsStageFittedParameters, ReturnComponentsStageParameterArtifactInput,
    ReturnMarkHourOfWeekScaleProfile, SeasonalityStageParameterArtifactInput,
    SurpriseNewsDeterministicIntensityProfile, TickGridObservationModel,
    build_calibration_stage_parameter_artifact,
};
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;
use janiform_model_kernels::news::KnownNewsArrivalConfiguration;

use super::shared::{
    CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION, CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, HawkesKernelLoadings, HawkesKernelParameters,
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
    NewsArrivalConfiguration, NewsComponentMarkScaleConfiguration, NewsProcessConfiguration,
    NigSharedPlusShiftConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration, build_seasonality_stage_success_payload,
    resolve_calibration_stage_seed_metadata, stage_solver_settings_for_stage,
};

pub(crate) fn build_stage_parameter_artifact_fixture_set(
    run_contract: &CalibrationRunContract,
) -> Vec<CalibrationStageParameterArtifact> {
    let hawkes_fitted_parameters = regression_hawkes_fitted_parameters();
    let return_components_fitted_parameters = regression_return_components_fitted_parameters();
    let mean_reversion_fitted_parameters =
        MeanReversionFittedParameters::zero_kappa_flat_anchor_mean_reversion();

    vec![
        build_regression_stage_parameter_artifact(
            run_contract,
            CalibrationStageParameterArtifactInput::Seasonality(Box::new(
                regression_seasonality_stage_parameter_input(run_contract),
            )),
        ),
        build_regression_stage_parameter_artifact(
            run_contract,
            CalibrationStageParameterArtifactInput::Hawkes(Box::new(
                regression_hawkes_stage_parameter_input(
                    run_contract,
                    hawkes_fitted_parameters.clone(),
                ),
            )),
        ),
        build_regression_stage_parameter_artifact(
            run_contract,
            CalibrationStageParameterArtifactInput::ReturnComponents(Box::new(
                regression_return_components_stage_parameter_input(
                    run_contract,
                    return_components_fitted_parameters.clone(),
                ),
            )),
        ),
        build_regression_stage_parameter_artifact(
            run_contract,
            CalibrationStageParameterArtifactInput::MeanReversion(Box::new(
                regression_mean_reversion_stage_parameter_input(
                    run_contract,
                    mean_reversion_fitted_parameters.clone(),
                ),
            )),
        ),
        build_regression_stage_parameter_artifact(
            run_contract,
            CalibrationStageParameterArtifactInput::JointRefinement(Box::new(
                regression_joint_refinement_stage_parameter_input(
                    run_contract,
                    hawkes_fitted_parameters,
                    return_components_fitted_parameters,
                ),
            )),
        ),
    ]
}

fn build_regression_stage_parameter_artifact(
    run_contract: &CalibrationRunContract,
    stage_input: CalibrationStageParameterArtifactInput,
) -> CalibrationStageParameterArtifact {
    build_calibration_stage_parameter_artifact(run_contract, stage_input)
        .expect("regression stage artifact should build")
}

fn regression_seasonality_stage_parameter_input(
    run_contract: &CalibrationRunContract,
) -> SeasonalityStageParameterArtifactInput {
    let payload = build_seasonality_stage_success_payload();

    SeasonalityStageParameterArtifactInput {
        metadata: regression_stage_build_metadata(
            run_contract,
            CalibrationStage::Seasonality,
            CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION,
        ),
        dataset_id: payload.dataset_id,
        exchange_code: payload.exchange_code,
        trade_events_data_reference: payload.trade_events_data_reference,
        estimator_version: "artifact_regression_fixture.v1".to_string(),
        timestamp_event_input_schema_version: payload.timestamp_event_input_schema_version,
        timestamp_event_preprocessed_schema_version: payload
            .timestamp_event_preprocessed_schema_version,
        intraday_estimate_schema_version: payload.intraday_estimate_schema_version,
        day_of_week_estimate_schema_version: payload.day_of_week_estimate_schema_version,
        day_of_year_estimate_schema_version: payload.day_of_year_estimate_schema_version,
        composition_schema_version: payload.composition_schema_version,
        intraday_minutes_per_bin: payload.intraday_minutes_per_bin,
        intraday_factor_by_bin: payload.intraday_factor_by_bin,
        day_of_week_factor: payload.day_of_week_factor,
        hour_of_week_factor: payload.hour_of_week_factor,
        day_of_year_factor: payload.day_of_year_factor,
        normalization_metadata: payload.normalization_metadata,
    }
}

fn regression_hawkes_stage_parameter_input(
    run_contract: &CalibrationRunContract,
    fitted_parameters: HawkesStageFittedParameters,
) -> HawkesStageParameterArtifactInput {
    HawkesStageParameterArtifactInput {
        metadata: regression_stage_build_metadata(
            run_contract,
            CalibrationStage::Hawkes,
            CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
        ),
        optimizer_state: regression_optimizer_state(run_contract, CalibrationStage::Hawkes),
        selected_candidate_id: "artifact_regression_hawkes".to_string(),
        fitted_parameters,
        seasonality_input_transport: CalibrationStageHandoffTransport::PersistedArtifactReference,
        seasonality_dataset_id: None,
        seasonality_artifact_reference: Some(format!(
            "artifact://seasonality/{}",
            run_contract.run_spec.run_id
        )),
    }
}

fn regression_return_components_stage_parameter_input(
    run_contract: &CalibrationRunContract,
    fitted_parameters: ReturnComponentsStageFittedParameters,
) -> ReturnComponentsStageParameterArtifactInput {
    ReturnComponentsStageParameterArtifactInput {
        metadata: regression_stage_build_metadata(
            run_contract,
            CalibrationStage::ReturnComponents,
            CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
        ),
        optimizer_state: regression_optimizer_state(
            run_contract,
            CalibrationStage::ReturnComponents,
        ),
        selected_candidate_id: "artifact_regression_return_components".to_string(),
        fitted_parameters,
        candidate_generation_policy: "artifact_regression_fixture.v1".to_string(),
        objective_weighting_policy: "artifact_regression_fixture.v1".to_string(),
        objective_metric_bucket_policy: "artifact_regression_fixture.v1".to_string(),
        activity_input_transport: CalibrationStageHandoffTransport::PersistedArtifactReference,
        activity_selected_hawkes_candidate_id: None,
        activity_artifact_reference: Some(format!(
            "artifact://hawkes/{}",
            run_contract.run_spec.run_id
        )),
    }
}

fn regression_mean_reversion_stage_parameter_input(
    run_contract: &CalibrationRunContract,
    fitted_parameters: MeanReversionFittedParameters,
) -> MeanReversionStageParameterArtifactInput {
    MeanReversionStageParameterArtifactInput {
        metadata: regression_stage_build_metadata(
            run_contract,
            CalibrationStage::MeanReversion,
            CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
        ),
        diagnostics: regression_mean_reversion_diagnostics(&fitted_parameters),
        fitted_parameters,
        return_components_input_transport:
            CalibrationStageHandoffTransport::PersistedArtifactReference,
        return_components_selected_candidate_id: None,
        return_components_artifact_reference: Some(format!(
            "artifact://return_components/{}",
            run_contract.run_spec.run_id
        )),
    }
}

fn regression_joint_refinement_stage_parameter_input(
    run_contract: &CalibrationRunContract,
    hawkes_fitted_parameters: HawkesStageFittedParameters,
    return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
) -> JointRefinementStageParameterArtifactInput {
    JointRefinementStageParameterArtifactInput {
        metadata: regression_stage_build_metadata(
            run_contract,
            CalibrationStage::JointRefinement,
            CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
        ),
        optimizer_state: regression_optimizer_state(
            run_contract,
            CalibrationStage::JointRefinement,
        ),
        selected_candidate_id: "artifact_regression_joint_refinement".to_string(),
        fitted_parameters: JointRefinementStageFittedParameters {
            hawkes_fitted_parameters: hawkes_fitted_parameters.clone(),
            return_components_fitted_parameters: return_components_fitted_parameters.clone(),
            pricing_block_fitted_parameters: None,
        },
        baseline_hawkes_fitted_parameters: hawkes_fitted_parameters,
        baseline_return_components_fitted_parameters: return_components_fitted_parameters,
        objective_breakdown: regression_joint_refinement_objective_breakdown(),
        optimizer_diagnostics: regression_joint_refinement_optimizer_diagnostics(),
    }
}

fn regression_stage_build_metadata(
    run_contract: &CalibrationRunContract,
    stage: CalibrationStage,
    stage_output_schema_version: &str,
) -> CalibrationStageParameterBuildMetadata {
    let resolved_seed = resolve_calibration_stage_seed_metadata(&run_contract.run_spec, stage);
    let solver_settings = stage_solver_settings_for_stage(&run_contract.run_spec, stage)
        .expect("regression run contract should include solver settings for every stage")
        .clone();

    CalibrationStageParameterBuildMetadata {
        stage_output_schema_version: stage_output_schema_version.to_string(),
        run_global_seed: resolved_seed.run_global_seed,
        stage_seed: resolved_seed.stage_seed,
        stage_seed_source: resolved_seed.stage_seed_source,
        stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY.to_string(),
        solver_settings,
    }
}

fn regression_optimizer_state(
    run_contract: &CalibrationRunContract,
    stage: CalibrationStage,
) -> CalibrationStageParameterOptimizerStateInput {
    let solver_settings = stage_solver_settings_for_stage(&run_contract.run_spec, stage)
        .expect("regression run contract should include solver settings for every stage")
        .clone();

    CalibrationStageParameterOptimizerStateInput {
        backend_id: "artifact_regression_fixture".to_string(),
        random_seed: resolve_calibration_stage_seed_metadata(&run_contract.run_spec, stage)
            .stage_seed,
        selection_policy: "artifact_regression_fixture.v1".to_string(),
        penalty_policy: "artifact_regression_fixture.v1".to_string(),
        bounds_policy: "artifact_regression_fixture.v1".to_string(),
        solver_settings,
        input_candidate_count: 1,
        valid_candidate_count: 1,
        selected_candidate_raw_objective: 0.0,
        selected_candidate_penalized_objective: 0.0,
    }
}

fn regression_hawkes_fitted_parameters() -> HawkesStageFittedParameters {
    HawkesStageFittedParameters {
        kernel_parameters: HawkesKernelParameters::try_new(
            12.0,
            1.0,
            HawkesKernelLoadings {
                plus_plus: 0.05,
                plus_minus: 0.02,
                minus_plus: 0.02,
                minus_minus: 0.05,
            },
            HawkesKernelLoadings {
                plus_plus: 0.02,
                plus_minus: 0.01,
                minus_plus: 0.01,
                minus_minus: 0.02,
            },
        )
        .expect("regression Hawkes kernel parameters should validate"),
        structured_alpha: HawkesStructuredAlphaParameterization::new(0.20, 0.10, 0.0, 0.0),
        structured_rho: HawkesStructuredRhoParameterization::new(0.05, 0.02, 0.0, 0.0),
        self_dominance_policy: None,
        stability_numeric_tolerance: 0.0,
    }
}

fn regression_return_components_fitted_parameters() -> ReturnComponentsStageFittedParameters {
    ReturnComponentsStageFittedParameters {
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration::try_new(
            2.0, -0.2, 0.5, 0.0, -0.1,
        )
        .expect("regression NIG configuration should validate"),
        news_process_configuration: regression_news_process_configuration(),
        surprise_deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile {
            weekly_hour_utc_multipliers: vec![1.0; 24 * 7],
            profile_resolution_hours: 1,
            fit_policy: "artifact_regression_fixture.v1".to_string(),
            normalization_policy: "artifact_regression_fixture.v1".to_string(),
        },
        return_mark_hour_of_week_scale_profile: ReturnMarkHourOfWeekScaleProfile {
            hour_of_week_factors: vec![1.0; 24 * 7],
            profile_resolution_hours: 1,
            fit_policy: "artifact_regression_fixture.v1".to_string(),
            normalization_policy: "artifact_regression_fixture.v1".to_string(),
        },
        tick_grid_observation_model: TickGridObservationModel {
            tick_size: None,
            tick_size_source: "not_applicable_for_artifact_regression_fixture".to_string(),
            inference_policy: "artifact_regression_fixture.v1".to_string(),
            observation_policy: "artifact_regression_fixture.v1".to_string(),
        },
        mean_reversion_fitted_parameters:
            MeanReversionFittedParameters::zero_kappa_flat_anchor_mean_reversion(),
    }
}

fn regression_news_process_configuration() -> NewsProcessConfiguration {
    NewsProcessConfiguration::try_new(
        NewsArrivalConfiguration {
            known: KnownNewsArrivalConfiguration {
                scheduled_event_timestamps_utc_nanos: Vec::new(),
            },
            surprise: SurpriseNewsArrivalConfiguration {
                baseline_intensity: 0.0,
            },
        },
        SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration {
                shape_k: 1.5,
                scale_theta: 0.8,
            },
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
                probability_positive: 0.5,
                eta_plus: 1.2,
                eta_minus: 1.2,
            },
            coupled_gap_marks: CoupledGapMarkConfiguration { sigma_g: 0.2 },
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 0.0,
            return_jump_scale: 0.0,
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 0.0,
            return_jump_scale: 0.0,
        },
    )
    .expect("regression news process configuration should validate")
}

fn regression_mean_reversion_diagnostics(
    fitted_parameters: &MeanReversionFittedParameters,
) -> MeanReversionStageDiagnostics {
    MeanReversionStageDiagnostics {
        selected_candidate_family: "artifact_regression_fixture".to_string(),
        calibration_grid: MeanReversionStageCalibrationGrid {
            calibration_grid_kind: "artifact_regression_fixture.v1".to_string(),
            calibration_grid_interval_hours: 1.0,
            source_observation_count: 0,
            retained_observation_count: 0,
            coverage_ratio: 0.0,
        },
        anchor_parameter_estimates: fitted_parameters.anchor_model.clone(),
        anchor_origin_level: None,
        phi_reference: 1.0,
        kappa: fitted_parameters.kappa,
        residual_sum_of_squares: 0.0,
        quasi_log_likelihood: None,
        information_criterion_bic: None,
        model_selection_policy: "artifact_regression_fixture.v1".to_string(),
        deviation_sample_count: 0,
        regression_pair_count: 0,
        least_squares_rank: 0,
        evaluated_candidates: Vec::new(),
        stage_decision_reason: "artifact regression fixture".to_string(),
    }
}

fn regression_joint_refinement_objective_breakdown() -> JointRefinementStageObjectiveBreakdown {
    JointRefinementStageObjectiveBreakdown {
        policy: CalibrationObjectivePolicy::default(),
        global_loss: 0.0,
        family_losses: Vec::new(),
        term_diagnostics: Vec::new(),
        hard_constraint_diagnostics: Vec::new(),
        admissibility: CalibrationLossAdmissibility {
            admissible: true,
            max_standardized_residual: 0.0,
            excess_loss: 0.0,
            blockers: Vec::new(),
        },
    }
}

fn regression_joint_refinement_optimizer_diagnostics() -> JointRefinementStageOptimizerDiagnostics {
    JointRefinementStageOptimizerDiagnostics {
        local_objective: 0.0,
        final_objective: 0.0,
        local_evaluation_count: 1,
        final_simulation_seeds: vec![1],
        stop_reason: "artifact_regression_fixture".to_string(),
        restart_diagnostics: JointRefinementStageRestartDiagnostics {
            restart_count: 1,
            valid_restart_count: 1,
            invalid_restart_count: 0,
            unique_terminal_candidate_count: 1,
            terminal_collision_count: 0,
            terminal_collision_share: 0.0,
            stop_reason_frequencies: Vec::new(),
            invalid_reason_frequencies: Vec::new(),
        },
        runtime_profile: JointRefinementStageRuntimeProfile {
            restart_optimization_wall_clock_millis: 0,
            coarse_consistency_wall_clock_millis: 0,
            final_seed_evaluation_wall_clock_millis: 0,
            final_assembly_wall_clock_millis: 0,
            objective_cache_hit_count: 0,
            objective_cache_miss_count: 1,
            deterministic_cache_hit_count: 0,
            deterministic_cache_miss_count: 1,
            seed_summary_cache_hit_count: 0,
            seed_summary_cache_miss_count: 1,
            aggregated_likelihood_context_cache_hit_count: 0,
            aggregated_likelihood_context_cache_miss_count: 0,
            option_pricing_cache_hit_count: 0,
            option_pricing_cache_miss_count: 0,
            option_pricing_run_count: 0,
            invalid_candidate_evaluation_count: 0,
            cumulative_deterministic_evaluation_millis: 0,
            cumulative_modeled_rate_millis: 0,
            cumulative_likelihood_context_resolution_millis: 0,
            cumulative_likelihood_context_5m_sample_millis: 0,
            cumulative_likelihood_context_15m_sample_millis: 0,
            cumulative_likelihood_context_tick_sample_millis: 0,
            cumulative_likelihood_context_tick_optimizer_millis: 0,
            cumulative_likelihood_context_5m_optimizer_millis: 0,
            cumulative_likelihood_context_15m_optimizer_millis: 0,
            cumulative_tick_likelihood_millis: 0,
            cumulative_5m_likelihood_millis: 0,
            cumulative_15m_likelihood_millis: 0,
            cumulative_expected_activity_counts_millis: 0,
            cumulative_deterministic_activity_shape_millis: 0,
            cumulative_deterministic_activity_level_millis: 0,
            cumulative_seed_summary_simulation_millis: 0,
            cumulative_raw_seed_metrics_millis: 0,
            cumulative_loss_build_millis: 0,
            cumulative_option_pricing_millis: 0,
        },
    }
}
