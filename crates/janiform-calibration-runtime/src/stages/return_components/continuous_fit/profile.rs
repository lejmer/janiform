use janiform_foundations::numeric::{
    arithmetic_mean_or_zero as arithmetic_mean, bowley_skewness_from_sorted,
    compute_sorted_quantile_or_zero as sorted_quantile, l_moment_lambda2_from_sorted,
    l_moment_tau3_from_sorted, root_mean_square_or_infinity, sample_adjusted_excess_kurtosis,
    sample_adjusted_skewness, sample_standard_deviation, smallest_positive_value,
    sorted_finite_values, strictly_positive_machine_value,
};

use super::super::contracts::ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic;
use super::super::execution::arithmetic_mean_signed;
use super::types::{ContinuousReturnDistributionProfile, ContinuousReturnScaleBucketProfile};

pub(crate) fn distribution_profile_fit_score(
    fitted_returns: &[f64],
    observed_returns: &[f64],
) -> ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
    distribution_profile_fit_breakdown(
        build_distribution_profile(fitted_returns),
        build_distribution_profile(observed_returns),
    )
}

pub(super) fn distribution_profile_fit_breakdown(
    fitted_profile: ContinuousReturnDistributionProfile,
    observed_profile: ContinuousReturnDistributionProfile,
) -> ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
    let central_scale = dominant_positive_scale(
        [
            (observed_profile.upper_quartile_return - observed_profile.lower_quartile_return).abs(),
            observed_profile.l_moment_lambda2.abs(),
        ]
        .as_slice(),
    );

    let tail_scale = dominant_positive_scale(
        [
            observed_profile.extreme_right_tail_return_q99.abs(),
            observed_profile.extreme_left_tail_return_q01.abs(),
            central_scale,
        ]
        .as_slice(),
    );

    let skew_shape_scale = dominant_positive_scale(
        [
            observed_profile.skewness_standard_error(),
            observed_profile.bowley_skewness.abs(),
            observed_profile.l_moment_tau3.abs(),
        ]
        .as_slice(),
    );

    let abs_quantile_ratio_scale = observed_profile.abs_quantile_ratio_q99_q95.abs().max(1.0);

    let mean_return_fit = scale_aware_absolute_error(
        fitted_profile.mean_return,
        observed_profile.mean_return,
        central_scale,
    );

    let volatility_scale = observed_profile.return_volatility.abs().max(central_scale);

    let volatility_fit = scale_aware_absolute_error(
        fitted_profile.return_volatility,
        observed_profile.return_volatility,
        volatility_scale,
    );

    let median_return_fit = scale_aware_absolute_error(
        fitted_profile.median_return,
        observed_profile.median_return,
        central_scale,
    );

    let lower_quartile_fit = scale_aware_absolute_error(
        fitted_profile.lower_quartile_return,
        observed_profile.lower_quartile_return,
        central_scale,
    );

    let upper_quartile_fit = scale_aware_absolute_error(
        fitted_profile.upper_quartile_return,
        observed_profile.upper_quartile_return,
        central_scale,
    );

    let lower_tail_q05_fit = scale_aware_absolute_error(
        fitted_profile.lower_tail_return_q05,
        observed_profile.lower_tail_return_q05,
        tail_scale,
    );

    let upper_tail_q95_fit = scale_aware_absolute_error(
        fitted_profile.upper_tail_return_q95,
        observed_profile.upper_tail_return_q95,
        tail_scale,
    );

    let extreme_left_tail_q01_fit = scale_aware_absolute_error(
        fitted_profile.extreme_left_tail_return_q01,
        observed_profile.extreme_left_tail_return_q01,
        tail_scale,
    );

    let extreme_right_tail_q99_fit = scale_aware_absolute_error(
        fitted_profile.extreme_right_tail_return_q99,
        observed_profile.extreme_right_tail_return_q99,
        tail_scale,
    );

    let mean_abs_return_fit = scale_aware_absolute_error(
        fitted_profile.mean_abs_return,
        observed_profile.mean_abs_return,
        central_scale,
    );

    let median_abs_return_fit = scale_aware_absolute_error(
        fitted_profile.median_abs_return,
        observed_profile.median_abs_return,
        central_scale,
    );

    let abs_quantile_ratio_fit = scale_aware_absolute_error(
        fitted_profile.abs_quantile_ratio_q99_q95,
        observed_profile.abs_quantile_ratio_q99_q95,
        abs_quantile_ratio_scale,
    );

    let bowley_skewness_fit = scale_aware_absolute_error(
        fitted_profile.bowley_skewness,
        observed_profile.bowley_skewness,
        skew_shape_scale,
    );

    let l_moment_lambda2_fit = scale_aware_absolute_error(
        fitted_profile.l_moment_lambda2,
        observed_profile.l_moment_lambda2,
        central_scale,
    );

    let l_moment_tau3_fit = scale_aware_absolute_error(
        fitted_profile.l_moment_tau3,
        observed_profile.l_moment_tau3,
        skew_shape_scale,
    );

    let location_fit = root_mean_square_or_infinity(
        [
            mean_return_fit,
            median_return_fit,
            lower_quartile_fit,
            upper_quartile_fit,
        ]
        .as_slice(),
    );

    let scale_fit = root_mean_square_or_infinity(
        [
            volatility_fit,
            mean_abs_return_fit,
            median_abs_return_fit,
            l_moment_lambda2_fit,
        ]
        .as_slice(),
    );

    let tail_fit = root_mean_square_or_infinity(
        [
            lower_tail_q05_fit,
            upper_tail_q95_fit,
            extreme_left_tail_q01_fit,
            extreme_right_tail_q99_fit,
            abs_quantile_ratio_fit,
        ]
        .as_slice(),
    );

    let asymmetry_fit =
        root_mean_square_or_infinity([bowley_skewness_fit, l_moment_tau3_fit].as_slice());

    let global_fit_score =
        root_mean_square_or_infinity([location_fit, scale_fit, tail_fit, asymmetry_fit].as_slice());

    ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
        global_fit_score,
        conditional_fit_score: 0.0,
        boundary_penalty: 0.0,
        profile_fit_score: global_fit_score,
        log_likelihood_shift: 0.0,
        optimizer_seed_objective: 0.0,
        optimizer_local_objective: 0.0,
        optimizer_final_objective: 0.0,
        optimizer_local_evaluation_count: 0,
        optimizer_stop_reason: "not_optimized".to_string(),
        optimizer_basin_diagnostics: Vec::new(),
        exact_likelihood_failures: Vec::new(),
        exact_closed_form_single_side_buy_block_count: 0,
        exact_closed_form_single_side_sell_block_count: 0,
        exact_tilted_mixed_inversion_block_count: 0,
        exact_structural_failure_count: 0,
        exact_uncertified_scope_count: 0,
        exact_global_average_log_likelihood_error_upper_bound: None,
        exact_conditional_average_log_likelihood_error_upper_bound: None,
        mean_return_fit,
        volatility_fit,
        median_return_fit,
        lower_quartile_fit,
        upper_quartile_fit,
        lower_tail_q05_fit,
        upper_tail_q95_fit,
        extreme_left_tail_q01_fit,
        extreme_right_tail_q99_fit,
        mean_abs_return_fit,
        median_abs_return_fit,
        abs_quantile_ratio_fit,
        bowley_skewness_fit,
        l_moment_lambda2_fit,
        l_moment_tau3_fit,
    }
}

pub(super) fn scale_aware_absolute_error(observed: f64, target: f64, scale_floor: f64) -> f64 {
    let denominator = scale_floor.abs().max(strictly_positive_machine_value());

    ((observed - target).abs() / denominator).max(0.0)
}

pub(super) fn build_scale_bucket_profiles(
    observed_returns: &[f64],
    business_time_scales: &[f64],
) -> Vec<ContinuousReturnScaleBucketProfile> {
    let aligned_count = observed_returns.len().min(business_time_scales.len());
    if aligned_count < 96 {
        return Vec::new();
    }

    let finite_scales: Vec<f64> = business_time_scales
        .iter()
        .copied()
        .take(aligned_count)
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect();
    if finite_scales.len() < 96 {
        return Vec::new();
    }

    let sorted_scales = sorted_finite_values(finite_scales.as_slice());
    let quantile_cutoffs = [
        sorted_quantile(sorted_scales.as_slice(), 0.25),
        sorted_quantile(sorted_scales.as_slice(), 0.50),
        sorted_quantile(sorted_scales.as_slice(), 0.75),
    ];

    let mut bucket_member_indices = vec![Vec::new(), Vec::new(), Vec::new(), Vec::new()];

    for (aligned_index, business_time_scale) in business_time_scales
        .iter()
        .copied()
        .enumerate()
        .take(aligned_count)
    {
        if !business_time_scale.is_finite() || business_time_scale <= 0.0 {
            continue;
        }

        let bucket_index = if business_time_scale <= quantile_cutoffs[0] {
            0
        } else if business_time_scale <= quantile_cutoffs[1] {
            1
        } else if business_time_scale <= quantile_cutoffs[2] {
            2
        } else {
            3
        };

        bucket_member_indices[bucket_index].push(aligned_index);
    }

    bucket_member_indices
        .into_iter()
        .filter_map(|member_indices| {
            if member_indices.is_empty() {
                return None;
            }

            Some(ContinuousReturnScaleBucketProfile { member_indices })
        })
        .collect()
}

pub(super) fn build_distribution_profile(returns: &[f64]) -> ContinuousReturnDistributionProfile {
    let sorted_returns = sorted_finite_values(returns);
    let mut sorted_absolute_returns: Vec<f64> =
        sorted_returns.iter().map(|value| value.abs()).collect();
    sorted_absolute_returns.sort_by(f64::total_cmp);

    let lower_quartile_return = sorted_quantile(sorted_returns.as_slice(), 0.25);
    let median_return = sorted_quantile(sorted_returns.as_slice(), 0.50);
    let upper_quartile_return = sorted_quantile(sorted_returns.as_slice(), 0.75);

    let lower_tail_return_q05 = sorted_quantile(sorted_returns.as_slice(), 0.05);
    let upper_tail_return_q95 = sorted_quantile(sorted_returns.as_slice(), 0.95);

    let extreme_left_tail_return_q01 = sorted_quantile(sorted_returns.as_slice(), 0.01);
    let extreme_right_tail_return_q99 = sorted_quantile(sorted_returns.as_slice(), 0.99);

    let quantile_95_abs = sorted_quantile(sorted_absolute_returns.as_slice(), 0.95);
    let quantile_99_abs = sorted_quantile(sorted_absolute_returns.as_slice(), 0.99);

    let shape_resolution_floor = distribution_shape_resolution_floor(
        sorted_returns.as_slice(),
        sorted_absolute_returns.as_slice(),
        lower_quartile_return,
        median_return,
        upper_quartile_return,
    );

    ContinuousReturnDistributionProfile {
        sample_count: sorted_returns.len(),
        mean_return: arithmetic_mean_signed(returns),
        return_volatility: sample_standard_deviation(returns),
        adjusted_skewness: sample_adjusted_skewness(returns).unwrap_or(0.0),
        adjusted_excess_kurtosis: sample_adjusted_excess_kurtosis(returns).unwrap_or(0.0),
        median_return,
        lower_quartile_return,
        upper_quartile_return,
        lower_tail_return_q05,
        upper_tail_return_q95,
        extreme_left_tail_return_q01,
        extreme_right_tail_return_q99,
        mean_abs_return: arithmetic_mean(sorted_absolute_returns.as_slice()),
        median_abs_return: sorted_quantile(sorted_absolute_returns.as_slice(), 0.50),
        abs_quantile_ratio_q99_q95: if quantile_95_abs <= 0.0 {
            1.0
        } else {
            quantile_99_abs / quantile_95_abs
        },
        bowley_skewness: bowley_skewness_from_sorted(
            sorted_returns.as_slice(),
            shape_resolution_floor,
        )
        .unwrap_or(0.0),
        l_moment_lambda2: l_moment_lambda2_from_sorted(sorted_returns.as_slice()).unwrap_or(0.0),
        l_moment_tau3: l_moment_tau3_from_sorted(sorted_returns.as_slice(), shape_resolution_floor)
            .unwrap_or(0.0),
    }
}

fn positive_scale_floor(values: &[f64]) -> f64 {
    smallest_positive_value(values).unwrap_or_else(strictly_positive_machine_value)
}

fn dominant_positive_scale(values: &[f64]) -> f64 {
    values
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .max_by(f64::total_cmp)
        .unwrap_or_else(strictly_positive_machine_value)
}

fn distribution_shape_resolution_floor(
    sorted_returns: &[f64],
    sorted_absolute_returns: &[f64],
    lower_quartile_return: f64,
    median_return: f64,
    upper_quartile_return: f64,
) -> f64 {
    let minimum_adjacent_return_gap = sorted_returns
        .windows(2)
        .filter_map(|return_pair| {
            let gap = (return_pair[1] - return_pair[0]).abs();
            (gap.is_finite() && gap > 0.0).then_some(gap)
        })
        .min_by(f64::total_cmp)
        .unwrap_or(0.0);

    let minimum_positive_absolute_return = sorted_absolute_returns
        .iter()
        .copied()
        .find(|absolute_return| *absolute_return > 0.0)
        .unwrap_or(0.0);

    positive_scale_floor(
        [
            minimum_adjacent_return_gap,
            minimum_positive_absolute_return,
            (upper_quartile_return - lower_quartile_return).abs(),
            median_return.abs(),
        ]
        .as_slice(),
    )
}
