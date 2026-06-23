use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionBoundaryStateEvolutionError,
    MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
    evolve_mean_reversion_boundary_state_on_interval_grid,
    evolve_mean_reversion_deviation_with_decay_on_interval_grid,
};

#[test]
fn mean_reversion_fitted_parameters_round_trip_serialization_preserves_contract_fields() {
    let fitted_parameters =
        MeanReversionFittedParameters::try_new(0.25).expect("parameters should validate");

    let serialized = serde_json::to_value(fitted_parameters)
        .expect("mean reversion fitted parameters should serialize");
    assert_eq!(serialized, serde_json::json!({ "kappa": 0.25 }));

    let parsed: MeanReversionFittedParameters = serde_json::from_value(serialized)
        .expect("mean reversion fitted parameters should deserialize");

    assert_eq!(parsed, fitted_parameters);
}

#[test]
fn mean_reversion_fitted_parameters_reject_negative_kappa() {
    let fitted_parameters = MeanReversionFittedParameters { kappa: -0.01 };

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
    let parsed: MeanReversionRuntimeIntervalAnchors = serde_json::from_str(serialized.as_str())
        .expect("runtime interval anchors should deserialize");

    assert_eq!(parsed, runtime_interval_anchors);
}

#[test]
fn mean_reversion_boundary_state_rejects_invariant_drift() {
    let error = MeanReversionBoundaryState::try_new(5.0, 1.5, 3.7)
        .expect_err("boundary state must reject deviation drift");

    assert!(
        error
            .to_string()
            .contains("deviation must equal observed_log_price - anchor_value")
    );
}

#[test]
fn mean_reversion_interval_grid_evolution_matches_anchor_deviation_formula() {
    let boundary_state = MeanReversionBoundaryState::try_new(1.5, 1.0, 0.5)
        .expect("boundary state should construct");

    let precomputed_deviation = evolve_mean_reversion_deviation_with_decay_on_interval_grid(
        0.5,
        (-0.8_f64).exp(),
        0.25,
        1.0,
        1.25,
    )
    .expect("precomputed deviation update should succeed");
    let evolved_state =
        evolve_mean_reversion_boundary_state_on_interval_grid(boundary_state, 0.4, 2.0, 0.25, 1.25)
            .expect("interval-grid state update should succeed");

    let expected_deviation = (-0.8_f64).exp().mul_add(0.5, 0.25 + 1.0 - 1.25);
    let expected_state =
        MeanReversionBoundaryState::try_new(1.25 + expected_deviation, 1.25, expected_deviation)
            .expect("expected state should validate");

    assert_eq!(precomputed_deviation, expected_deviation);
    assert_eq!(evolved_state, expected_state);
}

#[test]
fn mean_reversion_interval_grid_evolution_rejects_invalid_kappa() {
    let boundary_state = MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0)
        .expect("boundary state should construct");

    let error =
        evolve_mean_reversion_boundary_state_on_interval_grid(boundary_state, -0.1, 1.0, 0.0, 0.0)
            .expect_err("negative kappa must fail");

    assert_eq!(
        error,
        MeanReversionBoundaryStateEvolutionError::InvalidKappa { value: -0.1 }
    );
}
