use serde::Serialize;

use janiform_calibration_artifacts::{
    CALIBRATION_NEWS_SHOCKS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_NEWS_SHOCKS_STAGE_HANDOFF_POLICY,
    CALIBRATION_NEWS_SHOCKS_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    CalibrationStageSelectedCandidateSource, NewsShocksStageDiagnostics,
    SurpriseNewsDeterministicIntensityProfile, stage_solver_settings_for_stage,
};
use janiform_model_kernels::news::NewsProcessConfiguration;

use crate::stages::return_components::ReturnComponentsStageFittedPayload;
use crate::stages::seasonality::CalibrationStageExecutionStatus;

const NEWS_SHOCKS_ESTIMATION_POLICY: &str =
    "joint_residual_activity_surprise_gap_estimator_with_coupled_activity_return_marks.v1";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewsShocksStageExecutionInput {
    pub return_components_input: Option<CalibrationStageSelectedCandidateSource>,
    pub return_components_stage_payload: Option<ReturnComponentsStageFittedPayload>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewsShocksStageFittedPayload {
    pub schema_version: &'static str,
    pub news_process_configuration: NewsProcessConfiguration,
    pub surprise_deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile,
    pub diagnostics: NewsShocksStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_selected_candidate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewsShocksStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub fitted_news_parameter_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum NewsShocksStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingReturnComponentsInput,
    MissingReturnComponentsStagePayload,
    InvalidNewsProcessConfiguration { error_message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewsShocksStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<NewsShocksStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<NewsShocksStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<NewsShocksStageExecutionFailure>,
}

pub fn run_news_shocks_calibration_stage(
    run_spec: &CalibrationRunSpec,
    execution_input: NewsShocksStageExecutionInput,
) -> NewsShocksStageExecutionReport {
    if let Err(error_message) = run_spec.validate() {
        return failure_report(
            run_spec,
            NewsShocksStageExecutionFailure::InvalidRunSpec {
                error_message: error_message.to_string(),
            },
        );
    }

    if !run_spec
        .selected_stages
        .contains(&CalibrationStage::NewsShocks)
    {
        return failure_report(
            run_spec,
            NewsShocksStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::NewsShocks)
    else {
        return failure_report(
            run_spec,
            NewsShocksStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    let Some(return_components_input) = execution_input.return_components_input else {
        return failure_report(
            run_spec,
            NewsShocksStageExecutionFailure::MissingReturnComponentsInput,
        );
    };

    let Some(return_components_stage_payload) = execution_input.return_components_stage_payload
    else {
        return failure_report(
            run_spec,
            NewsShocksStageExecutionFailure::MissingReturnComponentsStagePayload,
        );
    };

    let fitted_stage_payload = match build_news_shocks_fitted_stage_payload(
        return_components_input,
        &return_components_stage_payload,
    ) {
        Ok(fitted_stage_payload) => fitted_stage_payload,
        Err(failure) => return failure_report(run_spec, failure),
    };

    let seed_metadata = janiform_calibration_artifacts::resolve_calibration_stage_seed_metadata(
        run_spec,
        CalibrationStage::NewsShocks,
    );

    NewsShocksStageExecutionReport {
        schema_version: CALIBRATION_NEWS_SHOCKS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::NewsShocks,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_NEWS_SHOCKS_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(NewsShocksStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_NEWS_SHOCKS_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: seed_metadata.run_global_seed,
            stage_seed: seed_metadata.stage_seed,
            stage_seed_source: seed_metadata.stage_seed_source,
            stage_seed_derivation_policy:
                janiform_calibration_artifacts::CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings,
            fitted_news_parameter_count: 8,
        }),
        failure: None,
    }
}

fn build_news_shocks_fitted_stage_payload(
    return_components_input: CalibrationStageSelectedCandidateSource,
    return_components_stage_payload: &ReturnComponentsStageFittedPayload,
) -> Result<NewsShocksStageFittedPayload, NewsShocksStageExecutionFailure> {
    return_components_stage_payload
        .fitted_parameters
        .news_process_configuration
        .validate()
        .map_err(
            |source| NewsShocksStageExecutionFailure::InvalidNewsProcessConfiguration {
                error_message: source.to_string(),
            },
        )?;

    let diagnostics = news_shocks_stage_diagnostics(
        selected_candidate_id(&return_components_input, return_components_stage_payload),
        &return_components_stage_payload
            .fitted_parameters
            .news_process_configuration,
    )?;

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

    Ok(NewsShocksStageFittedPayload {
        schema_version: CALIBRATION_NEWS_SHOCKS_STAGE_OUTPUT_SCHEMA_VERSION,
        // Intentional copy because the stage output owns the physical news fit.
        news_process_configuration: return_components_stage_payload
            .fitted_parameters
            .news_process_configuration
            .clone(),
        // Intentional copy because downstream interval-factor-grid materialization owns the profile.
        surprise_deterministic_intensity_profile: return_components_stage_payload
            .fitted_parameters
            .surprise_deterministic_intensity_profile
            .clone(),
        diagnostics,
        return_components_input_transport,
        return_components_selected_candidate_id,
        return_components_artifact_reference,
    })
}

fn selected_candidate_id<'a>(
    return_components_input: &'a CalibrationStageSelectedCandidateSource,
    return_components_stage_payload: &'a ReturnComponentsStageFittedPayload,
) -> &'a str {
    return_components_input.selected_candidate_id().unwrap_or(
        return_components_stage_payload
            .selected_candidate_id
            .as_str(),
    )
}

fn news_shocks_stage_diagnostics(
    selected_candidate_id: &str,
    news_process_configuration: &NewsProcessConfiguration,
) -> Result<NewsShocksStageDiagnostics, NewsShocksStageExecutionFailure> {
    let shared_mark_law = news_process_configuration.shared_mark_law;
    let severity_configuration = shared_mark_law.gamma_severity_marks;

    let expected_severity = severity_configuration
        .expected_severity_mark()
        .map_err(
            |source| NewsShocksStageExecutionFailure::InvalidNewsProcessConfiguration {
                error_message: source.to_string(),
            },
        )?;

    let expected_sqrt_severity = severity_configuration
        .expected_square_root_severity_mark()
        .map_err(
            |source| NewsShocksStageExecutionFailure::InvalidNewsProcessConfiguration {
                error_message: source.to_string(),
            },
        )?;

    Ok(NewsShocksStageDiagnostics {
        estimation_policy: NEWS_SHOCKS_ESTIMATION_POLICY.to_string(),
        selected_candidate_id: selected_candidate_id.to_string(),
        surprise_open_intensity: news_process_configuration
            .arrivals
            .surprise
            .open_baseline_intensity,
        surprise_closed_intensity: news_process_configuration
            .arrivals
            .surprise
            .closed_baseline_intensity,
        severity_shape: severity_configuration.shape_k,
        expected_severity,
        expected_sqrt_severity,
        known_activity_coupling_scale: news_process_configuration
            .known_component_mark_scales
            .activity_coupling_scale,
        surprise_activity_coupling_scale: news_process_configuration
            .surprise_component_mark_scales
            .activity_coupling_scale,
        known_return_jump_drift: news_process_configuration
            .known_component_mark_scales
            .return_jump_drift,
        known_return_jump_volatility: news_process_configuration
            .known_component_mark_scales
            .return_jump_volatility,
        surprise_return_jump_drift: news_process_configuration
            .surprise_component_mark_scales
            .return_jump_drift,
        surprise_return_jump_volatility: news_process_configuration
            .surprise_component_mark_scales
            .return_jump_volatility,
    })
}

fn failure_report(
    run_spec: &CalibrationRunSpec,
    failure: NewsShocksStageExecutionFailure,
) -> NewsShocksStageExecutionReport {
    NewsShocksStageExecutionReport {
        schema_version: CALIBRATION_NEWS_SHOCKS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::NewsShocks,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_NEWS_SHOCKS_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}
