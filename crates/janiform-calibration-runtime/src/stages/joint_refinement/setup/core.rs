use std::error::Error;
use std::fmt::{Display, Formatter};
#[cfg(test)]
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::time::Instant;

use chrono::{DateTime, Utc};
use janiform_engine_pricing::VanillaOptionPricingKernelParameters;
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::numeric::clamp_point_in_place as clamp_in_place;
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameters, HawkesStationaryTargetSourceRates,
    baseline_intensity_for_stationary_total_intensity,
};
use janiform_model_kernels::long_variance::LongVarianceParameters;
use janiform_model_kernels::news::{NewsProcessConfiguration, SurpriseNewsArrivalConfiguration};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;
use janiform_work_scheduler::{StaticWorkPoolInput, run_static_work_pool};

use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals;
use crate::stages::return_components::{
    NigShapeSearchPoint, ReturnComponentsStageCandidateDiagnostic,
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
    ReturnComponentsStageContinuousReturnCalibrationSample, ReturnComponentsStageFittedPayload,
    build_nig_moment_seed_configurations, nig_configuration_from_search_point,
    nig_log_alpha_bounds_for_calibration_sample,
    nig_search_bounds_for_calibration_sample_with_solver_settings,
    nig_search_point_from_configuration,
    rescale_continuous_return_calibration_sample_business_time_scales,
};

use super::super::contracts::{
    JointRefinementStageObjectiveTermDiagnostic, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};
use super::super::execution::{
    JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND,
    JOINT_REFINEMENT_PRESSURE_RISK_PRICE_LOG_DENSITY_BOUND, JointRefinementActiveParameterLayout,
    JointRefinementContext, JointRefinementSeedSummary, JointRefinementSeedSummaryCacheKey,
    build_halton_restart_id, deduplicate_restart_seed_candidates, halton_parameter_vector,
    joint_refinement_has_parallel_worker_capacity, joint_refinement_parallel_worker_count,
    precheck_seed_summary_workload, rescaled_observed_tick_calibration_sample_for_modeled_rate,
};
use super::super::objective::{
    JointRefinementRestartAdmissionDiagnostics, JointRefinementRestartAdmissionScreening,
    evaluate_joint_fitted_candidate_restart_admission_diagnostics,
};
use super::super::worker_runtime::JointRefinementSeedSummaryAcquisitionError;
use super::long_variance_search::{
    apply_long_variance_parameter_bounds, joint_long_variance_restart_seeds,
};
use super::return_clock_restart_seeds::{
    JointReturnClockExposureSeedsBuildInput, ReturnClockExposureRestartSeedProfile,
    joint_return_clock_exposure_restart_seeds, return_clock_exposure_restart_seed_profile,
};
use crate::stages::long_variance::generated_energy::project_generated_stationary_feedback_to_budget;
use crate::stages::long_variance::policy::{
    long_variance_retained_stationary_level_acceptance_floor,
    long_variance_retained_stationary_level_floor,
};
use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageFittedParameters,
    JointRefinementStagePricingBlockFittedParameters, ReturnComponentsStageFittedParameters,
};

const RETURN_SCALE_REALIZED_VARIANCE_1D_METRIC_ID: &str = "return_scale.1d.realized_variance";
const RETURN_SCALE_MEAN_ABSOLUTE_RETURN_1D_METRIC_ID: &str = "return_scale.1d.mean_absolute_return";
const RETURN_SCALE_TICK_REALIZED_VARIANCE_METRIC_ID: &str = "return_scale.tick.realized_variance";
const RETURN_SCALE_TICK_MEAN_ABSOLUTE_RETURN_METRIC_ID: &str =
    "return_scale.tick.mean_absolute_return";
const PROTECTED_RESTART_SEED_IDS: [&str; 2] = ["incumbent_local_search", "activity_aligned"];

const RETURN_SCALE_ALIGNMENT_METRICS: [ReturnScaleAlignmentMetric; 4] = [
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_REALIZED_VARIANCE_1D_METRIC_ID,
        seed_label: "realized_variance_1d",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Quadratic,
    },
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_MEAN_ABSOLUTE_RETURN_1D_METRIC_ID,
        seed_label: "mean_absolute_return_1d",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Linear,
    },
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_TICK_REALIZED_VARIANCE_METRIC_ID,
        seed_label: "tick_realized_variance",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Quadratic,
    },
    ReturnScaleAlignmentMetric {
        metric_id: RETURN_SCALE_TICK_MEAN_ABSOLUTE_RETURN_METRIC_ID,
        seed_label: "tick_mean_absolute_return",
        scale_mapping: ReturnScaleAlignmentScaleMapping::Linear,
    },
];

#[derive(Debug, Clone, Copy)]
struct ReturnScaleAlignmentMetric {
    metric_id: &'static str,
    seed_label: &'static str,
    scale_mapping: ReturnScaleAlignmentScaleMapping,
}

#[derive(Debug, Clone, Copy)]
enum ReturnScaleAlignmentScaleMapping {
    Linear,
    Quadratic,
}

#[derive(Debug, Clone, Copy)]
struct ReturnScaleAlignmentLogDeltaShift {
    seed_label: &'static str,
    log_delta_shift: f64,
}

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
    let option_pricing_only = context.option_pricing_refinement_enabled;

    let mut structured_alpha = context.baseline_hawkes_fitted_parameters.structured_alpha;

    let mut news_process_configuration = context
        .baseline_return_components_fitted_parameters
        .news_process_configuration
        .clone();

    let hawkes_fitted_parameters = if option_pricing_only {
        context.baseline_hawkes_fitted_parameters.clone()
    } else {
        let beta_scale =
            parameter_vector[context.active_parameter_layout.log_beta_scale_index].exp();

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
            HawkesKernelParameters::try_new(lambda_bar, beta, alpha_loadings, rho_loadings)
                .map_err(|source| {
                    parameter_decode_error(format!(
                        "scaled Hawkes kernel parameters are invalid: {source}"
                    ))
                })?;

        janiform_calibration_artifacts::HawkesStageFittedParameters {
            kernel_parameters,
            structured_alpha,
            structured_rho: context.baseline_hawkes_fitted_parameters.structured_rho,
            self_dominance_policy: context
                .baseline_hawkes_fitted_parameters
                .self_dominance_policy,
            stability_numeric_tolerance: context
                .baseline_hawkes_fitted_parameters
                .stability_numeric_tolerance,
        }
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

    let (return_components_fitted_parameters, pricing_block_fitted_parameters) =
        if option_pricing_only {
            let pricing_kernel_parameters =
                pricing_kernel_parameters_from_option_vector(parameter_vector, context)?;

            (
                context.baseline_return_components_fitted_parameters.clone(),
                Some(JointRefinementStagePricingBlockFittedParameters {
                    pricing_kernel_parameters,
                }),
            )
        } else {
            disable_known_activity_coupling_if_known_news_channel_absent(
                &mut news_process_configuration,
            );

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

            let baseline_return_clock_exposure_parameters = context
                .baseline_return_components_fitted_parameters
                .return_clock_exposure_parameters;

            let return_clock_exposure_parameters = ReturnClockExposureParameters::try_new(
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
            })?;

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
            if stationary_long_variance_level
                < long_variance_retained_stationary_level_acceptance_floor()
            {
                return Err(parameter_decode_error(format!(
                    "long-variance stationary level is below the retained floor: stationary_long_variance_level={stationary_long_variance_level} retained_floor={} acceptance_floor={}",
                    long_variance_retained_stationary_level_floor(),
                    long_variance_retained_stationary_level_acceptance_floor()
                )));
            }

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

            (return_components_fitted_parameters, None)
        };

    Ok(JointRefinementStageFittedParameters {
        hawkes_fitted_parameters,
        return_components_fitted_parameters,
        pricing_block_fitted_parameters,
    })
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

    let long_variance_parameters = return_components_fitted_parameters.long_variance_parameters;

    parameter_vector[active_parameter_layout.log_long_variance_mean_reversion_rate_scale_index] =
        0.0;
    parameter_vector[active_parameter_layout.long_variance_vol_of_vol_index] =
        long_variance_parameters.vol_of_vol;
    parameter_vector[active_parameter_layout.long_variance_return_energy_loading_index] =
        long_variance_parameters.return_energy_loading;
    parameter_vector[active_parameter_layout.long_variance_buy_event_loading_index] =
        long_variance_parameters.buy_event_loading;
    parameter_vector[active_parameter_layout.long_variance_sell_event_loading_index] =
        long_variance_parameters.sell_event_loading;

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

fn disable_known_activity_coupling_if_known_news_channel_absent(
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

pub(in crate::stages::joint_refinement) struct ParameterBoundsBuildInput<'a> {
    pub(in crate::stages::joint_refinement) active_parameter_layout:
        &'a JointRefinementActiveParameterLayout,
    pub(in crate::stages::joint_refinement) baseline_parameter_vector: &'a [f64],
    pub(in crate::stages::joint_refinement) baseline_15m_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(in crate::stages::joint_refinement) selected_return_nig_configuration:
        NigSharedPlusShiftConfiguration,
    pub(in crate::stages::joint_refinement) return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    pub(in crate::stages::joint_refinement) baseline_hawkes_kernel_parameters:
        HawkesKernelParameters,
    pub(in crate::stages::joint_refinement) baseline_long_variance_parameters:
        LongVarianceParameters,
    pub(in crate::stages::joint_refinement) observed_data:
        &'a super::super::execution::JointRefinementObservedData,
    pub(in crate::stages::joint_refinement) baseline_modeled_open_event_rate_per_hour: f64,
    pub(in crate::stages::joint_refinement) solver_settings: &'a CalibrationOptimizerSettings,
}

pub(in crate::stages::joint_refinement) fn build_parameter_bounds(
    input: ParameterBoundsBuildInput<'_>,
) -> Result<(Vec<f64>, Vec<f64>), String> {
    let ParameterBoundsBuildInput {
        active_parameter_layout,
        baseline_parameter_vector,
        baseline_15m_calibration_sample,
        selected_return_nig_configuration,
        return_component_basin_diagnostics,
        baseline_hawkes_kernel_parameters,
        baseline_long_variance_parameters,
        observed_data,
        baseline_modeled_open_event_rate_per_hour,
        solver_settings,
    } = input;

    let mut lower_bounds = vec![0.0; baseline_parameter_vector.len()];
    let mut upper_bounds = vec![0.0; baseline_parameter_vector.len()];

    for index in &active_parameter_layout.hawkes_block {
        lower_bounds[*index] = -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
        upper_bounds[*index] = JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
    }
    lower_bounds[active_parameter_layout.log_stationary_event_rate_scale_index] = 0.0;
    upper_bounds[active_parameter_layout.log_stationary_event_rate_scale_index] = 0.0;

    let (nig_lower_bounds, nig_upper_bounds) = joint_nig_search_bounds_from_feasible_sources(
        baseline_15m_calibration_sample,
        selected_return_nig_configuration,
        return_component_basin_diagnostics,
        observed_data,
        baseline_modeled_open_event_rate_per_hour,
        solver_settings,
    )?;

    for (coordinate_position, parameter_index) in
        active_parameter_layout.nig_block.iter().enumerate()
    {
        lower_bounds[*parameter_index] = nig_lower_bounds[coordinate_position];
        upper_bounds[*parameter_index] = nig_upper_bounds[coordinate_position];
    }

    for index in &active_parameter_layout.coupling_block {
        lower_bounds[*index] = -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
        upper_bounds[*index] = JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
    }

    for index in &active_parameter_layout.return_clock_block {
        lower_bounds[*index] = -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
        upper_bounds[*index] = JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
    }

    apply_long_variance_parameter_bounds(
        active_parameter_layout,
        &mut lower_bounds,
        &mut upper_bounds,
        baseline_hawkes_kernel_parameters,
        baseline_long_variance_parameters,
    )?;

    apply_option_pricing_kernel_bounds(
        active_parameter_layout,
        &mut lower_bounds,
        &mut upper_bounds,
    );

    ensure_parameter_vector_inside_bounds(
        baseline_parameter_vector,
        lower_bounds.as_slice(),
        upper_bounds.as_slice(),
    )?;

    Ok((lower_bounds, upper_bounds))
}

pub(in crate::stages::joint_refinement) fn apply_option_pricing_kernel_bounds(
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &mut [f64],
    upper_bounds: &mut [f64],
) {
    lower_bounds[active_parameter_layout.option_pricing_log_buy_activity_premium_index] =
        -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
    upper_bounds[active_parameter_layout.option_pricing_log_buy_activity_premium_index] =
        JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;

    lower_bounds[active_parameter_layout.option_pricing_log_sell_activity_premium_index] =
        -JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;
    upper_bounds[active_parameter_layout.option_pricing_log_sell_activity_premium_index] =
        JOINT_REFINEMENT_MULTIPLICATIVE_SCALE_LOG_BOUND;

    lower_bounds[active_parameter_layout
        .option_pricing_pressure_signed_imbalance_exposure_risk_price_index] =
        -JOINT_REFINEMENT_PRESSURE_RISK_PRICE_LOG_DENSITY_BOUND;
    upper_bounds[active_parameter_layout
        .option_pricing_pressure_signed_imbalance_exposure_risk_price_index] =
        JOINT_REFINEMENT_PRESSURE_RISK_PRICE_LOG_DENSITY_BOUND;
}

fn joint_nig_search_bounds_from_feasible_sources(
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    selected_return_nig_configuration: NigSharedPlusShiftConfiguration,
    return_component_basin_diagnostics: &[ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    observed_data: &super::super::execution::JointRefinementObservedData,
    baseline_modeled_open_event_rate_per_hour: f64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<([f64; 4], [f64; 4]), String> {
    let (mut lower_bounds, mut upper_bounds) =
        nig_search_bounds_for_calibration_sample_with_solver_settings(
            baseline_15m_calibration_sample,
            solver_settings,
        );

    let selected_search_point = nig_search_point_from_configuration(
        baseline_15m_calibration_sample,
        selected_return_nig_configuration,
    )
    .ok_or_else(|| {
        "selected return-stage NIG cannot be projected into joint NIG coordinates".to_string()
    })?;

    widen_nig_bounds_to_include_search_point(
        &mut lower_bounds,
        &mut upper_bounds,
        selected_search_point,
    );

    for basin_search_point in return_component_basin_diagnostics
        .iter()
        .filter_map(|diagnostic| {
            project_basin_diagnostic_into_joint_nig_coordinates(
                baseline_15m_calibration_sample,
                diagnostic,
            )
        })
    {
        widen_nig_bounds_to_include_search_point(
            &mut lower_bounds,
            &mut upper_bounds,
            basin_search_point,
        );
    }

    let activity_aligned_15m_calibration_sample =
        build_continuous_calibration_sample_from_aggregated_intervals(
            observed_data.observed_15m_intervals.as_slice(),
            observed_data.observed_open_event_rate_per_hour,
            observed_data.observed_open_event_rate_per_hour,
            observed_data.fallback_sell_share_proxy,
        );

    for tick_moment_search_point in
        projected_tick_moment_nig_search_points(ProjectedTickMomentNigSearchPointsInput {
            calibration_sample: &activity_aligned_15m_calibration_sample,
            observed_data,
            baseline_modeled_open_event_rate_per_hour,
            target_modeled_open_event_rate_per_hour: observed_data
                .observed_open_event_rate_per_hour,
            solver_settings,
        })
    {
        widen_nig_bounds_to_include_search_point(
            &mut lower_bounds,
            &mut upper_bounds,
            tick_moment_search_point,
        );
    }

    widen_nig_tail_bounds_for_tick_shape(
        &mut lower_bounds,
        &mut upper_bounds,
        observed_data,
        baseline_modeled_open_event_rate_per_hour,
    );

    Ok((lower_bounds, upper_bounds))
}

fn widen_nig_tail_bounds_for_tick_shape(
    lower_bounds: &mut [f64; 4],
    upper_bounds: &mut [f64; 4],
    observed_data: &super::super::execution::JointRefinementObservedData,
    baseline_modeled_open_event_rate_per_hour: f64,
) {
    if !baseline_modeled_open_event_rate_per_hour.is_finite()
        || baseline_modeled_open_event_rate_per_hour <= 0.0
        || !observed_data.observed_open_event_rate_per_hour.is_finite()
        || observed_data.observed_open_event_rate_per_hour <= 0.0
    {
        return;
    }

    let tick_calibration_sample = rescaled_observed_tick_calibration_sample_for_modeled_rate(
        observed_data,
        baseline_modeled_open_event_rate_per_hour,
        observed_data.observed_open_event_rate_per_hour,
    );

    let (tick_log_alpha_lower_bound, tick_log_alpha_upper_bound) =
        nig_log_alpha_bounds_for_calibration_sample(&tick_calibration_sample);

    lower_bounds[0] = lower_bounds[0].min(tick_log_alpha_lower_bound);
    upper_bounds[0] = upper_bounds[0].max(tick_log_alpha_upper_bound);
}

fn widen_nig_bounds_to_include_search_point(
    lower_bounds: &mut [f64; 4],
    upper_bounds: &mut [f64; 4],
    search_point: NigShapeSearchPoint,
) {
    let coordinates = nig_search_point_coordinates(search_point);

    for coordinate_index in 0..coordinates.len() {
        lower_bounds[coordinate_index] =
            lower_bounds[coordinate_index].min(coordinates[coordinate_index]);
        upper_bounds[coordinate_index] =
            upper_bounds[coordinate_index].max(coordinates[coordinate_index]);
    }
}

fn nig_search_point_coordinates(search_point: NigShapeSearchPoint) -> [f64; 4] {
    [
        search_point.log_alpha,
        search_point.buy_skew_coordinate,
        search_point.sell_relative_coordinate,
        search_point.log_delta_offset,
    ]
}

fn ensure_parameter_vector_inside_bounds(
    parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Result<(), String> {
    if parameter_vector.len() != lower_bounds.len() || parameter_vector.len() != upper_bounds.len()
    {
        return Err(format!(
            "joint refinement parameter bound length mismatch vector={} lower={} upper={}",
            parameter_vector.len(),
            lower_bounds.len(),
            upper_bounds.len()
        ));
    }

    for (parameter_index, (value, (lower_bound, upper_bound))) in parameter_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .enumerate()
    {
        if value < lower_bound || value > upper_bound {
            return Err(format!(
                "joint refinement baseline parameter is outside bounds parameter_index={parameter_index} value={value} lower_bound={lower_bound} upper_bound={upper_bound}"
            ));
        }
    }

    Ok(())
}

pub(in crate::stages::joint_refinement) fn build_restart_seeds(
    input: RestartSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let RestartSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
        joint_context,
        solver_settings,
        restart_candidate_budget,
        progress_sender,
    } = input;

    emit_restart_seed_generation_progress(
        progress_sender,
        0,
        2,
        Some("core_restart_seed_generation"),
    );

    let return_clock_seed_profile = return_clock_exposure_restart_seed_profile(joint_context);

    let mut restart_candidates =
        build_core_restart_seed_candidates(CoreRestartSeedCandidatesInput {
            baseline_parameter_vector,
            active_parameter_layout,
            lower_bounds,
            upper_bounds,
            baseline_15m_calibration_sample,
            return_component_basin_diagnostics,
            baseline_final_objective_terms,
            activity_log_shift,
            return_clock_seed_profile,
        });

    emit_restart_seed_generation_progress(
        progress_sender,
        1,
        2,
        Some("tick_moment_seed_generation"),
    );

    restart_candidates.extend(joint_tick_moment_restart_seeds(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        activity_log_shift,
        joint_context,
        solver_settings,
    ));

    emit_restart_seed_generation_progress(
        progress_sender,
        2,
        2,
        Some("restart_seed_candidates_built"),
    );

    let restart_seed_score = |_: &str, restart_seed: &[f64]| {
        screened_exploratory_restart_seed_score(restart_seed, joint_context)
    };

    finalize_restart_seed_candidates(RestartSeedCandidateFinalizationInput {
        restart_candidates,
        baseline_parameter_vector,
        lower_bounds,
        upper_bounds,
        restart_candidate_budget,
        restart_seed_score: &restart_seed_score,
        protected_restart_ids: &PROTECTED_RESTART_SEED_IDS,
        joint_context: Some(joint_context),
        progress_sender,
    })
}

pub(in crate::stages::joint_refinement) struct RestartSeedsBuildInput<'a> {
    pub(in crate::stages::joint_refinement) baseline_parameter_vector: &'a [f64],
    pub(in crate::stages::joint_refinement) active_parameter_layout:
        &'a JointRefinementActiveParameterLayout,
    pub(in crate::stages::joint_refinement) lower_bounds: &'a [f64],
    pub(in crate::stages::joint_refinement) upper_bounds: &'a [f64],
    pub(in crate::stages::joint_refinement) baseline_15m_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(in crate::stages::joint_refinement) return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    pub(in crate::stages::joint_refinement) baseline_final_objective_terms:
        &'a [JointRefinementStageObjectiveTermDiagnostic],
    pub(in crate::stages::joint_refinement) activity_log_shift: f64,
    pub(in crate::stages::joint_refinement) joint_context: &'a JointRefinementContext,
    pub(in crate::stages::joint_refinement) solver_settings: &'a CalibrationOptimizerSettings,
    pub(in crate::stages::joint_refinement) restart_candidate_budget: usize,
    pub(in crate::stages::joint_refinement) progress_sender:
        Option<&'a Sender<JointRefinementStageProgressUpdate>>,
}

#[cfg(test)]
pub(in crate::stages::joint_refinement) fn build_restart_seeds_with_halton_acceptance(
    input: RestartSeedsWithHaltonAcceptanceBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let RestartSeedsWithHaltonAcceptanceBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
        restart_candidate_budget,
        halton_seed_is_accepted,
    } = input;

    let halton_seed_is_accepted = Mutex::new(halton_seed_is_accepted);

    let restart_candidates = build_core_restart_seed_candidates(CoreRestartSeedCandidatesInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
        return_clock_seed_profile: None,
    });

    let restart_seed_score = |restart_id: &str, restart_seed: &[f64]| {
        if !restart_id.starts_with("halton_") {
            return restart_admission_screening_from_scalar(0.0);
        }

        let Ok(mut halton_seed_is_accepted) = halton_seed_is_accepted.lock() else {
            return None;
        };

        halton_seed_is_accepted(restart_seed).then(|| {
            restart_admission_screening_from_scalar(0.0)
                .expect("finite scalar screening score should build")
        })
    };

    finalize_restart_seed_candidates(RestartSeedCandidateFinalizationInput {
        restart_candidates,
        baseline_parameter_vector,
        lower_bounds,
        upper_bounds,
        restart_candidate_budget,
        restart_seed_score: &restart_seed_score,
        protected_restart_ids: &PROTECTED_RESTART_SEED_IDS,
        joint_context: None,
        progress_sender: None,
    })
}

#[cfg(test)]
type HaltonSeedAcceptancePredicate<'a> = Box<dyn FnMut(&[f64]) -> bool + Send + 'a>;

#[cfg(test)]
pub(in crate::stages::joint_refinement) struct RestartSeedsWithHaltonAcceptanceBuildInput<'a> {
    pub(in crate::stages::joint_refinement) baseline_parameter_vector: &'a [f64],
    pub(in crate::stages::joint_refinement) active_parameter_layout:
        &'a JointRefinementActiveParameterLayout,
    pub(in crate::stages::joint_refinement) lower_bounds: &'a [f64],
    pub(in crate::stages::joint_refinement) upper_bounds: &'a [f64],
    pub(in crate::stages::joint_refinement) baseline_15m_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(in crate::stages::joint_refinement) return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    pub(in crate::stages::joint_refinement) baseline_final_objective_terms:
        &'a [JointRefinementStageObjectiveTermDiagnostic],
    pub(in crate::stages::joint_refinement) activity_log_shift: f64,
    pub(in crate::stages::joint_refinement) restart_candidate_budget: usize,
    pub(in crate::stages::joint_refinement) halton_seed_is_accepted:
        HaltonSeedAcceptancePredicate<'a>,
}

struct CoreRestartSeedCandidatesInput<'a> {
    baseline_parameter_vector: &'a [f64],
    active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    baseline_15m_calibration_sample: &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    baseline_final_objective_terms: &'a [JointRefinementStageObjectiveTermDiagnostic],
    activity_log_shift: f64,
    return_clock_seed_profile: Option<ReturnClockExposureRestartSeedProfile>,
}

fn build_core_restart_seed_candidates(
    input: CoreRestartSeedCandidatesInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let CoreRestartSeedCandidatesInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        return_component_basin_diagnostics,
        baseline_final_objective_terms,
        activity_log_shift,
        return_clock_seed_profile,
    } = input;

    let mut incumbent_local_search = baseline_parameter_vector.to_vec();

    clamp_in_place(
        incumbent_local_search.as_mut_slice(),
        lower_bounds,
        upper_bounds,
    );

    let mut activity_aligned = baseline_parameter_vector.to_vec();

    apply_activity_alignment(
        activity_aligned.as_mut_slice(),
        active_parameter_layout,
        activity_log_shift,
    );

    clamp_in_place(activity_aligned.as_mut_slice(), lower_bounds, upper_bounds);

    let mut midpoint = lower_bounds
        .iter()
        .zip(upper_bounds.iter())
        .map(|(lower_bound, upper_bound)| 0.5 * (lower_bound + upper_bound))
        .collect::<Vec<_>>();

    clamp_in_place(midpoint.as_mut_slice(), lower_bounds, upper_bounds);

    let mut restart_candidates =
        vec![("incumbent_local_search".to_string(), incumbent_local_search)];

    restart_candidates.push(("activity_aligned".to_string(), activity_aligned));

    restart_candidates.extend(joint_return_scale_alignment_restart_seeds(
        JointReturnScaleAlignmentSeedsBuildInput {
            baseline_parameter_vector,
            active_parameter_layout,
            lower_bounds,
            upper_bounds,
            baseline_final_objective_terms,
            activity_log_shift,
        },
    ));

    if let Some(return_clock_seed_profile) = return_clock_seed_profile {
        restart_candidates.extend(joint_return_clock_exposure_restart_seeds(
            JointReturnClockExposureSeedsBuildInput {
                baseline_parameter_vector,
                active_parameter_layout,
                lower_bounds,
                upper_bounds,
                return_clock_seed_profile,
            },
        ));
    }

    restart_candidates.push(("midpoint".to_string(), midpoint));

    let activity_aligned_15m_calibration_sample =
        activity_aligned_calibration_sample(baseline_15m_calibration_sample, activity_log_shift);

    restart_candidates.extend(joint_nig_basin_restart_seeds(
        JointNigRestartSeedsBuildInput {
            baseline_parameter_vector,
            active_parameter_layout,
            lower_bounds,
            upper_bounds,
            baseline_15m_calibration_sample,
            activity_aligned_15m_calibration_sample: &activity_aligned_15m_calibration_sample,
            return_component_basin_diagnostics,
            activity_log_shift,
            maximum_seed_count: 4,
        },
    ));

    restart_candidates.extend(joint_nig_alpha_delta_restart_seeds(
        JointNigRestartSeedsBuildInput {
            baseline_parameter_vector,
            active_parameter_layout,
            lower_bounds,
            upper_bounds,
            baseline_15m_calibration_sample,
            activity_aligned_15m_calibration_sample: &activity_aligned_15m_calibration_sample,
            return_component_basin_diagnostics,
            activity_log_shift,
            maximum_seed_count: 2,
        },
    ));

    restart_candidates.extend(joint_long_variance_restart_seeds(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
    ));

    restart_candidates
}

struct JointReturnScaleAlignmentSeedsBuildInput<'a> {
    baseline_parameter_vector: &'a [f64],
    active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    baseline_final_objective_terms: &'a [JointRefinementStageObjectiveTermDiagnostic],
    activity_log_shift: f64,
}

fn joint_return_scale_alignment_restart_seeds(
    input: JointReturnScaleAlignmentSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let JointReturnScaleAlignmentSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_final_objective_terms,
        activity_log_shift,
    } = input;

    return_scale_alignment_log_delta_shifts(baseline_final_objective_terms)
        .into_iter()
        .flat_map(|alignment| {
            let mut baseline_scale_aligned = baseline_parameter_vector.to_vec();
            apply_nig_log_delta_offset_shift(
                baseline_scale_aligned.as_mut_slice(),
                active_parameter_layout,
                alignment.log_delta_shift,
            );
            clamp_in_place(
                baseline_scale_aligned.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            let mut activity_scale_aligned = baseline_parameter_vector.to_vec();

            apply_nig_log_delta_offset_shift(
                activity_scale_aligned.as_mut_slice(),
                active_parameter_layout,
                alignment.log_delta_shift,
            );

            apply_activity_alignment(
                activity_scale_aligned.as_mut_slice(),
                active_parameter_layout,
                activity_log_shift,
            );

            clamp_in_place(
                activity_scale_aligned.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            vec![
                (
                    format!("return_scale_{}", alignment.seed_label),
                    baseline_scale_aligned,
                ),
                (
                    format!("return_scale_{}_activity_aligned", alignment.seed_label),
                    activity_scale_aligned,
                ),
            ]
        })
        .collect()
}

fn return_scale_alignment_log_delta_shifts(
    objective_terms: &[JointRefinementStageObjectiveTermDiagnostic],
) -> Vec<ReturnScaleAlignmentLogDeltaShift> {
    RETURN_SCALE_ALIGNMENT_METRICS
        .iter()
        .filter_map(|alignment_metric| {
            objective_terms
                .iter()
                .find(|term| term.term_id == alignment_metric.metric_id)
                .and_then(|term| return_scale_alignment_ratio(term, alignment_metric.scale_mapping))
                .map(|scale_ratio| ReturnScaleAlignmentLogDeltaShift {
                    seed_label: alignment_metric.seed_label,
                    log_delta_shift: scale_ratio.ln(),
                })
        })
        .collect()
}

fn return_scale_alignment_ratio(
    objective_term: &JointRefinementStageObjectiveTermDiagnostic,
    scale_mapping: ReturnScaleAlignmentScaleMapping,
) -> Option<f64> {
    let observed_value = objective_term.observed_value?;
    let model_value = objective_term.model_value?;

    if !observed_value.is_finite()
        || !model_value.is_finite()
        || observed_value <= 0.0
        || model_value <= 0.0
    {
        return None;
    }

    let observed_to_simulated_ratio = observed_value / model_value;
    if !observed_to_simulated_ratio.is_finite() || observed_to_simulated_ratio <= 0.0 {
        return None;
    }

    let scale_ratio = match scale_mapping {
        ReturnScaleAlignmentScaleMapping::Linear => observed_to_simulated_ratio,
        ReturnScaleAlignmentScaleMapping::Quadratic => observed_to_simulated_ratio.sqrt(),
    };

    scale_ratio.is_finite().then_some(scale_ratio)
}

fn finalize_restart_seed_candidates(
    input: RestartSeedCandidateFinalizationInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let RestartSeedCandidateFinalizationInput {
        restart_candidates,
        baseline_parameter_vector,
        lower_bounds,
        upper_bounds,
        restart_candidate_budget,
        restart_seed_score,
        protected_restart_ids,
        joint_context,
        progress_sender,
    } = input;

    let target_restart_count = restart_candidate_budget.max(1);

    let deduplicated_candidates = deduplicate_restart_seed_candidates(restart_candidates);

    let total_screening_units = deduplicated_candidates
        .len()
        .saturating_add(target_restart_count);

    emit_restart_candidate_screening_progress(
        progress_sender,
        0,
        total_screening_units,
        Some("candidate_frontier"),
    );

    let completed_screening_units = Mutex::new(0usize);

    let screened_restart_candidates = screen_restart_candidates(
        deduplicated_candidates.as_slice(),
        &restart_seed_score,
        joint_context,
        progress_sender,
        &completed_screening_units,
        total_screening_units,
    );

    let mut screened_halton_candidates = Vec::new();

    if screened_restart_candidates.len() < target_restart_count {
        screened_halton_candidates =
            screen_halton_restart_candidates(HaltonRestartCandidateScreeningInput {
                baseline_parameter_vector,
                lower_bounds,
                upper_bounds,
                target_restart_count,
                source_order_offset: deduplicated_candidates.len(),
                halton_seed_score: &restart_seed_score,
                joint_context,
                progress_sender,
                completed_screening_units: &completed_screening_units,
                total_screening_units,
            });
    }

    emit_restart_candidate_screening_progress(
        progress_sender,
        total_screening_units,
        total_screening_units,
        Some("restart_candidates_screened"),
    );

    let selected_restart_candidates = if screened_restart_candidates.len() < target_restart_count {
        let remaining_restart_count = target_restart_count - screened_restart_candidates.len();
        let mut selected_restart_candidates =
            screened_restart_candidates_to_seed_pairs(screened_restart_candidates);
        selected_restart_candidates.extend(select_screened_restart_candidates(
            screened_halton_candidates,
            remaining_restart_count,
            lower_bounds,
            upper_bounds,
            &[],
        ));
        selected_restart_candidates
    } else {
        select_screened_restart_candidates(
            screened_restart_candidates,
            target_restart_count,
            lower_bounds,
            upper_bounds,
            protected_restart_ids,
        )
    };

    deduplicate_restart_seed_candidates(selected_restart_candidates)
        .into_iter()
        .take(target_restart_count)
        .collect()
}

fn screened_restart_candidates_to_seed_pairs(
    screened_candidates: Vec<ScreenedRestartCandidate>,
) -> Vec<(String, Vec<f64>)> {
    screened_candidates
        .into_iter()
        .map(|candidate| (candidate.restart_id, candidate.parameter_vector))
        .collect()
}

struct RestartSeedCandidateFinalizationInput<'a> {
    restart_candidates: Vec<(String, Vec<f64>)>,
    baseline_parameter_vector: &'a [f64],
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    restart_candidate_budget: usize,
    restart_seed_score:
        &'a (dyn Fn(&str, &[f64]) -> Option<JointRefinementRestartAdmissionScreening> + Sync),
    protected_restart_ids: &'a [&'a str],
    joint_context: Option<&'a JointRefinementContext>,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
}

struct ScreenedRestartCandidateOutput {
    candidate_position: usize,
    candidate: Option<ScreenedRestartCandidate>,
}

#[derive(Debug)]
struct ScreenedRestartCandidate {
    restart_id: String,
    parameter_vector: Vec<f64>,
    score: JointRefinementRestartAdmissionScreening,
    mechanism_family: RestartSeedMechanismFamily,
    source_order: usize,
}

impl ScreenedRestartCandidate {
    fn try_new(
        restart_id: &str,
        parameter_vector: &[f64],
        score: Option<JointRefinementRestartAdmissionScreening>,
        source_order: usize,
    ) -> Option<Self> {
        score.map(|score| Self {
            restart_id: restart_id.to_string(),
            parameter_vector: parameter_vector.to_vec(),
            score,
            mechanism_family: restart_seed_mechanism_family(restart_id),
            source_order,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartSeedMechanismFamily {
    Incumbent,
    ActivityAlignment,
    ReturnScaleAlignment,
    ReturnClockExposure,
    NigBasin,
    NigAlphaDelta,
    LongVariance,
    TickShapeAlpha,
    TickMoment,
    Midpoint,
    Halton,
    Other,
}

fn restart_seed_mechanism_family(restart_id: &str) -> RestartSeedMechanismFamily {
    if restart_id == "incumbent_local_search" {
        return RestartSeedMechanismFamily::Incumbent;
    }

    if restart_id == "activity_aligned" {
        return RestartSeedMechanismFamily::ActivityAlignment;
    }

    if restart_id == "midpoint" {
        return RestartSeedMechanismFamily::Midpoint;
    }

    if restart_id.starts_with("return_scale") {
        return RestartSeedMechanismFamily::ReturnScaleAlignment;
    }

    if restart_id.starts_with("return_clock") {
        return RestartSeedMechanismFamily::ReturnClockExposure;
    }

    if restart_id.starts_with("nig_basin") {
        return RestartSeedMechanismFamily::NigBasin;
    }

    if restart_id.starts_with("nig_alpha_delta") {
        return RestartSeedMechanismFamily::NigAlphaDelta;
    }

    if restart_id.starts_with("long_variance") {
        return RestartSeedMechanismFamily::LongVariance;
    }

    if restart_id.starts_with("tick_shape_alpha") {
        return RestartSeedMechanismFamily::TickShapeAlpha;
    }

    if restart_id.starts_with("tick_moment") {
        return RestartSeedMechanismFamily::TickMoment;
    }

    if restart_id.starts_with("halton") {
        return RestartSeedMechanismFamily::Halton;
    }

    RestartSeedMechanismFamily::Other
}

#[cfg(test)]
fn restart_admission_screening_from_scalar(
    aggregate_score: f64,
) -> Option<JointRefinementRestartAdmissionScreening> {
    aggregate_score
        .is_finite()
        .then_some(JointRefinementRestartAdmissionScreening {
            aggregate_score,
            component_ids: vec!["scalar".to_string()],
            component_scores: vec![aggregate_score],
        })
}

fn select_screened_restart_candidates(
    screened_candidates: Vec<ScreenedRestartCandidate>,
    target_restart_count: usize,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    protected_restart_ids: &[&str],
) -> Vec<(String, Vec<f64>)> {
    if target_restart_count == 0 || screened_candidates.is_empty() {
        return Vec::new();
    }

    let mut remaining_candidate_indices = (0..screened_candidates.len()).collect::<Vec<_>>();
    let mut selected_candidate_indices = select_protected_screened_restart_candidate_indices(
        screened_candidates.as_slice(),
        protected_restart_ids,
        target_restart_count,
    );

    remaining_candidate_indices
        .retain(|candidate_index| !selected_candidate_indices.contains(candidate_index));

    while selected_candidate_indices.len() < target_restart_count
        && !remaining_candidate_indices.is_empty()
    {
        let remaining_slots = target_restart_count - selected_candidate_indices.len();

        let selected_candidate_index = select_next_screened_restart_candidate(
            screened_candidates.as_slice(),
            remaining_candidate_indices.as_slice(),
            selected_candidate_indices.as_slice(),
            remaining_slots,
            lower_bounds,
            upper_bounds,
        );

        selected_candidate_indices.push(selected_candidate_index);

        remaining_candidate_indices
            .retain(|candidate_index| *candidate_index != selected_candidate_index);
    }

    selected_candidate_indices
        .into_iter()
        .map(|candidate_index| {
            let candidate = &screened_candidates[candidate_index];
            (
                candidate.restart_id.clone(),
                candidate.parameter_vector.clone(),
            )
        })
        .collect()
}

fn select_protected_screened_restart_candidate_indices(
    screened_candidates: &[ScreenedRestartCandidate],
    protected_restart_ids: &[&str],
    target_restart_count: usize,
) -> Vec<usize> {
    if target_restart_count < protected_restart_ids.len() {
        return Vec::new();
    }

    let mut selected_candidate_indices = Vec::new();

    for protected_restart_id in protected_restart_ids {
        if selected_candidate_indices.len() >= target_restart_count {
            break;
        }

        let Some(candidate_index) = screened_candidates
            .iter()
            .position(|candidate| candidate.restart_id == *protected_restart_id)
        else {
            continue;
        };

        if !selected_candidate_indices.contains(&candidate_index) {
            selected_candidate_indices.push(candidate_index);
        }
    }

    selected_candidate_indices
}

fn select_next_screened_restart_candidate(
    screened_candidates: &[ScreenedRestartCandidate],
    remaining_candidate_indices: &[usize],
    selected_candidate_indices: &[usize],
    remaining_slots: usize,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> usize {
    if selected_candidate_indices.is_empty() {
        return lowest_score_screened_restart_candidate(
            screened_candidates,
            remaining_candidate_indices,
        );
    }

    if let Some(candidate_index) = lowest_score_unrepresented_mechanism_candidate(
        screened_candidates,
        remaining_candidate_indices,
        selected_candidate_indices,
    ) {
        return candidate_index;
    }

    let frontier_candidate_indices = global_loss_frontier_candidate_indices(
        screened_candidates,
        remaining_candidate_indices,
        remaining_slots + selected_candidate_indices.len(),
    );

    most_distant_screened_restart_candidate(
        screened_candidates,
        frontier_candidate_indices.as_slice(),
        selected_candidate_indices,
        lower_bounds,
        upper_bounds,
    )
}

fn lowest_score_unrepresented_mechanism_candidate(
    screened_candidates: &[ScreenedRestartCandidate],
    remaining_candidate_indices: &[usize],
    selected_candidate_indices: &[usize],
) -> Option<usize> {
    let selected_mechanism_families = selected_candidate_indices
        .iter()
        .filter_map(|candidate_index| screened_candidates.get(*candidate_index))
        .map(|candidate| candidate.mechanism_family)
        .collect::<Vec<_>>();

    remaining_candidate_indices
        .iter()
        .copied()
        .filter(|candidate_index| {
            screened_candidates
                .get(*candidate_index)
                .map(|candidate| !selected_mechanism_families.contains(&candidate.mechanism_family))
                .unwrap_or(false)
        })
        .min_by(|left_candidate_index, right_candidate_index| {
            compare_screened_restart_candidates_by_score(
                &screened_candidates[*left_candidate_index],
                &screened_candidates[*right_candidate_index],
            )
        })
}

fn global_loss_frontier_candidate_indices(
    screened_candidates: &[ScreenedRestartCandidate],
    remaining_candidate_indices: &[usize],
    candidate_frontier_count: usize,
) -> Vec<usize> {
    let frontier_count = candidate_frontier_count
        .max(1)
        .min(remaining_candidate_indices.len());

    let mut ranked_candidate_indices = remaining_candidate_indices.to_vec();

    ranked_candidate_indices.sort_by(|left_candidate_index, right_candidate_index| {
        compare_screened_restart_candidates_by_score(
            &screened_candidates[*left_candidate_index],
            &screened_candidates[*right_candidate_index],
        )
    });

    ranked_candidate_indices.truncate(frontier_count);

    ranked_candidate_indices
}

fn lowest_score_screened_restart_candidate(
    screened_candidates: &[ScreenedRestartCandidate],
    candidate_indices: &[usize],
) -> usize {
    let Some(first_candidate_index) = candidate_indices.first().copied() else {
        return 0;
    };

    candidate_indices.iter().copied().skip(1).fold(
        first_candidate_index,
        |best_candidate_index, candidate_index| {
            if compare_screened_restart_candidates_by_score(
                &screened_candidates[candidate_index],
                &screened_candidates[best_candidate_index],
            )
            .is_lt()
            {
                candidate_index
            } else {
                best_candidate_index
            }
        },
    )
}

fn most_distant_screened_restart_candidate(
    screened_candidates: &[ScreenedRestartCandidate],
    candidate_indices: &[usize],
    selected_candidate_indices: &[usize],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> usize {
    let mut best_candidate_index = candidate_indices[0];

    for candidate_index in candidate_indices.iter().copied().skip(1) {
        if screened_restart_candidate_is_more_distant(
            screened_candidates,
            candidate_index,
            best_candidate_index,
            selected_candidate_indices,
            lower_bounds,
            upper_bounds,
        ) {
            best_candidate_index = candidate_index;
        }
    }

    best_candidate_index
}

fn screened_restart_candidate_is_more_distant(
    screened_candidates: &[ScreenedRestartCandidate],
    left_candidate_index: usize,
    right_candidate_index: usize,
    selected_candidate_indices: &[usize],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> bool {
    let left_distance = minimum_normalized_parameter_distance_to_selected(
        screened_candidates,
        left_candidate_index,
        selected_candidate_indices,
        lower_bounds,
        upper_bounds,
    );

    let right_distance = minimum_normalized_parameter_distance_to_selected(
        screened_candidates,
        right_candidate_index,
        selected_candidate_indices,
        lower_bounds,
        upper_bounds,
    );

    match left_distance.total_cmp(&right_distance) {
        std::cmp::Ordering::Greater => return true,
        std::cmp::Ordering::Less => return false,
        std::cmp::Ordering::Equal => {}
    }

    let left_objective_distance = minimum_objective_component_distance_to_selected(
        screened_candidates,
        left_candidate_index,
        selected_candidate_indices,
    );

    let right_objective_distance = minimum_objective_component_distance_to_selected(
        screened_candidates,
        right_candidate_index,
        selected_candidate_indices,
    );

    match left_objective_distance.total_cmp(&right_objective_distance) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => compare_screened_restart_candidates_by_score(
            &screened_candidates[left_candidate_index],
            &screened_candidates[right_candidate_index],
        )
        .is_lt(),
    }
}

fn minimum_objective_component_distance_to_selected(
    screened_candidates: &[ScreenedRestartCandidate],
    candidate_index: usize,
    selected_candidate_indices: &[usize],
) -> f64 {
    selected_candidate_indices
        .iter()
        .map(|selected_candidate_index| {
            objective_component_distance(
                &screened_candidates[candidate_index].score,
                &screened_candidates[*selected_candidate_index].score,
            )
        })
        .fold(f64::INFINITY, f64::min)
}

fn objective_component_distance(
    left: &JointRefinementRestartAdmissionScreening,
    right: &JointRefinementRestartAdmissionScreening,
) -> f64 {
    let mut squared_distance_sum = 0.0;
    let mut shared_component_count = 0usize;

    for (left_component_id, left_component_score) in
        left.component_ids.iter().zip(left.component_scores.iter())
    {
        let Some(right_component_position) = right
            .component_ids
            .iter()
            .position(|right_component_id| right_component_id == left_component_id)
        else {
            continue;
        };

        let right_component_score = right.component_scores[right_component_position];
        if !left_component_score.is_finite() || !right_component_score.is_finite() {
            continue;
        }

        let component_distance = left_component_score - right_component_score;
        squared_distance_sum += component_distance * component_distance;
        shared_component_count += 1;
    }

    if shared_component_count == 0 {
        return 0.0;
    }

    (squared_distance_sum / shared_component_count as f64).sqrt()
}

fn minimum_normalized_parameter_distance_to_selected(
    screened_candidates: &[ScreenedRestartCandidate],
    candidate_index: usize,
    selected_candidate_indices: &[usize],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> f64 {
    selected_candidate_indices
        .iter()
        .map(|selected_candidate_index| {
            normalized_parameter_distance(
                screened_candidates[candidate_index]
                    .parameter_vector
                    .as_slice(),
                screened_candidates[*selected_candidate_index]
                    .parameter_vector
                    .as_slice(),
                lower_bounds,
                upper_bounds,
            )
        })
        .fold(f64::INFINITY, f64::min)
}

fn compare_screened_restart_candidates_by_score(
    left: &ScreenedRestartCandidate,
    right: &ScreenedRestartCandidate,
) -> std::cmp::Ordering {
    left.score
        .aggregate_score
        .total_cmp(&right.score.aggregate_score)
        .then_with(|| {
            left.score
                .component_score_sum()
                .total_cmp(&right.score.component_score_sum())
        })
        .then_with(|| {
            left.score
                .maximum_component_score()
                .total_cmp(&right.score.maximum_component_score())
        })
        .then_with(|| left.source_order.cmp(&right.source_order))
        .then_with(|| left.restart_id.cmp(&right.restart_id))
}

fn normalized_parameter_distance(
    left_parameter_vector: &[f64],
    right_parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> f64 {
    let squared_distance = left_parameter_vector
        .iter()
        .zip(right_parameter_vector.iter())
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .filter_map(|((left_value, right_value), (lower_bound, upper_bound))| {
            let width = upper_bound - lower_bound;
            (width.is_finite() && width > 0.0)
                .then_some(((left_value - right_value) / width).powi(2))
        })
        .sum::<f64>();

    squared_distance.sqrt()
}

fn screen_restart_candidates<F>(
    restart_candidates: &[(String, Vec<f64>)],
    seed_score: &F,
    joint_context: Option<&JointRefinementContext>,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_screening_units: &Mutex<usize>,
    total_screening_units: usize,
) -> Vec<ScreenedRestartCandidate>
where
    F: Fn(&str, &[f64]) -> Option<JointRefinementRestartAdmissionScreening> + Sync + ?Sized,
{
    let thread_pool = joint_context.and_then(|joint_context| {
        joint_context
            .stage_thread_pool
            .as_ref()
            .map(|thread_pool| thread_pool.as_ref())
    });

    let cancellation_token = joint_context
        .map(|joint_context| joint_context.cancellation_token.shared_handle())
        .unwrap_or_else(CancellationToken::uncancelled);

    screen_restart_candidates_with_thread_pool(
        restart_candidates,
        seed_score,
        thread_pool,
        cancellation_token,
        progress_sender,
        completed_screening_units,
        total_screening_units,
    )
}

fn screen_restart_candidates_with_thread_pool<F>(
    restart_candidates: &[(String, Vec<f64>)],
    seed_score: &F,
    thread_pool: Option<&rayon::ThreadPool>,
    cancellation_token: CancellationToken,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_screening_units: &Mutex<usize>,
    total_screening_units: usize,
) -> Vec<ScreenedRestartCandidate>
where
    F: Fn(&str, &[f64]) -> Option<JointRefinementRestartAdmissionScreening> + Sync + ?Sized,
{
    let screen_candidate =
        |(candidate_position, (restart_id, seed)): (usize, &(String, Vec<f64>))| {
            let score = seed_score(restart_id.as_str(), seed.as_slice());

            emit_completed_restart_candidate_screening_unit(
                progress_sender,
                completed_screening_units,
                total_screening_units,
                restart_id.as_str(),
            );

            ScreenedRestartCandidateOutput {
                candidate_position,
                candidate: ScreenedRestartCandidate::try_new(
                    restart_id.as_str(),
                    seed.as_slice(),
                    score,
                    candidate_position,
                ),
            }
        };

    if let Some(thread_pool) = thread_pool
        && thread_pool.current_num_threads() > 1
        && restart_candidates.len() > 1
    {
        let candidate_jobs = restart_candidates.iter().enumerate().collect::<Vec<_>>();

        let mut screened_candidates = std::iter::repeat_with(|| None)
            .take(candidate_jobs.len())
            .collect::<Vec<Option<ScreenedRestartCandidate>>>();

        run_static_work_pool(
            Some(thread_pool),
            StaticWorkPoolInput::new(
                &mut screened_candidates,
                cancellation_token.shared_handle(),
                candidate_jobs,
                thread_pool.current_num_threads(),
                |candidate: (usize, &(String, Vec<f64>)), cancellation_token: CancellationToken| {
                    if cancellation_token.is_cancelled() {
                        return ScreenedRestartCandidateOutput {
                            candidate_position: candidate.0,
                            candidate: None,
                        };
                    }

                    screen_candidate(candidate)
                },
                |screened_candidates: &mut Vec<Option<ScreenedRestartCandidate>>,
                 output: ScreenedRestartCandidateOutput,
                 _status| {
                    if let Some(candidate_slot) =
                        screened_candidates.get_mut(output.candidate_position)
                    {
                        *candidate_slot = output.candidate;
                    }
                },
            ),
        );

        return screened_candidates.into_iter().flatten().collect();
    }

    restart_candidates
        .iter()
        .enumerate()
        .filter_map(|candidate| {
            if cancellation_token.is_cancelled() {
                return None;
            }

            screen_candidate(candidate).candidate
        })
        .collect()
}

#[cfg(test)]
pub(in crate::stages::joint_refinement) fn restart_candidate_screening_max_parallelism_for_test(
    cpu_count: usize,
    candidate_count: usize,
) -> usize {
    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(crate::stages::parallelism::resolved_stage_cpu_count(
            cpu_count,
        ))
        .build()
        .expect("test thread pool should build");

    let active_count = Arc::new(AtomicUsize::new(0));
    let max_active_count = Arc::new(AtomicUsize::new(0));

    let restart_candidates = (0..candidate_count)
        .map(|candidate_index| {
            (
                format!("candidate_{candidate_index}"),
                vec![candidate_index as f64],
            )
        })
        .collect::<Vec<_>>();

    let completed_screening_units = Mutex::new(0usize);

    let score_candidate = |_: &str, _: &[f64]| {
        let current_active_count = active_count.fetch_add(1, Ordering::SeqCst) + 1;
        max_active_count.fetch_max(current_active_count, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(25));
        active_count.fetch_sub(1, Ordering::SeqCst);
        restart_admission_screening_from_scalar(0.0)
    };

    let _ = screen_restart_candidates_with_thread_pool(
        restart_candidates.as_slice(),
        &score_candidate,
        Some(&thread_pool),
        CancellationToken::uncancelled(),
        None,
        &completed_screening_units,
        candidate_count,
    );

    max_active_count.load(Ordering::SeqCst)
}

fn screen_halton_restart_candidates<F>(
    input: HaltonRestartCandidateScreeningInput<'_, F>,
) -> Vec<ScreenedRestartCandidate>
where
    F: Fn(&str, &[f64]) -> Option<JointRefinementRestartAdmissionScreening> + Sync + ?Sized,
{
    let HaltonRestartCandidateScreeningInput {
        baseline_parameter_vector,
        lower_bounds,
        upper_bounds,
        target_restart_count,
        source_order_offset,
        halton_seed_score,
        joint_context,
        progress_sender,
        completed_screening_units,
        total_screening_units,
    } = input;

    let screen_halton_position = |halton_position: usize| {
        let halton_seed = local_trust_region_halton_parameter_vector(
            halton_position,
            baseline_parameter_vector,
            lower_bounds,
            upper_bounds,
        );

        let restart_id = build_halton_restart_id(halton_position);

        let score = halton_seed_score(restart_id.as_str(), halton_seed.as_slice());

        emit_completed_restart_candidate_screening_unit(
            progress_sender,
            completed_screening_units,
            total_screening_units,
            restart_id.as_str(),
        );

        ScreenedRestartCandidateOutput {
            candidate_position: halton_position.saturating_sub(1),
            candidate: score.and_then(|score| {
                ScreenedRestartCandidate::try_new(
                    restart_id.as_str(),
                    halton_seed.as_slice(),
                    Some(score),
                    source_order_offset.saturating_add(halton_position),
                )
            }),
        }
    };

    if let Some(joint_context) = joint_context
        && joint_refinement_has_parallel_worker_capacity(joint_context)
        && target_restart_count > 1
    {
        let halton_jobs = (1..=target_restart_count).collect::<Vec<_>>();

        let mut screened_candidates = std::iter::repeat_with(|| None)
            .take(target_restart_count)
            .collect::<Vec<Option<ScreenedRestartCandidate>>>();

        run_static_work_pool(
            joint_context.stage_thread_pool.as_deref(),
            StaticWorkPoolInput::new(
                &mut screened_candidates,
                joint_context.cancellation_token.shared_handle(),
                halton_jobs,
                joint_refinement_parallel_worker_count(joint_context),
                |halton_position, _cancellation_token| screen_halton_position(halton_position),
                |screened_candidates: &mut Vec<Option<ScreenedRestartCandidate>>,
                 output: ScreenedRestartCandidateOutput,
                 _status| {
                    if let Some(candidate_slot) =
                        screened_candidates.get_mut(output.candidate_position)
                    {
                        *candidate_slot = output.candidate;
                    }
                },
            ),
        );

        return screened_candidates.into_iter().flatten().collect();
    }

    (1..=target_restart_count)
        .filter_map(|halton_position| screen_halton_position(halton_position).candidate)
        .collect()
}

fn local_trust_region_halton_parameter_vector(
    halton_position: usize,
    baseline_parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<f64> {
    let full_bounds_halton =
        halton_parameter_vector(halton_position as u64, lower_bounds, upper_bounds);

    let radius_fraction =
        normalized_local_trust_region_radius_fraction(baseline_parameter_vector.len());

    baseline_parameter_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .zip(full_bounds_halton.iter())
        .map(
            |((baseline_value, (lower_bound, upper_bound)), halton_value)| {
                local_trust_region_coordinate(
                    *baseline_value,
                    *lower_bound,
                    *upper_bound,
                    *halton_value,
                    radius_fraction,
                )
            },
        )
        .collect()
}

fn normalized_local_trust_region_radius_fraction(parameter_count: usize) -> f64 {
    // The radius keeps a full-coordinate displacement at unit length in normalized space.
    (parameter_count.max(1) as f64).sqrt().recip()
}

fn local_trust_region_coordinate(
    baseline_value: f64,
    lower_bound: f64,
    upper_bound: f64,
    full_bounds_halton_value: f64,
    radius_fraction: f64,
) -> f64 {
    let full_width = upper_bound - lower_bound;
    if !baseline_value.is_finite()
        || !lower_bound.is_finite()
        || !upper_bound.is_finite()
        || !full_width.is_finite()
        || full_width <= 0.0
    {
        return baseline_value;
    }

    let unit_position = ((full_bounds_halton_value - lower_bound) / full_width).clamp(0.0, 1.0);
    let signed_position = 2.0 * unit_position - 1.0;

    let directional_width = if signed_position < 0.0 {
        baseline_value - lower_bound
    } else {
        upper_bound - baseline_value
    };

    (baseline_value + signed_position * radius_fraction * directional_width)
        .clamp(lower_bound, upper_bound)
}

struct HaltonRestartCandidateScreeningInput<'a, F: ?Sized> {
    baseline_parameter_vector: &'a [f64],
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    target_restart_count: usize,
    source_order_offset: usize,
    halton_seed_score: &'a F,
    joint_context: Option<&'a JointRefinementContext>,
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    completed_screening_units: &'a Mutex<usize>,
    total_screening_units: usize,
}

fn emit_completed_restart_candidate_screening_unit(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_screening_units: &Mutex<usize>,
    total_screening_units: usize,
    current_item_label: &str,
) {
    let Ok(mut completed_units_guard) = completed_screening_units.lock() else {
        return;
    };

    *completed_units_guard = completed_units_guard.saturating_add(1);

    emit_restart_candidate_screening_progress(
        progress_sender,
        *completed_units_guard,
        total_screening_units,
        Some(current_item_label),
    );
}

fn emit_restart_candidate_screening_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(JointRefinementStageProgressUpdate {
        current_item_label: current_item_label.map(str::to_string),
        ..JointRefinementStageProgressUpdate::new(
            JointRefinementStageProgressPhase::RestartCandidateScreening,
            completed_units,
            total_units,
        )
    });
}

fn emit_restart_seed_generation_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(JointRefinementStageProgressUpdate {
        current_item_label: current_item_label.map(str::to_string),
        ..JointRefinementStageProgressUpdate::new(
            JointRefinementStageProgressPhase::RestartSeedGeneration,
            completed_units,
            total_units,
        )
    });
}

fn screened_exploratory_restart_seed_score(
    halton_seed: &[f64],
    joint_context: &JointRefinementContext,
) -> Option<JointRefinementRestartAdmissionScreening> {
    exploratory_restart_seed_diagnostics(halton_seed, joint_context)
        .ok()
        .map(|diagnostics| diagnostics.screening)
}

fn exploratory_restart_seed_diagnostics(
    restart_seed: &[f64],
    joint_context: &JointRefinementContext,
) -> Result<JointRefinementRestartAdmissionDiagnostics, &'static str> {
    let Some(fitted_parameters) =
        parameter_vector_to_fitted_parameters(restart_seed, joint_context)
    else {
        return Err("restart_seed_parameter_decoding_failed");
    };

    precheck_seed_summary_workload(&fitted_parameters, joint_context)
        .map_err(|_| "restart_seed_summary_workload_infeasible")?;

    evaluate_joint_fitted_candidate_restart_admission_diagnostics(&fitted_parameters, joint_context)
}

fn joint_tick_moment_restart_seeds(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    activity_log_shift: f64,
    joint_context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<(String, Vec<f64>)> {
    if !joint_context
        .baseline_modeled_open_event_rate_per_hour
        .is_finite()
        || joint_context.baseline_modeled_open_event_rate_per_hour <= 0.0
    {
        return Vec::new();
    }

    let baseline_tick_moment_calibration_sample = baseline_15m_calibration_sample_for_context(
        joint_context,
        joint_context.baseline_modeled_open_event_rate_per_hour,
    );

    let mut restart_seeds = joint_tick_shape_alpha_boundary_restart_seeds(
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        joint_context,
        joint_context.baseline_modeled_open_event_rate_per_hour,
        None,
    );

    restart_seeds.extend(
        projected_tick_moment_nig_search_points(ProjectedTickMomentNigSearchPointsInput {
            calibration_sample: &baseline_tick_moment_calibration_sample,
            observed_data: &joint_context.observed_data,
            baseline_modeled_open_event_rate_per_hour: joint_context
                .baseline_modeled_open_event_rate_per_hour,
            target_modeled_open_event_rate_per_hour: joint_context
                .baseline_modeled_open_event_rate_per_hour,
            solver_settings,
        })
        .into_iter()
        .enumerate()
        .map(|(configuration_index, projected_search_point)| {
            let mut baseline_restart_seed = baseline_parameter_vector.to_vec();

            apply_nig_search_point(
                baseline_restart_seed.as_mut_slice(),
                active_parameter_layout,
                projected_search_point,
            );

            clamp_in_place(
                baseline_restart_seed.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            (
                format!("tick_moment_{:02}", configuration_index + 1),
                baseline_restart_seed,
            )
        })
        .collect::<Vec<_>>(),
    );

    let mut activity_aligned_parameter_vector = baseline_parameter_vector.to_vec();

    apply_activity_alignment(
        activity_aligned_parameter_vector.as_mut_slice(),
        active_parameter_layout,
        activity_log_shift,
    );

    clamp_in_place(
        activity_aligned_parameter_vector.as_mut_slice(),
        lower_bounds,
        upper_bounds,
    );

    let Some(activity_aligned_fitted_parameters) = parameter_vector_to_fitted_parameters(
        activity_aligned_parameter_vector.as_slice(),
        joint_context,
    ) else {
        return restart_seeds;
    };

    let Some(modeled_open_event_rate_per_hour) =
        crate::stages::hawkes::stationary_open_event_rate_per_hour(
            &activity_aligned_fitted_parameters.hawkes_fitted_parameters,
        )
    else {
        return restart_seeds;
    };

    let activity_aligned_15m_calibration_sample = baseline_15m_calibration_sample_for_context(
        joint_context,
        modeled_open_event_rate_per_hour,
    );

    restart_seeds.extend(joint_tick_shape_alpha_boundary_restart_seeds(
        activity_aligned_parameter_vector.as_slice(),
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        joint_context,
        modeled_open_event_rate_per_hour,
        Some("activity_aligned"),
    ));

    restart_seeds.extend(
        projected_tick_moment_nig_search_points(ProjectedTickMomentNigSearchPointsInput {
            calibration_sample: &activity_aligned_15m_calibration_sample,
            observed_data: &joint_context.observed_data,
            baseline_modeled_open_event_rate_per_hour: joint_context
                .baseline_modeled_open_event_rate_per_hour,
            target_modeled_open_event_rate_per_hour: modeled_open_event_rate_per_hour,
            solver_settings,
        })
        .into_iter()
        .enumerate()
        .map(|(configuration_index, projected_search_point)| {
            let mut activity_aligned_restart_seed = activity_aligned_parameter_vector.clone();

            apply_nig_search_point(
                activity_aligned_restart_seed.as_mut_slice(),
                active_parameter_layout,
                projected_search_point,
            );

            clamp_in_place(
                activity_aligned_restart_seed.as_mut_slice(),
                lower_bounds,
                upper_bounds,
            );

            (
                format!(
                    "tick_moment_{:02}_activity_aligned",
                    configuration_index + 1
                ),
                activity_aligned_restart_seed,
            )
        }),
    );

    restart_seeds
}

fn joint_tick_shape_alpha_boundary_restart_seeds(
    baseline_parameter_vector: &[f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    joint_context: &JointRefinementContext,
    target_modeled_open_event_rate_per_hour: f64,
    seed_suffix: Option<&str>,
) -> Vec<(String, Vec<f64>)> {
    if !joint_context
        .baseline_modeled_open_event_rate_per_hour
        .is_finite()
        || joint_context.baseline_modeled_open_event_rate_per_hour <= 0.0
        || !target_modeled_open_event_rate_per_hour.is_finite()
        || target_modeled_open_event_rate_per_hour <= 0.0
    {
        return Vec::new();
    }

    let tick_calibration_sample = rescaled_observed_tick_calibration_sample_for_modeled_rate(
        &joint_context.observed_data,
        joint_context.baseline_modeled_open_event_rate_per_hour,
        target_modeled_open_event_rate_per_hour,
    );

    let (tick_log_alpha_lower_bound, tick_log_alpha_upper_bound) =
        nig_log_alpha_bounds_for_calibration_sample(&tick_calibration_sample);

    [
        ("tick_shape_alpha_lower", tick_log_alpha_lower_bound),
        ("tick_shape_alpha_upper", tick_log_alpha_upper_bound),
    ]
    .into_iter()
    .map(|(seed_label, alpha_coordinate)| {
        let mut restart_seed = baseline_parameter_vector.to_vec();
        restart_seed[active_parameter_layout.nig_block[0]] = alpha_coordinate;
        clamp_in_place(restart_seed.as_mut_slice(), lower_bounds, upper_bounds);

        (
            tick_shape_alpha_boundary_seed_id(seed_label, seed_suffix),
            restart_seed,
        )
    })
    .collect()
}

fn tick_shape_alpha_boundary_seed_id(seed_label: &str, seed_suffix: Option<&str>) -> String {
    match seed_suffix {
        Some(seed_suffix) => format!("{seed_label}_{seed_suffix}"),
        None => seed_label.to_string(),
    }
}

fn baseline_15m_calibration_sample_for_context(
    joint_context: &JointRefinementContext,
    modeled_open_event_rate_per_hour: f64,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    build_continuous_calibration_sample_from_aggregated_intervals(
        joint_context
            .observed_data
            .observed_15m_intervals
            .as_slice(),
        modeled_open_event_rate_per_hour,
        joint_context
            .observed_data
            .observed_open_event_rate_per_hour,
        joint_context.observed_data.fallback_sell_share_proxy,
    )
}

fn activity_aligned_calibration_sample(
    baseline_15m_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    activity_log_shift: f64,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    rescale_continuous_return_calibration_sample_business_time_scales(
        baseline_15m_calibration_sample,
        activity_log_shift.exp(),
    )
}

struct ProjectedTickMomentNigSearchPointsInput<'a> {
    calibration_sample: &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    observed_data: &'a super::super::execution::JointRefinementObservedData,
    baseline_modeled_open_event_rate_per_hour: f64,
    target_modeled_open_event_rate_per_hour: f64,
    solver_settings: &'a CalibrationOptimizerSettings,
}

fn projected_tick_moment_nig_search_points(
    input: ProjectedTickMomentNigSearchPointsInput<'_>,
) -> Vec<NigShapeSearchPoint> {
    let ProjectedTickMomentNigSearchPointsInput {
        calibration_sample,
        observed_data,
        baseline_modeled_open_event_rate_per_hour,
        target_modeled_open_event_rate_per_hour,
        solver_settings,
    } = input;

    if !baseline_modeled_open_event_rate_per_hour.is_finite()
        || baseline_modeled_open_event_rate_per_hour <= 0.0
        || !target_modeled_open_event_rate_per_hour.is_finite()
        || target_modeled_open_event_rate_per_hour <= 0.0
    {
        return Vec::new();
    }

    let tick_sample_business_time_scaled =
        rescaled_observed_tick_calibration_sample_for_modeled_rate(
            observed_data,
            baseline_modeled_open_event_rate_per_hour,
            target_modeled_open_event_rate_per_hour,
        );

    build_nig_moment_seed_configurations(&tick_sample_business_time_scaled, solver_settings)
        .into_iter()
        .filter_map(|configuration| {
            nig_search_point_from_configuration(calibration_sample, configuration)
        })
        .collect()
}

struct JointNigRestartSeedsBuildInput<'a> {
    baseline_parameter_vector: &'a [f64],
    active_parameter_layout: &'a JointRefinementActiveParameterLayout,
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    baseline_15m_calibration_sample: &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    activity_aligned_15m_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    return_component_basin_diagnostics:
        &'a [ReturnComponentsStageContinuousOptimizerBasinDiagnostic],
    activity_log_shift: f64,
    maximum_seed_count: usize,
}

fn joint_nig_basin_restart_seeds(
    input: JointNigRestartSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let JointNigRestartSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        activity_aligned_15m_calibration_sample,
        return_component_basin_diagnostics,
        activity_log_shift,
        maximum_seed_count,
    } = input;

    return_component_basin_diagnostics
        .iter()
        .take(maximum_seed_count.div_ceil(2))
        .filter_map(|diagnostic| {
            project_basin_diagnostic_into_joint_nig_coordinates(
                baseline_15m_calibration_sample,
                diagnostic,
            )
            .map(|baseline_search_point| {
                let activity_aligned_search_point =
                    project_basin_diagnostic_into_joint_nig_coordinates(
                        activity_aligned_15m_calibration_sample,
                        diagnostic,
                    );

                (
                    diagnostic,
                    baseline_search_point,
                    activity_aligned_search_point,
                )
            })
        })
        .flat_map(
            |(diagnostic, baseline_search_point, activity_aligned_search_point)| {
                let mut baseline_nig_seed = baseline_parameter_vector.to_vec();
                apply_nig_search_point(
                    baseline_nig_seed.as_mut_slice(),
                    active_parameter_layout,
                    baseline_search_point,
                );
                clamp_in_place(baseline_nig_seed.as_mut_slice(), lower_bounds, upper_bounds);

                let mut seeds = vec![(
                    format!("nig_basin_{}", diagnostic.family_label),
                    baseline_nig_seed,
                )];

                if let Some(activity_aligned_search_point) = activity_aligned_search_point {
                    let mut activity_aligned_nig_seed = baseline_parameter_vector.to_vec();
                    apply_nig_search_point(
                        activity_aligned_nig_seed.as_mut_slice(),
                        active_parameter_layout,
                        activity_aligned_search_point,
                    );
                    apply_activity_alignment(
                        activity_aligned_nig_seed.as_mut_slice(),
                        active_parameter_layout,
                        activity_log_shift,
                    );
                    clamp_in_place(
                        activity_aligned_nig_seed.as_mut_slice(),
                        lower_bounds,
                        upper_bounds,
                    );

                    seeds.push((
                        format!("nig_basin_{}_activity_aligned", diagnostic.family_label),
                        activity_aligned_nig_seed,
                    ));
                }

                seeds
            },
        )
        .collect()
}

fn joint_nig_alpha_delta_restart_seeds(
    input: JointNigRestartSeedsBuildInput<'_>,
) -> Vec<(String, Vec<f64>)> {
    let JointNigRestartSeedsBuildInput {
        baseline_parameter_vector,
        active_parameter_layout,
        lower_bounds,
        upper_bounds,
        baseline_15m_calibration_sample,
        activity_aligned_15m_calibration_sample,
        return_component_basin_diagnostics,
        activity_log_shift,
        maximum_seed_count,
    } = input;

    return_component_basin_diagnostics
        .iter()
        .take(maximum_seed_count.div_ceil(2))
        .filter_map(|diagnostic| {
            project_basin_diagnostic_into_joint_nig_coordinates(
                baseline_15m_calibration_sample,
                diagnostic,
            )
            .map(|baseline_search_point| {
                let activity_aligned_search_point =
                    project_basin_diagnostic_into_joint_nig_coordinates(
                        activity_aligned_15m_calibration_sample,
                        diagnostic,
                    );

                (
                    diagnostic,
                    baseline_search_point,
                    activity_aligned_search_point,
                )
            })
        })
        .flat_map(
            |(diagnostic, baseline_search_point, activity_aligned_search_point)| {
                let mut alpha_delta_seed = baseline_parameter_vector.to_vec();

                apply_nig_alpha_delta_search_point(
                    alpha_delta_seed.as_mut_slice(),
                    active_parameter_layout,
                    baseline_search_point,
                );

                clamp_in_place(alpha_delta_seed.as_mut_slice(), lower_bounds, upper_bounds);

                let mut seeds = vec![(
                    format!("nig_alpha_delta_{}", diagnostic.family_label),
                    alpha_delta_seed,
                )];

                if let Some(activity_aligned_search_point) = activity_aligned_search_point {
                    let mut activity_aligned_alpha_delta_seed = baseline_parameter_vector.to_vec();

                    apply_nig_alpha_delta_search_point(
                        activity_aligned_alpha_delta_seed.as_mut_slice(),
                        active_parameter_layout,
                        activity_aligned_search_point,
                    );

                    apply_activity_alignment(
                        activity_aligned_alpha_delta_seed.as_mut_slice(),
                        active_parameter_layout,
                        activity_log_shift,
                    );

                    clamp_in_place(
                        activity_aligned_alpha_delta_seed.as_mut_slice(),
                        lower_bounds,
                        upper_bounds,
                    );

                    seeds.push((
                        format!(
                            "nig_alpha_delta_{}_activity_aligned",
                            diagnostic.family_label
                        ),
                        activity_aligned_alpha_delta_seed,
                    ));
                }

                seeds
            },
        )
        .collect()
}

fn apply_activity_alignment(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    activity_log_shift: f64,
) {
    if let Some(index) = active_parameter_layout.known_activity_scale_index {
        parameter_vector[index] = activity_log_shift;
    }

    if let Some(index) = active_parameter_layout.surprise_activity_scale_index {
        parameter_vector[index] = activity_log_shift;
    }

    if let Some(index) = active_parameter_layout.surprise_baseline_scale_index {
        parameter_vector[index] = activity_log_shift;
    }
}

pub(in crate::stages::joint_refinement) fn selected_return_components_candidate(
    return_components_payload: &ReturnComponentsStageFittedPayload,
) -> Option<&ReturnComponentsStageCandidateDiagnostic> {
    return_components_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == return_components_payload.selected_candidate_id)
}

pub(in crate::stages::joint_refinement) fn project_basin_diagnostic_into_joint_nig_coordinates(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    diagnostic: &ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
) -> Option<NigShapeSearchPoint> {
    if !nig_basin_coordinates_are_finite(diagnostic) {
        return None;
    }

    let configuration = NigSharedPlusShiftConfiguration::try_new(
        diagnostic.final_alpha,
        diagnostic.final_beta_0,
        diagnostic.final_delta,
        0.0,
        diagnostic.final_delta_beta,
    )
    .ok()?;

    nig_search_point_from_configuration(calibration_sample, configuration)
}

fn nig_basin_coordinates_are_finite(
    diagnostic: &ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
) -> bool {
    [
        diagnostic.final_log_alpha,
        diagnostic.final_buy_skew_coordinate,
        diagnostic.final_sell_relative_coordinate,
        diagnostic.final_log_delta_offset,
    ]
    .into_iter()
    .all(f64::is_finite)
}

fn apply_nig_search_point(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    search_point: NigShapeSearchPoint,
) {
    parameter_vector[active_parameter_layout.nig_block[0]] = search_point.log_alpha;
    parameter_vector[active_parameter_layout.nig_block[1]] = search_point.buy_skew_coordinate;
    parameter_vector[active_parameter_layout.nig_block[2]] = search_point.sell_relative_coordinate;
    parameter_vector[active_parameter_layout.nig_block[3]] = search_point.log_delta_offset;
}

fn apply_nig_alpha_delta_search_point(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    search_point: NigShapeSearchPoint,
) {
    parameter_vector[active_parameter_layout.nig_block[0]] = search_point.log_alpha;
    parameter_vector[active_parameter_layout.nig_block[3]] = search_point.log_delta_offset;
}

pub(in crate::stages::joint_refinement) fn apply_nig_log_delta_offset_shift(
    parameter_vector: &mut [f64],
    active_parameter_layout: &JointRefinementActiveParameterLayout,
    log_delta_offset_shift: f64,
) {
    parameter_vector[active_parameter_layout.nig_block[3]] += log_delta_offset_shift;
}

pub(in crate::stages::joint_refinement) fn observed_activity_log_shift(
    observed_open_event_rate_per_hour: f64,
    modeled_open_event_rate_per_hour: f64,
) -> f64 {
    if !observed_open_event_rate_per_hour.is_finite()
        || observed_open_event_rate_per_hour <= 0.0
        || !modeled_open_event_rate_per_hour.is_finite()
        || modeled_open_event_rate_per_hour <= 0.0
    {
        return 0.0;
    }

    (observed_open_event_rate_per_hour / modeled_open_event_rate_per_hour).ln()
}

pub(in crate::stages::joint_refinement) fn observed_activity_log_shift_for_joint_context(
    joint_context: &JointRefinementContext,
) -> Option<f64> {
    let observed_total_count = joint_context
        .observed_data
        .observed_hourly_tick_counts
        .iter()
        .sum::<f64>();
    if !observed_total_count.is_finite() || observed_total_count <= 0.0 {
        return None;
    }

    let baseline_exposure = activity_exposure_for_simulation_template(joint_context)?;

    let baseline_expected_total_count =
        joint_context.baseline_modeled_open_event_rate_per_hour * baseline_exposure;
    if !baseline_expected_total_count.is_finite() || baseline_expected_total_count <= 0.0 {
        return None;
    }

    Some((observed_total_count / baseline_expected_total_count).ln())
}

fn activity_exposure_for_simulation_template(
    joint_context: &JointRefinementContext,
) -> Option<f64> {
    if joint_context.runtime_interval_seasonality_factors.len()
        != joint_context
            .observed_data
            .simulated_hourly_interval_template
            .len()
    {
        return None;
    }

    let mut exposure = 0.0;

    for (interval, interval_factor_grid) in joint_context
        .observed_data
        .simulated_hourly_interval_template
        .iter()
        .zip(joint_context.runtime_interval_seasonality_factors.iter())
    {
        if !interval_factor_grid.is_finite()
            || *interval_factor_grid < 0.0
            || interval.interval_end_utc_nanos < interval.interval_start_utc_nanos
        {
            return None;
        }

        let duration_hours = duration_nanos_to_hours(
            interval.interval_end_utc_nanos - interval.interval_start_utc_nanos,
        );

        exposure += *interval_factor_grid * duration_hours;
    }

    (exposure.is_finite() && exposure > 0.0).then_some(exposure)
}

pub(in crate::stages::joint_refinement) fn joint_refinement_final_simulation_seed_count(
    solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    match solver_settings.max_iterations {
        0..=32 => 1,
        33..=96 => 2,
        _ => 3,
    }
}

pub(in crate::stages::joint_refinement) fn cached_seed_summaries_for_candidate_seeds(
    input: CandidateSeedSummariesCacheInput<'_>,
) -> Result<Vec<(u64, JointRefinementSeedSummary)>, String> {
    let CandidateSeedSummariesCacheInput {
        parameter_bits,
        fitted_parameters,
        context,
        simulation_seeds,
        progress_sender,
        progress_phase,
        completed_units_before_seed_summaries,
        total_units,
    } = input;

    let cache_key = parameter_bits.map(|bits| JointRefinementSeedSummaryCacheKey {
        parameter_bits: bits.to_vec(),
        simulation_seed: 0,
    });

    let mut resolved_seed_summaries = Vec::with_capacity(simulation_seeds.len());
    let mut missing_seeds = Vec::new();

    for simulation_seed in simulation_seeds {
        let per_seed_cache_key =
            cache_key
                .as_ref()
                .map(|candidate_cache_key| JointRefinementSeedSummaryCacheKey {
                    parameter_bits: candidate_cache_key.parameter_bits.clone(),
                    simulation_seed: *simulation_seed,
                });

        if let Some(seed_cache_key) = per_seed_cache_key.as_ref()
            && let Ok(cache_guard) = context.seed_summary_cache.lock()
            && let Some(cached_summary) = cache_guard.get(seed_cache_key)
        {
            context.profiling_state.record_seed_summary_cache_hit();

            match cached_summary.clone() {
                Ok(summary) => {
                    resolved_seed_summaries.push((*simulation_seed, summary));
                    continue;
                }
                Err(error_message) => return Err(error_message),
            }
        }

        missing_seeds.push(*simulation_seed);
    }

    context
        .profiling_state
        .record_seed_summary_cache_miss(missing_seeds.len());

    let completed_units = completed_units_before_seed_summaries + resolved_seed_summaries.len();

    emit_seed_summary_progress(
        progress_sender,
        progress_phase,
        completed_units,
        total_units,
        None,
    );

    if !missing_seeds.is_empty() {
        let missing_seed_summaries = build_seed_summaries_for_candidate_seeds(
            context,
            fitted_parameters,
            missing_seeds.as_slice(),
            progress_sender,
            progress_phase,
            completed_units,
            total_units,
        )?;

        if let Some(candidate_cache_key) = cache_key
            && let Ok(mut cache_guard) = context.seed_summary_cache.lock()
        {
            for (simulation_seed, seed_summary) in &missing_seed_summaries {
                cache_guard.insert(
                    JointRefinementSeedSummaryCacheKey {
                        parameter_bits: candidate_cache_key.parameter_bits.clone(),
                        simulation_seed: *simulation_seed,
                    },
                    Ok(seed_summary.clone()),
                );
            }
        }

        resolved_seed_summaries.extend(missing_seed_summaries);
    }

    resolved_seed_summaries.sort_by_key(|(simulation_seed, _)| {
        simulation_seeds
            .iter()
            .position(|candidate_seed| candidate_seed == simulation_seed)
            .unwrap_or(simulation_seeds.len())
    });

    Ok(resolved_seed_summaries)
}

pub(in crate::stages::joint_refinement) struct CandidateSeedSummariesCacheInput<'a> {
    pub(in crate::stages::joint_refinement) parameter_bits: Option<&'a [u64]>,
    pub(in crate::stages::joint_refinement) fitted_parameters:
        &'a JointRefinementStageFittedParameters,
    pub(in crate::stages::joint_refinement) context: &'a JointRefinementContext,
    pub(in crate::stages::joint_refinement) simulation_seeds: &'a [u64],
    pub(in crate::stages::joint_refinement) progress_sender:
        Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(in crate::stages::joint_refinement) progress_phase:
        Option<JointRefinementStageProgressPhase>,
    pub(in crate::stages::joint_refinement) completed_units_before_seed_summaries: usize,
    pub(in crate::stages::joint_refinement) total_units: usize,
}

struct SeedSummaryOutput {
    seed_position: usize,
    result: Result<(u64, JointRefinementSeedSummary), String>,
}

pub(crate) fn build_surprise_multipliers_for_interval_grid(
    simulation_start_utc: DateTime<Utc>,
    interval_count: usize,
    interval_duration_nanos: i64,
    surprise_profile: &janiform_calibration_artifacts::SurpriseNewsDeterministicIntensityProfile,
) -> Result<Vec<(u64, f64)>, String> {
    crate::surprise_profile::materialize_surprise_news_weekly_profile_multipliers_for_interval_grid(
        simulation_start_utc,
        interval_count,
        interval_duration_nanos,
        surprise_profile,
    )
    .map_err(|source| source.to_string())
}

fn build_seed_summaries_for_candidate_seeds(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seeds: &[u64],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units_before_missing: usize,
    total_units: usize,
) -> Result<Vec<(u64, JointRefinementSeedSummary)>, String> {
    if joint_refinement_has_parallel_worker_capacity(context) && simulation_seeds.len() > 1 {
        return build_seed_summaries_for_candidate_seeds_parallel(
            context,
            fitted_parameters,
            simulation_seeds,
            progress_sender,
            progress_phase,
            completed_units_before_missing,
            total_units,
        );
    }

    let mut completed_units = completed_units_before_missing;
    let mut seed_summaries = Vec::with_capacity(simulation_seeds.len());

    for simulation_seed in simulation_seeds {
        let seed_summary =
            build_seed_summary_for_candidate_seed(context, fitted_parameters, *simulation_seed)?;

        completed_units += 1;

        emit_seed_summary_progress(
            progress_sender,
            progress_phase,
            completed_units,
            total_units,
            Some(*simulation_seed),
        );

        seed_summaries.push((*simulation_seed, seed_summary));
    }

    Ok(seed_summaries)
}

fn build_seed_summaries_for_candidate_seeds_parallel(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seeds: &[u64],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units_before_missing: usize,
    total_units: usize,
) -> Result<Vec<(u64, JointRefinementSeedSummary)>, String> {
    let completed_units = Mutex::new(completed_units_before_missing);
    let seed_summary_jobs = simulation_seeds
        .iter()
        .copied()
        .enumerate()
        .collect::<Vec<_>>();

    let mut seed_summary_outputs = std::iter::repeat_with(|| None)
        .take(seed_summary_jobs.len())
        .collect::<Vec<Option<Result<(u64, JointRefinementSeedSummary), String>>>>();

    run_static_work_pool(
        context.stage_thread_pool.as_deref(),
        StaticWorkPoolInput::new(
            &mut seed_summary_outputs,
            context.cancellation_token.shared_handle(),
            seed_summary_jobs,
            joint_refinement_parallel_worker_count(context),
            |(seed_position, simulation_seed), cancellation_token: CancellationToken| {
                if cancellation_token.is_cancelled() {
                    return SeedSummaryOutput {
                        seed_position,
                        result: Err("operation cancelled".to_string()),
                    };
                }

                let result = build_seed_summary_for_candidate_seed(
                    context,
                    fitted_parameters,
                    simulation_seed,
                )
                .map(|seed_summary| (simulation_seed, seed_summary));

                if result.is_ok()
                    && let Ok(mut completed_units_guard) = completed_units.lock()
                {
                    *completed_units_guard = completed_units_guard.saturating_add(1);
                    emit_seed_summary_progress(
                        progress_sender,
                        progress_phase,
                        *completed_units_guard,
                        total_units,
                        Some(simulation_seed),
                    );
                }

                SeedSummaryOutput {
                    seed_position,
                    result,
                }
            },
            |seed_summary_outputs: &mut Vec<
                Option<Result<(u64, JointRefinementSeedSummary), String>>,
            >,
             output: SeedSummaryOutput,
             _status| {
                if let Some(output_slot) = seed_summary_outputs.get_mut(output.seed_position) {
                    *output_slot = Some(output.result);
                }
            },
        ),
    );

    seed_summary_outputs
        .into_iter()
        .map(|output| {
            output.unwrap_or_else(|| Err("seed summary worker did not return result".to_string()))
        })
        .collect()
}

fn build_seed_summary_for_candidate_seed(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
) -> Result<JointRefinementSeedSummary, String> {
    let seed_summary_started_at = Instant::now();

    let seed_summary_result = context
        .summary_simulation_scheduler
        .run_seed_summary_simulation(
            fitted_parameters,
            simulation_seed,
            context.cancellation_token.shared_handle(),
        );

    context
        .profiling_state
        .record_seed_summary_simulation_duration(
            seed_summary_started_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        );

    seed_summary_result.map_err(|error| match error {
        JointRefinementSeedSummaryAcquisitionError::SimulationFailure { error_message } => {
            error_message
        }
        JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure { error_message } => {
            if let Ok(mut failure_guard) = context.simulation_infrastructure_failure.lock()
                && failure_guard.is_none()
            {
                *failure_guard = Some(error_message.clone());
            }

            error_message
        }
    })
}

fn emit_seed_summary_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units: usize,
    total_units: usize,
    simulation_seed: Option<u64>,
) {
    let Some(progress_phase) = progress_phase else {
        return;
    };

    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(JointRefinementStageProgressUpdate {
        current_item_label: simulation_seed.map(format_seed_summary_progress_item),
        ..JointRefinementStageProgressUpdate::new(progress_phase, completed_units, total_units)
    });
}

fn format_seed_summary_progress_item(simulation_seed: u64) -> String {
    format!("simulation_seed {simulation_seed}")
}

pub(in crate::stages::joint_refinement) fn selected_hawkes_observed_open_event_rate_per_hour(
    hawkes_payload: &HawkesStageFittedPayload,
) -> Option<f64> {
    hawkes_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == hawkes_payload.selected_candidate_id)
        .map(|candidate| candidate.implied_metrics.observed_open_event_rate_per_hour)
        .or_else(|| {
            crate::stages::hawkes::stationary_open_event_rate_per_hour(
                &hawkes_payload.fitted_parameters,
            )
        })
}

pub(in crate::stages::joint_refinement) fn stage_input_to_activity_input(
    hawkes_input: &janiform_calibration_artifacts::CalibrationStagePayloadSource<
        HawkesStageFittedPayload,
    >,
) -> janiform_calibration_artifacts::CalibrationStagePayloadSource<HawkesStageFittedPayload> {
    // Intentional copy because joint refinement builds a nested return-components input that owns
    // the same upstream Hawkes source contract.
    hawkes_input.clone()
}

#[cfg(test)]
mod tests {
    use super::{
        JointRefinementRestartAdmissionScreening, ScreenedRestartCandidate,
        build_active_parameter_layout, compare_screened_restart_candidates_by_score,
        disable_known_activity_coupling_if_known_news_channel_absent,
        restart_seed_mechanism_family, select_screened_restart_candidates,
    };
    use crate::stages::joint_refinement::execution::test_joint_fitted_parameters;

    fn screened_restart_candidate(
        restart_id: &str,
        parameter_vector: Vec<f64>,
        aggregate_score: f64,
        component_scores: Vec<f64>,
        source_order: usize,
    ) -> ScreenedRestartCandidate {
        ScreenedRestartCandidate {
            restart_id: restart_id.to_string(),
            parameter_vector,
            score: JointRefinementRestartAdmissionScreening {
                aggregate_score,
                component_ids: (0..component_scores.len())
                    .map(|component_position| format!("component_{component_position:02}"))
                    .collect(),
                component_scores,
            },
            mechanism_family: restart_seed_mechanism_family(restart_id),
            source_order,
        }
    }

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

    #[test]
    fn screened_restart_candidate_ordering_prefers_lower_global_loss() {
        let lower_global_loss_candidate = screened_restart_candidate(
            "lower_global_loss_candidate",
            vec![0.0],
            0.7,
            vec![0.2, 1.4],
            0,
        );
        let higher_global_loss_candidate =
            screened_restart_candidate("balanced_candidate", vec![1.0], 0.9, vec![0.9, 0.9], 1);

        assert!(
            compare_screened_restart_candidates_by_score(
                &lower_global_loss_candidate,
                &higher_global_loss_candidate,
            )
            .is_lt()
        );
    }

    #[test]
    fn screened_restart_selection_uses_parameter_diversity_within_selected_layer() {
        let screened_candidates = vec![
            screened_restart_candidate("near", vec![0.0, 0.0], 0.5, vec![0.5, 0.5], 0),
            screened_restart_candidate("close", vec![0.1, 0.0], 0.6, vec![0.4, 0.7], 1),
            screened_restart_candidate("far", vec![1.0, 1.0], 0.6, vec![0.7, 0.4], 2),
        ];
        let selected = select_screened_restart_candidates(
            screened_candidates,
            2,
            &[0.0, 0.0],
            &[1.0, 1.0],
            &[],
        );

        let selected_ids = selected
            .iter()
            .map(|(restart_id, _)| restart_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected_ids, vec!["near", "far"]);
    }

    #[test]
    fn screened_restart_selection_represents_mechanisms_before_repeating_one() {
        let screened_candidates = vec![
            screened_restart_candidate("tick_moment_01", vec![0.0, 0.0], 0.1, vec![0.1], 0),
            screened_restart_candidate("tick_moment_02", vec![1.0, 1.0], 0.2, vec![0.2], 1),
            screened_restart_candidate(
                "return_scale_tick_mean_absolute_return",
                vec![0.2, 0.0],
                5.0,
                vec![5.0],
                2,
            ),
            screened_restart_candidate("tick_shape_alpha_lower", vec![0.0, 0.2], 6.0, vec![6.0], 3),
        ];

        let selected = select_screened_restart_candidates(
            screened_candidates,
            3,
            &[0.0, 0.0],
            &[1.0, 1.0],
            &[],
        );

        let selected_ids = selected
            .iter()
            .map(|(restart_id, _)| restart_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            selected_ids,
            vec![
                "tick_moment_01",
                "return_scale_tick_mean_absolute_return",
                "tick_shape_alpha_lower"
            ]
        );
    }

    #[test]
    fn screened_restart_selection_uses_objective_family_coverage_after_parameter_distance() {
        let screened_candidates = vec![
            screened_restart_candidate("dominant", vec![0.0, 0.0], 0.1, vec![0.0, 0.0], 0),
            screened_restart_candidate(
                "objective_distinct",
                vec![0.05, 0.0],
                0.2,
                vec![3.0, 0.0],
                1,
            ),
            screened_restart_candidate("parameter_far", vec![1.0, 1.0], 0.2, vec![0.1, 0.1], 2),
        ];

        let selected = select_screened_restart_candidates(
            screened_candidates,
            2,
            &[0.0, 0.0],
            &[1.0, 1.0],
            &[],
        );

        let selected_ids = selected
            .iter()
            .map(|(restart_id, _)| restart_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected_ids, vec!["dominant", "parameter_far"]);
    }

    #[test]
    fn screened_restart_selection_uses_objective_family_coverage_as_distance_tiebreaker() {
        let screened_candidates = vec![
            screened_restart_candidate("dominant", vec![0.0, 0.0], 0.1, vec![0.0, 0.0], 0),
            screened_restart_candidate(
                "objective_distinct",
                vec![0.5, 0.0],
                0.2,
                vec![3.0, 0.0],
                1,
            ),
            screened_restart_candidate("objective_close", vec![-0.5, 0.0], 0.2, vec![0.1, 0.1], 2),
        ];

        let selected = select_screened_restart_candidates(
            screened_candidates,
            2,
            &[-1.0, -1.0],
            &[1.0, 1.0],
            &[],
        );

        let selected_ids = selected
            .iter()
            .map(|(restart_id, _)| restart_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected_ids, vec!["dominant", "objective_distinct"]);
    }

    #[test]
    fn screened_restart_selection_retains_protected_anchors_before_frontier_fill() {
        let screened_candidates = vec![
            screened_restart_candidate("dominant", vec![0.0, 0.0], 0.1, vec![0.1, 0.1], 0),
            screened_restart_candidate(
                "incumbent_local_search",
                vec![1.0, 1.0],
                5.0,
                vec![5.0, 5.0],
                1,
            ),
            screened_restart_candidate("next_best", vec![0.5, 0.5], 0.2, vec![0.2, 0.2], 2),
        ];
        let selected = select_screened_restart_candidates(
            screened_candidates,
            2,
            &[0.0, 0.0],
            &[1.0, 1.0],
            &["incumbent_local_search"],
        );

        let selected_ids = selected
            .iter()
            .map(|(restart_id, _)| restart_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected_ids, vec!["incumbent_local_search", "dominant"]);
    }

    #[test]
    fn screened_restart_selection_uses_frontier_when_budget_cannot_cover_protected_set() {
        let screened_candidates = vec![
            screened_restart_candidate("dominant", vec![0.0, 0.0], 0.1, vec![0.1, 0.1], 0),
            screened_restart_candidate(
                "incumbent_local_search",
                vec![1.0, 1.0],
                5.0,
                vec![5.0, 5.0],
                1,
            ),
            screened_restart_candidate("activity_aligned", vec![0.5, 0.5], 4.0, vec![4.0, 4.0], 2),
        ];
        let selected = select_screened_restart_candidates(
            screened_candidates,
            1,
            &[0.0, 0.0],
            &[1.0, 1.0],
            &["incumbent_local_search", "activity_aligned"],
        );

        let selected_ids = selected
            .iter()
            .map(|(restart_id, _)| restart_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected_ids, vec!["dominant"]);
    }
}
