use janiform_foundations::numeric::round_to;

use super::constants::RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT;
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
        known_activity_coupling_scale: round_candidate_value(
            summary_statistics.known_activity_coupling_target,
        ),
        surprise_activity_coupling_scale: if summary_statistics.selected_surprise_jump_episode_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
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
    use crate::stages::return_components::ReturnComponentsStageObservedSignalSummaryStatistics;

    #[test]
    fn sparse_surprise_evidence_collapses_surprise_coupling_to_zero() {
        let fitted_layer =
            fit_activity_coupling_layer(ReturnComponentsStageObservedSignalSummaryStatistics {
                known_activity_coupling_target: 0.125432198765,
                surprise_activity_coupling_target: 0.875,
                selected_surprise_jump_episode_count: 1,
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
    fn sufficient_surprise_evidence_keeps_both_targets() {
        let fitted_layer =
            fit_activity_coupling_layer(ReturnComponentsStageObservedSignalSummaryStatistics {
                known_activity_coupling_target: 0.25,
                surprise_activity_coupling_target: 0.75,
                selected_surprise_jump_episode_count: 2,
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

    fn test_summary_statistics() -> ReturnComponentsStageObservedSignalSummaryStatistics {
        ReturnComponentsStageObservedSignalSummaryStatistics {
            known_calendar_event_count: 0,
            known_realized_sample_count: 0,
            source_window_duration_hours: 1.0,
            observed_open_session_return_count: 0,
            continuous_fit_return_count: 0,
            intraday_jump_candidate_count: 0,
            intraday_jump_episode_count: 0,
            selected_surprise_jump_episode_count: 0,
            close_open_gap_candidate_count: 0,
            close_open_gap_episode_count: 0,
            open_session_sell_share_proxy: 0.5,
            mean_continuous_business_time_scale: 1.0,
            continuous_return_mean: 0.0,
            continuous_return_volatility: 0.0,
            continuous_return_skewness: 0.0,
            continuous_return_excess_kurtosis: 0.0,
            observed_log_price_span: 0.0,
            mean_abs_known_open_jump_return: 0.0,
            mean_abs_known_closed_gap_return: 0.0,
            mean_abs_surprise_open_jump_return: 0.0,
            mean_abs_surprise_closed_gap_return: 0.0,
            mean_known_post_shock_activity_lift: 0.0,
            mean_surprise_post_shock_activity_lift: 0.0,
            known_activity_coupling_target: 0.0,
            surprise_activity_coupling_target: 0.0,
            known_news_coverage_ratio: 0.0,
            surprise_arrival_intensity_per_hour: 0.0,
        }
    }
}
