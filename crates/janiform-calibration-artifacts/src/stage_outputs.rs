use serde::{Deserialize, Serialize};

use crate::contracts::CalibrationLossEvaluation;
use janiform_engine_pricing::PricingIntervalSchedule;
use janiform_model_kernels::hawkes::{
    HawkesKernelParameters, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
};
use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionFittedParameters,
};
use janiform_model_kernels::news::NewsProcessConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeasonalityStageNormalizationMetadata {
    pub normalization_policy: String,
    pub fit_window_event_count: usize,
    pub target_mean_factor: f64,
    pub pre_normalization_mean_factor: f64,
    pub normalization_constant: f64,
    pub normalization_log_offset: f64,
    pub post_normalization_mean_factor: f64,
    pub mean_anchor_tolerance: f64,
}

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
    pub weekly_hour_utc_multipliers: Vec<f64>,
    pub profile_resolution_hours: usize,
    pub fit_policy: String,
    pub normalization_policy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnMarkHourOfWeekScaleProfile {
    pub hour_of_week_factors: Vec<f64>,
    pub profile_resolution_hours: usize,
    pub fit_policy: String,
    pub normalization_policy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickGridObservationModel {
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
    pub return_mark_hour_of_week_scale_profile: ReturnMarkHourOfWeekScaleProfile,
    pub tick_grid_observation_model: TickGridObservationModel,
    pub mean_reversion_fitted_parameters: MeanReversionFittedParameters,
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
    pub anchor_parameter_estimates: MeanReversionAnchorModel,
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
    pub stage_decision_reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeanReversionStageCandidateDiagnostics {
    pub candidate_family: String,
    pub anchor_model: MeanReversionAnchorModel,
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
    pub core_expected_increments: Vec<f64>,
    pub pressure_state_path: Vec<f64>,
    pub boundary_anchor_values: Vec<f64>,
    pub deviations: Vec<f64>,
    pub residuals: Vec<f64>,
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
    pub cumulative_likelihood_context_5m_sample_millis: u64,
    pub cumulative_likelihood_context_15m_sample_millis: u64,
    pub cumulative_likelihood_context_tick_sample_millis: u64,
    pub cumulative_likelihood_context_tick_optimizer_millis: u64,
    pub cumulative_likelihood_context_5m_optimizer_millis: u64,
    pub cumulative_likelihood_context_15m_optimizer_millis: u64,
    pub cumulative_tick_likelihood_millis: u64,
    pub cumulative_5m_likelihood_millis: u64,
    pub cumulative_15m_likelihood_millis: u64,
    pub cumulative_expected_activity_counts_millis: u64,
    pub cumulative_deterministic_activity_shape_millis: u64,
    pub cumulative_deterministic_activity_level_millis: u64,
    pub cumulative_seed_summary_simulation_millis: u64,
    pub cumulative_raw_seed_metrics_millis: u64,
    pub cumulative_loss_build_millis: u64,
    pub cumulative_option_pricing_millis: u64,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageFittedParameters {
    pub hawkes_fitted_parameters: HawkesStageFittedParameters,
    pub return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
    #[serde(default)]
    pub pricing_block_fitted_parameters: Option<JointRefinementStagePricingBlockFittedParameters>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStagePricingBlockFittedParameters {
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
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
}
