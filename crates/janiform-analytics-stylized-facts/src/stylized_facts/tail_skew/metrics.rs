//! Tail/skew metric definitions and computations.

use janiform_foundations::numeric::{
    bowley_skewness_from_sorted, compute_sorted_quantile, l_moment_lambda2_from_sorted,
    l_moment_tau3_from_sorted, sample_adjusted_excess_kurtosis, sample_adjusted_skewness,
    sample_variance, sorted_finite_values,
};

use super::contracts::{
    TailSkewMetricClass, TailSkewMetricSampleSufficiencyPolicy, TailSkewSampleSufficiencyPolicy,
};

const RETURNS_ABS_QUANTILE_SHAPE_RATIO_Q99_Q95_METRIC_ID: &str =
    "returns_abs_quantile_shape_ratio_q99_q95";
const RETURNS_ABS_QUANTILE_SHAPE_RATIO_Q99_Q95_METRIC_NAME: &str =
    "Returns Abs Quantile Shape Ratio Q99 Q95";
const RETURNS_LEFT_TAIL_EXPECTED_SHORTFALL_TO_VAR_RATIO_METRIC_ID: &str =
    "returns_left_tail_expected_shortfall_to_var_ratio";
const RETURNS_LEFT_TAIL_EXPECTED_SHORTFALL_TO_VAR_RATIO_METRIC_NAME: &str =
    "Returns Left Tail Expected Shortfall To VaR Ratio";
const RETURNS_BOWLEY_SKEWNESS_QUANTILE_METRIC_ID: &str = "returns_bowley_skewness_quantile";
const RETURNS_BOWLEY_SKEWNESS_QUANTILE_METRIC_NAME: &str = "Returns Bowley Skewness Quantile";
const RETURNS_SIGNED_TAIL_EXPECTED_SHORTFALL_IMBALANCE_METRIC_ID: &str =
    "returns_signed_tail_expected_shortfall_imbalance";
const RETURNS_SIGNED_TAIL_EXPECTED_SHORTFALL_IMBALANCE_METRIC_NAME: &str =
    "Returns Signed Tail Expected Shortfall Imbalance";
const RETURNS_L_MOMENT_LAMBDA2_METRIC_ID: &str = "returns_l_moment_lambda2";
const RETURNS_L_MOMENT_LAMBDA2_METRIC_NAME: &str = "Returns L Moment Lambda2";
const RETURNS_L_MOMENT_TAU3_METRIC_ID: &str = "returns_l_moment_tau3";
const RETURNS_L_MOMENT_TAU3_METRIC_NAME: &str = "Returns L Moment Tau3";
const RETURNS_CENTRAL_VARIANCE_METRIC_ID: &str = "returns_central_variance";
const RETURNS_CENTRAL_VARIANCE_METRIC_NAME: &str = "Returns Central Variance";
const RETURNS_CENTRAL_SKEWNESS_METRIC_ID: &str = "returns_central_skewness";
const RETURNS_CENTRAL_SKEWNESS_METRIC_NAME: &str = "Returns Central Skewness";
const RETURNS_CENTRAL_EXCESS_KURTOSIS_METRIC_ID: &str = "returns_central_excess_kurtosis";
const RETURNS_CENTRAL_EXCESS_KURTOSIS_METRIC_NAME: &str = "Returns Central Excess Kurtosis";

const LEFT_TAIL_QUANTILE_LEVEL: f64 = 0.05;
const RIGHT_TAIL_QUANTILE_LEVEL: f64 = 0.95;
const EXTREME_RIGHT_TAIL_QUANTILE_LEVEL: f64 = 0.99;
const MINIMUM_DENOMINATOR_MAGNITUDE: f64 = 1e-12;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TailSkewMetricComputation {
    pub(super) value: Option<f64>,
    pub(super) effective_sample_count: usize,
    pub(super) tail_observation_count: usize,
}

pub(super) type TailSkewMetricEvaluator = fn(&[f64]) -> TailSkewMetricComputation;

#[derive(Debug, Clone, Copy)]
pub(super) struct TailSkewMetricSpecification {
    pub(super) metric_id: &'static str,
    pub(super) metric_name: &'static str,
    pub(super) metric_class: TailSkewMetricClass,
    pub(super) metric_role: u64,
    pub(super) sample_sufficiency_selector:
        fn(TailSkewSampleSufficiencyPolicy) -> TailSkewMetricSampleSufficiencyPolicy,
    pub(super) metric_evaluator: TailSkewMetricEvaluator,
}

pub(super) const TAIL_SKEW_METRIC_SPECIFICATIONS: [TailSkewMetricSpecification; 9] = [
    TailSkewMetricSpecification {
        metric_id: RETURNS_ABS_QUANTILE_SHAPE_RATIO_Q99_Q95_METRIC_ID,
        metric_name: RETURNS_ABS_QUANTILE_SHAPE_RATIO_Q99_Q95_METRIC_NAME,
        metric_class: TailSkewMetricClass::PrimaryRobust,
        metric_role: 101,
        sample_sufficiency_selector: select_abs_quantile_shape_ratio_q99_q95,
        metric_evaluator: compute_abs_quantile_shape_ratio_q99_q95,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_LEFT_TAIL_EXPECTED_SHORTFALL_TO_VAR_RATIO_METRIC_ID,
        metric_name: RETURNS_LEFT_TAIL_EXPECTED_SHORTFALL_TO_VAR_RATIO_METRIC_NAME,
        metric_class: TailSkewMetricClass::PrimaryRobust,
        metric_role: 102,
        sample_sufficiency_selector: select_left_tail_expected_shortfall_to_var_ratio,
        metric_evaluator: compute_left_tail_expected_shortfall_to_var_ratio,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_BOWLEY_SKEWNESS_QUANTILE_METRIC_ID,
        metric_name: RETURNS_BOWLEY_SKEWNESS_QUANTILE_METRIC_NAME,
        metric_class: TailSkewMetricClass::PrimaryRobust,
        metric_role: 103,
        sample_sufficiency_selector: select_bowley_skewness_quantile,
        metric_evaluator: compute_bowley_skewness_quantile,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_SIGNED_TAIL_EXPECTED_SHORTFALL_IMBALANCE_METRIC_ID,
        metric_name: RETURNS_SIGNED_TAIL_EXPECTED_SHORTFALL_IMBALANCE_METRIC_NAME,
        metric_class: TailSkewMetricClass::PrimaryRobust,
        metric_role: 104,
        sample_sufficiency_selector: select_signed_tail_expected_shortfall_imbalance,
        metric_evaluator: compute_signed_tail_expected_shortfall_imbalance,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_L_MOMENT_LAMBDA2_METRIC_ID,
        metric_name: RETURNS_L_MOMENT_LAMBDA2_METRIC_NAME,
        metric_class: TailSkewMetricClass::SecondaryMoment,
        metric_role: 105,
        sample_sufficiency_selector: select_l_moment_lambda2,
        metric_evaluator: compute_l_moment_lambda2,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_L_MOMENT_TAU3_METRIC_ID,
        metric_name: RETURNS_L_MOMENT_TAU3_METRIC_NAME,
        metric_class: TailSkewMetricClass::SecondaryMoment,
        metric_role: 106,
        sample_sufficiency_selector: select_l_moment_tau3,
        metric_evaluator: compute_l_moment_tau3,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_CENTRAL_VARIANCE_METRIC_ID,
        metric_name: RETURNS_CENTRAL_VARIANCE_METRIC_NAME,
        metric_class: TailSkewMetricClass::SecondaryMoment,
        metric_role: 107,
        sample_sufficiency_selector: select_central_variance,
        metric_evaluator: compute_central_variance,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_CENTRAL_SKEWNESS_METRIC_ID,
        metric_name: RETURNS_CENTRAL_SKEWNESS_METRIC_NAME,
        metric_class: TailSkewMetricClass::SecondaryMoment,
        metric_role: 108,
        sample_sufficiency_selector: select_central_skewness,
        metric_evaluator: compute_central_skewness,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_CENTRAL_EXCESS_KURTOSIS_METRIC_ID,
        metric_name: RETURNS_CENTRAL_EXCESS_KURTOSIS_METRIC_NAME,
        metric_class: TailSkewMetricClass::SecondaryMoment,
        metric_role: 109,
        sample_sufficiency_selector: select_central_excess_kurtosis,
        metric_evaluator: compute_central_excess_kurtosis,
    },
];

/// Number of dataset-specific metric evaluations in one tail/skew interval.
pub const TAIL_SKEW_DATASET_METRIC_TASK_COUNT: usize = TAIL_SKEW_METRIC_SPECIFICATIONS.len() * 2;

fn select_abs_quantile_shape_ratio_q99_q95(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_abs_quantile_shape_ratio_q99_q95
}

fn select_left_tail_expected_shortfall_to_var_ratio(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_left_tail_expected_shortfall_to_var_ratio
}

fn select_bowley_skewness_quantile(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_bowley_skewness_quantile
}

fn select_signed_tail_expected_shortfall_imbalance(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_signed_tail_expected_shortfall_imbalance
}

fn select_l_moment_lambda2(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_l_moment_lambda2
}

fn select_l_moment_tau3(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_l_moment_tau3
}

fn select_central_variance(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_central_variance
}

fn select_central_skewness(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_central_skewness
}

fn select_central_excess_kurtosis(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_central_excess_kurtosis
}

fn compute_abs_quantile_shape_ratio_q99_q95(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let mut absolute_returns: Vec<f64> = interval_returns
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(f64::abs)
        .collect();

    if absolute_returns.is_empty() {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: 0,
            tail_observation_count: 0,
        };
    }

    absolute_returns.sort_by(f64::total_cmp);

    let quantile_95 = compute_sorted_quantile(&absolute_returns, RIGHT_TAIL_QUANTILE_LEVEL);
    let quantile_99 = compute_sorted_quantile(&absolute_returns, EXTREME_RIGHT_TAIL_QUANTILE_LEVEL);

    let value = if !quantile_95.is_finite()
        || quantile_95.abs() <= MINIMUM_DENOMINATOR_MAGNITUDE
        || !quantile_99.is_finite()
    {
        None
    } else {
        let ratio = quantile_99 / quantile_95;
        ratio.is_finite().then_some(ratio)
    };

    TailSkewMetricComputation {
        value,
        effective_sample_count: absolute_returns.len(),
        tail_observation_count: 0,
    }
}

fn compute_left_tail_expected_shortfall_to_var_ratio(
    interval_returns: &[f64],
) -> TailSkewMetricComputation {
    let sorted_returns = sorted_finite_values(interval_returns);

    if sorted_returns.is_empty() {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: 0,
            tail_observation_count: 0,
        };
    }
    let left_tail_var = compute_sorted_quantile(&sorted_returns, LEFT_TAIL_QUANTILE_LEVEL);

    let left_tail_values: Vec<f64> = sorted_returns
        .iter()
        .copied()
        .filter(|value| *value <= left_tail_var)
        .collect();

    let tail_observation_count = left_tail_values.len();

    let value = if left_tail_var >= 0.0
        || left_tail_var.abs() <= MINIMUM_DENOMINATOR_MAGNITUDE
        || left_tail_values.is_empty()
    {
        None
    } else {
        let expected_shortfall =
            left_tail_values.iter().sum::<f64>() / left_tail_values.len() as f64;
        let ratio = expected_shortfall.abs() / left_tail_var.abs();
        ratio.is_finite().then_some(ratio)
    };

    TailSkewMetricComputation {
        value,
        effective_sample_count: sorted_returns.len(),
        tail_observation_count,
    }
}

fn compute_bowley_skewness_quantile(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let sorted_returns = sorted_finite_values(interval_returns);

    if sorted_returns.is_empty() {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: 0,
            tail_observation_count: 0,
        };
    }
    let value = bowley_skewness_from_sorted(&sorted_returns, MINIMUM_DENOMINATOR_MAGNITUDE);

    TailSkewMetricComputation {
        value,
        effective_sample_count: sorted_returns.len(),
        tail_observation_count: 0,
    }
}

fn compute_signed_tail_expected_shortfall_imbalance(
    interval_returns: &[f64],
) -> TailSkewMetricComputation {
    let sorted_returns = sorted_finite_values(interval_returns);

    if sorted_returns.is_empty() {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: 0,
            tail_observation_count: 0,
        };
    }
    let left_tail_var = compute_sorted_quantile(&sorted_returns, LEFT_TAIL_QUANTILE_LEVEL);
    let right_tail_var = compute_sorted_quantile(&sorted_returns, RIGHT_TAIL_QUANTILE_LEVEL);

    let left_tail_values: Vec<f64> = sorted_returns
        .iter()
        .copied()
        .filter(|value| *value <= left_tail_var)
        .collect();
    let right_tail_values: Vec<f64> = sorted_returns
        .iter()
        .copied()
        .filter(|value| *value >= right_tail_var)
        .collect();

    let tail_observation_count = left_tail_values
        .len()
        .saturating_add(right_tail_values.len());

    let value = if left_tail_values.is_empty() || right_tail_values.is_empty() {
        None
    } else {
        let left_expected_shortfall_magnitude =
            left_tail_values.iter().sum::<f64>().abs() / left_tail_values.len() as f64;
        let right_expected_shortfall_magnitude =
            right_tail_values.iter().sum::<f64>().abs() / right_tail_values.len() as f64;

        let normalizer = 0.5 * (left_tail_var.abs() + right_tail_var.abs());
        if !normalizer.is_finite() || normalizer <= MINIMUM_DENOMINATOR_MAGNITUDE {
            None
        } else {
            let imbalance = (right_expected_shortfall_magnitude
                - left_expected_shortfall_magnitude)
                / normalizer;
            imbalance.is_finite().then_some(imbalance)
        }
    };

    TailSkewMetricComputation {
        value,
        effective_sample_count: sorted_returns.len(),
        tail_observation_count,
    }
}

fn compute_l_moment_lambda2(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let sorted_returns = sorted_finite_values(interval_returns);

    if sorted_returns.len() < 2 {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: sorted_returns.len(),
            tail_observation_count: 0,
        };
    }
    let value = l_moment_lambda2_from_sorted(&sorted_returns);

    TailSkewMetricComputation {
        value,
        effective_sample_count: sorted_returns.len(),
        tail_observation_count: 0,
    }
}

fn compute_l_moment_tau3(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let sorted_returns = sorted_finite_values(interval_returns);

    if sorted_returns.len() < 3 {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: sorted_returns.len(),
            tail_observation_count: 0,
        };
    }
    let value = l_moment_tau3_from_sorted(&sorted_returns, MINIMUM_DENOMINATOR_MAGNITUDE);

    TailSkewMetricComputation {
        value,
        effective_sample_count: sorted_returns.len(),
        tail_observation_count: 0,
    }
}

fn compute_central_variance(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let value = sample_variance(interval_returns);

    TailSkewMetricComputation {
        value,
        effective_sample_count: interval_returns.len().saturating_sub(1),
        tail_observation_count: 0,
    }
}

fn compute_central_skewness(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let value = sample_adjusted_skewness(interval_returns);

    TailSkewMetricComputation {
        value,
        effective_sample_count: interval_returns.len().saturating_sub(2),
        tail_observation_count: 0,
    }
}

fn compute_central_excess_kurtosis(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let value = sample_adjusted_excess_kurtosis(interval_returns);

    TailSkewMetricComputation {
        value,
        effective_sample_count: interval_returns.len().saturating_sub(3),
        tail_observation_count: 0,
    }
}
