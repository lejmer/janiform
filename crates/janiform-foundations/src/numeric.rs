//! Shared numeric helpers.

mod bounds;
mod constants;
mod conversions;
mod domain;
mod l_moments;
mod logarithms;
mod ordering;
mod samples;

pub use bounds::{clamp_point, clamp_point_in_place};
pub use constants::{GOLDEN_RATIO, GOLDEN_RATIO_RECIPROCAL, square_root_machine_epsilon};
pub use conversions::{IntegerToF64ExactConversionError, usize_to_f64_exact};
pub use domain::{
    FiniteNumberValidationError, NegativeNumberValidationError, NonNegativeNumberValidationError,
    PositiveNumberValidationError, ceil_integer_square_root, clamp_to_open_unit_interval,
    clamp_to_unit_interval, coverage_ratio, integer_square_root, logistic, normalized_pseudocount,
    representable_positive_f64_log_span, round_positive_to_domain, round_to,
    saturating_positive_exp, strictly_positive_machine_value, support_information_depth,
    validate_finite_number, validate_negative_number, validate_non_negative_number,
    validate_positive_number,
};
pub use l_moments::{
    bowley_skewness_from_sorted, compute_probability_weighted_moment, l_moment_lambda2_from_sorted,
    l_moment_tau3_from_sorted,
};
pub use logarithms::{log_sum_exp, log_sum_exp_pair};
pub use ordering::{
    compute_finite_median_or_zero, compute_finite_quantile_or_zero, compute_median_f64_or_zero,
    compute_median_i64_or_zero, compute_sorted_median, compute_sorted_quantile,
    compute_sorted_quantile_or_zero, median_abs, median_abs_non_zero, smallest_positive_value,
    sorted_finite_values, unique_sorted_values,
};
pub use samples::{
    AdjustedShapeJackknifeStandardErrors, MeanEstimate, adjusted_shape_jackknife_standard_errors,
    arithmetic_mean, arithmetic_mean_or_zero, central_skewness_or_zero, coefficient_of_variation,
    finite_arithmetic_mean_or_zero, jackknife_standard_error_from_leave_one_out_values,
    mean_absolute_aligned_distance, mean_estimate_from_standard_errors, pearson_correlation,
    relative_rmse, rolling_sum, root_mean_square_or_infinity, sample_adjusted_excess_kurtosis,
    sample_adjusted_skewness, sample_standard_deviation, sample_variance,
    standardized_path_distance, winsorized_sample_standard_deviation_from_sorted, z_score,
};
