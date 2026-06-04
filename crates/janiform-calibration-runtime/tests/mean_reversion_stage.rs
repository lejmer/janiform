mod support;

use janiform_calibration_artifacts::{
    CalibrationExecutionMode, CalibrationRunContract, CalibrationStage,
    CalibrationStageSelectedCandidateSource,
};
use janiform_calibration_runtime::{
    CalibrationMemoryCheckpointStore, CalibrationStageExecutionStatus, CalibrationStagedRunInputs,
    CalibrationStagedRunReport, CalibrationStagedRunStageReport, MeanReversionStageExecutionInput,
    MeanReversionStageGridCandidate, MeanReversionStageObservation,
    MeanReversionStageSeasonalityInput, run_mean_reversion_calibration_stage,
    run_staged_calibration_with_checkpointing as run_staged_calibration_with_checkpointing_core,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_market_time::calendar::ExchangeCalendarRegistry;
use std::path::Path;
use support::shared::{
    build_hawkes_stage_success_payload, build_return_components_stage_success_payload,
    build_run_contract_with_mode_and_resume, build_seasonality_stage_success_payload,
    build_through_mean_reversion_run_contract_with_mode_and_resume,
    calibration_stage_test_registry, staged_run_inputs,
};

const TEST_PREDICTIVE_EVIDENCE_ALPHA: f64 = 0.05;

fn run_staged_calibration_with_checkpointing(
    run_contract: &CalibrationRunContract,
    calendar_registry: &ExchangeCalendarRegistry,
    staged_run_inputs: CalibrationStagedRunInputs,
    memory_checkpoint_store: Option<&mut CalibrationMemoryCheckpointStore>,
    disk_checkpoint_directory: Option<&Path>,
) -> CalibrationStagedRunReport {
    run_staged_calibration_with_checkpointing_core(
        run_contract,
        calendar_registry,
        staged_run_inputs,
        CancellationToken::uncancelled(),
        memory_checkpoint_store,
        disk_checkpoint_directory,
    )
}

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

fn synthetic_weekly_anchor_observations() -> Vec<MeanReversionStageObservation> {
    [
        (1_770_042_600_000_000_000_i64, 4.5_f64),
        (1_770_129_000_000_000_000_i64, 4.4_f64),
        (1_770_215_400_000_000_000_i64, 4.1_f64),
        (1_770_301_800_000_000_000_i64, 3.9_f64),
        (1_770_388_200_000_000_000_i64, 3.8_f64),
        (1_770_474_600_000_000_000_i64, 3.7_f64),
        (1_770_561_000_000_000_000_i64, 3.5_f64),
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

fn synthetic_partial_weekly_anchor_observations() -> Vec<MeanReversionStageObservation> {
    synthetic_weekly_anchor_observations()
        .into_iter()
        .take(6)
        .collect()
}

fn synthetic_regular_grid_observations(
    levels: Vec<f64>,
    interval_hours: i64,
) -> Vec<MeanReversionStageObservation> {
    let base_timestamp_utc_nanos = 1_770_042_600_000_000_000_i64;
    let interval_nanos = interval_hours * 3_600_000_000_000_i64;

    levels
        .into_iter()
        .enumerate()
        .map(
            |(observation_index, observed_log_price)| MeanReversionStageObservation {
                observation_timestamp_utc_nanos: base_timestamp_utc_nanos
                    + observation_index as i64 * interval_nanos,
                observed_log_price,
            },
        )
        .collect()
}

fn synthetic_predictive_mean_reverting_observations() -> Vec<MeanReversionStageObservation> {
    let levels = (0..96)
        .map(|observation_index| {
            if observation_index % 2 == 0 {
                0.001
            } else {
                -0.001
            }
        })
        .collect();

    synthetic_regular_grid_observations(levels, 24)
}

fn synthetic_random_walk_observations() -> Vec<MeanReversionStageObservation> {
    let mut levels = Vec::new();
    let mut level = 0.0_f64;

    for observation_index in 0..96 {
        levels.push(level);

        if observation_index % 2 == 0 {
            level += 0.0006;
        } else {
            level -= 0.0004;
        }
    }

    synthetic_regular_grid_observations(levels, 24)
}

fn direct_stage_input(
    observations: Vec<MeanReversionStageObservation>,
) -> MeanReversionStageExecutionInput {
    MeanReversionStageExecutionInput {
        return_components_input: Some(CalibrationStageSelectedCandidateSource::InMemoryOutput {
            selected_candidate_id: "return_candidate_balanced".to_string(),
        }),
        seasonality_stage_input: Some(
            MeanReversionStageSeasonalityInput::from_seasonality_stage_payload(
                &build_seasonality_stage_success_payload(),
            ),
        ),
        hawkes_stage_payload: Some(build_hawkes_stage_success_payload()),
        return_components_stage_payload: Some(build_return_components_stage_success_payload()),
        predictive_evidence_alpha: TEST_PREDICTIVE_EVIDENCE_ALPHA,
        grid_candidates: vec![MeanReversionStageGridCandidate {
            calibration_grid_kind: "synthetic_daily_close_grid.v1".to_string(),
            calibration_grid_interval_hours: 24.0,
            source_observation_count: observations.len(),
            coverage_ratio: 1.0,
            observations,
        }],
    }
}

fn direct_multi_grid_stage_input(
    grid_candidates: Vec<MeanReversionStageGridCandidate>,
) -> MeanReversionStageExecutionInput {
    MeanReversionStageExecutionInput {
        return_components_input: Some(CalibrationStageSelectedCandidateSource::InMemoryOutput {
            selected_candidate_id: "return_candidate_balanced".to_string(),
        }),
        seasonality_stage_input: Some(
            MeanReversionStageSeasonalityInput::from_seasonality_stage_payload(
                &build_seasonality_stage_success_payload(),
            ),
        ),
        hawkes_stage_payload: Some(build_hawkes_stage_success_payload()),
        return_components_stage_payload: Some(build_return_components_stage_success_payload()),
        predictive_evidence_alpha: TEST_PREDICTIVE_EVIDENCE_ALPHA,
        grid_candidates,
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
    assert_ne!(
        payload.diagnostics.stage_decision_reason,
        "endogenous_anchor_runtime_projection_required.v1"
    );
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
fn mean_reversion_stage_selects_positive_kappa_when_predictive_evidence_passes() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-positive-predictive-evidence",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        direct_stage_input(synthetic_predictive_mean_reverting_observations()),
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

    let payload = report
        .fitted_stage_payload
        .expect("predictive mean-reversion sample should emit a fitted payload");

    assert!(payload.fitted_parameters.kappa > 0.0);
    assert_eq!(
        payload.diagnostics.model_selection_policy,
        "rolling_origin_predictive_sign_test_mean_reversion_evidence_gate.v1"
    );
    assert!(
        payload
            .diagnostics
            .evaluated_candidates
            .iter()
            .any(|candidate| candidate
                .predictive_evidence
                .as_ref()
                .is_some_and(|evidence| {
                    evidence.gate_status == "predictive_evidence_passed.v1"
                        && evidence
                            .relative_loss_reduction
                            .is_some_and(|value| value > 0.0)
                }))
    );
}

#[test]
fn mean_reversion_stage_records_predictive_evidence_alpha_override() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-predictive-evidence-alpha-override",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let mut execution_input =
        direct_stage_input(synthetic_predictive_mean_reverting_observations());
    execution_input.predictive_evidence_alpha = 0.25;

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        execution_input,
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

    let payload = report
        .fitted_stage_payload
        .expect("predictive mean-reversion sample should emit a fitted payload");

    assert!(
        payload
            .diagnostics
            .evaluated_candidates
            .iter()
            .filter_map(|candidate| candidate.predictive_evidence.as_ref())
            .all(|evidence| evidence.significance_level == 0.25)
    );
}

#[test]
fn mean_reversion_stage_selects_random_walk_when_predictive_evidence_fails() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-random-walk-predictive-evidence",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        direct_stage_input(synthetic_random_walk_observations()),
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

    let payload = report
        .fitted_stage_payload
        .expect("random-walk sample should emit a fitted payload");

    assert_eq!(payload.fitted_parameters.kappa, 0.0);
    assert_eq!(
        payload.diagnostics.selected_candidate_family,
        "random_walk_null"
    );
    assert!(
        payload
            .diagnostics
            .evaluated_candidates
            .iter()
            .filter_map(|candidate| candidate.predictive_evidence.as_ref())
            .all(|evidence| evidence.gate_status != "predictive_evidence_passed.v1")
    );
}

#[test]
fn mean_reversion_stage_selects_predictive_grid_across_automatic_candidates() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-multi-grid-predictive-evidence",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let daily_observations = synthetic_predictive_mean_reverting_observations();
    let weekly_observations = synthetic_regular_grid_observations(
        (0..16)
            .map(|observation_index| 0.001 * (observation_index as f64))
            .collect(),
        168,
    );

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        direct_multi_grid_stage_input(vec![
            MeanReversionStageGridCandidate {
                calibration_grid_kind: "synthetic_weekly_boundary_grid.v1".to_string(),
                calibration_grid_interval_hours: 168.0,
                source_observation_count: weekly_observations.len(),
                coverage_ratio: 1.0,
                observations: weekly_observations,
            },
            MeanReversionStageGridCandidate {
                calibration_grid_kind: "synthetic_daily_session_grid.v1".to_string(),
                calibration_grid_interval_hours: 24.0,
                source_observation_count: daily_observations.len(),
                coverage_ratio: 1.0,
                observations: daily_observations,
            },
        ]),
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

    let payload = report
        .fitted_stage_payload
        .expect("multi-grid sample should emit a fitted payload");

    assert!(payload.fitted_parameters.kappa > 0.0);
    assert_eq!(
        payload.diagnostics.calibration_grid.calibration_grid_kind,
        "synthetic_daily_session_grid.v1"
    );
    assert_eq!(payload.diagnostics.evaluated_grids.len(), 2);
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
fn mean_reversion_stage_accepts_sparse_coverage_when_identifiable_structure_is_present() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-derived-sufficiency-accepts-sparse-coverage",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();
    let mut execution_input = direct_stage_input(synthetic_weekly_anchor_observations());

    execution_input.grid_candidates[0].source_observation_count = 3;
    execution_input.grid_candidates[0].coverage_ratio = 3.0 / 7.0;

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        execution_input,
    );

    let payload = report
        .fitted_stage_payload
        .expect("derived sparse sufficiency should still emit a fitted payload");

    assert_ne!(payload.fitted_parameters.kappa, 0.0);
    assert_ne!(
        payload.diagnostics.stage_decision_reason,
        "endogenous_anchor_runtime_projection_required.v1"
    );
}

#[test]
fn mean_reversion_stage_uses_identifiable_candidate_subset_when_full_regression_is_not_supported() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-derived-sufficiency-rejects-unidentified-coverage",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();
    let mut execution_input = direct_stage_input(synthetic_weekly_anchor_observations());

    execution_input.grid_candidates[0].source_observation_count = 2;
    execution_input.grid_candidates[0].coverage_ratio = 2.0 / 7.0;

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        execution_input,
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

    let payload = report
        .fitted_stage_payload
        .expect("partial direct coverage should still emit a fitted payload");

    assert_eq!(payload.fitted_parameters.kappa, 0.0);
    assert_ne!(
        payload.diagnostics.stage_decision_reason,
        "endogenous_anchor_runtime_projection_required.v1"
    );
    assert_eq!(payload.diagnostics.evaluated_candidates.len(), 2);
    assert!(
        payload
            .diagnostics
            .evaluated_candidates
            .iter()
            .any(|candidate| candidate.candidate_family
                == "endogenous_core_pressure_level_no_mean_reversion")
    );
    assert!(
        payload
            .diagnostics
            .evaluated_candidates
            .iter()
            .any(|candidate| candidate.candidate_family == "random_walk_null")
    );
    assert!(
        payload
            .diagnostics
            .evaluated_candidates
            .iter()
            .all(|candidate| candidate.candidate_family
                != "endogenous_core_pressure_level_mean_reversion")
    );
    assert!(
        payload
            .diagnostics
            .evaluated_candidates
            .iter()
            .all(|candidate| candidate.candidate_family != "flat_anchor_mean_reversion")
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

    let no_mean_reversion_candidate = payload
        .diagnostics
        .evaluated_candidates
        .iter()
        .find(|candidate| {
            candidate.candidate_family == "endogenous_core_pressure_level_no_mean_reversion"
        })
        .expect("pressure-only candidate should be evaluated");

    assert_eq!(
        no_mean_reversion_candidate.regression_pair_count,
        synthetic_constant_anchor_observations().len() - 1
    );
    assert_eq!(
        payload.diagnostics.model_selection_policy,
        "rolling_origin_predictive_sign_test_with_gaussian_quasi_likelihood_bic_thin_sample_fallback.v1"
    );
    assert!(
        payload
            .diagnostics
            .evaluated_candidates
            .iter()
            .all(|candidate| (0.0..=1.0).contains(&candidate.anchor_model.pressure_level_gain))
    );
}

#[test]
fn mean_reversion_stage_rejects_full_endogenous_family_when_bic_sample_support_is_too_thin() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-bic-sample-support-gate",
        CalibrationExecutionMode::Memory,
        None,
    );

    let calendar_registry = calibration_stage_test_registry();

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        direct_stage_input(synthetic_partial_weekly_anchor_observations()),
    );

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);

    let payload = report
        .fitted_stage_payload
        .expect("thin-support sample should still emit a fitted payload");

    assert!(
        payload
            .diagnostics
            .evaluated_candidates
            .iter()
            .any(|candidate| candidate.candidate_family
                == "endogenous_core_pressure_level_mean_reversion")
    );
    assert_ne!(
        payload.diagnostics.selected_candidate_family,
        "endogenous_core_pressure_level_mean_reversion"
    );
    assert!(payload.diagnostics.stage_decision_reason.contains(
        "predictive_evidence_support_too_thin_gaussian_quasi_likelihood_bic_fallback.v1"
    ));
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

    mean_reversion_input.grid_candidates[0].source_observation_count = 4;
    mean_reversion_input.grid_candidates[0].coverage_ratio = 1.0;
    mean_reversion_input.grid_candidates[0].observations = synthetic_constant_anchor_observations();

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

    mean_reversion_input.grid_candidates[0].source_observation_count = 4;
    mean_reversion_input.grid_candidates[0].coverage_ratio = 1.0;
    mean_reversion_input.grid_candidates[0].observations = synthetic_constant_anchor_observations();

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
    assert_ne!(
        payload.diagnostics.stage_decision_reason,
        "endogenous_anchor_runtime_projection_required.v1"
    );
    assert!(payload.diagnostics.phi_reference.is_finite());
    assert!(payload.diagnostics.kappa.is_finite());
    assert!(payload.fitted_parameters.kappa.is_finite());
}
