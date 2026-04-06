use janiform_foundations::numeric::representable_positive_f64_log_span;

use super::contracts::{
    ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStagePriceRangeSanityPolicy,
};

const RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_POLICY: &str = "observed_continuous_path_geometry_plus_observed_component_reference_bounded_by_representable_positive_price_domain.v1";

pub(crate) fn fit_price_range_sanity_policy(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> ReturnComponentsStagePriceRangeSanityPolicy {
    ReturnComponentsStagePriceRangeSanityPolicy {
        fit_policy: RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_POLICY,
        observed_continuous_log_price_span: summary_statistics.observed_continuous_log_price_span,
        observed_component_reference_log_price_span: summary_statistics
            .observed_continuous_log_price_span
            + summary_statistics.known_realized_sample_count as f64
                * summary_statistics.mean_abs_known_open_jump_return
            + summary_statistics.selected_surprise_jump_episode_count as f64
                * summary_statistics.mean_abs_surprise_open_jump_return
            + summary_statistics.close_open_gap_candidate_count as f64
                * summary_statistics.mean_abs_surprise_closed_gap_return,
        maximum_representable_log_price_span: representable_positive_f64_log_span(),
    }
}

#[cfg(test)]
mod tests {
    use janiform_foundations::numeric::representable_positive_f64_log_span;

    use super::fit_price_range_sanity_policy;
    use crate::stages::return_components::test_support::test_summary_statistics;

    #[test]
    fn fitted_policy_uses_observed_geometry_and_machine_domain_limits() {
        let policy = fit_price_range_sanity_policy(
            crate::stages::return_components::ReturnComponentsStageObservedSignalSummaryStatistics {
                observed_continuous_log_price_span: 0.018,
                known_realized_sample_count: 2,
                mean_abs_known_open_jump_return: 0.0015,
                selected_surprise_jump_episode_count: 3,
                mean_abs_surprise_open_jump_return: 0.002,
                close_open_gap_candidate_count: 1,
                mean_abs_surprise_closed_gap_return: 0.004,
                ..test_summary_statistics()
            },
        );

        assert_eq!(policy.observed_continuous_log_price_span, 0.018);
        assert_eq!(
            policy.observed_component_reference_log_price_span,
            0.018 + 2.0 * 0.0015 + 3.0 * 0.002 + 0.004
        );
        assert_eq!(
            policy.maximum_representable_log_price_span,
            representable_positive_f64_log_span()
        );
    }
}
