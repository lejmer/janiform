use janiform_foundations::numeric::round_to;

use super::contracts::ReturnComponentsStageQuantileBinningPolicy;

const RETURN_COMPONENTS_STAGE_JUMP_BIN_QUANTILES: [f64; 3] = [0.25, 0.5, 0.9];
const RETURN_COMPONENTS_STAGE_GAP_BIN_QUANTILES: [f64; 3] = [0.25, 0.5, 0.9];
const RETURN_COMPONENTS_STAGE_QUANTILE_BINNING_POLICY: &str =
    "deterministic_nearest_rank_quantile_binning.v1";

pub(crate) fn fit_quantile_binning_policy(
    jump_abs_returns: &[f64],
    gap_abs_returns: &[f64],
) -> ReturnComponentsStageQuantileBinningPolicy {
    ReturnComponentsStageQuantileBinningPolicy {
        fit_policy: RETURN_COMPONENTS_STAGE_QUANTILE_BINNING_POLICY,
        jump_bin_quantiles: RETURN_COMPONENTS_STAGE_JUMP_BIN_QUANTILES.to_vec(),
        gap_bin_quantiles: RETURN_COMPONENTS_STAGE_GAP_BIN_QUANTILES.to_vec(),
        jump_abs_return_bin_edges: deterministic_quantile_values(
            jump_abs_returns,
            &RETURN_COMPONENTS_STAGE_JUMP_BIN_QUANTILES,
        ),
        gap_abs_return_bin_edges: deterministic_quantile_values(
            gap_abs_returns,
            &RETURN_COMPONENTS_STAGE_GAP_BIN_QUANTILES,
        ),
    }
}

pub(crate) fn deterministic_quantile_values(values: &[f64], quantiles: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return vec![0.0];
    }

    let mut sorted_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .collect();
    sorted_values.sort_by(f64::total_cmp);

    if sorted_values.is_empty() {
        return vec![0.0];
    }

    let mut bins = Vec::with_capacity(quantiles.len());
    for quantile in quantiles {
        let clamped_quantile = quantile.clamp(0.0, 1.0);
        let quantile_position =
            (clamped_quantile * (sorted_values.len().saturating_sub(1)) as f64).round() as usize;
        bins.push(round_non_negative_candidate_value(
            sorted_values[quantile_position],
        ));
    }

    deduplicate_sorted_f64_values(bins)
}

fn round_non_negative_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    round_to(value.max(0.0), 12)
}

fn deduplicate_sorted_f64_values(mut values: Vec<f64>) -> Vec<f64> {
    values.retain(|value| value.is_finite() && *value >= 0.0);
    values.sort_by(f64::total_cmp);
    values.dedup_by(|left, right| (*left - *right).abs() <= 1e-12);
    values
}

#[cfg(test)]
mod tests {
    use super::{deterministic_quantile_values, fit_quantile_binning_policy};

    #[test]
    fn deterministic_quantile_values_returns_zero_for_empty_input() {
        assert_eq!(deterministic_quantile_values(&[], &[0.25, 0.5]), vec![0.0]);
    }

    #[test]
    fn fitted_policy_records_quantiles_and_edges() {
        let policy = fit_quantile_binning_policy(&[0.1, 0.2, 0.3, 0.4], &[0.5, 0.6, 0.7, 0.8]);

        assert_eq!(policy.jump_bin_quantiles, vec![0.25, 0.5, 0.9]);
        assert_eq!(policy.gap_bin_quantiles, vec![0.25, 0.5, 0.9]);
        assert_eq!(policy.jump_abs_return_bin_edges, vec![0.2, 0.3, 0.4]);
        assert_eq!(policy.gap_abs_return_bin_edges, vec![0.6, 0.7, 0.8]);
    }
}
