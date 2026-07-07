use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_calibration_artifacts::{
    HawkesStageFittedParameters, JointRefinementStageFittedParameters,
    JointRefinementStagePricingBlockFittedParameters, ReturnComponentsStageFittedParameters,
};
use janiform_engine_pricing::VanillaOptionPricingKernelParameters;
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameters, HawkesStationaryTargetSourceRates,
    baseline_intensity_for_stationary_total_intensity,
};
use janiform_model_kernels::long_variance::LongVarianceParameters;
use janiform_model_kernels::news::{NewsProcessConfiguration, SurpriseNewsArrivalConfiguration};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;

use crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals;
use crate::stages::long_variance::generated_energy::project_generated_stationary_feedback_to_budget;
use crate::stages::long_variance::policy::{
    long_variance_retained_stationary_level_acceptance_floor,
    long_variance_retained_stationary_level_floor,
};
use crate::stages::return_components::nig_configuration_from_search_point;

use super::super::super::execution::JointRefinementContext;
use super::layout::{
    disable_known_activity_coupling_if_known_news_channel_absent,
    parameter_vector_to_nig_search_point,
};

pub(in crate::stages::joint_refinement) fn parameter_vector_to_fitted_parameters(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Option<JointRefinementStageFittedParameters> {
    checked_parameter_vector_to_fitted_parameters(parameter_vector, context).ok()
}

pub(in crate::stages::joint_refinement) fn checked_parameter_vector_to_fitted_parameters(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Result<JointRefinementStageFittedParameters, JointRefinementParameterDecodeError> {
    let hawkes_fitted_parameters = if context.option_pricing_refinement_enabled {
        context.baseline_hawkes_fitted_parameters.clone()
    } else {
        scaled_hawkes_fitted_parameters_from_parameter_vector(parameter_vector, context)?
    };

    let modeled_open_event_rate_per_hour =
        crate::stages::hawkes::stationary_open_event_rate_per_hour(&hawkes_fitted_parameters)
            .ok_or_else(|| {
                parameter_decode_error(format!(
                    "scaled Hawkes kernel has no finite stationary open event rate: kernel={:?}",
                    hawkes_fitted_parameters.kernel_parameters
                ))
            })?;

    let aggregated_15m_sample = build_continuous_calibration_sample_from_aggregated_intervals(
        context.observed_data.observed_15m_intervals.as_slice(),
        modeled_open_event_rate_per_hour,
        context.observed_data.observed_open_event_rate_per_hour,
        context.observed_data.fallback_sell_share_proxy,
    );

    let nig_search_point =
        parameter_vector_to_nig_search_point(parameter_vector, &context.active_parameter_layout);

    let nig_shared_plus_shift_configuration =
        nig_configuration_from_search_point(&aggregated_15m_sample, nig_search_point).ok_or_else(
            || {
                parameter_decode_error(format!(
                    "NIG coordinates do not decode for the modeled 15m sample: search_point={nig_search_point:?} modeled_open_event_rate_per_hour={modeled_open_event_rate_per_hour} observed_open_event_rate_per_hour={}",
                    context.observed_data.observed_open_event_rate_per_hour
                ))
            },
        )?;

    let return_components_fitted_parameters =
        return_components_fitted_parameters_from_parameter_vector(
            parameter_vector,
            context,
            &hawkes_fitted_parameters,
            nig_shared_plus_shift_configuration,
        )?;

    let pricing_block_fitted_parameters =
        pricing_block_fitted_parameters_from_parameter_vector(parameter_vector, context)?;

    Ok(JointRefinementStageFittedParameters {
        hawkes_fitted_parameters,
        return_components_fitted_parameters,
        pricing_block_fitted_parameters,
    })
}

fn scaled_hawkes_fitted_parameters_from_parameter_vector(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Result<HawkesStageFittedParameters, JointRefinementParameterDecodeError> {
    let mut structured_alpha = context.baseline_hawkes_fitted_parameters.structured_alpha;

    let beta_scale = parameter_vector[context.active_parameter_layout.log_beta_scale_index].exp();

    let self_excitation_scale = parameter_vector[context
        .active_parameter_layout
        .log_self_excitation_scale_index]
        .exp();

    let cross_excitation_scale = parameter_vector[context
        .active_parameter_layout
        .log_cross_excitation_scale_index]
        .exp();

    structured_alpha.a_self *= self_excitation_scale;
    structured_alpha.delta_self *= self_excitation_scale;
    structured_alpha.a_cross *= cross_excitation_scale;
    structured_alpha.delta_cross *= cross_excitation_scale;

    let alpha_loadings = HawkesKernelLoadings::try_from_structured_alpha(structured_alpha)
        .map_err(|source| {
            parameter_decode_error(format!(
                "scaled Hawkes alpha loadings are invalid: {source}"
            ))
        })?;

    let rho_loadings = HawkesKernelLoadings::try_from_structured_rho(
        context.baseline_hawkes_fitted_parameters.structured_rho,
        context
            .baseline_hawkes_fitted_parameters
            .self_dominance_policy,
    )
    .map_err(|source| {
        parameter_decode_error(format!(
            "baseline Hawkes rho loadings are invalid: {source}"
        ))
    })?;

    let beta = context
        .baseline_hawkes_fitted_parameters
        .kernel_parameters
        .beta
        * beta_scale;

    let stationary_open_event_rate_per_hour =
        context.observed_data.observed_open_event_rate_per_hour;

    let lambda_bar = baseline_intensity_for_stationary_total_intensity(
        stationary_open_event_rate_per_hour,
        alpha_loadings,
        beta,
    )
    .ok_or_else(|| {
        parameter_decode_error(format!(
            "scaled Hawkes stationary event rate cannot be converted to a positive baseline intensity: stationary_open_event_rate_per_hour={stationary_open_event_rate_per_hour} beta={beta} alpha={alpha_loadings:?}"
        ))
    })?;

    let kernel_parameters =
        HawkesKernelParameters::try_new(lambda_bar, beta, alpha_loadings, rho_loadings).map_err(
            |source| {
                parameter_decode_error(format!(
                    "scaled Hawkes kernel parameters are invalid: {source}"
                ))
            },
        )?;

    Ok(HawkesStageFittedParameters {
        kernel_parameters,
        structured_alpha,
        structured_rho: context.baseline_hawkes_fitted_parameters.structured_rho,
        self_dominance_policy: context
            .baseline_hawkes_fitted_parameters
            .self_dominance_policy,
        stability_numeric_tolerance: context
            .baseline_hawkes_fitted_parameters
            .stability_numeric_tolerance,
    })
}

fn return_components_fitted_parameters_from_parameter_vector(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
    hawkes_fitted_parameters: &HawkesStageFittedParameters,
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
) -> Result<ReturnComponentsStageFittedParameters, JointRefinementParameterDecodeError> {
    if context.option_pricing_refinement_enabled {
        return Ok(context.baseline_return_components_fitted_parameters.clone());
    }

    let news_process_configuration =
        scaled_news_process_configuration_from_parameter_vector(parameter_vector, context);

    let return_clock_exposure_parameters =
        return_clock_exposure_parameters_from_parameter_vector(parameter_vector, context)?;

    let long_variance_parameters = long_variance_parameters_from_parameter_vector(
        parameter_vector,
        context,
        hawkes_fitted_parameters,
        nig_shared_plus_shift_configuration,
        return_clock_exposure_parameters,
    )?;

    let return_components_fitted_parameters = ReturnComponentsStageFittedParameters {
        nig_shared_plus_shift_configuration,
        news_process_configuration,
        surprise_deterministic_intensity_profile: context
            .baseline_return_components_fitted_parameters
            .surprise_deterministic_intensity_profile
            .clone(),
        return_clock_exposure_parameters,
        long_variance_parameters,
        // Joint-refinement candidates own a pressure contract copied from baseline fit.
        market_pressure_parameters: context
            .baseline_return_components_fitted_parameters
            .market_pressure_parameters
            .clone(),
        price_discretization_surface: context
            .baseline_return_components_fitted_parameters
            .price_discretization_surface
            .clone(),
        timestamp_batching_surface: context
            .baseline_return_components_fitted_parameters
            .timestamp_batching_surface
            .clone(),
        mean_reversion_fitted_parameters: context.baseline_mean_reversion_fitted_parameters,
    };

    return_components_fitted_parameters
        .news_process_configuration
        .validate()
        .map_err(|source| {
            parameter_decode_error(format!(
                "news process configuration is invalid after joint scaling: {source}"
            ))
        })?;

    Ok(return_components_fitted_parameters)
}

fn pricing_block_fitted_parameters_from_parameter_vector(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Result<
    Option<JointRefinementStagePricingBlockFittedParameters>,
    JointRefinementParameterDecodeError,
> {
    if !context.option_pricing_refinement_enabled {
        return Ok(None);
    }

    let pricing_kernel_parameters =
        pricing_kernel_parameters_from_option_vector(parameter_vector, context)?;

    Ok(Some(JointRefinementStagePricingBlockFittedParameters {
        pricing_kernel_parameters,
    }))
}

fn scaled_news_process_configuration_from_parameter_vector(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> NewsProcessConfiguration {
    let mut news_process_configuration = context
        .baseline_return_components_fitted_parameters
        .news_process_configuration
        .clone();

    disable_known_activity_coupling_if_known_news_channel_absent(&mut news_process_configuration);

    if let Some(index) = context.active_parameter_layout.known_activity_scale_index {
        news_process_configuration
            .known_component_mark_scales
            .activity_coupling_scale *= parameter_vector[index].exp();
    }

    if let Some(index) = context
        .active_parameter_layout
        .surprise_activity_scale_index
    {
        news_process_configuration
            .surprise_component_mark_scales
            .activity_coupling_scale *= parameter_vector[index].exp();
    }

    if let Some(index) = context
        .active_parameter_layout
        .surprise_baseline_scale_index
    {
        let surprise_scale = parameter_vector[index].exp();

        news_process_configuration.arrivals.surprise = SurpriseNewsArrivalConfiguration {
            open_baseline_intensity: news_process_configuration
                .arrivals
                .surprise
                .open_baseline_intensity
                * surprise_scale,
            closed_baseline_intensity: news_process_configuration
                .arrivals
                .surprise
                .closed_baseline_intensity
                * surprise_scale,
        };
    }

    news_process_configuration
}

fn return_clock_exposure_parameters_from_parameter_vector(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Result<ReturnClockExposureParameters, JointRefinementParameterDecodeError> {
    let baseline_return_clock_exposure_parameters = context
        .baseline_return_components_fitted_parameters
        .return_clock_exposure_parameters;

    ReturnClockExposureParameters::try_new(
        baseline_return_clock_exposure_parameters.buy_excitation_return_clock_scale
            * parameter_vector[context
                .active_parameter_layout
                .log_buy_excitation_return_clock_scale_index]
                .exp(),
        baseline_return_clock_exposure_parameters.sell_excitation_return_clock_scale
            * parameter_vector[context
                .active_parameter_layout
                .log_sell_excitation_return_clock_scale_index]
                .exp(),
    )
    .map_err(|source| {
        parameter_decode_error(format!(
            "return-clock exposure parameters are invalid: {source}"
        ))
    })
}

fn long_variance_parameters_from_parameter_vector(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
    hawkes_fitted_parameters: &HawkesStageFittedParameters,
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
) -> Result<LongVarianceParameters, JointRefinementParameterDecodeError> {
    let baseline_long_variance_parameters = context
        .baseline_return_components_fitted_parameters
        .long_variance_parameters;

    let decoded_long_variance_parameters = LongVarianceParameters::try_new(
        baseline_long_variance_parameters.mean_reversion_rate
            * parameter_vector[context
                .active_parameter_layout
                .log_long_variance_mean_reversion_rate_scale_index]
                .exp(),
        parameter_vector[context
            .active_parameter_layout
            .long_variance_vol_of_vol_index],
        baseline_long_variance_parameters.stationary_return_energy_rate_per_hour,
        parameter_vector[context
            .active_parameter_layout
            .long_variance_return_energy_loading_index],
        parameter_vector[context
            .active_parameter_layout
            .long_variance_buy_event_loading_index],
        parameter_vector[context
            .active_parameter_layout
            .long_variance_sell_event_loading_index],
    )
    .map_err(|source| {
        parameter_decode_error(format!("long-variance parameters are invalid: {source}"))
    })?;

    let long_variance_parameters = project_generated_stationary_feedback_to_budget(
        decoded_long_variance_parameters,
        hawkes_fitted_parameters.kernel_parameters,
        nig_shared_plus_shift_configuration,
        return_clock_exposure_parameters,
    )
    .map_err(parameter_decode_error)?;

    validate_retained_long_variance_stationary_level(
        &long_variance_parameters,
        hawkes_fitted_parameters,
    )?;

    Ok(long_variance_parameters)
}

fn validate_retained_long_variance_stationary_level(
    long_variance_parameters: &LongVarianceParameters,
    hawkes_fitted_parameters: &HawkesStageFittedParameters,
) -> Result<(), JointRefinementParameterDecodeError> {
    let stationary_long_variance_level = long_variance_parameters
        .stationary_mean_reversion_level_for_hawkes_kernel(
            hawkes_fitted_parameters.kernel_parameters,
            HawkesStationaryTargetSourceRates::zero(),
        )
        .map_err(|source| {
            parameter_decode_error(format!(
                "long-variance stationary level is invalid: {source} parameters={long_variance_parameters:?} hawkes_kernel={:?}",
                hawkes_fitted_parameters.kernel_parameters
            ))
        })?;

    if stationary_long_variance_level < long_variance_retained_stationary_level_acceptance_floor() {
        return Err(parameter_decode_error(format!(
            "long-variance stationary level is below the retained floor: stationary_long_variance_level={stationary_long_variance_level} retained_floor={} acceptance_floor={}",
            long_variance_retained_stationary_level_floor(),
            long_variance_retained_stationary_level_acceptance_floor()
        )));
    }

    Ok(())
}

fn pricing_kernel_parameters_from_option_vector(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Result<VanillaOptionPricingKernelParameters, JointRefinementParameterDecodeError> {
    let layout = &context.active_parameter_layout;

    VanillaOptionPricingKernelParameters::try_new(
        parameter_vector[layout.option_pricing_log_buy_activity_premium_index].exp(),
        parameter_vector[layout.option_pricing_log_sell_activity_premium_index].exp(),
        parameter_vector[layout.option_pricing_pressure_signed_imbalance_exposure_risk_price_index],
    )
    .map_err(|source| {
        parameter_decode_error(format!(
            "option-pricing kernel parameters are invalid: {source}"
        ))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::stages::joint_refinement) struct JointRefinementParameterDecodeError {
    message: String,
}

impl Display for JointRefinementParameterDecodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message.as_str())
    }
}

impl Error for JointRefinementParameterDecodeError {}

fn parameter_decode_error(message: String) -> JointRefinementParameterDecodeError {
    JointRefinementParameterDecodeError { message }
}
