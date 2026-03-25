mod support;

use janiform_calibration_artifacts::{CalibrationExecutionMode, CalibrationStage};
use janiform_calibration_runtime::{
    CalibrationMemoryCheckpointStore, CalibrationStageExecutionStatus,
    CalibrationStagedRunStageReport, MeanReversionStageExecutionInput,
    MeanReversionStageObservation, MeanReversionStageReturnComponentsInput,
    run_mean_reversion_calibration_stage, run_staged_calibration_with_checkpointing,
};
use janiform_model_kernels::mean_reversion::MeanReversionAnchorModelKind;
use support::shared::{
    build_run_contract_with_mode_and_resume, calibration_stage_test_registry, staged_run_inputs,
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
    requested_anchor_model_kind: MeanReversionAnchorModelKind,
    observations: Vec<MeanReversionStageObservation>,
) -> MeanReversionStageExecutionInput {
    MeanReversionStageExecutionInput {
        return_components_input: Some(MeanReversionStageReturnComponentsInput::InMemoryOutput {
            selected_candidate_id: "return_candidate_balanced".to_string(),
        }),
        requested_anchor_model_kind,
        calibration_grid_kind: "synthetic_daily_close_grid.v1".to_string(),
        calibration_grid_interval_hours: 24.0,
        source_observation_count: observations.len(),
        coverage_ratio: 1.0,
        observations,
        disabled_stage_reason: None,
    }
}

#[test]
fn mean_reversion_stage_emits_disabled_fit_when_requested() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-disabled",
        CalibrationExecutionMode::Memory,
        None,
    );
    let calendar_registry = calibration_stage_test_registry();
    let execution_input = MeanReversionStageExecutionInput {
        requested_anchor_model_kind: MeanReversionAnchorModelKind::Disabled,
        disabled_stage_reason: Some("requested_by_contract_test.v1".to_string()),
        ..direct_stage_input(
            MeanReversionAnchorModelKind::Disabled,
            synthetic_constant_anchor_observations(),
        )
    };

    let report =
        run_mean_reversion_calibration_stage(&run_contract.run_spec, &calendar_registry, execution_input);

    assert_eq!(report.status, CalibrationStageExecutionStatus::Success);
    let payload = report
        .fitted_stage_payload
        .expect("disabled stage should still emit a fitted payload");
    assert_eq!(payload.fitted_parameters.kappa, 0.0);
    assert_eq!(
        payload.diagnostics.chosen_anchor_family,
        MeanReversionAnchorModelKind::Disabled
    );
    assert_eq!(
        payload.diagnostics.stage_decision_reason,
        "requested_by_contract_test.v1"
    );
}

#[test]
fn mean_reversion_stage_recovers_zero_kappa_for_flat_constant_anchor_levels() {
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
        direct_stage_input(MeanReversionAnchorModelKind::Constant, observations),
    );

    let payload = report
        .fitted_stage_payload
        .expect("flat levels should emit a fitted payload");
    assert_eq!(payload.fitted_parameters.kappa, 0.0);
    assert_eq!(payload.diagnostics.phi, 1.0);
    assert_eq!(
        payload.diagnostics.anchor_parameter_estimates.anchor_value,
        Some(4.0)
    );
}

#[test]
fn mean_reversion_stage_recovers_constant_anchor_decay_from_synthetic_levels() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-mean-reversion-constant-anchor",
        CalibrationExecutionMode::Memory,
        None,
    );
    let calendar_registry = calibration_stage_test_registry();

    let report = run_mean_reversion_calibration_stage(
        &run_contract.run_spec,
        &calendar_registry,
        direct_stage_input(
            MeanReversionAnchorModelKind::Constant,
            synthetic_constant_anchor_observations(),
        ),
    );

    let payload = report
        .fitted_stage_payload
        .expect("constant-anchor fit should emit a fitted payload");
    let expected_phi: f64 = 0.5;
    let expected_kappa = -expected_phi.ln() / 24.0;

    assert_eq!(
        payload.diagnostics.chosen_anchor_family,
        MeanReversionAnchorModelKind::Constant
    );
    assert!((payload.diagnostics.phi - expected_phi).abs() < 1.0e-12);
    assert!((payload.fitted_parameters.kappa - expected_kappa).abs() < 1.0e-12);
    assert_eq!(
        payload.diagnostics.anchor_parameter_estimates.anchor_value,
        Some(4.0)
    );
}

#[test]
fn staged_runner_end_to_end_reports_mean_reversion_fit_from_synthetic_levels() {
    let run_contract = build_run_contract_with_mode_and_resume(
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
    mean_reversion_input.requested_anchor_model_kind = MeanReversionAnchorModelKind::Constant;
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
    assert!((payload.diagnostics.phi - 0.5).abs() < 1.0e-12);
    assert!((payload.fitted_parameters.kappa - (-0.5_f64.ln() / 24.0)).abs() < 1.0e-12);
}
