use janiform_foundations::numeric::round_to;
use janiform_model_kernels::news::SharedNewsMarkLawConfiguration;

use super::constants::RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT;
use super::contracts::ReturnComponentsStageObservedSignalSummaryStatistics;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FittedReturnJumpMarkLayer {
    pub(crate) sigma_g: f64,
}

pub(crate) fn fit_return_jump_mark_layer(
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> FittedReturnJumpMarkLayer {
    let jump_evidence_count = summary_statistics.known_realized_sample_count
        + summary_statistics.selected_surprise_jump_episode_count;
    let gap_evidence_count = summary_statistics.close_open_gap_episode_count;

    if jump_evidence_count < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        && gap_evidence_count < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
    {
        return FittedReturnJumpMarkLayer { sigma_g: 0.0 };
    }

    FittedReturnJumpMarkLayer {
        sigma_g: round_candidate_value(shared_mark_law_template.coupled_gap_marks.sigma_g),
    }
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
        CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
        GammaSeverityMarkConfiguration, SharedNewsMarkLawConfiguration,
    };

    use super::{FittedReturnJumpMarkLayer, fit_return_jump_mark_layer};
    use crate::stages::return_components::{
        ReturnComponentsStageObservedSignalSummaryStatistics, test_support::test_summary_statistics,
    };

    #[test]
    fn sparse_shock_evidence_collapses_mark_layer_to_zero() {
        let fitted_layer = fit_return_jump_mark_layer(
            test_shared_mark_law_template(),
            ReturnComponentsStageObservedSignalSummaryStatistics {
                known_realized_sample_count: 1,
                selected_surprise_jump_episode_count: 0,
                close_open_gap_episode_count: 1,
                ..test_summary_statistics()
            },
        );

        assert_eq!(fitted_layer.sigma_g, 0.0);
    }

    #[test]
    fn sufficient_shock_evidence_keeps_template_sigma_g() {
        let fitted_layer = fit_return_jump_mark_layer(
            test_shared_mark_law_template(),
            ReturnComponentsStageObservedSignalSummaryStatistics {
                selected_surprise_jump_episode_count: 2,
                ..test_summary_statistics()
            },
        );

        assert_eq!(
            fitted_layer,
            FittedReturnJumpMarkLayer {
                sigma_g: 0.001234567891
            }
        );
    }

    fn test_shared_mark_law_template() -> SharedNewsMarkLawConfiguration {
        SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration {
                shape_k: 2.0,
                scale_theta: 1.0e-4,
            },
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
                probability_positive: 0.5,
                eta_plus: 8.0,
                eta_minus: 10.0,
            },
            coupled_gap_marks: CoupledGapMarkConfiguration {
                sigma_g: 0.001234567891234,
            },
        }
    }
}
