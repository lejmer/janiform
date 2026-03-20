//! Shared numeric helpers for deterministic floating-point domain handling.

/// Return the smallest strictly positive normal `f64` value.
pub fn strictly_positive_machine_value() -> f64 {
    f64::MIN_POSITIVE
}

/// Clamp a finite value into the closed unit interval.
pub fn clamp_to_unit_interval(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// Clamp a finite value into the open unit interval using machine-adjacent bounds.
pub fn clamp_to_open_unit_interval(value: f64) -> f64 {
    value.clamp(0.0f64.next_up(), 1.0f64.next_down())
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

    let midpoint = retained_values.len() / 2;
    if retained_values.len() % 2 == 0 {
        (retained_values[midpoint - 1] + retained_values[midpoint]) / 2.0
    } else {
        retained_values[midpoint]
    }
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

    let midpoint = retained_values.len() / 2;
    Some(if retained_values.len() % 2 == 0 {
        (retained_values[midpoint - 1] + retained_values[midpoint]) / 2.0
    } else {
        retained_values[midpoint]
    })
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
        arithmetic_mean, arithmetic_mean_or_zero, clamp_to_open_unit_interval,
        clamp_to_unit_interval, coefficient_of_variation, compute_finite_quantile_or_zero,
        compute_probability_weighted_moment, compute_sorted_quantile,
        compute_sorted_quantile_or_zero, logistic, median_abs, median_abs_non_zero,
        normalized_pseudocount, pearson_correlation, round_positive_to_domain, round_to,
        smallest_positive_value, standardized_path_distance, strictly_positive_machine_value,
        unique_sorted_values, z_score,
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
        assert_eq!(coefficient_of_variation(&[2.0, 2.0, 2.0]), 0.0);
        assert_eq!(median_abs(&[-3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median_abs_non_zero(&[0.0, -3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median_abs_non_zero(&[0.0, 0.0]), None);
        assert_eq!(round_to(1.23456, 3), 1.235);
        assert_eq!(smallest_positive_value(&[-1.0, 2.0, 0.0, 1.0]), Some(1.0));
        assert_eq!(unique_sorted_values(&[2.0, 1.0, 2.0]), vec![1.0, 2.0]);
        assert_eq!(normalized_pseudocount(4), 0.25);
        assert_eq!(logistic(0.0), 0.5);
        assert_eq!(compute_sorted_quantile(&[1.0, 2.0, 4.0], 0.5), 2.0);
        assert_eq!(compute_sorted_quantile_or_zero(&[], 0.5), 0.0);
        assert_eq!(
            compute_finite_quantile_or_zero(&[1.0, f64::NAN, 3.0], 0.5),
            2.0
        );
        assert_eq!(pearson_correlation(&[1.0, 2.0], &[2.0, 4.0]), 1.0);
        assert_eq!(z_score(&[2.0, 2.0]), vec![0.0, 0.0]);
        assert_eq!(
            standardized_path_distance(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]),
            0.0
        );
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
