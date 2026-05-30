use serde::Serialize;

use janiform_calibration_artifacts::{
    CALIBRATION_LONG_VARIANCE_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_LONG_VARIANCE_STAGE_HANDOFF_POLICY,
    CALIBRATION_LONG_VARIANCE_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    CalibrationStageSelectedCandidateSource, LongVarianceStageDiagnostics,
    LongVarianceStageHorizonDiagnostic, stage_solver_settings_for_stage,
};
use janiform_foundations::time::{NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR};
use janiform_market_time::calendar::ExchangeCalendarRegistry;
use janiform_model_kernels::long_variance::LongVarianceParameters;

use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::market_data::preparation::CalibrationTickObjectiveDataUsagePolicy;
use crate::stages::coarse_data::coarse_data_interval_observations;
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
use crate::stages::joint_metrics::build_observed_open_session_interval_observations;
use crate::stages::long_variance::estimation::estimate_long_variance_parameters;
use crate::stages::long_variance::metrics::{
    LONG_VARIANCE_HORIZON_DEFINITIONS, LONG_VARIANCE_HORIZON_MOMENT_METRIC_COUNT,
    LongVarianceHorizonMomentSummary, long_variance_horizon_summaries,
};
use crate::stages::long_variance::policy::long_variance_stationary_event_rates_for_hawkes_kernel;
use crate::stages::return_components::ReturnComponentsStageCandidateGenerationInput;
use crate::stages::return_components::ReturnComponentsStageFittedPayload;
use crate::stages::seasonality::CalibrationStageExecutionStatus;
use crate::stages::tick_objective_data::{
    retained_open_session_returns_for_tick_objectives, source_window_for_retained_tick_objectives,
};

const LONG_VARIANCE_ESTIMATION_POLICY: &str =
    "observed_horizon_moment_initializer_with_standard_cir_dispersion_projection.v1";
const LONG_VARIANCE_STATIONARY_NORMALIZATION_POLICY: &str = "unit_stationary_mean_with_mean_normalized_seasonal_baseline_and_uncompensated_event_feedback.v1";
pub(crate) const LONG_VARIANCE_ACTIVE_PARAMETER_COUNT: usize = 4;
pub(crate) const LONG_VARIANCE_OBJECTIVE_TERM_COUNT: usize =
    LONG_VARIANCE_HORIZON_DEFINITIONS.len() * LONG_VARIANCE_HORIZON_MOMENT_METRIC_COUNT;
pub(crate) const LONG_VARIANCE_CANDIDATE_GEOMETRY_COUNT: usize =
    LONG_VARIANCE_ACTIVE_PARAMETER_COUNT;

struct LongVarianceFittedStagePayloadBuildInput<'a> {
    run_spec: &'a CalibrationRunSpec,
    calendar_registry: &'a ExchangeCalendarRegistry,
    return_components_input: CalibrationStageSelectedCandidateSource,
    hawkes_stage_payload: &'a HawkesStageFittedPayload,
    return_components_stage_payload: &'a ReturnComponentsStageFittedPayload,
    candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    coarse_data_records: Option<&'a [MarketDataNormalizedCoarseDataRecord]>,
    tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LongVarianceStageExecutionInput {
    pub return_components_input: Option<CalibrationStageSelectedCandidateSource>,
    pub hawkes_stage_payload: Option<HawkesStageFittedPayload>,
    pub return_components_stage_payload: Option<ReturnComponentsStageFittedPayload>,
    pub candidate_generation_input: Option<ReturnComponentsStageCandidateGenerationInput>,
    pub coarse_data_records: Option<Vec<MarketDataNormalizedCoarseDataRecord>>,
    pub tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy,
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
    MissingHawkesStagePayload,
    InvalidHawkesStagePayload { error_message: String },
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

    let Some(hawkes_stage_payload) = execution_input.hawkes_stage_payload else {
        return failure_report(
            run_spec,
            LongVarianceStageExecutionFailure::MissingHawkesStagePayload,
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

    let fitted_stage_payload =
        match build_long_variance_fitted_stage_payload(LongVarianceFittedStagePayloadBuildInput {
            run_spec,
            calendar_registry,
            return_components_input,
            hawkes_stage_payload: &hawkes_stage_payload,
            return_components_stage_payload: &return_components_stage_payload,
            candidate_generation_input: &candidate_generation_input,
            coarse_data_records: execution_input.coarse_data_records.as_deref(),
            tick_objective_data_usage_policy: execution_input.tick_objective_data_usage_policy,
        }) {
            Ok(fitted_stage_payload) => fitted_stage_payload,
            Err(failure) => return failure_report(run_spec, failure),
        };

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
    input: LongVarianceFittedStagePayloadBuildInput<'_>,
) -> Result<LongVarianceStageFittedPayload, LongVarianceStageExecutionFailure> {
    let LongVarianceFittedStagePayloadBuildInput {
        run_spec,
        calendar_registry,
        return_components_input,
        hawkes_stage_payload,
        return_components_stage_payload,
        candidate_generation_input,
        coarse_data_records,
        tick_objective_data_usage_policy,
    } = input;

    let baseline_parameters = return_components_stage_payload
        .fitted_parameters
        .long_variance_parameters;

    let stationary_event_rates = long_variance_stationary_event_rates_for_hawkes_kernel(
        hawkes_stage_payload.fitted_parameters.kernel_parameters,
    )
    .ok_or_else(
        || LongVarianceStageExecutionFailure::InvalidHawkesStagePayload {
            error_message: "long variance stage requires finite stationary Hawkes event rates"
                .to_string(),
        },
    )?;

    let observed_intervals = observed_long_variance_intervals(
        calendar_registry,
        run_spec.data_inputs.exchange_code.as_str(),
        candidate_generation_input,
        coarse_data_records,
        tick_objective_data_usage_policy,
    );

    let fitted_parameters = estimate_long_variance_parameters(
        baseline_parameters,
        stationary_event_rates,
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
            CalibrationStageSelectedCandidateSource::InMemoryOutput {
                selected_candidate_id,
            } => (Some(selected_candidate_id), None),
            CalibrationStageSelectedCandidateSource::PersistedArtifactReference {
                artifact_reference,
            } => (None, Some(artifact_reference)),
        };

    Ok(LongVarianceStageFittedPayload {
        schema_version: CALIBRATION_LONG_VARIANCE_STAGE_OUTPUT_SCHEMA_VERSION,
        fitted_parameters,
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
    coarse_data_records: Option<&[MarketDataNormalizedCoarseDataRecord]>,
    tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy,
) -> ObservedLongVarianceIntervals {
    let observed_returns = retained_open_session_returns_for_tick_objectives(
        candidate_generation_input,
        tick_objective_data_usage_policy,
    );

    let source_window = source_window_for_retained_tick_objectives(
        candidate_generation_input,
        observed_returns.as_slice(),
    );

    let five_minute = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        observed_returns.as_slice(),
        NANOS_PER_FIVE_MINUTES,
    );

    let tick_hourly = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        observed_returns.as_slice(),
        NANOS_PER_HOUR,
    );

    let coarse_intervals = coarse_data_records
        .map(coarse_data_interval_observations)
        .unwrap_or_default();

    ObservedLongVarianceIntervals {
        five_minute,
        hourly: if coarse_intervals.is_empty() {
            tick_hourly
        } else {
            coarse_intervals
        },
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
