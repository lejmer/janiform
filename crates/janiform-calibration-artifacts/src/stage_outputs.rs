use serde::{Deserialize, Serialize};

use janiform_model_kernels::hawkes::{
    HawkesKernelParameters, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
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
    #[serde(skip_serializing_if = "Option::is_none")]
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
    pub weighted_objective: f64,
    pub hard_constraint_penalty: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageOptimizerDiagnostics {
    pub local_objective: f64,
    pub final_objective: f64,
    pub local_evaluation_count: usize,
    pub final_simulation_seeds: Vec<u64>,
    pub stop_reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRefinementStageFittedParameters {
    pub hawkes_fitted_parameters: HawkesStageFittedParameters,
    pub return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
}
