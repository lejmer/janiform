mod bounds;
mod budget;
mod closed_form;
mod constants;
mod exact_block;
mod exact_block_cache;
mod likelihood;
mod optimization;
mod profile;
mod proxy_em;
mod sample;
mod scoring;
mod shape_mapping;
#[cfg(test)]
mod tests;
mod tilted_mixed;
mod types;

pub(crate) use bounds::{
    nig_configuration_from_search_point, nig_search_bounds_for_calibration_sample,
    nig_search_point_from_configuration,
};
pub(crate) use likelihood::evaluate_configuration_likelihood_objective_with_context_and_counts;
#[cfg(test)]
pub(crate) use optimization::build_nig_search_candidates;
pub(crate) use optimization::{
    build_nig_moment_seed_configurations, build_nig_search_candidates_with_progress,
    evaluate_continuous_nig_candidate,
};
pub(crate) use profile::distribution_profile_fit_score;
pub(crate) use sample::{
    build_continuous_return_calibration_sample, continuous_return_calibration_sample_count,
    mean_continuous_return_calibration_business_time_scale,
    summarize_observed_continuous_return_calibration_sample,
};
pub(crate) use sample::{
    build_continuous_return_calibration_sample_from_components,
    build_continuous_return_optimizer_context,
    continuous_return_calibration_sample_business_time_scales,
    continuous_return_calibration_sample_observed_returns,
    rescale_continuous_return_calibration_sample_business_time_scales,
};
pub(crate) use types::{
    ContinuousLikelihoodEvaluatorCounts, ContinuousLikelihoodFidelityTier,
    ContinuousReturnOptimizerContext, NigShapeSearchPoint,
    ReturnComponentsStageContinuousReturnCalibrationSample,
};
pub(crate) use types::{ContinuousReturnCandidateEvaluation, OptimizedNigCandidate};
