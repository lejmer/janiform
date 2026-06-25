use std::cmp::Ordering;

/// Ordering policy for optional values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionalOrderingMode {
    /// Smaller present values sort first. Present values sort before missing values.
    AscendingSomeFirst,
    /// Smaller present values sort first. Missing values sort before present values.
    AscendingNoneFirst,
    /// Larger present values sort first. Present values sort before missing values.
    DescendingSomeFirst,
    /// Larger present values sort first. Missing values sort before present values.
    DescendingNoneFirst,
}

/// Compare optional values using the requested value and missing-value ordering policy.
pub fn compare_optional_by<T>(
    left: Option<T>,
    right: Option<T>,
    mode: OptionalOrderingMode,
    compare_present_values: impl FnOnce(T, T) -> Ordering,
) -> Ordering {
    match (left, right) {
        (Some(left_value), Some(right_value)) => compare_present_values(left_value, right_value),
        (Some(_), None) => compare_some_to_none_by_mode(mode),
        (None, Some(_)) => compare_some_to_none_by_mode(mode).reverse(),
        (None, None) => Ordering::Equal,
    }
}

fn compare_some_to_none_by_mode(mode: OptionalOrderingMode) -> Ordering {
    match mode {
        OptionalOrderingMode::AscendingSomeFirst | OptionalOrderingMode::DescendingSomeFirst => {
            Ordering::Less
        }
        OptionalOrderingMode::AscendingNoneFirst | OptionalOrderingMode::DescendingNoneFirst => {
            Ordering::Greater
        }
    }
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

/// Compute the median after sorting the sample and use zero for empty input.
pub fn compute_median_f64_or_zero(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    values.sort_by(f64::total_cmp);

    compute_sorted_median(values)
}

/// Compute the integer median after sorting the sample and use zero for empty input.
pub fn compute_median_i64_or_zero(values: &mut [i64]) -> i64 {
    if values.is_empty() {
        return 0;
    }

    values.sort();

    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        values[middle - 1] / 2 + values[middle] / 2
    } else {
        values[middle]
    }
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

#[cfg(test)]
mod tests {
    use super::{
        OptionalOrderingMode, compare_optional_by, compute_finite_median_or_zero,
        compute_finite_quantile_or_zero, compute_median_f64_or_zero, compute_median_i64_or_zero,
        compute_sorted_median, compute_sorted_quantile, compute_sorted_quantile_or_zero,
        median_abs, median_abs_non_zero, smallest_positive_value, sorted_finite_values,
        unique_sorted_values,
    };

    #[test]
    fn ordering_helpers_are_deterministic() {
        assert_eq!(median_abs(&[-3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median_abs_non_zero(&[0.0, -3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median_abs_non_zero(&[0.0, 0.0]), None);
        assert_eq!(
            sorted_finite_values(&[3.0, f64::NAN, 1.0, 2.0]),
            vec![1.0, 2.0, 3.0]
        );
        assert_eq!(smallest_positive_value(&[-1.0, 2.0, 0.0, 1.0]), Some(1.0));
        assert_eq!(unique_sorted_values(&[2.0, 1.0, 2.0]), vec![1.0, 2.0]);
        assert_eq!(compute_sorted_median(&[1.0, 2.0, 4.0]), 2.0);
        assert_eq!(compute_sorted_median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(compute_sorted_quantile(&[1.0, 2.0, 4.0], 0.5), 2.0);
        assert_eq!(compute_sorted_quantile_or_zero(&[], 0.5), 0.0);
        assert_eq!(compute_finite_median_or_zero(&[1.0, f64::NAN, 3.0]), 2.0);

        let mut median_values = vec![4.0, 1.0, 3.0, 2.0];

        assert_eq!(
            compute_median_f64_or_zero(median_values.as_mut_slice()),
            2.5
        );

        let mut integer_median_values = vec![5, 1, 3];

        assert_eq!(
            compute_median_i64_or_zero(integer_median_values.as_mut_slice()),
            3
        );

        assert_eq!(
            compute_finite_quantile_or_zero(&[1.0, f64::NAN, 3.0], 0.5),
            2.0
        );
    }

    #[test]
    fn optional_comparator_orders_values_and_missing_values() {
        assert_eq!(
            compare_optional_by(
                Some(1.0_f64),
                Some(2.0_f64),
                OptionalOrderingMode::AscendingSomeFirst,
                |left, right| left.total_cmp(&right),
            ),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_optional_by(
                Some(1.0_f64),
                None,
                OptionalOrderingMode::AscendingSomeFirst,
                |left, right| left.total_cmp(&right),
            ),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_optional_by(
                Some(1.0_f64),
                None,
                OptionalOrderingMode::AscendingNoneFirst,
                |left, right| left.total_cmp(&right),
            ),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_optional_by(
                Some(1.0_f64),
                Some(2.0_f64),
                OptionalOrderingMode::DescendingSomeFirst,
                |left, right| right.total_cmp(&left),
            ),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_optional_by(
                None,
                Some(1.0_f64),
                OptionalOrderingMode::DescendingSomeFirst,
                |left, right| right.total_cmp(&left),
            ),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_optional_by(
                Some(1.0_f64),
                None,
                OptionalOrderingMode::DescendingNoneFirst,
                |left, right| right.total_cmp(&left),
            ),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_optional_by(
                Some(1.0_f64),
                Some(2.0_f64),
                OptionalOrderingMode::DescendingNoneFirst,
                |left, right| right.total_cmp(&left),
            ),
            std::cmp::Ordering::Greater
        );

        let left = ComparableScore { value: 1 };
        let right = ComparableScore { value: 2 };

        assert_eq!(
            compare_optional_by(
                Some(&left),
                Some(&right),
                OptionalOrderingMode::AscendingSomeFirst,
                |left_score, right_score| left_score.compare(right_score),
            ),
            std::cmp::Ordering::Less
        );
    }

    struct ComparableScore {
        value: u8,
    }

    impl ComparableScore {
        fn compare(&self, other: &Self) -> std::cmp::Ordering {
            self.value.cmp(&other.value)
        }
    }
}
