use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use rayon::prelude::*;
use serde::Serialize;

use crate::adapters::load_hourly_factor_contract_for_calibration;
use crate::calendar::ExchangeCalendarRegistry;
use crate::optimizer_adapter::CalibrationOptimizerAdapterRunMetadata;
use crate::stages::hawkes::{HawkesStageFittedPayload, stationary_open_event_rate_per_hour};
use crate::stages::joint_metrics::{
    JointRefinementAggregatedIntervalObservation, JointRefinementRollingVarianceActivityMetric,
    build_continuous_calibration_sample_from_aggregated_intervals,
    build_observed_open_session_interval_observations,
    build_simulated_open_session_interval_observations, rolling_variance_activity_metric,
    zero_aware_normalized_bucket_value,
};
use crate::stages::optimization::{PowellOptimizerSettings, optimize_bounded_powell};
use crate::stages::return_components::{
    ContinuousLikelihoodFidelityTier, NigShapeSearchPoint,
    ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageContinuousReturnCalibrationSample, ReturnComponentsStageFittedPayload,
    ReturnComponentsStageSignalSourceWindow, distribution_profile_fit_score,
    evaluate_configuration_likelihood_objective, nig_configuration_from_search_point,
    nig_search_point_from_configuration, prepare_observed_signal_inputs_for_joint_refinement,
    resolve_activity_calibration_context,
};
use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, build_runtime_hourly_seasonality_factors_from_components,
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
use janiform_engine_simulation::{
    SeededTickEventStateSnapshotCapture, SeededTickSimulationModelInputs,
    SeededTickSimulationRunConfig, SeededTickSimulationRunOptions,
    run_seeded_tick_event_simulation,
};
use janiform_foundations::splitmix64_output_mixer;
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
const JOINT_REFINEMENT_MAX_RESTART_COUNT: usize = 3;

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
    source_window: ReturnComponentsStageSignalSourceWindow,
    observed_open_event_rate_per_hour: f64,
    fallback_sell_share_proxy: f64,
    observed_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    observed_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    observed_15m_rv_activity_metric: JointRefinementRollingVarianceActivityMetric,
    observed_log_price_span: f64,
    hourly_simulation_start_utc: DateTime<Utc>,
    hourly_simulation_interval_count: usize,
    surprise_multiplier_by_hour_index: Vec<(u64, f64)>,
}

#[derive(Debug, Clone)]
struct JointRefinementBaselineBuckets {
    aggregated_likelihood_fit: f64,
    aggregated_shape_fit: f64,
    rv_activity_coupling_fit: f64,
    price_span_sanity_fit: f64,
}

#[derive(Debug, Clone)]
struct JointRefinementContext {
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
    cache:
        Arc<Mutex<BTreeMap<JointRefinementObjectiveCacheKey, JointRefinementCandidateEvaluation>>>,
}

#[derive(Debug, Clone)]
struct JointRefinementCandidateEvaluation {
    fitted_parameters: JointRefinementStageFittedParameters,
    breakdown: JointRefinementStageObjectiveBreakdown,
    invalid_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct JointRefinementRestartOutcome {
    restart_id: &'static str,
    local_evaluation: JointRefinementCandidateEvaluation,
    final_evaluation: JointRefinementCandidateEvaluation,
    local_evaluation_count: usize,
    stop_reason: &'static str,
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
    let runtime_hourly_seasonality_range_end_utc = observed_data
        .hourly_simulation_start_utc
        .checked_add_signed(Duration::hours(
            observed_data.hourly_simulation_interval_count as i64,
        ));
    let Some(runtime_hourly_seasonality_range_end_utc) = runtime_hourly_seasonality_range_end_utc
    else {
        return build_joint_refinement_stage_failure_report(
            run_spec,
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint refinement runtime seasonality range overflowed its hourly horizon"
                        .to_string(),
            },
        );
    };
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
    let Some(runtime_hourly_seasonality_range_end_utc_nanos) =
        runtime_hourly_seasonality_range_end_utc.timestamp_nanos_opt()
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

    let mut bootstrap_context = JointRefinementContext {
        calendar_registry: calendar_registry.clone(),
        exchange_code: run_spec.data_inputs.exchange_code.clone(),
        max_runtime_hourly_seasonality_factor: runtime_hourly_seasonality_factors
            .iter()
            .copied()
            .fold(1.0, f64::max),
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
            aggregated_shape_fit: 1.0,
            rv_activity_coupling_fit: 1.0,
            price_span_sanity_fit: 1.0,
        },
        final_baseline_buckets: JointRefinementBaselineBuckets {
            aggregated_likelihood_fit: 1.0,
            aggregated_shape_fit: 1.0,
            rv_activity_coupling_fit: 1.0,
            price_span_sanity_fit: 1.0,
        },
        cache: Arc::new(Mutex::new(BTreeMap::new())),
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
        aggregated_shape_fit: raw_bucket_value_from_normalized(
            baseline_local_evaluation.breakdown.aggregated_shape_fit,
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
        aggregated_shape_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation.breakdown.aggregated_shape_fit,
        ),
        rv_activity_coupling_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation.breakdown.rv_activity_coupling_fit,
        ),
        price_span_sanity_fit: raw_bucket_value_from_normalized(
            baseline_final_evaluation.breakdown.price_span_sanity_fit,
        ),
    };
    if let Ok(mut cache_guard) = bootstrap_context.cache.lock() {
        cache_guard.clear();
    }

    let restart_seeds = build_restart_seeds(
        baseline_parameter_vector.as_slice(),
        &bootstrap_context.active_parameter_layout,
        bootstrap_context.lower_bounds.as_slice(),
        bootstrap_context.upper_bounds.as_slice(),
        joint_refinement_restart_count(&stage_solver_settings),
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

    let best_restart = restart_outcomes
        .into_iter()
        .filter(|restart_outcome| {
            restart_outcome.local_evaluation.invalid_reason.is_none()
                && restart_outcome.final_evaluation.invalid_reason.is_none()
        })
        .min_by(|left, right| {
            left.final_evaluation
                .breakdown
                .weighted_objective
                .partial_cmp(&right.final_evaluation.breakdown.weighted_objective)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.restart_id.cmp(right.restart_id))
        })
        .unwrap_or_else(|| JointRefinementRestartOutcome {
            restart_id: "baseline",
            local_evaluation: baseline_local_evaluation.clone(),
            final_evaluation: baseline_final_evaluation.clone(),
            local_evaluation_count: 0,
            stop_reason: "baseline_seed",
        });

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
                backend_id: "joint_refinement.block_coordinate_powell.v1".to_string(),
                random_seed: resolved_stage_seed.stage_seed,
                selection_policy: "multi_restart_final_fidelity_rerank.v1",
                penalty_policy: "hard_constraints_added_to_weighted_joint_objective.v1",
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

fn optimize_restart_seeds(
    restart_seeds: &[(&'static str, Vec<f64>)],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<JointRefinementRestartOutcome> {
    let thread_pool_builder = rayon::ThreadPoolBuilder::new()
        .num_threads(solver_settings.cpu_count.max(1))
        .build();

    match thread_pool_builder {
        Ok(thread_pool) => thread_pool.install(|| {
            restart_seeds
                .par_iter()
                .map(|(restart_id, seed_vector)| {
                    optimize_single_restart(
                        *restart_id,
                        seed_vector.as_slice(),
                        context,
                        solver_settings,
                    )
                })
                .collect()
        }),
        Err(_) => restart_seeds
            .iter()
            .map(|(restart_id, seed_vector)| {
                optimize_single_restart(
                    *restart_id,
                    seed_vector.as_slice(),
                    context,
                    solver_settings,
                )
            })
            .collect(),
    }
}

fn optimize_single_restart(
    restart_id: &'static str,
    seed_vector: &[f64],
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementRestartOutcome {
    let mut current_vector = seed_vector.to_vec();
    let mut evaluation_count = 0usize;
    let mut stop_reason = "tolerance";
    let outer_cycle_count = joint_refinement_outer_cycle_count(solver_settings);

    for _ in 0..outer_cycle_count {
        let (updated_hawkes_vector, hawkes_stop_reason, hawkes_evaluations) =
            optimize_parameter_block(
                current_vector.as_slice(),
                context.active_parameter_layout.hawkes_block.as_slice(),
                context,
                solver_settings,
            );
        current_vector = updated_hawkes_vector;
        evaluation_count += hawkes_evaluations;

        let (updated_nig_vector, nig_stop_reason, nig_evaluations) = optimize_parameter_block(
            current_vector.as_slice(),
            context.active_parameter_layout.nig_block.as_slice(),
            context,
            solver_settings,
        );
        current_vector = updated_nig_vector;
        evaluation_count += nig_evaluations;

        let (updated_coupling_vector, coupling_stop_reason, coupling_evaluations) =
            optimize_parameter_block(
                current_vector.as_slice(),
                context.active_parameter_layout.coupling_block.as_slice(),
                context,
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
        restart_id,
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
    let optimizer_settings = joint_refinement_powell_settings(solver_settings);

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
        parameter_bits: parameter_vector
            .iter()
            .map(|value| value.to_bits())
            .collect(),
    };
    if let Ok(cache_guard) = context.cache.lock()
        && let Some(cached_value) = cache_guard.get(&cache_key)
    {
        return cached_value.clone();
    }

    let candidate_evaluation =
        build_joint_candidate_evaluation(parameter_vector, context, fidelity_tier, solver_settings);

    if let Ok(mut cache_guard) = context.cache.lock() {
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
        context,
        fidelity_tier,
        solver_settings,
    )
}

fn build_joint_candidate_evaluation_for_fitted_parameters(
    fitted_parameters: JointRefinementStageFittedParameters,
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
    let mut aggregated_shape_scores = Vec::new();
    let mut rv_activity_scores = Vec::new();
    let mut price_span_scores = Vec::new();
    for simulation_seed in simulation_seeds {
        let Some(simulated_5m_intervals) = simulate_aggregated_intervals(
            &context.calendar_registry,
            context,
            &fitted_parameters,
            simulation_seed,
            JOINT_REFINEMENT_FIVE_MINUTES_NANOS,
        ) else {
            return invalid_candidate_evaluation(
                &context.baseline_hawkes_fitted_parameters,
                &context.baseline_return_components_fitted_parameters,
                solver_settings.constraint_penalty_weight,
                "simulate_aggregated_intervals_failed_5m",
            );
        };
        let Some(simulated_15m_intervals) = simulate_aggregated_intervals(
            &context.calendar_registry,
            context,
            &fitted_parameters,
            simulation_seed,
            JOINT_REFINEMENT_FIFTEEN_MINUTES_NANOS,
        ) else {
            return invalid_candidate_evaluation(
                &context.baseline_hawkes_fitted_parameters,
                &context.baseline_return_components_fitted_parameters,
                solver_settings.constraint_penalty_weight,
                "simulate_aggregated_intervals_failed_15m",
            );
        };

        let shape_5m = distribution_profile_fit_score(
            &simulated_5m_intervals
                .iter()
                .map(|interval| interval.log_return)
                .collect::<Vec<_>>(),
            &context
                .observed_data
                .observed_5m_intervals
                .iter()
                .map(|interval| interval.log_return)
                .collect::<Vec<_>>(),
        )
        .profile_fit_score;
        let shape_15m = distribution_profile_fit_score(
            &simulated_15m_intervals
                .iter()
                .map(|interval| interval.log_return)
                .collect::<Vec<_>>(),
            &context
                .observed_data
                .observed_15m_intervals
                .iter()
                .map(|interval| interval.log_return)
                .collect::<Vec<_>>(),
        )
        .profile_fit_score;
        aggregated_shape_scores.push(0.4 * shape_5m + 0.6 * shape_15m);

        let simulated_rv_metric =
            rolling_variance_activity_metric(simulated_15m_intervals.as_slice());
        rv_activity_scores.push(
            0.7 * (simulated_rv_metric.correlation
                - context
                    .observed_data
                    .observed_15m_rv_activity_metric
                    .correlation)
                .abs()
                + 0.3 * simulated_rv_metric.standardized_path_distance,
        );

        let simulated_log_price_span =
            compute_cumulative_log_price_span(simulated_15m_intervals.as_slice());
        price_span_scores.push(
            (simulated_log_price_span - context.observed_data.observed_log_price_span).abs()
                / context.observed_data.observed_log_price_span.max(1.0e-8),
        );
    }

    let raw_aggregated_shape_fit =
        aggregated_shape_scores.iter().sum::<f64>() / aggregated_shape_scores.len().max(1) as f64;
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
    let normalized_aggregated_shape_fit = zero_aware_normalized_bucket_value(
        raw_aggregated_shape_fit,
        baseline_buckets.aggregated_shape_fit,
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
    let weighted_objective = 0.40 * normalized_aggregated_likelihood_fit
        + 0.25 * normalized_aggregated_shape_fit
        + 0.25 * normalized_rv_activity_coupling_fit
        + 0.10 * normalized_price_span_sanity_fit
        + hard_constraint_penalty;

    JointRefinementCandidateEvaluation {
        fitted_parameters,
        breakdown: JointRefinementStageObjectiveBreakdown {
            aggregated_likelihood_fit: if baseline_buckets.aggregated_likelihood_fit == 1.0
                && baseline_buckets.aggregated_shape_fit == 1.0
                && baseline_buckets.rv_activity_coupling_fit == 1.0
                && baseline_buckets.price_span_sanity_fit == 1.0
            {
                raw_aggregated_likelihood_fit
            } else {
                normalized_aggregated_likelihood_fit
            },
            aggregated_shape_fit: if baseline_buckets.aggregated_likelihood_fit == 1.0
                && baseline_buckets.aggregated_shape_fit == 1.0
                && baseline_buckets.rv_activity_coupling_fit == 1.0
                && baseline_buckets.price_span_sanity_fit == 1.0
            {
                raw_aggregated_shape_fit
            } else {
                normalized_aggregated_shape_fit
            },
            rv_activity_coupling_fit: if baseline_buckets.aggregated_likelihood_fit == 1.0
                && baseline_buckets.aggregated_shape_fit == 1.0
                && baseline_buckets.rv_activity_coupling_fit == 1.0
                && baseline_buckets.price_span_sanity_fit == 1.0
            {
                raw_rv_activity_coupling_fit
            } else {
                normalized_rv_activity_coupling_fit
            },
            price_span_sanity_fit: if baseline_buckets.aggregated_likelihood_fit == 1.0
                && baseline_buckets.aggregated_shape_fit == 1.0
                && baseline_buckets.rv_activity_coupling_fit == 1.0
                && baseline_buckets.price_span_sanity_fit == 1.0
            {
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
            aggregated_shape_fit: 100.0,
            rv_activity_coupling_fit: 100.0,
            price_span_sanity_fit: 100.0,
            weighted_objective: 100.0 + constraint_penalty_weight,
            hard_constraint_penalty: constraint_penalty_weight,
        },
        invalid_reason: Some(reason.to_string()),
    }
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
    let observed_15m_rv_activity_metric =
        rolling_variance_activity_metric(observed_15m_intervals.as_slice());
    let observed_log_price_span =
        compute_cumulative_log_price_span(observed_15m_intervals.as_slice());
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

    JointRefinementObservedData {
        source_window,
        observed_open_event_rate_per_hour,
        fallback_sell_share_proxy: candidate_generation_input.open_session_sell_share_proxy,
        observed_5m_intervals,
        observed_15m_intervals,
        observed_15m_rv_activity_metric,
        observed_log_price_span,
        hourly_simulation_start_utc,
        hourly_simulation_interval_count,
        surprise_multiplier_by_hour_index,
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
) -> Vec<(&'static str, Vec<f64>)> {
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

    let restart_seeds = vec![
        ("baseline", baseline),
        ("higher_activity", higher_activity),
        ("lower_activity_stronger_move", lower_activity_stronger_move),
    ];
    restart_seeds
        .into_iter()
        .take(restart_count.max(1).min(JOINT_REFINEMENT_MAX_RESTART_COUNT))
        .collect()
}

fn joint_refinement_restart_count(solver_settings: &CalibrationOptimizerSettings) -> usize {
    match solver_settings.max_iterations {
        0..=32 => 1,
        33..=96 => 2,
        _ => JOINT_REFINEMENT_MAX_RESTART_COUNT,
    }
}

fn joint_refinement_outer_cycle_count(solver_settings: &CalibrationOptimizerSettings) -> usize {
    match solver_settings.max_iterations {
        0..=32 => 1,
        33..=96 => 2,
        _ => JOINT_REFINEMENT_MAX_OUTER_CYCLE_COUNT,
    }
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

fn joint_refinement_powell_settings(
    solver_settings: &CalibrationOptimizerSettings,
) -> PowellOptimizerSettings {
    let (max_cycle_count, line_search_evaluation_cap, minimum_evaluation_budget) =
        match solver_settings.max_iterations {
            0..=32 => (2, 4, 8),
            33..=96 => (4, 6, 16),
            _ => (8, 8, 24),
        };

    PowellOptimizerSettings {
        absolute_tolerance: solver_settings.absolute_tolerance,
        relative_tolerance: solver_settings.relative_tolerance,
        min_step_norm: solver_settings.absolute_tolerance,
        max_cycle_count,
        line_search_evaluation_cap,
        evaluation_budget: solver_settings
            .max_iterations
            .clamp(minimum_evaluation_budget, 256),
    }
}

fn simulate_aggregated_intervals(
    calendar_registry: &ExchangeCalendarRegistry,
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
    interval_duration_nanos: i64,
) -> Option<Vec<JointRefinementAggregatedIntervalObservation>> {
    let hourly_factor_contract = load_hourly_factor_contract_for_calibration(
        calendar_registry,
        HourlyFactorProviderRequest {
            exchange_code: context.exchange_code.as_str(),
            range_start_utc: context.observed_data.hourly_simulation_start_utc,
            range_end_utc: context
                .observed_data
                .hourly_simulation_start_utc
                .checked_add_signed(Duration::hours(
                    context.observed_data.hourly_simulation_interval_count as i64,
                ))?,
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
                surprise_deterministic_time_multiplier_by_interval_index: context
                    .observed_data
                    .surprise_multiplier_by_hour_index
                    .as_slice(),
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

    Some(build_simulated_open_session_interval_observations(
        calendar_registry,
        context.exchange_code.as_str(),
        context.observed_data.source_window,
        simulation_result.fine_path.as_slice(),
        interval_duration_nanos,
    ))
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
    let weekly_hour_utc_multipliers = &return_components_fitted_parameters
        .surprise_deterministic_intensity_profile
        .weekly_hour_utc_multipliers;
    (0..hourly_simulation_interval_count)
        .map(|interval_index| {
            let interval_start = hourly_simulation_start_utc
                .checked_add_signed(Duration::hours(interval_index as i64))
                .unwrap_or(hourly_simulation_start_utc);
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
