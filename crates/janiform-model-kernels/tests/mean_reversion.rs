use std::f64::NAN;

use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionBoundaryStateEvolutionError,
    MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
    evolve_mean_reversion_boundary_state_on_interval_grid,
    evolve_mean_reversion_boundary_state_with_anchor_increment,
};

#[test]
fn mean_reversion_fitted_parameters_round_trip_serialization_preserves_contract_fields() {
    let fitted_parameters = MeanReversionFittedParameters::try_new(
        0.25,
        MeanReversionAnchorModel::endogenous_core_expectation_one_interval(),
    )
    .expect("mean reversion fitted parameters should validate");

    let serialized = serde_json::to_value(&fitted_parameters)
        .expect("mean reversion fitted parameters should serialize");
    assert_eq!(serialized["kappa"], serde_json::json!(0.25));
    assert_eq!(
        serialized["anchor_model"]["anchor_model_kind"],
        serde_json::json!("endogenous_core_expectation_one_interval")
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
        anchor_model: MeanReversionAnchorModel::endogenous_core_expectation_one_interval(),
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
    let anchor_model = MeanReversionAnchorModel {
        anchor_model_kind: janiform_model_kernels::mean_reversion::MeanReversionAnchorModelKind::EndogenousCoreExpectationOneInterval,
        anchor_value: Some(NAN),
        source_interval_start_utc_nanos: Vec::new(),
        source_interval_end_utc_nanos: Vec::new(),
        anchor_values: Vec::new(),
        source_alignment_policy: None,
    }
    .validate()
    .expect_err("endogenous anchor policy must reject exogenous anchor fields");

    assert!(
        anchor_model
            .to_string()
            .contains("must not set field `anchor_value`")
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

    let evolved_state =
        evolve_mean_reversion_boundary_state_on_interval_grid(boundary_state, 0.4, 2.0, 0.25, 1.25)
            .expect("interval-grid state update should succeed");

    let expected_deviation = (-0.8_f64).exp() * 0.5 + 0.25 + 1.0 - 1.25;
    assert!((evolved_state.deviation - expected_deviation).abs() < 1.0e-12);
    assert!((evolved_state.observed_log_price - (1.25 + expected_deviation)).abs() < 1.0e-12);
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

#[test]
fn endogenous_anchor_increment_evolution_matches_canonical_recursion() {
    let boundary_state = MeanReversionBoundaryState::try_new(1.5, 1.0, 0.5)
        .expect("boundary state should construct");

    let evolved_state = evolve_mean_reversion_boundary_state_with_anchor_increment(
        boundary_state,
        0.4,
        2.0,
        0.25,
        0.25,
    )
    .expect("endogenous-anchor state update should succeed");

    let expected_anchor_value = 1.25;
    let expected_deviation = (-0.8_f64).exp() * 0.5 + 0.25 - 0.25;

    assert!((evolved_state.anchor_value - expected_anchor_value).abs() < 1.0e-12);
    assert!((evolved_state.deviation - expected_deviation).abs() < 1.0e-12);
    assert!(
        (evolved_state.observed_log_price - (expected_anchor_value + expected_deviation)).abs()
            < 1.0e-12
    );
}
