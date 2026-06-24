use janiform_foundations::numeric::{
    arithmetic_mean_or_zero as arithmetic_mean, compute_sorted_quantile_or_zero,
    root_mean_square_or_infinity, sorted_finite_values, strictly_positive_machine_value,
    validate_positive_number,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::RETURN_BACKFIT_QUANTILE_LEVELS;
use super::aggregation::AggregatedQuantileCalibrationSample;
use crate::stages::return_components::{
    build_continuous_return_calibration_sample_from_components,
    simulate_matched_scale_returns_with_seed,
};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct QuantileShapeEvaluation {
    pub(super) delta_multiplier: f64,
    pub(super) nig_configuration: NigSharedPlusShiftConfiguration,
    pub(super) score: f64,
    pub(super) observed_quantiles: Vec<f64>,
    pub(super) fitted_quantiles: Vec<f64>,
    observed_median: f64,
    fitted_median: f64,
}

pub(super) fn evaluate_quantile_shape_delta_candidate(
    selected_nig_configuration: NigSharedPlusShiftConfiguration,
    delta_multiplier: f64,
    aggregation_sample: &AggregatedQuantileCalibrationSample,
    stage_seed: u64,
) -> Result<QuantileShapeEvaluation, String> {
    let candidate_configuration =
        delta_quantile_shape_nig_configuration(selected_nig_configuration, delta_multiplier)?;

    let calibration_sample = build_continuous_return_calibration_sample_from_components(
        aggregation_sample.observed_returns.clone(),
        aggregation_sample.business_time_scales.clone(),
        aggregation_sample.sell_share_proxies.clone(),
        arithmetic_mean(aggregation_sample.sell_share_proxies.as_slice()).clamp(0.0, 1.0),
    );

    let fitted_returns = simulate_matched_scale_returns_with_seed(
        candidate_configuration,
        &calibration_sample,
        stage_seed,
    );

    if fitted_returns.len() != aggregation_sample.sample_count() {
        return Err(format!(
            "return-backfit fitted {} returns for {} aggregated observations",
            fitted_returns.len(),
            aggregation_sample.sample_count()
        ));
    }

    let observed_quantiles = quantile_values(
        aggregation_sample.observed_returns.as_slice(),
        &RETURN_BACKFIT_QUANTILE_LEVELS,
    );
    let fitted_quantiles =
        quantile_values(fitted_returns.as_slice(), &RETURN_BACKFIT_QUANTILE_LEVELS);
    let observed_median = compute_median(aggregation_sample.observed_returns.as_slice());
    let fitted_median = compute_median(fitted_returns.as_slice());
    let score = quantile_shape_score(
        observed_quantiles.as_slice(),
        fitted_quantiles.as_slice(),
        observed_median,
        fitted_median,
    );

    Ok(QuantileShapeEvaluation {
        delta_multiplier,
        nig_configuration: candidate_configuration,
        score,
        observed_quantiles,
        fitted_quantiles,
        observed_median,
        fitted_median,
    })
}

pub(super) fn data_derived_delta_multipliers(
    baseline_evaluation: &QuantileShapeEvaluation,
    long_variance_clock_scale_multiplier: f64,
) -> Result<Vec<f64>, String> {
    let mut candidate_multipliers = vec![1.0];

    let clock_offset_multiplier =
        validate_delta_clock_offset_multiplier(long_variance_clock_scale_multiplier)?;
    candidate_multipliers.push(clock_offset_multiplier);

    for (observed_quantile, fitted_quantile) in baseline_evaluation
        .observed_quantiles
        .iter()
        .zip(baseline_evaluation.fitted_quantiles.iter())
    {
        let observed_centered_quantile = observed_quantile - baseline_evaluation.observed_median;
        let fitted_centered_quantile = fitted_quantile - baseline_evaluation.fitted_median;

        let Some(delta_multiplier) = delta_multiplier_from_centered_quantiles(
            observed_centered_quantile,
            fitted_centered_quantile,
        ) else {
            continue;
        };

        candidate_multipliers.push(delta_multiplier);
    }

    let quantile_ratio_median = finite_positive_median(candidate_multipliers.as_slice());
    if let Some(quantile_ratio_median) = quantile_ratio_median {
        candidate_multipliers.push(quantile_ratio_median);
    }

    let mut candidate_multipliers = sorted_unique_positive_values(candidate_multipliers);
    let mut midpoint_candidates = Vec::new();

    for adjacent_candidates in candidate_multipliers.windows(2) {
        let [left_multiplier, right_multiplier] = adjacent_candidates else {
            continue;
        };

        let midpoint_multiplier = ((*left_multiplier).ln() + (*right_multiplier).ln())
            .mul_add(0.5, 0.0)
            .exp();

        if midpoint_multiplier.is_finite() && midpoint_multiplier > 0.0 {
            midpoint_candidates.push(midpoint_multiplier);
        }
    }

    candidate_multipliers.extend(midpoint_candidates);

    Ok(sorted_unique_positive_values(candidate_multipliers))
}

pub(super) fn max_delta_multiplier_candidate_count(quantile_count: usize) -> usize {
    let baseline_candidate_count = 1usize;
    let clock_offset_candidate_count = 1usize;
    let median_candidate_count = 1usize;
    let candidate_count_before_midpoints = baseline_candidate_count
        .saturating_add(clock_offset_candidate_count)
        .saturating_add(quantile_count)
        .saturating_add(median_candidate_count);

    candidate_count_before_midpoints
        .saturating_add(candidate_count_before_midpoints.saturating_sub(1))
}

fn quantile_values(returns: &[f64], quantile_levels: &[f64]) -> Vec<f64> {
    let sorted_returns = sorted_finite_values(returns);

    quantile_levels
        .iter()
        .map(|quantile_level| {
            compute_sorted_quantile_or_zero(sorted_returns.as_slice(), *quantile_level)
        })
        .collect()
}

fn compute_median(returns: &[f64]) -> f64 {
    let sorted_returns = sorted_finite_values(returns);
    compute_sorted_quantile_or_zero(sorted_returns.as_slice(), 0.5)
}

fn quantile_shape_score(
    observed_quantiles: &[f64],
    fitted_quantiles: &[f64],
    observed_median: f64,
    fitted_median: f64,
) -> f64 {
    let aligned_quantile_count = observed_quantiles.len().min(fitted_quantiles.len());
    if aligned_quantile_count == 0 {
        return f64::INFINITY;
    }

    let observed_centered_quantiles: Vec<f64> = observed_quantiles
        .iter()
        .take(aligned_quantile_count)
        .map(|observed_quantile| observed_quantile - observed_median)
        .collect();

    let score_scale = largest_positive_scale(observed_centered_quantiles.as_slice());

    let normalized_errors: Vec<f64> = observed_quantiles
        .iter()
        .zip(fitted_quantiles.iter())
        .take(aligned_quantile_count)
        .map(|(observed_quantile, fitted_quantile)| {
            let observed_centered_quantile = observed_quantile - observed_median;
            let fitted_centered_quantile = fitted_quantile - fitted_median;

            (fitted_centered_quantile - observed_centered_quantile).abs() / score_scale
        })
        .collect();

    root_mean_square_or_infinity(normalized_errors.as_slice())
}

fn delta_multiplier_from_centered_quantiles(
    observed_centered_quantile: f64,
    fitted_centered_quantile: f64,
) -> Option<f64> {
    if !observed_centered_quantile.is_finite()
        || !fitted_centered_quantile.is_finite()
        || fitted_centered_quantile == 0.0
    {
        return None;
    }

    let absolute_ratio = observed_centered_quantile.abs() / fitted_centered_quantile.abs();
    let delta_multiplier = absolute_ratio * absolute_ratio;

    if delta_multiplier.is_finite() && delta_multiplier > 0.0 {
        Some(delta_multiplier)
    } else {
        None
    }
}

fn finite_positive_median(values: &[f64]) -> Option<f64> {
    let sorted_values = sorted_unique_positive_values(values.to_vec());
    if sorted_values.is_empty() {
        return None;
    }

    Some(compute_sorted_quantile_or_zero(
        sorted_values.as_slice(),
        0.5,
    ))
}

fn sorted_unique_positive_values(values: Vec<f64>) -> Vec<f64> {
    let mut positive_values: Vec<f64> = values
        .into_iter()
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect();

    positive_values.sort_by(f64::total_cmp);
    positive_values.dedup_by(|left, right| left.total_cmp(right).is_eq());

    positive_values
}

fn delta_quantile_shape_nig_configuration(
    nig_configuration: NigSharedPlusShiftConfiguration,
    delta_multiplier: f64,
) -> Result<NigSharedPlusShiftConfiguration, String> {
    validate_positive_number(delta_multiplier, "delta_backfit_multiplier").map_err(|source| {
        format!("return-backfit delta quantile-shape multiplier is invalid: {source}")
    })?;

    let post_backfit_delta = nig_configuration.delta * delta_multiplier;

    validate_positive_number(post_backfit_delta, "post_backfit_nig_delta")
        .map_err(|source| format!("return-backfit post-backfit NIG delta is invalid: {source}"))?;

    NigSharedPlusShiftConfiguration::try_new(
        nig_configuration.alpha,
        nig_configuration.beta_0,
        post_backfit_delta,
        nig_configuration.mu,
        nig_configuration.delta_beta,
    )
    .map_err(|source| format!("return-backfit post-backfit NIG configuration is invalid: {source}"))
}

fn validate_delta_clock_offset_multiplier(
    long_variance_clock_scale_multiplier: f64,
) -> Result<f64, String> {
    validate_positive_number(
        long_variance_clock_scale_multiplier,
        "long_variance_clock_scale_multiplier",
    )
    .map_err(|source| {
        format!("return-backfit long-variance clock scale multiplier is invalid: {source}")
    })?;

    let delta_multiplier = 1.0 / long_variance_clock_scale_multiplier;

    validate_positive_number(delta_multiplier, "delta_clock_offset_multiplier")
        .map(|()| delta_multiplier)
        .map_err(|source| {
            format!("return-backfit delta clock-offset multiplier is invalid: {source}")
        })
}

fn largest_positive_scale(values: &[f64]) -> f64 {
    values
        .iter()
        .copied()
        .map(f64::abs)
        .filter(|value| value.is_finite() && *value > 0.0)
        .max_by(f64::total_cmp)
        .unwrap_or_else(strictly_positive_machine_value)
}

#[cfg(test)]
mod tests {
    use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

    use super::{
        QuantileShapeEvaluation, data_derived_delta_multipliers,
        delta_quantile_shape_nig_configuration,
    };

    #[test]
    fn data_derived_delta_multipliers_include_quantile_scale_ratios() {
        let configuration =
            NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
                .expect("fixture NIG parameters should be valid");

        let evaluation = QuantileShapeEvaluation {
            delta_multiplier: 1.0,
            nig_configuration: configuration,
            score: 0.0,
            observed_quantiles: vec![-0.5, 0.5],
            fitted_quantiles: vec![-1.0, 1.0],
            observed_median: 0.0,
            fitted_median: 0.0,
        };

        let multipliers = data_derived_delta_multipliers(&evaluation, 1.0)
            .expect("clock multiplier should be valid");

        assert!(multipliers.contains(&0.25));
        assert!(multipliers.contains(&1.0));
    }

    #[test]
    fn delta_quantile_shape_configuration_preserves_nig_shape_parameters() {
        let configuration =
            NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
                .expect("fixture NIG parameters should be valid");

        let post_backfit = delta_quantile_shape_nig_configuration(configuration, 0.25)
            .expect("delta backfit should preserve NIG parameter validity");

        assert_eq!(post_backfit.alpha, configuration.alpha);
        assert_eq!(post_backfit.beta_0, configuration.beta_0);
        assert_eq!(post_backfit.delta, 5.0e-5);
        assert_eq!(post_backfit.mu, configuration.mu);
        assert_eq!(post_backfit.delta_beta, configuration.delta_beta);
    }
}
