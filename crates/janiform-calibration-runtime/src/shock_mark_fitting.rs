use janiform_foundations::numeric::{
    arithmetic_mean, round_positive_to_domain, smallest_positive_value,
    strictly_positive_machine_value,
};
use janiform_model_kernels::news::{
    GammaSeverityMarkConfiguration, SharedNewsMarkLawConfiguration,
};

pub(crate) fn fit_shared_news_mark_law_from_shock_samples(
    open_shock_returns: &[f64],
    gap_returns: &[f64],
    fallback_absolute_returns: &[f64],
) -> SharedNewsMarkLawConfiguration {
    let open_shock_magnitudes = positive_magnitudes(open_shock_returns);
    let gap_magnitudes = positive_magnitudes(gap_returns);

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

    SharedNewsMarkLawConfiguration {
        gamma_severity_marks: GammaSeverityMarkConfiguration {
            shape_k: fit_gamma_unit_mean_shape(gamma_magnitude_sample.as_slice()),
        },
    }
}

fn positive_magnitudes(return_sample: &[f64]) -> Vec<f64> {
    return_sample
        .iter()
        .copied()
        .filter(|return_value| return_value.is_finite())
        .map(f64::abs)
        .filter(|return_value| *return_value > 0.0)
        .collect()
}

fn fit_gamma_unit_mean_shape(magnitude_sample: &[f64]) -> f64 {
    let Some(mean_magnitude) = arithmetic_mean(magnitude_sample) else {
        return 1.0;
    };
    if !mean_magnitude.is_finite() || mean_magnitude <= 0.0 {
        return 1.0;
    }

    let variance = if magnitude_sample.len() < 2 {
        smallest_positive_value(magnitude_sample).unwrap_or_else(strictly_positive_machine_value)
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

    if !variance.is_finite() || variance <= 0.0 {
        return 1.0;
    }

    round_positive_to_domain(mean_magnitude * mean_magnitude / variance, 12)
}

#[cfg(test)]
mod tests {
    use super::fit_shared_news_mark_law_from_shock_samples;

    #[test]
    fn shared_mark_law_fit_tracks_observed_shock_severity_dispersion() {
        let fitted_mark_law = fit_shared_news_mark_law_from_shock_samples(
            &[0.004, 0.002, -0.001, -0.003],
            &[0.006, -0.004],
            &[0.0005, 0.0007],
        );

        assert!(fitted_mark_law.gamma_severity_marks.shape_k > 0.0);
        assert_eq!(
            fitted_mark_law
                .gamma_severity_marks
                .expected_severity_mark()
                .expect("unit-mean severity should validate"),
            1.0
        );
    }
}
