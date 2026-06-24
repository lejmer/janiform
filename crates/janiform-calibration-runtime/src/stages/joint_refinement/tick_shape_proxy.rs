use janiform_calibration_artifacts::JointRefinementStageFittedParameters;

use super::execution::{
    JointRefinementContext, rescaled_observed_tick_calibration_sample_for_modeled_rate,
};
use crate::stages::return_components::{
    sample_continuous_return_calibration_sample_by_equal_stride,
    simulate_matched_scale_returns_with_seed,
};

pub(crate) fn coarse_search_tick_shape_proxy_returns(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
) -> Vec<f64> {
    let Some(modeled_open_event_rate_per_hour) =
        crate::stages::hawkes::stationary_open_event_rate_per_hour(
            &fitted_parameters.hawkes_fitted_parameters,
        )
    else {
        return Vec::new();
    };

    let calibration_sample = rescaled_observed_tick_calibration_sample_for_modeled_rate(
        &context.observed_data,
        context.baseline_modeled_open_event_rate_per_hour,
        modeled_open_event_rate_per_hour,
    );

    let sampled_calibration_sample = sample_continuous_return_calibration_sample_by_equal_stride(
        &calibration_sample,
        context.observed_data.observed_tick_shape_returns.len(),
    );

    let nig_configuration = fitted_parameters.pricing_nig_shared_plus_shift_configuration();

    simulate_matched_scale_returns_with_seed(
        nig_configuration,
        &sampled_calibration_sample,
        simulation_seed,
    )
}
