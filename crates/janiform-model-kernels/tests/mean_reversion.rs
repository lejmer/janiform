use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionBoundaryStateEvolutionError,
    MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
    evolve_mean_reversion_boundary_state_on_interval_grid,
    evolve_mean_reversion_boundary_state_with_endogenous_pressure,
    materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments,
};

#[test]
fn mean_reversion_fitted_parameters_round_trip_serialization_preserves_contract_fields() {
    let fitted_parameters = MeanReversionFittedParameters::try_new(
        0.25,
        MeanReversionAnchorModel::endogenous_core_pressure_level(24.0, 0.5, 1.25),
    )
    .expect("mean reversion fitted parameters should validate");

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
fn endogenous_mean_reversion_anchor_model_rejects_unknown_fields() {
    let anchor_model = serde_json::from_value::<MeanReversionAnchorModel>(serde_json::json!({
        "anchor_model_kind": janiform_model_kernels::mean_reversion::MeanReversionAnchorModelKind::EndogenousCorePressureLevel,
        "reference_interval_hours": 24.0,
        "pressure_persistence_per_reference_interval": 0.5,
        "pressure_level_gain": 1.0,
        "anchor_value": 0.0,
    }))
    .expect_err("mean-reversion anchor model must reject unknown fields");

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
fn mean_reversion_interval_grid_evolution_matches_anchor_deviation_formula() {
    let boundary_state = MeanReversionBoundaryState::try_new(1.5, 1.0, 0.2, 0.5)
        .expect("boundary state should construct");

    let evolved_state = evolve_mean_reversion_boundary_state_on_interval_grid(
        boundary_state,
        0.4,
        2.0,
        0.25,
        1.25,
        0.35,
    )
    .expect("interval-grid state update should succeed");

    let expected_deviation = (-0.8_f64).exp() * 0.5 + 0.25 + 1.0 - 1.25;
    assert!((evolved_state.deviation - expected_deviation).abs() < 1.0e-12);
    assert!((evolved_state.observed_log_price - (1.25 + expected_deviation)).abs() < 1.0e-12);
    assert_eq!(evolved_state.pressure_state, 0.35);
}

#[test]
fn mean_reversion_interval_grid_evolution_rejects_invalid_kappa() {
    let boundary_state = MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
        .expect("boundary state should construct");

    let error = evolve_mean_reversion_boundary_state_on_interval_grid(
        boundary_state,
        -0.1,
        1.0,
        0.0,
        0.0,
        0.0,
    )
    .expect_err("negative kappa must fail");

    assert_eq!(
        error,
        MeanReversionBoundaryStateEvolutionError::InvalidKappa { value: -0.1 }
    );
}

#[test]
fn endogenous_pressure_state_evolution_matches_canonical_recursion() {
    let boundary_state = MeanReversionBoundaryState::try_new(1.5, 1.0, 0.1, 0.5)
        .expect("boundary state should construct");
    let fitted_parameters = MeanReversionFittedParameters::try_new(
        0.4,
        MeanReversionAnchorModel::endogenous_core_pressure_level(2.0, 0.5, 2.0),
    )
    .expect("pressure-to-level mean reversion parameters should validate");

    let evolved_state = evolve_mean_reversion_boundary_state_with_endogenous_pressure(
        boundary_state,
        &fitted_parameters,
        2.0,
        0.25,
        0.25,
    )
    .expect("pressure-to-level state update should succeed");

    let expected_pressure_state = 0.5_f64 * 0.1 + (1.0 - 0.5_f64) * 0.25;
    let expected_anchor_value = 1.0 + 2.0 * (expected_pressure_state - 0.1);
    let expected_deviation = (-0.8_f64).exp() * 0.5 + 0.25 + 1.0 - expected_anchor_value;

    assert!((evolved_state.pressure_state - expected_pressure_state).abs() < 1.0e-12);
    assert!((evolved_state.anchor_value - expected_anchor_value).abs() < 1.0e-12);
    assert!((evolved_state.deviation - expected_deviation).abs() < 1.0e-12);
}

#[test]
fn endogenous_pressure_level_anchor_materialization_tracks_pressure_state_and_anchor_level() {
    let anchor_model = MeanReversionAnchorModel::endogenous_core_pressure_level(1.0, 0.5, 2.0);

    let runtime_interval_anchors =
        materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments(
            "explicit_runtime_interval_grid_alignment.v1",
            0.0,
            0.0,
            &anchor_model,
            &[1.0, 3.0],
            &[1.0, 1.0],
        )
        .expect("pressure-to-level anchor materialization should succeed");

    assert_eq!(runtime_interval_anchors.start_anchor_value, 0.0);
    assert_eq!(runtime_interval_anchors.start_pressure_state, 0.0);
    assert!((runtime_interval_anchors.end_pressure_states[0] - 0.5).abs() < 1.0e-12);
    assert!((runtime_interval_anchors.end_anchor_values[0] - 1.0).abs() < 1.0e-12);
    assert!((runtime_interval_anchors.end_pressure_states[1] - 1.75).abs() < 1.0e-12);
    assert!((runtime_interval_anchors.end_anchor_values[1] - 3.5).abs() < 1.0e-12);
}
