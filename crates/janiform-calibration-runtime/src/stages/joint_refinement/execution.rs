use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskModelInputs,
};
use janiform_engine_simulation::{
    CoarseSimulationModelInputs, CoarseSimulationRunRequest, run_coarse_simulation,
};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::adapters::load_hourly_factor_contract_for_calibration;
use crate::calendar::ExchangeCalendarRegistry;
use crate::optimizer_adapter::CalibrationOptimizerAdapterRunMetadata;
use crate::stages::hawkes::stationary_open_event_rate_per_hour;
use crate::stages::joint_metrics::{
    JointRefinementAggregatedIntervalObservation,
    JointRefinementBusinessTimeBucketReturnScaleSummaries,
    JointRefinementBusinessTimeBucketThresholds, JointRefinementReturnScaleSummary,
    JointRefinementRollingVarianceActivityMetric,
    build_continuous_calibration_sample_from_aggregated_intervals, finalize_sell_share_proxies,
    return_scale_summary, zeroed_interval_observation_template,
};
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageCandidateDiagnostic, JointRefinementStageCoarseTickConsistencyMetric,
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionInput,
    JointRefinementStageExecutionMetadata, JointRefinementStageExecutionReport,
    JointRefinementStageFittedPayload, JointRefinementStageObjectiveTermDiagnostic,
    JointRefinementStageSimulationSummary,
};
use crate::stages::optimization::{PowellOptimizerSettings, optimize_bounded_powell};
use crate::stages::return_components::{
    ReturnComponentsStageContinuousReturnCalibrationSample,
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
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationOptimizerSettings, CalibrationRunSpec,
    CalibrationStage, HawkesStageFittedParameters, JointRefinementStageFittedParameters,
    JointRefinementStageObjectiveBreakdown, JointRefinementStageOptimizerDiagnostics,
    ReturnComponentsStageFittedParameters, resolve_calibration_stage_seed_metadata,
    stage_solver_settings_for_stage,
};
use janiform_foundations::low_discrepancy::van_der_corput;
use janiform_foundations::numeric::integer_square_root;
use janiform_foundations::splitmix64_output_mixer;
use janiform_foundations::time::{FIVE_MINUTES_NANOS, ONE_HOUR_NANOS};
use janiform_market_time::calendar::{
    CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarRegistryArtifact,
    RegularSessionArtifact, SessionWindowArtifact,
};
use janiform_market_time::hourly_factors::{
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
};

use super::objective::{
    compare_joint_restart_outcomes, evaluate_joint_candidate, evaluate_joint_fitted_parameters,
    joint_primary_metrics_from_breakdown,
};
use super::setup::{
    build_active_parameter_layout, build_baseline_parameter_vector, build_observed_data,
    build_parameter_bounds, build_restart_seeds, build_surprise_multipliers_for_interval_grid,
    cached_seed_summary_for_candidate_seed, joint_refinement_final_simulation_seed_count,
    observed_activity_log_shift, selected_hawkes_observed_open_event_rate_per_hour,
    selected_return_components_candidate, stage_input_to_activity_input,
};

pub(super) const JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR: f64 = 1.0e-8;
pub(super) const JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND: f64 = 1.386_294_361_119_890_6;
const JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT: usize = 3;
pub(super) const JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR: f64 = 1.0e-8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum JointRefinementFidelityTier {
    Local,
    Final,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct JointRefinementObjectiveCacheKey {
    pub(super) fidelity_tier: JointRefinementFidelityTier,
    pub(super) parameter_bits: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct JointRefinementSeedSummaryCacheKey {
    pub(super) parameter_bits: Vec<u64>,
    pub(super) simulation_seed: u64,
}

#[derive(Debug, Clone)]
pub(super) struct JointRefinementActiveParameterLayout {
    pub(super) hawkes_block: Vec<usize>,
    pub(super) nig_block: Vec<usize>,
    pub(super) coupling_block: Vec<usize>,
    pub(super) log_lambda_scale_index: usize,
    pub(super) log_beta_scale_index: usize,
    pub(super) log_self_excitation_scale_index: usize,
    pub(super) log_cross_excitation_scale_index: usize,
    pub(super) known_activity_scale_index: Option<usize>,
    pub(super) surprise_activity_scale_index: Option<usize>,
    pub(super) surprise_baseline_scale_index: Option<usize>,
}

#[derive(Debug, Clone)]
pub(super) struct JointRefinementObservedData {
    pub(super) observed_open_event_rate_per_hour: f64,
    pub(super) fallback_sell_share_proxy: f64,
    pub(super) observed_tick_continuous_return_calibration_sample:
        ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(super) observed_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(super) observed_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(super) observed_tick_intraday_mean_absolute_returns: Vec<f64>,
    pub(super) observed_tick_shape_returns: Vec<f64>,
    pub(super) observed_5m_shape_returns: Vec<f64>,
    pub(super) observed_15m_shape_returns: Vec<f64>,
    pub(super) observed_tick_return_scale_summary: JointRefinementReturnScaleSummary,
    pub(super) observed_5m_return_scale_summary: JointRefinementReturnScaleSummary,
    pub(super) observed_15m_return_scale_summary: JointRefinementReturnScaleSummary,
    pub(super) observed_5m_business_time_bucket_thresholds:
        JointRefinementBusinessTimeBucketThresholds,
    pub(super) observed_15m_business_time_bucket_thresholds:
        JointRefinementBusinessTimeBucketThresholds,
    pub(super) observed_5m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    pub(super) observed_15m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    pub(super) observed_5m_intraday_mean_absolute_returns: Vec<f64>,
    pub(super) observed_15m_intraday_mean_absolute_returns: Vec<f64>,
    pub(super) observed_hourly_tick_counts: Vec<f64>,
    pub(super) observed_15m_rv_activity_metric: JointRefinementRollingVarianceActivityMetric,
    pub(super) observed_log_price_span: f64,
    pub(super) hourly_simulation_start_utc: DateTime<Utc>,
    pub(super) hourly_simulation_end_utc: DateTime<Utc>,
    pub(super) hourly_simulation_interval_count: usize,
    pub(super) surprise_weekly_hour_utc_multipliers: Vec<f64>,
    pub(super) simulated_5m_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(super) simulated_15m_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(super) simulated_hourly_interval_template:
        Vec<JointRefinementAggregatedIntervalObservation>,
}

#[derive(Debug, Clone)]
pub(super) struct JointRefinementBaselineBuckets {
    pub(super) aggregated_likelihood_fit: f64,
    pub(super) aggregated_return_shape_fit: f64,
    pub(super) return_asymmetry_fit: f64,
    pub(super) tick_return_asymmetry_fit: f64,
    pub(super) aggregated_return_scale_fit: f64,
    pub(super) tick_return_scale_fit: f64,
    pub(super) business_time_return_scale_fit: f64,
    pub(super) intraday_return_level_fit: f64,
    pub(super) activity_shape_fit: f64,
    pub(super) activity_level_fit: f64,
    pub(super) rv_activity_coupling_fit: f64,
    pub(super) price_span_sanity_fit: f64,
}

#[derive(Debug, Clone)]
pub(super) struct JointRefinementContext {
    pub(super) stage_thread_pool: Option<Arc<rayon::ThreadPool>>,
    pub(super) calendar_registry: ExchangeCalendarRegistry,
    pub(super) exchange_code: String,
    pub(super) runtime_hourly_seasonality_factors: Vec<f64>,
    pub(super) max_runtime_hourly_seasonality_factor: f64,
    pub(super) observed_data: JointRefinementObservedData,
    pub(super) baseline_modeled_open_event_rate_per_hour: f64,
    pub(super) baseline_hawkes_fitted_parameters: HawkesStageFittedParameters,
    pub(super) baseline_return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
    pub(super) active_parameter_layout: JointRefinementActiveParameterLayout,
    pub(super) lower_bounds: Vec<f64>,
    pub(super) upper_bounds: Vec<f64>,
    pub(super) local_simulation_seed: u64,
    pub(super) final_simulation_seeds: Vec<u64>,
    pub(super) local_baseline_buckets: JointRefinementBaselineBuckets,
    pub(super) final_baseline_buckets: JointRefinementBaselineBuckets,
    pub(super) objective_cache:
        Arc<Mutex<BTreeMap<JointRefinementObjectiveCacheKey, JointRefinementCandidateEvaluation>>>,
    pub(super) seed_summary_cache: Arc<
        Mutex<BTreeMap<JointRefinementSeedSummaryCacheKey, Option<JointRefinementSeedSummary>>>,
    >,
}

#[derive(Debug, Clone)]
pub(super) struct JointRefinementCandidateEvaluation {
    pub(super) fitted_parameters: JointRefinementStageFittedParameters,
    pub(super) breakdown: JointRefinementStageObjectiveBreakdown,
    pub(super) objective_terms: Vec<JointRefinementStageObjectiveTermDiagnostic>,
    pub(super) invalid_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct JointRefinementRestartOutcome {
    pub(super) restart_id: String,
    pub(super) parameter_vector: Vec<f64>,
    pub(super) local_evaluation: JointRefinementCandidateEvaluation,
    pub(super) final_evaluation: JointRefinementCandidateEvaluation,
    pub(super) local_evaluation_count: usize,
    pub(super) stop_reason: &'static str,
}

#[derive(Debug, Clone)]
pub(super) struct JointRefinementSeedSummary {
    pub(super) simulated_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(super) simulated_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(super) simulated_tick_shape_returns: Vec<f64>,
    pub(super) simulated_5m_shape_returns: Vec<f64>,
    pub(super) simulated_15m_shape_returns: Vec<f64>,
    pub(super) simulated_tick_return_scale_summary: JointRefinementReturnScaleSummary,
    pub(super) simulated_5m_return_scale_summary: JointRefinementReturnScaleSummary,
    pub(super) simulated_15m_return_scale_summary: JointRefinementReturnScaleSummary,
    pub(super) simulated_5m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    pub(super) simulated_15m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    pub(super) simulated_tick_intraday_mean_absolute_returns: Vec<f64>,
    pub(super) simulated_5m_intraday_mean_absolute_returns: Vec<f64>,
    pub(super) simulated_15m_intraday_mean_absolute_returns: Vec<f64>,
    pub(super) simulated_hourly_tick_counts: Vec<f64>,
    pub(super) simulated_15m_rv_activity_metric: JointRefinementRollingVarianceActivityMetric,
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

    let observed_signal_preparation = match prepare_observed_signal_inputs_for_joint_refinement(
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
        &observed_signal_preparation,
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
        &baseline_15m_calibration_sample,
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
            ONE_HOUR_NANOS,
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
        baseline_modeled_open_event_rate_per_hour,
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
            return_asymmetry_fit: 1.0,
            tick_return_asymmetry_fit: 1.0,
            aggregated_return_scale_fit: 1.0,
            tick_return_scale_fit: 1.0,
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
            return_asymmetry_fit: 1.0,
            tick_return_asymmetry_fit: 1.0,
            aggregated_return_scale_fit: 1.0,
            tick_return_scale_fit: 1.0,
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

    bootstrap_context.local_baseline_buckets =
        baseline_buckets_from_evaluation(&baseline_local_evaluation);
    bootstrap_context.final_baseline_buckets =
        baseline_buckets_from_evaluation(&baseline_final_evaluation);

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
        &baseline_15m_calibration_sample,
        selected_return_components_candidate(return_components_payload)
            .map(|candidate| {
                candidate
                    .continuous_distribution_fit_breakdown
                    .optimizer_basin_diagnostics
                    .as_slice()
            })
            .unwrap_or(&[]),
        observed_activity_log_shift(
            bootstrap_context
                .observed_data
                .observed_open_event_rate_per_hour,
            baseline_modeled_open_event_rate_per_hour,
        ),
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
pub(super) struct JointRefinementAdmissibilityThresholds {
    aggregated_likelihood_fit: f64,
    aggregated_return_shape_fit: f64,
    return_asymmetry_fit: f64,
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

    let has_admissible_candidate = restart_outcomes.iter().any(|candidate| {
        joint_restart_outcome_is_valid(candidate)
            && joint_restart_outcome_is_admissible(candidate, thresholds)
    });

    let mut ranked_candidates: Vec<(usize, &JointRefinementRestartOutcome)> = restart_outcomes
        .iter()
        .enumerate()
        .map(|(position, outcome)| (pareto_ranks[position], *outcome))
        .collect();

    ranked_candidates.sort_by(|left, right| {
        compare_joint_restart_outcomes(*left, *right, thresholds, has_admissible_candidate)
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
        return_asymmetry_fit: admissibility_threshold_from_sample_count(
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
    1.0 + (sample_count.max(1) as f64).recip().sqrt()
}

pub(super) fn joint_restart_outcome_is_admissible(
    restart_outcome: &JointRefinementRestartOutcome,
    thresholds: JointRefinementAdmissibilityThresholds,
) -> bool {
    let breakdown = &restart_outcome.final_evaluation.breakdown;

    breakdown.aggregated_likelihood_fit <= thresholds.aggregated_likelihood_fit
        && breakdown.aggregated_return_shape_fit <= thresholds.aggregated_return_shape_fit
        && breakdown.return_asymmetry_fit <= thresholds.return_asymmetry_fit
        && breakdown.aggregated_return_scale_fit <= thresholds.aggregated_return_scale_fit
        && breakdown.business_time_return_scale_fit <= thresholds.business_time_return_scale_fit
        && breakdown.intraday_return_level_fit <= thresholds.intraday_return_level_fit
        && breakdown.activity_shape_fit <= thresholds.activity_shape_fit
        && breakdown.activity_level_fit <= thresholds.activity_level_fit
        && breakdown.rv_activity_coupling_fit <= thresholds.rv_activity_coupling_fit
        && breakdown.price_span_sanity_fit <= thresholds.price_span_sanity_fit
}

fn joint_restart_outcome_is_valid(restart_outcome: &JointRefinementRestartOutcome) -> bool {
    restart_outcome.local_evaluation.invalid_reason.is_none()
        && restart_outcome.final_evaluation.invalid_reason.is_none()
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
) -> [f64; 11] {
    joint_primary_metrics_from_breakdown(&restart_outcome.final_evaluation.breakdown)
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

    let has_admissible_candidate = restart_outcomes.iter().any(|candidate| {
        joint_restart_outcome_is_valid(candidate)
            && joint_restart_outcome_is_admissible(candidate, thresholds)
    });

    let mut ranked_candidates: Vec<(usize, &JointRefinementRestartOutcome)> = restart_outcomes
        .iter()
        .enumerate()
        .map(|(position, outcome)| (pareto_ranks[position], outcome))
        .collect();

    ranked_candidates.sort_by(|left, right| {
        compare_joint_restart_outcomes(*left, *right, thresholds, has_admissible_candidate)
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
                objective_terms: outcome.final_evaluation.objective_terms.clone(),
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
            FIVE_MINUTES_NANOS,
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

    let interval_delta = interval_duration_nanos as f64 / ONE_HOUR_NANOS as f64;

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

    let model_inputs = build_joint_coarse_model_inputs(
        context,
        fitted_parameters,
        hourly_factor_contract.interval_indices.len(),
        interval_delta,
    )?;

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

    finalize_sell_share_proxies(intervals.as_mut_slice(), 0.5);

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

    finalize_sell_share_proxies(aggregated_intervals.as_mut_slice(), 0.5);
    aggregated_intervals
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
    interval_count: usize,
    interval_delta: f64,
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
        MeanReversionFittedParameters::disabled(),
        MeanReversionRuntimeIntervalAnchors::try_new(
            "explicit_runtime_interval_grid_alignment.v1",
            0.0,
            vec![0.0; interval_count],
            vec![interval_delta; interval_count],
        )
        .ok()?,
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0).ok()?,
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

pub(super) fn with_joint_refinement_thread_pool<T, F>(
    context: &JointRefinementContext,
    operation: F,
) -> T
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    match context.stage_thread_pool.as_ref() {
        Some(thread_pool) => thread_pool.install(operation),
        None => operation(),
    }
}

pub(super) fn ordered_parameter_bits(parameter_vector: &[f64]) -> Vec<u64> {
    parameter_vector
        .iter()
        .map(|value| value.to_bits())
        .collect::<Vec<_>>()
}

pub(super) fn deduplicate_restart_seed_candidates(
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

pub(super) fn halton_parameter_vector(
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

pub(super) fn build_halton_restart_id(position: usize) -> String {
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
    let (base_outer_cycle_count, base_powell_settings) =
        joint_refinement_base_schedule_components(solver_settings);

    let restart_count = solver_settings
        .cpu_count
        .max(1)
        .min(integer_square_root(
            base_outer_cycle_count * base_powell_settings.evaluation_budget,
        ))
        .max(1);

    let total_block_cycle_budget = base_outer_cycle_count * base_powell_settings.evaluation_budget;

    let outer_cycle_count = ((base_outer_cycle_count * base_outer_cycle_count)
        / restart_count.max(1))
    .clamp(1, base_outer_cycle_count);

    let evaluation_budget = (total_block_cycle_budget
        / (restart_count.max(1) * outer_cycle_count.max(1)))
    .max(base_powell_settings.line_search_evaluation_cap.max(1));

    let line_search_evaluation_cap = integer_square_root(evaluation_budget).max(1);

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
    let outer_cycle_count = JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT;
    let evaluation_budget = solver_settings.max_iterations.max(outer_cycle_count);
    let line_search_evaluation_cap = integer_square_root(evaluation_budget).max(1);

    let max_cycle_count = evaluation_budget
        .div_ceil(line_search_evaluation_cap.max(1))
        .max(1);

    let powell_settings = PowellOptimizerSettings {
        absolute_tolerance: solver_settings.absolute_tolerance,
        relative_tolerance: solver_settings.relative_tolerance,
        min_step_norm: solver_settings.absolute_tolerance,
        max_cycle_count,
        line_search_evaluation_cap,
        evaluation_budget,
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
        let (updated_nig_vector, nig_stop_reason, nig_evaluations) = optimize_parameter_block(
            current_vector.as_slice(),
            context.active_parameter_layout.nig_block.as_slice(),
            context,
            budget_schedule.powell_settings,
            solver_settings,
        );

        current_vector = updated_nig_vector;
        evaluation_count += nig_evaluations;

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

pub(super) fn clamp_in_place(values: &mut [f64], lower_bounds: &[f64], upper_bounds: &[f64]) {
    for (value, (lower_bound, upper_bound)) in values
        .iter_mut()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
    {
        *value = value.clamp(*lower_bound, *upper_bound);
    }
}

fn baseline_buckets_from_evaluation(
    evaluation: &JointRefinementCandidateEvaluation,
) -> JointRefinementBaselineBuckets {
    let breakdown = &evaluation.breakdown;

    JointRefinementBaselineBuckets {
        aggregated_likelihood_fit: breakdown.aggregated_likelihood_fit,
        aggregated_return_shape_fit: breakdown.aggregated_return_shape_fit,
        return_asymmetry_fit: breakdown.return_asymmetry_fit,
        tick_return_asymmetry_fit: breakdown.tick_return_asymmetry_fit,
        aggregated_return_scale_fit: breakdown.aggregated_return_scale_fit,
        tick_return_scale_fit: breakdown.tick_return_scale_fit,
        business_time_return_scale_fit: breakdown.business_time_return_scale_fit,
        intraday_return_level_fit: breakdown.intraday_return_level_fit,
        activity_shape_fit: breakdown.activity_shape_fit,
        activity_level_fit: breakdown.activity_level_fit,
        rv_activity_coupling_fit: breakdown.rv_activity_coupling_fit,
        price_span_sanity_fit: breakdown.price_span_sanity_fit,
    }
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
    use std::cmp::Ordering;

    use super::super::objective::{
        business_time_bucket_return_scale_fit, compare_joint_candidate_breakdowns,
        intraday_return_level_horizon_fit, joint_primary_metrics_from_breakdown,
        return_asymmetry_horizon_fit, root_mean_square,
    };
    use super::super::setup::{
        build_restart_seeds, project_basin_diagnostic_into_joint_nig_coordinates,
        sampled_observed_tick_returns,
    };
    use super::{
        JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT, JointRefinementActiveParameterLayout,
        admissibility_threshold_from_sample_count, joint_refinement_base_schedule_components,
        joint_refinement_budget_schedule,
    };
    use crate::stages::joint_metrics::{
        JointRefinementBusinessTimeBucketReturnScaleSummaries, JointRefinementReturnScaleSummary,
    };
    use crate::stages::return_components::{
        ReturnComponentsObservedOpenSessionReturn,
        ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
        ReturnComponentsStageContinuousReturnCalibrationSample,
        build_continuous_return_calibration_sample_from_components,
    };
    use janiform_calibration_artifacts::{
        CalibrationOptimizerSettings, JointRefinementStageObjectiveBreakdown,
    };
    use janiform_foundations::numeric::integer_square_root;

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

    fn build_test_basin_diagnostic(
        family_label: &str,
        final_log_alpha: f64,
        final_buy_skew_coordinate: f64,
        final_sell_relative_coordinate: f64,
        final_log_delta_offset: f64,
    ) -> ReturnComponentsStageContinuousOptimizerBasinDiagnostic {
        ReturnComponentsStageContinuousOptimizerBasinDiagnostic {
            family_label: family_label.to_string(),
            seed_objective: 1.0,
            local_objective: 0.9,
            final_objective: 0.8,
            local_evaluation_count: 8,
            stop_reason: "test".to_string(),
            seed_log_alpha: final_log_alpha,
            seed_buy_skew_coordinate: final_buy_skew_coordinate,
            seed_sell_relative_coordinate: final_sell_relative_coordinate,
            seed_log_delta_offset: final_log_delta_offset,
            final_log_alpha,
            final_buy_skew_coordinate,
            final_sell_relative_coordinate,
            final_log_delta_offset,
            final_alpha: final_log_alpha.exp(),
            final_beta_0: -0.5,
            final_delta: final_log_delta_offset.exp(),
            final_delta_beta: -0.2,
            profile_fit_score: 0.8,
            fitted_return_volatility: 0.01,
            fitted_skewness: -0.1,
        }
    }

    fn build_test_calibration_sample() -> ReturnComponentsStageContinuousReturnCalibrationSample {
        build_continuous_return_calibration_sample_from_components(
            vec![0.012, -0.009, 0.006, -0.004, 0.011, -0.007],
            vec![1.0, 0.8, 1.2, 0.9, 1.1, 0.95],
            vec![0.48, 0.53, 0.51, 0.47, 0.55, 0.49],
            0.5,
        )
    }

    fn build_test_joint_breakdown(
        tick_return_asymmetry_fit: f64,
        tick_return_scale_fit: f64,
        weighted_objective: f64,
    ) -> JointRefinementStageObjectiveBreakdown {
        JointRefinementStageObjectiveBreakdown {
            aggregated_likelihood_fit: 1.0,
            aggregated_return_shape_fit: 1.0,
            return_asymmetry_fit: 1.0,
            tick_return_asymmetry_fit,
            aggregated_return_scale_fit: 1.0,
            tick_return_scale_fit,
            business_time_return_scale_fit: 1.0,
            intraday_return_level_fit: 1.0,
            activity_shape_fit: 1.0,
            activity_level_fit: 1.0,
            rv_activity_coupling_fit: 1.0,
            price_span_sanity_fit: 0.5,
            weighted_objective,
            hard_constraint_penalty: 0.0,
        }
    }

    #[test]
    fn joint_refinement_restart_count_scales_with_available_budget() {
        let single_core_schedule = joint_refinement_budget_schedule(&build_test_solver_settings(1));
        let eight_core_schedule = joint_refinement_budget_schedule(&build_test_solver_settings(8));
        let high_core_schedule = joint_refinement_budget_schedule(&build_test_solver_settings(64));

        assert_eq!(single_core_schedule.restart_count, 1);
        assert_eq!(eight_core_schedule.restart_count, 8);
        assert_eq!(
            high_core_schedule.restart_count,
            integer_square_root(JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT * 256)
        );
    }

    #[test]
    fn joint_refinement_high_core_budget_stays_within_base_block_budget() {
        let solver_settings = build_test_solver_settings(64);
        let (base_outer_cycle_count, base_powell_settings) =
            joint_refinement_base_schedule_components(&solver_settings);
        let base_total_block_evaluation_budget = JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT
            * base_outer_cycle_count
            * base_powell_settings.evaluation_budget;

        let high_core_schedule = joint_refinement_budget_schedule(&solver_settings);
        let scheduled_block_evaluation_budget = high_core_schedule.restart_count
            * high_core_schedule.outer_cycle_count
            * JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT
            * high_core_schedule.powell_settings.evaluation_budget;

        assert!(
            scheduled_block_evaluation_budget <= base_total_block_evaluation_budget,
            "scheduled block evaluation budget exceeded base budget: scheduled={scheduled_block_evaluation_budget} base={base_total_block_evaluation_budget}",
        );
    }

    #[test]
    fn joint_refinement_restart_seed_generation_is_deterministic() {
        let active_parameter_layout = build_test_active_parameter_layout();
        let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];
        let baseline_15m_calibration_sample = build_test_calibration_sample();

        let lower_bounds = vec![-1.0; baseline_parameter_vector.len()];
        let upper_bounds = vec![2.0; baseline_parameter_vector.len()];

        let first_restart_seeds = build_restart_seeds(
            baseline_parameter_vector.as_slice(),
            &active_parameter_layout,
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
            &baseline_15m_calibration_sample,
            &[],
            0.25,
            8,
        );
        let second_restart_seeds = build_restart_seeds(
            baseline_parameter_vector.as_slice(),
            &active_parameter_layout,
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
            &baseline_15m_calibration_sample,
            &[],
            0.25,
            8,
        );

        assert_eq!(first_restart_seeds, second_restart_seeds);
        assert_eq!(first_restart_seeds.len(), 8);
        assert_eq!(first_restart_seeds[0].0, "baseline");
        assert_eq!(first_restart_seeds[1].0, "activity_aligned");
        assert_eq!(first_restart_seeds[2].0, "midpoint");
        assert_eq!(first_restart_seeds[3].0, "halton_01");
    }

    #[test]
    fn joint_refinement_restart_seeds_include_return_component_nig_basins() {
        let active_parameter_layout = build_test_active_parameter_layout();
        let baseline_parameter_vector = vec![0.0; active_parameter_layout.vector_length()];
        let baseline_15m_calibration_sample = build_test_calibration_sample();
        let lower_bounds = vec![-1.0; baseline_parameter_vector.len()];
        let upper_bounds = vec![2.0; baseline_parameter_vector.len()];
        let basin_diagnostics = vec![build_test_basin_diagnostic(
            "skew_heavy",
            0.4,
            -0.2,
            1.3,
            -0.6,
        )];

        let restart_seeds = build_restart_seeds(
            baseline_parameter_vector.as_slice(),
            &active_parameter_layout,
            lower_bounds.as_slice(),
            upper_bounds.as_slice(),
            &baseline_15m_calibration_sample,
            basin_diagnostics.as_slice(),
            0.25,
            8,
        );
        let projected_search_point = project_basin_diagnostic_into_joint_nig_coordinates(
            &baseline_15m_calibration_sample,
            &basin_diagnostics[0],
        )
        .expect("basin diagnostic should project into joint coordinates");
        let clamped_projected_coordinates = [
            projected_search_point.log_alpha.clamp(
                lower_bounds[active_parameter_layout.nig_block[0]],
                upper_bounds[active_parameter_layout.nig_block[0]],
            ),
            projected_search_point.buy_skew_coordinate.clamp(
                lower_bounds[active_parameter_layout.nig_block[1]],
                upper_bounds[active_parameter_layout.nig_block[1]],
            ),
            projected_search_point.sell_relative_coordinate.clamp(
                lower_bounds[active_parameter_layout.nig_block[2]],
                upper_bounds[active_parameter_layout.nig_block[2]],
            ),
            projected_search_point.log_delta_offset.clamp(
                lower_bounds[active_parameter_layout.nig_block[3]],
                upper_bounds[active_parameter_layout.nig_block[3]],
            ),
        ];

        assert_eq!(restart_seeds[3].0, "nig_basin_skew_heavy");
        assert_eq!(restart_seeds[4].0, "nig_basin_skew_heavy_activity_aligned");
        assert_eq!(
            restart_seeds[3].1[active_parameter_layout.nig_block[0]],
            clamped_projected_coordinates[0]
        );
        assert_eq!(
            restart_seeds[3].1[active_parameter_layout.nig_block[1]],
            clamped_projected_coordinates[1]
        );
        assert_eq!(
            restart_seeds[3].1[active_parameter_layout.nig_block[2]],
            clamped_projected_coordinates[2]
        );
        assert_eq!(
            restart_seeds[3].1[active_parameter_layout.nig_block[3]],
            clamped_projected_coordinates[3]
        );
        assert_eq!(
            restart_seeds[4].1[active_parameter_layout.log_lambda_scale_index],
            0.25
        );
        assert_eq!(restart_seeds[5].0, "nig_alpha_delta_skew_heavy");
        assert_eq!(
            restart_seeds[5].1[active_parameter_layout.nig_block[0]],
            clamped_projected_coordinates[0]
        );
        assert_eq!(
            restart_seeds[5].1[active_parameter_layout.nig_block[1]],
            baseline_parameter_vector[active_parameter_layout.nig_block[1]]
        );
        assert_eq!(
            restart_seeds[5].1[active_parameter_layout.nig_block[2]],
            baseline_parameter_vector[active_parameter_layout.nig_block[2]]
        );
        assert_eq!(
            restart_seeds[5].1[active_parameter_layout.nig_block[3]],
            clamped_projected_coordinates[3]
        );
        assert_eq!(
            restart_seeds[6].0,
            "nig_alpha_delta_skew_heavy_activity_aligned"
        );
        assert_eq!(
            restart_seeds[6].1[active_parameter_layout.log_lambda_scale_index],
            0.25
        );
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
    fn business_time_bucket_return_scale_fit_penalizes_missing_supported_buckets() {
        let observed_summaries = JointRefinementBusinessTimeBucketReturnScaleSummaries {
            bucket_summaries: [Some(JointRefinementReturnScaleSummary {
                mean_absolute_return: 0.001,
                realized_variance: 1.0e-6,
                absolute_return_quantile_q90: 0.002,
            }); 4],
        };
        let candidate_summaries = JointRefinementBusinessTimeBucketReturnScaleSummaries {
            bucket_summaries: [
                None,
                Some(JointRefinementReturnScaleSummary {
                    mean_absolute_return: 0.001,
                    realized_variance: 1.0e-6,
                    absolute_return_quantile_q90: 0.002,
                }),
                Some(JointRefinementReturnScaleSummary {
                    mean_absolute_return: 0.001,
                    realized_variance: 1.0e-6,
                    absolute_return_quantile_q90: 0.002,
                }),
                Some(JointRefinementReturnScaleSummary {
                    mean_absolute_return: 0.001,
                    realized_variance: 1.0e-6,
                    absolute_return_quantile_q90: 0.002,
                }),
            ],
        };

        let matched_fit =
            business_time_bucket_return_scale_fit(observed_summaries, observed_summaries);
        let missing_bucket_fit =
            business_time_bucket_return_scale_fit(candidate_summaries, observed_summaries);

        assert_eq!(matched_fit, 0.0);
        assert!(missing_bucket_fit > matched_fit);
    }

    #[test]
    fn admissibility_threshold_tightens_with_sample_count() {
        let sparse_threshold = admissibility_threshold_from_sample_count(1);
        let dense_threshold = admissibility_threshold_from_sample_count(400);

        assert_eq!(sparse_threshold, 2.0);
        assert!(dense_threshold < sparse_threshold);
        assert!(dense_threshold > 1.0);
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

    #[test]
    fn return_asymmetry_horizon_fit_penalizes_mirrored_asymmetry() {
        let observed_profile = vec![-0.004, -0.002, -0.001, 0.0, 0.001, 0.002];
        let matched_profile = observed_profile.clone();
        let mirrored_profile = observed_profile
            .iter()
            .map(|value| -*value)
            .collect::<Vec<_>>();

        let matched_fit =
            return_asymmetry_horizon_fit(matched_profile.as_slice(), observed_profile.as_slice());
        let mirrored_fit =
            return_asymmetry_horizon_fit(mirrored_profile.as_slice(), observed_profile.as_slice());

        assert_eq!(matched_fit, 0.0);
        assert!(mirrored_fit > matched_fit);
    }

    #[test]
    fn root_mean_square_penalizes_concentrated_joint_misses() {
        let concentrated_miss = root_mean_square([3.0, 0.0, 0.0].as_slice());
        let balanced_miss = root_mean_square([1.0, 1.0, 1.0].as_slice());

        assert!(concentrated_miss > balanced_miss);
    }

    #[test]
    fn sampled_observed_tick_returns_use_raw_open_session_returns() {
        let observed_open_session_returns = vec![
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: 1,
                log_return: 0.01,
                post_shock_activity_lift: 0.0,
                sell_share_proxy: 0.5,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: 2,
                log_return: 0.02,
                post_shock_activity_lift: 0.0,
                sell_share_proxy: 0.5,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: 3,
                log_return: 0.03,
                post_shock_activity_lift: 0.0,
                sell_share_proxy: 0.5,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: 4,
                log_return: 0.04,
                post_shock_activity_lift: 0.0,
                sell_share_proxy: 0.5,
            },
        ];

        let sampled_returns =
            sampled_observed_tick_returns(observed_open_session_returns.as_slice(), 3);

        assert_eq!(sampled_returns, vec![0.01, 0.02, 0.04]);
    }

    #[test]
    fn joint_primary_metrics_include_direct_tick_distribution_terms() {
        let breakdown = build_test_joint_breakdown(1.7, 1.9, 1.2);

        let primary_metrics = joint_primary_metrics_from_breakdown(&breakdown);

        assert_eq!(primary_metrics.len(), 11);
        assert_eq!(primary_metrics[3], 1.7);
        assert_eq!(primary_metrics[5], 1.9);
    }

    #[test]
    fn admissible_candidate_ordering_uses_direct_tick_terms_before_weighted_tie_break() {
        let better_tick_fit = build_test_joint_breakdown(0.8, 0.7, 1.4);
        let worse_tick_fit = build_test_joint_breakdown(1.4, 1.5, 1.1);

        let ordering = compare_joint_candidate_breakdowns(
            1,
            &better_tick_fit,
            true,
            "better_tick_fit",
            1,
            &worse_tick_fit,
            true,
            "worse_tick_fit",
            true,
        );

        assert_eq!(ordering, Ordering::Less);
    }

    #[test]
    fn admissible_candidate_ordering_uses_weighted_objective_after_primary_metrics_tie() {
        let lower_objective = build_test_joint_breakdown(1.0, 1.0, 0.9);
        let higher_objective = build_test_joint_breakdown(1.0, 1.0, 1.1);

        let ordering = compare_joint_candidate_breakdowns(
            1,
            &lower_objective,
            true,
            "lower_objective",
            1,
            &higher_objective,
            true,
            "higher_objective",
            true,
        );

        assert_eq!(ordering, Ordering::Less);
    }
}
