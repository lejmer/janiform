use super::domain::strictly_positive_machine_value;

/// Computes the absolute natural-log distance between two positive magnitudes.
pub fn log_ratio_distance(candidate_value: f64, observed_value: f64, floor: f64) -> f64 {
    log_ratio_distance_with_minimum_floor(
        candidate_value,
        observed_value,
        floor,
        strictly_positive_machine_value(),
    )
}

/// Computes the absolute natural-log distance with an explicit positive floor policy.
pub fn log_ratio_distance_with_minimum_floor(
    candidate_value: f64,
    observed_value: f64,
    floor: f64,
    minimum_floor: f64,
) -> f64 {
    let safe_floor = floor
        .abs()
        .max(minimum_floor.abs())
        .max(strictly_positive_machine_value());
    let candidate = candidate_value.abs().max(safe_floor);
    let observed = observed_value.abs().max(safe_floor);

    (candidate / observed).ln().abs()
}

/// Computes `ln(sum(exp(x)))` for finite log terms.
///
/// Non-finite terms are ignored. When no finite terms are provided, the result
/// is `ln(0)`, represented as negative infinity.
pub fn log_sum_exp(log_terms: &[f64]) -> f64 {
    let Some(maximum_log_term) = log_terms
        .iter()
        .copied()
        .filter(|log_term| log_term.is_finite())
        .reduce(f64::max)
    else {
        return f64::NEG_INFINITY;
    };

    let scaled_sum = log_terms
        .iter()
        .copied()
        .filter(|log_term| log_term.is_finite())
        .map(|log_term| (log_term - maximum_log_term).exp())
        .sum::<f64>();

    maximum_log_term + scaled_sum.ln()
}

/// Computes `ln(exp(left) + exp(right))` for two finite log terms.
///
/// This preserves the historical two-term behavior used by calibration
/// likelihood code: if either term is non-finite, the other term is returned.
pub fn log_sum_exp_pair(left: f64, right: f64) -> f64 {
    if !left.is_finite() {
        return right;
    }
    if !right.is_finite() {
        return left;
    }

    let maximum = left.max(right);
    maximum + ((left - maximum).exp() + (right - maximum).exp()).ln()
}

#[cfg(test)]
mod tests {
    use super::{log_ratio_distance, log_sum_exp, log_sum_exp_pair};

    #[test]
    fn log_ratio_distance_matches_absolute_natural_log_ratio_for_positive_values() {
        assert_eq!(
            log_ratio_distance(4.0, 2.0, f64::MIN_POSITIVE),
            2.0_f64.ln()
        );
    }

    #[test]
    fn log_sum_exp_matches_direct_sum_for_finite_terms() {
        let log_terms = [0.0, 0.0, 0.0];

        assert_eq!(log_sum_exp(log_terms.as_slice()), 3.0_f64.ln());
    }

    #[test]
    fn log_sum_exp_returns_log_zero_for_empty_or_non_finite_terms() {
        assert_eq!(log_sum_exp(&[]), f64::NEG_INFINITY);
        assert_eq!(log_sum_exp(&[f64::NAN]), f64::NEG_INFINITY);
    }

    #[test]
    fn log_sum_exp_pair_matches_legacy_non_finite_contract() {
        assert_eq!(log_sum_exp_pair(f64::NAN, 2.0), 2.0);
        assert_eq!(log_sum_exp_pair(1.0, f64::NAN), 1.0);
    }
}
