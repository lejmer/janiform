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

/// Use a data-normalized smoothing pseudocount rather than a fixed constant.
pub fn normalized_pseudocount(event_count: usize) -> f64 {
    1.0 / event_count.max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::{
        arithmetic_mean, arithmetic_mean_or_zero, clamp_to_open_unit_interval,
        clamp_to_unit_interval, coefficient_of_variation, logistic, median_abs,
        normalized_pseudocount, round_positive_to_domain, round_to, smallest_positive_value,
        strictly_positive_machine_value, unique_sorted_values,
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
        assert_eq!(round_to(1.23456, 3), 1.235);
        assert_eq!(smallest_positive_value(&[-1.0, 2.0, 0.0, 1.0]), Some(1.0));
        assert_eq!(unique_sorted_values(&[2.0, 1.0, 2.0]), vec![1.0, 2.0]);
        assert_eq!(normalized_pseudocount(4), 0.25);
        assert_eq!(logistic(0.0), 0.5);
    }
}
