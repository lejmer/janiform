use super::{
    CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY, CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
    CalibrationDiagnosticRuntimeProfileEntry, CalibrationRunContract, CalibrationStage,
    CalibrationStageCheckpointMetadata, CalibrationStageExecutionStatus,
    CalibrationStagedRunFailure, CalibrationStagedRunReport, CalibrationStagedRunStageReport,
};

pub(super) fn build_staged_run_failure_report(
    run_contract: &CalibrationRunContract,
    stage_reports: Vec<CalibrationStagedRunStageReport>,
    emitted_checkpoints: Vec<CalibrationStageCheckpointMetadata>,
    resumed_from_checkpoint_reference: Option<String>,
    resumed_from_last_successful_stage: Option<CalibrationStage>,
    started_from_stage: Option<CalibrationStage>,
    failure: CalibrationStagedRunFailure,
) -> CalibrationStagedRunReport {
    CalibrationStagedRunReport {
        schema_version: CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
        failure_triage_policy: CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
        run_id: run_contract.run_spec.run_id.clone(),
        execution_mode: run_contract.run_spec.execution_mode,
        status: CalibrationStageExecutionStatus::Failure,
        stage_reports,
        emitted_checkpoints,
        diagnostic_artifact_references: Vec::new(),
        diagnostic_artifact_bundle: None,
        resumed_from_checkpoint_reference,
        resumed_from_last_successful_stage,
        started_from_stage,
        failure: Some(failure),
    }
}

pub(super) fn diagnostic_runtime_profile_entry_for_stage_report(
    stage_report: &CalibrationStagedRunStageReport,
    elapsed_seconds: f64,
) -> CalibrationDiagnosticRuntimeProfileEntry {
    match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => {
            CalibrationDiagnosticRuntimeProfileEntry {
                component_id: "stage.seasonality".to_string(),
                elapsed_seconds,
                evaluation_count: report
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.accepted_record_count)
                    .unwrap_or(0),
            }
        }
        CalibrationStagedRunStageReport::Hawkes(report) => {
            CalibrationDiagnosticRuntimeProfileEntry {
                component_id: "stage.hawkes".to_string(),
                elapsed_seconds,
                evaluation_count: report
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.input_candidate_count)
                    .unwrap_or(0),
            }
        }
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            CalibrationDiagnosticRuntimeProfileEntry {
                component_id: "stage.return_components".to_string(),
                elapsed_seconds,
                evaluation_count: report
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.input_candidate_count)
                    .unwrap_or(0),
            }
        }
        CalibrationStagedRunStageReport::MeanReversion(report) => {
            CalibrationDiagnosticRuntimeProfileEntry {
                component_id: "stage.mean_reversion".to_string(),
                elapsed_seconds,
                evaluation_count: report
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.retained_observation_count)
                    .unwrap_or(0),
            }
        }
        CalibrationStagedRunStageReport::JointRefinement(report) => {
            CalibrationDiagnosticRuntimeProfileEntry {
                component_id: "stage.joint_refinement".to_string(),
                elapsed_seconds,
                evaluation_count: report
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.input_candidate_count)
                    .unwrap_or(0),
            }
        }
    }
}

pub(super) fn retained_stage_report(
    stage_report: &CalibrationStagedRunStageReport,
) -> CalibrationStagedRunStageReport {
    match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => {
            let normalized_stage_payload = report
                .normalized_stage_payload
                .as_ref()
                .map(|payload| payload.without_event_level_vectors());

            CalibrationStagedRunStageReport::Seasonality(
                crate::stages::seasonality::SeasonalityStageExecutionReport {
                    schema_version: report.schema_version,
                    stage: report.stage,
                    status: report.status,
                    run_id: report.run_id.clone(),
                    execution_mode: report.execution_mode,
                    handoff_policy: report.handoff_policy,
                    handoffs: report.handoffs.clone(),
                    normalized_stage_payload,
                    metadata: report.metadata.clone(),
                    failure: report.failure.clone(),
                },
            )
        }
        _ => stage_report.clone(),
    }
}
