use std::f64::consts::PI;

use janiform_affine_oracle::sufficient_statistics::SufficientStatisticsTransformSliceSettings;
use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};
use janiform_foundations::numeric::arithmetic_mean_or_zero as arithmetic_mean;
use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
use janiform_foundations::splitmix64_output_mixer;
use janiform_model_kernels::nig::{NigBuySellDriverParameters, NigSharedPlusShiftConfiguration};

use super::super::derived_contracts::return_components_stage_positive_domain_floor;
use super::constants::{
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_FINAL_INTEGRATION_STEP_COUNT,
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_LOCAL_INTEGRATION_STEP_COUNT,
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_SEED_INTEGRATION_STEP_COUNT,
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_STANDARDIZED_INTEGRATION_UPPER_BOUND,
};
use super::sample::build_continuous_return_optimizer_context;
use super::types::{
    ContinuousLikelihoodFidelityTier, ContinuousLikelihoodObjective,
    ContinuousReturnOptimizerContext, ReturnComponentsStageContinuousReturnCalibrationSample,
};

pub(crate) fn evaluate_configuration_likelihood_objective(
    configuration: NigSharedPlusShiftConfiguration,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> ContinuousLikelihoodObjective {
    let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);
    continuous_block_likelihood_objective(configuration, &optimizer_context, fidelity_tier)
}

pub(super) fn profile_likelihood_score(objective: ContinuousLikelihoodObjective) -> f64 {
    arithmetic_mean(
        [
            objective.global_negative_log_likelihood,
            objective.conditional_negative_log_likelihood,
        ]
        .as_slice(),
    ) + objective.boundary_penalty
}

pub(super) fn maximum_profiled_open_beta_ratio(alpha: f64) -> f64 {
    let probe_gamma_floor = coarse_transform_probe_gamma_floor();
    if !alpha.is_finite() || alpha <= probe_gamma_floor {
        return 0.0;
    }

    (1.0 - (probe_gamma_floor / alpha).powi(2))
        .sqrt()
        .clamp(0.0, maximum_open_beta_ratio())
}

pub(super) fn continuous_block_likelihood_objective(
    configuration: NigSharedPlusShiftConfiguration,
    optimizer_context: &ContinuousReturnOptimizerContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> ContinuousLikelihoodObjective {
    let boundary_penalty = alpha_beta_boundary_penalty(configuration);

    let integration_step_count = integration_step_count_for_fidelity_tier(fidelity_tier);
    let selected_member_indices =
        member_indices_for_fidelity_tier(optimizer_context, fidelity_tier);

    let global_negative_log_likelihood = average_negative_log_likelihood(
        configuration,
        optimizer_context,
        selected_member_indices,
        integration_step_count,
    )
    .unwrap_or(f64::INFINITY);

    let conditional_negative_log_likelihood = if optimizer_context.scale_bucket_profiles.is_empty()
    {
        global_negative_log_likelihood
    } else {
        arithmetic_mean(
            optimizer_context
                .scale_bucket_profiles
                .iter()
                .filter_map(|scale_bucket_profile| {
                    let bucket_member_indices: Vec<usize> = scale_bucket_profile
                        .member_indices
                        .iter()
                        .copied()
                        .filter(|member_index| {
                            selected_member_indices.binary_search(member_index).is_ok()
                        })
                        .collect();
                    if bucket_member_indices.len() < 24 {
                        return None;
                    }

                    average_negative_log_likelihood(
                        configuration,
                        optimizer_context,
                        bucket_member_indices.as_slice(),
                        integration_step_count,
                    )
                })
                .collect::<Vec<_>>()
                .as_slice(),
        )
    };

    let log_likelihood_shift = optimizer_context.log_likelihood_shift;
    let blended_negative_log_likelihood = arithmetic_mean(
        [
            global_negative_log_likelihood,
            conditional_negative_log_likelihood,
        ]
        .as_slice(),
    );

    ContinuousLikelihoodObjective {
        shifted_objective_score: log_likelihood_shift
            + blended_negative_log_likelihood
            + boundary_penalty,
        global_negative_log_likelihood,
        conditional_negative_log_likelihood,
        boundary_penalty,
        log_likelihood_shift,
    }
}

pub(super) fn average_negative_log_likelihood(
    configuration: NigSharedPlusShiftConfiguration,
    optimizer_context: &ContinuousReturnOptimizerContext,
    member_indices: &[usize],
    integration_step_count: usize,
) -> Option<f64> {
    let driver_parameters = configuration.derive_driver_parameters().ok()?;
    let mut log_likelihood_sum = 0.0;
    let mut evaluated_count = 0usize;

    for member_index in member_indices {
        let observed_return = *optimizer_context.observed_returns.get(*member_index)?;
        let business_time_scale = *optimizer_context.business_time_scales.get(*member_index)?;
        let sell_share_proxy = *optimizer_context.sell_share_proxies.get(*member_index)?;
        let integrated_buy_intensity = *optimizer_context
            .integrated_buy_intensities
            .get(*member_index)?;
        let integrated_sell_intensity = *optimizer_context
            .integrated_sell_intensities
            .get(*member_index)?;

        let block_log_density = log_density_net_trading_time_return(
            driver_parameters,
            configuration,
            observed_return,
            business_time_scale,
            sell_share_proxy,
            integrated_buy_intensity,
            integrated_sell_intensity,
            integration_step_count,
        )?;

        log_likelihood_sum += block_log_density;
        evaluated_count += 1;
    }

    if evaluated_count == 0 {
        return None;
    }

    Some(-log_likelihood_sum / evaluated_count as f64)
}

pub(super) fn log_density_net_trading_time_return(
    driver_parameters: NigBuySellDriverParameters,
    configuration: NigSharedPlusShiftConfiguration,
    observed_return: f64,
    business_time_scale: f64,
    sell_share_proxy: f64,
    integrated_buy_intensity: f64,
    integrated_sell_intensity: f64,
    integration_step_count: usize,
) -> Option<f64> {
    if !observed_return.is_finite()
        || !business_time_scale.is_finite()
        || business_time_scale <= 0.0
        || !integrated_buy_intensity.is_finite()
        || !integrated_sell_intensity.is_finite()
        || integrated_buy_intensity < 0.0
        || integrated_sell_intensity < 0.0
        || integration_step_count == 0
    {
        return None;
    }

    let block_standard_deviation =
        block_return_standard_deviation(configuration, business_time_scale, sell_share_proxy)?;
    let standardized_return = observed_return / block_standard_deviation;
    let integration_upper_bound = standardized_integration_upper_bound(standardized_return.abs());

    let integration_step_width = integration_upper_bound / integration_step_count as f64;
    if !integration_step_width.is_finite() || integration_step_width <= 0.0 {
        return None;
    }

    let mut integral_estimate = 0.0;
    for integration_index in 0..integration_step_count {
        let standardized_frequency = (integration_index as f64 + 0.5) * integration_step_width;
        let actual_frequency = standardized_frequency / block_standard_deviation;

        let characteristic_exponent = driver_parameters
            .compose_trading_time_weighted_characteristic_exponent(
                ComplexScalar::new(actual_frequency, 0.0),
                integrated_buy_intensity,
                integrated_sell_intensity,
            )
            .combined_weighted_exponent;
        if !characteristic_exponent.is_finite() {
            return None;
        }

        let characteristic_function = complex_exponential(characteristic_exponent);
        if !characteristic_function.is_finite() {
            return None;
        }

        let oscillation = complex_exponential(ComplexScalar::new(
            0.0,
            -standardized_return * standardized_frequency,
        ));
        let integrand = complex_multiply(characteristic_function, oscillation).real;
        if !integrand.is_finite() {
            return None;
        }

        integral_estimate += integrand;
    }

    let density = integration_step_width * integral_estimate / (PI * block_standard_deviation);
    if !density.is_finite() {
        return None;
    }

    Some(density.max(f64::MIN_POSITIVE).ln())
}

pub(super) fn alpha_beta_boundary_penalty(configuration: NigSharedPlusShiftConfiguration) -> f64 {
    let buy_margin = configuration.alpha - configuration.beta_0.abs();
    let sell_margin = configuration.alpha - configuration.derived_sell_beta().abs();
    if !buy_margin.is_finite() || !sell_margin.is_finite() {
        return f64::INFINITY;
    }

    if buy_margin <= 0.0 || sell_margin <= 0.0 {
        return f64::INFINITY;
    }

    let probe_gamma_floor = coarse_transform_probe_gamma_floor();
    let minimum_gamma = boundary_distance_gamma(configuration.alpha, configuration.beta_0).min(
        boundary_distance_gamma(configuration.alpha, configuration.derived_sell_beta()),
    );
    if !minimum_gamma.is_finite() || minimum_gamma <= 0.0 {
        return f64::INFINITY;
    }

    if minimum_gamma >= probe_gamma_floor {
        return 0.0;
    }

    (probe_gamma_floor / minimum_gamma).ln()
}

pub(super) fn simulate_matched_scale_returns(
    configuration: NigSharedPlusShiftConfiguration,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
) -> Vec<f64> {
    let Ok(driver_parameters) = configuration.derive_driver_parameters() else {
        return Vec::new();
    };

    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(
        candidate_sampling_seed(stage_seed, configuration),
    );
    let mut synthetic_returns = Vec::with_capacity(calibration_sample.business_time_scales.len());

    for (business_time_scale, sell_share_proxy) in calibration_sample
        .business_time_scales
        .iter()
        .zip(calibration_sample.sell_share_proxies.iter())
    {
        let sell_weight = sell_share_proxy.clamp(0.0, 1.0);
        let buy_weight = 1.0 - sell_weight;

        let buy_business_time_scale = (business_time_scale * buy_weight).max(0.0);
        let sell_business_time_scale = (business_time_scale * sell_weight).max(0.0);

        let Ok(sampled_increment) = driver_parameters.sample_trading_time_return_increment(
            buy_business_time_scale,
            sell_business_time_scale,
            &mut randomness_stream,
        ) else {
            return Vec::new();
        };

        synthetic_returns.push(sampled_increment.combined_increment);
    }

    synthetic_returns
}

pub(super) fn minimum_open_beta_ratio() -> f64 {
    (-1.0f64).next_up()
}

pub(super) fn maximum_open_beta_ratio() -> f64 {
    1.0f64.next_down()
}

fn member_indices_for_fidelity_tier(
    optimizer_context: &ContinuousReturnOptimizerContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> &[usize] {
    match fidelity_tier {
        ContinuousLikelihoodFidelityTier::Seed => optimizer_context.seed_member_indices.as_slice(),
        ContinuousLikelihoodFidelityTier::Local => {
            optimizer_context.local_member_indices.as_slice()
        }
        ContinuousLikelihoodFidelityTier::Full => optimizer_context.full_member_indices.as_slice(),
    }
}

fn integration_step_count_for_fidelity_tier(
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> usize {
    match fidelity_tier {
        ContinuousLikelihoodFidelityTier::Seed => {
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_SEED_INTEGRATION_STEP_COUNT
        }
        ContinuousLikelihoodFidelityTier::Local => {
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_LOCAL_INTEGRATION_STEP_COUNT
        }
        ContinuousLikelihoodFidelityTier::Full => {
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_FINAL_INTEGRATION_STEP_COUNT
        }
    }
}

fn block_return_standard_deviation(
    configuration: NigSharedPlusShiftConfiguration,
    business_time_scale: f64,
    sell_share_proxy: f64,
) -> Option<f64> {
    let (_, variance_per_unit, _, _) =
        super::super::execution::theoretical_net_process_moments(configuration, sell_share_proxy)?;

    let variance = (variance_per_unit * business_time_scale)
        .max(return_components_stage_positive_domain_floor());
    let standard_deviation = variance.sqrt();

    standard_deviation.is_finite().then_some(standard_deviation)
}

fn standardized_integration_upper_bound(absolute_standardized_return: f64) -> f64 {
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_STANDARDIZED_INTEGRATION_UPPER_BOUND
        + 8.0 * absolute_standardized_return.min(4.0)
}

fn boundary_distance_gamma(alpha: f64, beta: f64) -> f64 {
    let alpha_minus_abs_beta = alpha - beta.abs();
    let alpha_plus_abs_beta = alpha + beta.abs();
    if !alpha_minus_abs_beta.is_finite()
        || !alpha_plus_abs_beta.is_finite()
        || alpha_minus_abs_beta <= 0.0
        || alpha_plus_abs_beta <= 0.0
    {
        return f64::NAN;
    }

    (alpha_minus_abs_beta * alpha_plus_abs_beta).sqrt()
}

fn coarse_transform_probe_gamma_floor() -> f64 {
    SufficientStatisticsTransformSliceSettings::default().a_plus_u
}

fn candidate_sampling_seed(stage_seed: u64, configuration: NigSharedPlusShiftConfiguration) -> u64 {
    let mut mixed = stage_seed ^ 0xC6A4_A793_5BD1_E995;
    for parameter_bits in [
        configuration.alpha.to_bits(),
        configuration.beta_0.to_bits(),
        configuration.delta.to_bits(),
        configuration.mu.to_bits(),
        configuration.delta_beta.to_bits(),
    ] {
        mixed = splitmix64_output_mixer(mixed ^ parameter_bits);
    }

    mixed
}
