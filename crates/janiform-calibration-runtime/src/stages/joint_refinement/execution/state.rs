use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use janiform_calibration_artifacts::{
    HawkesStageFittedParameters, JointRefinementStageFittedParameters,
    JointRefinementStageObjectiveBreakdown, JointRefinementStageRuntimeProfile,
    ReturnComponentsStageFittedParameters,
};
use janiform_engine_pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
    VanillaOptionGridBatchPricingRequest, VanillaOptionPayoffType,
    VanillaOptionPricingIntervalFactors,
};
use serde::{Deserialize, Serialize};

use crate::stages::joint_metrics::{
    JointRefinementAggregatedIntervalObservation,
    JointRefinementBusinessTimeBucketReturnScaleSummaries,
    JointRefinementBusinessTimeBucketThresholds, JointRefinementReturnScaleSummary,
    JointRefinementRollingVarianceActivityMetric,
};
use crate::stages::joint_refinement::contracts::JointRefinementStageObjectiveTermDiagnostic;
use crate::stages::return_components::ReturnComponentsStageContinuousReturnCalibrationSample;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::super::worker_runtime::JointRefinementSummarySimulationScheduler;

pub(crate) const JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR: f64 = 1.0e-8;
pub(crate) const JOINT_REFINEMENT_HAWKES_SCALE_LOG_BOUND: f64 = 1.386_294_361_119_890_6;
pub(crate) const JOINT_REFINEMENT_PARAMETER_BLOCK_COUNT: usize = 3;
pub(crate) const JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR: f64 = 1.0e-8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum JointRefinementFidelityTier {
    Local,
    Final,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct JointRefinementObjectiveCacheKey {
    pub(crate) fidelity_tier: JointRefinementFidelityTier,
    pub(crate) parameter_bits: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct JointRefinementDeterministicCacheKey {
    pub(crate) parameter_bits: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JointRefinementDeterministicEvaluation {
    pub(crate) modeled_open_event_rate_per_hour: f64,
    pub(crate) raw_aggregated_likelihood_fit: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct JointRefinementSeedSummaryCacheKey {
    pub(crate) parameter_bits: Vec<u64>,
    pub(crate) simulation_seed: u64,
}

#[derive(Debug, Default)]
pub(crate) struct JointRefinementProfilingState {
    objective_cache_hit_count: AtomicUsize,
    objective_cache_miss_count: AtomicUsize,
    deterministic_cache_hit_count: AtomicUsize,
    deterministic_cache_miss_count: AtomicUsize,
    seed_summary_cache_hit_count: AtomicUsize,
    seed_summary_cache_miss_count: AtomicUsize,
    invalid_candidate_evaluation_count: AtomicUsize,
    cumulative_deterministic_evaluation_millis: AtomicU64,
    cumulative_seed_summary_simulation_millis: AtomicU64,
}

impl JointRefinementProfilingState {
    pub(crate) fn record_objective_cache_hit(&self) {
        self.objective_cache_hit_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_objective_cache_miss(&self) {
        self.objective_cache_miss_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_deterministic_cache_hit(&self) {
        self.deterministic_cache_hit_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_deterministic_cache_miss(&self) {
        self.deterministic_cache_miss_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_seed_summary_cache_hit(&self) {
        self.seed_summary_cache_hit_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_seed_summary_cache_miss(&self, miss_count: usize) {
        self.seed_summary_cache_miss_count
            .fetch_add(miss_count, Ordering::Relaxed);
    }

    pub(crate) fn record_invalid_candidate_evaluation(&self) {
        self.invalid_candidate_evaluation_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_deterministic_evaluation_duration(&self, elapsed_millis: u64) {
        self.cumulative_deterministic_evaluation_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_seed_summary_simulation_duration(&self, elapsed_millis: u64) {
        self.cumulative_seed_summary_simulation_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn snapshot_runtime_profile(
        &self,
        restart_optimization_wall_clock_millis: u64,
        coarse_consistency_wall_clock_millis: u64,
        final_seed_evaluation_wall_clock_millis: u64,
        final_assembly_wall_clock_millis: u64,
    ) -> JointRefinementStageRuntimeProfile {
        JointRefinementStageRuntimeProfile {
            restart_optimization_wall_clock_millis,
            coarse_consistency_wall_clock_millis,
            final_seed_evaluation_wall_clock_millis,
            final_assembly_wall_clock_millis,
            objective_cache_hit_count: self.objective_cache_hit_count.load(Ordering::Relaxed),
            objective_cache_miss_count: self.objective_cache_miss_count.load(Ordering::Relaxed),
            deterministic_cache_hit_count: self
                .deterministic_cache_hit_count
                .load(Ordering::Relaxed),
            deterministic_cache_miss_count: self
                .deterministic_cache_miss_count
                .load(Ordering::Relaxed),
            seed_summary_cache_hit_count: self.seed_summary_cache_hit_count.load(Ordering::Relaxed),
            seed_summary_cache_miss_count: self
                .seed_summary_cache_miss_count
                .load(Ordering::Relaxed),
            invalid_candidate_evaluation_count: self
                .invalid_candidate_evaluation_count
                .load(Ordering::Relaxed),
            cumulative_deterministic_evaluation_millis: self
                .cumulative_deterministic_evaluation_millis
                .load(Ordering::Relaxed),
            cumulative_seed_summary_simulation_millis: self
                .cumulative_seed_summary_simulation_millis
                .load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementActiveParameterLayout {
    pub(crate) hawkes_block: Vec<usize>,
    pub(crate) nig_block: Vec<usize>,
    pub(crate) coupling_block: Vec<usize>,
    pub(crate) log_lambda_scale_index: usize,
    pub(crate) log_beta_scale_index: usize,
    pub(crate) log_self_excitation_scale_index: usize,
    pub(crate) log_cross_excitation_scale_index: usize,
    pub(crate) known_activity_scale_index: Option<usize>,
    pub(crate) surprise_activity_scale_index: Option<usize>,
    pub(crate) surprise_baseline_scale_index: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementObservedData {
    pub(crate) observed_open_event_rate_per_hour: f64,
    pub(crate) fallback_sell_share_proxy: f64,
    pub(crate) observed_tick_continuous_return_calibration_sample:
        ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(crate) observed_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) observed_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) observed_tick_intraday_mean_absolute_returns: Vec<f64>,
    pub(crate) observed_tick_shape_returns: Vec<f64>,
    pub(crate) observed_5m_shape_returns: Vec<f64>,
    pub(crate) observed_15m_shape_returns: Vec<f64>,
    pub(crate) observed_tick_return_scale_summary: JointRefinementReturnScaleSummary,
    pub(crate) observed_5m_return_scale_summary: JointRefinementReturnScaleSummary,
    pub(crate) observed_15m_return_scale_summary: JointRefinementReturnScaleSummary,
    pub(crate) observed_5m_business_time_bucket_thresholds:
        JointRefinementBusinessTimeBucketThresholds,
    pub(crate) observed_15m_business_time_bucket_thresholds:
        JointRefinementBusinessTimeBucketThresholds,
    pub(crate) observed_5m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    pub(crate) observed_15m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    pub(crate) observed_5m_intraday_mean_absolute_returns: Vec<f64>,
    pub(crate) observed_15m_intraday_mean_absolute_returns: Vec<f64>,
    pub(crate) observed_hourly_tick_counts: Vec<f64>,
    pub(crate) observed_15m_rv_activity_metric: JointRefinementRollingVarianceActivityMetric,
    pub(crate) observed_log_price_span: f64,
    pub(crate) hourly_simulation_start_utc: DateTime<Utc>,
    pub(crate) hourly_simulation_end_utc: DateTime<Utc>,
    pub(crate) hourly_simulation_interval_count: usize,
    pub(crate) surprise_weekly_hour_utc_multipliers: Vec<f64>,
    pub(crate) simulated_5m_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) simulated_15m_interval_template: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) simulated_hourly_interval_template:
        Vec<JointRefinementAggregatedIntervalObservation>,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementBaselineBuckets {
    pub(crate) aggregated_likelihood_fit: f64,
    pub(crate) aggregated_return_shape_fit: f64,
    pub(crate) return_asymmetry_fit: f64,
    pub(crate) tick_return_asymmetry_fit: f64,
    pub(crate) aggregated_return_scale_fit: f64,
    pub(crate) tick_return_scale_fit: f64,
    pub(crate) business_time_return_scale_fit: f64,
    pub(crate) intraday_return_level_fit: f64,
    pub(crate) activity_shape_fit: f64,
    pub(crate) activity_level_fit: f64,
    pub(crate) rv_activity_coupling_fit: f64,
    pub(crate) price_span_sanity_fit: f64,
    pub(crate) option_spread_normalized_price_fit: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementContext {
    pub(crate) stage_thread_pool: Option<Arc<rayon::ThreadPool>>,
    pub(crate) summary_simulation_scheduler: Arc<JointRefinementSummarySimulationScheduler>,
    pub(crate) simulation_infrastructure_failure: Arc<Mutex<Option<String>>>,
    pub(crate) calendar_registry: ExchangeCalendarRegistry,
    pub(crate) exchange_code: String,
    pub(crate) runtime_hourly_seasonality_factors: Vec<f64>,
    pub(crate) max_runtime_hourly_seasonality_factor: f64,
    pub(crate) observed_data: JointRefinementObservedData,
    pub(crate) baseline_modeled_open_event_rate_per_hour: f64,
    pub(crate) baseline_hawkes_fitted_parameters: HawkesStageFittedParameters,
    pub(crate) baseline_return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
    pub(crate) active_parameter_layout: JointRefinementActiveParameterLayout,
    pub(crate) lower_bounds: Vec<f64>,
    pub(crate) upper_bounds: Vec<f64>,
    pub(crate) local_simulation_seed: u64,
    pub(crate) final_simulation_seeds: Vec<u64>,
    pub(crate) local_baseline_buckets: JointRefinementBaselineBuckets,
    pub(crate) final_baseline_buckets: JointRefinementBaselineBuckets,
    pub(crate) objective_cache:
        Arc<Mutex<BTreeMap<JointRefinementObjectiveCacheKey, JointRefinementCandidateEvaluation>>>,
    pub(crate) deterministic_evaluation_cache: Arc<
        Mutex<
            BTreeMap<JointRefinementDeterministicCacheKey, JointRefinementDeterministicEvaluation>,
        >,
    >,
    pub(crate) seed_summary_cache: Arc<
        Mutex<
            BTreeMap<
                JointRefinementSeedSummaryCacheKey,
                Result<JointRefinementSeedSummary, String>,
            >,
        >,
    >,
    pub(crate) option_pricing_context: Option<JointRefinementOptionPricingContext>,
    pub(crate) profiling_state: Arc<JointRefinementProfilingState>,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementOptionNodeLocation {
    pub(crate) grid_index: usize,
    pub(crate) maturity_index: usize,
    pub(crate) strike_index: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementOptionNodeDefinition {
    pub(crate) quote_timestamp_utc_nanos: i64,
    pub(crate) expiry_timestamp_utc_nanos: i64,
    pub(crate) strike: f64,
    pub(crate) payoff_type: VanillaOptionPayoffType,
    pub(crate) bid_price: f64,
    pub(crate) ask_price: f64,
    pub(crate) observed_mid_price: f64,
    pub(crate) location: JointRefinementOptionNodeLocation,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementOptionPricingContext {
    pub(crate) valuation_time_utc_nanos: i64,
    pub(crate) grid_batch_pricing_request: VanillaOptionGridBatchPricingRequest,
    pub(crate) boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    pub(crate) runtime_interval_schedule: PricingIntervalSchedule,
    pub(crate) interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
    pub(crate) snapshot_maturity_intervals: Vec<f64>,
    pub(crate) snapshot_maturity_terminal_interval_indices: Vec<usize>,
    pub(crate) carry_input: janiform_calibration_artifacts::JointRefinementStageCarryInput,
    pub(crate) observed_nodes: Vec<JointRefinementOptionNodeDefinition>,
    pub(crate) cpu_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementCandidateEvaluation {
    pub(crate) fitted_parameters: JointRefinementStageFittedParameters,
    pub(crate) breakdown: JointRefinementStageObjectiveBreakdown,
    pub(crate) objective_terms: Vec<JointRefinementStageObjectiveTermDiagnostic>,
    pub(crate) invalid_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementRestartOutcome {
    pub(crate) restart_id: String,
    pub(crate) parameter_vector: Vec<f64>,
    pub(crate) local_evaluation: JointRefinementCandidateEvaluation,
    pub(crate) final_evaluation: JointRefinementCandidateEvaluation,
    pub(crate) local_evaluation_count: usize,
    pub(crate) stop_reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct JointRefinementCoarseConsistencySummary {
    pub(crate) raw_fit: f64,
    pub(crate) normalized_fit: f64,
    pub(crate) admissibility_threshold: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementSeedSummary {
    pub simulated_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub simulated_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub simulated_tick_shape_returns: Vec<f64>,
    pub simulated_5m_shape_returns: Vec<f64>,
    pub simulated_15m_shape_returns: Vec<f64>,
    pub simulated_tick_return_scale_summary: JointRefinementReturnScaleSummary,
    pub simulated_5m_return_scale_summary: JointRefinementReturnScaleSummary,
    pub simulated_15m_return_scale_summary: JointRefinementReturnScaleSummary,
    pub simulated_5m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    pub simulated_15m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    pub simulated_tick_intraday_mean_absolute_returns: Vec<f64>,
    pub simulated_5m_intraday_mean_absolute_returns: Vec<f64>,
    pub simulated_15m_intraday_mean_absolute_returns: Vec<f64>,
    pub simulated_hourly_tick_counts: Vec<f64>,
    pub simulated_15m_rv_activity_metric: JointRefinementRollingVarianceActivityMetric,
}
