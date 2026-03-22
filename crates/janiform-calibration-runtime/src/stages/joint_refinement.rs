use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskModelInputs,
};
use janiform_engine_simulation::{
    CoarseSimulationModelInputs, CoarseSimulationRunRequest, SeededTickEventStateSnapshotCapture,
    SeededTickSimulationModelInputs, SeededTickSimulationRunConfig, SeededTickSimulationRunOptions,
    run_coarse_simulation, run_seeded_tick_event_simulation,
};
use rayon::prelude::*;
use serde::Serialize;

use crate::adapters::load_hourly_factor_contract_for_calibration;
use crate::calendar::ExchangeCalendarRegistry;
use crate::optimizer_adapter::CalibrationOptimizerAdapterRunMetadata;
use crate::stages::hawkes::{HawkesStageFittedPayload, stationary_open_event_rate_per_hour};
use crate::stages::joint_metrics::{
    JointRefinementAggregatedIntervalObservation,
    JointRefinementBusinessTimeBucketReturnScaleSummaries,
    JointRefinementBusinessTimeBucketThresholds, JointRefinementReturnScaleSummary,
    JointRefinementRollingVarianceActivityMetric, aggregate_simulated_path_to_interval_template,
    aggregate_simulated_path_to_interval_templates,
    build_continuous_calibration_sample_from_aggregated_intervals,
    build_observed_open_session_interval_observations, business_time_bucket_thresholds,
    log_ratio_distance, mean_absolute_return_by_utc_hour, normalized_profile_distance,
    relative_rmse, return_scale_summaries_by_business_time_bucket, return_scale_summary,
    rolling_variance_activity_metric, scale_normalized_returns, tick_count_series,
    zero_aware_normalized_bucket_value, zeroed_interval_observation_template,
};
use crate::stages::optimization::{PowellOptimizerSettings, optimize_bounded_powell};
use crate::stages::return_components::{
    ContinuousLikelihoodFidelityTier, NigShapeSearchPoint,
    ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageContinuousReturnCalibrationSample, ReturnComponentsStageFittedPayload,
    distribution_profile_fit_score, evaluate_configuration_likelihood_objective,
    nig_configuration_from_search_point, nig_search_point_from_configuration,
    prepare_observed_signal_inputs_for_joint_refinement, resolve_activity_calibration_context,
};
use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, build_runtime_hourly_seasonality_factors_from_components,
    maximum_realizable_runtime_hourly_seasonality_factor_from_components,
};
use janiform_calibration_artifacts::{
    CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY,
    CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource, HawkesStageFittedParameters,
    JointRefinementStageFittedParameters, JointRefinementStageObjectiveBreakdown,
    JointRefinementStageOptimizerDiagnostics, ReturnComponentsStageFittedParameters,
    resolve_calibration_stage_seed_metadata, stage_solver_settings_for_stage,
};
use janiform_foundations::low_discrepancy::van_der_corput;
use janiform_foundations::numeric::standardized_path_distance;
use janiform_foundations::splitmix64_output_mixer;
use janiform_market_time::calendar::{
    CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarRegistryArtifact,
    RegularSessionArtifact, SessionWindowArtifact,
};
use janiform_market_time::hourly_factors::{
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::news::SurpriseNewsArrivalConfiguration;

const JOINT_REFINEMENT_FIVE_MINUTES_NANOS: i64 = 5 * 60 * 1_000_000_000;
const JOINT_REFINEMENT_FIFTEEN_MINUTES_NANOS: i64 = 15 * 60 * 1_000_000_000;
const JOINT_REFINEMENT_ONE_HOUR_NANOS: i64 = 60 * 60 * 1_000_000_000;
const JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR: f64 = 1.0e-8;
const JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND: f64 = 1.386_294_361_119_890_6;
const JOINT_REFINEMENT_NIG_LOG_ALPHA_HALF_SPAN: f64 = 1.5;
const JOINT_REFINEMENT_NIG_SKEW_HALF_SPAN: f64 = 1.25;
const JOINT_REFINEMENT_NIG_LOG_DELTA_HALF_SPAN: f64 = 1.5;
const JOINT_REFINEMENT_MAX_OUTER_CYCLE_COUNT: usize = 3;
const JOINT_REFINEMENT_MAX_RESTART_COUNT: usize = 24;
const JOINT_REFINEMENT_WEIGHT_AGGREGATED_LIKELIHOOD_FIT: f64 = 0.18;
const JOINT_REFINEMENT_WEIGHT_AGGREGATED_RETURN_SHAPE_FIT: f64 = 0.06;
const JOINT_REFINEMENT_WEIGHT_AGGREGATED_RETURN_SCALE_FIT: f64 = 0.16;
const JOINT_REFINEMENT_WEIGHT_BUSINESS_TIME_RETURN_SCALE_FIT: f64 = 0.17;
const JOINT_REFINEMENT_WEIGHT_INTRADAY_RETURN_LEVEL_FIT: f64 = 0.15;
const JOINT_REFINEMENT_WEIGHT_ACTIVITY_SHAPE_FIT: f64 = 0.07;
const JOINT_REFINEMENT_WEIGHT_ACTIVITY_LEVEL_FIT: f64 = 0.13;
const JOINT_REFINEMENT_WEIGHT_RV_ACTIVITY_COUPLING_FIT: f64 = 0.03;
const JOINT_REFINEMENT_WEIGHT_PRICE_SPAN_SANITY_FIT: f64 = 0.05;
const JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR: f64 = 1.0e-8;
const JOINT_REFINEMENT_MISSING_BUSINESS_TIME_BUCKET_PENALTY: f64 = 5.0;
const JOINT_REFINEMENT_ADMISSIBILITY_SCALE_MULTIPLIER: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum JointRefinementFidelityTier {
    Local,
    Final,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct JointRefinementObjectiveCacheKey {
    fidelity_tier: JointRefinementFidelityTier,
    parameter_bits: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct JointRefinementSeedSummaryCacheKey {
    parameter_bits: Vec<u64>,
    simulation_seed: u64,
}

#[derive(Debug, Clone)]
struct JointRefinementActiveParameterLayout {
    hawkes_block: Vec<usize>,
    nig_block: Vec<usize>,
    coupling_block: Vec<usize>,
    log_lambda_scale_index: usize,
    log_beta_scale_index: usize,
    log_self_excitation_scale_index: usize,
    log_cross_excitation_scale_index: usize,
    known_activity_scale_index: Option<usize>,
    surprise_activity_scale_index: Option<usize>,
    surprise_baseline_scale_index: Option<usize>,
}

#[derive(Debug, Clone)]
struct JointRefinementObservedData {
    observed_open_event_rate_per_hour: f64,
    fallback_sell_share_proxy: f64,
    observed_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    observed_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    observed_5m_shape_returns: Vec<f64>,
    observed_15m_shape_returns: Vec<f64>,
    observed_5m_return_scale_summary: JointRefinementReturnScaleSummary,
    observed_15m_return_scale_summary: JointRefinementReturnScaleSummary,
    observed_5m_business_time_bucket_thresholds: JointRefinementBusinessTimeBucketThresholds,
    observed_15m_business_time_bucket_thresholds: JointRefinementBusinessTimeBucketThresholds,
    observed_5m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    observed_15m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    observed_5m_intraday_mean_absolute_returns: Vec<f64>,
    observed_15m_intraday_mean_absolute_returns: Vec<f64>,
    observed_hourly_tick_counts: Vec<f64>,
    observed_15m_rv_activity_metric: JointRefinementRollingVarianceActivityMetric,
    observed_log_price_span: f64,
    hourly_simulation_start_utc: DateTime<Utc>,
    hourly_simulation_end_utc: DateTime<Utc>,
    hourly_simulation_interval_count: usize,
    surprise_weekly_hour_utc_multipliers: Vec<f64>,
    simulated_5m_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_15m_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_hourly_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
}

#[derive(Debug, Clone)]
struct JointRefinementBaselineBuckets {
    aggregated_likelihood_fit: f64,
    aggregated_return_shape_fit: f64,
    aggregated_return_scale_fit: f64,
    business_time_return_scale_fit: f64,
    intraday_return_level_fit: f64,
    activity_shape_fit: f64,
    activity_level_fit: f64,
    rv_activity_coupling_fit: f64,
    price_span_sanity_fit: f64,
}

#[derive(Debug, Clone)]
struct JointRefinementContext {
    stage_thread_pool: Option<Arc<rayon::ThreadPool>>,
    calendar_registry: ExchangeCalendarRegistry,
    exchange_code: String,
    runtime_hourly_seasonality_factors: Vec<f64>,
    max_runtime_hourly_seasonality_factor: f64,
    observed_data: JointRefinementObservedData,
    baseline_hawkes_fitted_parameters: HawkesStageFittedParameters,
    baseline_return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
    active_parameter_layout: JointRefinementActiveParameterLayout,
    lower_bounds: Vec<f64>,
    upper_bounds: Vec<f64>,
    local_simulation_seed: u64,
    final_simulation_seeds: Vec<u64>,
    local_baseline_buckets: JointRefinementBaselineBuckets,
    final_baseline_buckets: JointRefinementBaselineBuckets,
    objective_cache:
        Arc<Mutex<BTreeMap<JointRefinementObjectiveCacheKey, JointRefinementCandidateEvaluation>>>,
    seed_summary_cache: Arc<
        Mutex<BTreeMap<JointRefinementSeedSummaryCacheKey, Option<JointRefinementSeedSummary>>>,
    >,
}

#[derive(Debug, Clone)]
struct JointRefinementCandidateEvaluation {
    fitted_parameters: JointRefinementStageFittedParameters,
    breakdown: JointRefinementStageObjectiveBreakdown,
    invalid_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct JointRefinementRestartOutcome {
    restart_id: String,
    parameter_vector: Vec<f64>,
    local_evaluation: JointRefinementCandidateEvaluation,
    final_evaluation: JointRefinementCandidateEvaluation,
    local_evaluation_count: usize,
    stop_reason: &'static str,
}

#[derive(Debug, Clone)]
struct JointRefinementSeedSummary {
    simulated_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    simulated_5m_shape_returns: Vec<f64>,
    simulated_15m_shape_returns: Vec<f64>,
    simulated_5m_return_scale_summary: JointRefinementReturnScaleSummary,
    simulated_15m_return_scale_summary: JointRefinementReturnScaleSummary,
    simulated_5m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    simulated_15m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    simulated_5m_intraday_mean_absolute_returns: Vec<f64>,
    simulated_15m_intraday_mean_absolute_returns: Vec<f64>,
    simulated_hourly_tick_counts: Vec<f64>,
    simulated_15m_rv_activity_metric: JointRefinementRollingVarianceActivityMetric,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JointRefinementStageHawkesInput {
    InMemoryOutput {
        payload: HawkesStageFittedPayload,
    },
    PersistedArtifactReference {
        artifact_reference: String,
        payload: Option<HawkesStageFittedPayload>,
    },
}

impl JointRefinementStageHawkesInput {
    pub const fn transport(&self) -> CalibrationStageHandoffTransport {
        match self {
            Self::InMemoryOutput { .. } => CalibrationStageHandoffTransport::InMemoryOutput,
            Self::PersistedArtifactReference { .. } => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        }
    }

    pub fn artifact_reference(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { .. } => None,
            Self::PersistedArtifactReference {
                artifact_reference, ..
            } => Some(artifact_reference.as_str()),
        }
    }

    pub fn payload(&self) -> Option<&HawkesStageFittedPayload> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload),
            Self::PersistedArtifactReference { payload, .. } => payload.as_ref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum JointRefinementStageReturnComponentsInput {
    InMemoryOutput {
        payload: ReturnComponentsStageFittedPayload,
    },
    PersistedArtifactReference {
        artifact_reference: String,
        payload: Option<ReturnComponentsStageFittedPayload>,
    },
}

impl JointRefinementStageReturnComponentsInput {
    pub const fn transport(&self) -> CalibrationStageHandoffTransport {
        match self {
            Self::InMemoryOutput { .. } => CalibrationStageHandoffTransport::InMemoryOutput,
            Self::PersistedArtifactReference { .. } => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        }
    }

    pub fn artifact_reference(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { .. } => None,
            Self::PersistedArtifactReference {
                artifact_reference, ..
            } => Some(artifact_reference.as_str()),
        }
    }

    pub fn payload(&self) -> Option<&ReturnComponentsStageFittedPayload> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload),
            Self::PersistedArtifactReference { payload, .. } => payload.as_ref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageSeasonalityFactorsInput {
    pub intraday_minutes_per_bin: u16,
    pub intraday_factor_by_bin: Vec<f64>,
    pub day_of_week_factor: Vec<f64>,
    pub day_of_year_factor: Vec<f64>,
    pub timestamp_utc_nanos: Vec<i64>,
    pub normalized_combined_factor_by_event: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageExecutionInput {
    pub seasonality_factors_input: JointRefinementStageSeasonalityFactorsInput,
    pub hawkes_input: JointRefinementStageHawkesInput,
    pub return_components_input: JointRefinementStageReturnComponentsInput,
    pub candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageFittedPayload {
    pub schema_version: &'static str,
    pub selected_candidate_id: String,
    pub fitted_parameters: JointRefinementStageFittedParameters,
    pub baseline_hawkes_fitted_parameters:
        janiform_calibration_artifacts::HawkesStageFittedParameters,
    pub baseline_return_components_fitted_parameters:
        janiform_calibration_artifacts::ReturnComponentsStageFittedParameters,
    pub objective_breakdown: JointRefinementStageObjectiveBreakdown,
    pub optimizer_diagnostics: JointRefinementStageOptimizerDiagnostics,
    pub candidate_frontier: Vec<JointRefinementStageCandidateDiagnostic>,
    pub simulation_summaries: Vec<JointRefinementStageSimulationSummary>,
    pub coarse_tick_consistency: Vec<JointRefinementStageCoarseTickConsistencyMetric>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageCandidateDiagnostic {
    pub candidate_id: String,
    pub valid: bool,
    pub admissible: bool,
    pub pareto_rank: usize,
    pub lexicographic_rank: usize,
    pub scalar_tie_break: f64,
    pub local_evaluation_count: usize,
    pub stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<String>,
    pub objective_breakdown: JointRefinementStageObjectiveBreakdown,
    pub canonical_coordinates: Vec<(String, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageSimulationSummary {
    pub simulation_seed: u64,
    pub horizon_label: String,
    pub event_count: usize,
    pub realized_variance: f64,
    pub mean_absolute_return: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageCoarseTickConsistencyMetric {
    pub simulation_seed: u64,
    pub horizon_label: String,
    pub metric_id: String,
    pub coarse_value: f64,
    pub tick_value: f64,
    pub absolute_delta: f64,
    pub normalized_delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub optimizer_adapter_metadata: CalibrationOptimizerAdapterRunMetadata,
    pub input_candidate_count: usize,
    pub valid_candidate_count: usize,
    pub selected_candidate_raw_objective: f64,
    pub selected_candidate_penalized_objective: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum JointRefinementStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingSeasonalityPayload,
    MissingHawkesPayload,
    MissingReturnComponentsPayload,
    InvalidCandidateGenerationInput { error_message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JointRefinementStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<JointRefinementStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JointRefinementStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<JointRefinementStageExecutionFailure>,
}

pub fn run_joint_refinement_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: JointRefinementStageExecutionInput,
) -> JointRefinementStageExecutionReport {
    if let Err(source) = run_spec.validate() {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::InvalidRunSpec {
                error_message: format!("{source} (classification={:?})", source.classification()),
            },
        );
    }

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::JointRefinement)
    {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(stage_solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::JointRefinement)
    else {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    let Some(hawkes_payload) = stage_input.hawkes_input.payload() else {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::MissingHawkesPayload,
        );
    };
    let Some(return_components_payload) = stage_input.return_components_input.payload() else {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::MissingReturnComponentsPayload,
        );
    };

    let resolved_stage_seed =
        resolve_calibration_stage_seed_metadata(run_spec, CalibrationStage::JointRefinement);
    let activity_context = resolve_activity_calibration_context(&stage_input_to_activity_input(
        &stage_input.hawkes_input,
    ));
    let _observed_signal_preparation = match prepare_observed_signal_inputs_for_joint_refinement(
        run_spec.data_inputs.exchange_code.as_str(),
        calendar_registry,
        stage_input.candidate_generation_input.clone(),
        activity_context,
    ) {
        Ok(preparation) => preparation,
        Err(source) => {
            return build_joint_refinement_stage_failure_report(
                run_spec,
                JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                    error_message: source.to_string(),
                },
            );
        }
    };

    let Some(observed_open_event_rate_per_hour) =
        selected_hawkes_observed_open_event_rate_per_hour(hawkes_payload)
    else {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::MissingHawkesPayload,
        );
    };
    let Some(baseline_modeled_open_event_rate_per_hour) =
        stationary_open_event_rate_per_hour(&hawkes_payload.fitted_parameters)
    else {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::MissingHawkesPayload,
        );
    };

    let local_simulation_seed = resolved_stage_seed.stage_seed;
    let all_final_simulation_seeds = [
        resolved_stage_seed.stage_seed,
        splitmix64_output_mixer(resolved_stage_seed.stage_seed ^ 0xA5A5_A5A5_A5A5_A5A5),
        splitmix64_output_mixer(resolved_stage_seed.stage_seed ^ 0x5A5A_5A5A_5A5A_5A5A),
    ];
    let final_simulation_seed_count =
        joint_refinement_final_simulation_seed_count(&stage_solver_settings);
    let final_simulation_seeds = all_final_simulation_seeds
        .into_iter()
        .take(final_simulation_seed_count)
        .collect::<Vec<_>>();
    let stage_thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(stage_solver_settings.cpu_count.max(1))
        .build()
        .ok()
        .map(Arc::new);

    let observed_data = build_observed_data(
        calendar_registry,
        run_spec.data_inputs.exchange_code.as_str(),
        &return_components_payload.fitted_parameters,
        &stage_input.candidate_generation_input,
        observed_open_event_rate_per_hour,
    );
    let baseline_15m_calibration_sample =
        build_continuous_calibration_sample_from_aggregated_intervals(
            observed_data.observed_15m_intervals.as_slice(),
            baseline_modeled_open_event_rate_per_hour,
            observed_data.observed_open_event_rate_per_hour,
            observed_data.fallback_sell_share_proxy,
        );
    let active_parameter_layout =
        build_active_parameter_layout(&return_components_payload.fitted_parameters);
    let baseline_parameter_vector = build_baseline_parameter_vector(
        &active_parameter_layout,
        &hawkes_payload.fitted_parameters,
        &return_components_payload.fitted_parameters,
        &baseline_15m_calibration_sample,
    );
    let (lower_bounds, upper_bounds) = build_parameter_bounds(
        &active_parameter_layout,
        baseline_parameter_vector.as_slice(),
    );
    let Some(runtime_hourly_seasonality_range_start_utc_nanos) = observed_data
        .hourly_simulation_start_utc
        .timestamp_nanos_opt()
    else {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint refinement runtime seasonality start timestamp is out of range"
                        .to_string(),
            },
        );
    };
    let Some(runtime_hourly_seasonality_range_end_utc_nanos) = observed_data
        .hourly_simulation_end_utc
        .timestamp_nanos_opt()
    else {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: "joint refinement runtime seasonality end timestamp is out of range"
                    .to_string(),
            },
        );
    };
    let runtime_hourly_seasonality_factors =
        match build_runtime_hourly_seasonality_factors_from_components(
            calendar_registry,
            run_spec.data_inputs.exchange_code.as_str(),
            stage_input
                .seasonality_factors_input
                .intraday_minutes_per_bin,
            stage_input
                .seasonality_factors_input
                .intraday_factor_by_bin
                .as_slice(),
            stage_input
                .seasonality_factors_input
                .day_of_week_factor
                .as_slice(),
            stage_input
                .seasonality_factors_input
                .day_of_year_factor
                .as_slice(),
            runtime_hourly_seasonality_range_start_utc_nanos,
            runtime_hourly_seasonality_range_end_utc_nanos,
            JOINT_REFINEMENT_ONE_HOUR_NANOS,
        ) {
            Ok(values) => values,
            Err(error_message) => {
                return build_joint_refinement_stage_failure_report(
                    run_spec,
                    JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                        error_message: format!(
                            "joint refinement runtime seasonality materialization failed: {error_message}"
                        ),
                    },
                );
            }
        };

    let max_runtime_hourly_seasonality_factor =
        match maximum_realizable_runtime_hourly_seasonality_factor_from_components(
            stage_input
                .seasonality_factors_input
                .intraday_minutes_per_bin,
            stage_input
                .seasonality_factors_input
                .intraday_factor_by_bin
                .as_slice(),
            stage_input
                .seasonality_factors_input
                .day_of_week_factor
                .as_slice(),
            stage_input
                .seasonality_factors_input
                .day_of_year_factor
                .as_slice(),
        ) {
            Ok(value) => value,
            Err(error_message) => {
                return build_joint_refinement_stage_failure_report(
                    run_spec,
                    JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                        error_message: format!(
                            "joint refinement runtime seasonality upper-bound estimation failed: {error_message}"
                        ),
                    },
                );
            }
        };

    let mut bootstrap_context = JointRefinementContext {
        stage_thread_pool,
        calendar_registry: calendar_registry.clone(),
        exchange_code: run_spec.data_inputs.exchange_code.clone(),
        max_runtime_hourly_seasonality_factor,
        runtime_hourly_seasonality_factors,
        observed_data,
        baseline_hawkes_fitted_parameters: hawkes_payload.fitted_parameters.clone(),
        baseline_return_components_fitted_parameters: return_components_payload
            .fitted_parameters
            .clone(),
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        local_simulation_seed,
        final_simulation_seeds,
        local_baseline_buckets: JointRefinementBaselineBuckets {
            aggregated_likelihood_fit: 1.0,
            aggregated_return_shape_fit: 1.0,
            aggregated_return_scale_fit: 1.0,
            business_time_return_scale_fit: 1.0,
            intraday_return_level_fit: 1.0,
            activity_shape_fit: 1.0,
            activity_level_fit: 1.0,
            rv_activity_coupling_fit: 1.0,
            price_span_sanity_fit: 1.0,
        },
        final_baseline_buckets: JointRefinementBaselineBuckets {
            aggregated_likelihood_fit: 1.0,
            aggregated_return_shape_fit: 1.0,
            aggregated_return_scale_fit: 1.0,
            business_time_return_scale_fit: 1.0,
            intraday_return_level_fit: 1.0,
            activity_shape_fit: 1.0,
            activity_level_fit: 1.0,
            rv_activity_coupling_fit: 1.0,
            price_span_sanity_fit: 1.0,
        },
        objective_cache: Arc::new(Mutex::new(BTreeMap::new())),
        seed_summary_cache: Arc::new(Mutex::new(BTreeMap::new())),
    };

    let baseline_fitted_parameters = JointRefinementStageFittedParameters {
        hawkes_fitted_parameters: hawkes_payload.fitted_parameters.clone(),
        return_components_fitted_parameters: return_components_payload.fitted_parameters.clone(),
    };
    let baseline_local_evaluation = evaluate_joint_fitted_parameters(
        baseline_fitted_parameters.clone(),
        &bootstrap_context,
        JointRefinementFidelityTier::Local,
        &stage_solver_settings,
    );
    let baseline_final_evaluation = evaluate_joint_fitted_parameters(
        baseline_fitted_parameters,
        &bootstrap_context,
        JointRefinementFidelityTier::Final,
        &stage_solver_settings,
    );

    if baseline_local_evaluation.invalid_reason.is_some()
        || baseline_final_evaluation.invalid_reason.is_some()
    {
        let local_reason = baseline_local_evaluation
            .invalid_reason
            .unwrap_or_else(|| "unknown_local_joint_refinement_failure".to_string());
        let final_reason = baseline_final_evaluation
            .invalid_reason
            .unwrap_or_else(|| "unknown_final_joint_refinement_failure".to_string());
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint refinement baseline candidate is invalid: local={local_reason} final={final_reason}"
                ),
            },
        );
    }

    bootstrap_context.local_baseline_buckets = JointRefinementBaselineBuckets {
        aggregated_likelihood_fit: raw_bucket_value_from_normalized(
            baseline_local_evaluation
                .breakdown
                .aggregated_likelihood_fit,
        ),
        aggregated_return_shape_fit: raw_bucket_value_from_normalized(
            baseline_local_evaluation
                .breakdown
                .aggregated_return_shape_fit,
        ),
        aggregated_return_scale_fit: raw_bucket_value_from_normalized(
            baseline_local_evaluation
                .breakdown
                .aggregated_return_scale_fit,
        ),
        business_time_return_scale_fit: raw_bucket_value_from_normalized(
            baseline_local_evaluation
                .breakdown
                .business_time_return_scale_fit,
        ),
        intraday_return_level_fit: raw_bucket_value_from_normalized(
            baseline_local_evaluation
                .breakdown
                .intraday_return_level_fit,
        ),
        activity_shape_fit: raw_bucket_value_from_normalized(
            baseline_local_evaluation.breakdown.activity_shape_fit,
        ),
        activity_level_fit: raw_bucket_value_from_normalized(
            baseline_local_evaluation.breakdown.activity_level_fit,
        ),
        rv_activity_coupling_fit: raw_bucket_value_from_normalized(
            baseline_local_evaluation.breakdown.rv_activity_coupling_fit,
        ),
        price_span_sanity_fit: raw_bucket_value_from_normalized(
            baseline_local_evaluation.breakdown.price_span_sanity_fit,
        ),
    };

    bootstrap_context.final_baseline_buckets = JointRefinementBaselineBuckets {
        aggregated_likelihood_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation
                .breakdown
                .aggregated_likelihood_fit,
        ),
        aggregated_return_shape_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation
                .breakdown
                .aggregated_return_shape_fit,
        ),
        aggregated_return_scale_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation
                .breakdown
                .aggregated_return_scale_fit,
        ),
        business_time_return_scale_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation
                .breakdown
                .business_time_return_scale_fit,
        ),
        intraday_return_level_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation
                .breakdown
                .intraday_return_level_fit,
        ),
        activity_shape_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation.breakdown.activity_shape_fit,
        ),
        activity_level_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation.breakdown.activity_level_fit,
        ),
        rv_activity_coupling_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation.breakdown.rv_activity_coupling_fit,
        ),
        price_span_sanity_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation.breakdown.price_span_sanity_fit,
        ),
    };

    if let Ok(mut cache_guard) = bootstrap_context.objective_cache.lock() {
        cache_guard.clear();
    }
    if let Ok(mut cache_guard) = bootstrap_context.seed_summary_cache.lock() {
        cache_guard.clear();
    }

    let restart_seeds = build_restart_seeds(
        baseline_parameter_vector.as_slice(),
        &bootstrap_context.active_parameter_layout,
        bootstrap_context.lower_bounds.as_slice(),
        bootstrap_context.upper_bounds.as_slice(),
        joint_refinement_budget_schedule(&stage_solver_settings).restart_count,
    );
    let restart_outcomes = optimize_restart_seeds(
        restart_seeds.as_slice(),
        &bootstrap_context,
        &stage_solver_settings,
    );
    let valid_restart_count = restart_outcomes
        .iter()
        .filter(|restart_outcome| {
            restart_outcome.local_evaluation.invalid_reason.is_none()
                && restart_outcome.final_evaluation.invalid_reason.is_none()
        })
        .count();

    let best_restart = select_best_restart_outcome(
        restart_outcomes
            .iter()
            .filter(|restart_outcome| {
                restart_outcome.local_evaluation.invalid_reason.is_none()
                    && restart_outcome.final_evaluation.invalid_reason.is_none()
            })
            .collect::<Vec<_>>()
            .as_slice(),
        &bootstrap_context,
    )
    .cloned()
    .unwrap_or_else(|| JointRefinementRestartOutcome {
        restart_id: "baseline".to_string(),
        parameter_vector: baseline_parameter_vector.clone(),
        local_evaluation: baseline_local_evaluation.clone(),
        final_evaluation: baseline_final_evaluation.clone(),
        local_evaluation_count: 0,
        stop_reason: "baseline_seed",
    });
    let candidate_frontier =
        build_joint_candidate_frontier_diagnostics(restart_outcomes.as_slice(), &bootstrap_context);
    let finalist_seed_summaries = build_joint_finalist_seed_summaries(
        &bootstrap_context,
        &best_restart.final_evaluation.fitted_parameters,
    );
    let simulation_summaries = build_joint_simulation_summaries(finalist_seed_summaries.as_slice());
    let coarse_tick_consistency = build_joint_coarse_tick_consistency_metrics(
        &bootstrap_context,
        &best_restart.final_evaluation.fitted_parameters,
        finalist_seed_summaries.as_slice(),
    );

    let fitted_stage_payload = JointRefinementStageFittedPayload {
        schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: format!("joint_refinement_{}", best_restart.restart_id),
        fitted_parameters: best_restart.final_evaluation.fitted_parameters.clone(),
        baseline_hawkes_fitted_parameters: hawkes_payload.fitted_parameters.clone(),
        baseline_return_components_fitted_parameters: return_components_payload
            .fitted_parameters
            .clone(),
        objective_breakdown: best_restart.final_evaluation.breakdown.clone(),
        optimizer_diagnostics: JointRefinementStageOptimizerDiagnostics {
            local_objective: best_restart.local_evaluation.breakdown.weighted_objective,
            final_objective: best_restart.final_evaluation.breakdown.weighted_objective,
            local_evaluation_count: best_restart.local_evaluation_count,
            final_simulation_seeds: bootstrap_context.final_simulation_seeds.clone(),
            stop_reason: best_restart.stop_reason.to_string(),
        },
        candidate_frontier,
        simulation_summaries,
        coarse_tick_consistency,
    };

    JointRefinementStageExecutionReport {
        schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::JointRefinement,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: Some(fitted_stage_payload.clone()),
        metadata: Some(JointRefinementStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: resolved_stage_seed.run_global_seed,
            stage_seed: resolved_stage_seed.stage_seed,
            stage_seed_source: resolved_stage_seed.stage_seed_source,
            stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings: stage_solver_settings.clone(),
            optimizer_adapter_metadata: CalibrationOptimizerAdapterRunMetadata {
                schema_version: "calibration.optimizer.adapter_run.v1",
                backend_id: "joint_refinement.block_coordinate_reconciliation.v1".to_string(),
                random_seed: resolved_stage_seed.stage_seed,
                selection_policy: "validity_then_admissibility_then_pareto_then_lexicographic_tie_break.v1",
                penalty_policy: "invalid_candidates_are_screened_before_lexicographic_frontier_selection.v1",
                bounds_policy: "bounded_joint_block_coordinate_refinement.v1",
                solver_settings: stage_solver_settings.clone(),
            },
            input_candidate_count: restart_seeds.len(),
            valid_candidate_count: valid_restart_count,
            selected_candidate_raw_objective: fitted_stage_payload
                .objective_breakdown
                .weighted_objective,
            selected_candidate_penalized_objective: fitted_stage_payload
                .objective_breakdown
                .weighted_objective
                + fitted_stage_payload
                    .objective_breakdown
                    .hard_constraint_penalty,
        }),
        failure: None,
    }
}

#[derive(Debug, Clone, Copy)]
struct JointRefinementAdmissibilityThresholds {
    aggregated_likelihood_fit: f64,
    aggregated_return_shape_fit: f64,
    aggregated_return_scale_fit: f64,
    business_time_return_scale_fit: f64,
    intraday_return_level_fit: f64,
    activity_shape_fit: f64,
    activity_level_fit: f64,
    rv_activity_coupling_fit: f64,
    price_span_sanity_fit: f64,
}

fn select_best_restart_outcome<'a>(
    restart_outcomes: &'a [&'a JointRefinementRestartOutcome],
    context: &JointRefinementContext,
) -> Option<&'a JointRefinementRestartOutcome> {
    if restart_outcomes.is_empty() {
        return None;
    }

    let thresholds = joint_refinement_admissibility_thresholds(context);
    let pareto_ranks: Vec<usize> = restart_outcomes
        .iter()
        .map(|candidate| {
            1 + restart_outcomes
                .iter()
                .filter(|other| {
                    candidate.restart_id != other.restart_id
                        && joint_restart_outcome_dominates(other, candidate)
                })
                .count()
        })
        .collect();

    let mut ranked_candidates: Vec<(usize, &JointRefinementRestartOutcome)> = restart_outcomes
        .iter()
        .enumerate()
        .map(|(position, outcome)| (pareto_ranks[position], *outcome))
        .collect();
    ranked_candidates.sort_by(|left, right| {
        let left_admissible = joint_restart_outcome_is_admissible(left.1, thresholds);
        let right_admissible = joint_restart_outcome_is_admissible(right.1, thresholds);
        (!left_admissible)
            .cmp(&!right_admissible)
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| {
                joint_restart_outcome_max_primary_metric(left.1)
                    .total_cmp(&joint_restart_outcome_max_primary_metric(right.1))
            })
            .then_with(|| {
                joint_restart_outcome_primary_metric_sum(left.1)
                    .total_cmp(&joint_restart_outcome_primary_metric_sum(right.1))
            })
            .then_with(|| {
                left.1
                    .final_evaluation
                    .breakdown
                    .price_span_sanity_fit
                    .total_cmp(&right.1.final_evaluation.breakdown.price_span_sanity_fit)
            })
            .then_with(|| {
                left.1
                    .final_evaluation
                    .breakdown
                    .weighted_objective
                    .total_cmp(&right.1.final_evaluation.breakdown.weighted_objective)
            })
            .then_with(|| left.1.restart_id.cmp(&right.1.restart_id))
    });

    ranked_candidates.first().map(|(_, outcome)| *outcome)
}

fn joint_refinement_admissibility_thresholds(
    context: &JointRefinementContext,
) -> JointRefinementAdmissibilityThresholds {
    let fifteen_minute_sample_count = context.observed_data.observed_15m_intervals.len();
    let hourly_sample_count = context.observed_data.observed_hourly_tick_counts.len();
    let rv_sample_count = context.observed_data.observed_15m_intervals.len();

    JointRefinementAdmissibilityThresholds {
        aggregated_likelihood_fit: admissibility_threshold_from_sample_count(
            fifteen_minute_sample_count,
        ),
        aggregated_return_shape_fit: admissibility_threshold_from_sample_count(
            fifteen_minute_sample_count,
        ),
        aggregated_return_scale_fit: admissibility_threshold_from_sample_count(
            fifteen_minute_sample_count,
        ),
        business_time_return_scale_fit: admissibility_threshold_from_sample_count(
            fifteen_minute_sample_count / 4,
        ),
        intraday_return_level_fit: admissibility_threshold_from_sample_count(hourly_sample_count),
        activity_shape_fit: admissibility_threshold_from_sample_count(hourly_sample_count),
        activity_level_fit: admissibility_threshold_from_sample_count(hourly_sample_count),
        rv_activity_coupling_fit: admissibility_threshold_from_sample_count(rv_sample_count),
        price_span_sanity_fit: admissibility_threshold_from_sample_count(
            fifteen_minute_sample_count / 8,
        ),
    }
}

fn admissibility_threshold_from_sample_count(sample_count: usize) -> f64 {
    1.0 + JOINT_REFINEMENT_ADMISSIBILITY_SCALE_MULTIPLIER / (sample_count.max(1) as f64).sqrt()
}

fn joint_restart_outcome_is_admissible(
    restart_outcome: &JointRefinementRestartOutcome,
    thresholds: JointRefinementAdmissibilityThresholds,
) -> bool {
    let breakdown = &restart_outcome.final_evaluation.breakdown;
    breakdown.aggregated_likelihood_fit <= thresholds.aggregated_likelihood_fit
        && breakdown.aggregated_return_shape_fit <= thresholds.aggregated_return_shape_fit
        && breakdown.aggregated_return_scale_fit <= thresholds.aggregated_return_scale_fit
        && breakdown.business_time_return_scale_fit <= thresholds.business_time_return_scale_fit
        && breakdown.intraday_return_level_fit <= thresholds.intraday_return_level_fit
        && breakdown.activity_shape_fit <= thresholds.activity_shape_fit
        && breakdown.activity_level_fit <= thresholds.activity_level_fit
        && breakdown.rv_activity_coupling_fit <= thresholds.rv_activity_coupling_fit
        && breakdown.price_span_sanity_fit <= thresholds.price_span_sanity_fit
}

fn joint_restart_outcome_dominates(
    left: &JointRefinementRestartOutcome,
    right: &JointRefinementRestartOutcome,
) -> bool {
    let left_metrics = joint_restart_outcome_primary_metrics(left);
    let right_metrics = joint_restart_outcome_primary_metrics(right);

    let all_not_worse = left_metrics
        .iter()
        .zip(right_metrics.iter())
        .all(|(left_value, right_value)| left_value <= right_value);
    let any_strictly_better = left_metrics
        .iter()
        .zip(right_metrics.iter())
        .any(|(left_value, right_value)| left_value < right_value);

    all_not_worse && any_strictly_better
}

fn joint_restart_outcome_primary_metrics(
    restart_outcome: &JointRefinementRestartOutcome,
) -> [f64; 8] {
    let breakdown = &restart_outcome.final_evaluation.breakdown;
    [
        breakdown.aggregated_likelihood_fit,
        breakdown.aggregated_return_shape_fit,
        breakdown.aggregated_return_scale_fit,
        breakdown.business_time_return_scale_fit,
        breakdown.intraday_return_level_fit,
        breakdown.activity_shape_fit,
        breakdown.activity_level_fit,
        breakdown.rv_activity_coupling_fit,
    ]
}

fn joint_restart_outcome_primary_metric_sum(
    restart_outcome: &JointRefinementRestartOutcome,
) -> f64 {
    joint_restart_outcome_primary_metrics(restart_outcome)
        .into_iter()
        .sum()
}

fn joint_restart_outcome_max_primary_metric(
    restart_outcome: &JointRefinementRestartOutcome,
) -> f64 {
    joint_restart_outcome_primary_metrics(restart_outcome)
        .into_iter()
        .fold(f64::NEG_INFINITY, f64::max)
}

fn build_joint_candidate_frontier_diagnostics(
    restart_outcomes: &[JointRefinementRestartOutcome],
    context: &JointRefinementContext,
) -> Vec<JointRefinementStageCandidateDiagnostic> {
    if restart_outcomes.is_empty() {
        return Vec::new();
    }

    let thresholds = joint_refinement_admissibility_thresholds(context);
    let pareto_ranks: Vec<usize> = restart_outcomes
        .iter()
        .map(|candidate| {
            1 + restart_outcomes
                .iter()
                .filter(|other| joint_restart_outcome_dominates(other, candidate))
                .count()
        })
        .collect();

    let mut ranked_candidates: Vec<(usize, &JointRefinementRestartOutcome)> = restart_outcomes
        .iter()
        .enumerate()
        .map(|(position, outcome)| (pareto_ranks[position], outcome))
        .collect();
    ranked_candidates.sort_by(|left, right| {
        let left_admissible = joint_restart_outcome_is_admissible(left.1, thresholds);
        let right_admissible = joint_restart_outcome_is_admissible(right.1, thresholds);
        (!left_admissible)
            .cmp(&!right_admissible)
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| {
                joint_restart_outcome_max_primary_metric(left.1)
                    .total_cmp(&joint_restart_outcome_max_primary_metric(right.1))
            })
            .then_with(|| {
                joint_restart_outcome_primary_metric_sum(left.1)
                    .total_cmp(&joint_restart_outcome_primary_metric_sum(right.1))
            })
            .then_with(|| {
                left.1
                    .final_evaluation
                    .breakdown
                    .price_span_sanity_fit
                    .total_cmp(&right.1.final_evaluation.breakdown.price_span_sanity_fit)
            })
            .then_with(|| {
                left.1
                    .final_evaluation
                    .breakdown
                    .weighted_objective
                    .total_cmp(&right.1.final_evaluation.breakdown.weighted_objective)
            })
            .then_with(|| left.1.restart_id.cmp(&right.1.restart_id))
    });

    let lexicographic_rank_by_restart_id: BTreeMap<&str, usize> = ranked_candidates
        .iter()
        .enumerate()
        .map(|(position, (_, outcome))| (outcome.restart_id.as_str(), position + 1))
        .collect();

    restart_outcomes
        .iter()
        .enumerate()
        .map(|(position, outcome)| {
            let valid = outcome.local_evaluation.invalid_reason.is_none()
                && outcome.final_evaluation.invalid_reason.is_none();
            let invalid_reason = outcome
                .final_evaluation
                .invalid_reason
                .clone()
                .or_else(|| outcome.local_evaluation.invalid_reason.clone());

            JointRefinementStageCandidateDiagnostic {
                candidate_id: format!("joint_refinement_{}", outcome.restart_id),
                valid,
                admissible: valid && joint_restart_outcome_is_admissible(outcome, thresholds),
                pareto_rank: pareto_ranks[position],
                lexicographic_rank: *lexicographic_rank_by_restart_id
                    .get(outcome.restart_id.as_str())
                    .unwrap_or(&(restart_outcomes.len() + 1)),
                scalar_tie_break: outcome.final_evaluation.breakdown.weighted_objective,
                local_evaluation_count: outcome.local_evaluation_count,
                stop_reason: outcome.stop_reason.to_string(),
                invalid_reason,
                objective_breakdown: outcome.final_evaluation.breakdown.clone(),
                canonical_coordinates: named_joint_parameter_coordinates(
                    outcome.parameter_vector.as_slice(),
                    &context.active_parameter_layout,
                ),
            }
        })
        .collect()
}

fn named_joint_parameter_coordinates(
    parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
) -> Vec<(String, f64)> {
    let mut coordinates = vec![
        (
            active_parameter_layout.log_lambda_scale_index,
            "log_lambda_scale".to_string(),
        ),
        (
            active_parameter_layout.log_beta_scale_index,
            "log_beta_scale".to_string(),
        ),
        (
            active_parameter_layout.log_self_excitation_scale_index,
            "log_self_excitation_scale".to_string(),
        ),
        (
            active_parameter_layout.log_cross_excitation_scale_index,
            "log_cross_excitation_scale".to_string(),
        ),
        (
            active_parameter_layout.nig_block[0],
            "nig.log_alpha".to_string(),
        ),
        (
            active_parameter_layout.nig_block[1],
            "nig.buy_skew_coordinate".to_string(),
        ),
        (
            active_parameter_layout.nig_block[2],
            "nig.sell_relative_coordinate".to_string(),
        ),
        (
            active_parameter_layout.nig_block[3],
            "nig.log_delta_offset".to_string(),
        ),
    ];

    if let Some(index) = active_parameter_layout.known_activity_scale_index {
        coordinates.push((index, "known_activity_scale".to_string()));
    }
    if let Some(index) = active_parameter_layout.surprise_activity_scale_index {
        coordinates.push((index, "surprise_activity_scale".to_string()));
    }
    if let Some(index) = active_parameter_layout.surprise_baseline_scale_index {
        coordinates.push((index, "surprise_baseline_scale".to_string()));
    }

    coordinates.sort_by_key(|(index, _)| *index);
    coordinates
        .into_iter()
        .filter_map(|(index, label)| {
            parameter_vector
                .get(index)
                .copied()
                .map(|value| (label, value))
        })
        .collect()
}

fn build_joint_finalist_seed_summaries(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
) -> Vec<(u64, JointRefinementSeedSummary)> {
    context
        .final_simulation_seeds
        .iter()
        .filter_map(|simulation_seed| {
            cached_seed_summary_for_candidate_seed(
                None,
                fitted_parameters,
                context,
                *simulation_seed,
            )
            .map(|summary| (*simulation_seed, summary))
        })
        .collect()
}

fn build_joint_simulation_summaries(
    seed_summaries: &[(u64, JointRefinementSeedSummary)],
) -> Vec<JointRefinementStageSimulationSummary> {
    let mut summaries = Vec::with_capacity(seed_summaries.len() * 2);
    for (simulation_seed, seed_summary) in seed_summaries {
        summaries.push(JointRefinementStageSimulationSummary {
            simulation_seed: *simulation_seed,
            horizon_label: "5m".to_string(),
            event_count: seed_summary
                .simulated_5m_intervals
                .iter()
                .map(|interval| interval.tick_count)
                .sum(),
            realized_variance: seed_summary
                .simulated_5m_return_scale_summary
                .realized_variance,
            mean_absolute_return: seed_summary
                .simulated_5m_return_scale_summary
                .mean_absolute_return,
        });
        summaries.push(JointRefinementStageSimulationSummary {
            simulation_seed: *simulation_seed,
            horizon_label: "15m".to_string(),
            event_count: seed_summary
                .simulated_15m_intervals
                .iter()
                .map(|interval| interval.tick_count)
                .sum(),
            realized_variance: seed_summary
                .simulated_15m_return_scale_summary
                .realized_variance,
            mean_absolute_return: seed_summary
                .simulated_15m_return_scale_summary
                .mean_absolute_return,
        });
    }

    summaries
}

fn build_joint_coarse_tick_consistency_metrics(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    tick_seed_summaries: &[(u64, JointRefinementSeedSummary)],
) -> Vec<JointRefinementStageCoarseTickConsistencyMetric> {
    let mut metrics = Vec::new();

    for (simulation_seed, tick_summary) in tick_seed_summaries {
        if let Some(coarse_5m_intervals) = build_joint_coarse_interval_observations(
            context,
            fitted_parameters,
            *simulation_seed,
            JOINT_REFINEMENT_FIVE_MINUTES_NANOS,
            context
                .observed_data
                .simulated_5m_interval_template
                .as_slice(),
        ) {
            let coarse_15m_intervals = aggregate_coarse_intervals_to_template(
                coarse_5m_intervals.as_slice(),
                context
                    .observed_data
                    .simulated_15m_interval_template
                    .as_slice(),
            );
            metrics.extend(joint_coarse_tick_metrics_for_horizon(
                *simulation_seed,
                "5m",
                sum_interval_tick_count(coarse_5m_intervals.as_slice()) as f64,
                return_scale_summary(coarse_5m_intervals.as_slice()),
                tick_summary
                    .simulated_5m_intervals
                    .iter()
                    .map(|interval| interval.tick_count)
                    .sum::<usize>() as f64,
                tick_summary.simulated_5m_return_scale_summary,
            ));
            metrics.extend(joint_coarse_tick_metrics_for_horizon(
                *simulation_seed,
                "15m",
                sum_interval_tick_count(coarse_15m_intervals.as_slice()) as f64,
                return_scale_summary(coarse_15m_intervals.as_slice()),
                tick_summary
                    .simulated_15m_intervals
                    .iter()
                    .map(|interval| interval.tick_count)
                    .sum::<usize>() as f64,
                tick_summary.simulated_15m_return_scale_summary,
            ));
        }
    }

    metrics
}

fn joint_coarse_tick_metrics_for_horizon(
    simulation_seed: u64,
    horizon_label: &str,
    coarse_event_count: f64,
    coarse_summary: JointRefinementReturnScaleSummary,
    tick_event_count: f64,
    tick_summary: JointRefinementReturnScaleSummary,
) -> Vec<JointRefinementStageCoarseTickConsistencyMetric> {
    [
        ("event_count", coarse_event_count, tick_event_count),
        (
            "mean_absolute_return",
            coarse_summary.mean_absolute_return,
            tick_summary.mean_absolute_return,
        ),
        (
            "realized_variance",
            coarse_summary.realized_variance,
            tick_summary.realized_variance,
        ),
        (
            "absolute_return_quantile_q90",
            coarse_summary.absolute_return_quantile_q90,
            tick_summary.absolute_return_quantile_q90,
        ),
    ]
    .into_iter()
    .map(|(metric_id, coarse_value, tick_value)| {
        let absolute_delta = (coarse_value - tick_value).abs();
        let normalized_delta = absolute_delta / tick_value.abs().max(1.0e-8);
        JointRefinementStageCoarseTickConsistencyMetric {
            simulation_seed,
            horizon_label: horizon_label.to_string(),
            metric_id: metric_id.to_string(),
            coarse_value,
            tick_value,
            absolute_delta,
            normalized_delta,
        }
    })
    .collect()
}

fn build_joint_coarse_interval_observations(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
    interval_duration_nanos: i64,
    template: &[JointRefinementAggregatedIntervalObservation],
) -> Option<Vec<JointRefinementAggregatedIntervalObservation>> {
    if template.is_empty() {
        return None;
    }
    let interval_count = ((context
        .observed_data
        .hourly_simulation_end_utc
        .timestamp_nanos_opt()?
        - context
            .observed_data
            .hourly_simulation_start_utc
            .timestamp_nanos_opt()?)
    .div_euclid(interval_duration_nanos)) as usize;
    let surprise_deterministic_time_multiplier_by_interval_index =
        build_surprise_multipliers_for_interval_grid(
            context.observed_data.hourly_simulation_start_utc,
            interval_count,
            interval_duration_nanos,
            context
                .observed_data
                .surprise_weekly_hour_utc_multipliers
                .as_slice(),
        );
    let hourly_factor_contract = load_hourly_factor_contract_for_calibration(
        &context.calendar_registry,
        HourlyFactorProviderRequest {
            exchange_code: context.exchange_code.as_str(),
            range_start_utc: context.observed_data.hourly_simulation_start_utc,
            range_end_utc: context.observed_data.hourly_simulation_end_utc,
            interval_duration_nanos,
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: context.runtime_hourly_seasonality_factors.as_slice(),
                materialization_method:
                    "runtime_compact_seasonality_parameters_mean_renormalized.v1",
            }),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Materialized {
                known_scheduled_event_timestamps_utc_nanos: fitted_parameters
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .known
                    .scheduled_event_timestamps_utc_nanos
                    .as_slice(),
                surprise_baseline_intensity: fitted_parameters
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .surprise
                    .baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index:
                    surprise_deterministic_time_multiplier_by_interval_index.as_slice(),
            }),
        },
    )
    .ok()?;

    let interval_delta = interval_duration_nanos as f64 / JOINT_REFINEMENT_ONE_HOUR_NANOS as f64;
    let interval_factors = build_coarse_interval_factors_from_hourly_contract(
        &hourly_factor_contract,
        fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration
            .arrivals
            .known
            .scheduled_event_timestamps_utc_nanos
            .as_slice(),
        interval_delta,
    )?;
    let model_inputs = build_joint_coarse_model_inputs(context, fitted_parameters)?;
    let coarse_path = run_coarse_simulation(
        CoarseSimulationRunRequest::try_new(
            simulation_seed,
            hourly_factor_contract.request_start_utc_nanos,
            hourly_factor_contract.interval_indices.len(),
            interval_delta,
            model_inputs,
            interval_factors,
        )
        .ok()?,
    )
    .ok()?;

    let mut intervals = zeroed_interval_observation_template(template);
    let interval_index_by_bounds: BTreeMap<(i64, i64), usize> = intervals
        .iter()
        .enumerate()
        .map(|(index, interval)| {
            (
                (
                    interval.interval_start_utc_nanos,
                    interval.interval_end_utc_nanos,
                ),
                index,
            )
        })
        .collect();

    for record in coarse_path.records {
        if let Some(interval_index) = interval_index_by_bounds.get(&(
            record.interval_start_utc_nanos,
            record.interval_end_utc_nanos,
        )) {
            let Some(interval) = intervals.get_mut(*interval_index) else {
                continue;
            };
            interval.log_return += record.return_components.total_return;
            interval.tick_count += (record.sufficient_statistics.n_plus
                + record.sufficient_statistics.n_minus) as usize;
            interval.sell_share_proxy += record.sufficient_statistics.n_minus as f64;
        }
    }

    finalize_coarse_sell_share_proxies(intervals.as_mut_slice(), 0.5);

    Some(intervals)
}

fn aggregate_coarse_intervals_to_template(
    source_intervals: &[JointRefinementAggregatedIntervalObservation],
    template: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<JointRefinementAggregatedIntervalObservation> {
    let mut aggregated_intervals = zeroed_interval_observation_template(template);
    if aggregated_intervals.is_empty() {
        return aggregated_intervals;
    }

    let mut target_index = 0usize;
    for source_interval in source_intervals {
        while target_index < aggregated_intervals.len()
            && source_interval.interval_start_utc_nanos
                >= aggregated_intervals[target_index].interval_end_utc_nanos
        {
            target_index += 1;
        }
        if target_index >= aggregated_intervals.len() {
            break;
        }

        let target_interval = &mut aggregated_intervals[target_index];
        if source_interval.interval_start_utc_nanos < target_interval.interval_start_utc_nanos
            || source_interval.interval_end_utc_nanos > target_interval.interval_end_utc_nanos
        {
            continue;
        }

        target_interval.log_return += source_interval.log_return;
        target_interval.tick_count += source_interval.tick_count;
        target_interval.sell_share_proxy +=
            source_interval.sell_share_proxy * source_interval.tick_count as f64;
    }

    finalize_coarse_sell_share_proxies(aggregated_intervals.as_mut_slice(), 0.5);
    aggregated_intervals
}

fn finalize_coarse_sell_share_proxies(
    intervals: &mut [JointRefinementAggregatedIntervalObservation],
    fallback_sell_share_proxy: f64,
) {
    for interval in intervals {
        interval.sell_share_proxy = if interval.tick_count == 0 {
            fallback_sell_share_proxy
        } else {
            (interval.sell_share_proxy / interval.tick_count as f64).clamp(0.0, 1.0)
        };
    }
}

fn sum_interval_tick_count(intervals: &[JointRefinementAggregatedIntervalObservation]) -> usize {
    intervals.iter().map(|interval| interval.tick_count).sum()
}

fn build_coarse_interval_factors_from_hourly_contract(
    hourly_factor_contract: &janiform_market_time::hourly_factors::HourlyFactorContractOutput,
    known_news_timestamps_utc_nanos: &[i64],
    interval_delta: f64,
) -> Option<SufficientStatisticsRiskIntervalFactors> {
    let nu_h_known = hourly_factor_contract
        .interval_start_utc_nanos
        .iter()
        .zip(hourly_factor_contract.interval_end_utc_nanos.iter())
        .map(|(interval_start_utc_nanos, interval_end_utc_nanos)| {
            count_known_news_events(
                known_news_timestamps_utc_nanos,
                *interval_start_utc_nanos,
                *interval_end_utc_nanos,
            ) as f64
                / interval_delta
        })
        .collect::<Vec<_>>();
    let nu_h_surprise = hourly_factor_contract
        .nu_h
        .iter()
        .zip(nu_h_known.iter())
        .map(|(combined_intensity, known_intensity)| {
            (combined_intensity - known_intensity).max(0.0)
        })
        .collect::<Vec<_>>();

    SufficientStatisticsRiskIntervalFactors::try_new(
        hourly_factor_contract.s_h.clone(),
        nu_h_known,
        nu_h_surprise,
        Some(interval_delta),
    )
    .ok()
}

fn count_known_news_events(
    known_news_timestamps_utc_nanos: &[i64],
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> usize {
    known_news_timestamps_utc_nanos
        .iter()
        .filter(|timestamp_utc_nanos| {
            **timestamp_utc_nanos >= interval_start_utc_nanos
                && **timestamp_utc_nanos < interval_end_utc_nanos
        })
        .count()
}

fn build_joint_coarse_model_inputs(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
) -> Option<CoarseSimulationModelInputs> {
    let risk_model_inputs = SufficientStatisticsRiskModelInputs::try_new(
        ExponentialAffineTransformEquationSpecification::try_new(
            fitted_parameters.hawkes_fitted_parameters.kernel_parameters,
            fitted_parameters
                .return_components_fitted_parameters
                .news_process_configuration
                .shared_mark_law
                .gamma_severity_marks,
        )
        .ok()?,
        fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
    )
    .ok()?;

    CoarseSimulationModelInputs::try_new(
        risk_model_inputs,
        fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration
            .clone(),
        build_exchange_calendar_registry_artifact(
            &context.calendar_registry,
            context.exchange_code.as_str(),
            context.observed_data.hourly_simulation_start_utc,
        )?,
        context.exchange_code.clone(),
    )
    .ok()
}

fn build_exchange_calendar_registry_artifact(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    artifact_anchor_time_utc: DateTime<Utc>,
) -> Option<ExchangeCalendarRegistryArtifact> {
    let definition = calendar_registry.get_required(exchange_code).ok()?;
    let regular_sessions = definition
        .regular_sessions()
        .iter()
        .map(|regular_session| RegularSessionArtifact {
            weekday: regular_session.weekday(),
            windows: regular_session
                .windows()
                .iter()
                .map(|window| SessionWindowArtifact {
                    label: window.label().to_string(),
                    open_time: window.open_time().to_string(),
                    close_time: window.close_time().to_string(),
                })
                .collect(),
        })
        .collect();

    Some(ExchangeCalendarRegistryArtifact {
        schema_version: "calendar.registry.v1".to_string(),
        artifact_date: artifact_anchor_time_utc.date_naive().to_string(),
        exchanges: vec![ExchangeCalendarArtifact {
            schema_version: definition.schema_version().to_string(),
            exchange_code: definition.exchange_metadata().exchange_code().to_string(),
            exchange_name: definition.exchange_metadata().exchange_name().to_string(),
            timezone_name: Some(definition.timezone_name().to_string()),
            rule_metadata: CalendarRuleMetadataArtifact {
                holiday_calendar_code: definition
                    .rule_metadata()
                    .holiday_calendar_code()
                    .to_string(),
                early_close_rule_set: definition
                    .rule_metadata()
                    .early_close_rule_set()
                    .map(str::to_string),
            },
            regular_sessions,
            holiday_dates: Vec::new(),
            early_close_sessions: Vec::new(),
        }],
    })
}

fn optimize_restart_seeds(
    restart_seeds: &[(String, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<JointRefinementRestartOutcome> {
    with_joint_refinement_thread_pool(context, || {
        restart_seeds
            .par_iter()
            .map(|(restart_id, seed_vector)| {
                optimize_single_restart(
                    restart_id.as_str(),
                    seed_vector.as_slice(),
                    context,
                    solver_settings,
                )
            })
            .collect()
    })
}

fn with_joint_refinement_thread_pool<T, F>(context: &JointRefinementContext, operation: F) -> T
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    match context.stage_thread_pool.as_ref() {
        Some(thread_pool) => thread_pool.install(operation),
        None => operation(),
    }
}

fn ordered_parameter_bits(parameter_vector: &[f64]) -> Vec<u64> {
    parameter_vector
        .iter()
        .map(|value| value.to_bits())
        .collect::<Vec<_>>()
}

fn deduplicate_restart_seed_candidates(
    restart_candidates: Vec<(String, Vec<f64>)>,
) -> Vec<(String, Vec<f64>)> {
    let mut deduplicated_candidates = Vec::new();
    let mut seen_parameter_bits = Vec::new();

    for (restart_id, candidate_vector) in restart_candidates {
        let parameter_bits = ordered_parameter_bits(candidate_vector.as_slice());
        if seen_parameter_bits
            .iter()
            .any(|seen_bits: &Vec<u64>| seen_bits == &parameter_bits)
        {
            continue;
        }

        seen_parameter_bits.push(parameter_bits);
        deduplicated_candidates.push((restart_id, candidate_vector));
    }

    deduplicated_candidates
}

fn halton_parameter_vector(
    halton_index: u64,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<f64> {
    const HALTON_BASES: [u64; 16] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53];

    lower_bounds
        .iter()
        .zip(upper_bounds.iter())
        .enumerate()
        .map(|(dimension_index, (lower_bound, upper_bound))| {
            let halton_coordinate = van_der_corput(
                halton_index,
                HALTON_BASES[dimension_index % HALTON_BASES.len()],
            );
            lower_bound + (upper_bound - lower_bound) * halton_coordinate
        })
        .collect()
}

fn build_halton_restart_id(position: usize) -> String {
    format!("halton_{position:02}")
}

#[derive(Debug, Clone, Copy)]
struct JointRefinementBudgetSchedule {
    restart_count: usize,
    outer_cycle_count: usize,
    powell_settings: PowellOptimizerSettings,
}

fn joint_refinement_budget_schedule(
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementBudgetSchedule {
    let restart_count = solver_settings
        .cpu_count
        .max(1)
        .min(JOINT_REFINEMENT_MAX_RESTART_COUNT);
    let (base_outer_cycle_count, base_powell_settings) =
        joint_refinement_base_schedule_components(solver_settings);
    let total_block_eval_budget =
        3usize * base_outer_cycle_count * base_powell_settings.evaluation_budget;
    let outer_cycle_count = ((3usize * base_outer_cycle_count) / restart_count.max(1)).max(1);
    let evaluation_budget =
        (total_block_eval_budget / (restart_count.max(1) * outer_cycle_count.max(1))).max(16);
    let line_search_evaluation_cap = (evaluation_budget / 4).clamp(4, 8);

    JointRefinementBudgetSchedule {
        restart_count,
        outer_cycle_count,
        powell_settings: PowellOptimizerSettings {
            absolute_tolerance: solver_settings.absolute_tolerance,
            relative_tolerance: solver_settings.relative_tolerance,
            min_step_norm: solver_settings.absolute_tolerance,
            max_cycle_count: base_powell_settings.max_cycle_count,
            line_search_evaluation_cap,
            evaluation_budget,
        },
    }
}

fn joint_refinement_base_schedule_components(
    solver_settings: &CalibrationOptimizerSettings,
) -> (usize, PowellOptimizerSettings) {
    let (outer_cycle_count, max_cycle_count, line_search_evaluation_cap, minimum_evaluation_budget) =
        match solver_settings.max_iterations {
            0..=32 => (1, 2, 4, 8),
            33..=96 => (2, 4, 6, 16),
            _ => (JOINT_REFINEMENT_MAX_OUTER_CYCLE_COUNT, 8, 8, 24),
        };

    let powell_settings = PowellOptimizerSettings {
        absolute_tolerance: solver_settings.absolute_tolerance,
        relative_tolerance: solver_settings.relative_tolerance,
        min_step_norm: solver_settings.absolute_tolerance,
        max_cycle_count,
        line_search_evaluation_cap,
        evaluation_budget: solver_settings
            .max_iterations
            .clamp(minimum_evaluation_budget, 256),
    };

    (outer_cycle_count, powell_settings)
}

fn optimize_single_restart(
    restart_id: &str,
    seed_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementRestartOutcome {
    let mut current_vector = seed_vector.to_vec();
    let mut evaluation_count = 0usize;
    let mut stop_reason = "tolerance";

    let budget_schedule = joint_refinement_budget_schedule(solver_settings);
    let outer_cycle_count = budget_schedule.outer_cycle_count;

    for _ in 0..outer_cycle_count {
        let (updated_hawkes_vector, hawkes_stop_reason, hawkes_evaluations) =
            optimize_parameter_block(
                current_vector.as_slice(),
                context.active_parameter_layout.hawkes_block.as_slice(),
                context,
                budget_schedule.powell_settings,
                solver_settings,
            );

        current_vector = updated_hawkes_vector;
        evaluation_count += hawkes_evaluations;

        let (updated_nig_vector, nig_stop_reason, nig_evaluations) = optimize_parameter_block(
            current_vector.as_slice(),
            context.active_parameter_layout.nig_block.as_slice(),
            context,
            budget_schedule.powell_settings,
            solver_settings,
        );

        current_vector = updated_nig_vector;
        evaluation_count += nig_evaluations;

        let (updated_coupling_vector, coupling_stop_reason, coupling_evaluations) =
            optimize_parameter_block(
                current_vector.as_slice(),
                context.active_parameter_layout.coupling_block.as_slice(),
                context,
                budget_schedule.powell_settings,
                solver_settings,
            );

        current_vector = updated_coupling_vector;
        evaluation_count += coupling_evaluations;

        stop_reason =
            select_restart_stop_reason(hawkes_stop_reason, nig_stop_reason, coupling_stop_reason);
        if stop_reason != "tolerance" {
            break;
        }
    }

    let local_evaluation = evaluate_joint_candidate(
        current_vector.as_slice(),
        context,
        JointRefinementFidelityTier::Local,
        solver_settings,
    );
    let final_evaluation = evaluate_joint_candidate(
        current_vector.as_slice(),
        context,
        JointRefinementFidelityTier::Final,
        solver_settings,
    );

    JointRefinementRestartOutcome {
        restart_id: restart_id.to_string(),
        parameter_vector: current_vector,
        local_evaluation,
        final_evaluation,
        local_evaluation_count: evaluation_count,
        stop_reason,
    }
}

fn select_restart_stop_reason(
    hawkes_stop_reason: &'static str,
    nig_stop_reason: &'static str,
    coupling_stop_reason: &'static str,
) -> &'static str {
    [coupling_stop_reason, nig_stop_reason, hawkes_stop_reason]
        .into_iter()
        .find(|stop_reason| *stop_reason != "tolerance")
        .unwrap_or("tolerance")
}

fn optimize_parameter_block(
    current_vector: &[f64],
    block_indices: &[usize],
    context: &JointRefinementContext,
    optimizer_settings: PowellOptimizerSettings,
    solver_settings: &CalibrationOptimizerSettings,
) -> (Vec<f64>, &'static str, usize) {
    if block_indices.is_empty() {
        return (current_vector.to_vec(), "fixed_block", 0);
    }

    let initial_point: Vec<f64> = block_indices
        .iter()
        .map(|index| current_vector[*index])
        .collect();
    let lower_bounds: Vec<f64> = block_indices
        .iter()
        .map(|index| context.lower_bounds[*index])
        .collect();
    let upper_bounds: Vec<f64> = block_indices
        .iter()
        .map(|index| context.upper_bounds[*index])
        .collect();

    let mut evaluate_block = |block_vector: &[f64]| -> Option<f64> {
        let full_vector = substitute_block(current_vector, block_indices, block_vector);
        Some(
            evaluate_joint_candidate(
                full_vector.as_slice(),
                context,
                JointRefinementFidelityTier::Local,
                solver_settings,
            )
            .breakdown
            .weighted_objective,
        )
    };

    let optimizer_result = optimize_bounded_powell(
        initial_point.as_slice(),
        lower_bounds.as_slice(),
        upper_bounds.as_slice(),
        optimizer_settings,
        &mut evaluate_block,
    );

    match optimizer_result {
        Some(result) => (
            substitute_block(current_vector, block_indices, result.best_point.as_slice()),
            result.stop_reason,
            result.evaluation_count,
        ),
        None => (current_vector.to_vec(), "invalid_step", 0),
    }
}

fn evaluate_joint_candidate(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
    fidelity_tier: JointRefinementFidelityTier,
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementCandidateEvaluation {
    let cache_key = JointRefinementObjectiveCacheKey {
        fidelity_tier,
        parameter_bits: ordered_parameter_bits(parameter_vector),
    };
    if let Ok(cache_guard) = context.objective_cache.lock()
        && let Some(cached_value) = cache_guard.get(&cache_key)
    {
        return cached_value.clone();
    }

    let candidate_evaluation =
        build_joint_candidate_evaluation(parameter_vector, context, fidelity_tier, solver_settings);

    if let Ok(mut cache_guard) = context.objective_cache.lock() {
        cache_guard.insert(cache_key, candidate_evaluation.clone());
    }

    candidate_evaluation
}

fn evaluate_joint_fitted_parameters(
    fitted_parameters: JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
    fidelity_tier: JointRefinementFidelityTier,
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementCandidateEvaluation {
    build_joint_candidate_evaluation_for_fitted_parameters(
        fitted_parameters,
        None,
        context,
        fidelity_tier,
        solver_settings,
    )
}

fn build_joint_candidate_evaluation(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
    fidelity_tier: JointRefinementFidelityTier,
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementCandidateEvaluation {
    let parameter_bits = ordered_parameter_bits(parameter_vector);
    let Some(fitted_parameters) = parameter_vector_to_fitted_parameters(parameter_vector, context)
    else {
        return invalid_candidate_evaluation(
            &context.baseline_hawkes_fitted_parameters,
            &context.baseline_return_components_fitted_parameters,
            solver_settings.constraint_penalty_weight,
            "parameter_vector_to_fitted_parameters_failed",
        );
    };

    build_joint_candidate_evaluation_for_fitted_parameters(
        fitted_parameters,
        Some(parameter_bits.as_slice()),
        context,
        fidelity_tier,
        solver_settings,
    )
}

fn build_joint_candidate_evaluation_for_fitted_parameters(
    fitted_parameters: JointRefinementStageFittedParameters,
    parameter_bits: Option<&[u64]>,
    context: &JointRefinementContext,
    fidelity_tier: JointRefinementFidelityTier,
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementCandidateEvaluation {
    if violates_runtime_seasonality_scaled_stationarity(&fitted_parameters, context) {
        return invalid_candidate_evaluation(
            &context.baseline_hawkes_fitted_parameters,
            &context.baseline_return_components_fitted_parameters,
            solver_settings.constraint_penalty_weight,
            "seasonality_scaled_stationarity_failed",
        );
    }

    let modeled_open_event_rate_per_hour =
        stationary_open_event_rate_per_hour(&fitted_parameters.hawkes_fitted_parameters)
            .unwrap_or(context.observed_data.observed_open_event_rate_per_hour);

    let observed_5m_sample = build_continuous_calibration_sample_from_aggregated_intervals(
        context.observed_data.observed_5m_intervals.as_slice(),
        modeled_open_event_rate_per_hour,
        context.observed_data.observed_open_event_rate_per_hour,
        context.observed_data.fallback_sell_share_proxy,
    );
    let observed_15m_sample = build_continuous_calibration_sample_from_aggregated_intervals(
        context.observed_data.observed_15m_intervals.as_slice(),
        modeled_open_event_rate_per_hour,
        context.observed_data.observed_open_event_rate_per_hour,
        context.observed_data.fallback_sell_share_proxy,
    );
    let likelihood_5m = evaluate_configuration_likelihood_objective(
        fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        &observed_5m_sample,
        ContinuousLikelihoodFidelityTier::Full,
    )
    .shifted_objective_score;
    let likelihood_15m = evaluate_configuration_likelihood_objective(
        fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        &observed_15m_sample,
        ContinuousLikelihoodFidelityTier::Full,
    )
    .shifted_objective_score;
    let raw_aggregated_likelihood_fit = 0.5 * (likelihood_5m + likelihood_15m);

    let simulation_seeds = match fidelity_tier {
        JointRefinementFidelityTier::Local => vec![context.local_simulation_seed],
        JointRefinementFidelityTier::Final => context.final_simulation_seeds.clone(),
    };

    let seed_summaries = with_joint_refinement_thread_pool(context, || {
        simulation_seeds
            .par_iter()
            .map(|simulation_seed| {
                cached_seed_summary_for_candidate_seed(
                    parameter_bits,
                    &fitted_parameters,
                    context,
                    *simulation_seed,
                )
            })
            .collect::<Vec<_>>()
    });
    if seed_summaries.iter().any(Option::is_none) {
        return invalid_candidate_evaluation(
            &context.baseline_hawkes_fitted_parameters,
            &context.baseline_return_components_fitted_parameters,
            solver_settings.constraint_penalty_weight,
            "simulate_seed_summary_failed",
        );
    }

    let mut aggregated_return_shape_scores = Vec::with_capacity(seed_summaries.len());
    let mut aggregated_return_scale_scores = Vec::with_capacity(seed_summaries.len());
    let mut business_time_return_scale_scores = Vec::with_capacity(seed_summaries.len());
    let mut intraday_return_level_scores = Vec::with_capacity(seed_summaries.len());
    let mut activity_shape_scores = Vec::with_capacity(seed_summaries.len());
    let mut activity_level_scores = Vec::with_capacity(seed_summaries.len());
    let mut rv_activity_scores = Vec::with_capacity(seed_summaries.len());
    let mut price_span_scores = Vec::with_capacity(seed_summaries.len());

    for seed_summary in seed_summaries.into_iter().flatten() {
        debug_assert_eq!(
            seed_summary.simulated_5m_intervals.len(),
            seed_summary.simulated_5m_shape_returns.len()
        );
        debug_assert_eq!(
            seed_summary.simulated_15m_intervals.len(),
            seed_summary.simulated_15m_shape_returns.len()
        );

        aggregated_return_shape_scores.push(aggregated_return_shape_fit(
            &seed_summary,
            &context.observed_data,
        ));
        aggregated_return_scale_scores.push(aggregated_return_scale_fit(
            &seed_summary,
            &context.observed_data,
        ));
        business_time_return_scale_scores.push(business_time_return_scale_fit(
            &seed_summary,
            &context.observed_data,
        ));
        intraday_return_level_scores.push(intraday_return_level_fit(
            &seed_summary,
            &context.observed_data,
        ));
        activity_shape_scores.push(activity_shape_fit(&seed_summary, &context.observed_data));
        activity_level_scores.push(activity_level_fit(&seed_summary, &context.observed_data));
        rv_activity_scores.push(rv_activity_coupling_fit(
            seed_summary.simulated_15m_rv_activity_metric,
            context.observed_data.observed_15m_rv_activity_metric,
        ));

        price_span_scores.push(
            (compute_cumulative_log_price_span(seed_summary.simulated_15m_intervals.as_slice())
                - context.observed_data.observed_log_price_span)
                .abs()
                / context.observed_data.observed_log_price_span.max(1.0e-8),
        );
    }

    let raw_aggregated_return_shape_fit = aggregated_return_shape_scores.iter().sum::<f64>()
        / aggregated_return_shape_scores.len().max(1) as f64;
    let raw_aggregated_return_scale_fit = aggregated_return_scale_scores.iter().sum::<f64>()
        / aggregated_return_scale_scores.len().max(1) as f64;
    let raw_business_time_return_scale_fit = business_time_return_scale_scores.iter().sum::<f64>()
        / business_time_return_scale_scores.len().max(1) as f64;
    let raw_intraday_return_level_fit = intraday_return_level_scores.iter().sum::<f64>()
        / intraday_return_level_scores.len().max(1) as f64;
    let raw_activity_shape_fit =
        activity_shape_scores.iter().sum::<f64>() / activity_shape_scores.len().max(1) as f64;
    let raw_activity_level_fit =
        activity_level_scores.iter().sum::<f64>() / activity_level_scores.len().max(1) as f64;
    let raw_rv_activity_coupling_fit =
        rv_activity_scores.iter().sum::<f64>() / rv_activity_scores.len().max(1) as f64;
    let raw_price_span_sanity_fit =
        price_span_scores.iter().sum::<f64>() / price_span_scores.len().max(1) as f64;

    let baseline_buckets = match fidelity_tier {
        JointRefinementFidelityTier::Local => &context.local_baseline_buckets,
        JointRefinementFidelityTier::Final => &context.final_baseline_buckets,
    };
    let normalized_aggregated_likelihood_fit = zero_aware_normalized_bucket_value(
        raw_aggregated_likelihood_fit,
        baseline_buckets.aggregated_likelihood_fit,
        JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
    );
    let normalized_aggregated_return_shape_fit = zero_aware_normalized_bucket_value(
        raw_aggregated_return_shape_fit,
        baseline_buckets.aggregated_return_shape_fit,
        JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
    );
    let normalized_aggregated_return_scale_fit = zero_aware_normalized_bucket_value(
        raw_aggregated_return_scale_fit,
        baseline_buckets.aggregated_return_scale_fit,
        JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
    );
    let normalized_business_time_return_scale_fit = zero_aware_normalized_bucket_value(
        raw_business_time_return_scale_fit,
        baseline_buckets.business_time_return_scale_fit,
        JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
    );
    let normalized_intraday_return_level_fit = zero_aware_normalized_bucket_value(
        raw_intraday_return_level_fit,
        baseline_buckets.intraday_return_level_fit,
        JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
    );
    let normalized_activity_shape_fit = zero_aware_normalized_bucket_value(
        raw_activity_shape_fit,
        baseline_buckets.activity_shape_fit,
        JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
    );
    let normalized_activity_level_fit = zero_aware_normalized_bucket_value(
        raw_activity_level_fit,
        baseline_buckets.activity_level_fit,
        JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
    );
    let normalized_rv_activity_coupling_fit = zero_aware_normalized_bucket_value(
        raw_rv_activity_coupling_fit,
        baseline_buckets.rv_activity_coupling_fit,
        JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
    );
    let normalized_price_span_sanity_fit = zero_aware_normalized_bucket_value(
        raw_price_span_sanity_fit,
        baseline_buckets.price_span_sanity_fit,
        JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
    );

    let hard_constraint_penalty = 0.0;
    let weighted_objective = JOINT_REFINEMENT_WEIGHT_AGGREGATED_LIKELIHOOD_FIT
        * normalized_aggregated_likelihood_fit
        + JOINT_REFINEMENT_WEIGHT_AGGREGATED_RETURN_SHAPE_FIT
            * normalized_aggregated_return_shape_fit
        + JOINT_REFINEMENT_WEIGHT_AGGREGATED_RETURN_SCALE_FIT
            * normalized_aggregated_return_scale_fit
        + JOINT_REFINEMENT_WEIGHT_BUSINESS_TIME_RETURN_SCALE_FIT
            * normalized_business_time_return_scale_fit
        + JOINT_REFINEMENT_WEIGHT_INTRADAY_RETURN_LEVEL_FIT * normalized_intraday_return_level_fit
        + JOINT_REFINEMENT_WEIGHT_ACTIVITY_SHAPE_FIT * normalized_activity_shape_fit
        + JOINT_REFINEMENT_WEIGHT_ACTIVITY_LEVEL_FIT * normalized_activity_level_fit
        + JOINT_REFINEMENT_WEIGHT_RV_ACTIVITY_COUPLING_FIT * normalized_rv_activity_coupling_fit
        + JOINT_REFINEMENT_WEIGHT_PRICE_SPAN_SANITY_FIT * normalized_price_span_sanity_fit
        + hard_constraint_penalty;
    let use_raw_bucket_values = baseline_bucket_values_are_unity(baseline_buckets);

    JointRefinementCandidateEvaluation {
        fitted_parameters,
        breakdown: JointRefinementStageObjectiveBreakdown {
            aggregated_likelihood_fit: if use_raw_bucket_values {
                raw_aggregated_likelihood_fit
            } else {
                normalized_aggregated_likelihood_fit
            },
            aggregated_return_shape_fit: if use_raw_bucket_values {
                raw_aggregated_return_shape_fit
            } else {
                normalized_aggregated_return_shape_fit
            },
            aggregated_return_scale_fit: if use_raw_bucket_values {
                raw_aggregated_return_scale_fit
            } else {
                normalized_aggregated_return_scale_fit
            },
            business_time_return_scale_fit: if use_raw_bucket_values {
                raw_business_time_return_scale_fit
            } else {
                normalized_business_time_return_scale_fit
            },
            intraday_return_level_fit: if use_raw_bucket_values {
                raw_intraday_return_level_fit
            } else {
                normalized_intraday_return_level_fit
            },
            activity_shape_fit: if use_raw_bucket_values {
                raw_activity_shape_fit
            } else {
                normalized_activity_shape_fit
            },
            activity_level_fit: if use_raw_bucket_values {
                raw_activity_level_fit
            } else {
                normalized_activity_level_fit
            },
            rv_activity_coupling_fit: if use_raw_bucket_values {
                raw_rv_activity_coupling_fit
            } else {
                normalized_rv_activity_coupling_fit
            },
            price_span_sanity_fit: if use_raw_bucket_values {
                raw_price_span_sanity_fit
            } else {
                normalized_price_span_sanity_fit
            },
            weighted_objective,
            hard_constraint_penalty,
        },
        invalid_reason: None,
    }
}

fn invalid_candidate_evaluation(
    baseline_hawkes_fitted_parameters: &HawkesStageFittedParameters,
    baseline_return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
    constraint_penalty_weight: f64,
    reason: &str,
) -> JointRefinementCandidateEvaluation {
    JointRefinementCandidateEvaluation {
        fitted_parameters: JointRefinementStageFittedParameters {
            hawkes_fitted_parameters: baseline_hawkes_fitted_parameters.clone(),
            return_components_fitted_parameters: baseline_return_components_fitted_parameters
                .clone(),
        },
        breakdown: JointRefinementStageObjectiveBreakdown {
            aggregated_likelihood_fit: 100.0,
            aggregated_return_shape_fit: 100.0,
            aggregated_return_scale_fit: 100.0,
            business_time_return_scale_fit: 100.0,
            intraday_return_level_fit: 100.0,
            activity_shape_fit: 100.0,
            activity_level_fit: 100.0,
            rv_activity_coupling_fit: 100.0,
            price_span_sanity_fit: 100.0,
            weighted_objective: 100.0 + constraint_penalty_weight,
            hard_constraint_penalty: constraint_penalty_weight,
        },
        invalid_reason: Some(reason.to_string()),
    }
}

fn baseline_bucket_values_are_unity(baseline_buckets: &JointRefinementBaselineBuckets) -> bool {
    baseline_buckets.aggregated_likelihood_fit == 1.0
        && baseline_buckets.aggregated_return_shape_fit == 1.0
        && baseline_buckets.aggregated_return_scale_fit == 1.0
        && baseline_buckets.business_time_return_scale_fit == 1.0
        && baseline_buckets.intraday_return_level_fit == 1.0
        && baseline_buckets.activity_shape_fit == 1.0
        && baseline_buckets.activity_level_fit == 1.0
        && baseline_buckets.rv_activity_coupling_fit == 1.0
        && baseline_buckets.price_span_sanity_fit == 1.0
}

fn aggregated_return_shape_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let shape_5m = distribution_profile_fit_score(
        seed_summary.simulated_5m_shape_returns.as_slice(),
        observed_data.observed_5m_shape_returns.as_slice(),
    )
    .profile_fit_score;
    let shape_15m = distribution_profile_fit_score(
        seed_summary.simulated_15m_shape_returns.as_slice(),
        observed_data.observed_15m_shape_returns.as_slice(),
    )
    .profile_fit_score;

    0.4 * shape_5m + 0.6 * shape_15m
}

fn aggregated_return_scale_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let fit_15m = return_scale_horizon_fit(
        seed_summary.simulated_15m_return_scale_summary,
        observed_data.observed_15m_return_scale_summary,
    );
    let fit_5m = return_scale_horizon_fit(
        seed_summary.simulated_5m_return_scale_summary,
        observed_data.observed_5m_return_scale_summary,
    );

    0.75 * fit_15m + 0.25 * fit_5m
}

fn business_time_return_scale_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let fit_15m = business_time_bucket_return_scale_fit(
        seed_summary.simulated_15m_business_time_return_scale_summaries,
        observed_data.observed_15m_business_time_return_scale_summaries,
    );
    let fit_5m = business_time_bucket_return_scale_fit(
        seed_summary.simulated_5m_business_time_return_scale_summaries,
        observed_data.observed_5m_business_time_return_scale_summaries,
    );

    0.75 * fit_15m + 0.25 * fit_5m
}

fn intraday_return_level_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let fit_15m = intraday_return_level_horizon_fit(
        seed_summary
            .simulated_15m_intraday_mean_absolute_returns
            .as_slice(),
        observed_data
            .observed_15m_intraday_mean_absolute_returns
            .as_slice(),
    );
    let fit_5m = intraday_return_level_horizon_fit(
        seed_summary
            .simulated_5m_intraday_mean_absolute_returns
            .as_slice(),
        observed_data
            .observed_5m_intraday_mean_absolute_returns
            .as_slice(),
    );

    0.75 * fit_15m + 0.25 * fit_5m
}

fn return_scale_horizon_fit(
    candidate_summary: JointRefinementReturnScaleSummary,
    observed_summary: JointRefinementReturnScaleSummary,
) -> f64 {
    let mean_abs_fit = log_ratio_distance(
        candidate_summary.mean_absolute_return,
        observed_summary.mean_absolute_return,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );
    let realized_variance_fit = log_ratio_distance(
        candidate_summary.realized_variance,
        observed_summary.realized_variance,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );
    let tail_fit = log_ratio_distance(
        candidate_summary.absolute_return_quantile_q90,
        observed_summary.absolute_return_quantile_q90,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );

    (mean_abs_fit + realized_variance_fit + tail_fit) / 3.0
}

fn business_time_bucket_return_scale_fit(
    candidate_summaries: JointRefinementBusinessTimeBucketReturnScaleSummaries,
    observed_summaries: JointRefinementBusinessTimeBucketReturnScaleSummaries,
) -> f64 {
    candidate_summaries
        .bucket_summaries
        .iter()
        .zip(observed_summaries.bucket_summaries.iter())
        .map(
            |(candidate_summary, observed_summary)| match (candidate_summary, observed_summary) {
                (Some(candidate_summary), Some(observed_summary)) => {
                    return_scale_horizon_fit(*candidate_summary, *observed_summary)
                }
                (None, Some(_)) => JOINT_REFINEMENT_MISSING_BUSINESS_TIME_BUCKET_PENALTY,
                (Some(_), None) => JOINT_REFINEMENT_MISSING_BUSINESS_TIME_BUCKET_PENALTY,
                (None, None) => 0.0,
            },
        )
        .sum::<f64>()
        / candidate_summaries.bucket_summaries.len() as f64
}

fn intraday_return_level_horizon_fit(candidate_profile: &[f64], observed_profile: &[f64]) -> f64 {
    let candidate_total = candidate_profile.iter().sum::<f64>();
    let observed_total = observed_profile.iter().sum::<f64>();
    let candidate_peak = candidate_profile.iter().copied().fold(0.0, f64::max);
    let observed_peak = observed_profile.iter().copied().fold(0.0, f64::max);
    let observed_mean = observed_total / observed_profile.len().max(1) as f64;

    let path_fit = relative_rmse(
        candidate_profile,
        observed_profile,
        observed_mean.max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR),
    );
    let total_fit = log_ratio_distance(
        candidate_total,
        observed_total,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );
    let peak_fit = log_ratio_distance(
        candidate_peak,
        observed_peak,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );

    0.6 * path_fit + 0.2 * total_fit + 0.2 * peak_fit
}

fn activity_shape_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let profile_fit = normalized_profile_distance(
        seed_summary.simulated_hourly_tick_counts.as_slice(),
        observed_data.observed_hourly_tick_counts.as_slice(),
    );
    let path_fit = standardized_path_distance(
        seed_summary.simulated_hourly_tick_counts.as_slice(),
        observed_data.observed_hourly_tick_counts.as_slice(),
    );

    0.7 * profile_fit + 0.3 * path_fit
}

fn activity_level_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let candidate_counts = seed_summary.simulated_hourly_tick_counts.as_slice();
    let observed_counts = observed_data.observed_hourly_tick_counts.as_slice();

    let candidate_total = candidate_counts.iter().sum::<f64>();
    let observed_total = observed_counts.iter().sum::<f64>();
    let candidate_mean = candidate_total / candidate_counts.len().max(1) as f64;
    let observed_mean = observed_total / observed_counts.len().max(1) as f64;
    let candidate_peak = candidate_counts.iter().copied().fold(0.0, f64::max);
    let observed_peak = observed_counts.iter().copied().fold(0.0, f64::max);

    let total_fit = log_ratio_distance(
        candidate_total,
        observed_total,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );
    let mean_fit = log_ratio_distance(
        candidate_mean,
        observed_mean,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );
    let peak_fit = log_ratio_distance(
        candidate_peak,
        observed_peak,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );
    let path_fit = relative_rmse(
        candidate_counts,
        observed_counts,
        observed_mean.max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR),
    );

    0.30 * total_fit + 0.20 * mean_fit + 0.20 * peak_fit + 0.30 * path_fit
}

fn rv_activity_coupling_fit(
    candidate_metric: JointRefinementRollingVarianceActivityMetric,
    observed_metric: JointRefinementRollingVarianceActivityMetric,
) -> f64 {
    0.6 * (candidate_metric.correlation - observed_metric.correlation).abs()
        + 0.4
            * (candidate_metric.standardized_path_distance
                - observed_metric.standardized_path_distance)
                .abs()
}

fn violates_runtime_seasonality_scaled_stationarity(
    fitted_parameters: &JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
) -> bool {
    let Ok(stability_check) = HawkesKernelParameters::check_spectral_radius_stability(
        fitted_parameters
            .hawkes_fitted_parameters
            .kernel_parameters
            .beta,
        fitted_parameters
            .hawkes_fitted_parameters
            .kernel_parameters
            .alpha,
        fitted_parameters
            .hawkes_fitted_parameters
            .stability_numeric_tolerance,
    ) else {
        return true;
    };

    context.max_runtime_hourly_seasonality_factor * stability_check.spectral_radius_alpha_over_beta
        >= 1.0
}

fn parameter_vector_to_fitted_parameters(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Option<JointRefinementStageFittedParameters> {
    let mut structured_alpha = context.baseline_hawkes_fitted_parameters.structured_alpha;
    let mut news_process_configuration = context
        .baseline_return_components_fitted_parameters
        .news_process_configuration
        .clone();

    let lambda_scale =
        parameter_vector[context.active_parameter_layout.log_lambda_scale_index].exp();
    let beta_scale = parameter_vector[context.active_parameter_layout.log_beta_scale_index].exp();
    let self_excitation_scale = parameter_vector[context
        .active_parameter_layout
        .log_self_excitation_scale_index]
        .exp();
    let cross_excitation_scale = parameter_vector[context
        .active_parameter_layout
        .log_cross_excitation_scale_index]
        .exp();

    structured_alpha.a_self *= self_excitation_scale;
    structured_alpha.delta_self *= self_excitation_scale;
    structured_alpha.a_cross *= cross_excitation_scale;
    structured_alpha.delta_cross *= cross_excitation_scale;

    let alpha_loadings = HawkesKernelLoadings::try_from_structured_alpha(structured_alpha).ok()?;
    let rho_loadings = HawkesKernelLoadings::try_from_structured_rho(
        context.baseline_hawkes_fitted_parameters.structured_rho,
        context
            .baseline_hawkes_fitted_parameters
            .self_dominance_policy,
    )
    .ok()?;
    let lambda_bar = context
        .baseline_hawkes_fitted_parameters
        .kernel_parameters
        .lambda_bar
        * lambda_scale;
    let beta = context
        .baseline_hawkes_fitted_parameters
        .kernel_parameters
        .beta
        * beta_scale;
    let kernel_parameters =
        HawkesKernelParameters::try_new(lambda_bar, beta, alpha_loadings, rho_loadings).ok()?;
    let hawkes_fitted_parameters = HawkesStageFittedParameters {
        kernel_parameters,
        structured_alpha,
        structured_rho: context.baseline_hawkes_fitted_parameters.structured_rho,
        self_dominance_policy: context
            .baseline_hawkes_fitted_parameters
            .self_dominance_policy,
        stability_numeric_tolerance: context
            .baseline_hawkes_fitted_parameters
            .stability_numeric_tolerance,
    };
    let modeled_open_event_rate_per_hour =
        stationary_open_event_rate_per_hour(&hawkes_fitted_parameters)?;
    let aggregated_15m_sample = build_continuous_calibration_sample_from_aggregated_intervals(
        context.observed_data.observed_15m_intervals.as_slice(),
        modeled_open_event_rate_per_hour,
        context.observed_data.observed_open_event_rate_per_hour,
        context.observed_data.fallback_sell_share_proxy,
    );
    let nig_search_point = NigShapeSearchPoint {
        log_alpha: parameter_vector[context.active_parameter_layout.nig_block[0]],
        buy_skew_coordinate: parameter_vector[context.active_parameter_layout.nig_block[1]],
        sell_relative_coordinate: parameter_vector[context.active_parameter_layout.nig_block[2]],
        log_delta_offset: parameter_vector[context.active_parameter_layout.nig_block[3]],
    };
    let nig_shared_plus_shift_configuration =
        nig_configuration_from_search_point(&aggregated_15m_sample, nig_search_point)?;

    if let Some(index) = context.active_parameter_layout.known_activity_scale_index {
        news_process_configuration
            .known_component_mark_scales
            .activity_coupling_scale *= parameter_vector[index].exp();
    }
    if let Some(index) = context
        .active_parameter_layout
        .surprise_activity_scale_index
    {
        news_process_configuration
            .surprise_component_mark_scales
            .activity_coupling_scale *= parameter_vector[index].exp();
    }
    if let Some(index) = context
        .active_parameter_layout
        .surprise_baseline_scale_index
    {
        news_process_configuration.arrivals.surprise = SurpriseNewsArrivalConfiguration {
            baseline_intensity: news_process_configuration
                .arrivals
                .surprise
                .baseline_intensity
                * parameter_vector[index].exp(),
        };
    }
    let return_components_fitted_parameters = ReturnComponentsStageFittedParameters {
        nig_shared_plus_shift_configuration,
        news_process_configuration,
        surprise_deterministic_intensity_profile: context
            .baseline_return_components_fitted_parameters
            .surprise_deterministic_intensity_profile
            .clone(),
    };
    return_components_fitted_parameters
        .news_process_configuration
        .validate()
        .ok()?;

    Some(JointRefinementStageFittedParameters {
        hawkes_fitted_parameters,
        return_components_fitted_parameters,
    })
}

fn build_observed_data(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    observed_open_event_rate_per_hour: f64,
) -> JointRefinementObservedData {
    let source_window = candidate_generation_input.known_news_calendar.source_window;

    let observed_5m_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        JOINT_REFINEMENT_FIVE_MINUTES_NANOS,
    );

    let observed_15m_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        JOINT_REFINEMENT_FIFTEEN_MINUTES_NANOS,
    );

    let observed_hourly_intervals = build_observed_open_session_interval_observations(
        calendar_registry,
        exchange_code,
        source_window,
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        JOINT_REFINEMENT_ONE_HOUR_NANOS,
    );

    let observed_15m_rv_activity_metric =
        rolling_variance_activity_metric(observed_15m_intervals.as_slice());

    let observed_log_price_span =
        compute_cumulative_log_price_span(observed_15m_intervals.as_slice());

    let observed_5m_log_returns = observed_5m_intervals
        .iter()
        .map(|interval| interval.log_return)
        .collect::<Vec<_>>();
    let observed_15m_log_returns = observed_15m_intervals
        .iter()
        .map(|interval| interval.log_return)
        .collect::<Vec<_>>();

    let observed_5m_shape_returns = scale_normalized_returns(observed_5m_log_returns.as_slice());
    let observed_15m_shape_returns = scale_normalized_returns(observed_15m_log_returns.as_slice());
    let observed_5m_return_scale_summary = return_scale_summary(observed_5m_intervals.as_slice());
    let observed_15m_return_scale_summary = return_scale_summary(observed_15m_intervals.as_slice());

    let observed_5m_business_time_bucket_thresholds = business_time_bucket_thresholds(
        observed_5m_intervals.as_slice(),
    )
    .unwrap_or(JointRefinementBusinessTimeBucketThresholds {
        lower_quartile_cutoff: 0.0,
        median_cutoff: 0.0,
        upper_quartile_cutoff: 0.0,
    });

    let observed_15m_business_time_bucket_thresholds = business_time_bucket_thresholds(
        observed_15m_intervals.as_slice(),
    )
    .unwrap_or(JointRefinementBusinessTimeBucketThresholds {
        lower_quartile_cutoff: 0.0,
        median_cutoff: 0.0,
        upper_quartile_cutoff: 0.0,
    });

    let observed_5m_business_time_return_scale_summaries =
        return_scale_summaries_by_business_time_bucket(
            observed_5m_intervals.as_slice(),
            observed_5m_business_time_bucket_thresholds,
        );
    let observed_15m_business_time_return_scale_summaries =
        return_scale_summaries_by_business_time_bucket(
            observed_15m_intervals.as_slice(),
            observed_15m_business_time_bucket_thresholds,
        );

    let observed_5m_intraday_mean_absolute_returns =
        mean_absolute_return_by_utc_hour(observed_5m_intervals.as_slice());
    let observed_15m_intraday_mean_absolute_returns =
        mean_absolute_return_by_utc_hour(observed_15m_intervals.as_slice());

    let observed_hourly_tick_counts = tick_count_series(observed_hourly_intervals.as_slice());

    let hourly_simulation_start_utc =
        DateTime::<Utc>::from_timestamp_nanos(floor_to_hour(source_window.window_start_utc_nanos));
    let hourly_simulation_end_utc =
        DateTime::<Utc>::from_timestamp_nanos(ceil_to_hour(source_window.window_end_utc_nanos));

    let hourly_simulation_interval_count = ((hourly_simulation_end_utc
        .timestamp_nanos_opt()
        .unwrap_or(source_window.window_end_utc_nanos)
        - hourly_simulation_start_utc
            .timestamp_nanos_opt()
            .unwrap_or(source_window.window_start_utc_nanos))
    .div_euclid(JOINT_REFINEMENT_ONE_HOUR_NANOS))
        as usize;

    let surprise_multiplier_by_hour_index = build_surprise_multipliers_by_hour_index(
        hourly_simulation_start_utc,
        hourly_simulation_interval_count,
        return_components_fitted_parameters,
    );

    let simulated_5m_interval_template =
        zeroed_interval_observation_template(observed_5m_intervals.as_slice());
    let simulated_15m_interval_template =
        zeroed_interval_observation_template(observed_15m_intervals.as_slice());
    let simulated_hourly_interval_template =
        zeroed_interval_observation_template(observed_hourly_intervals.as_slice());

    JointRefinementObservedData {
        observed_open_event_rate_per_hour,
        fallback_sell_share_proxy: candidate_generation_input.open_session_sell_share_proxy,
        observed_5m_intervals,
        observed_15m_intervals,
        observed_5m_shape_returns,
        observed_15m_shape_returns,
        observed_5m_return_scale_summary,
        observed_15m_return_scale_summary,
        observed_5m_business_time_bucket_thresholds,
        observed_15m_business_time_bucket_thresholds,
        observed_5m_business_time_return_scale_summaries,
        observed_15m_business_time_return_scale_summaries,
        observed_5m_intraday_mean_absolute_returns,
        observed_15m_intraday_mean_absolute_returns,
        observed_hourly_tick_counts,
        observed_15m_rv_activity_metric,
        observed_log_price_span,
        hourly_simulation_start_utc,
        hourly_simulation_end_utc,
        hourly_simulation_interval_count,
        surprise_weekly_hour_utc_multipliers: surprise_multiplier_by_hour_index
            .into_iter()
            .map(|(_, multiplier)| multiplier)
            .collect(),
        simulated_5m_interval_template,
        simulated_15m_interval_template,
        simulated_hourly_interval_template,
    }
}

fn build_baseline_parameter_vector(
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    hawkes_fitted_parameters: &HawkesStageFittedParameters,
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> Vec<f64> {
    let mut parameter_vector = vec![0.0; active_parameter_layout.vector_length()];
    parameter_vector[active_parameter_layout.log_lambda_scale_index] = 0.0;
    parameter_vector[active_parameter_layout.log_beta_scale_index] = 0.0;
    parameter_vector[active_parameter_layout.log_self_excitation_scale_index] = 0.0;
    parameter_vector[active_parameter_layout.log_cross_excitation_scale_index] = 0.0;

    if let Some(search_point) = nig_search_point_from_configuration(
        baseline_15m_calibration_sample,
        return_components_fitted_parameters.nig_shared_plus_shift_configuration,
    ) {
        parameter_vector[active_parameter_layout.nig_block[0]] = search_point.log_alpha;
        parameter_vector[active_parameter_layout.nig_block[1]] = search_point.buy_skew_coordinate;
        parameter_vector[active_parameter_layout.nig_block[2]] =
            search_point.sell_relative_coordinate;
        parameter_vector[active_parameter_layout.nig_block[3]] = search_point.log_delta_offset;
    }

    if let Some(index) = active_parameter_layout.known_activity_scale_index {
        let _ = hawkes_fitted_parameters;
        parameter_vector[index] = 0.0;
    }
    if let Some(index) = active_parameter_layout.surprise_activity_scale_index {
        parameter_vector[index] = 0.0;
    }
    if let Some(index) = active_parameter_layout.surprise_baseline_scale_index {
        parameter_vector[index] = 0.0;
    }

    parameter_vector
}

fn build_active_parameter_layout(
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
) -> JointRefinementActiveParameterLayout {
    let mut next_index = 0usize;
    let log_lambda_scale_index = next_index;
    next_index += 1;
    let log_beta_scale_index = next_index;
    next_index += 1;
    let log_self_excitation_scale_index = next_index;
    next_index += 1;
    let log_cross_excitation_scale_index = next_index;
    next_index += 1;

    let hawkes_block = vec![
        log_lambda_scale_index,
        log_beta_scale_index,
        log_self_excitation_scale_index,
        log_cross_excitation_scale_index,
    ];

    let nig_block = vec![next_index, next_index + 1, next_index + 2, next_index + 3];
    next_index += 4;

    let known_activity_scale_index = (return_components_fitted_parameters
        .news_process_configuration
        .known_component_mark_scales
        .activity_coupling_scale
        > 0.0)
        .then_some(next_index);
    if known_activity_scale_index.is_some() {
        next_index += 1;
    }

    let surprise_activity_scale_index = (return_components_fitted_parameters
        .news_process_configuration
        .surprise_component_mark_scales
        .activity_coupling_scale
        > 0.0)
        .then_some(next_index);
    if surprise_activity_scale_index.is_some() {
        next_index += 1;
    }

    let surprise_baseline_scale_index = (return_components_fitted_parameters
        .news_process_configuration
        .arrivals
        .surprise
        .baseline_intensity
        > 0.0)
        .then_some(next_index);

    let mut coupling_block = Vec::new();
    if let Some(index) = known_activity_scale_index {
        coupling_block.push(index);
    }
    if let Some(index) = surprise_activity_scale_index {
        coupling_block.push(index);
    }
    if let Some(index) = surprise_baseline_scale_index {
        coupling_block.push(index);
    }

    JointRefinementActiveParameterLayout {
        hawkes_block,
        nig_block,
        coupling_block,
        log_lambda_scale_index,
        log_beta_scale_index,
        log_self_excitation_scale_index,
        log_cross_excitation_scale_index,
        known_activity_scale_index,
        surprise_activity_scale_index,
        surprise_baseline_scale_index,
    }
}

impl JointRefinementActiveParameterLayout {
    fn vector_length(&self) -> usize {
        self.coupling_block
            .iter()
            .copied()
            .chain(self.nig_block.iter().copied())
            .chain(self.hawkes_block.iter().copied())
            .max()
            .map(|index| index + 1)
            .unwrap_or(0)
    }
}

fn build_parameter_bounds(
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    baseline_parameter_vector: &[f64],
) -> (Vec<f64>, Vec<f64>) {
    let mut lower_bounds = vec![0.0; baseline_parameter_vector.len()];
    let mut upper_bounds = vec![0.0; baseline_parameter_vector.len()];

    for index in &active_parameter_layout.hawkes_block {
        lower_bounds[*index] = -JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND;
        upper_bounds[*index] = JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND;
    }
    lower_bounds[active_parameter_layout.nig_block[0]] = baseline_parameter_vector
        [active_parameter_layout.nig_block[0]]
        - JOINT_REFINEMENT_NIG_LOG_ALPHA_HALF_SPAN;
    upper_bounds[active_parameter_layout.nig_block[0]] = baseline_parameter_vector
        [active_parameter_layout.nig_block[0]]
        + JOINT_REFINEMENT_NIG_LOG_ALPHA_HALF_SPAN;
    lower_bounds[active_parameter_layout.nig_block[1]] = baseline_parameter_vector
        [active_parameter_layout.nig_block[1]]
        - JOINT_REFINEMENT_NIG_SKEW_HALF_SPAN;
    upper_bounds[active_parameter_layout.nig_block[1]] = baseline_parameter_vector
        [active_parameter_layout.nig_block[1]]
        + JOINT_REFINEMENT_NIG_SKEW_HALF_SPAN;
    lower_bounds[active_parameter_layout.nig_block[2]] = baseline_parameter_vector
        [active_parameter_layout.nig_block[2]]
        - JOINT_REFINEMENT_NIG_SKEW_HALF_SPAN;
    upper_bounds[active_parameter_layout.nig_block[2]] = baseline_parameter_vector
        [active_parameter_layout.nig_block[2]]
        + JOINT_REFINEMENT_NIG_SKEW_HALF_SPAN;
    lower_bounds[active_parameter_layout.nig_block[3]] = baseline_parameter_vector
        [active_parameter_layout.nig_block[3]]
        - JOINT_REFINEMENT_NIG_LOG_DELTA_HALF_SPAN;
    upper_bounds[active_parameter_layout.nig_block[3]] = baseline_parameter_vector
        [active_parameter_layout.nig_block[3]]
        + JOINT_REFINEMENT_NIG_LOG_DELTA_HALF_SPAN;

    for index in &active_parameter_layout.coupling_block {
        lower_bounds[*index] = -JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND;
        upper_bounds[*index] = JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND;
    }

    (lower_bounds, upper_bounds)
}

fn build_restart_seeds(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    restart_count: usize,
) -> Vec<(String, Vec<f64>)> {
    let mut baseline = baseline_parameter_vector.to_vec();
    clamp_in_place(baseline.as_mut_slice(), lower_bounds, upper_bounds);

    let mut higher_activity = baseline_parameter_vector.to_vec();
    higher_activity[active_parameter_layout.log_lambda_scale_index] += 0.35;
    higher_activity[active_parameter_layout.log_beta_scale_index] -= 0.20;
    higher_activity[active_parameter_layout.log_self_excitation_scale_index] += 0.20;
    higher_activity[active_parameter_layout.log_cross_excitation_scale_index] += 0.20;

    if let Some(index) = active_parameter_layout.known_activity_scale_index {
        higher_activity[index] += 0.15;
    }
    if let Some(index) = active_parameter_layout.surprise_activity_scale_index {
        higher_activity[index] += 0.15;
    }

    clamp_in_place(higher_activity.as_mut_slice(), lower_bounds, upper_bounds);

    let mut lower_activity_stronger_move = baseline_parameter_vector.to_vec();
    lower_activity_stronger_move[active_parameter_layout.log_lambda_scale_index] -= 0.35;
    lower_activity_stronger_move[active_parameter_layout.log_self_excitation_scale_index] -= 0.20;
    lower_activity_stronger_move[active_parameter_layout.log_cross_excitation_scale_index] -= 0.20;
    lower_activity_stronger_move[active_parameter_layout.nig_block[0]] -= 0.35;
    lower_activity_stronger_move[active_parameter_layout.nig_block[3]] += 0.35;

    clamp_in_place(
        lower_activity_stronger_move.as_mut_slice(),
        lower_bounds,
        upper_bounds,
    );

    let mut restart_candidates = vec![
        ("baseline".to_string(), baseline),
        ("higher_activity".to_string(), higher_activity),
        (
            "lower_activity_stronger_move".to_string(),
            lower_activity_stronger_move,
        ),
    ];

    let target_restart_count = restart_count.max(1).min(JOINT_REFINEMENT_MAX_RESTART_COUNT);
    let mut halton_index = 1u64;
    let mut halton_position = 1usize;
    while restart_candidates.len() < target_restart_count {
        let mut halton_seed = halton_parameter_vector(halton_index, lower_bounds, upper_bounds);
        if halton_seed.len() >= baseline_parameter_vector.len() {
            halton_seed[active_parameter_layout.log_lambda_scale_index] =
                halton_seed[active_parameter_layout.log_lambda_scale_index].clamp(
                    lower_bounds[active_parameter_layout.log_lambda_scale_index],
                    upper_bounds[active_parameter_layout.log_lambda_scale_index],
                );
        }

        clamp_in_place(halton_seed.as_mut_slice(), lower_bounds, upper_bounds);

        restart_candidates.push((build_halton_restart_id(halton_position), halton_seed));

        halton_index += 1;
        halton_position += 1;
    }

    deduplicate_restart_seed_candidates(restart_candidates)
        .into_iter()
        .take(target_restart_count)
        .collect()
}

fn joint_refinement_final_simulation_seed_count(
    solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    match solver_settings.max_iterations {
        0..=32 => 1,
        33..=96 => 2,
        _ => 3,
    }
}

fn cached_seed_summary_for_candidate_seed(
    parameter_bits: Option<&[u64]>,
    fitted_parameters: &JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
    simulation_seed: u64,
) -> Option<JointRefinementSeedSummary> {
    let cache_key = parameter_bits.map(|bits| JointRefinementSeedSummaryCacheKey {
        parameter_bits: bits.to_vec(),
        simulation_seed,
    });
    if let Some(seed_cache_key) = cache_key.as_ref()
        && let Ok(cache_guard) = context.seed_summary_cache.lock()
        && let Some(cached_summary) = cache_guard.get(seed_cache_key)
    {
        return cached_summary.clone();
    }

    let seed_summary = build_seed_summary_for_candidate_seed(
        &context.calendar_registry,
        context,
        fitted_parameters,
        simulation_seed,
    );
    if let Some(seed_cache_key) = cache_key
        && let Ok(mut cache_guard) = context.seed_summary_cache.lock()
    {
        cache_guard.insert(seed_cache_key, seed_summary.clone());
    }

    seed_summary
}

fn build_seed_summary_for_candidate_seed(
    calendar_registry: &ExchangeCalendarRegistry,
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
) -> Option<JointRefinementSeedSummary> {
    let surprise_deterministic_time_multiplier_by_interval_index =
        build_surprise_multipliers_for_interval_grid(
            context.observed_data.hourly_simulation_start_utc,
            context.observed_data.hourly_simulation_interval_count,
            JOINT_REFINEMENT_ONE_HOUR_NANOS,
            context
                .observed_data
                .surprise_weekly_hour_utc_multipliers
                .as_slice(),
        );
    let hourly_factor_contract = load_hourly_factor_contract_for_calibration(
        calendar_registry,
        HourlyFactorProviderRequest {
            exchange_code: context.exchange_code.as_str(),
            range_start_utc: context.observed_data.hourly_simulation_start_utc,
            range_end_utc: context.observed_data.hourly_simulation_end_utc,
            interval_duration_nanos: JOINT_REFINEMENT_ONE_HOUR_NANOS,
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: context.runtime_hourly_seasonality_factors.as_slice(),
                materialization_method:
                    "runtime_compact_seasonality_parameters_mean_renormalized.v1",
            }),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Materialized {
                known_scheduled_event_timestamps_utc_nanos: fitted_parameters
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .known
                    .scheduled_event_timestamps_utc_nanos
                    .as_slice(),
                surprise_baseline_intensity: fitted_parameters
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .surprise
                    .baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index:
                    surprise_deterministic_time_multiplier_by_interval_index.as_slice(),
            }),
        },
    )
    .ok()?;

    let simulation_result = run_seeded_tick_event_simulation(
        SeededTickSimulationRunConfig::try_new(
            simulation_seed,
            context.observed_data.hourly_simulation_start_utc,
            context.observed_data.hourly_simulation_interval_count,
            SeededTickSimulationModelInputs {
                kernel_parameters: fitted_parameters.hawkes_fitted_parameters.kernel_parameters,
                nig_shared_plus_shift_configuration: fitted_parameters
                    .return_components_fitted_parameters
                    .nig_shared_plus_shift_configuration,
                news_process_configuration: fitted_parameters
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .clone(),
                initial_excitation_state: HawkesExcitationState {
                    y_plus_plus: 0.0,
                    y_plus_minus: 0.0,
                    y_minus_plus: 0.0,
                    y_minus_minus: 0.0,
                },
                hourly_factor_contract,
                calendar_registry: calendar_registry.clone(),
            },
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: false,
                debug_trace_options: Default::default(),
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .ok()?,
    )
    .ok()?;

    let (simulated_5m_intervals, simulated_15m_intervals) =
        aggregate_simulated_path_to_interval_templates(
            simulation_result.fine_path.as_slice(),
            context
                .observed_data
                .simulated_5m_interval_template
                .as_slice(),
            context
                .observed_data
                .simulated_15m_interval_template
                .as_slice(),
            JOINT_REFINEMENT_FIVE_MINUTES_NANOS,
            JOINT_REFINEMENT_FIFTEEN_MINUTES_NANOS,
        );

    let simulated_hourly_intervals = aggregate_simulated_path_to_interval_template(
        simulation_result.fine_path.as_slice(),
        context
            .observed_data
            .simulated_hourly_interval_template
            .as_slice(),
        JOINT_REFINEMENT_ONE_HOUR_NANOS,
    );

    let simulated_5m_log_returns = simulated_5m_intervals
        .iter()
        .map(|interval| interval.log_return)
        .collect::<Vec<_>>();
    let simulated_15m_log_returns = simulated_15m_intervals
        .iter()
        .map(|interval| interval.log_return)
        .collect::<Vec<_>>();

    let simulated_5m_shape_returns = scale_normalized_returns(simulated_5m_log_returns.as_slice());
    let simulated_15m_shape_returns =
        scale_normalized_returns(simulated_15m_log_returns.as_slice());

    let simulated_5m_return_scale_summary = return_scale_summary(simulated_5m_intervals.as_slice());
    let simulated_15m_return_scale_summary =
        return_scale_summary(simulated_15m_intervals.as_slice());

    let simulated_5m_business_time_return_scale_summaries =
        return_scale_summaries_by_business_time_bucket(
            simulated_5m_intervals.as_slice(),
            context
                .observed_data
                .observed_5m_business_time_bucket_thresholds,
        );
    let simulated_15m_business_time_return_scale_summaries =
        return_scale_summaries_by_business_time_bucket(
            simulated_15m_intervals.as_slice(),
            context
                .observed_data
                .observed_15m_business_time_bucket_thresholds,
        );

    let simulated_5m_intraday_mean_absolute_returns =
        mean_absolute_return_by_utc_hour(simulated_5m_intervals.as_slice());
    let simulated_15m_intraday_mean_absolute_returns =
        mean_absolute_return_by_utc_hour(simulated_15m_intervals.as_slice());

    let simulated_hourly_tick_counts = tick_count_series(simulated_hourly_intervals.as_slice());

    let simulated_15m_rv_activity_metric =
        rolling_variance_activity_metric(simulated_15m_intervals.as_slice());

    Some(JointRefinementSeedSummary {
        simulated_5m_intervals,
        simulated_15m_intervals,
        simulated_5m_shape_returns,
        simulated_15m_shape_returns,
        simulated_5m_return_scale_summary,
        simulated_15m_return_scale_summary,
        simulated_5m_business_time_return_scale_summaries,
        simulated_15m_business_time_return_scale_summaries,
        simulated_5m_intraday_mean_absolute_returns,
        simulated_15m_intraday_mean_absolute_returns,
        simulated_hourly_tick_counts,
        simulated_15m_rv_activity_metric,
    })
}

fn selected_hawkes_observed_open_event_rate_per_hour(
    hawkes_payload: &HawkesStageFittedPayload,
) -> Option<f64> {
    hawkes_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == hawkes_payload.selected_candidate_id)
        .map(|candidate| candidate.implied_metrics.observed_open_event_rate_per_hour)
        .or_else(|| stationary_open_event_rate_per_hour(&hawkes_payload.fitted_parameters))
}

fn stage_input_to_activity_input(
    hawkes_input: &JointRefinementStageHawkesInput,
) -> crate::stages::return_components::ReturnComponentsStageActivityInput {
    match hawkes_input {
        JointRefinementStageHawkesInput::InMemoryOutput { payload } => {
            crate::stages::return_components::ReturnComponentsStageActivityInput::InMemoryOutput {
                payload: payload.clone(),
            }
        }
        JointRefinementStageHawkesInput::PersistedArtifactReference {
            artifact_reference,
            payload,
        } => crate::stages::return_components::ReturnComponentsStageActivityInput::PersistedArtifactReference {
            artifact_reference: artifact_reference.clone(),
            payload: payload.clone(),
        },
    }
}

fn substitute_block(
    current_vector: &[f64],
    block_indices: &[usize],
    block_vector: &[f64],
) -> Vec<f64> {
    let mut updated = current_vector.to_vec();
    for (block_position, parameter_index) in block_indices.iter().enumerate() {
        updated[*parameter_index] = block_vector[block_position];
    }

    updated
}

fn clamp_in_place(values: &mut [f64], lower_bounds: &[f64], upper_bounds: &[f64]) {
    for (value, (lower_bound, upper_bound)) in values
        .iter_mut()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
    {
        *value = value.clamp(*lower_bound, *upper_bound);
    }
}

fn compute_cumulative_log_price_span(
    intervals: &[JointRefinementAggregatedIntervalObservation],
) -> f64 {
    let mut cumulative_value: f64 = 0.0;
    let mut minimum_cumulative_value: f64 = 0.0;
    let mut maximum_cumulative_value: f64 = 0.0;

    for interval in intervals {
        cumulative_value += interval.log_return;
        minimum_cumulative_value = minimum_cumulative_value.min(cumulative_value);
        maximum_cumulative_value = maximum_cumulative_value.max(cumulative_value);
    }

    maximum_cumulative_value - minimum_cumulative_value
}

fn build_surprise_multipliers_by_hour_index(
    hourly_simulation_start_utc: DateTime<Utc>,
    hourly_simulation_interval_count: usize,
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
) -> Vec<(u64, f64)> {
    build_surprise_multipliers_for_interval_grid(
        hourly_simulation_start_utc,
        hourly_simulation_interval_count,
        JOINT_REFINEMENT_ONE_HOUR_NANOS,
        return_components_fitted_parameters
            .surprise_deterministic_intensity_profile
            .weekly_hour_utc_multipliers
            .as_slice(),
    )
}

fn build_surprise_multipliers_for_interval_grid(
    simulation_start_utc: DateTime<Utc>,
    interval_count: usize,
    interval_duration_nanos: i64,
    weekly_hour_utc_multipliers: &[f64],
) -> Vec<(u64, f64)> {
    (0..interval_count)
        .map(|interval_index| {
            let interval_start = simulation_start_utc
                .checked_add_signed(Duration::nanoseconds(
                    interval_duration_nanos.saturating_mul(interval_index as i64),
                ))
                .unwrap_or(simulation_start_utc);

            let weekday_index = interval_start.weekday().num_days_from_monday() as usize;
            let hour_index = interval_start.hour() as usize;
            let bucket_index = weekday_index * 24 + hour_index;
            (
                interval_index as u64,
                *weekly_hour_utc_multipliers
                    .get(bucket_index)
                    .unwrap_or(&1.0),
            )
        })
        .collect()
}

fn floor_to_hour(timestamp_utc_nanos: i64) -> i64 {
    timestamp_utc_nanos.div_euclid(JOINT_REFINEMENT_ONE_HOUR_NANOS)
        * JOINT_REFINEMENT_ONE_HOUR_NANOS
}

fn ceil_to_hour(timestamp_utc_nanos: i64) -> i64 {
    let remainder = timestamp_utc_nanos.rem_euclid(JOINT_REFINEMENT_ONE_HOUR_NANOS);
    if remainder == 0 {
        timestamp_utc_nanos
    } else {
        timestamp_utc_nanos.saturating_add(JOINT_REFINEMENT_ONE_HOUR_NANOS - remainder)
    }
}

fn raw_bucket_value_from_normalized(value: f64) -> f64 {
    value
}

fn build_joint_refinement_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    failure: JointRefinementStageExecutionFailure,
) -> JointRefinementStageExecutionReport {
    JointRefinementStageExecutionReport {
        schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::JointRefinement,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        JointRefinementActiveParameterLayout, build_restart_seeds,
        business_time_bucket_return_scale_fit, intraday_return_level_horizon_fit,
        joint_refinement_budget_schedule,
    };
    use crate::stages::joint_metrics::{
        JointRefinementBusinessTimeBucketReturnScaleSummaries, JointRefinementReturnScaleSummary,
    };
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;

    fn build_test_solver_settings(cpu_count: usize) -> CalibrationOptimizerSettings {
        CalibrationOptimizerSettings::try_new("joint-test", 256, cpu_count, 1.0e-6, 1.0e-6, 10.0)
            .expect("test optimizer settings should be valid")
    }

    fn build_test_active_parameter_layout() -> JointRefinementActiveParameterLayout {
        JointRefinementActiveParameterLayout {
            hawkes_block: vec![0, 1, 2, 3],
            nig_block: vec![4, 5, 6, 7],
            coupling_block: vec![8, 9, 10],
            log_lambda_scale_index: 0,
            log_beta_scale_index: 1,
            log_self_excitation_scale_index: 2,
            log_cross_excitation_scale_index: 3,
            known_activity_scale_index: Some(8),
            surprise_activity_scale_index: Some(9),
            surprise_baseline_scale_index: Some(10),
        }
    }

    #[test]
    fn joint_refinement_restart_count_scales_with_cpu_count_and_caps_at_24() {
        let single_core_schedule = joint_refinement_budget_schedule(&build_test_solver_settings(1));
        let eight_core_schedule = joint_refinement_budget_schedule(&build_test_solver_settings(8));
        let high_core_schedule = joint_refinement_budget_schedule(&build_test_solver_settings(64));

        assert_eq!(single_core_schedule.restart_count, 1);
        assert_eq!(eight_core_schedule.restart_count, 8);
        assert_eq!(high_core_schedule.restart_count, 24);
    }

    #[test]
    fn joint_refinement_restart_seed_generation_is_deterministic() {
        let active_parameter_layout = build_test_active_parameter_layout();
        let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];

        let lower_bounds = vec![-1.0; baseline_parameter_vector.len()];
        let upper_bounds = vec![1.0; baseline_parameter_vector.len()];

        let first_restart_seeds = build_restart_seeds(
            baseline_parameter_vector.as_slice(),
            &active_parameter_layout,
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
            8,
        );
        let second_restart_seeds = build_restart_seeds(
            baseline_parameter_vector.as_slice(),
            &active_parameter_layout,
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
            8,
        );

        assert_eq!(first_restart_seeds, second_restart_seeds);
        assert_eq!(first_restart_seeds.len(), 8);
        assert_eq!(first_restart_seeds[0].0, "baseline");
        assert_eq!(first_restart_seeds[1].0, "higher_activity");
        assert_eq!(first_restart_seeds[2].0, "lower_activity_stronger_move");
        assert_eq!(first_restart_seeds[3].0, "halton_01");
    }

    #[test]
    fn business_time_bucket_return_scale_fit_penalizes_oversized_returns_within_same_regimes() {
        let observed_summaries = JointRefinementBusinessTimeBucketReturnScaleSummaries {
            bucket_summaries: [Some(JointRefinementReturnScaleSummary {
                mean_absolute_return: 0.001,
                realized_variance: 1.0e-6,
                absolute_return_quantile_q90: 0.002,
            }); 4],
        };
        let matched_summaries = observed_summaries;
        let amplified_summaries = JointRefinementBusinessTimeBucketReturnScaleSummaries {
            bucket_summaries: [Some(JointRefinementReturnScaleSummary {
                mean_absolute_return: 0.004,
                realized_variance: 1.6e-5,
                absolute_return_quantile_q90: 0.008,
            }); 4],
        };

        let matched_fit =
            business_time_bucket_return_scale_fit(matched_summaries, observed_summaries);
        let amplified_fit =
            business_time_bucket_return_scale_fit(amplified_summaries, observed_summaries);

        assert_eq!(matched_fit, 0.0);
        assert!(amplified_fit > matched_fit);
    }

    #[test]
    fn intraday_return_level_horizon_fit_penalizes_uniformly_oversized_hourly_profiles() {
        let observed_profile = vec![0.001; 24];
        let matched_profile = observed_profile.clone();
        let amplified_profile = vec![0.004; 24];

        let matched_fit = intraday_return_level_horizon_fit(
            matched_profile.as_slice(),
            observed_profile.as_slice(),
        );
        let amplified_fit = intraday_return_level_horizon_fit(
            amplified_profile.as_slice(),
            observed_profile.as_slice(),
        );

        assert_eq!(matched_fit, 0.0);
        assert!(amplified_fit > matched_fit);
    }
}
