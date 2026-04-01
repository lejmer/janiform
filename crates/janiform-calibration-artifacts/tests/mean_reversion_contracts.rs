mod support;

use janiform_calibration_artifacts::CalibrationExecutionMode;
use janiform_model_kernels::mean_reversion::MeanReversionAnchorModelKind;
use support::shared::{
    CalibrationRunContract, CalibrationRunSeedSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageGraph, CalibrationStageHandoffTransport,
    CalibrationStageParameterArtifactInput, CalibrationStageParameterBuildMetadata,
    CalibrationStageSolverSettings, MeanReversionAnchorModel, MeanReversionBoundaryState,
    MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
    MeanReversionStageCalibrationGrid, MeanReversionStageCandidateDiagnostics,
    MeanReversionStageDiagnostics, MeanReversionStageParameterArtifactInput,
    build_calibration_stage_parameter_artifact, build_data_inputs,
    build_endogenous_mean_reversion_parameters, build_solver_settings,
    load_calibration_stage_parameter_artifact_from_disk,
    persist_calibration_stage_parameter_artifact_to_run_directory,
    resolve_calibration_stage_seed_metadata, stage_solver_settings_for_stage,
    temporary_checkpoint_directory,
};

#[test]
fn mean_reversion_fitted_parameters_round_trip_serialization_preserves_contract_fields() {
    let fitted_parameters = build_endogenous_mean_reversion_parameters();

    let serialized = serde_json::to_value(&fitted_parameters)
        .expect("mean reversion fitted parameters should serialize");
    assert_eq!(serialized["kappa"], serde_json::json!(0.25));
    assert_eq!(
        serialized["anchor_model"]["anchor_model_kind"],
        serde_json::json!("endogenous_core_pressure_level")
    );

    let parsed: MeanReversionFittedParameters = serde_json::from_value(serialized)
        .expect("mean reversion fitted parameters should deserialize");
    assert_eq!(parsed, fitted_parameters);

    fitted_parameters
        .validate()
        .expect("round-tripped mean reversion fitted parameters should validate");
}

#[test]
fn mean_reversion_fitted_parameters_reject_negative_kappa() {
    let fitted_parameters = MeanReversionFittedParameters {
        kappa: -0.01,
        anchor_model: MeanReversionAnchorModel::zero_gain_endogenous_core_pressure_level(),
    };

    let error = fitted_parameters
        .validate()
        .expect_err("negative kappa must fail validation");
    assert!(
        error
            .to_string()
            .contains("kappa must be a finite non-negative number")
    );
}

#[test]
fn endogenous_mean_reversion_anchor_model_rejects_exogenous_anchor_values() {
    let anchor_model = serde_json::from_value::<MeanReversionAnchorModel>(serde_json::json!({
        "anchor_model_kind": MeanReversionAnchorModelKind::EndogenousCorePressureLevel,
        "reference_interval_hours": 24.0,
        "pressure_persistence_per_reference_interval": 0.5,
        "pressure_level_gain": 1.0,
        "anchor_value": 0.0,
    }))
    .expect_err("endogenous mean-reversion anchor must reject exogenous values");

    assert!(
        anchor_model
            .to_string()
            .contains("unknown field `anchor_value`")
    );
}

#[test]
fn mean_reversion_runtime_interval_anchors_validate_alignment_metadata() {
    let runtime_interval_anchors = MeanReversionRuntimeIntervalAnchors::try_new(
        "explicit_runtime_interval_grid_alignment.v1",
        4.2,
        0.3,
        vec![4.1, 4.0],
        vec![0.2, 0.1],
        vec![1.0, 1.0],
    )
    .expect("runtime interval anchors should validate");

    let serialized = serde_json::to_string(&runtime_interval_anchors)
        .expect("runtime interval anchors should serialize");
    let parsed: MeanReversionRuntimeIntervalAnchors = serde_json::from_str(serialized.as_str())
        .expect("runtime interval anchors should deserialize");
    assert_eq!(parsed, runtime_interval_anchors);
}

#[test]
fn mean_reversion_boundary_state_rejects_invariant_drift() {
    let error = MeanReversionBoundaryState::try_new(5.0, 1.5, 0.0, 3.7)
        .expect_err("boundary state must reject deviation drift");

    assert!(
        error
            .to_string()
            .contains("deviation must equal observed_log_price - anchor_value")
    );
}

#[test]
fn calibration_stage_parameter_artifact_persistence_round_trips_mean_reversion_block() {
    let run_spec = CalibrationRunSpec::try_new(
        "run-mean-reversion-artifact",
        build_data_inputs(),
        CalibrationExecutionMode::Disk,
        vec![CalibrationStage::MeanReversion],
        CalibrationRunSeedSettings::try_new(17, vec![])
            .expect("mean-reversion seed settings should validate"),
        vec![CalibrationStageSolverSettings {
            stage: CalibrationStage::MeanReversion,
            optimizer_settings: build_solver_settings(),
        }],
    )
    .expect("mean-reversion run spec should validate");
    let run_contract =
        CalibrationRunContract::try_new(run_spec, CalibrationStageGraph::new(vec![]))
            .expect("mean-reversion run contract should validate");

    let run_directory = temporary_checkpoint_directory("mean-reversion-stage-parameter");
    let stage_seed_metadata = resolve_calibration_stage_seed_metadata(
        &run_contract.run_spec,
        CalibrationStage::MeanReversion,
    );
    let solver_settings =
        stage_solver_settings_for_stage(&run_contract.run_spec, CalibrationStage::MeanReversion)
            .expect("mean-reversion solver settings should exist");
    let fitted_parameters = build_endogenous_mean_reversion_parameters();
    let diagnostics = MeanReversionStageDiagnostics {
        chosen_anchor_family: MeanReversionAnchorModelKind::EndogenousCorePressureLevel,
        selected_candidate_family: "endogenous_core_pressure_level_mean_reversion".to_string(),
        calibration_grid: MeanReversionStageCalibrationGrid {
            calibration_grid_kind: "synthetic_daily_close_grid.v1".to_string(),
            calibration_grid_interval_hours: 24.0,
            source_observation_count: 4,
            retained_observation_count: 4,
            coverage_ratio: 1.0,
        },
        anchor_parameter_estimates: fitted_parameters.anchor_model.clone(),
        anchor_origin_level: Some(4.2),
        phi_reference: 0.85,
        kappa: fitted_parameters.kappa,
        residual_sum_of_squares: 0.2,
        quasi_log_likelihood: Some(-1.5),
        information_criterion_bic: Some(5.0),
        model_selection_policy:
            "gaussian_quasi_likelihood_bic_with_endogenous_core_pressure_level_candidate_family_profile.v1".to_string(),
        deviation_sample_count: 4,
        regression_pair_count: 3,
        least_squares_rank: 2,
        evaluated_candidates: vec![MeanReversionStageCandidateDiagnostics {
            candidate_family: "endogenous_core_pressure_level_mean_reversion".to_string(),
            anchor_model: fitted_parameters.anchor_model.clone(),
            anchor_origin_level: Some(4.2),
            phi_reference: 0.85,
            kappa: fitted_parameters.kappa,
            residual_sum_of_squares: 0.2,
            quasi_log_likelihood: Some(-1.5),
            information_criterion_bic: Some(5.0),
            parameter_count: 4,
            least_squares_rank: 2,
            deviation_sample_count: 4,
            regression_pair_count: 3,
            core_expected_increments: vec![0.1, 0.1, 0.1],
            pressure_state_path: vec![0.0, 0.1, 0.15, 0.175],
            boundary_anchor_values: vec![4.2, 4.3, 4.4, 4.5],
            deviations: vec![0.0, -0.1, -0.2, -0.3],
            residuals: vec![-0.1, -0.115, -0.13],
            stage_decision_reason:
                "endogenous_core_pressure_level_stage_contract_roundtrip_fixture.v1"
                    .to_string(),
        }],
        stage_decision_reason:
            "endogenous_core_pressure_level_stage_contract_roundtrip_fixture.v1".to_string(),
    };
    let artifact = build_calibration_stage_parameter_artifact(
        &run_contract,
        CalibrationStageParameterArtifactInput::MeanReversion(
            MeanReversionStageParameterArtifactInput {
                metadata: CalibrationStageParameterBuildMetadata {
                    stage_output_schema_version:
                        janiform_calibration_artifacts::CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION
                            .to_string(),
                    run_global_seed: stage_seed_metadata.run_global_seed,
                    stage_seed: stage_seed_metadata.stage_seed,
                    stage_seed_source: stage_seed_metadata.stage_seed_source,
                    stage_seed_derivation_policy:
                        janiform_calibration_artifacts::CALIBRATION_STAGE_SEED_DERIVATION_POLICY
                            .to_string(),
                    solver_settings,
                },
                fitted_parameters: fitted_parameters.clone(),
                diagnostics,
                return_components_input_transport: CalibrationStageHandoffTransport::InMemoryOutput,
                return_components_selected_candidate_id: Some(
                    "return_candidate_balanced".to_string(),
                ),
                return_components_artifact_reference: None,
            },
        ),
    )
    .expect("mean-reversion stage parameter artifact should build");

    let artifact_path = persist_calibration_stage_parameter_artifact_to_run_directory(
        run_directory.as_path(),
        &artifact,
    )
    .expect("stage parameter artifact should persist");

    let loaded_artifact =
        load_calibration_stage_parameter_artifact_from_disk(artifact_path.as_path())
            .expect("stage parameter artifact should reload");

    assert_eq!(loaded_artifact, artifact);
}
