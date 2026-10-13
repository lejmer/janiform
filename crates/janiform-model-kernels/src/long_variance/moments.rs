/// Covariance of equal-duration averages of a stationary CIR factor, per unit latent variance.
/// `lag_hours` is the separation between window starts. Positive lags must not overlap.
pub fn integrated_cir_covariance_factor(
    rate_per_hour: f64,
    window_hours: f64,
    lag_hours: f64,
) -> Option<f64> {
    if !rate_per_hour.is_finite()
        || rate_per_hour <= 0.0
        || !window_hours.is_finite()
        || window_hours <= 0.0
        || !lag_hours.is_finite()
        || (lag_hours != 0.0 && lag_hours < window_hours)
    {
        return None;
    }

    let rate = rate_per_hour * window_hours;
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }

    if lag_hours == 0.0 {
        // For rate <= 1 the alternating integrated-exponential series has decreasing terms.
        // Stop when the next term cannot change the floating-point sum.
        if rate <= 1.0 {
            let mut term = 1.0;
            let mut sum = term;
            let mut denominator = 3.0;

            loop {
                term *= -rate / denominator;

                let next = sum + term;

                if next == sum {
                    return Some(sum);
                }

                sum = next;

                denominator += 1.0;
            }
        }
        return Some(2.0 * (rate + (-rate).exp_m1()) / rate / rate);
    }

    let integrated_decay = -(-rate).exp_m1() / rate;

    Some(integrated_decay.powi(2) * (-rate_per_hour * (lag_hours - window_hours)).exp())
}

#[cfg(test)]
mod tests {
    use super::integrated_cir_covariance_factor;

    #[test]
    fn interval_covariance_uses_measured_width_and_elapsed_lag() {
        let expected = ((1.0 - (-2.0_f64).exp()) / 2.0).powi(2);

        assert_eq!(
            integrated_cir_covariance_factor(1.0, 2.0, 2.0),
            Some(expected)
        );

        assert_eq!(
            integrated_cir_covariance_factor(2.0, 1.0, 1.0),
            Some(expected)
        );

        assert_eq!(integrated_cir_covariance_factor(1.0, 0.0, 1.0), None);
        assert_eq!(integrated_cir_covariance_factor(1.0, f64::NAN, 1.0), None);
    }

    #[test]
    fn integrated_covariance_has_stationary_and_slow_decay_limits() {
        assert_eq!(
            integrated_cir_covariance_factor(f64::MIN_POSITIVE, 1.0, 0.0),
            Some(1.0)
        );

        assert_eq!(
            integrated_cir_covariance_factor(f64::MIN_POSITIVE, 1.0, 1.0),
            Some(1.0)
        );

        let zero = integrated_cir_covariance_factor(1.0, 1.0, 0.0).unwrap();

        assert!((zero - 2.0 / std::f64::consts::E).abs() <= f64::EPSILON);

        let first = integrated_cir_covariance_factor(0.2, 1.0, 1.0).unwrap();
        let second = integrated_cir_covariance_factor(0.2, 1.0, 2.0).unwrap();

        assert!((second / first - (-0.2_f64).exp()).abs() <= f64::EPSILON);
        assert_eq!(integrated_cir_covariance_factor(0.0, 1.0, 1.0), None);
        assert_eq!(integrated_cir_covariance_factor(1.0, 1.0, 0.5), None);
    }
}
