use janiform_foundations::numeric::logistic;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::super::derived_contracts::return_components_stage_positive_domain_floor;
use super::super::execution::{
    round_negative_candidate_value, round_positive_candidate_value, round_signed_candidate_value,
};
use super::likelihood::{maximum_open_beta_ratio, minimum_open_beta_ratio};
use super::sample::build_continuous_return_optimizer_context;
use super::types::{
    ContinuousReturnOptimizerContext, NigShapeSearchPoint,
    ReturnComponentsStageContinuousReturnCalibrationSample,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NigSharedPlusShiftConfigurationSearchPointGradient {
    pub(crate) alpha_gradient: [f64; 4],
    pub(crate) beta_0_gradient: [f64; 4],
    pub(crate) delta_gradient: [f64; 4],
    pub(crate) delta_beta_gradient: [f64; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NigSharedPlusShiftConfigurationSearchPointMapping {
    pub(crate) configuration: NigSharedPlusShiftConfiguration,
    pub(crate) gradient: NigSharedPlusShiftConfigurationSearchPointGradient,
}

pub(super) fn configuration_from_shape_search_point(
    optimizer_context: &ContinuousReturnOptimizerContext,
    shape_search_point: NigShapeSearchPoint,
) -> Option<NigSharedPlusShiftConfiguration> {
    let mapping = exact_configuration_and_gradient_from_shape_search_point(
        optimizer_context,
        shape_search_point,
    )?;

    NigSharedPlusShiftConfiguration::try_new(
        round_positive_candidate_value(mapping.configuration.alpha),
        round_signed_candidate_value(mapping.configuration.beta_0),
        round_positive_candidate_value(mapping.configuration.delta),
        0.0,
        round_negative_candidate_value(mapping.configuration.delta_beta),
    )
    .ok()
}

pub(super) fn exact_configuration_and_gradient_from_shape_search_point(
    optimizer_context: &ContinuousReturnOptimizerContext,
    shape_search_point: NigShapeSearchPoint,
) -> Option<NigSharedPlusShiftConfigurationSearchPointMapping> {
    let alpha = shape_search_point.log_alpha.exp();
    if !alpha.is_finite() || alpha <= return_components_stage_positive_domain_floor() {
        return None;
    }

    let buy_beta_ratio = coordinate_to_ratio(shape_search_point.buy_skew_coordinate);
    let sell_relative_share = coordinate_to_share(shape_search_point.sell_relative_coordinate);
    let sell_beta_ratio = sell_beta_ratio_from_relative_share(buy_beta_ratio, sell_relative_share);

    let beta_0 = alpha * buy_beta_ratio;
    let sell_beta = alpha * sell_beta_ratio;
    let delta_beta = sell_beta - beta_0;
    let delta_center = inferred_delta_center(optimizer_context, alpha, beta_0, delta_beta)?;
    let delta_offset_multiplier = shape_search_point.log_delta_offset.exp();

    if !delta_offset_multiplier.is_finite()
        || delta_offset_multiplier <= return_components_stage_positive_domain_floor()
    {
        return None;
    }

    let delta = delta_center * delta_offset_multiplier;

    let configuration =
        NigSharedPlusShiftConfiguration::try_new(alpha, beta_0, delta, 0.0, delta_beta).ok()?;

    let buy_beta_ratio_derivative =
        buy_beta_ratio_coordinate_derivative(shape_search_point.buy_skew_coordinate);
    let sell_relative_share_derivative =
        sell_relative_share_coordinate_derivative(shape_search_point.sell_relative_coordinate);

    let sell_beta_ratio_buy_coordinate_derivative = sell_relative_share * buy_beta_ratio_derivative;
    let sell_beta_ratio_sell_coordinate_derivative =
        (buy_beta_ratio - minimum_open_beta_ratio()) * sell_relative_share_derivative;

    let beta_0_log_alpha_derivative = beta_0;
    let beta_0_buy_coordinate_derivative = alpha * buy_beta_ratio_derivative;

    let delta_beta_log_alpha_derivative = delta_beta;
    let delta_beta_buy_coordinate_derivative =
        alpha * (sell_beta_ratio_buy_coordinate_derivative - buy_beta_ratio_derivative);
    let delta_beta_sell_coordinate_derivative = alpha * sell_beta_ratio_sell_coordinate_derivative;

    let delta_center_shape_derivative = delta_center_shape_search_point_gradient(
        optimizer_context,
        alpha,
        buy_beta_ratio,
        sell_beta_ratio,
        sell_relative_share,
        buy_beta_ratio_derivative,
        sell_relative_share_derivative,
    )?;

    Some(NigSharedPlusShiftConfigurationSearchPointMapping {
        configuration,
        gradient: NigSharedPlusShiftConfigurationSearchPointGradient {
            alpha_gradient: [alpha, 0.0, 0.0, 0.0],
            beta_0_gradient: [
                beta_0_log_alpha_derivative,
                beta_0_buy_coordinate_derivative,
                0.0,
                0.0,
            ],
            delta_gradient: [
                delta_center_shape_derivative[0] * delta_offset_multiplier,
                delta_center_shape_derivative[1] * delta_offset_multiplier,
                delta_center_shape_derivative[2] * delta_offset_multiplier,
                delta,
            ],
            delta_beta_gradient: [
                delta_beta_log_alpha_derivative,
                delta_beta_buy_coordinate_derivative,
                delta_beta_sell_coordinate_derivative,
                0.0,
            ],
        },
    })
}

fn delta_center_shape_search_point_gradient(
    optimizer_context: &ContinuousReturnOptimizerContext,
    alpha: f64,
    buy_beta_ratio: f64,
    sell_beta_ratio: f64,
    sell_relative_share: f64,
    buy_beta_ratio_derivative: f64,
    sell_relative_share_derivative: f64,
) -> Option<[f64; 3]> {
    let buy_weight = 1.0
        - optimizer_context
            .open_session_sell_share_proxy
            .clamp(0.0, 1.0);
    let sell_weight = optimizer_context
        .open_session_sell_share_proxy
        .clamp(0.0, 1.0);

    let target_variance = optimizer_context
        .observed_return_volatility
        .powi(2)
        .max(return_components_stage_positive_domain_floor());

    let mean_business_time_scale = optimizer_context
        .mean_business_time_scale
        .max(return_components_stage_positive_domain_floor());

    let target_delta_center = target_variance / mean_business_time_scale;

    let buy_gamma_ratio_squared = 1.0 - buy_beta_ratio * buy_beta_ratio;
    let sell_gamma_ratio_squared = 1.0 - sell_beta_ratio * sell_beta_ratio;

    if !buy_gamma_ratio_squared.is_finite()
        || !sell_gamma_ratio_squared.is_finite()
        || buy_gamma_ratio_squared <= 0.0
        || sell_gamma_ratio_squared <= 0.0
    {
        return None;
    }

    let buy_inverse_gamma_ratio_cubed = buy_gamma_ratio_squared.powf(-1.5);
    let sell_inverse_gamma_ratio_cubed = sell_gamma_ratio_squared.powf(-1.5);

    let weighted_inverse_gamma_ratio_cubed =
        buy_weight * buy_inverse_gamma_ratio_cubed + sell_weight * sell_inverse_gamma_ratio_cubed;
    if !weighted_inverse_gamma_ratio_cubed.is_finite() || weighted_inverse_gamma_ratio_cubed <= 0.0
    {
        return None;
    }

    let delta_center = target_delta_center * alpha / weighted_inverse_gamma_ratio_cubed;
    if !delta_center.is_finite() || delta_center <= return_components_stage_positive_domain_floor()
    {
        return None;
    }

    let buy_inverse_gamma_ratio_cubed_derivative =
        3.0 * buy_beta_ratio * buy_beta_ratio_derivative * buy_gamma_ratio_squared.powf(-2.5);
    let sell_beta_ratio_buy_coordinate_derivative = sell_relative_share * buy_beta_ratio_derivative;
    let sell_beta_ratio_sell_coordinate_derivative =
        (buy_beta_ratio - minimum_open_beta_ratio()) * sell_relative_share_derivative;
    let sell_inverse_gamma_ratio_cubed_buy_coordinate_derivative = 3.0
        * sell_beta_ratio
        * sell_beta_ratio_buy_coordinate_derivative
        * sell_gamma_ratio_squared.powf(-2.5);
    let sell_inverse_gamma_ratio_cubed_sell_coordinate_derivative = 3.0
        * sell_beta_ratio
        * sell_beta_ratio_sell_coordinate_derivative
        * sell_gamma_ratio_squared.powf(-2.5);

    let weighted_inverse_gamma_ratio_cubed_buy_coordinate_derivative = buy_weight
        * buy_inverse_gamma_ratio_cubed_derivative
        + sell_weight * sell_inverse_gamma_ratio_cubed_buy_coordinate_derivative;
    let weighted_inverse_gamma_ratio_cubed_sell_coordinate_derivative =
        sell_weight * sell_inverse_gamma_ratio_cubed_sell_coordinate_derivative;

    Some([
        delta_center,
        -delta_center * weighted_inverse_gamma_ratio_cubed_buy_coordinate_derivative
            / weighted_inverse_gamma_ratio_cubed,
        -delta_center * weighted_inverse_gamma_ratio_cubed_sell_coordinate_derivative
            / weighted_inverse_gamma_ratio_cubed,
    ])
}

pub(crate) fn exact_configuration_and_gradient_from_shape_search_point_for_calibration_sample(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    shape_search_point: NigShapeSearchPoint,
) -> Option<NigSharedPlusShiftConfigurationSearchPointMapping> {
    let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);

    exact_configuration_and_gradient_from_shape_search_point(&optimizer_context, shape_search_point)
}

pub(super) fn ratio_to_coordinate(ratio: f64) -> f64 {
    let bounded_ratio = clamp_open_beta_ratio(ratio);
    0.5 * ((1.0 + bounded_ratio) / (1.0 - bounded_ratio)).ln()
}

pub(super) fn share_to_coordinate(share: f64) -> f64 {
    let bounded_share = share.clamp(0.0f64.next_up(), 1.0f64.next_down());
    (bounded_share / (1.0 - bounded_share)).ln()
}

pub(super) fn coordinate_to_ratio(coordinate: f64) -> f64 {
    clamp_open_beta_ratio(coordinate.tanh())
}

pub(super) fn coordinate_to_share(coordinate: f64) -> f64 {
    logistic(coordinate).clamp(0.0f64.next_up(), 1.0f64.next_down())
}

pub(super) fn sell_beta_ratio_from_relative_share(buy_beta_ratio: f64, relative_share: f64) -> f64 {
    let lower_bound = minimum_open_beta_ratio();
    lower_bound + (buy_beta_ratio - lower_bound) * relative_share
}

pub(super) fn relative_share_from_beta_ratios(
    buy_beta_ratio: f64,
    sell_beta_ratio: f64,
) -> Option<f64> {
    let lower_bound = minimum_open_beta_ratio();
    let denominator = buy_beta_ratio - lower_bound;
    if !denominator.is_finite() || denominator <= 0.0 {
        return None;
    }

    let share = (sell_beta_ratio - lower_bound) / denominator;
    share
        .is_finite()
        .then_some(share.clamp(0.0f64.next_up(), 1.0f64.next_down()))
}

fn clamp_open_beta_ratio(value: f64) -> f64 {
    value.clamp(minimum_open_beta_ratio(), maximum_open_beta_ratio())
}

pub(super) fn inferred_delta_center(
    optimizer_context: &ContinuousReturnOptimizerContext,
    alpha: f64,
    beta_0: f64,
    delta_beta: f64,
) -> Option<f64> {
    let (_, variance_per_unit, _, _) =
        crate::stages::return_components::execution::theoretical_net_process_moments(
            NigSharedPlusShiftConfiguration {
                alpha,
                beta_0,
                delta: 1.0,
                mu: 0.0,
                delta_beta,
            },
            optimizer_context.open_session_sell_share_proxy,
        )?;

    let target_variance = optimizer_context
        .observed_return_volatility
        .powi(2)
        .max(return_components_stage_positive_domain_floor());

    let mean_business_time_scale = optimizer_context
        .mean_business_time_scale
        .max(return_components_stage_positive_domain_floor());

    let variance_per_unit = variance_per_unit.max(return_components_stage_positive_domain_floor());

    Some(
        (target_variance / (variance_per_unit * mean_business_time_scale))
            .max(return_components_stage_positive_domain_floor()),
    )
}

fn buy_beta_ratio_coordinate_derivative(coordinate: f64) -> f64 {
    let ratio = coordinate_to_ratio(coordinate);
    1.0 - ratio * ratio
}

fn sell_relative_share_coordinate_derivative(coordinate: f64) -> f64 {
    let share = coordinate_to_share(coordinate);
    share * (1.0 - share)
}
