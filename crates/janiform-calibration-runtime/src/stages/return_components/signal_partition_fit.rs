use super::contracts::{
    ReturnComponentsDetectedIntradayJumpCandidate, ReturnComponentsObservedOpenSessionReturn,
    ReturnComponentsRealizedKnownNewsReturnSample,
};

const RETURN_COMPONENTS_STAGE_CONTINUOUS_RETURN_FILTER_POLICY: &str = "maximum_explicit_shock_magnitude_or_widest_log_gap_midpoint_in_positive_absolute_open_return_spectrum.v1";

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FittedSignalPartitionLayer {
    pub(crate) continuous_return_filter_policy: &'static str,
    pub(crate) continuous_return_filter_threshold: f64,
}

pub(crate) fn fit_signal_partition_layer(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    realized_known_news_return_samples: &[ReturnComponentsRealizedKnownNewsReturnSample],
    detected_intraday_jump_candidates: &[ReturnComponentsDetectedIntradayJumpCandidate],
) -> FittedSignalPartitionLayer {
    FittedSignalPartitionLayer {
        continuous_return_filter_policy: RETURN_COMPONENTS_STAGE_CONTINUOUS_RETURN_FILTER_POLICY,
        continuous_return_filter_threshold: fit_continuous_return_filter_threshold(
            observed_open_session_returns,
            realized_known_news_return_samples,
            detected_intraday_jump_candidates,
        ),
    }
}

fn fit_continuous_return_filter_threshold(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    realized_known_news_return_samples: &[ReturnComponentsRealizedKnownNewsReturnSample],
    detected_intraday_jump_candidates: &[ReturnComponentsDetectedIntradayJumpCandidate],
) -> f64 {
    let explicit_shock_ceiling = maximum_explicit_shock_magnitude(
        realized_known_news_return_samples,
        detected_intraday_jump_candidates,
    );

    let mut sorted_positive_absolute_returns: Vec<f64> = observed_open_session_returns
        .iter()
        .filter_map(|observation| {
            let absolute_return = observation.log_return.abs();
            (absolute_return.is_finite() && absolute_return > 0.0).then_some(absolute_return)
        })
        .collect();
    if sorted_positive_absolute_returns.is_empty() {
        return 0.0;
    }

    sorted_positive_absolute_returns.sort_by(f64::total_cmp);

    let fallback_threshold = sorted_positive_absolute_returns[0];

    sorted_positive_absolute_returns
        .dedup_by(|left_value, right_value| *left_value == *right_value);
    let spectrum_threshold = if sorted_positive_absolute_returns.len() < 2 {
        fallback_threshold
    } else {
        let mut widest_gap_pair = (
            sorted_positive_absolute_returns[0],
            sorted_positive_absolute_returns[1],
        );

        let mut widest_log_gap = f64::NEG_INFINITY;

        for return_pair in sorted_positive_absolute_returns.windows(2) {
            let log_gap = (return_pair[1] / return_pair[0]).ln();
            if log_gap > widest_log_gap {
                widest_gap_pair = (return_pair[0], return_pair[1]);
                widest_log_gap = log_gap;
            }
        }

        let threshold = (widest_gap_pair.0 * widest_gap_pair.1).sqrt();
        if threshold.is_finite() && threshold > 0.0 {
            threshold
        } else {
            fallback_threshold
        }
    };

    if let Some(explicit_shock_ceiling) = explicit_shock_ceiling {
        spectrum_threshold.max(explicit_shock_ceiling)
    } else {
        spectrum_threshold
    }
}

fn maximum_explicit_shock_magnitude(
    realized_known_news_return_samples: &[ReturnComponentsRealizedKnownNewsReturnSample],
    detected_intraday_jump_candidates: &[ReturnComponentsDetectedIntradayJumpCandidate],
) -> Option<f64> {
    realized_known_news_return_samples
        .iter()
        .map(|sample| sample.open_window_jump_return.abs())
        .chain(
            detected_intraday_jump_candidates
                .iter()
                .map(|candidate| candidate.jump_return.abs()),
        )
        .filter(|absolute_return| absolute_return.is_finite() && *absolute_return > 0.0)
        .max_by(f64::total_cmp)
}

#[cfg(test)]
mod tests {
    use super::{fit_continuous_return_filter_threshold, fit_signal_partition_layer};
    use crate::stages::return_components::{
        ReturnComponentsDetectedIntradayJumpCandidate, ReturnComponentsObservedOpenSessionReturn,
        ReturnComponentsRealizedKnownNewsReturnSample,
    };

    #[test]
    fn continuous_return_filter_threshold_is_raised_to_maximum_explicit_shock_magnitude() {
        let threshold = fit_continuous_return_filter_threshold(
            &[
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 1,
                    log_return: 1.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 2,
                    log_return: -2.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 3,
                    log_return: 3.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 4,
                    log_return: 5.0e-3,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 5,
                    log_return: 8.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 6,
                    log_return: 6.0e-3,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
            ],
            &[ReturnComponentsRealizedKnownNewsReturnSample {
                event_timestamp_utc_nanos: 10,
                attributed_open_return_timestamp_utc_nanos: Some(4),
                open_window_jump_return: 9.0e-4,
                closed_window_gap_return: 0.0,
                post_shock_activity_lift: 1.0,
            }],
            &[ReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: 4,
                jump_return: 5.0e-3,
                post_shock_activity_lift: 1.0,
            }],
        );

        assert!((threshold - 5.0e-3).abs() <= 1.0e-12);
    }

    #[test]
    fn continuous_return_filter_threshold_uses_widest_positive_gap_midpoint_without_shocks() {
        let threshold = fit_continuous_return_filter_threshold(
            &[
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 1,
                    log_return: 1.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 2,
                    log_return: -2.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 3,
                    log_return: 3.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 4,
                    log_return: 5.0e-3,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
            ],
            &[],
            &[],
        );

        assert!((threshold - (3.0e-4_f64 * 5.0e-3_f64).sqrt()).abs() <= 1.0e-12);
    }

    #[test]
    fn fitted_signal_partition_layer_records_derived_filter_policy() {
        let fitted_layer = fit_signal_partition_layer(&[], &[], &[]);

        assert_eq!(
            fitted_layer.continuous_return_filter_policy,
            "maximum_explicit_shock_magnitude_or_widest_log_gap_midpoint_in_positive_absolute_open_return_spectrum.v1"
        );
        assert_eq!(fitted_layer.continuous_return_filter_threshold, 0.0);
    }
}
