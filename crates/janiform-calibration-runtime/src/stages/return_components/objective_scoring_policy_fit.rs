use janiform_foundations::numeric::{smallest_positive_value, strictly_positive_machine_value};

use super::constants::{
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_ACTIVITY_COUPLING_FIT,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CLOSED_GAP_FIT,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CONTINUOUS_DISTRIBUTION_FIT,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_OPEN_JUMP_FIT,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_PRICE_RANGE_SANITY_FIT,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_SURPRISE_ARRIVAL_FIT,
};
use super::contracts::{
    ReturnComponentsStageActivityCouplingDiagnostic, ReturnComponentsStageGapDiagnostic,
    ReturnComponentsStageJumpDiagnostic, ReturnComponentsStageObjectiveMetricBuckets,
    ReturnComponentsStageObjectiveMetricWeights, ReturnComponentsStageObjectiveScoringPolicy,
    ReturnComponentsStageObjectiveTermInput, ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStagePriceRangeSanityDiagnostic,
    ReturnComponentsStageSurpriseArrivalDiagnostic,
};

const RETURN_COMPONENTS_STAGE_OBJECTIVE_SCORING_POLICY: &str =
    "relative_error_bucket_scoring_with_log_price_range_excess_penalty.v1";
const RETURN_COMPONENTS_STAGE_PRICE_RANGE_EXCESS_TRANSFORM: &str = "ln1p(max(fitted_reference_log_price_span/max(admissible_reference_log_price_span,price_range_threshold_floor)-1,0))";

pub(crate) fn fit_objective_scoring_policy(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> ReturnComponentsStageObjectiveScoringPolicy {
    ReturnComponentsStageObjectiveScoringPolicy {
        fit_policy: RETURN_COMPONENTS_STAGE_OBJECTIVE_SCORING_POLICY,
        arrival_frequency_zero_floor: observed_arrival_frequency_zero_floor(summary_statistics),
        positive_metric_zero_floor: observed_positive_metric_zero_floor(summary_statistics),
        price_range_threshold_floor: strictly_positive_machine_value(),
        price_range_excess_transform: RETURN_COMPONENTS_STAGE_PRICE_RANGE_EXCESS_TRANSFORM,
        dominant_scale_driver_order: vec![
            "nig_scale".to_string(),
            "surprise_arrivals".to_string(),
            "jump_scales".to_string(),
            "gap_behavior".to_string(),
            "activity_coupling".to_string(),
            "price_range_sanity".to_string(),
        ],
    }
}

pub(crate) fn score_candidate_metric_buckets(
    objective_scoring_policy: &ReturnComponentsStageObjectiveScoringPolicy,
    continuous_distribution_fit: f64,
    surprise_arrival_diagnostic: &ReturnComponentsStageSurpriseArrivalDiagnostic,
    jump_diagnostic: &ReturnComponentsStageJumpDiagnostic,
    gap_diagnostic: &ReturnComponentsStageGapDiagnostic,
    activity_coupling_diagnostic: &ReturnComponentsStageActivityCouplingDiagnostic,
    price_range_sanity_diagnostic: &ReturnComponentsStagePriceRangeSanityDiagnostic,
) -> ReturnComponentsStageObjectiveMetricBuckets {
    let surprise_arrival_fit = zero_aware_relative_error(
        surprise_arrival_diagnostic.fitted_surprise_arrival_intensity_per_hour,
        surprise_arrival_diagnostic.observed_jump_frequency_per_hour,
        objective_scoring_policy.arrival_frequency_zero_floor,
    ) + surprise_arrival_diagnostic.surprise_arrival_profile_fit_error;

    let open_jump_fit = zero_aware_relative_error(
        jump_diagnostic.fitted_mean_abs_jump_return,
        jump_diagnostic.observed_mean_abs_jump_return,
        objective_scoring_policy.positive_metric_zero_floor,
    );

    let closed_gap_fit = if gap_diagnostic.observed_gap_count == 0 {
        0.0
    } else {
        zero_aware_relative_error(
            gap_diagnostic.fitted_mean_abs_gap_return,
            gap_diagnostic.observed_mean_abs_gap_return,
            objective_scoring_policy.positive_metric_zero_floor,
        )
    };

    let activity_coupling_fit = zero_aware_relative_error(
        activity_coupling_diagnostic.fitted_known_activity_coupling_scale,
        activity_coupling_diagnostic.observed_known_post_shock_activity_lift,
        objective_scoring_policy.positive_metric_zero_floor,
    ) + zero_aware_relative_error(
        activity_coupling_diagnostic.fitted_surprise_activity_coupling_scale,
        activity_coupling_diagnostic.observed_surprise_post_shock_activity_lift,
        objective_scoring_policy.positive_metric_zero_floor,
    );

    ReturnComponentsStageObjectiveMetricBuckets {
        continuous_distribution_fit: round_non_negative_value(continuous_distribution_fit),
        surprise_arrival_fit: round_non_negative_value(surprise_arrival_fit),
        open_jump_fit: round_non_negative_value(open_jump_fit),
        closed_gap_fit: round_non_negative_value(closed_gap_fit),
        activity_coupling_fit: round_non_negative_value(activity_coupling_fit),
        price_range_sanity_fit: round_non_negative_value(
            if price_range_sanity_diagnostic.is_flagged {
                let normalized_excess = (price_range_sanity_diagnostic
                    .fitted_reference_log_price_span
                    / price_range_sanity_diagnostic
                        .admissible_reference_log_price_span
                        .max(objective_scoring_policy.price_range_threshold_floor))
                    - 1.0;
                normalized_excess.max(0.0).ln_1p()
            } else {
                0.0
            },
        ),
    }
}

pub(crate) fn dominant_scale_driver_from_metric_buckets(
    objective_scoring_policy: &ReturnComponentsStageObjectiveScoringPolicy,
    objective_metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
) -> String {
    let mut dominant_driver = objective_scoring_policy
        .dominant_scale_driver_order
        .first()
        .cloned()
        .unwrap_or_else(|| "nig_scale".to_string());
    let mut dominant_value =
        bucket_value_for_driver(objective_metric_buckets, dominant_driver.as_str());

    for driver in objective_scoring_policy
        .dominant_scale_driver_order
        .iter()
        .skip(1)
    {
        let value = bucket_value_for_driver(objective_metric_buckets, driver.as_str());
        if value > dominant_value {
            dominant_driver = driver.clone();
            dominant_value = value;
        }
    }

    dominant_driver
}

pub(crate) fn build_candidate_objective_terms(
    objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights,
    objective_metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
) -> Vec<ReturnComponentsStageObjectiveTermInput> {
    vec![
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CONTINUOUS_DISTRIBUTION_FIT.to_string(),
            weight: objective_metric_weights.continuous_distribution_fit_weight,
            observed_score: objective_metric_buckets.continuous_distribution_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_SURPRISE_ARRIVAL_FIT.to_string(),
            weight: objective_metric_weights.surprise_arrival_fit_weight,
            observed_score: objective_metric_buckets.surprise_arrival_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_OPEN_JUMP_FIT.to_string(),
            weight: objective_metric_weights.jump_fit_weight,
            observed_score: objective_metric_buckets.open_jump_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CLOSED_GAP_FIT.to_string(),
            weight: objective_metric_weights.gap_fit_weight,
            observed_score: objective_metric_buckets.closed_gap_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_ACTIVITY_COUPLING_FIT.to_string(),
            weight: objective_metric_weights.activity_coupling_fit_weight,
            observed_score: objective_metric_buckets.activity_coupling_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_PRICE_RANGE_SANITY_FIT.to_string(),
            weight: objective_metric_weights.price_range_sanity_fit_weight,
            observed_score: objective_metric_buckets.price_range_sanity_fit,
        },
    ]
}

pub(crate) fn weighted_objective_value(
    objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights,
    objective_metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
) -> f64 {
    objective_metric_weights.continuous_distribution_fit_weight
        * objective_metric_buckets.continuous_distribution_fit
        + objective_metric_weights.surprise_arrival_fit_weight
            * objective_metric_buckets.surprise_arrival_fit
        + objective_metric_weights.jump_fit_weight * objective_metric_buckets.open_jump_fit
        + objective_metric_weights.gap_fit_weight * objective_metric_buckets.closed_gap_fit
        + objective_metric_weights.activity_coupling_fit_weight
            * objective_metric_buckets.activity_coupling_fit
        + objective_metric_weights.price_range_sanity_fit_weight
            * objective_metric_buckets.price_range_sanity_fit
}

fn bucket_value_for_driver(
    objective_metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
    driver: &str,
) -> f64 {
    match driver {
        "surprise_arrivals" => objective_metric_buckets.surprise_arrival_fit,
        "jump_scales" => objective_metric_buckets.open_jump_fit,
        "gap_behavior" => objective_metric_buckets.closed_gap_fit,
        "activity_coupling" => objective_metric_buckets.activity_coupling_fit,
        "price_range_sanity" => objective_metric_buckets.price_range_sanity_fit,
        _ => objective_metric_buckets.continuous_distribution_fit,
    }
}

fn zero_aware_relative_error(observed: f64, target: f64, zero_floor: f64) -> f64 {
    let denominator = if target.abs() <= zero_floor {
        zero_floor
    } else {
        target.abs()
    };

    ((observed - target).abs() / denominator).max(0.0)
}

fn observed_arrival_frequency_zero_floor(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> f64 {
    if summary_statistics.source_window_duration_hours > 0.0 {
        1.0 / summary_statistics.source_window_duration_hours
    } else {
        strictly_positive_machine_value()
    }
}

fn observed_positive_metric_zero_floor(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> f64 {
    smallest_positive_value(
        [
            summary_statistics.mean_abs_known_open_jump_return,
            summary_statistics.mean_abs_known_closed_gap_return,
            summary_statistics.mean_abs_surprise_open_jump_return,
            summary_statistics.mean_abs_surprise_closed_gap_return,
            summary_statistics.mean_known_post_shock_activity_lift,
            summary_statistics.mean_surprise_post_shock_activity_lift,
            summary_statistics.known_activity_coupling_target,
            summary_statistics.surprise_activity_coupling_target,
        ]
        .as_slice(),
    )
    .unwrap_or_else(strictly_positive_machine_value)
}

fn round_non_negative_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    let rounded = (value * 1_000_000_000_000.0).round() / 1_000_000_000_000.0;
    if rounded < 0.0 { 0.0 } else { rounded }
}

#[cfg(test)]
mod tests {
    use janiform_foundations::numeric::strictly_positive_machine_value;

    use super::fit_objective_scoring_policy;
    use crate::stages::return_components::ReturnComponentsStageObservedSignalSummaryStatistics;

    #[test]
    fn fitted_objective_scoring_policy_uses_source_window_and_observed_metric_floors() {
        let policy =
            fit_objective_scoring_policy(ReturnComponentsStageObservedSignalSummaryStatistics {
                known_calendar_event_count: 1,
                known_realized_sample_count: 1,
                source_window_duration_hours: 12.0,
                observed_open_session_return_count: 4,
                continuous_fit_return_count: 4,
                intraday_jump_candidate_count: 1,
                intraday_jump_episode_count: 1,
                selected_surprise_jump_episode_count: 1,
                close_open_gap_candidate_count: 1,
                close_open_gap_episode_count: 1,
                open_session_sell_share_proxy: 0.5,
                mean_continuous_business_time_scale: 1.0,
                continuous_return_mean: 0.0,
                continuous_return_volatility: 0.01,
                continuous_return_skewness: 0.0,
                continuous_return_excess_kurtosis: 0.0,
                observed_continuous_log_price_span: 0.02,
                observed_log_price_span: 0.03,
                mean_abs_known_open_jump_return: 0.0015,
                mean_abs_known_closed_gap_return: 0.004,
                mean_abs_surprise_open_jump_return: 0.002,
                mean_abs_surprise_closed_gap_return: 0.003,
                mean_known_post_shock_activity_lift: 1.2,
                mean_surprise_post_shock_activity_lift: 1.1,
                known_activity_coupling_target: 0.015,
                surprise_activity_coupling_target: 0.025,
                known_news_coverage_ratio: 1.0,
                surprise_arrival_intensity_per_hour: 0.25,
            });

        assert_eq!(policy.arrival_frequency_zero_floor, 1.0 / 12.0);
        assert_eq!(policy.positive_metric_zero_floor, 0.0015);
        assert_eq!(
            policy.price_range_threshold_floor,
            strictly_positive_machine_value()
        );
        assert_eq!(
            policy.dominant_scale_driver_order,
            vec![
                "nig_scale".to_string(),
                "surprise_arrivals".to_string(),
                "jump_scales".to_string(),
                "gap_behavior".to_string(),
                "activity_coupling".to_string(),
                "price_range_sanity".to_string(),
            ]
        );
    }
}
