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

pub(super) fn build_staged_run_cancellation_report(
    run_contract: &CalibrationRunContract,
    stage_reports: Vec<CalibrationStagedRunStageReport>,
    emitted_checkpoints: Vec<CalibrationStageCheckpointMetadata>,
    resumed_from_checkpoint_reference: Option<String>,
    resumed_from_last_successful_stage: Option<CalibrationStage>,
    started_from_stage: Option<CalibrationStage>,
) -> CalibrationStagedRunReport {
    CalibrationStagedRunReport {
        schema_version: CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
        failure_triage_policy: CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
        run_id: run_contract.run_spec.run_id.clone(),
        execution_mode: run_contract.run_spec.execution_mode,
        status: CalibrationStageExecutionStatus::Cancelled,
        stage_reports,
        emitted_checkpoints,
        diagnostic_artifact_references: Vec::new(),
        diagnostic_artifact_bundle: None,
        resumed_from_checkpoint_reference,
        resumed_from_last_successful_stage,
        started_from_stage,
        failure: None,
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
        CalibrationStagedRunStageReport::ObservationModels(report) => {
            CalibrationDiagnosticRuntimeProfileEntry {
                component_id: "stage.observation_models".to_string(),
                elapsed_seconds,
                evaluation_count: report
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.observed_open_session_return_count)
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
        CalibrationStagedRunStageReport::LongVariance(report) => {
            CalibrationDiagnosticRuntimeProfileEntry {
                component_id: "stage.long_variance".to_string(),
                elapsed_seconds,
                evaluation_count: report
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.horizon_target_count)
                    .unwrap_or(0),
            }
        }
        CalibrationStagedRunStageReport::NewsShocks(report) => {
            CalibrationDiagnosticRuntimeProfileEntry {
                component_id: "stage.news_shocks".to_string(),
                elapsed_seconds,
                evaluation_count: report
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.fitted_news_parameter_count)
                    .unwrap_or(0),
            }
        }
        CalibrationStagedRunStageReport::ReturnBackfit(report) => {
            CalibrationDiagnosticRuntimeProfileEntry {
                component_id: "stage.return_backfit".to_string(),
                elapsed_seconds,
                evaluation_count: usize::from(report.metadata.is_some()),
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
        CalibrationStagedRunStageReport::OptionRefinement(report) => {
            CalibrationDiagnosticRuntimeProfileEntry {
                component_id: "stage.option_refinement".to_string(),
                elapsed_seconds,
                evaluation_count: report
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.input_candidate_count)
                    .unwrap_or(0),
            }
        }
        CalibrationStagedRunStageReport::Finalization(report) => {
            CalibrationDiagnosticRuntimeProfileEntry {
                component_id: "stage.finalization".to_string(),
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

            CalibrationStagedRunStageReport::Seasonality(Box::new(
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
            ))
        }
        CalibrationStagedRunStageReport::Hawkes(report) => {
            let fitted_stage_payload = report
                .fitted_stage_payload
                .as_ref()
                .map(|payload| payload.without_candidate_diagnostics());
            let failure = report
                .failure
                .as_ref()
                .map(|failure| failure.without_candidate_diagnostics());

            CalibrationStagedRunStageReport::Hawkes(Box::new(
                crate::stages::hawkes::HawkesStageExecutionReport {
                    schema_version: report.schema_version,
                    stage: report.stage,
                    status: report.status,
                    run_id: report.run_id.clone(),
                    execution_mode: report.execution_mode,
                    handoff_policy: report.handoff_policy,
                    handoffs: report.handoffs.clone(),
                    fitted_stage_payload,
                    metadata: report.metadata.clone(),
                    failure,
                },
            ))
        }
        CalibrationStagedRunStageReport::ObservationModels(report) => {
            let fitted_stage_payload = report
                .fitted_stage_payload
                .as_ref()
                .map(|payload| payload.without_source_activity_vector());

            CalibrationStagedRunStageReport::ObservationModels(Box::new(
                crate::stages::observation_models::ObservationModelsStageExecutionReport {
                    schema_version: report.schema_version,
                    stage: report.stage,
                    status: report.status,
                    run_id: report.run_id.clone(),
                    execution_mode: report.execution_mode,
                    handoff_policy: report.handoff_policy,
                    fitted_stage_payload,
                    metadata: report.metadata.clone(),
                    failure: report.failure.clone(),
                },
            ))
        }
        CalibrationStagedRunStageReport::JointRefinement(report) => {
            let mut retained_report = (**report).clone();
            retained_report.runtime_handoff = None;
            CalibrationStagedRunStageReport::JointRefinement(Box::new(retained_report))
        }
        CalibrationStagedRunStageReport::OptionRefinement(report) => {
            let mut retained_report = (**report).clone();
            retained_report.runtime_handoff = None;
            CalibrationStagedRunStageReport::OptionRefinement(Box::new(retained_report))
        }
        CalibrationStagedRunStageReport::Finalization(report) => {
            let mut retained_report = (**report).clone();
            retained_report.runtime_handoff = None;
            CalibrationStagedRunStageReport::Finalization(Box::new(retained_report))
        }
        CalibrationStagedRunStageReport::ReturnComponents(_)
        | CalibrationStagedRunStageReport::LongVariance(_)
        | CalibrationStagedRunStageReport::NewsShocks(_)
        | CalibrationStagedRunStageReport::ReturnBackfit(_)
        | CalibrationStagedRunStageReport::MeanReversion(_) => stage_report.clone(),
    }
}
