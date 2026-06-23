mod support;
use std::path::Path;

use janiform_calibration_artifacts::{CalibrationExecutionMode, CalibrationRunContract};
use janiform_calibration_runtime::{
    CalibrationMemoryCheckpointStore, CalibrationStagedRunInputs, CalibrationStagedRunReport,
    run_staged_calibration_with_checkpointing as run_staged_calibration_with_checkpointing_core,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_market_time::calendar::ExchangeCalendarRegistry;
use support::shared::{
    build_run_contract_with_mode_and_resume, calibration_stage_test_registry, staged_run_inputs,
};

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

#[test]
fn joint_refinement_diagnostics_bundle_exposes_real_observed_simulated_metric_pairs() {
    let run_contract = build_run_contract_with_mode_and_resume(
        "run-joint-refinement-diagnostics",
        CalibrationExecutionMode::Memory,
        None,
    );
    let calendar_registry = calibration_stage_test_registry();

    let report = run_staged_calibration_with_checkpointing(
        &run_contract,
        &calendar_registry,
        staged_run_inputs(true, true, true),
        None,
        None,
    );

    assert!(report.status.is_successful(), "{report:#?}");

    let diagnostic_bundle = report
        .diagnostic_artifact_bundle
        .expect("memory-mode staged run should include a diagnostic artifact bundle");
    let selected_candidate_id = diagnostic_bundle
        .selection_rationale
        .decision
        .selected_candidate_id
        .clone();
    let selected_restart_id = selected_candidate_id
        .strip_prefix("finalization_")
        .or_else(|| selected_candidate_id.strip_prefix("joint_refinement_"))
        .unwrap_or(selected_candidate_id.as_str());
    let selected_joint_candidate_id = format!("joint_refinement_{selected_restart_id}");
    let selected_joint_candidate = diagnostic_bundle
        .candidate_frontier
        .frontier_entries
        .iter()
        .find(|entry| {
            entry.stage == "joint_refinement" && entry.candidate_id == selected_joint_candidate_id
        })
        .expect("selected joint-refinement frontier entry should be present");

    assert!(
        selected_joint_candidate.family_metrics.len() >= 6,
        "selected joint-refinement candidate should expose multiple diagnostic terms"
    );

    let activity_level_metric = selected_joint_candidate
        .family_metrics
        .iter()
        .find(|metric| metric.metric_id == "activity_level_timing.total_count")
        .expect("joint activity-level metric should be emitted");
    assert_eq!(activity_level_metric.target_family, "activity_level_timing");
    assert!(activity_level_metric.observed_value > 0.0);
    assert!(activity_level_metric.simulated_value > 0.0);
    assert!(activity_level_metric.uncertainty_scale > 0.0);
    assert_ne!(
        activity_level_metric.simulated_value,
        activity_level_metric.normalized_distance
    );

    let return_scale_metric = selected_joint_candidate
        .family_metrics
        .iter()
        .find(|metric| metric.metric_id == "return_scale.1d.realized_variance")
        .expect("joint return-scale metric should be emitted");
    assert_eq!(return_scale_metric.target_family, "return_scale");
    assert!(return_scale_metric.observed_value > 0.0);
    assert!(return_scale_metric.simulated_value > 0.0);
    assert!(return_scale_metric.uncertainty_scale > 0.0);
    assert_ne!(
        return_scale_metric.simulated_value,
        return_scale_metric.normalized_distance
    );

    let rv_activity_metric = selected_joint_candidate
        .family_metrics
        .iter()
        .find(|metric| metric.metric_id == "rv_activity_coupling.correlation")
        .expect("joint rv-activity coupling metric should be emitted");
    assert_eq!(rv_activity_metric.target_family, "rv_activity_coupling");
    assert!(rv_activity_metric.uncertainty_scale > 0.0);
    assert!(
        rv_activity_metric.observed_value != 0.0
            || rv_activity_metric.simulated_value != rv_activity_metric.normalized_distance
            || rv_activity_metric.uncertainty_scale != 1.0
    );
}
