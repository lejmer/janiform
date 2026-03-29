use janiform_foundations::numeric::{
    arithmetic_mean_or_zero as arithmetic_mean, bowley_skewness_from_sorted,
    compute_sorted_quantile_or_zero as sorted_quantile, l_moment_lambda2_from_sorted,
    l_moment_tau3_from_sorted, root_mean_square_or_infinity, sample_adjusted_excess_kurtosis,
    sample_adjusted_skewness, sample_standard_deviation, sorted_finite_values,
};

use super::super::constants::RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER;
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
    let central_scale = (observed_profile.upper_quartile_return
        - observed_profile.lower_quartile_return)
        .abs()
        .max(observed_profile.l_moment_lambda2.abs())
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let tail_scale = observed_profile
        .extreme_right_tail_return_q99
        .abs()
        .max(observed_profile.extreme_left_tail_return_q01.abs())
        .max(central_scale);
    let skew_shape_scale = observed_profile
        .skewness_standard_error()
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let abs_quantile_ratio_scale = observed_profile.abs_quantile_ratio_q99_q95.abs().max(1.0);

    let mean_return_fit = scale_aware_absolute_error(
        fitted_profile.mean_return,
        observed_profile.mean_return,
        central_scale,
    );
    let volatility_scale = observed_profile
        .return_volatility
        .abs()
        .max(central_scale)
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
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
    let denominator = scale_floor
        .abs()
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);

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

    for aligned_index in 0..aligned_count {
        let business_time_scale = business_time_scales[aligned_index];
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
            if member_indices.len() < 24 {
                return None;
            }

            Some(ContinuousReturnScaleBucketProfile { member_indices })
        })
        .collect()
}

pub(super) fn build_distribution_profile(returns: &[f64]) -> ContinuousReturnDistributionProfile {
    let absolute_returns: Vec<f64> = returns.iter().map(|value| value.abs()).collect();
    let sorted_returns = sorted_finite_values(returns);
    let lower_quartile_return = sorted_quantile(sorted_returns.as_slice(), 0.25);
    let median_return = sorted_quantile(sorted_returns.as_slice(), 0.50);
    let upper_quartile_return = sorted_quantile(sorted_returns.as_slice(), 0.75);
    let lower_tail_return_q05 = sorted_quantile(sorted_returns.as_slice(), 0.05);
    let upper_tail_return_q95 = sorted_quantile(sorted_returns.as_slice(), 0.95);
    let extreme_left_tail_return_q01 = sorted_quantile(sorted_returns.as_slice(), 0.01);
    let extreme_right_tail_return_q99 = sorted_quantile(sorted_returns.as_slice(), 0.99);
    let sorted_absolute_returns = sorted_finite_values(absolute_returns.as_slice());
    let quantile_95_abs = sorted_quantile(sorted_absolute_returns.as_slice(), 0.95);
    let quantile_99_abs = sorted_quantile(sorted_absolute_returns.as_slice(), 0.99);

    ContinuousReturnDistributionProfile {
        sample_count: returns.iter().filter(|value| value.is_finite()).count(),
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
        mean_abs_return: arithmetic_mean(absolute_returns.as_slice()),
        median_abs_return: sorted_quantile(sorted_absolute_returns.as_slice(), 0.50),
        abs_quantile_ratio_q99_q95: if quantile_95_abs.abs()
            <= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER
        {
            1.0
        } else {
            quantile_99_abs / quantile_95_abs
        },
        bowley_skewness: bowley_skewness_from_sorted(
            sorted_returns.as_slice(),
            RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER,
        )
        .unwrap_or(0.0),
        l_moment_lambda2: l_moment_lambda2_from_sorted(sorted_returns.as_slice()).unwrap_or(0.0),
        l_moment_tau3: l_moment_tau3_from_sorted(
            sorted_returns.as_slice(),
            RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER,
        )
        .unwrap_or(0.0),
    }
}
