mod support;

use janiform_calibration_artifacts::{CalibrationExecutionMode, CalibrationStage};
use janiform_calibration_runtime::{
    CalibrationMemoryCheckpointStore, CalibrationStageExecutionStatus,
    CalibrationStagedRunStageReport, MeanReversionStageExecutionInput,
    MeanReversionStageObservation, MeanReversionStageReturnComponentsInput,
    MeanReversionStageSeasonalityInput, run_mean_reversion_calibration_stage,
    run_staged_calibration_with_checkpointing,
};
use support::shared::{
    build_hawkes_stage_success_payload, build_return_components_stage_success_payload,
    build_run_contract_with_mode_and_resume, build_seasonality_stage_success_payload,
    build_through_mean_reversion_run_contract_with_mode_and_resume,
    calibration_stage_test_registry, staged_run_inputs,
};

fn synthetic_constant_anchor_observations() -> Vec<MeanReversionStageObservation> {
    [
        (1_770_042_600_000_000_000_i64, 4.4_f64),
        (1_770_129_000_000_000_000_i64, 4.2_f64),
        (1_770_215_400_000_000_000_i64, 3.8_f64),
        (1_770_301_800_000_000_000_i64, 3.6_f64),
    ]
    .into_iter()
    .map(
        |(observation_timestamp_utc_nanos, observed_log_price)| MeanReversionStageObservation {
            observation_timestamp_utc_nanos,
            observed_log_price,
        },
    )
    .collect()
}

fn direct_stage_input(
    observations: Vec<MeanReversionStageObservation>,
) -> MeanReversionStageExecutionInput {
    MeanReversionStageExecutionInput {
        return_components_input: Some(MeanReversionStageReturnComponentsInput::InMemoryOutput {
            selected_candidate_id: "return_candidate_balanced".to_string(),
        }),
        seasonality_stage_input: Some(
            MeanReversionStageSeasonalityInput::from_seasonality_stage_payload(
                &build_seasonality_stage_success_payload(),
            ),
        ),
        hawkes_stage_payload: Some(build_hawkes_stage_success_payload()),
        return_components_stage_payload: Some(build_return_components_stage_success_payload()),
        calibration_grid_kind: "synthetic_daily_close_grid.v1".to_string(),
        calibration_grid_interval_hours: 24.0,
        source_observation_count: observations.len(),
        coverage_ratio: 1.0,
        observations,
    }
}

#[test]
fn mean_reversion_stage_estimates_endogenous_anchor_decay_from_structural_recursion() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-endogenous-estimated",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        direct_stage_input(synthetic_constant_anchor_observations()),
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

    let payload = report
        .fitted_stage_payload
        .expect("endogenous anchor fit should emit a fitted payload");
    assert!(payload.fitted_parameters.kappa >= 0.0);
    assert!(payload.diagnostics.stage_decision_reason.contains(
        "endogenous_core_pressure_level_profiled_over_pressure_persistence_and_phi_reference.v1"
    ));
}

#[test]
fn mean_reversion_stage_accepts_flat_levels_under_endogenous_anchor_estimation() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-flat-levels",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let observations = vec![
        MeanReversionStageObservation {
            observation_timestamp_utc_nanos: 1_770_042_600_000_000_000,
            observed_log_price: 4.0,
        },
        MeanReversionStageObservation {
            observation_timestamp_utc_nanos: 1_770_129_000_000_000_000,
            observed_log_price: 4.0,
        },
        MeanReversionStageObservation {
            observation_timestamp_utc_nanos: 1_770_215_400_000_000_000,
            observed_log_price: 4.0,
        },
        MeanReversionStageObservation {
            observation_timestamp_utc_nanos: 1_770_301_800_000_000_000,
            observed_log_price: 4.0,
        },
    ];

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        direct_stage_input(observations),
    );

    let payload = report
        .fitted_stage_payload
        .expect("flat levels should emit a fitted payload under endogenous estimation");
    assert!(payload.diagnostics.phi_reference.is_finite());
    assert!(payload.fitted_parameters.kappa.is_finite());
}

#[test]
fn mean_reversion_stage_reports_endogenous_anchor_fit_from_synthetic_levels() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-endogenous-anchor",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        direct_stage_input(synthetic_constant_anchor_observations()),
    );

    let payload = report
        .fitted_stage_payload
        .expect("endogenous-anchor fit should emit a fitted payload");

    assert!(payload.diagnostics.phi_reference.is_finite());
    assert!(payload.fitted_parameters.kappa.is_finite());
}

#[test]
fn mean_reversion_stage_falls_back_to_zero_kappa_endogenous_fit_when_runtime_projection_is_required()
 {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-runtime-projection-required",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        direct_stage_input(Vec::new()),
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

    let payload = report
        .fitted_stage_payload
        .expect("runtime-projection fallback should emit a fitted payload");

    assert_eq!(payload.fitted_parameters.kappa, 0.0);
    assert_eq!(payload.diagnostics.phi_reference, 1.0);
    assert_eq!(payload.diagnostics.deviation_sample_count, 0);
    assert_eq!(payload.diagnostics.regression_pair_count, 0);
    assert_eq!(
        payload.diagnostics.stage_decision_reason,
        "endogenous_anchor_runtime_projection_required.v1"
    );
}

#[test]
fn mean_reversion_stage_profiles_slow_anchor_against_immediate_anchor() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-slow-anchor-profile",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        direct_stage_input(synthetic_constant_anchor_observations()),
    );

    let payload = report
        .fitted_stage_payload
        .expect("slow anchor profile fit should emit a fitted payload");

    assert_eq!(payload.diagnostics.evaluated_candidates.len(), 4);
    assert!(
        payload
            .diagnostics
            .evaluated_candidates
            .iter()
            .any(|candidate| candidate.candidate_family
                == "endogenous_core_pressure_level_mean_reversion")
    );
    assert_eq!(
        payload.diagnostics.model_selection_policy,
        "gaussian_quasi_likelihood_bic_with_endogenous_core_pressure_level_candidate_family_profile.v1"
    );
}

#[test]
fn staged_runner_completes_when_mean_reversion_stage_estimates_endogenous_anchor_requests() {
    let run_contract = build_through_mean_reversion_run_contract_with_mode_and_resume(
        "run-mean-reversion-endogenous-success",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();
    let mut execution_input = staged_run_inputs(true, true, true);

    let mean_reversion_input = execution_input
        .mean_reversion_stage_input
        .as_mut()
        .expect("mean-reversion stage input fixture should exist");

    mean_reversion_input.source_observation_count = 4;
    mean_reversion_input.coverage_ratio = 1.0;
    mean_reversion_input.observations = synthetic_constant_anchor_observations();

    let report = run_staged_calibration_with_checkpointing(
        &run_contract,
        &calendar_registry,
        execution_input,
        Some(&mut checkpoint_store),
        None,
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);
    assert_eq!(report.failure, None);

    let mean_reversion_report = report
        .stage_reports
        .iter()
        .find_map(|stage_report| match stage_report {
            CalibrationStagedRunStageReport::MeanReversion(mean_reversion_report) => {
                Some(mean_reversion_report)
            }
            _ => None,
        })
        .expect("staged run should emit the mean-reversion stage failure report");

    assert_eq!(
        mean_reversion_report.status,
        CalibrationStageExecutionStatus::Success
    );
    assert!(matches!(
        report.stage_reports.last(),
        Some(CalibrationStagedRunStageReport::MeanReversion(_))
    ));
}

#[test]
fn staged_runner_end_to_end_reports_mean_reversion_fit_from_synthetic_levels() {
    let run_contract = build_through_mean_reversion_run_contract_with_mode_and_resume(
        "run-mean-reversion-end-to-end",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();
    let mut execution_input = staged_run_inputs(true, true, true);

    let mean_reversion_input = execution_input
        .mean_reversion_stage_input
        .as_mut()
        .expect("mean-reversion stage input fixture should exist");

    mean_reversion_input.source_observation_count = 4;
    mean_reversion_input.coverage_ratio = 1.0;
    mean_reversion_input.observations = synthetic_constant_anchor_observations();

    let report = run_staged_calibration_with_checkpointing(
        &run_contract,
        &calendar_registry,
        execution_input,
        Some(&mut checkpoint_store),
        None,
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

    let mean_reversion_report = report
        .stage_reports
        .iter()
        .find_map(|stage_report| match stage_report {
            CalibrationStagedRunStageReport::MeanReversion(mean_reversion_report) => {
                Some(mean_reversion_report)
            }
            _ => None,
        })
        .expect("full staged run should include the mean-reversion stage");

    let payload = mean_reversion_report
        .fitted_stage_payload
        .as_ref()
        .expect("mean-reversion stage should succeed");

    assert_eq!(mean_reversion_report.stage, CalibrationStage::MeanReversion);
    assert!(
        payload
            .return_components_selected_candidate_id
            .as_deref()
            .is_some_and(|candidate_id| !candidate_id.is_empty())
    );
    assert!(payload.diagnostics.stage_decision_reason.contains(
        "endogenous_core_pressure_level_profiled_over_pressure_persistence_and_phi_reference.v1"
    ));
    assert!(payload.diagnostics.phi_reference.is_finite());
    assert!(payload.diagnostics.kappa.is_finite());
    assert!(payload.fitted_parameters.kappa.is_finite());
}
