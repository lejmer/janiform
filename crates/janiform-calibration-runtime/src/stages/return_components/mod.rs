mod activity_coupling_fit;
mod constants;
mod continuous_fit;
mod continuous_layer_fit;
mod contracts;
mod diagnostic_policy_fit;
mod execution;
mod known_arrival_fit;
mod mark_layer_fit;
mod objective_scoring_policy_fit;
mod price_range_policy_fit;
mod quantile_binning_fit;
mod signal_partition_fit;
mod surprise_episode_selection_fit;
mod surprise_intensity_fit;
#[cfg(test)]
mod test_support;

pub(crate) use continuous_fit::{
    ContinuousLikelihoodFidelityTier, NigShapeSearchPoint,
    ReturnComponentsStageContinuousReturnCalibrationSample,
    build_continuous_return_calibration_sample_from_components, distribution_profile_fit_score,
    evaluate_configuration_likelihood_objective, nig_configuration_from_search_point,
    nig_search_point_from_configuration,
};
pub use contracts::*;
pub use execution::*;
