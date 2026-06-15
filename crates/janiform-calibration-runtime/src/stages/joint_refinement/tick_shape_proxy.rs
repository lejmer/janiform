use janiform_calibration_artifacts::JointRefinementStageFittedParameters;
use janiform_engine_simulation::project_core_increment_sequence_to_tick_grid;

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

    let latent_returns = simulate_matched_scale_returns_with_seed(
        nig_configuration,
        &sampled_calibration_sample,
        simulation_seed,
    );

    observed_tick_shape_proxy_returns(
        latent_returns.as_slice(),
        fitted_parameters
            .return_components_fitted_parameters
            .tick_grid_observation_model
            .tick_size,
    )
}

fn observed_tick_shape_proxy_returns(
    latent_returns: &[f64],
    observed_price_tick_size: Option<f64>,
) -> Vec<f64> {
    project_core_increment_sequence_to_tick_grid(latent_returns, observed_price_tick_size)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::observed_tick_shape_proxy_returns;

    #[test]
    fn tick_shape_proxy_applies_residual_preserving_tick_grid_projection() {
        assert_eq!(
            observed_tick_shape_proxy_returns(&[0.004, 0.004], Some(0.01)),
            vec![0.0, 1.01_f64.ln()]
        );
    }
}
