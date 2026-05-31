use serde::Serialize;

use janiform_calibration_artifacts::{
    CALIBRATION_RETURN_BACKFIT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_RETURN_BACKFIT_STAGE_HANDOFF_POLICY,
    CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    CalibrationStageSelectedCandidateSource, ReturnBackfitStageDiagnostics,
    ReturnBackfitStageVarianceBudgetDiagnostic, stage_solver_settings_for_stage,
};

use crate::stages::long_variance::LongVarianceStageFittedPayload;
use crate::stages::news_shocks::NewsShocksStageFittedPayload;
use crate::stages::return_components::ReturnComponentsStageFittedPayload;
use crate::stages::seasonality::CalibrationStageExecutionStatus;

const RETURN_BACKFIT_ESTIMATION_POLICY: &str =
    "variance_budget_backfit_no_op_candidate_baseline.v1";
const RETURN_BACKFIT_NO_OP_CANDIDATE_ID: &str = "no_op";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReturnBackfitStageExecutionInput {
    pub return_components_input: Option<CalibrationStageSelectedCandidateSource>,
    pub return_components_stage_payload: Option<ReturnComponentsStageFittedPayload>,
    pub long_variance_stage_payload: Option<LongVarianceStageFittedPayload>,
    pub news_shocks_stage_payload: Option<NewsShocksStageFittedPayload>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnBackfitStageFittedPayload {
    pub schema_version: &'static str,
    pub selected_candidate_id: String,
    pub effective_return_components_payload: ReturnComponentsStageFittedPayload,
    pub diagnostics: ReturnBackfitStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_selected_candidate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnBackfitStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub candidate_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum ReturnBackfitStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingReturnComponentsInput,
    MissingReturnComponentsStagePayload,
    MissingLongVarianceStagePayload,
    MissingNewsShocksStagePayload,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnBackfitStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<ReturnBackfitStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ReturnBackfitStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<ReturnBackfitStageExecutionFailure>,
}

pub fn run_return_backfit_calibration_stage(
    run_spec: &CalibrationRunSpec,
    execution_input: ReturnBackfitStageExecutionInput,
) -> ReturnBackfitStageExecutionReport {
    if let Err(error_message) = run_spec.validate() {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::InvalidRunSpec {
                error_message: error_message.to_string(),
            },
        );
    }

    if !run_spec
        .selected_stages
        .contains(&CalibrationStage::ReturnBackfit)
    {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::ReturnBackfit)
    else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    let Some(return_components_input) = execution_input.return_components_input else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingReturnComponentsInput,
        );
    };

    let Some(return_components_stage_payload) = execution_input.return_components_stage_payload
    else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingReturnComponentsStagePayload,
        );
    };

    let Some(long_variance_stage_payload) = execution_input.long_variance_stage_payload else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingLongVarianceStagePayload,
        );
    };

    let Some(news_shocks_stage_payload) = execution_input.news_shocks_stage_payload else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingNewsShocksStagePayload,
        );
    };

    let fitted_stage_payload = build_return_backfit_fitted_stage_payload(
        return_components_input,
        return_components_stage_payload,
        long_variance_stage_payload,
        news_shocks_stage_payload,
    );

    let seed_metadata = janiform_calibration_artifacts::resolve_calibration_stage_seed_metadata(
        run_spec,
        CalibrationStage::ReturnBackfit,
    );

    ReturnBackfitStageExecutionReport {
        schema_version: CALIBRATION_RETURN_BACKFIT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ReturnBackfit,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_RETURN_BACKFIT_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(ReturnBackfitStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: seed_metadata.run_global_seed,
            stage_seed: seed_metadata.stage_seed,
            stage_seed_source: seed_metadata.stage_seed_source,
            stage_seed_derivation_policy:
                janiform_calibration_artifacts::CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings,
            candidate_count: 1,
        }),
        failure: None,
    }
}

fn build_return_backfit_fitted_stage_payload(
    return_components_input: CalibrationStageSelectedCandidateSource,
    mut effective_return_components_payload: ReturnComponentsStageFittedPayload,
    long_variance_stage_payload: LongVarianceStageFittedPayload,
    news_shocks_stage_payload: NewsShocksStageFittedPayload,
) -> ReturnBackfitStageFittedPayload {
    effective_return_components_payload
        .fitted_parameters
        .long_variance_parameters = long_variance_stage_payload.fitted_parameters;

    effective_return_components_payload
        .fitted_parameters
        .news_process_configuration = news_shocks_stage_payload.news_process_configuration;

    effective_return_components_payload
        .fitted_parameters
        .surprise_deterministic_intensity_profile =
        news_shocks_stage_payload.surprise_deterministic_intensity_profile;

    let return_components_input_transport = return_components_input.transport();

    let (return_components_selected_candidate_id, return_components_artifact_reference) =
        match return_components_input {
            CalibrationStageSelectedCandidateSource::InMemoryOutput {
                selected_candidate_id,
            } => (Some(selected_candidate_id), None),
            CalibrationStageSelectedCandidateSource::PersistedArtifactReference {
                artifact_reference,
            } => (None, Some(artifact_reference)),
        };

    ReturnBackfitStageFittedPayload {
        schema_version: CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: RETURN_BACKFIT_NO_OP_CANDIDATE_ID.to_string(),
        effective_return_components_payload,
        diagnostics: ReturnBackfitStageDiagnostics {
            estimation_policy: RETURN_BACKFIT_ESTIMATION_POLICY.to_string(),
            selected_candidate_id: RETURN_BACKFIT_NO_OP_CANDIDATE_ID.to_string(),
            no_op_selected: true,
            candidate_count: 1,
            variance_budget: ReturnBackfitStageVarianceBudgetDiagnostic {
                initial_nig_contribution: None,
                fixed_long_variance_news_contribution: None,
                backfitted_nig_contribution: None,
                final_total_modeled_return_scale: None,
                observed_target_scale: None,
            },
            objective_blocks: Vec::new(),
            warnings: Vec::new(),
        },
        return_components_input_transport,
        return_components_selected_candidate_id,
        return_components_artifact_reference,
    }
}

fn failure_report(
    run_spec: &CalibrationRunSpec,
    failure: ReturnBackfitStageExecutionFailure,
) -> ReturnBackfitStageExecutionReport {
    ReturnBackfitStageExecutionReport {
        schema_version: CALIBRATION_RETURN_BACKFIT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ReturnBackfit,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_RETURN_BACKFIT_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}
