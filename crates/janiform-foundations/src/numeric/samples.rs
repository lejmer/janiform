use super::ordering::compute_sorted_quantile_or_zero;

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

/// Compute the root-mean-square over finite values and return infinity when none are finite.
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

/// Compute the root-mean-square error relative to the scale of the reference sample.
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

/// Compute sample standard deviation after clamping sorted observations to quantile bounds.
pub fn winsorized_sample_standard_deviation_from_sorted(
    sorted_values: &[f64],
    lower_quantile: f64,
    upper_quantile: f64,
) -> f64 {
    if sorted_values.is_empty() {
        return f64::NAN;
    }

    let lower_bound = compute_sorted_quantile_or_zero(sorted_values, lower_quantile);
    let upper_bound = compute_sorted_quantile_or_zero(sorted_values, upper_quantile);

    if lower_bound > upper_bound {
        return f64::NAN;
    }

    let winsorized_values = sorted_values
        .iter()
        .copied()
        .map(|value| value.clamp(lower_bound, upper_bound))
        .collect::<Vec<_>>();

    sample_standard_deviation(winsorized_values.as_slice())
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

/// Estimated mean and standard error for aligned independent estimates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeanEstimate {
    pub value: f64,
    pub standard_error: f64,
}

/// Pool independent estimate standard errors and between-estimate dispersion.
pub fn mean_estimate_from_standard_errors(
    values: &[f64],
    standard_errors: &[f64],
) -> Option<MeanEstimate> {
    if values.is_empty() || values.len() != standard_errors.len() {
        return None;
    }

    if values.iter().any(|value| !value.is_finite())
        || standard_errors
            .iter()
            .any(|standard_error| !standard_error.is_finite() || *standard_error < 0.0)
    {
        return None;
    }

    let standard_error_variance = standard_errors
        .iter()
        .map(|standard_error| standard_error * standard_error)
        .sum::<f64>()
        / standard_errors.len() as f64;

    let between_estimate_variance = if values.len() > 1 {
        sample_variance(values).unwrap_or(0.0) / values.len() as f64
    } else {
        0.0
    };

    let standard_error = (standard_error_variance + between_estimate_variance).sqrt();

    standard_error.is_finite().then_some(MeanEstimate {
        value: arithmetic_mean_or_zero(values),
        standard_error,
    })
}

/// Estimate a metric's standard error with delete-one jackknife resampling.
pub fn jackknife_standard_error(
    values: &[f64],
    compute_metric: fn(&[f64]) -> Option<f64>,
) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }

    let mut leave_one_out_values = Vec::with_capacity(values.len());

    for omitted_position in 0..values.len() {
        let sample = values
            .iter()
            .enumerate()
            .filter_map(|(value_position, value)| {
                (value_position != omitted_position).then_some(*value)
            })
            .collect::<Vec<_>>();

        let metric_value = compute_metric(sample.as_slice())?;
        if !metric_value.is_finite() {
            return None;
        }

        leave_one_out_values.push(metric_value);
    }

    let jackknife_mean = arithmetic_mean_or_zero(leave_one_out_values.as_slice());

    let squared_deviation_sum = leave_one_out_values
        .iter()
        .map(|value| {
            let deviation = value - jackknife_mean;
            deviation * deviation
        })
        .sum::<f64>();

    let variance = (values.len() - 1) as f64 / values.len() as f64 * squared_deviation_sum;
    let standard_error = variance.sqrt();

    (standard_error.is_finite() && standard_error > 0.0).then_some(standard_error)
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

#[cfg(test)]
mod tests {
    use super::{
        arithmetic_mean, arithmetic_mean_or_zero, central_skewness_or_zero,
        coefficient_of_variation, finite_arithmetic_mean_or_zero, jackknife_standard_error,
        mean_estimate_from_standard_errors, pearson_correlation, relative_rmse, rolling_sum,
        root_mean_square_or_infinity, sample_adjusted_excess_kurtosis, sample_adjusted_skewness,
        sample_standard_deviation, sample_variance, standardized_path_distance,
        winsorized_sample_standard_deviation_from_sorted, z_score,
    };

    #[test]
    fn sample_helpers_are_deterministic() {
        assert_eq!(arithmetic_mean(&[1.0, 2.0, 3.0]), Some(2.0));
        assert_eq!(arithmetic_mean_or_zero(&[]), 0.0);
        assert_eq!(finite_arithmetic_mean_or_zero(&[1.0, f64::NAN, 3.0]), 2.0);
        assert_eq!(rolling_sum(&[1.0, 2.0, 3.0], 2), vec![1.0, 3.0, 5.0]);
        assert_eq!(rolling_sum(&[1.0, 2.0], 0), vec![0.0, 0.0]);
        assert_eq!(coefficient_of_variation(&[2.0, 2.0, 2.0]), 0.0);
        assert_eq!(pearson_correlation(&[1.0, 2.0], &[2.0, 4.0]), 1.0);
        assert!((root_mean_square_or_infinity(&[3.0, 4.0]) - (12.5f64).sqrt()).abs() <= 1.0e-12);
        assert_eq!(z_score(&[2.0, 2.0]), vec![0.0, 0.0]);
        assert_eq!(
            standardized_path_distance(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]),
            0.0
        );
        assert!((relative_rmse(&[2.0, 4.0], &[1.0, 3.0], 1.0e-8) - 0.5).abs() <= 1.0e-12);
        assert!((sample_standard_deviation(&[1.0, 2.0, 3.0]) - 1.0).abs() <= 1.0e-12);
        assert!(
            winsorized_sample_standard_deviation_from_sorted(&[1.0, 2.0, 3.0, 100.0], 0.25, 0.75)
                < sample_standard_deviation(&[1.0, 2.0, 3.0, 100.0])
        );
        assert_eq!(sample_variance(&[1.0, 2.0, 3.0]), Some(1.0));
        assert_eq!(
            central_skewness_or_zero(&[1.0, 2.0, 3.0, 4.0, 5.0], 1.0e-12),
            0.0
        );
        assert_eq!(sample_adjusted_skewness(&[1.0, 2.0]), None);
        assert_eq!(sample_adjusted_excess_kurtosis(&[1.0, 2.0, 3.0]), None);
    }

    #[test]
    fn mean_estimate_from_standard_errors_pools_within_and_between_estimate_variance() {
        let estimate = mean_estimate_from_standard_errors(&[2.0, 4.0], &[1.0, 1.0])
            .expect("aligned finite estimates should pool");

        assert_eq!(estimate.value, 3.0);
        assert_eq!(estimate.standard_error, 2.0_f64.sqrt());
    }

    #[test]
    fn jackknife_standard_error_resamples_delete_one_metric() {
        let standard_error = jackknife_standard_error(&[1.0, 2.0, 4.0], arithmetic_mean)
            .expect("finite delete-one estimates should produce a standard error");

        assert!((standard_error - (7.0 / 9.0_f64).sqrt()).abs() <= 1.0e-12);
    }
}
