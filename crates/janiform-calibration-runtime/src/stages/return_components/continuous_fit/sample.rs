use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use janiform_foundations::numeric::{
    arithmetic_mean_or_zero as arithmetic_mean, smallest_positive_value,
    strictly_positive_machine_value,
};

use super::super::contracts::ReturnComponentsObservedOpenSessionReturn;
use super::super::execution::{
    ReturnComponentsStageReturnDistributionSummary, ReturnComponentsStageSortedSignalInputs,
    summarize_return_distribution,
};
use super::profile::{build_distribution_profile, build_scale_bucket_profiles};
use super::proxy_em_sample::build_proxy_em_fitting_sample;
use super::stratified_representatives::representative_member_indices_from_sorted_strata;
use super::types::{
    ContinuousLikelihoodFidelityTier, ContinuousReturnOptimizerContext,
    ReturnComponentsStageContinuousReturnCalibrationSample,
};
use crate::stages::joint_metrics::equal_stride_sample_indices;

pub(crate) fn build_continuous_return_calibration_sample(
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    continuous_fit_business_time_scales: &[f64],
    open_session_sell_share_proxy: f64,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    let aligned_pairs = build_equal_business_time_blocks(
        sorted_signals.continuous_fit_returns.as_slice(),
        continuous_fit_business_time_scales,
        sorted_signals.continuous_fit_returns.len(),
    );
    let observed_returns: Vec<f64> = aligned_pairs
        .iter()
        .map(|(observation, _)| observation.log_return)
        .collect();
    let business_time_scales: Vec<f64> = aligned_pairs.iter().map(|(_, scale)| *scale).collect();
    let sell_share_proxies: Vec<f64> = aligned_pairs
        .iter()
        .map(|(observation, _)| observation.sell_share_proxy)
        .collect();
    let observed_profile = build_distribution_profile(observed_returns.as_slice());
    let scale_bucket_profiles =
        build_scale_bucket_profiles(observed_returns.as_slice(), business_time_scales.as_slice());

    ReturnComponentsStageContinuousReturnCalibrationSample {
        observed_returns,
        business_time_scales,
        sell_share_proxies,
        observed_profile,
        scale_bucket_profiles,
        open_session_sell_share_proxy,
    }
}

pub(crate) fn summarize_observed_continuous_return_calibration_sample(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> ReturnComponentsStageReturnDistributionSummary {
    summarize_return_distribution(
        calibration_sample.observed_returns.as_slice(),
        calibration_sample.business_time_scales.as_slice(),
    )
}

pub(crate) fn continuous_return_calibration_sample_count(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> usize {
    calibration_sample.observed_returns.len()
}

pub(crate) fn mean_continuous_return_calibration_business_time_scale(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> f64 {
    arithmetic_mean(calibration_sample.business_time_scales.as_slice())
}

pub(crate) fn continuous_return_calibration_sample_observed_returns(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> &[f64] {
    calibration_sample.observed_returns.as_slice()
}

pub(crate) fn continuous_return_calibration_sample_business_time_scales(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> &[f64] {
    calibration_sample.business_time_scales.as_slice()
}

pub(crate) fn sample_continuous_return_calibration_sample_by_equal_stride(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    target_count: usize,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    let aligned_count = calibration_sample
        .observed_returns
        .len()
        .min(calibration_sample.business_time_scales.len())
        .min(calibration_sample.sell_share_proxies.len());

    let sample_indices = equal_stride_sample_indices(aligned_count, target_count);

    build_continuous_return_calibration_sample_from_components(
        sample_indices
            .iter()
            .filter_map(|sample_index| calibration_sample.observed_returns.get(*sample_index))
            .copied()
            .collect(),
        sample_indices
            .iter()
            .filter_map(|sample_index| calibration_sample.business_time_scales.get(*sample_index))
            .copied()
            .collect(),
        sample_indices
            .iter()
            .filter_map(|sample_index| calibration_sample.sell_share_proxies.get(*sample_index))
            .copied()
            .collect(),
        calibration_sample.open_session_sell_share_proxy,
    )
}

pub(crate) fn build_continuous_return_calibration_sample_from_components(
    observed_returns: Vec<f64>,
    business_time_scales: Vec<f64>,
    sell_share_proxies: Vec<f64>,
    open_session_sell_share_proxy: f64,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    let observed_profile = build_distribution_profile(observed_returns.as_slice());

    let scale_bucket_profiles =
        build_scale_bucket_profiles(observed_returns.as_slice(), business_time_scales.as_slice());

    ReturnComponentsStageContinuousReturnCalibrationSample {
        observed_returns,
        business_time_scales,
        sell_share_proxies,
        observed_profile,
        scale_bucket_profiles,
        open_session_sell_share_proxy,
    }
}

pub(crate) fn rescale_continuous_return_calibration_sample_business_time_scales(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    scale_multiplier: f64,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    let resolved_scale_multiplier = scale_multiplier.max(strictly_positive_machine_value());

    build_continuous_return_calibration_sample_from_components(
        calibration_sample.observed_returns.clone(),
        calibration_sample
            .business_time_scales
            .iter()
            .map(|business_time_scale| business_time_scale * resolved_scale_multiplier)
            .collect(),
        calibration_sample.sell_share_proxies.clone(),
        calibration_sample.open_session_sell_share_proxy,
    )
}

pub(crate) fn build_continuous_return_optimizer_context(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> ContinuousReturnOptimizerContext {
    let business_time_floor =
        positive_business_time_floor(calibration_sample.business_time_scales.as_slice());

    let integrated_buy_intensities: Vec<f64> = calibration_sample
        .business_time_scales
        .iter()
        .zip(calibration_sample.sell_share_proxies.iter())
        .map(|(business_time_scale, sell_share_proxy)| {
            business_time_scale * (1.0 - sell_share_proxy.clamp(0.0, 1.0))
        })
        .collect();

    let integrated_sell_intensities: Vec<f64> = calibration_sample
        .business_time_scales
        .iter()
        .zip(calibration_sample.sell_share_proxies.iter())
        .map(|(business_time_scale, sell_share_proxy)| {
            business_time_scale * sell_share_proxy.clamp(0.0, 1.0)
        })
        .collect();

    ContinuousReturnOptimizerContext {
        observed_returns: calibration_sample.observed_returns.clone(),
        business_time_scales: calibration_sample.business_time_scales.clone(),
        mean_business_time_scale: arithmetic_mean(
            calibration_sample.business_time_scales.as_slice(),
        )
        .max(business_time_floor),
        sell_share_proxies: calibration_sample.sell_share_proxies.clone(),
        member_weights: vec![1.0; calibration_sample.observed_returns.len()],
        integrated_buy_intensities,
        integrated_sell_intensities,
        scale_bucket_profiles: calibration_sample.scale_bucket_profiles.clone(),
        observed_return_volatility: calibration_sample.observed_profile.return_volatility,
        log_likelihood_shift: log_likelihood_shift_from_observed_profile(
            calibration_sample.observed_profile,
        ),
        open_session_sell_share_proxy: calibration_sample.open_session_sell_share_proxy,
        seed_member_indices: fidelity_member_indices(
            calibration_sample.observed_returns.len(),
            calibration_sample.sell_share_proxies.as_slice(),
            calibration_sample.scale_bucket_profiles.as_slice(),
            ContinuousLikelihoodFidelityTier::Seed,
        ),
        local_member_indices: fidelity_member_indices(
            calibration_sample.observed_returns.len(),
            calibration_sample.sell_share_proxies.as_slice(),
            calibration_sample.scale_bucket_profiles.as_slice(),
            ContinuousLikelihoodFidelityTier::Local,
        ),
        full_member_indices: fidelity_member_indices(
            calibration_sample.observed_returns.len(),
            calibration_sample.sell_share_proxies.as_slice(),
            calibration_sample.scale_bucket_profiles.as_slice(),
            ContinuousLikelihoodFidelityTier::Full,
        ),
        cache: Arc::new(Mutex::new(BTreeMap::new())),
    }
}

pub(crate) fn build_weighted_continuous_return_optimizer_context(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> ContinuousReturnOptimizerContext {
    let representative_sample = build_proxy_em_fitting_sample(calibration_sample);
    if representative_sample.sample_count() == calibration_sample.observed_returns.len() {
        return build_continuous_return_optimizer_context(calibration_sample);
    }

    let sell_share_proxies = representative_sample
        .member_indices()
        .iter()
        .map(|member_index| calibration_sample.sell_share_proxies[*member_index])
        .collect();

    let weighted_calibration_sample = build_continuous_return_calibration_sample_from_components(
        representative_sample.observed_returns().to_vec(),
        representative_sample.business_time_scales().to_vec(),
        sell_share_proxies,
        calibration_sample.open_session_sell_share_proxy,
    );

    let mut optimizer_context =
        build_continuous_return_optimizer_context(&weighted_calibration_sample);

    optimizer_context.member_weights = representative_sample.weights().to_vec();

    optimizer_context
}

pub(crate) fn rescale_continuous_return_optimizer_context_business_time_scales(
    optimizer_context: &ContinuousReturnOptimizerContext,
    scale_multiplier: f64,
) -> ContinuousReturnOptimizerContext {
    let resolved_scale_multiplier = scale_multiplier.max(strictly_positive_machine_value());

    let business_time_scales = optimizer_context
        .business_time_scales
        .iter()
        .map(|business_time_scale| business_time_scale * resolved_scale_multiplier)
        .collect::<Vec<_>>();

    let business_time_floor = positive_business_time_floor(business_time_scales.as_slice());

    let mean_business_time_scale =
        arithmetic_mean(business_time_scales.as_slice()).max(business_time_floor);

    let integrated_buy_intensities = business_time_scales
        .iter()
        .zip(optimizer_context.sell_share_proxies.iter())
        .map(|(business_time_scale, sell_share_proxy)| {
            business_time_scale * (1.0 - sell_share_proxy.clamp(0.0, 1.0))
        })
        .collect();

    let integrated_sell_intensities = business_time_scales
        .iter()
        .zip(optimizer_context.sell_share_proxies.iter())
        .map(|(business_time_scale, sell_share_proxy)| {
            business_time_scale * sell_share_proxy.clamp(0.0, 1.0)
        })
        .collect();

    // Cached optimizer contexts own their vectors so callers can share them
    // without borrowing the source context across objective evaluations.
    ContinuousReturnOptimizerContext {
        observed_returns: optimizer_context.observed_returns.clone(),
        business_time_scales,
        mean_business_time_scale,
        sell_share_proxies: optimizer_context.sell_share_proxies.clone(),
        member_weights: optimizer_context.member_weights.clone(),
        integrated_buy_intensities,
        integrated_sell_intensities,
        scale_bucket_profiles: optimizer_context.scale_bucket_profiles.clone(),
        observed_return_volatility: optimizer_context.observed_return_volatility,
        log_likelihood_shift: optimizer_context.log_likelihood_shift,
        open_session_sell_share_proxy: optimizer_context.open_session_sell_share_proxy,
        seed_member_indices: optimizer_context.seed_member_indices.clone(),
        local_member_indices: optimizer_context.local_member_indices.clone(),
        full_member_indices: optimizer_context.full_member_indices.clone(),
        cache: Arc::new(Mutex::new(BTreeMap::new())),
    }
}

pub(super) fn build_equal_business_time_blocks(
    continuous_fit_returns: &[ReturnComponentsObservedOpenSessionReturn],
    business_time_scales: &[f64],
    maximum_count: usize,
) -> Vec<(ReturnComponentsObservedOpenSessionReturn, f64)> {
    let business_time_floor = positive_business_time_floor(business_time_scales);

    let aligned_count = continuous_fit_returns.len().min(business_time_scales.len());

    let aligned_pairs: Vec<(ReturnComponentsObservedOpenSessionReturn, f64)> =
        continuous_fit_returns
            .iter()
            .copied()
            .zip(business_time_scales.iter().copied())
            .take(aligned_count)
            .collect();

    if aligned_pairs.is_empty() {
        return Vec::new();
    }

    if aligned_pairs.len() == 1 || aligned_pairs.len() <= maximum_count {
        return aligned_pairs;
    }

    let total_business_time: f64 = aligned_pairs
        .iter()
        .map(|(_, business_time_scale)| {
            if business_time_scale.is_finite() && *business_time_scale > 0.0 {
                *business_time_scale
            } else {
                0.0
            }
        })
        .sum();
    if total_business_time <= 0.0 {
        return aligned_pairs;
    }

    let target_block_count = maximum_count.min(aligned_pairs.len());
    if target_block_count <= 1 {
        let total_log_return = aligned_pairs
            .iter()
            .map(|(observation, _)| observation.log_return)
            .sum::<f64>();

        let total_elapsed_same_session_hours = aligned_pairs
            .iter()
            .map(|(observation, _)| observation.elapsed_same_session_hours)
            .sum::<f64>();

        let weighted_activity_lift = weighted_average(
            aligned_pairs
                .iter()
                .map(|(observation, business_time_scale)| {
                    (observation.post_shock_activity_lift, *business_time_scale)
                })
                .collect::<Vec<_>>()
                .as_slice(),
        );

        let weighted_sell_share = weighted_average(
            aligned_pairs
                .iter()
                .map(|(observation, business_time_scale)| {
                    (observation.sell_share_proxy, *business_time_scale)
                })
                .collect::<Vec<_>>()
                .as_slice(),
        );

        let last_observation = aligned_pairs
            .last()
            .map(|(observation, _)| *observation)
            .unwrap_or(continuous_fit_returns[0]);

        return vec![(
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: last_observation.return_timestamp_utc_nanos,
                elapsed_same_session_hours: total_elapsed_same_session_hours,
                log_return: total_log_return,
                post_shock_activity_lift: weighted_activity_lift,
                sell_share_proxy: weighted_sell_share,
            },
            total_business_time,
        )];
    }

    let mut aggregated_blocks: Vec<(ReturnComponentsObservedOpenSessionReturn, f64)> =
        Vec::with_capacity(target_block_count);

    let mut running_log_return = 0.0;
    let mut running_elapsed_same_session_hours = 0.0;
    let mut running_business_time = 0.0;
    let mut running_weighted_activity_lift = 0.0;
    let mut running_weighted_sell_share = 0.0;
    let mut running_weight_sum = 0.0;

    let mut emitted_block_count = 0usize;

    let mut cumulative_business_time = 0.0;

    for (aligned_index, (observation, business_time_scale)) in aligned_pairs.iter().enumerate() {
        let positive_business_time =
            if business_time_scale.is_finite() && *business_time_scale > 0.0 {
                *business_time_scale
            } else {
                0.0
            };

        running_log_return += observation.log_return;
        running_elapsed_same_session_hours += observation.elapsed_same_session_hours;
        running_business_time += positive_business_time;
        running_weighted_activity_lift +=
            observation.post_shock_activity_lift * positive_business_time;
        running_weighted_sell_share += observation.sell_share_proxy * positive_business_time;
        running_weight_sum += positive_business_time;
        cumulative_business_time += positive_business_time;

        let remaining_observation_count = aligned_pairs.len().saturating_sub(aligned_index + 1);
        let remaining_block_count = target_block_count.saturating_sub(emitted_block_count + 1);

        let should_emit = aligned_index + 1 == aligned_pairs.len()
            || remaining_observation_count == remaining_block_count
            || cumulative_business_time
                >= total_business_time * (emitted_block_count + 1) as f64
                    / target_block_count as f64;

        if !should_emit {
            continue;
        }

        let weight_denominator = running_weight_sum.max(1.0);

        aggregated_blocks.push((
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: observation.return_timestamp_utc_nanos,
                elapsed_same_session_hours: running_elapsed_same_session_hours,
                log_return: running_log_return,
                post_shock_activity_lift: running_weighted_activity_lift / weight_denominator,
                sell_share_proxy: (running_weighted_sell_share / weight_denominator)
                    .clamp(0.0, 1.0),
            },
            running_business_time.max(business_time_floor),
        ));

        emitted_block_count += 1;

        running_log_return = 0.0;
        running_elapsed_same_session_hours = 0.0;
        running_business_time = 0.0;
        running_weighted_activity_lift = 0.0;
        running_weighted_sell_share = 0.0;
        running_weight_sum = 0.0;
    }

    aggregated_blocks
}

fn weighted_average(values_and_weights: &[(f64, f64)]) -> f64 {
    let total_weight: f64 = values_and_weights
        .iter()
        .map(|(_, weight)| {
            if weight.is_finite() && *weight > 0.0 {
                *weight
            } else {
                0.0
            }
        })
        .sum();

    if total_weight <= 0.0 {
        return arithmetic_mean(
            values_and_weights
                .iter()
                .map(|(value, _)| *value)
                .collect::<Vec<_>>()
                .as_slice(),
        );
    }

    values_and_weights
        .iter()
        .map(|(value, weight)| {
            let finite_weight = if weight.is_finite() && *weight > 0.0 {
                *weight
            } else {
                0.0
            };
            value * finite_weight
        })
        .sum::<f64>()
        / total_weight
}

fn positive_business_time_floor(business_time_scales: &[f64]) -> f64 {
    smallest_positive_value(business_time_scales).unwrap_or_else(strictly_positive_machine_value)
}

fn log_likelihood_shift_from_observed_profile(
    observed_profile: super::types::ContinuousReturnDistributionProfile,
) -> f64 {
    -observed_profile
        .return_volatility
        .max(strictly_positive_machine_value())
        .ln()
}

fn fidelity_member_indices(
    total_count: usize,
    sell_share_proxies: &[f64],
    scale_bucket_profiles: &[super::types::ContinuousReturnScaleBucketProfile],
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> Vec<usize> {
    if fidelity_tier == ContinuousLikelihoodFidelityTier::Full
        || scale_bucket_profiles.is_empty()
        || total_count <= scale_bucket_profiles.len()
    {
        return (0..total_count).collect();
    }

    let sell_share_stratum_count = match fidelity_tier {
        ContinuousLikelihoodFidelityTier::Seed => 1,
        ContinuousLikelihoodFidelityTier::Representative => return (0..total_count).collect(),
        ContinuousLikelihoodFidelityTier::Local => scale_bucket_profiles.len(),
        ContinuousLikelihoodFidelityTier::Full => unreachable!("full tier handled above"),
    };

    let mut selected_member_indices = Vec::new();

    for scale_bucket_profile in scale_bucket_profiles {
        let mut bucket_member_indices = scale_bucket_profile.member_indices.clone();
        bucket_member_indices.sort_by(|left_member_index, right_member_index| {
            sell_share_proxies[*left_member_index]
                .total_cmp(&sell_share_proxies[*right_member_index])
                .then_with(|| left_member_index.cmp(right_member_index))
        });

        selected_member_indices.extend(representative_member_indices_from_sorted_strata(
            bucket_member_indices.as_slice(),
            sell_share_stratum_count,
        ));
    }

    selected_member_indices.sort_unstable();
    selected_member_indices.dedup();

    if selected_member_indices.is_empty() {
        return (0..total_count).collect();
    }

    selected_member_indices
}
