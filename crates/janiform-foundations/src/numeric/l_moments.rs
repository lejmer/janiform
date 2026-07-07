use super::ordering::compute_sorted_quantile;

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

/// Compute the fourth L-moment ratio from a sorted finite sample.
pub fn l_moment_tau4_from_sorted(
    sorted_values: &[f64],
    minimum_denominator_magnitude: f64,
) -> Option<f64> {
    let probability_weighted_moment_0 = compute_probability_weighted_moment(sorted_values, 0)?;
    let probability_weighted_moment_1 = compute_probability_weighted_moment(sorted_values, 1)?;
    let probability_weighted_moment_2 = compute_probability_weighted_moment(sorted_values, 2)?;
    let probability_weighted_moment_3 = compute_probability_weighted_moment(sorted_values, 3)?;

    let lambda2 = 2.0 * probability_weighted_moment_1 - probability_weighted_moment_0;
    if !lambda2.is_finite() || lambda2.abs() <= minimum_denominator_magnitude {
        return None;
    }

    let lambda4 = 20.0 * probability_weighted_moment_3 - 30.0 * probability_weighted_moment_2
        + 12.0 * probability_weighted_moment_1
        - probability_weighted_moment_0;
    if !lambda4.is_finite() {
        return None;
    }

    let tau4 = lambda4 / lambda2;
    tau4.is_finite().then_some(tau4)
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
        bowley_skewness_from_sorted, compute_probability_weighted_moment,
        l_moment_lambda2_from_sorted, l_moment_tau3_from_sorted, l_moment_tau4_from_sorted,
    };

    #[test]
    fn l_moment_helpers_are_deterministic() {
        assert_eq!(
            bowley_skewness_from_sorted(&[1.0, 2.0, 3.0, 4.0, 5.0], 1.0e-12),
            Some(0.0)
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
        assert_eq!(
            l_moment_tau4_from_sorted(&[1.0, 2.0, 3.0, 4.0], 1.0e-12),
            Some(0.0)
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
