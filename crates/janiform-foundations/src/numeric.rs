//! Shared numeric helpers for deterministic floating-point domain handling.

use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FiniteNumberValidationError {
    pub field: &'static str,
    pub value: f64,
}

impl Display for FiniteNumberValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} must be finite, found {}",
            self.field, self.value
        )
    }
}

impl Error for FiniteNumberValidationError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositiveNumberValidationError {
    pub field: &'static str,
    pub value: f64,
}

impl Display for PositiveNumberValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} must be finite and strictly positive, found {}",
            self.field, self.value
        )
    }
}

impl Error for PositiveNumberValidationError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NonNegativeNumberValidationError {
    pub field: &'static str,
    pub value: f64,
}

impl Display for NonNegativeNumberValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} must be finite and non-negative, found {}",
            self.field, self.value
        )
    }
}

impl Error for NonNegativeNumberValidationError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NegativeNumberValidationError {
    pub field: &'static str,
    pub value: f64,
}

impl Display for NegativeNumberValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} must be finite and strictly negative, found {}",
            self.field, self.value
        )
    }
}

impl Error for NegativeNumberValidationError {}

/// Return the smallest strictly positive normal `f64` value.
pub fn strictly_positive_machine_value() -> f64 {
    f64::MIN_POSITIVE
}

/// Return the widest finite log span representable by strictly positive `f64` values.
pub fn representable_positive_f64_log_span() -> f64 {
    f64::MAX.ln() - f64::MIN_POSITIVE.ln()
}

/// Validate that a numeric field is finite.
pub fn validate_finite_number(
    value: f64,
    field: &'static str,
) -> Result<(), FiniteNumberValidationError> {
    if !value.is_finite() {
        return Err(FiniteNumberValidationError { field, value });
    }

    Ok(())
}

/// Validate that a numeric field is finite and strictly positive.
pub fn validate_positive_number(
    value: f64,
    field: &'static str,
) -> Result<(), PositiveNumberValidationError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(PositiveNumberValidationError { field, value });
    }

    Ok(())
}

/// Validate that a numeric field is finite and non-negative.
pub fn validate_non_negative_number(
    value: f64,
    field: &'static str,
) -> Result<(), NonNegativeNumberValidationError> {
    if !value.is_finite() || value < 0.0 {
        return Err(NonNegativeNumberValidationError { field, value });
    }

    Ok(())
}

/// Validate that a numeric field is finite and strictly negative.
pub fn validate_negative_number(
    value: f64,
    field: &'static str,
) -> Result<(), NegativeNumberValidationError> {
    if !value.is_finite() || value >= 0.0 {
        return Err(NegativeNumberValidationError { field, value });
    }

    Ok(())
}

/// Clamp a finite value into the closed unit interval.
pub fn clamp_to_unit_interval(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// Clamp a finite value into the open unit interval using machine-adjacent bounds.
pub fn clamp_to_open_unit_interval(value: f64) -> f64 {
    value.clamp(0.0f64.next_up(), 1.0f64.next_down())
}

/// Compute the fraction of covered intervals within a horizon.
pub fn coverage_ratio(covered_interval_count: usize, horizon_interval_count: usize) -> f64 {
    if horizon_interval_count == 0 {
        return 0.0;
    }

    covered_interval_count as f64 / horizon_interval_count as f64
}

/// Compute the logistic sigmoid in a numerically stable piecewise form.
pub fn logistic(value: f64) -> f64 {
    if value >= 0.0 {
        let exp_neg = (-value).exp();
        1.0 / (1.0 + exp_neg)
    } else {
        let exp_pos = value.exp();
        exp_pos / (1.0 + exp_pos)
    }
}

/// Round to a fixed number of decimal places.
pub fn round_to(value: f64, digits: i32) -> f64 {
    let scale = 10f64.powi(digits);
    (value * scale).round() / scale
}

/// Compute the integer square root by flooring the real square root.
pub fn integer_square_root(value: usize) -> usize {
    (value as f64).sqrt().floor() as usize
}

/// Round while preserving a strictly positive domain after quantization.
pub fn round_positive_to_domain(value: f64, digits: i32) -> f64 {
    round_to(value.max(strictly_positive_machine_value()), digits)
        .max(strictly_positive_machine_value())
}

/// Return sorted unique values while preserving caller-supplied domain adjustments.
pub fn unique_sorted_values(values: &[f64]) -> Vec<f64> {
    let mut sorted_values = values.to_vec();

    sorted_values.sort_by(f64::total_cmp);
    sorted_values.dedup_by(|left_value, right_value| *left_value == *right_value);

    sorted_values
}

/// Return finite values sorted in ascending order.
pub fn sorted_finite_values(values: &[f64]) -> Vec<f64> {
    let mut sorted_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();

    sorted_values.sort_by(f64::total_cmp);

    sorted_values
}

/// Return the smallest finite value that is strictly greater than zero.
pub fn smallest_positive_value(values: &[f64]) -> Option<f64> {
    values
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .min_by(f64::total_cmp)
}

/// Compute the arithmetic mean of a finite numeric sample.
pub fn arithmetic_mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    let sum: f64 = values.iter().copied().sum();
    if !sum.is_finite() {
        return None;
    }

    Some(sum / values.len() as f64)
}

/// Compute the arithmetic mean and fall back to zero for empty or invalid samples.
pub fn arithmetic_mean_or_zero(values: &[f64]) -> f64 {
    arithmetic_mean(values).unwrap_or(0.0)
}

/// Compute the arithmetic mean after discarding non-finite observations.
pub fn finite_arithmetic_mean_or_zero(values: &[f64]) -> f64 {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();

    arithmetic_mean_or_zero(finite_values.as_slice())
}

/// Compute the rolling sum over a trailing window.
pub fn rolling_sum(values: &[f64], window: usize) -> Vec<f64> {
    if window == 0 {
        return vec![0.0; values.len()];
    }

    let mut output = Vec::with_capacity(values.len());
    let mut running_sum = 0.0;

    for (position, value) in values.iter().enumerate() {
        running_sum += *value;

        if position >= window {
            running_sum -= values[position - window];
        }

        output.push(running_sum);
    }

    output
}

/// Compute the root mean square over finite values and return infinity when none are finite.
pub fn root_mean_square_or_infinity(values: &[f64]) -> f64 {
    let mut squared_sum = 0.0;
    let mut finite_value_count = 0usize;

    for value in values {
        if !value.is_finite() {
            continue;
        }

        squared_sum += value * value;
        finite_value_count += 1;
    }

    if finite_value_count == 0 {
        return f64::INFINITY;
    }

    (squared_sum / finite_value_count as f64).sqrt()
}

/// Compute the root mean square error relative to the scale of the reference sample.
pub fn relative_rmse(left: &[f64], right: &[f64], floor: f64) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return f64::INFINITY;
    }

    let squared_error_sum = left
        .iter()
        .zip(right.iter())
        .map(|(left_value, right_value)| {
            let error = left_value - right_value;
            error * error
        })
        .sum::<f64>();

    let rmse = (squared_error_sum / left.len() as f64).sqrt();
    let scale = arithmetic_mean_or_zero(right).abs().max(floor);

    rmse / scale
}

/// Compute the sample standard deviation after discarding non-finite observations.
pub fn sample_standard_deviation(values: &[f64]) -> f64 {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if finite_values.len() < 2 {
        return finite_values.first().copied().unwrap_or(0.0).abs();
    }

    let sample_mean = finite_arithmetic_mean_or_zero(finite_values.as_slice());
    let sum_squared_deviation = finite_values
        .iter()
        .map(|value| {
            let centered_value = *value - sample_mean;
            centered_value * centered_value
        })
        .sum::<f64>();

    (sum_squared_deviation / (finite_values.len() - 1) as f64)
        .max(0.0)
        .sqrt()
}

/// Compute the sample variance after discarding non-finite observations.
pub fn sample_variance(values: &[f64]) -> Option<f64> {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if finite_values.len() < 2 {
        return None;
    }

    let sample_mean = finite_arithmetic_mean_or_zero(finite_values.as_slice());
    let sum_squared_deviation = finite_values
        .iter()
        .map(|value| {
            let centered_value = *value - sample_mean;
            centered_value * centered_value
        })
        .sum::<f64>();

    let variance = sum_squared_deviation / (finite_values.len() - 1) as f64;
    variance.is_finite().then_some(variance)
}

/// Compute the standardized third central moment after discarding non-finite observations.
pub fn central_skewness_or_zero(values: &[f64], minimum_scale: f64) -> f64 {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if finite_values.len() < 2 {
        return 0.0;
    }

    let mean = finite_arithmetic_mean_or_zero(finite_values.as_slice());
    let centered_values = finite_values
        .iter()
        .map(|value| *value - mean)
        .collect::<Vec<_>>();
    let variance = centered_values
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        / centered_values.len() as f64;

    let standard_deviation = variance.sqrt();
    if standard_deviation <= minimum_scale {
        return 0.0;
    }

    centered_values
        .iter()
        .map(|value| (value / standard_deviation).powi(3))
        .sum::<f64>()
        / centered_values.len() as f64
}

/// Compute the adjusted sample skewness after discarding non-finite observations.
pub fn sample_adjusted_skewness(values: &[f64]) -> Option<f64> {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    let sample_count = finite_values.len();
    if sample_count < 3 {
        return None;
    }

    let sample_mean = finite_arithmetic_mean_or_zero(finite_values.as_slice());
    let (sum_squared_deviation, sum_cubed_deviation) =
        finite_values
            .iter()
            .fold((0.0, 0.0), |(running_sum2, running_sum3), value| {
                let centered_value = *value - sample_mean;
                (
                    running_sum2 + centered_value * centered_value,
                    running_sum3 + centered_value * centered_value * centered_value,
                )
            });
    let variance_population = sum_squared_deviation / sample_count as f64;
    if !variance_population.is_finite() || variance_population <= 1.0e-12 {
        return Some(0.0);
    }

    let unadjusted_skewness =
        (sum_cubed_deviation / sample_count as f64) / variance_population.powf(1.5);
    if !unadjusted_skewness.is_finite() {
        return None;
    }

    let adjustment =
        ((sample_count * (sample_count - 1)) as f64).sqrt() / (sample_count - 2) as f64;

    Some(adjustment * unadjusted_skewness)
}

/// Compute the adjusted sample excess kurtosis after discarding non-finite observations.
pub fn sample_adjusted_excess_kurtosis(values: &[f64]) -> Option<f64> {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();

    let sample_count = finite_values.len();
    if sample_count < 4 {
        return None;
    }

    let sample_mean = finite_arithmetic_mean_or_zero(finite_values.as_slice());

    let (sum_squared_deviation, sum_fourth_deviation) =
        finite_values
            .iter()
            .fold((0.0, 0.0), |(running_sum2, running_sum4), value| {
                let centered_value = *value - sample_mean;
                let centered_square = centered_value * centered_value;
                (
                    running_sum2 + centered_square,
                    running_sum4 + centered_square * centered_square,
                )
            });

    let variance_population = sum_squared_deviation / sample_count as f64;
    if !variance_population.is_finite() || variance_population <= 1.0e-12 {
        return Some(0.0);
    }

    let unadjusted_excess_kurtosis = (sum_fourth_deviation / sample_count as f64)
        / (variance_population * variance_population)
        - 3.0;
    if !unadjusted_excess_kurtosis.is_finite() {
        return None;
    }

    let n = sample_count as f64;
    Some(((n - 1.0) / ((n - 2.0) * (n - 3.0))) * ((n + 1.0) * unadjusted_excess_kurtosis + 6.0))
}

/// Compute an interpolated quantile from an already sorted sample.
pub fn compute_sorted_quantile(sorted_values: &[f64], quantile_level: f64) -> f64 {
    if sorted_values.is_empty() {
        return f64::NAN;
    }
    if sorted_values.len() == 1 {
        return sorted_values[0];
    }

    let clamped_quantile = quantile_level.clamp(0.0, 1.0);
    let quantile_position = clamped_quantile * (sorted_values.len() - 1) as f64;
    let lower_position = quantile_position.floor() as usize;
    let upper_position = quantile_position.ceil() as usize;

    let interpolation_weight = quantile_position - lower_position as f64;

    let lower_value = sorted_values[lower_position];
    let upper_value = sorted_values[upper_position];

    lower_value + interpolation_weight * (upper_value - lower_value)
}

/// Compute an interpolated quantile from an already sorted sample and use zero for empty input.
pub fn compute_sorted_quantile_or_zero(sorted_values: &[f64], quantile_level: f64) -> f64 {
    let quantile = compute_sorted_quantile(sorted_values, quantile_level);
    if quantile.is_nan() { 0.0 } else { quantile }
}

/// Compute the median from an already sorted sample.
pub fn compute_sorted_median(sorted_values: &[f64]) -> f64 {
    compute_sorted_quantile(sorted_values, 0.5)
}

/// Compute an interpolated quantile after filtering non-finite values and sorting the sample.
pub fn compute_finite_quantile_or_zero(values: &[f64], quantile_level: f64) -> f64 {
    let mut finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if finite_values.is_empty() {
        return 0.0;
    }

    finite_values.sort_by(f64::total_cmp);

    compute_sorted_quantile_or_zero(finite_values.as_slice(), quantile_level)
}

/// Compute the median after filtering non-finite values and sorting the sample.
pub fn compute_finite_median_or_zero(values: &[f64]) -> f64 {
    compute_finite_quantile_or_zero(values, 0.5)
}

/// Compute a probability-weighted moment for a sorted sample.
pub fn compute_probability_weighted_moment(sorted_values: &[f64], order: usize) -> Option<f64> {
    let sample_count = sorted_values.len();
    if sample_count <= order {
        return None;
    }

    let denominator_index = sample_count - 1;

    let mut weighted_sum = 0.0;
    for (value_position, value) in sorted_values.iter().enumerate() {
        if value_position < order {
            continue;
        }

        let weight = combination_ratio(value_position, denominator_index, order)?;
        weighted_sum += weight * *value;
    }

    let probability_weighted_moment = weighted_sum / sample_count as f64;

    probability_weighted_moment
        .is_finite()
        .then_some(probability_weighted_moment)
}

/// Compute Bowley skewness from a sorted finite sample.
pub fn bowley_skewness_from_sorted(
    sorted_values: &[f64],
    minimum_denominator_magnitude: f64,
) -> Option<f64> {
    if sorted_values.len() < 3 {
        return None;
    }

    let lower_quartile = compute_sorted_quantile(sorted_values, 0.25);
    let median = compute_sorted_quantile(sorted_values, 0.50);
    let upper_quartile = compute_sorted_quantile(sorted_values, 0.75);

    let denominator = (upper_quartile - lower_quartile).abs();
    if denominator <= minimum_denominator_magnitude {
        return None;
    }

    let value = (upper_quartile + lower_quartile - 2.0 * median) / denominator;
    value.is_finite().then_some(value)
}

/// Compute the second L-moment from a sorted finite sample.
pub fn l_moment_lambda2_from_sorted(sorted_values: &[f64]) -> Option<f64> {
    let probability_weighted_moment_0 = compute_probability_weighted_moment(sorted_values, 0)?;
    let probability_weighted_moment_1 = compute_probability_weighted_moment(sorted_values, 1)?;

    let lambda2 = 2.0 * probability_weighted_moment_1 - probability_weighted_moment_0;
    lambda2.is_finite().then_some(lambda2)
}

/// Compute the third L-moment ratio from a sorted finite sample.
pub fn l_moment_tau3_from_sorted(
    sorted_values: &[f64],
    minimum_denominator_magnitude: f64,
) -> Option<f64> {
    let probability_weighted_moment_0 = compute_probability_weighted_moment(sorted_values, 0)?;
    let probability_weighted_moment_1 = compute_probability_weighted_moment(sorted_values, 1)?;
    let probability_weighted_moment_2 = compute_probability_weighted_moment(sorted_values, 2)?;

    let lambda2 = 2.0 * probability_weighted_moment_1 - probability_weighted_moment_0;
    if !lambda2.is_finite() || lambda2.abs() <= minimum_denominator_magnitude {
        return None;
    }

    let lambda3 = 6.0 * probability_weighted_moment_2 - 6.0 * probability_weighted_moment_1
        + probability_weighted_moment_0;
    if !lambda3.is_finite() {
        return None;
    }

    let tau3 = lambda3 / lambda2;
    tau3.is_finite().then_some(tau3)
}

/// Compute the coefficient of variation for a non-negative sample.
pub fn coefficient_of_variation(values: &[f64]) -> f64 {
    let Some(mean_value) = arithmetic_mean(values) else {
        return 0.0;
    };
    if mean_value <= 0.0 {
        return 0.0;
    }

    let variance = values
        .iter()
        .map(|value| {
            let centered = *value - mean_value;
            centered * centered
        })
        .sum::<f64>()
        / values.len() as f64;

    variance.sqrt() / mean_value
}

/// Compute Pearson correlation for two aligned series.
pub fn pearson_correlation(left: &[f64], right: &[f64]) -> f64 {
    if left.len() != right.len() || left.len() < 2 {
        return f64::NAN;
    }

    let left_mean = arithmetic_mean_or_zero(left);
    let right_mean = arithmetic_mean_or_zero(right);

    let mut covariance = 0.0;
    let mut left_variance = 0.0;
    let mut right_variance = 0.0;

    for (left_value, right_value) in left.iter().zip(right.iter()) {
        let left_centered = left_value - left_mean;
        let right_centered = right_value - right_mean;

        covariance += left_centered * right_centered;
        left_variance += left_centered * left_centered;
        right_variance += right_centered * right_centered;
    }

    let denominator = (left_variance * right_variance).sqrt();
    if denominator <= f64::EPSILON {
        return 0.0;
    }

    covariance / denominator
}

/// Compute z-scores for one series, returning zeros when the scale collapses.
pub fn z_score(values: &[f64]) -> Vec<f64> {
    let mean_value = arithmetic_mean_or_zero(values);
    let variance = values
        .iter()
        .map(|value| {
            let centered = value - mean_value;
            centered * centered
        })
        .sum::<f64>()
        / values.len().max(1) as f64;

    let standard_deviation = variance.sqrt();
    if !standard_deviation.is_finite() || standard_deviation <= f64::EPSILON {
        return vec![0.0; values.len()];
    }

    values
        .iter()
        .map(|value| (value - mean_value) / standard_deviation)
        .collect()
}

/// Compute mean absolute z-score path error between two aligned series.
pub fn standardized_path_distance(left: &[f64], right: &[f64]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return f64::INFINITY;
    }

    let left_standardized = z_score(left);
    let right_standardized = z_score(right);

    left_standardized
        .iter()
        .zip(right_standardized.iter())
        .map(|(left_value, right_value)| (left_value - right_value).abs())
        .sum::<f64>()
        / left_standardized.len() as f64
}

/// Compute the median absolute value of a finite sample.
pub fn median_abs(values: &[f64]) -> f64 {
    let mut retained_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(f64::abs)
        .collect();
    if retained_values.is_empty() {
        return 0.0;
    }

    retained_values.sort_by(f64::total_cmp);
    compute_sorted_median(retained_values.as_slice())
}

/// Compute the median absolute value of strictly non-zero finite observations.
pub fn median_abs_non_zero(values: &[f64]) -> Option<f64> {
    let mut retained_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value != 0.0)
        .map(f64::abs)
        .collect();
    if retained_values.is_empty() {
        return None;
    }

    retained_values.sort_by(f64::total_cmp);
    Some(compute_sorted_median(retained_values.as_slice()))
}

/// Use a data-normalized smoothing pseudocount rather than a fixed constant.
pub fn normalized_pseudocount(event_count: usize) -> f64 {
    1.0 / event_count.max(1) as f64
}

fn combination_ratio(
    numerator_index: usize,
    denominator_index: usize,
    order: usize,
) -> Option<f64> {
    if order == 0 {
        return Some(1.0);
    }

    if numerator_index < order || denominator_index < order {
        return Some(0.0);
    }

    let mut ratio = 1.0;
    for order_position in 0..order {
        let numerator = numerator_index.saturating_sub(order_position) as f64;
        let denominator = denominator_index.saturating_sub(order_position) as f64;
        if denominator <= 0.0 {
            return None;
        }

        ratio *= numerator / denominator;
    }

    ratio.is_finite().then_some(ratio)
}

#[cfg(test)]
mod tests {
    use super::{
        arithmetic_mean, arithmetic_mean_or_zero, bowley_skewness_from_sorted,
        central_skewness_or_zero, clamp_to_open_unit_interval, clamp_to_unit_interval,
        coefficient_of_variation, compute_finite_median_or_zero, compute_finite_quantile_or_zero,
        compute_probability_weighted_moment, compute_sorted_median, compute_sorted_quantile,
        compute_sorted_quantile_or_zero, coverage_ratio, finite_arithmetic_mean_or_zero,
        integer_square_root, l_moment_lambda2_from_sorted, l_moment_tau3_from_sorted, logistic,
        median_abs, median_abs_non_zero, normalized_pseudocount, pearson_correlation,
        relative_rmse, representable_positive_f64_log_span, rolling_sum,
        root_mean_square_or_infinity, round_positive_to_domain, round_to,
        sample_adjusted_excess_kurtosis, sample_adjusted_skewness, sample_standard_deviation,
        sample_variance, smallest_positive_value, sorted_finite_values, standardized_path_distance,
        strictly_positive_machine_value, unique_sorted_values, validate_finite_number,
        validate_negative_number, validate_non_negative_number, validate_positive_number, z_score,
    };

    #[test]
    fn open_unit_interval_clamp_preserves_strict_bounds() {
        let lower = clamp_to_open_unit_interval(0.0);
        let upper = clamp_to_open_unit_interval(1.0);

        assert!(lower > 0.0);
        assert!(upper < 1.0);
    }

    #[test]
    fn positive_rounding_preserves_domain_after_quantization() {
        let rounded = round_positive_to_domain(1.0e-300, 12);

        assert!(rounded >= strictly_positive_machine_value());
    }

    #[test]
    fn helper_outputs_are_deterministic() {
        assert_eq!(clamp_to_unit_interval(-1.0), 0.0);
        assert_eq!(arithmetic_mean(&[1.0, 2.0, 3.0]), Some(2.0));
        assert_eq!(arithmetic_mean_or_zero(&[]), 0.0);
        assert_eq!(finite_arithmetic_mean_or_zero(&[1.0, f64::NAN, 3.0]), 2.0);
        assert_eq!(rolling_sum(&[1.0, 2.0, 3.0], 2), vec![1.0, 3.0, 5.0]);
        assert_eq!(rolling_sum(&[1.0, 2.0], 0), vec![0.0, 0.0]);
        assert_eq!(coverage_ratio(2, 5), 0.4);
        assert_eq!(coverage_ratio(0, 0), 0.0);
        assert_eq!(validate_finite_number(1.0, "field"), Ok(()));
        let error = validate_finite_number(f64::NAN, "field")
            .expect_err("non-finite values must fail validation");
        assert_eq!(error.field, "field");
        assert!(error.value.is_nan());
        assert_eq!(validate_positive_number(1.0, "field"), Ok(()));
        let error = validate_positive_number(0.0, "field")
            .expect_err("non-positive values must fail validation");
        assert_eq!(error.field, "field");
        assert_eq!(error.value, 0.0);
        assert_eq!(validate_non_negative_number(0.0, "field"), Ok(()));
        let error = validate_non_negative_number(-1.0, "field")
            .expect_err("negative values must fail validation");
        assert_eq!(error.field, "field");
        assert_eq!(error.value, -1.0);
        assert_eq!(validate_negative_number(-1.0, "field"), Ok(()));
        let error = validate_negative_number(0.0, "field")
            .expect_err("non-negative values must fail validation");
        assert_eq!(error.field, "field");
        assert_eq!(error.value, 0.0);
        assert_eq!(coefficient_of_variation(&[2.0, 2.0, 2.0]), 0.0);
        assert_eq!(median_abs(&[-3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median_abs_non_zero(&[0.0, -3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median_abs_non_zero(&[0.0, 0.0]), None);
        assert_eq!(round_to(1.23456, 3), 1.235);
        assert_eq!(integer_square_root(15), 3);
        assert_eq!(integer_square_root(16), 4);
        assert_eq!(
            sorted_finite_values(&[3.0, f64::NAN, 1.0, 2.0]),
            vec![1.0, 2.0, 3.0]
        );
        assert_eq!(smallest_positive_value(&[-1.0, 2.0, 0.0, 1.0]), Some(1.0));
        assert_eq!(unique_sorted_values(&[2.0, 1.0, 2.0]), vec![1.0, 2.0]);
        assert_eq!(normalized_pseudocount(4), 0.25);
        assert_eq!(logistic(0.0), 0.5);
        assert_eq!(
            representable_positive_f64_log_span(),
            f64::MAX.ln() - f64::MIN_POSITIVE.ln()
        );
        assert_eq!(compute_sorted_median(&[1.0, 2.0, 4.0]), 2.0);
        assert_eq!(compute_sorted_median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(compute_sorted_quantile(&[1.0, 2.0, 4.0], 0.5), 2.0);
        assert_eq!(compute_sorted_quantile_or_zero(&[], 0.5), 0.0);
        assert_eq!(compute_finite_median_or_zero(&[1.0, f64::NAN, 3.0]), 2.0);
        assert_eq!(
            compute_finite_quantile_or_zero(&[1.0, f64::NAN, 3.0], 0.5),
            2.0
        );
        assert_eq!(pearson_correlation(&[1.0, 2.0], &[2.0, 4.0]), 1.0);
        assert!((root_mean_square_or_infinity(&[3.0, 4.0]) - (12.5f64).sqrt()).abs() <= 1.0e-12);
        assert_eq!(z_score(&[2.0, 2.0]), vec![0.0, 0.0]);
        assert_eq!(
            standardized_path_distance(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]),
            0.0
        );
        assert!((relative_rmse(&[2.0, 4.0], &[1.0, 3.0], 1.0e-8) - 0.5).abs() <= 1.0e-12);
        assert!((sample_standard_deviation(&[1.0, 2.0, 3.0]) - 1.0).abs() <= 1.0e-12);
        assert_eq!(sample_variance(&[1.0, 2.0, 3.0]), Some(1.0));
        assert_eq!(
            bowley_skewness_from_sorted(&[1.0, 2.0, 3.0, 4.0, 5.0], 1.0e-12),
            Some(0.0)
        );
        assert_eq!(
            central_skewness_or_zero(&[1.0, 2.0, 3.0, 4.0, 5.0], 1.0e-12),
            0.0
        );
        assert!(
            (l_moment_lambda2_from_sorted(&[1.0, 2.0, 3.0, 4.0]).unwrap_or(f64::NAN) - (5.0 / 6.0))
                .abs()
                <= 1.0e-12
        );
        assert_eq!(
            l_moment_tau3_from_sorted(&[1.0, 2.0, 3.0, 4.0], 1.0e-12),
            Some(0.0)
        );
        assert_eq!(sample_adjusted_skewness(&[1.0, 2.0]), None);
        assert_eq!(sample_adjusted_excess_kurtosis(&[1.0, 2.0, 3.0]), None);
    }

    #[test]
    fn probability_weighted_moment_matches_expected_orders() {
        let sorted_values = [1.0, 2.0, 3.0, 4.0];

        let beta0 = compute_probability_weighted_moment(&sorted_values, 0)
            .expect("order zero should exist");
        let beta1 =
            compute_probability_weighted_moment(&sorted_values, 1).expect("order one should exist");
        let beta2 =
            compute_probability_weighted_moment(&sorted_values, 2).expect("order two should exist");

        assert!((beta0 - 2.5).abs() < 1.0e-12);
        assert!((beta1 - (5.0 / 3.0)).abs() < 1.0e-12);
        assert!((beta2 - 1.25).abs() < 1.0e-12);
    }

    #[test]
    fn probability_weighted_moment_requires_sample_larger_than_order() {
        assert_eq!(compute_probability_weighted_moment(&[1.0, 2.0], 2), None);
    }
}
