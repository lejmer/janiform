use janiform_foundations::numeric::round_to;
use janiform_model_kernels::news::SharedNewsMarkLawConfiguration;

use super::contracts::ReturnComponentsStageObservedSignalSummaryStatistics;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FittedReturnJumpMarkLayer {
    pub(crate) known_return_jump_drift: f64,
    pub(crate) known_return_jump_volatility: f64,
    pub(crate) surprise_return_jump_drift: f64,
    pub(crate) surprise_return_jump_volatility: f64,
}

pub(crate) fn fit_return_jump_mark_layer(
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> FittedReturnJumpMarkLayer {
    let known_evidence_count = summary_statistics.known_realized_sample_count;
    let surprise_evidence_count = summary_statistics.selected_surprise_jump_episode_count
        + summary_statistics.close_open_gap_episode_count;

    if known_evidence_count == 0 && surprise_evidence_count == 0 {
        return FittedReturnJumpMarkLayer::neutral();
    }

    let expected_abs_unit_return_shock =
        expected_absolute_unit_return_shock(shared_mark_law_template);

    FittedReturnJumpMarkLayer {
        known_return_jump_drift: 0.0,
        known_return_jump_volatility: if summary_statistics.known_activity_coupling_target > 0.0 {
            fitted_return_jump_volatility(
                weighted_mean_abs_return(
                    summary_statistics.known_realized_sample_count,
                    summary_statistics.mean_abs_known_open_jump_return,
                    summary_statistics.known_realized_sample_count,
                    summary_statistics.mean_abs_known_closed_gap_return,
                ),
                expected_abs_unit_return_shock,
                known_evidence_count,
            )
        } else {
            0.0
        },
        surprise_return_jump_drift: 0.0,
        surprise_return_jump_volatility: if summary_statistics.surprise_activity_coupling_target
            > 0.0
        {
            fitted_return_jump_volatility(
                weighted_mean_abs_return(
                    summary_statistics.selected_surprise_jump_episode_count,
                    summary_statistics.mean_abs_surprise_open_jump_return,
                    summary_statistics.close_open_gap_episode_count,
                    summary_statistics.mean_abs_surprise_closed_gap_return,
                ),
                expected_abs_unit_return_shock,
                surprise_evidence_count,
            )
        } else {
            0.0
        },
    }
}

impl FittedReturnJumpMarkLayer {
    pub(crate) const fn neutral() -> Self {
        Self {
            known_return_jump_drift: 0.0,
            known_return_jump_volatility: 0.0,
            surprise_return_jump_drift: 0.0,
            surprise_return_jump_volatility: 0.0,
        }
    }
}

fn expected_absolute_unit_return_shock(
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> f64 {
    let expected_square_root_severity = shared_mark_law_template
        .gamma_severity_marks
        .expected_square_root_severity_mark()
        .unwrap_or(1.0);

    expected_square_root_severity * (2.0 / std::f64::consts::PI).sqrt()
}

fn fitted_return_jump_volatility(
    observed_mean_abs_return: f64,
    expected_abs_unit_return_shock: f64,
    evidence_count: usize,
) -> f64 {
    if evidence_count == 0
        || !observed_mean_abs_return.is_finite()
        || observed_mean_abs_return <= 0.0
        || !expected_abs_unit_return_shock.is_finite()
        || expected_abs_unit_return_shock <= 0.0
    {
        return 0.0;
    }

    round_candidate_value(observed_mean_abs_return / expected_abs_unit_return_shock)
}

fn weighted_mean_abs_return(
    first_count: usize,
    first_mean_abs_return: f64,
    second_count: usize,
    second_mean_abs_return: f64,
) -> f64 {
    let total_count = first_count + second_count;
    if total_count == 0 {
        return 0.0;
    }

    let weighted_sum =
        first_count as f64 * first_mean_abs_return + second_count as f64 * second_mean_abs_return;

    weighted_sum / total_count as f64
}

fn round_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    round_to(value.max(0.0), 12)
}

#[cfg(test)]
mod tests {
    use janiform_model_kernels::news::{
        GammaSeverityMarkConfiguration, SharedNewsMarkLawConfiguration,
    };

    use super::{FittedReturnJumpMarkLayer, fit_return_jump_mark_layer};
    use crate::stages::return_components::{
        ReturnComponentsStageObservedSignalSummaryStatistics, test_support::test_summary_statistics,
    };

    #[test]
    fn absent_shock_evidence_collapses_mark_layer_to_zero() {
        let fitted_layer = fit_return_jump_mark_layer(
            test_shared_mark_law_template(),
            ReturnComponentsStageObservedSignalSummaryStatistics {
                selected_surprise_jump_episode_count: 0,
                close_open_gap_episode_count: 0,
                ..test_summary_statistics()
            },
        );

        assert_eq!(fitted_layer, FittedReturnJumpMarkLayer::neutral());
    }

    #[test]
    fn any_shock_evidence_fits_positive_gamma_normal_volatility() {
        let fitted_layer = fit_return_jump_mark_layer(
            test_shared_mark_law_template(),
            ReturnComponentsStageObservedSignalSummaryStatistics {
                selected_surprise_jump_episode_count: 1,
                mean_abs_surprise_open_jump_return: 0.01,
                surprise_activity_coupling_target: 0.5,
                ..test_summary_statistics()
            },
        );

        assert_eq!(fitted_layer.known_return_jump_drift, 0.0);
        assert_eq!(fitted_layer.surprise_return_jump_drift, 0.0);
        assert!(fitted_layer.surprise_return_jump_volatility > 0.0);
    }

    fn test_shared_mark_law_template() -> SharedNewsMarkLawConfiguration {
        SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration { shape_k: 2.0 },
        }
    }
}
