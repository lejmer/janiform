mod support;

use std::f64::NAN;

use support::shared::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors, build_deterministic_interval_grid_mean_reversion_parameters,
    build_return_components_stage_parameter_artifact_fixture, build_run_contract_with_mode_and_resume,
    load_calibration_stage_parameter_artifact_from_disk,
    persist_calibration_stage_parameter_artifact_to_run_directory, temporary_checkpoint_directory,
};
use janiform_calibration_artifacts::CalibrationExecutionMode;

#[test]
fn mean_reversion_fitted_parameters_round_trip_serialization_preserves_contract_fields() {
    let fitted_parameters = build_deterministic_interval_grid_mean_reversion_parameters();

    let serialized = serde_json::to_value(&fitted_parameters)
        .expect("mean reversion fitted parameters should serialize");
    assert_eq!(serialized["kappa"], serde_json::json!(0.25));
    assert_eq!(
        serialized["anchor_model"]["anchor_model_kind"],
        serde_json::json!("deterministic_interval_grid")
    );

    let parsed: MeanReversionFittedParameters =
        serde_json::from_value(serialized).expect("mean reversion fitted parameters should deserialize");
    assert_eq!(parsed, fitted_parameters);

    fitted_parameters
        .validate()
        .expect("round-tripped mean reversion fitted parameters should validate");
}

#[test]
fn mean_reversion_fitted_parameters_reject_negative_kappa() {
    let fitted_parameters = MeanReversionFittedParameters {
        kappa: -0.01,
        anchor_model: MeanReversionAnchorModel::disabled(),
    };

    let error = fitted_parameters
        .validate()
        .expect_err("negative kappa must fail validation");
    assert!(error.to_string().contains("kappa must be a finite non-negative number"));
}

#[test]
fn mean_reversion_anchor_model_rejects_non_finite_anchor_values() {
    let anchor_model = MeanReversionAnchorModel::deterministic_interval_grid(
        vec![1, 2],
        vec![2, 3],
        vec![1.0, NAN],
        "piecewise_constant_utc_interval_grid.v1",
    )
    .expect_err("non-finite anchor values must fail validation");

    assert!(anchor_model
        .to_string()
        .contains("anchor_values must be a finite number"));
}

#[test]
fn mean_reversion_runtime_interval_anchors_validate_alignment_metadata() {
    let runtime_interval_anchors = MeanReversionRuntimeIntervalAnchors::try_new(
        "explicit_runtime_interval_grid_alignment.v1",
        4.2,
        vec![4.1, 4.0],
        vec![1.0, 1.0],
    )
    .expect("runtime interval anchors should validate");

    let serialized = serde_json::to_string(&runtime_interval_anchors)
        .expect("runtime interval anchors should serialize");
    let parsed: MeanReversionRuntimeIntervalAnchors =
        serde_json::from_str(serialized.as_str()).expect("runtime interval anchors should deserialize");
    assert_eq!(parsed, runtime_interval_anchors);
}

#[test]
fn mean_reversion_boundary_state_rejects_invariant_drift() {
    let error = MeanReversionBoundaryState::try_new(5.0, 1.5, 3.7)
        .expect_err("boundary state must reject deviation drift");

    assert!(error
        .to_string()
        .contains("deviation must equal observed_log_price - anchor_value"));
}

#[test]
fn calibration_stage_parameter_artifact_persistence_round_trips_mean_reversion_block() {
    let run_contract =
        build_run_contract_with_mode_and_resume("run-mean-reversion-artifact", CalibrationExecutionMode::Disk, None);

    let run_directory = temporary_checkpoint_directory("mean-reversion-stage-parameter");
    let artifact = build_return_components_stage_parameter_artifact_fixture(&run_contract);

    let artifact_path = persist_calibration_stage_parameter_artifact_to_run_directory(
        run_directory.as_path(),
        &artifact,
    )
    .expect("stage parameter artifact should persist");

    let loaded_artifact = load_calibration_stage_parameter_artifact_from_disk(artifact_path.as_path())
        .expect("stage parameter artifact should reload");

    assert_eq!(loaded_artifact, artifact);
}
