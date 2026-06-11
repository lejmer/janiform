use super::contracts::{
    ReturnComponentsDetectedIntradayJumpCandidate, ReturnComponentsObservedOpenSessionReturn,
    ReturnComponentsRealizedKnownNewsReturnSample,
};
use janiform_calibration_artifacts::TickGridObservationModel;

const RETURN_COMPONENTS_STAGE_CONTINUOUS_RETURN_FILTER_POLICY: &str = "maximum_explicit_shock_magnitude_or_nearest_tick_grid_adjusted_widest_log_gap_midpoint_in_positive_absolute_open_return_spectrum.v1";
const MINIMUM_TICK_GRID_CONTINUOUS_THRESHOLD_MULTIPLE: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FittedSignalPartitionLayer {
    pub(crate) continuous_return_filter_policy: &'static str,
    pub(crate) continuous_return_filter_threshold: f64,
}

pub(crate) fn fit_signal_partition_layer(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    realized_known_news_return_samples: &[ReturnComponentsRealizedKnownNewsReturnSample],
    detected_intraday_jump_candidates: &[ReturnComponentsDetectedIntradayJumpCandidate],
    tick_grid_observation_model: &TickGridObservationModel,
) -> FittedSignalPartitionLayer {
    FittedSignalPartitionLayer {
        continuous_return_filter_policy: RETURN_COMPONENTS_STAGE_CONTINUOUS_RETURN_FILTER_POLICY,
        continuous_return_filter_threshold: fit_continuous_return_filter_threshold(
            observed_open_session_returns,
            realized_known_news_return_samples,
            detected_intraday_jump_candidates,
            tick_grid_observation_model,
        ),
    }
}

fn fit_continuous_return_filter_threshold(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    realized_known_news_return_samples: &[ReturnComponentsRealizedKnownNewsReturnSample],
    detected_intraday_jump_candidates: &[ReturnComponentsDetectedIntradayJumpCandidate],
    tick_grid_observation_model: &TickGridObservationModel,
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

    let tick_grid_adjusted_spectrum_threshold =
        tick_grid_adjusted_continuous_threshold(spectrum_threshold, tick_grid_observation_model);

    if let Some(explicit_shock_ceiling) = explicit_shock_ceiling {
        tick_grid_adjusted_spectrum_threshold.max(explicit_shock_ceiling)
    } else {
        tick_grid_adjusted_spectrum_threshold
    }
}

fn tick_grid_adjusted_continuous_threshold(
    spectrum_threshold: f64,
    tick_grid_observation_model: &TickGridObservationModel,
) -> f64 {
    let Some(tick_size) = tick_grid_observation_model.tick_size else {
        return spectrum_threshold;
    };

    if !spectrum_threshold.is_finite()
        || spectrum_threshold <= 0.0
        || !tick_size.is_finite()
        || tick_size <= 0.0
    {
        return spectrum_threshold;
    }

    let tick_multiple = (spectrum_threshold / tick_size)
        .round()
        .max(MINIMUM_TICK_GRID_CONTINUOUS_THRESHOLD_MULTIPLE);

    let adjusted_threshold = tick_multiple * tick_size;
    if adjusted_threshold.is_finite() && adjusted_threshold > 0.0 {
        adjusted_threshold
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
    use janiform_calibration_artifacts::TickGridObservationModel;

    fn disabled_tick_grid_observation_model() -> TickGridObservationModel {
        TickGridObservationModel {
            tick_size: None,
            tick_size_source: "unavailable".to_string(),
            inference_policy: "not_inferred_for_signal_partition_tests.v1".to_string(),
            observation_policy: "disabled_without_tick_size.v1".to_string(),
        }
    }

    #[test]
    fn continuous_return_filter_threshold_is_raised_to_maximum_explicit_shock_magnitude() {
        let threshold = fit_continuous_return_filter_threshold(
            &[
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 1,
                    elapsed_same_session_hours: 1.0,
                    log_return: 1.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 2,
                    elapsed_same_session_hours: 1.0,
                    log_return: -2.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 3,
                    elapsed_same_session_hours: 1.0,
                    log_return: 3.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 4,
                    elapsed_same_session_hours: 1.0,
                    log_return: 5.0e-3,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 5,
                    elapsed_same_session_hours: 1.0,
                    log_return: 8.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 6,
                    elapsed_same_session_hours: 1.0,
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
            &disabled_tick_grid_observation_model(),
        );

        assert!((threshold - 5.0e-3).abs() <= 1.0e-12);
    }

    #[test]
    fn continuous_return_filter_threshold_uses_widest_positive_gap_midpoint_without_shocks() {
        let threshold = fit_continuous_return_filter_threshold(
            &[
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 1,
                    elapsed_same_session_hours: 1.0,
                    log_return: 1.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 2,
                    elapsed_same_session_hours: 1.0,
                    log_return: -2.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 3,
                    elapsed_same_session_hours: 1.0,
                    log_return: 3.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 4,
                    elapsed_same_session_hours: 1.0,
                    log_return: 5.0e-3,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
            ],
            &[],
            &[],
            &disabled_tick_grid_observation_model(),
        );

        assert!((threshold - (3.0e-4_f64 * 5.0e-3_f64).sqrt()).abs() <= 1.0e-12);
    }

    #[test]
    fn continuous_return_filter_threshold_uses_nearest_tick_grid_multiple() {
        let tick_grid_observation_model = TickGridObservationModel {
            tick_size: Some(1.0e-3),
            tick_size_source: "fixture".to_string(),
            inference_policy: "fixture.v1".to_string(),
            observation_policy: "reference_price_normalized_tick_grid.v1".to_string(),
        };

        let threshold = fit_continuous_return_filter_threshold(
            &[
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 1,
                    elapsed_same_session_hours: 1.0,
                    log_return: 4.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: 2,
                    elapsed_same_session_hours: 1.0,
                    log_return: 4.9e-3,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
            ],
            &[],
            &[],
            &tick_grid_observation_model,
        );

        assert_eq!(threshold, 1.0e-3);
    }

    #[test]
    fn fitted_signal_partition_layer_records_derived_filter_policy() {
        let fitted_layer =
            fit_signal_partition_layer(&[], &[], &[], &disabled_tick_grid_observation_model());

        assert_eq!(
            fitted_layer.continuous_return_filter_policy,
            "maximum_explicit_shock_magnitude_or_nearest_tick_grid_adjusted_widest_log_gap_midpoint_in_positive_absolute_open_return_spectrum.v1"
        );
        assert_eq!(fitted_layer.continuous_return_filter_threshold, 0.0);
    }
}
