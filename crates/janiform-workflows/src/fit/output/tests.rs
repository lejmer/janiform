use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use janiform_calibration_artifacts::{
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION, CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY,
    CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_SPEC_SCHEMA_VERSION,
    CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY, CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION,
    CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION, CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
    CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY, CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationStage,
    CalibrationStageCheckpointMetadata, CalibrationStageSeedSource,
    calibration_stage_checkpoint_reference,
};
use janiform_calibration_runtime::{CalibrationStageExecutionStatus, CalibrationStagedRunReport};
use janiform_terminal_ui::CompletionSummary;
use serde_json::json;

use super::FitWorkflowPaths;
use super::success_summary;

#[test]
fn success_summary_reports_latest_emitted_resume_checkpoint() {
    let run_id = "test-run";

    let resumed_from_checkpoint_reference =
        calibration_stage_checkpoint_reference(run_id, CalibrationStage::MeanReversion);
    let latest_checkpoint_reference =
        calibration_stage_checkpoint_reference(run_id, CalibrationStage::JointRefinement);

    let report = successful_report(
        run_id,
        Some(resumed_from_checkpoint_reference),
        vec![
            checkpoint_metadata(run_id, CalibrationStage::MeanReversion),
            checkpoint_metadata(run_id, CalibrationStage::JointRefinement),
        ],
    );

    let report_payload = json!({});
    let summary = success_summary(
        &report,
        &report_payload,
        &test_paths(),
        Path::new("events.jsonl"),
        Duration::from_secs(1),
    );

    assert_eq!(
        summary_row_value(&summary, "Resume"),
        latest_checkpoint_reference
    );
}

fn successful_report(
    run_id: &str,
    resumed_from_checkpoint_reference: Option<String>,
    emitted_checkpoints: Vec<CalibrationStageCheckpointMetadata>,
) -> CalibrationStagedRunReport {
    CalibrationStagedRunReport {
        schema_version: CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
        failure_triage_policy: CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
        run_id: run_id.to_string(),
        execution_mode: CalibrationExecutionMode::Disk,
        status: CalibrationStageExecutionStatus::Success,
        stage_reports: Vec::new(),
        emitted_checkpoints,
        diagnostic_artifact_references: Vec::new(),
        diagnostic_artifact_bundle: None,
        resumed_from_checkpoint_reference,
        resumed_from_last_successful_stage: Some(CalibrationStage::MeanReversion),
        started_from_stage: Some(CalibrationStage::JointRefinement),
        failure: None,
    }
}

fn checkpoint_metadata(
    run_id: &str,
    completed_stage: CalibrationStage,
) -> CalibrationStageCheckpointMetadata {
    CalibrationStageCheckpointMetadata {
        schema_version: CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION.to_string(),
        checkpoint_reference_policy: CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY.to_string(),
        disk_write_policy: CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY.to_string(),
        artifact_manifest_schema_version: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
        checkpoint_reference: calibration_stage_checkpoint_reference(run_id, completed_stage),
        run_id: run_id.to_string(),
        execution_mode: CalibrationExecutionMode::Disk,
        run_contract_schema_version: CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION.to_string(),
        run_spec_schema_version: CALIBRATION_RUN_SPEC_SCHEMA_VERSION.to_string(),
        stage_graph_schema_version: CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION.to_string(),
        execution_stage_order: vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
            CalibrationStage::LongVariance,
            CalibrationStage::NewsShocks,
            CalibrationStage::MeanReversion,
            CalibrationStage::JointRefinement,
        ],
        completed_stage,
        completed_stage_position: completed_stage_position(completed_stage),
        run_global_seed: 17,
        completed_stage_output_schema_version: "test.stage_output.v1".to_string(),
        completed_stage_seed: 23,
        completed_stage_seed_source: CalibrationStageSeedSource::DerivedFromGlobalSeed,
        stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY.to_string(),
        completed_stage_solver_settings: test_optimizer_settings(),
    }
}

fn completed_stage_position(stage: CalibrationStage) -> usize {
    match stage {
        CalibrationStage::Seasonality => 0,
        CalibrationStage::Hawkes => 1,
        CalibrationStage::ReturnComponents => 2,
        CalibrationStage::LongVariance => 3,
        CalibrationStage::NewsShocks => 4,
        CalibrationStage::MeanReversion => 5,
        CalibrationStage::JointRefinement => 6,
        CalibrationStage::OptionRefinement => 7,
        CalibrationStage::Finalization => 8,
    }
}

fn test_optimizer_settings() -> CalibrationOptimizerSettings {
    CalibrationOptimizerSettings::try_new("test-solver", 1, 1, 1.0, 1.0, 0.0)
        .expect("test optimizer settings should be valid")
}

fn test_paths() -> FitWorkflowPaths {
    FitWorkflowPaths {
        output_directory: PathBuf::from("output"),
        model_path: PathBuf::from("model.jf"),
        report_path: PathBuf::from("report.json"),
        checkpoint_directory: PathBuf::from("checkpoints"),
    }
}

fn summary_row_value<'summary>(summary: &'summary CompletionSummary, label: &str) -> &'summary str {
    summary
        .rows
        .iter()
        .find(|row| row.label == label)
        .map(|row| row.value.as_str())
        .expect("summary row should be present")
}
