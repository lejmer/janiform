use serde::{Deserialize, Serialize};

use crate::contracts::CalibrationLossEvaluation;
use janiform_engine_pricing::{PricingIntervalSchedule, VanillaOptionPricingKernelParameters};
use janiform_model_kernels::hawkes::{
    HawkesKernelParameters, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
};
use janiform_model_kernels::long_variance::LongVarianceParameters;
use janiform_model_kernels::market_pressure::MarketPressureParameters;
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;
use janiform_model_kernels::news::NewsProcessConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;
use janiform_model_kernels::timestamp_batching_surface::TimestampBatchingSurface;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HawkesStageFittedParameters {
    pub kernel_parameters: HawkesKernelParameters,
    pub structured_alpha: HawkesStructuredAlphaParameterization,
    pub structured_rho: HawkesStructuredRhoParameterization,
    pub self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
    pub stability_numeric_tolerance: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurpriseNewsDeterministicIntensityProfile {
    pub weekly_utc_profile_multipliers: Vec<f64>,
    pub profile_resolution_hours: usize,
    pub fit_policy: String,
    pub normalization_policy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceDiscretizationSurface {
    pub tick_size: Option<f64>,
    pub tick_size_source: String,
    pub inference_policy: String,
    pub observation_policy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnComponentsStageFittedParameters {
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    pub news_process_configuration: NewsProcessConfiguration,
    pub surprise_deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile,
    pub return_clock_exposure_parameters: ReturnClockExposureParameters,
    pub long_variance_parameters: LongVarianceParameters,
    pub market_pressure_parameters: MarketPressureParameters,
    pub price_discretization_surface: PriceDiscretizationSurface,
    pub timestamp_batching_surface: TimestampBatchingSurface,
    pub mean_reversion_fitted_parameters: MeanReversionFittedParameters,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LongVarianceStageHorizonDiagnostic {
    /// Stable horizon label used in reports.
    pub horizon_label: String,
    /// Horizon duration in physical hours.
    pub horizon_hours: f64,
    /// Number of finite log-return observations at this horizon.
    pub sample_count: usize,
    /// Mean squared log-return at this horizon. This is a variance scale, not volatility.
    pub realized_variance: Option<f64>,
    /// Sample variance of squared log-returns at this horizon.
    pub variance_of_variance: Option<f64>,
    /// Adjusted skewness of log-returns at this horizon.
    pub return_skewness: Option<f64>,
    /// Adjusted excess kurtosis of log-returns at this horizon.
    pub return_excess_kurtosis: Option<f64>,
    /// Correlation between interval activity count and squared log-return.
    pub activity_variance_correlation: Option<f64>,
    /// Correlation between signed activity and the next interval's squared log-return.
    pub signed_activity_future_variance_relation: Option<f64>,
    /// Standard error for `realized_variance` in squared log-return units.
    pub realized_variance_standard_error: Option<f64>,
    /// Standard error for `variance_of_variance` in squared log-return units squared.
    pub variance_of_variance_standard_error: Option<f64>,
    /// Standard error for `return_skewness`.
    pub return_skewness_standard_error: Option<f64>,
    /// Standard error for `return_excess_kurtosis`.
    pub return_excess_kurtosis_standard_error: Option<f64>,
    /// Standard error for `activity_variance_correlation`.
    pub activity_variance_correlation_standard_error: Option<f64>,
    /// Standard error for `signed_activity_future_variance_relation`.
    pub signed_activity_future_variance_relation_standard_error: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LongVarianceStagePersistenceLagDiagnostic {
    /// Lag duration used for the persistence correlation.
    pub lag_hours: f64,
    /// Number of aligned deseasonalized squared-return pairs at this lag.
    pub pair_count: usize,
    /// Pearson correlation between lagged and current deseasonalized squared returns.
    pub correlation: Option<f64>,
    /// Mean-reversion rate implied by the correlation under exponential decay.
    pub implied_mean_reversion_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LongVarianceStageDiagnostics {
    pub estimation_policy: String,
    pub stationary_normalization_policy: String,
    pub interval_source_selection_policy: String,
    pub return_interval_tick_count: usize,
    pub return_interval_coarse_candidate_count: usize,
    pub return_interval_coarse_gap_fill_count: usize,
    pub return_interval_tick_coarse_overlap_count: usize,
    pub return_interval_count: usize,
    pub activity_interval_count: usize,
    pub persistence_lag_diagnostics: Vec<LongVarianceStagePersistenceLagDiagnostic>,
    pub selected_mean_reversion_rate: f64,
    pub event_loading_fit_candidate_pair_count: usize,
    pub event_loading_fit_pair_count: usize,
    pub event_loading_fit_coverage_ratio: f64,
    pub stationary_return_energy_rate_per_hour: f64,
    pub return_energy_fit_candidate_pair_count: usize,
    pub return_energy_fit_pair_count: usize,
    pub return_energy_fit_coverage_ratio: f64,
    pub selected_candidate_id: String,
    pub horizon_diagnostics: Vec<LongVarianceStageHorizonDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewsShocksStageDiagnostics {
    pub estimation_policy: String,
    pub selected_candidate_id: String,
    pub surprise_open_intensity: f64,
    pub surprise_closed_intensity: f64,
    pub severity_shape: f64,
    pub expected_severity: f64,
    pub expected_sqrt_severity: f64,
    pub known_activity_coupling_scale: f64,
    pub surprise_activity_coupling_scale: f64,
    pub known_return_jump_drift: f64,
    pub known_return_jump_volatility: f64,
    pub surprise_return_jump_drift: f64,
    pub surprise_return_jump_volatility: f64,
}

/// Scale accounting for the dynamic parameter handoff performed by return backfit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnBackfitStageScaleHandoffDiagnostic {
    /// Selected return-components NIG delta supplied to return backfit.
    pub selected_nig_delta: Option<f64>,
    /// NIG delta after scaling by the dynamic-clock variance handoff multiplier.
    pub post_handoff_nig_delta: Option<f64>,
    /// Selected return-components model variance scale supplied to return backfit.
    pub selected_continuous_model_variance_scale: Option<f64>,
    /// Multiplicative return-clock scale implied by the fitted long-variance and news state.
    pub long_variance_clock_scale_multiplier: Option<f64>,
    /// Multiplicative NIG delta adjustment applied during return backfit.
    pub delta_handoff_multiplier: Option<f64>,
    /// Base continuous-model variance scale after NIG delta handoff.
    pub post_handoff_base_variance_scale: Option<f64>,
    /// Dynamic-clock continuous-model variance scale after NIG delta handoff.
    pub post_handoff_dynamic_variance_scale: Option<f64>,
    /// Observed return variance used as a diagnostic reference.
    pub observed_reference_variance_scale: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnBackfitStageObjectiveBlockDiagnostic {
    pub target_block: String,
    pub before_objective: Option<f64>,
    pub after_objective: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnBackfitStageDiagnostics {
    pub estimation_policy: String,
    pub selected_candidate_id: String,
    pub scale_handoff: ReturnBackfitStageScaleHandoffDiagnostic,
    pub objective_blocks: Vec<ReturnBackfitStageObjectiveBlockDiagnostic>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeanReversionStageCalibrationGrid {
    pub calibration_grid_kind: String,
    pub calibration_grid_interval_hours: f64,
    pub source_observation_count: usize,
    pub retained_observation_count: usize,
    pub coverage_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeanReversionStageDiagnostics {
    pub selected_candidate_family: String,
    pub calibration_grid: MeanReversionStageCalibrationGrid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_origin_level: Option<f64>,
    pub phi_reference: f64,
    pub kappa: f64,
    pub residual_sum_of_squares: f64,
    pub quasi_log_likelihood: Option<f64>,
    pub information_criterion_bic: Option<f64>,
    pub model_selection_policy: String,
    pub deviation_sample_count: usize,
    pub regression_pair_count: usize,
    pub least_squares_rank: usize,
    pub evaluated_candidates: Vec<MeanReversionStageCandidateDiagnostics>,
    pub evaluated_grids: Vec<MeanReversionStageGridDiagnostics>,
    pub stage_decision_reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeanReversionStageCandidateDiagnostics {
    pub candidate_family: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_origin_level: Option<f64>,
    pub phi_reference: f64,
    pub kappa: f64,
    pub residual_sum_of_squares: f64,
    pub quasi_log_likelihood: Option<f64>,
    pub information_criterion_bic: Option<f64>,
    pub parameter_count: usize,
    pub least_squares_rank: usize,
    pub deviation_sample_count: usize,
    pub regression_pair_count: usize,
    pub series_summaries: MeanReversionStageCandidateSeriesDiagnostics,
    pub stage_decision_reason: String,
}

/// Compact summaries of the internal series used to fit one mean-reversion candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeanReversionStageCandidateSeriesDiagnostics {
    /// Anchor level paired with each retained calibration observation.
    pub boundary_anchor_values: MeanReversionStageSeriesSummary,
    /// Observed log-price deviations from the candidate anchor path.
    pub deviations: MeanReversionStageSeriesSummary,
    /// One-step regression residuals from the fitted candidate.
    pub residuals: MeanReversionStageSeriesSummary,
}

/// Compact report summary for a numeric diagnostic series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeanReversionStageSeriesSummary {
    /// Number of observations in the source series before filtering.
    pub sample_count: usize,
    /// Number of finite observations included in the numeric summary.
    pub finite_count: usize,
    /// First finite observation, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_value: Option<f64>,
    /// Last finite observation, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_value: Option<f64>,
    /// Minimum finite observation, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_value: Option<f64>,
    /// Maximum finite observation, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_value: Option<f64>,
    /// Arithmetic mean of finite observations, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_value: Option<f64>,
    /// Shared value when every source observation is finite and bitwise identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constant_value: Option<f64>,
}

impl MeanReversionStageSeriesSummary {
    /// Build a compact report summary for a diagnostic series.
    pub fn from_values(values: &[f64]) -> Self {
        let mut finite_count = 0usize;
        let mut first_value = None;
        let mut last_value = None;
        let mut minimum_value = None;
        let mut maximum_value = None;
        let mut running_mean = 0.0;
        let mut constant_bits = None;
        let mut all_finite_values_share_constant_bits = true;

        for value in values.iter().copied().filter(|value| value.is_finite()) {
            finite_count += 1;

            first_value.get_or_insert(value);

            last_value = Some(value);

            minimum_value = Some(
                minimum_value
                    .map(|current_minimum: f64| current_minimum.min(value))
                    .unwrap_or(value),
            );

            maximum_value = Some(
                maximum_value
                    .map(|current_maximum: f64| current_maximum.max(value))
                    .unwrap_or(value),
            );

            running_mean += (value - running_mean) / finite_count as f64;

            match constant_bits {
                Some(existing_bits) if existing_bits != value.to_bits() => {
                    all_finite_values_share_constant_bits = false;
                }
                Some(_) => {}
                None => constant_bits = Some(value.to_bits()),
            }
        }

        let mean_value = if finite_count == 0 || !running_mean.is_finite() {
            None
        } else {
            Some(running_mean)
        };

        let constant_value =
            if finite_count == values.len() && all_finite_values_share_constant_bits {
                constant_bits.map(f64::from_bits)
            } else {
                None
            };

        Self {
            sample_count: values.len(),
            finite_count,
            first_value,
            last_value,
            minimum_value,
            maximum_value,
            mean_value,
            constant_value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeanReversionStageGridDiagnostics {
    pub calibration_grid: MeanReversionStageCalibrationGrid,
    pub evaluated_candidates: Vec<MeanReversionStageCandidateDiagnostics>,
    pub stage_decision_reason: String,
}

pub type JointRefinementStageObjectiveBreakdown = CalibrationLossEvaluation;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageLabeledCount {
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageRestartDiagnostics {
    pub restart_count: usize,
    pub valid_restart_count: usize,
    pub invalid_restart_count: usize,
    pub unique_terminal_candidate_count: usize,
    pub terminal_collision_count: usize,
    pub terminal_collision_share: f64,
    pub stop_reason_frequencies: Vec<JointRefinementStageLabeledCount>,
    pub invalid_reason_frequencies: Vec<JointRefinementStageLabeledCount>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageRuntimeProfile {
    pub restart_optimization_wall_clock_millis: u64,
    pub coarse_consistency_wall_clock_millis: u64,
    pub final_seed_evaluation_wall_clock_millis: u64,
    pub final_assembly_wall_clock_millis: u64,
    pub objective_cache_hit_count: usize,
    pub objective_cache_miss_count: usize,
    pub deterministic_cache_hit_count: usize,
    pub deterministic_cache_miss_count: usize,
    pub seed_summary_cache_hit_count: usize,
    pub seed_summary_cache_miss_count: usize,
    pub aggregated_likelihood_context_cache_hit_count: usize,
    pub aggregated_likelihood_context_cache_miss_count: usize,
    pub option_pricing_cache_hit_count: usize,
    pub option_pricing_cache_miss_count: usize,
    pub option_pricing_run_count: usize,
    pub invalid_candidate_evaluation_count: usize,
    pub cumulative_deterministic_evaluation_millis: u64,
    pub cumulative_modeled_rate_millis: u64,
    pub cumulative_likelihood_context_resolution_millis: u64,
    pub cumulative_likelihood_context_1h_sample_millis: u64,
    pub cumulative_likelihood_context_1d_sample_millis: u64,
    pub cumulative_likelihood_context_tick_sample_millis: u64,
    pub cumulative_likelihood_context_tick_optimizer_millis: u64,
    pub cumulative_likelihood_context_1h_optimizer_millis: u64,
    pub cumulative_likelihood_context_1d_optimizer_millis: u64,
    pub cumulative_tick_likelihood_millis: u64,
    pub cumulative_1h_likelihood_millis: u64,
    pub cumulative_1d_likelihood_millis: u64,
    pub cumulative_expected_activity_counts_millis: u64,
    pub cumulative_deterministic_activity_shape_millis: u64,
    pub cumulative_deterministic_activity_level_millis: u64,
    pub cumulative_seed_summary_simulation_millis: u64,
    pub cumulative_raw_seed_metrics_millis: u64,
    pub cumulative_loss_build_millis: u64,
    pub cumulative_option_pricing_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageRestartEvaluationTimingDiagnostic {
    pub restart_id: String,
    pub schedule_category: String,
    pub candidate_evaluation_count: usize,
    pub search_direction_position: Option<usize>,
    pub search_direction_label: String,
    pub elapsed_millis: u64,
    pub objective: Option<f64>,
    pub admissibility_blocker_count: Option<usize>,
    pub max_standardized_residual: Option<f64>,
    pub exact_closed_form_buy_count: Option<usize>,
    pub exact_closed_form_sell_count: Option<usize>,
    pub exact_tilted_mixed_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptimizerDiagnostics {
    pub local_objective: f64,
    pub final_objective: f64,
    pub local_evaluation_count: usize,
    pub final_simulation_seeds: Vec<u64>,
    pub stop_reason: String,
    pub restart_diagnostics: JointRefinementStageRestartDiagnostics,
    pub runtime_profile: JointRefinementStageRuntimeProfile,
    pub restart_evaluation_timing_diagnostics:
        Vec<JointRefinementStageRestartEvaluationTimingDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageFittedParameters {
    pub hawkes_fitted_parameters: HawkesStageFittedParameters,
    pub return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
    #[serde(default)]
    pub pricing_block_fitted_parameters: Option<JointRefinementStagePricingBlockFittedParameters>,
}

impl JointRefinementStageFittedParameters {
    /// Return the Hawkes kernel that option-pricing workflows should use.
    pub fn pricing_hawkes_kernel_parameters(&self) -> HawkesKernelParameters {
        self.hawkes_fitted_parameters.kernel_parameters
    }

    /// Return the NIG configuration that option-pricing workflows should use.
    pub fn pricing_nig_shared_plus_shift_configuration(&self) -> NigSharedPlusShiftConfiguration {
        self.return_components_fitted_parameters
            .nig_shared_plus_shift_configuration
    }

    /// Return the pricing kernel that option-pricing workflows should use.
    pub fn pricing_kernel_parameters(&self) -> VanillaOptionPricingKernelParameters {
        self.pricing_block_fitted_parameters
            .as_ref()
            .map(|pricing_block| pricing_block.pricing_kernel_parameters)
            .unwrap_or_else(VanillaOptionPricingKernelParameters::identity)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStagePricingBlockFittedParameters {
    pub pricing_kernel_parameters: VanillaOptionPricingKernelParameters,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageCarryInput {
    pub discount_rate: f64,
    pub carry_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionSnapshotMetadata {
    pub valuation_time_utc_nanos: i64,
    pub node_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spectral_collocation_node_count: Option<usize>,
    pub pricing_interval_schedule: PricingIntervalSchedule,
    pub maturity_intervals: Vec<f64>,
    pub maturity_terminal_interval_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionQuoteUniverseDiagnostics {
    pub input_quote_count: usize,
    pub canonical_quote_count: usize,
    pub dropped_quote_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionTierMaturityDiagnostic {
    pub terminal_interval_index: usize,
    pub maturity_interval: f64,
    pub expiry_timestamp_utc_nanos: i64,
    pub maturity_weight: usize,
    pub selected_quote_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionTierConstructionDiagnostic {
    pub tier_kind: String,
    pub maturity_count: usize,
    pub representative_maturities: Vec<JointRefinementStageOptionTierMaturityDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionTierConstructionDiagnostics {
    pub search: JointRefinementStageOptionTierConstructionDiagnostic,
    pub selection: JointRefinementStageOptionTierConstructionDiagnostic,
    pub audit: JointRefinementStageOptionTierConstructionDiagnostic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionNodeDiagnostic {
    pub quote_timestamp_utc_nanos: i64,
    pub expiry_timestamp_utc_nanos: i64,
    pub strike: f64,
    pub option_type: String,
    pub bid_price: f64,
    pub ask_price: f64,
    pub observed_mid_price: f64,
    pub model_price: f64,
    pub spread_normalized_residual: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionAggregateDiagnostic {
    pub bucket_kind: String,
    pub bucket_label: String,
    pub node_count: usize,
    pub mean_absolute_spread_normalized_residual: f64,
    pub root_mean_square_spread_normalized_residual: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionMaturityScoreDiagnostic {
    pub expiry_timestamp_utc_nanos: i64,
    pub maturity_weight: usize,
    pub quote_count: usize,
    pub root_mean_square_spread_normalized_residual: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionTierPricingDiagnostics {
    pub tier_kind: String,
    pub snapshot_metadata: JointRefinementStageOptionSnapshotMetadata,
    pub root_mean_square_spread_normalized_residual: f64,
    pub maturity_scores: Vec<JointRefinementStageOptionMaturityScoreDiagnostic>,
    pub node_diagnostics: Vec<JointRefinementStageOptionNodeDiagnostic>,
    pub aggregate_diagnostics: Vec<JointRefinementStageOptionAggregateDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionReducedModelDiagnostics {
    pub local_parameter_labels: Vec<String>,
    pub weighted_residual_jacobian_row_count: usize,
    pub weighted_residual_jacobian_column_count: usize,
    pub weighted_residual_jacobian_singular_values: Vec<f64>,
    pub cumulative_rank_energy: Vec<f64>,
    pub effective_rank_at_90_percent_energy: usize,
    pub effective_rank_at_95_percent_energy: usize,
    pub effective_rank_at_99_percent_energy: usize,
    pub current_search_evaluation_budget: usize,
    pub predicted_local_linear_oracle_call_count: usize,
    pub predicted_local_quadratic_oracle_call_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_oracle_call_wall_clock_millis: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_local_linear_wall_clock_millis: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_local_quadratic_wall_clock_millis: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_local_linear_wall_clock_saved_millis: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_local_quadratic_wall_clock_saved_millis: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionCalibrationDiagnostics {
    pub carry_input: JointRefinementStageCarryInput,
    pub quote_universe: JointRefinementStageOptionQuoteUniverseDiagnostics,
    pub tier_construction: JointRefinementStageOptionTierConstructionDiagnostics,
    pub selection_tier_pricing: JointRefinementStageOptionTierPricingDiagnostics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_tier_pricing: Option<JointRefinementStageOptionTierPricingDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduced_model: Option<JointRefinementStageOptionReducedModelDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_selection_agreement:
        Option<JointRefinementStageOptionSearchSelectionAgreementDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptionSearchSelectionAgreementDiagnostics {
    pub frontier_size: usize,
    pub best_by_search_restart_id: Option<String>,
    pub best_by_selection_restart_id: Option<String>,
    pub selected_restart_id: Option<String>,
    pub selected_search_rank: Option<usize>,
    pub selected_selection_rank: Option<usize>,
    pub selected_rank_movement: Option<i64>,
    pub spearman_rank_correlation: Option<f64>,
}
