use serde::Serialize;

use janiform_calibration_artifacts::{
    CALIBRATION_LONG_VARIANCE_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_LONG_VARIANCE_STAGE_HANDOFF_POLICY,
    CALIBRATION_LONG_VARIANCE_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource, LongVarianceStageDiagnostics,
    LongVarianceStageHorizonDiagnostic, stage_solver_settings_for_stage,
};
use janiform_foundations::time::{NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR};
use janiform_market_time::calendar::ExchangeCalendarRegistry;
use janiform_model_kernels::long_variance::LongVarianceParameters;

use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
use crate::stages::joint_metrics::build_observed_open_session_interval_observations;
use crate::stages::long_variance::estimation::estimate_long_variance_parameters;
use crate::stages::long_variance::metrics::{
    LONG_VARIANCE_HORIZON_DEFINITIONS, LONG_VARIANCE_HORIZON_MOMENT_METRIC_COUNT,
    LongVarianceHorizonMomentSummary, long_variance_horizon_summaries,
};
use crate::stages::return_components::ReturnComponentsStageCandidateGenerationInput;
use crate::stages::return_components::ReturnComponentsStageFittedPayload;
use crate::stages::seasonality::CalibrationStageExecutionStatus;

const LONG_VARIANCE_ESTIMATION_POLICY: &str =
    "observed_horizon_moment_initializer_with_standard_cir_dispersion_projection.v1";
const LONG_VARIANCE_STATIONARY_NORMALIZATION_POLICY: &str = "unit_stationary_mean_with_mean_normalized_seasonal_baseline_and_uncompensated_event_feedback.v1";
pub(crate) const LONG_VARIANCE_ACTIVE_PARAMETER_COUNT: usize = 4;
pub(crate) const LONG_VARIANCE_OBJECTIVE_TERM_COUNT: usize =
    LONG_VARIANCE_HORIZON_DEFINITIONS.len() * LONG_VARIANCE_HORIZON_MOMENT_METRIC_COUNT;
pub(crate) const LONG_VARIANCE_CANDIDATE_GEOMETRY_COUNT: usize =
    LONG_VARIANCE_ACTIVE_PARAMETER_COUNT;

#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceStageReturnComponentsInput {
    InMemoryOutput { selected_candidate_id: String },
    PersistedArtifactReference { artifact_reference: String },
}

impl LongVarianceStageReturnComponentsInput {
    pub const fn transport(&self) -> CalibrationStageHandoffTransport {
        match self {
            Self::InMemoryOutput { .. } => CalibrationStageHandoffTransport::InMemoryOutput,
            Self::PersistedArtifactReference { .. } => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LongVarianceStageExecutionInput {
    pub return_components_input: Option<LongVarianceStageReturnComponentsInput>,
    pub return_components_stage_payload: Option<ReturnComponentsStageFittedPayload>,
    pub candidate_generation_input: Option<ReturnComponentsStageCandidateGenerationInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LongVarianceStageFittedPayload {
    pub schema_version: &'static str,
    pub fitted_parameters: LongVarianceParameters,
    pub diagnostics: LongVarianceStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_selected_candidate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LongVarianceStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub horizon_target_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum LongVarianceStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingReturnComponentsInput,
    MissingReturnComponentsStagePayload,
    MissingCandidateGenerationInput,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LongVarianceStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<LongVarianceStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<LongVarianceStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<LongVarianceStageExecutionFailure>,
}

pub fn run_long_variance_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: LongVarianceStageExecutionInput,
) -> LongVarianceStageExecutionReport {
    if let Err(error_message) = run_spec.validate() {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::InvalidRunSpec {
                error_message: error_message.to_string(),
            },
        );
    }

    if !run_spec
        .selected_stages
        .contains(&CalibrationStage::LongVariance)
    {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::LongVariance)
    else {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    let Some(return_components_input) = execution_input.return_components_input else {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::MissingReturnComponentsInput,
        );
    };

    let Some(return_components_stage_payload) = execution_input.return_components_stage_payload
    else {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::MissingReturnComponentsStagePayload,
        );
    };

    let Some(candidate_generation_input) = execution_input.candidate_generation_input else {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::MissingCandidateGenerationInput,
        );
    };

    let fitted_stage_payload = build_long_variance_fitted_stage_payload(
        run_spec,
        calendar_registry,
        return_components_input,
        &return_components_stage_payload,
        &candidate_generation_input,
    );

    let seed_metadata = janiform_calibration_artifacts::resolve_calibration_stage_seed_metadata(
        run_spec,
        CalibrationStage::LongVariance,
    );

    LongVarianceStageExecutionReport {
        schema_version: CALIBRATION_LONG_VARIANCE_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::LongVariance,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_LONG_VARIANCE_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(LongVarianceStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_LONG_VARIANCE_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: seed_metadata.run_global_seed,
            stage_seed: seed_metadata.stage_seed,
            stage_seed_source: seed_metadata.stage_seed_source,
            stage_seed_derivation_policy:
                janiform_calibration_artifacts::CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings,
            horizon_target_count: LONG_VARIANCE_HORIZON_DEFINITIONS.len(),
        }),
        failure: None,
    }
}

fn build_long_variance_fitted_stage_payload(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    return_components_input: LongVarianceStageReturnComponentsInput,
    return_components_stage_payload: &ReturnComponentsStageFittedPayload,
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
) -> LongVarianceStageFittedPayload {
    let baseline_parameters = return_components_stage_payload
        .fitted_parameters
        .long_variance_parameters;

    let observed_intervals = observed_long_variance_intervals(
        calendar_registry,
        run_spec.data_inputs.exchange_code.as_str(),
        candidate_generation_input,
    );

    let fitted_parameters = estimate_long_variance_parameters(
        baseline_parameters,
        observed_intervals.five_minute.as_slice(),
        observed_intervals.hourly.as_slice(),
    );

    let horizon_summaries = long_variance_horizon_summaries(
        observed_intervals.five_minute.as_slice(),
        observed_intervals.hourly.as_slice(),
    );

    let diagnostics = long_variance_stage_diagnostics(
        selected_candidate_id(&return_components_input, return_components_stage_payload),
        horizon_summaries.as_slice(),
    );

    let return_components_input_transport = return_components_input.transport();

    let (return_components_selected_candidate_id, return_components_artifact_reference) =
        match return_components_input {
            LongVarianceStageReturnComponentsInput::InMemoryOutput {
                selected_candidate_id,
            } => (Some(selected_candidate_id), None),
            LongVarianceStageReturnComponentsInput::PersistedArtifactReference {
                artifact_reference,
            } => (None, Some(artifact_reference)),
        };

    LongVarianceStageFittedPayload {
        schema_version: CALIBRATION_LONG_VARIANCE_STAGE_OUTPUT_SCHEMA_VERSION,
        fitted_parameters,
        diagnostics,
        return_components_input_transport,
        return_components_selected_candidate_id,
        return_components_artifact_reference,
    }
}

fn selected_candidate_id<'a>(
    return_components_input: &'a LongVarianceStageReturnComponentsInput,
    return_components_stage_payload: &'a ReturnComponentsStageFittedPayload,
) -> &'a str {
    match return_components_input {
        LongVarianceStageReturnComponentsInput::InMemoryOutput {
            selected_candidate_id,
        } => selected_candidate_id,
        LongVarianceStageReturnComponentsInput::PersistedArtifactReference { .. } => {
            &return_components_stage_payload.selected_candidate_id
        }
    }
}

fn long_variance_stage_diagnostics(
    selected_candidate_id: &str,
    horizon_summaries: &[LongVarianceHorizonMomentSummary],
) -> LongVarianceStageDiagnostics {
    LongVarianceStageDiagnostics {
        estimation_policy: LONG_VARIANCE_ESTIMATION_POLICY.to_string(),
        stationary_normalization_policy: LONG_VARIANCE_STATIONARY_NORMALIZATION_POLICY.to_string(),
        selected_candidate_id: selected_candidate_id.to_string(),
        horizon_diagnostics: LONG_VARIANCE_HORIZON_DEFINITIONS
            .iter()
            .zip(horizon_summaries.iter())
            .map(
                |(horizon_definition, summary)| LongVarianceStageHorizonDiagnostic {
                    horizon_label: horizon_definition.label.to_string(),
                    horizon_hours: horizon_definition.hours,
                    sample_count: summary.sample_count,
                    realized_variance: summary.realized_variance,
                    variance_of_variance: summary.variance_of_variance,
                    return_skewness: summary.return_skewness,
                    return_excess_kurtosis: summary.return_excess_kurtosis,
                    activity_variance_correlation: summary.activity_variance_correlation,
                    signed_activity_future_variance_relation: summary
                        .signed_activity_future_variance_relation,
                    realized_variance_standard_error: summary.standard_errors.realized_variance,
                    variance_of_variance_standard_error: summary
                        .standard_errors
                        .variance_of_variance,
                    return_skewness_standard_error: summary.standard_errors.return_skewness,
                    return_excess_kurtosis_standard_error: summary
                        .standard_errors
                        .return_excess_kurtosis,
                    activity_variance_correlation_standard_error: summary
                        .standard_errors
                        .activity_variance_correlation,
                    signed_activity_future_variance_relation_standard_error: summary
                        .standard_errors
                        .signed_activity_future_variance_relation,
                },
            )
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ObservedLongVarianceIntervals {
    five_minute: Vec<JointRefinementAggregatedIntervalObservation>,
    hourly: Vec<JointRefinementAggregatedIntervalObservation>,
}

fn observed_long_variance_intervals(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
) -> ObservedLongVarianceIntervals {
    let source_window = candidate_generation_input.known_news_calendar.source_window;
    let observed_returns = candidate_generation_input
        .observed_open_session_returns
        .as_slice();

    ObservedLongVarianceIntervals {
        five_minute: build_observed_open_session_interval_observations(
            calendar_registry,
            exchange_code,
            source_window,
            observed_returns,
            NANOS_PER_FIVE_MINUTES,
        ),
        hourly: build_observed_open_session_interval_observations(
            calendar_registry,
            exchange_code,
            source_window,
            observed_returns,
            NANOS_PER_HOUR,
        ),
    }
}

fn failure_report(
    run_spec: &CalibrationRunSpec,
    failure: LongVarianceStageExecutionFailure,
) -> LongVarianceStageExecutionReport {
    LongVarianceStageExecutionReport {
        schema_version: CALIBRATION_LONG_VARIANCE_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::LongVariance,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_LONG_VARIANCE_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}
