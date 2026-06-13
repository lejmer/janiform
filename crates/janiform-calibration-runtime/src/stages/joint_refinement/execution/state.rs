use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use janiform_calibration_artifacts::{
    CalibrationLossEvaluation, CalibrationObjectivePolicy, HawkesStageFittedParameters,
    JointRefinementStageFittedParameters, JointRefinementStageObjectiveBreakdown,
    JointRefinementStageOptionAggregateDiagnostic, JointRefinementStageOptionNodeDiagnostic,
    JointRefinementStageOptionSearchSelectionAgreementDiagnostics,
    JointRefinementStageRuntimeProfile, ReturnComponentsStageFittedParameters,
};
use janiform_engine_pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
    VanillaOptionGridBatchPricingRequest, VanillaOptionPayoffType,
    VanillaOptionPricingIntervalFactors,
};
use janiform_foundations::cancellation::CancellationToken;
use serde::{Deserialize, Serialize};

use crate::stages::joint_metrics::{
    JointRefinementAggregatedIntervalObservation,
    JointRefinementBusinessTimeBucketReturnScaleSummaries,
    JointRefinementBusinessTimeBucketThresholds, JointRefinementReturnScaleSummary,
    JointRefinementRollingVarianceActivityMetric,
};
use crate::stages::joint_refinement::contracts::JointRefinementStageObjectiveTermDiagnostic;
use crate::stages::joint_refinement::option_surface_tiering::{
    JointRefinementOptionSurfaceTierKind, JointRefinementOptionSurfaceTierSet,
};
use crate::stages::long_variance::metrics::LongVarianceHorizonMomentSummary;
use crate::stages::return_components::{
    ContinuousLikelihoodEvaluatorCounts, ContinuousLikelihoodFidelityTier,
    ContinuousReturnOptimizerContext, ReturnComponentsStageContinuousReturnCalibrationSample,
};
use crate::stages::seasonality::FourierBusinessTimeClockModel;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::super::worker_runtime::JointRefinementSummarySimulationScheduler;
use super::option_refinement::objective_gradient::{
    JointRefinementOptionNodeResidualSensitivity, JointRefinementOptionTierScoreGradientArtifact,
};
use super::option_refinement::reduced_model::JointRefinementOptionReducedModelArtifact;

pub(crate) const JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND: f64 = 1.386_294_361_119_890_6;
pub(crate) const JOINT_REFINEMENT_PARAMETER_FAMILY_COUNT: usize = 6;
pub(crate) const JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR: f64 = 1.0e-8;

type JointRefinementSeedSummaryCache = Arc<
    Mutex<BTreeMap<JointRefinementSeedSummaryCacheKey, Result<JointRefinementSeedSummary, String>>>,
>;

type JointRefinementOptionPricingCache = Arc<
    Mutex<
        BTreeMap<
            JointRefinementOptionPricingCacheKey,
            Result<Arc<JointRefinementOptionTierPricingArtifact>, String>,
        >,
    >,
>;

type JointRefinementObservedReturnShapeStatisticCache = Arc<
    Mutex<
        BTreeMap<
            JointRefinementObservedReturnShapeStatisticCacheKey,
            Arc<[JointRefinementReturnShapeStatisticEstimate]>,
        >,
    >,
>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum JointRefinementFidelityTier {
    CoarseLocal,
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
    pub(crate) likelihood_fidelity_tier: ContinuousLikelihoodFidelityTier,
    pub(crate) parameter_bits: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct JointRefinementAggregatedLikelihoodContextCacheKey {
    pub(crate) likelihood_fidelity_tier: ContinuousLikelihoodFidelityTier,
    pub(crate) modeled_open_event_rate_per_hour_bits: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JointRefinementDeterministicEvaluation {
    pub(crate) modeled_open_event_rate_per_hour: f64,
    pub(crate) raw_aggregated_likelihood_fit: f64,
    pub(crate) component_shifted_average_negative_log_likelihoods: [f64; 3],
    pub(crate) exact_likelihood_evaluator_counts: ContinuousLikelihoodEvaluatorCounts,
    pub(crate) expected_hourly_tick_counts: Option<Vec<f64>>,
    pub(crate) raw_expected_activity_shape_fit: Option<f64>,
    pub(crate) raw_expected_activity_level_fit: Option<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementAggregatedLikelihoodContexts {
    pub(crate) tick: ContinuousReturnOptimizerContext,
    pub(crate) one_hour: ContinuousReturnOptimizerContext,
    pub(crate) one_day: ContinuousReturnOptimizerContext,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct JointRefinementReturnShapeStatisticEstimate {
    pub(crate) metric_name: &'static str,
    pub(crate) value: f64,
    pub(crate) standard_error: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum JointRefinementReturnShapeHorizon {
    Tick,
    OneHour,
    OneDay,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct JointRefinementObservedReturnShapeStatisticCacheKey {
    pub(crate) horizon: JointRefinementReturnShapeHorizon,
    pub(crate) temporal_block_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct JointRefinementSeedSummaryCacheKey {
    pub(crate) parameter_bits: Vec<u64>,
    pub(crate) simulation_seed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct JointRefinementOptionPricingCacheKey {
    pub(crate) tier_kind: JointRefinementOptionSurfaceTierKind,
    #[serde(default)]
    pub(crate) include_node_price_sensitivities: bool,
    pub(crate) source_dynamics_parameter_bits: Vec<u64>,
    pub(crate) pricing_parameter_bits: Vec<u64>,
}

#[derive(Debug, Default)]
pub(crate) struct JointRefinementProfilingState {
    objective_cache_hit_count: AtomicUsize,
    objective_cache_miss_count: AtomicUsize,
    deterministic_cache_hit_count: AtomicUsize,
    deterministic_cache_miss_count: AtomicUsize,
    seed_summary_cache_hit_count: AtomicUsize,
    seed_summary_cache_miss_count: AtomicUsize,
    aggregated_likelihood_context_cache_hit_count: AtomicUsize,
    aggregated_likelihood_context_cache_miss_count: AtomicUsize,
    option_pricing_cache_hit_count: AtomicUsize,
    option_pricing_cache_miss_count: AtomicUsize,
    option_pricing_run_count: AtomicUsize,
    invalid_candidate_evaluation_count: AtomicUsize,
    cumulative_deterministic_evaluation_millis: AtomicU64,
    cumulative_modeled_rate_millis: AtomicU64,
    cumulative_likelihood_context_resolution_millis: AtomicU64,
    cumulative_likelihood_context_1h_sample_millis: AtomicU64,
    cumulative_likelihood_context_1d_sample_millis: AtomicU64,
    cumulative_likelihood_context_tick_sample_millis: AtomicU64,
    cumulative_likelihood_context_tick_optimizer_millis: AtomicU64,
    cumulative_likelihood_context_1h_optimizer_millis: AtomicU64,
    cumulative_likelihood_context_1d_optimizer_millis: AtomicU64,
    cumulative_tick_likelihood_millis: AtomicU64,
    cumulative_1h_likelihood_millis: AtomicU64,
    cumulative_1d_likelihood_millis: AtomicU64,
    cumulative_expected_activity_counts_millis: AtomicU64,
    cumulative_deterministic_activity_shape_millis: AtomicU64,
    cumulative_deterministic_activity_level_millis: AtomicU64,
    cumulative_seed_summary_simulation_millis: AtomicU64,
    cumulative_raw_seed_metrics_millis: AtomicU64,
    cumulative_loss_build_millis: AtomicU64,
    cumulative_option_pricing_millis: AtomicU64,
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

    pub(crate) fn record_aggregated_likelihood_context_cache_hit(&self) {
        self.aggregated_likelihood_context_cache_hit_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_aggregated_likelihood_context_cache_miss(&self) {
        self.aggregated_likelihood_context_cache_miss_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_option_pricing_cache_hit(&self) {
        self.option_pricing_cache_hit_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_option_pricing_cache_miss(&self) {
        self.option_pricing_cache_miss_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_option_pricing_run_duration(&self, elapsed_millis: u64) {
        self.option_pricing_run_count
            .fetch_add(1, Ordering::Relaxed);
        self.cumulative_option_pricing_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_invalid_candidate_evaluation(&self) {
        self.invalid_candidate_evaluation_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_deterministic_evaluation_duration(&self, elapsed_millis: u64) {
        self.cumulative_deterministic_evaluation_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_modeled_rate_duration(&self, elapsed_millis: u64) {
        self.cumulative_modeled_rate_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_likelihood_context_resolution_duration(&self, elapsed_millis: u64) {
        self.cumulative_likelihood_context_resolution_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_likelihood_context_1h_sample_duration(&self, elapsed_millis: u64) {
        self.cumulative_likelihood_context_1h_sample_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_likelihood_context_1d_sample_duration(&self, elapsed_millis: u64) {
        self.cumulative_likelihood_context_1d_sample_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_likelihood_context_tick_sample_duration(&self, elapsed_millis: u64) {
        self.cumulative_likelihood_context_tick_sample_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_likelihood_context_tick_optimizer_duration(&self, elapsed_millis: u64) {
        self.cumulative_likelihood_context_tick_optimizer_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_likelihood_context_1h_optimizer_duration(&self, elapsed_millis: u64) {
        self.cumulative_likelihood_context_1h_optimizer_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_likelihood_context_1d_optimizer_duration(&self, elapsed_millis: u64) {
        self.cumulative_likelihood_context_1d_optimizer_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_tick_likelihood_duration(&self, elapsed_millis: u64) {
        self.cumulative_tick_likelihood_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_1h_likelihood_duration(&self, elapsed_millis: u64) {
        self.cumulative_1h_likelihood_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_1d_likelihood_duration(&self, elapsed_millis: u64) {
        self.cumulative_1d_likelihood_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_expected_activity_counts_duration(&self, elapsed_millis: u64) {
        self.cumulative_expected_activity_counts_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_deterministic_activity_shape_duration(&self, elapsed_millis: u64) {
        self.cumulative_deterministic_activity_shape_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_deterministic_activity_level_duration(&self, elapsed_millis: u64) {
        self.cumulative_deterministic_activity_level_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_seed_summary_simulation_duration(&self, elapsed_millis: u64) {
        self.cumulative_seed_summary_simulation_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_raw_seed_metrics_duration(&self, elapsed_millis: u64) {
        self.cumulative_raw_seed_metrics_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn record_loss_build_duration(&self, elapsed_millis: u64) {
        self.cumulative_loss_build_millis
            .fetch_add(elapsed_millis, Ordering::Relaxed);
    }

    pub(crate) fn mean_option_pricing_run_wall_clock_millis(&self) -> Option<f64> {
        let option_pricing_run_count = self.option_pricing_run_count.load(Ordering::Relaxed);
        if option_pricing_run_count == 0 {
            return None;
        }

        let cumulative_option_pricing_millis = self
            .cumulative_option_pricing_millis
            .load(Ordering::Relaxed);

        Some(cumulative_option_pricing_millis as f64 / option_pricing_run_count as f64)
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
            aggregated_likelihood_context_cache_hit_count: self
                .aggregated_likelihood_context_cache_hit_count
                .load(Ordering::Relaxed),
            aggregated_likelihood_context_cache_miss_count: self
                .aggregated_likelihood_context_cache_miss_count
                .load(Ordering::Relaxed),
            option_pricing_cache_hit_count: self
                .option_pricing_cache_hit_count
                .load(Ordering::Relaxed),
            option_pricing_cache_miss_count: self
                .option_pricing_cache_miss_count
                .load(Ordering::Relaxed),
            option_pricing_run_count: self.option_pricing_run_count.load(Ordering::Relaxed),
            invalid_candidate_evaluation_count: self
                .invalid_candidate_evaluation_count
                .load(Ordering::Relaxed),
            cumulative_deterministic_evaluation_millis: self
                .cumulative_deterministic_evaluation_millis
                .load(Ordering::Relaxed),
            cumulative_modeled_rate_millis: self
                .cumulative_modeled_rate_millis
                .load(Ordering::Relaxed),
            cumulative_likelihood_context_resolution_millis: self
                .cumulative_likelihood_context_resolution_millis
                .load(Ordering::Relaxed),
            cumulative_likelihood_context_1h_sample_millis: self
                .cumulative_likelihood_context_1h_sample_millis
                .load(Ordering::Relaxed),
            cumulative_likelihood_context_1d_sample_millis: self
                .cumulative_likelihood_context_1d_sample_millis
                .load(Ordering::Relaxed),
            cumulative_likelihood_context_tick_sample_millis: self
                .cumulative_likelihood_context_tick_sample_millis
                .load(Ordering::Relaxed),
            cumulative_likelihood_context_tick_optimizer_millis: self
                .cumulative_likelihood_context_tick_optimizer_millis
                .load(Ordering::Relaxed),
            cumulative_likelihood_context_1h_optimizer_millis: self
                .cumulative_likelihood_context_1h_optimizer_millis
                .load(Ordering::Relaxed),
            cumulative_likelihood_context_1d_optimizer_millis: self
                .cumulative_likelihood_context_1d_optimizer_millis
                .load(Ordering::Relaxed),
            cumulative_tick_likelihood_millis: self
                .cumulative_tick_likelihood_millis
                .load(Ordering::Relaxed),
            cumulative_1h_likelihood_millis: self
                .cumulative_1h_likelihood_millis
                .load(Ordering::Relaxed),
            cumulative_1d_likelihood_millis: self
                .cumulative_1d_likelihood_millis
                .load(Ordering::Relaxed),
            cumulative_expected_activity_counts_millis: self
                .cumulative_expected_activity_counts_millis
                .load(Ordering::Relaxed),
            cumulative_deterministic_activity_shape_millis: self
                .cumulative_deterministic_activity_shape_millis
                .load(Ordering::Relaxed),
            cumulative_deterministic_activity_level_millis: self
                .cumulative_deterministic_activity_level_millis
                .load(Ordering::Relaxed),
            cumulative_seed_summary_simulation_millis: self
                .cumulative_seed_summary_simulation_millis
                .load(Ordering::Relaxed),
            cumulative_raw_seed_metrics_millis: self
                .cumulative_raw_seed_metrics_millis
                .load(Ordering::Relaxed),
            cumulative_loss_build_millis: self.cumulative_loss_build_millis.load(Ordering::Relaxed),
            cumulative_option_pricing_millis: self
                .cumulative_option_pricing_millis
                .load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementActiveParameterLayout {
    pub(crate) hawkes_block: Vec<usize>,
    pub(crate) nig_block: Vec<usize>,
    pub(crate) return_clock_block: Vec<usize>,
    pub(crate) long_variance_block: Vec<usize>,
    pub(crate) option_pricing_kernel_block: Vec<usize>,
    pub(crate) coupling_block: Vec<usize>,
    pub(crate) log_stationary_event_rate_scale_index: usize,
    pub(crate) log_beta_scale_index: usize,
    pub(crate) log_self_excitation_scale_index: usize,
    pub(crate) log_cross_excitation_scale_index: usize,
    pub(crate) log_buy_excitation_return_clock_scale_index: usize,
    pub(crate) log_sell_excitation_return_clock_scale_index: usize,
    pub(crate) log_long_variance_mean_reversion_rate_scale_index: usize,
    pub(crate) long_variance_vol_of_vol_index: usize,
    pub(crate) long_variance_buy_event_loading_index: usize,
    pub(crate) long_variance_sell_event_loading_index: usize,
    pub(crate) option_pricing_log_buy_activity_premium_index: usize,
    pub(crate) option_pricing_log_sell_activity_premium_index: usize,
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
    pub(crate) observed_tick_continuous_return_optimizer_context: ContinuousReturnOptimizerContext,
    pub(crate) weighted_tick_continuous_return_optimizer_context: ContinuousReturnOptimizerContext,
    pub(crate) observed_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) observed_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) observed_1h_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) observed_1d_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub(crate) observed_tick_intraday_mean_absolute_returns: Vec<f64>,
    pub(crate) observed_tick_shape_returns: Vec<f64>,
    pub(crate) observed_1h_shape_returns: Vec<f64>,
    pub(crate) observed_1d_shape_returns: Vec<f64>,
    pub(crate) observed_tick_return_scale_summary: JointRefinementReturnScaleSummary,
    pub(crate) observed_1h_return_scale_summary: JointRefinementReturnScaleSummary,
    pub(crate) observed_1d_return_scale_summary: JointRefinementReturnScaleSummary,
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
    pub(crate) observed_long_variance_horizon_summaries: Vec<LongVarianceHorizonMomentSummary>,
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
pub(crate) struct JointRefinementContext {
    pub(crate) cancellation_token: CancellationToken,
    pub(crate) stage_thread_pool: Option<Arc<rayon::ThreadPool>>,
    pub(crate) summary_simulation_scheduler: Arc<JointRefinementSummarySimulationScheduler>,
    pub(crate) simulation_infrastructure_failure: Arc<Mutex<Option<String>>>,
    pub(crate) option_pricing_execution_failure: Arc<Mutex<Option<String>>>,
    pub(crate) calendar_registry: ExchangeCalendarRegistry,
    pub(crate) exchange_code: String,
    pub(crate) objective_policy: CalibrationObjectivePolicy,
    pub(crate) seasonality_clock_model: FourierBusinessTimeClockModel,
    pub(crate) runtime_interval_seasonality_factors: Vec<f64>,
    pub(crate) observed_data: JointRefinementObservedData,
    pub(crate) baseline_modeled_open_event_rate_per_hour: f64,
    pub(crate) baseline_hawkes_fitted_parameters: HawkesStageFittedParameters,
    pub(crate) baseline_return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
    pub(crate) baseline_mean_reversion_fitted_parameters:
        janiform_model_kernels::mean_reversion::MeanReversionFittedParameters,
    pub(crate) active_parameter_layout: JointRefinementActiveParameterLayout,
    pub(crate) lower_bounds: Vec<f64>,
    pub(crate) upper_bounds: Vec<f64>,
    pub(crate) option_pricing_refinement_enabled: bool,
    pub(crate) local_simulation_seed: u64,
    pub(crate) final_simulation_seeds: Vec<u64>,
    pub(crate) objective_cache:
        Arc<Mutex<BTreeMap<JointRefinementObjectiveCacheKey, JointRefinementCandidateEvaluation>>>,
    pub(crate) deterministic_evaluation_cache: Arc<
        Mutex<
            BTreeMap<JointRefinementDeterministicCacheKey, JointRefinementDeterministicEvaluation>,
        >,
    >,
    pub(crate) aggregated_likelihood_context_cache: Arc<
        Mutex<
            BTreeMap<
                JointRefinementAggregatedLikelihoodContextCacheKey,
                Arc<JointRefinementAggregatedLikelihoodContexts>,
            >,
        >,
    >,
    pub(crate) observed_return_shape_statistic_cache:
        JointRefinementObservedReturnShapeStatisticCache,
    pub(crate) seed_summary_cache: JointRefinementSeedSummaryCache,
    pub(crate) option_pricing_cache: JointRefinementOptionPricingCache,
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
    pub(crate) maturity_weight: usize,
    pub(crate) location: JointRefinementOptionNodeLocation,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementOptionTierPricingInputs {
    pub(crate) runtime_interval_schedule: PricingIntervalSchedule,
    pub(crate) interval_factor_grids: Vec<VanillaOptionPricingIntervalFactors>,
    pub(crate) grid_batch_pricing_request: VanillaOptionGridBatchPricingRequest,
    pub(crate) spectral_collocation_node_count_override: Option<usize>,
    pub(crate) observed_nodes: Vec<JointRefinementOptionNodeDefinition>,
    pub(crate) maturity_intervals: Vec<f64>,
    pub(crate) maturity_terminal_interval_indices: Vec<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementOptionTierPricingInputSet {
    pub(crate) search: JointRefinementOptionTierPricingInputs,
    pub(crate) selection: JointRefinementOptionTierPricingInputs,
    pub(crate) audit: JointRefinementOptionTierPricingInputs,
}

impl JointRefinementOptionTierPricingInputSet {
    pub(crate) fn tier(
        &self,
        tier_kind: JointRefinementOptionSurfaceTierKind,
    ) -> &JointRefinementOptionTierPricingInputs {
        match tier_kind {
            JointRefinementOptionSurfaceTierKind::Search => &self.search,
            JointRefinementOptionSurfaceTierKind::Selection => &self.selection,
            JointRefinementOptionSurfaceTierKind::Audit => &self.audit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementOptionTierScore {
    pub(crate) root_mean_square_spread_normalized_residual: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementOptionMaturityScoreArtifact {
    pub(crate) expiry_timestamp_utc_nanos: i64,
    pub(crate) maturity_weight: usize,
    pub(crate) quote_count: usize,
    pub(crate) root_mean_square_spread_normalized_residual: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementOptionTierPricingArtifact {
    pub(crate) cache_key: JointRefinementOptionPricingCacheKey,
    pub(crate) tier_kind: JointRefinementOptionSurfaceTierKind,
    pub(crate) runtime_interval_schedule: PricingIntervalSchedule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) spectral_collocation_node_count: Option<usize>,
    pub(crate) score: JointRefinementOptionTierScore,
    pub(crate) loss_evaluation: CalibrationLossEvaluation,
    pub(crate) maturity_scores: Vec<JointRefinementOptionMaturityScoreArtifact>,
    pub(crate) node_diagnostics: Vec<JointRefinementStageOptionNodeDiagnostic>,
    pub(crate) aggregate_diagnostics: Vec<JointRefinementStageOptionAggregateDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) score_gradient_artifact: Option<JointRefinementOptionTierScoreGradientArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reduced_model_artifact: Option<JointRefinementOptionReducedModelArtifact>,
    #[serde(skip)]
    pub(crate) node_sensitivities: Option<Vec<JointRefinementOptionNodeResidualSensitivity>>,
    pub(crate) maturity_intervals: Vec<f64>,
    pub(crate) maturity_terminal_interval_indices: Vec<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementOptionPricingContext {
    pub(crate) valuation_time_utc_nanos: i64,
    pub(crate) boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    pub(crate) snapshot_maturity_intervals: Vec<f64>,
    pub(crate) snapshot_maturity_terminal_interval_indices: Vec<usize>,
    pub(crate) option_surface_tiers: JointRefinementOptionSurfaceTierSet,
    pub(crate) tier_pricing_inputs: JointRefinementOptionTierPricingInputSet,
    pub(crate) carry_input: janiform_calibration_artifacts::JointRefinementStageCarryInput,
    pub(crate) cpu_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementRawAdmissibilityMetrics {
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
    pub(crate) long_variance_horizon_fit: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementCandidateEvaluation {
    pub(crate) fitted_parameters: JointRefinementStageFittedParameters,
    pub(crate) breakdown: JointRefinementStageObjectiveBreakdown,
    pub(crate) raw_admissibility_metrics: JointRefinementRawAdmissibilityMetrics,
    pub(crate) exact_likelihood_evaluator_counts: ContinuousLikelihoodEvaluatorCounts,
    pub(crate) objective_terms: Vec<JointRefinementStageObjectiveTermDiagnostic>,
    pub(crate) invalid_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementRestartOutcome {
    pub(crate) restart_id: String,
    pub(crate) parameter_vector: Vec<f64>,
    pub(crate) local_evaluation: JointRefinementCandidateEvaluation,
    pub(crate) final_evaluation: JointRefinementCandidateEvaluation,
    pub(crate) local_evaluation_count: usize,
    pub(crate) stop_reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JointRefinementStageRuntimeHandoff {
    pub(crate) baseline_restart_outcome: JointRefinementRestartOutcome,
    pub(crate) restart_outcomes: Vec<JointRefinementRestartOutcome>,
    pub(crate) restart_evaluation_timing_diagnostics:
        Vec<janiform_calibration_artifacts::JointRefinementStageRestartEvaluationTimingDiagnostic>,
    pub(crate) valid_restart_count: usize,
    pub(crate) best_restart: JointRefinementRestartOutcome,
    pub(crate) option_refinement_candidate_frontier: Vec<JointRefinementRestartOutcome>,
    pub(crate) selected_option_search_tier_pricing_artifact:
        Option<Arc<JointRefinementOptionTierPricingArtifact>>,
    pub(crate) selected_option_selection_tier_pricing_artifact:
        Option<Arc<JointRefinementOptionTierPricingArtifact>>,
    pub(crate) selected_option_audit_tier_pricing_artifact:
        Option<Arc<JointRefinementOptionTierPricingArtifact>>,
    pub(crate) option_search_selection_agreement:
        Option<JointRefinementStageOptionSearchSelectionAgreementDiagnostics>,
    pub(crate) warning: Option<String>,
    pub(crate) restart_optimization_wall_clock_millis: u64,
    pub(crate) coarse_consistency_wall_clock_millis: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementCoarseConsistencySummary {
    pub(crate) raw_fit: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementSeedSummary {
    pub simulated_5m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub simulated_15m_intervals: Vec<JointRefinementAggregatedIntervalObservation>,
    pub simulated_tick_shape_returns: Vec<f64>,
    pub simulated_5m_shape_returns: Vec<f64>,
    pub simulated_15m_shape_returns: Vec<f64>,
    pub simulated_1h_shape_returns: Vec<f64>,
    pub simulated_1d_shape_returns: Vec<f64>,
    pub simulated_tick_return_scale_summary: JointRefinementReturnScaleSummary,
    pub simulated_5m_return_scale_summary: JointRefinementReturnScaleSummary,
    pub simulated_15m_return_scale_summary: JointRefinementReturnScaleSummary,
    pub simulated_1h_return_scale_summary: JointRefinementReturnScaleSummary,
    pub simulated_1d_return_scale_summary: JointRefinementReturnScaleSummary,
    pub simulated_5m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    pub simulated_15m_business_time_return_scale_summaries:
        JointRefinementBusinessTimeBucketReturnScaleSummaries,
    pub simulated_tick_intraday_mean_absolute_returns: Vec<f64>,
    pub simulated_5m_intraday_mean_absolute_returns: Vec<f64>,
    pub simulated_15m_intraday_mean_absolute_returns: Vec<f64>,
    pub simulated_hourly_tick_counts: Vec<f64>,
    pub simulated_15m_rv_activity_metric: JointRefinementRollingVarianceActivityMetric,
    pub(crate) simulated_long_variance_horizon_summaries: Vec<LongVarianceHorizonMomentSummary>,
}
