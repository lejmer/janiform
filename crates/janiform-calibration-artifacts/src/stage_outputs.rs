use serde::{Deserialize, Serialize};

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
pub struct ReturnComponentsStageFittedParameters {
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    pub news_process_configuration: NewsProcessConfiguration,
    pub surprise_deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageObjectiveBreakdown {
    pub aggregated_likelihood_fit: f64,
    pub aggregated_return_shape_fit: f64,
    pub return_asymmetry_fit: f64,
    pub tick_return_asymmetry_fit: f64,
    pub aggregated_return_scale_fit: f64,
    pub tick_return_scale_fit: f64,
    pub business_time_return_scale_fit: f64,
    pub intraday_return_level_fit: f64,
    pub activity_shape_fit: f64,
    pub activity_level_fit: f64,
    pub rv_activity_coupling_fit: f64,
    pub price_span_sanity_fit: f64,
    pub option_spread_normalized_price_fit: f64,
    pub weighted_objective: f64,
    pub hard_constraint_penalty: f64,
}

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
    pub invalid_candidate_evaluation_count: usize,
    pub cumulative_deterministic_evaluation_millis: u64,
    pub cumulative_seed_summary_simulation_millis: u64,
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
pub struct JointRefinementStageOptionCalibrationDiagnostics {
    pub snapshot_metadata: JointRefinementStageOptionSnapshotMetadata,
    pub carry_input: JointRefinementStageCarryInput,
    pub node_diagnostics: Vec<JointRefinementStageOptionNodeDiagnostic>,
    pub aggregate_diagnostics: Vec<JointRefinementStageOptionAggregateDiagnostic>,
}
