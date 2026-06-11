use janiform_foundations::numeric::round_to;

use super::contracts::ReturnComponentsStageObservedSignalSummaryStatistics;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FittedActivityCouplingLayer {
    pub(crate) known_activity_coupling_scale: f64,
    pub(crate) surprise_activity_coupling_scale: f64,
}

pub(crate) fn fit_activity_coupling_layer(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> FittedActivityCouplingLayer {
    FittedActivityCouplingLayer {
        known_activity_coupling_scale: if summary_statistics.known_realized_sample_count == 0 {
            0.0
        } else {
            round_candidate_value(summary_statistics.known_activity_coupling_target)
        },
        surprise_activity_coupling_scale: if summary_statistics.selected_surprise_jump_episode_count
            == 0
        {
            0.0
        } else {
            round_candidate_value(summary_statistics.surprise_activity_coupling_target)
        },
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
    use super::{FittedActivityCouplingLayer, fit_activity_coupling_layer};
    use crate::stages::return_components::{
        ReturnComponentsStageObservedSignalSummaryStatistics, test_support::test_summary_statistics,
    };

    #[test]
    fn absent_surprise_evidence_collapses_surprise_coupling_to_zero() {
        let fitted_layer =
            fit_activity_coupling_layer(ReturnComponentsStageObservedSignalSummaryStatistics {
                known_realized_sample_count: 1,
                known_activity_coupling_target: 0.125432198765,
                surprise_activity_coupling_target: 0.875,
                selected_surprise_jump_episode_count: 0,
                ..test_summary_statistics()
            });

        assert_eq!(
            fitted_layer,
            FittedActivityCouplingLayer {
                known_activity_coupling_scale: 0.125432198765,
                surprise_activity_coupling_scale: 0.0,
            }
        );
    }

    #[test]
    fn absent_known_evidence_collapses_known_coupling_to_zero() {
        let fitted_layer =
            fit_activity_coupling_layer(ReturnComponentsStageObservedSignalSummaryStatistics {
                known_activity_coupling_target: 0.25,
                surprise_activity_coupling_target: 0.75,
                selected_surprise_jump_episode_count: 1,
                ..test_summary_statistics()
            });

        assert_eq!(
            fitted_layer,
            FittedActivityCouplingLayer {
                known_activity_coupling_scale: 0.0,
                surprise_activity_coupling_scale: 0.75,
            }
        );
    }

    #[test]
    fn any_surprise_evidence_keeps_both_targets() {
        let fitted_layer =
            fit_activity_coupling_layer(ReturnComponentsStageObservedSignalSummaryStatistics {
                known_realized_sample_count: 1,
                known_activity_coupling_target: 0.25,
                surprise_activity_coupling_target: 0.75,
                selected_surprise_jump_episode_count: 1,
                ..test_summary_statistics()
            });

        assert_eq!(
            fitted_layer,
            FittedActivityCouplingLayer {
                known_activity_coupling_scale: 0.25,
                surprise_activity_coupling_scale: 0.75,
            }
        );
    }
}
