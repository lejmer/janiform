use janiform_model_kernels::news::NewsProcessConfiguration;

use crate::stages::return_components::{
    NigShapeSearchPoint, ReturnComponentsStageContinuousReturnCalibrationSample,
    nig_search_point_from_configuration,
};

use super::super::super::execution::JointRefinementActiveParameterLayout;
use super::nig_candidate_seeds::apply_nig_search_point;
use janiform_calibration_artifacts::ReturnComponentsStageFittedParameters;
pub(in crate::stages::joint_refinement) fn parameter_vector_to_nig_search_point(
    parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
) -> NigShapeSearchPoint {
    NigShapeSearchPoint {
        log_alpha: parameter_vector[active_parameter_layout.nig_block[0]],
        buy_skew_coordinate: parameter_vector[active_parameter_layout.nig_block[1]],
        sell_relative_coordinate: parameter_vector[active_parameter_layout.nig_block[2]],
        log_delta_offset: parameter_vector[active_parameter_layout.nig_block[3]],
    }
}

pub(in crate::stages::joint_refinement) fn build_baseline_parameter_vector(
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    _hawkes_fitted_parameters: &janiform_calibration_artifacts::HawkesStageFittedParameters,
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> Result<Vec<f64>, String> {
    let mut parameter_vector = vec![0.0; active_parameter_layout.vector_length()];

    parameter_vector[active_parameter_layout.log_stationary_event_rate_scale_index] = 0.0;
    parameter_vector[active_parameter_layout.log_beta_scale_index] = 0.0;
    parameter_vector[active_parameter_layout.log_self_excitation_scale_index] = 0.0;
    parameter_vector[active_parameter_layout.log_cross_excitation_scale_index] = 0.0;

    let search_point = nig_search_point_from_configuration(
        baseline_15m_calibration_sample,
        return_components_fitted_parameters.nig_shared_plus_shift_configuration,
    )
    .ok_or_else(|| {
        "joint refinement baseline NIG is not representable in the joint NIG coordinate system"
            .to_string()
    })?;

    apply_nig_search_point(
        parameter_vector.as_mut_slice(),
        active_parameter_layout,
        search_point,
    );

    if let Some(index) = active_parameter_layout.known_activity_scale_index {
        parameter_vector[index] = 0.0;
    }

    if let Some(index) = active_parameter_layout.surprise_activity_scale_index {
        parameter_vector[index] = 0.0;
    }

    if let Some(index) = active_parameter_layout.surprise_baseline_scale_index {
        parameter_vector[index] = 0.0;
    }

    parameter_vector[active_parameter_layout.log_buy_excitation_return_clock_scale_index] = 0.0;
    parameter_vector[active_parameter_layout.log_sell_excitation_return_clock_scale_index] = 0.0;

    for (index, value) in active_parameter_layout.long_variance_coordinate_values(
        return_components_fitted_parameters.long_variance_parameters,
    ) {
        parameter_vector[index] = value;
    }

    Ok(parameter_vector)
}

pub(in crate::stages::joint_refinement) fn build_active_parameter_layout(
    return_components_fitted_parameters: &ReturnComponentsStageFittedParameters,
) -> JointRefinementActiveParameterLayout {
    let mut next_index = 0usize;

    let log_stationary_event_rate_scale_index = next_index;
    next_index += 1;

    let log_beta_scale_index = next_index;
    next_index += 1;

    let log_self_excitation_scale_index = next_index;
    next_index += 1;

    let log_cross_excitation_scale_index = next_index;
    next_index += 1;

    let hawkes_block = vec![
        log_stationary_event_rate_scale_index,
        log_beta_scale_index,
        log_self_excitation_scale_index,
        log_cross_excitation_scale_index,
    ];

    let nig_block = vec![next_index, next_index + 1, next_index + 2, next_index + 3];
    next_index += 4;

    let known_news_channel_enabled = known_news_channel_is_enabled(
        &return_components_fitted_parameters.news_process_configuration,
    );

    let known_activity_scale_index = (known_news_channel_enabled
        && return_components_fitted_parameters
            .news_process_configuration
            .known_component_mark_scales
            .activity_coupling_scale
            > 0.0)
        .then_some(next_index);
    if known_activity_scale_index.is_some() {
        next_index += 1;
    }

    let surprise_activity_scale_index = (return_components_fitted_parameters
        .news_process_configuration
        .surprise_component_mark_scales
        .activity_coupling_scale
        > 0.0)
        .then_some(next_index);
    if surprise_activity_scale_index.is_some() {
        next_index += 1;
    }

    let combined_surprise_arrival_intensity = return_components_fitted_parameters
        .news_process_configuration
        .arrivals
        .surprise
        .open_baseline_intensity
        + return_components_fitted_parameters
            .news_process_configuration
            .arrivals
            .surprise
            .closed_baseline_intensity;

    let surprise_baseline_scale_index =
        (combined_surprise_arrival_intensity > 0.0).then_some(next_index);
    if surprise_baseline_scale_index.is_some() {
        next_index += 1;
    }

    let mut coupling_block = Vec::new();

    if let Some(index) = known_activity_scale_index {
        coupling_block.push(index);
    }

    if let Some(index) = surprise_activity_scale_index {
        coupling_block.push(index);
    }

    if let Some(index) = surprise_baseline_scale_index {
        coupling_block.push(index);
    }

    let log_buy_excitation_return_clock_scale_index = next_index;
    next_index += 1;

    let log_sell_excitation_return_clock_scale_index = next_index;
    next_index += 1;

    let return_clock_block = vec![
        log_buy_excitation_return_clock_scale_index,
        log_sell_excitation_return_clock_scale_index,
    ];

    let log_long_variance_mean_reversion_rate_scale_index = next_index;
    next_index += 1;

    let long_variance_vol_of_vol_index = next_index;
    next_index += 1;

    let long_variance_return_energy_loading_index = next_index;
    next_index += 1;

    let long_variance_buy_event_loading_index = next_index;
    next_index += 1;

    let long_variance_sell_event_loading_index = next_index;
    next_index += 1;

    let long_variance_block = vec![
        log_long_variance_mean_reversion_rate_scale_index,
        long_variance_vol_of_vol_index,
        long_variance_return_energy_loading_index,
        long_variance_buy_event_loading_index,
        long_variance_sell_event_loading_index,
    ];

    let option_pricing_log_buy_activity_premium_index = next_index;
    next_index += 1;

    let option_pricing_log_sell_activity_premium_index = next_index;
    next_index += 1;

    let option_pricing_pressure_signed_imbalance_exposure_risk_price_index = next_index;

    let option_pricing_kernel_block = vec![
        option_pricing_log_buy_activity_premium_index,
        option_pricing_log_sell_activity_premium_index,
        option_pricing_pressure_signed_imbalance_exposure_risk_price_index,
    ];

    JointRefinementActiveParameterLayout {
        hawkes_block,
        nig_block,
        return_clock_block,
        long_variance_block,
        option_pricing_kernel_block,
        coupling_block,
        log_stationary_event_rate_scale_index,
        log_beta_scale_index,
        log_self_excitation_scale_index,
        log_cross_excitation_scale_index,
        log_buy_excitation_return_clock_scale_index,
        log_sell_excitation_return_clock_scale_index,
        log_long_variance_mean_reversion_rate_scale_index,
        long_variance_vol_of_vol_index,
        long_variance_return_energy_loading_index,
        long_variance_buy_event_loading_index,
        long_variance_sell_event_loading_index,
        option_pricing_log_buy_activity_premium_index,
        option_pricing_log_sell_activity_premium_index,
        option_pricing_pressure_signed_imbalance_exposure_risk_price_index,
        known_activity_scale_index,
        surprise_activity_scale_index,
        surprise_baseline_scale_index,
    }
}

impl JointRefinementActiveParameterLayout {
    pub(crate) fn long_variance_coordinate_values(
        &self,
        parameters: janiform_model_kernels::long_variance::LongVarianceParameters,
    ) -> [(usize, f64); 5] {
        [
            (self.log_long_variance_mean_reversion_rate_scale_index, 0.0),
            (self.long_variance_vol_of_vol_index, parameters.vol_of_vol),
            (
                self.long_variance_return_energy_loading_index,
                parameters.return_energy_loading,
            ),
            (
                self.long_variance_buy_event_loading_index,
                parameters.buy_event_loading,
            ),
            (
                self.long_variance_sell_event_loading_index,
                parameters.sell_event_loading,
            ),
        ]
    }

    pub(in crate::stages::joint_refinement) fn vector_length(&self) -> usize {
        self.coupling_block
            .iter()
            .copied()
            .chain(self.nig_block.iter().copied())
            .chain(self.hawkes_block.iter().copied())
            .chain(self.return_clock_block.iter().copied())
            .chain(self.long_variance_block.iter().copied())
            .chain(self.option_pricing_kernel_block.iter().copied())
            .max()
            .map(|index| index + 1)
            .unwrap_or(0)
    }
}

pub(super) fn disable_known_activity_coupling_if_known_news_channel_absent(
    news_process_configuration: &mut NewsProcessConfiguration,
) {
    if known_news_channel_is_enabled(news_process_configuration) {
        return;
    }

    news_process_configuration
        .known_component_mark_scales
        .activity_coupling_scale = 0.0;
}

fn known_news_channel_is_enabled(news_process_configuration: &NewsProcessConfiguration) -> bool {
    !news_process_configuration
        .arrivals
        .known
        .scheduled_event_timestamps_utc_nanos
        .is_empty()
}
#[cfg(test)]
mod tests {
    use super::{
        build_active_parameter_layout, disable_known_activity_coupling_if_known_news_channel_absent,
    };
    use crate::stages::joint_refinement::execution::test_joint_fitted_parameters;

    #[test]
    fn absent_known_news_channel_freezes_known_activity_coupling() {
        let mut fitted_parameters =
            test_joint_fitted_parameters().return_components_fitted_parameters;

        fitted_parameters
            .news_process_configuration
            .known_component_mark_scales
            .activity_coupling_scale = 0.25;

        let active_parameter_layout = build_active_parameter_layout(&fitted_parameters);

        assert_eq!(active_parameter_layout.known_activity_scale_index, None);

        disable_known_activity_coupling_if_known_news_channel_absent(
            &mut fitted_parameters.news_process_configuration,
        );

        assert_eq!(
            fitted_parameters
                .news_process_configuration
                .known_component_mark_scales
                .activity_coupling_scale,
            0.0
        );
    }
}
