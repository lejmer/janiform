use janiform_analytics_stylized_facts::stylized_facts::return_series_shared::compute_lag_autocorrelation;
use janiform_engine_risk::SufficientStatisticsRiskPath;
use janiform_foundations::numeric::compute_sorted_quantile;

use super::contracts::{
    ScaleConsistencyMetricComputationFailure, ScaleConsistencyMetricDatasetRole,
    ScaleConsistencyMetricFailureReasonCode,
};

const TAIL_QUANTILE_LEVEL: f64 = 0.95;
const MINIMUM_DENOMINATOR_MAGNITUDE: f64 = 1e-12;

#[derive(Debug, Clone)]
pub(super) struct ScalarMetricResult {
    pub(super) value: Option<f64>,
    pub(super) failures: Vec<ScaleConsistencyMetricComputationFailure>,
}

impl ScalarMetricResult {
    pub(super) fn with_dataset_role(self, dataset_role: ScaleConsistencyMetricDatasetRole) -> Self {
        Self {
            value: self.value,
            failures: self
                .failures
                .into_iter()
                .map(|failure| ScaleConsistencyMetricComputationFailure {
                    dataset_role,
                    reason_code: failure.reason_code,
                    lag_interval_count: failure.lag_interval_count,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum PairwiseMetricResult {
    Computed {
        distance: f64,
    },
    Missing {
        failures: Vec<ScaleConsistencyMetricComputationFailure>,
    },
}

#[derive(Debug, Clone, Copy)]
struct ScalarMetricFailureTemplate {
    reason_code: ScaleConsistencyMetricFailureReasonCode,
    lag_interval_count: Option<usize>,
}

pub(super) fn compute_returns_central_variance(
    path: &SufficientStatisticsRiskPath,
) -> ScalarMetricResult {
    let series = path
        .records
        .iter()
        .map(|record| record.return_components.total_return)
        .collect::<Vec<_>>();

    match compute_sample_variance(&series) {
        Some(value) => ScalarMetricResult {
            value: Some(value),
            failures: Vec::new(),
        },
        None => ScalarMetricResult {
            value: None,
            failures: vec![
                ScalarMetricFailureTemplate {
                    reason_code:
                        ScaleConsistencyMetricFailureReasonCode::InsufficientReturnObservations,
                    lag_interval_count: None,
                }
                .into(),
            ],
        },
    }
}

pub(super) fn compute_open_jump_abs_tail_expected_shortfall_ratio(
    path: &SufficientStatisticsRiskPath,
) -> ScalarMetricResult {
    let series = path
        .records
        .iter()
        .map(|record| record.return_components.open_jump_return.abs())
        .collect::<Vec<_>>();

    match compute_abs_tail_expected_shortfall_ratio(&series) {
        Some(value) => ScalarMetricResult {
            value: Some(value),
            failures: Vec::new(),
        },
        None => ScalarMetricResult {
            value: None,
            failures: vec![
                ScalarMetricFailureTemplate {
                    reason_code:
                        ScaleConsistencyMetricFailureReasonCode::MissingOpenJumpObservations,
                    lag_interval_count: None,
                }
                .into(),
            ],
        },
    }
}

pub(super) fn compute_closed_gap_abs_tail_expected_shortfall_ratio(
    path: &SufficientStatisticsRiskPath,
) -> ScalarMetricResult {
    let series = path
        .records
        .iter()
        .map(|record| record.return_components.closed_gap_return.abs())
        .collect::<Vec<_>>();

    match compute_abs_tail_expected_shortfall_ratio(&series) {
        Some(value) => ScalarMetricResult {
            value: Some(value),
            failures: Vec::new(),
        },
        None => ScalarMetricResult {
            value: None,
            failures: vec![
                ScalarMetricFailureTemplate {
                    reason_code:
                        ScaleConsistencyMetricFailureReasonCode::MissingClosedGapObservations,
                    lag_interval_count: None,
                }
                .into(),
            ],
        },
    }
}

pub(super) fn compute_closed_gap_vs_open_jump_wasserstein_distance(
    path: &SufficientStatisticsRiskPath,
) -> ScalarMetricResult {
    let open_jump_series = path
        .records
        .iter()
        .map(|record| record.return_components.open_jump_return.abs())
        .collect::<Vec<_>>();
    let closed_gap_series = path
        .records
        .iter()
        .map(|record| record.return_components.closed_gap_return.abs())
        .collect::<Vec<_>>();

    match compute_wasserstein_distance(&open_jump_series, &closed_gap_series) {
        Some(value) => ScalarMetricResult {
            value: Some(value),
            failures: Vec::new(),
        },
        None => ScalarMetricResult {
            value: None,
            failures: vec![
                ScalarMetricFailureTemplate {
                    reason_code:
                        ScaleConsistencyMetricFailureReasonCode::MissingOpenJumpObservations,
                    lag_interval_count: None,
                }
                .into(),
                ScalarMetricFailureTemplate {
                    reason_code:
                        ScaleConsistencyMetricFailureReasonCode::MissingClosedGapObservations,
                    lag_interval_count: None,
                }
                .into(),
            ],
        },
    }
}

pub(super) fn compute_return_distribution_wasserstein_distance(
    coarse_path: &SufficientStatisticsRiskPath,
    fine_path: &SufficientStatisticsRiskPath,
) -> PairwiseMetricResult {
    let coarse_returns = coarse_path
        .records
        .iter()
        .map(|record| record.return_components.total_return)
        .collect::<Vec<_>>();
    let fine_returns = fine_path
        .records
        .iter()
        .map(|record| record.return_components.total_return)
        .collect::<Vec<_>>();

    match compute_wasserstein_distance(&coarse_returns, &fine_returns) {
        Some(distance) => PairwiseMetricResult::Computed { distance },
        None => PairwiseMetricResult::Missing {
            failures: vec![
                ScaleConsistencyMetricComputationFailure {
                    dataset_role: ScaleConsistencyMetricDatasetRole::Coarse,
                    reason_code:
                        ScaleConsistencyMetricFailureReasonCode::InsufficientReturnObservations,
                    lag_interval_count: None,
                },
                ScaleConsistencyMetricComputationFailure {
                    dataset_role: ScaleConsistencyMetricDatasetRole::Fine,
                    reason_code:
                        ScaleConsistencyMetricFailureReasonCode::InsufficientReturnObservations,
                    lag_interval_count: None,
                },
            ],
        },
    }
}

pub(super) fn compute_abs_return_acf_distribution_distance(
    coarse_path: &SufficientStatisticsRiskPath,
    fine_path: &SufficientStatisticsRiskPath,
    lag_interval_counts: &[usize],
) -> PairwiseMetricResult {
    let coarse_abs_returns = coarse_path
        .records
        .iter()
        .map(|record| record.return_components.total_return.abs())
        .collect::<Vec<_>>();
    let fine_abs_returns = fine_path
        .records
        .iter()
        .map(|record| record.return_components.total_return.abs())
        .collect::<Vec<_>>();

    let mut total_distance = 0.0_f64;
    let mut comparison_count = 0_usize;
    let mut failures: Vec<ScaleConsistencyMetricComputationFailure> = Vec::new();

    for lag_interval_count in lag_interval_counts {
        let coarse_autocorrelation = validate_lag_autocorrelation(
            compute_lag_autocorrelation(&coarse_abs_returns, *lag_interval_count),
            ScaleConsistencyMetricDatasetRole::Coarse,
            *lag_interval_count,
            &mut failures,
        );
        let fine_autocorrelation = validate_lag_autocorrelation(
            compute_lag_autocorrelation(&fine_abs_returns, *lag_interval_count),
            ScaleConsistencyMetricDatasetRole::Fine,
            *lag_interval_count,
            &mut failures,
        );

        if let (Some(coarse_value), Some(fine_value)) =
            (coarse_autocorrelation, fine_autocorrelation)
        {
            total_distance += (coarse_value - fine_value).abs();
            comparison_count += 1;
        }
    }

    if !failures.is_empty() || comparison_count == 0 {
        return PairwiseMetricResult::Missing { failures };
    }

    PairwiseMetricResult::Computed {
        distance: total_distance / comparison_count as f64,
    }
}

fn validate_lag_autocorrelation(
    autocorrelation: Option<f64>,
    dataset_role: ScaleConsistencyMetricDatasetRole,
    lag_interval_count: usize,
    failures: &mut Vec<ScaleConsistencyMetricComputationFailure>,
) -> Option<f64> {
    match autocorrelation {
        Some(value) if value.is_finite() => Some(value),
        _ => {
            failures.push(ScaleConsistencyMetricComputationFailure {
                dataset_role,
                reason_code:
                    ScaleConsistencyMetricFailureReasonCode::InsufficientLaggedObservations,
                lag_interval_count: Some(lag_interval_count),
            });
            None
        }
    }
}

fn compute_sample_variance(series: &[f64]) -> Option<f64> {
    if series.len() < 2 || series.iter().any(|value| !value.is_finite()) {
        return None;
    }

    let sample_mean = series.iter().sum::<f64>() / series.len() as f64;
    let sum_squared_deviation = series
        .iter()
        .map(|value| {
            let centered_value = *value - sample_mean;
            centered_value * centered_value
        })
        .sum::<f64>();
    let variance = sum_squared_deviation / (series.len() - 1) as f64;

    variance.is_finite().then_some(variance)
}

fn compute_abs_tail_expected_shortfall_ratio(series: &[f64]) -> Option<f64> {
    if series.is_empty()
        || series
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
    {
        return None;
    }

    let mut sorted_values = series.to_vec();
    sorted_values.sort_by(f64::total_cmp);

    let value_at_risk = compute_sorted_quantile(&sorted_values, TAIL_QUANTILE_LEVEL);
    if !value_at_risk.is_finite() || value_at_risk <= MINIMUM_DENOMINATOR_MAGNITUDE {
        return None;
    }

    let tail_values = sorted_values
        .iter()
        .copied()
        .filter(|value| *value >= value_at_risk)
        .collect::<Vec<_>>();
    if tail_values.is_empty() {
        return None;
    }

    let expected_shortfall = tail_values.iter().sum::<f64>() / tail_values.len() as f64;
    let ratio = expected_shortfall / value_at_risk;

    ratio.is_finite().then_some(ratio)
}

fn compute_wasserstein_distance(left_series: &[f64], right_series: &[f64]) -> Option<f64> {
    if left_series.is_empty()
        || right_series.is_empty()
        || left_series.len() != right_series.len()
        || left_series.iter().any(|value| !value.is_finite())
        || right_series.iter().any(|value| !value.is_finite())
    {
        return None;
    }

    let mut sorted_left = left_series.to_vec();
    let mut sorted_right = right_series.to_vec();
    sorted_left.sort_by(f64::total_cmp);
    sorted_right.sort_by(f64::total_cmp);

    let total_distance = sorted_left
        .iter()
        .zip(sorted_right.iter())
        .map(|(left_value, right_value)| (left_value - right_value).abs())
        .sum::<f64>();

    let distance = total_distance / sorted_left.len() as f64;
    distance.is_finite().then_some(distance)
}

impl From<ScalarMetricFailureTemplate> for ScaleConsistencyMetricComputationFailure {
    fn from(template: ScalarMetricFailureTemplate) -> Self {
        Self {
            dataset_role: ScaleConsistencyMetricDatasetRole::Coarse,
            reason_code: template.reason_code,
            lag_interval_count: template.lag_interval_count,
        }
    }
}
