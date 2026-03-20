use janiform_foundations::numeric::{
    arithmetic_mean, clamp_to_open_unit_interval, round_positive_to_domain, round_to,
    smallest_positive_value, strictly_positive_machine_value,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, SharedNewsMarkLawConfiguration,
};

pub(crate) fn fit_shared_news_mark_law_from_shock_samples(
    open_shock_returns: &[f64],
    gap_returns: &[f64],
    fallback_absolute_returns: &[f64],
) -> SharedNewsMarkLawConfiguration {
    let positive_fallback_scale = smallest_positive_value(fallback_absolute_returns)
        .unwrap_or_else(strictly_positive_machine_value);
    let open_shock_magnitudes: Vec<f64> = open_shock_returns
        .iter()
        .copied()
        .filter(|return_value| return_value.is_finite())
        .map(f64::abs)
        .filter(|return_value| *return_value > 0.0)
        .collect();
    let gap_magnitudes: Vec<f64> = gap_returns
        .iter()
        .copied()
        .filter(|return_value| return_value.is_finite())
        .map(f64::abs)
        .filter(|return_value| *return_value > 0.0)
        .collect();
    let all_shock_magnitudes: Vec<f64> = open_shock_magnitudes
        .iter()
        .copied()
        .chain(gap_magnitudes.iter().copied())
        .collect();
    let gamma_magnitude_sample = if all_shock_magnitudes.is_empty() {
        fallback_absolute_returns
            .iter()
            .copied()
            .filter(|return_value| return_value.is_finite() && *return_value > 0.0)
            .collect::<Vec<_>>()
    } else {
        all_shock_magnitudes
    };
    let positive_open_shock_magnitudes: Vec<f64> = open_shock_returns
        .iter()
        .copied()
        .filter(|return_value| return_value.is_finite() && *return_value > 0.0)
        .collect();
    let negative_open_shock_magnitudes: Vec<f64> = open_shock_returns
        .iter()
        .copied()
        .filter(|return_value| return_value.is_finite() && *return_value < 0.0)
        .map(f64::abs)
        .collect();

    let (shape_k, scale_theta) =
        fit_gamma_shape_scale(gamma_magnitude_sample.as_slice(), positive_fallback_scale);
    let (probability_positive, eta_plus, eta_minus) = fit_double_exponential_base_shock(
        positive_open_shock_magnitudes.as_slice(),
        negative_open_shock_magnitudes.as_slice(),
        arithmetic_mean(gamma_magnitude_sample.as_slice()).unwrap_or(positive_fallback_scale),
        positive_fallback_scale,
    );
    let sigma_g = round_to(
        arithmetic_mean(gap_magnitudes.as_slice())
            .or_else(|| arithmetic_mean(gamma_magnitude_sample.as_slice()))
            .unwrap_or(positive_fallback_scale)
            .max(0.0),
        12,
    );

    SharedNewsMarkLawConfiguration {
        gamma_severity_marks: GammaSeverityMarkConfiguration {
            shape_k,
            scale_theta,
        },
        double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
            probability_positive,
            eta_plus,
            eta_minus,
        },
        coupled_gap_marks: CoupledGapMarkConfiguration { sigma_g },
    }
}

fn fit_gamma_shape_scale(magnitude_sample: &[f64], positive_fallback_scale: f64) -> (f64, f64) {
    let mean_magnitude = arithmetic_mean(magnitude_sample)
        .unwrap_or(positive_fallback_scale)
        .max(positive_fallback_scale);
    let variance = if magnitude_sample.len() < 2 {
        0.0
    } else {
        magnitude_sample
            .iter()
            .map(|magnitude| {
                let centered = *magnitude - mean_magnitude;
                centered * centered
            })
            .sum::<f64>()
            / magnitude_sample.len() as f64
    };

    if !variance.is_finite() || variance <= positive_fallback_scale * positive_fallback_scale {
        return (1.0, round_positive_to_domain(mean_magnitude, 12));
    }

    let shape_k = round_positive_to_domain(mean_magnitude * mean_magnitude / variance, 12);
    let scale_theta = round_positive_to_domain(variance / mean_magnitude, 12);

    (shape_k, scale_theta)
}

fn fit_double_exponential_base_shock(
    positive_open_shock_magnitudes: &[f64],
    negative_open_shock_magnitudes: &[f64],
    overall_mean_magnitude: f64,
    positive_fallback_scale: f64,
) -> (f64, f64, f64) {
    let total_signed_sample_count =
        positive_open_shock_magnitudes.len() + negative_open_shock_magnitudes.len();
    let raw_probability_positive = if total_signed_sample_count == 0 {
        0.5
    } else {
        positive_open_shock_magnitudes.len() as f64 / total_signed_sample_count as f64
    };
    let probability_positive = clamp_to_open_unit_interval(round_to(raw_probability_positive, 12));
    let eta_plus = round_positive_to_domain(
        1.0 / arithmetic_mean(positive_open_shock_magnitudes)
            .unwrap_or(overall_mean_magnitude)
            .max(positive_fallback_scale),
        12,
    );
    let eta_minus = round_positive_to_domain(
        1.0 / arithmetic_mean(negative_open_shock_magnitudes)
            .unwrap_or(overall_mean_magnitude)
            .max(positive_fallback_scale),
        12,
    );

    (probability_positive, eta_plus, eta_minus)
}

#[cfg(test)]
mod tests {
    use super::fit_shared_news_mark_law_from_shock_samples;

    #[test]
    fn shared_mark_law_fit_tracks_observed_sign_balance_and_scale() {
        let fitted_mark_law = fit_shared_news_mark_law_from_shock_samples(
            &[0.004, 0.002, -0.001, -0.003],
            &[0.006, -0.004],
            &[0.0005, 0.0007],
        );

        assert!(fitted_mark_law.gamma_severity_marks.shape_k > 0.0);
        assert!(fitted_mark_law.gamma_severity_marks.scale_theta > 0.0);
        assert!(
            fitted_mark_law
                .double_exponential_base_shock
                .probability_positive
                > 0.0
        );
        assert!(
            fitted_mark_law
                .double_exponential_base_shock
                .probability_positive
                < 1.0
        );
        assert!(fitted_mark_law.double_exponential_base_shock.eta_plus > 0.0);
        assert!(fitted_mark_law.double_exponential_base_shock.eta_minus > 0.0);
        assert!(fitted_mark_law.coupled_gap_marks.sigma_g > 0.0);
    }
}
