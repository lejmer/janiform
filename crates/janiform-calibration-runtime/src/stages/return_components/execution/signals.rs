use std::collections::{BTreeMap, BTreeSet};

use janiform_foundations::numeric::{
    arithmetic_mean_or_zero as arithmetic_mean, smallest_positive_value,
    strictly_positive_machine_value,
};

use super::super::contracts::{
    ReturnComponentsDetectedCloseOpenGapCandidate, ReturnComponentsDetectedIntradayJumpCandidate,
    ReturnComponentsRealizedKnownNewsReturnSample, ReturnComponentsStageActivityInput,
    ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageObservedSignalSummaryStatistics, ReturnComponentsStageSignalSourceWindow,
    ReturnComponentsSurpriseJumpEpisode,
};
use super::super::{continuous_fit, signal_partition_fit, surprise_episode_selection_fit};

#[derive(Debug, Clone)]
pub(crate) struct ReturnComponentsStageSortedSignalInputs {
    pub(crate) known_return_samples: Vec<ReturnComponentsRealizedKnownNewsReturnSample>,
    pub(crate) observed_open_session_returns:
        Vec<super::super::contracts::ReturnComponentsObservedOpenSessionReturn>,
    pub(crate) continuous_fit_returns:
        Vec<super::super::contracts::ReturnComponentsObservedOpenSessionReturn>,
    pub(crate) intraday_jump_candidates: Vec<ReturnComponentsDetectedIntradayJumpCandidate>,
    pub(crate) surprise_jump_episodes: Vec<ReturnComponentsSurpriseJumpEpisode>,
    pub(crate) selected_surprise_jump_episodes: Vec<ReturnComponentsSurpriseJumpEpisode>,
    pub(crate) close_open_gap_candidates: Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    pub(crate) surprise_open_jump_magnitudes: Vec<f64>,
    pub(crate) combined_gap_magnitudes: Vec<f64>,
    pub(crate) source_window: ReturnComponentsStageSignalSourceWindow,
}

#[derive(Debug, Clone)]
pub(crate) struct ReturnComponentsStagePreparedCandidateGenerationInput {
    pub(crate) candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
    pub(crate) derived_close_open_gap_candidates:
        Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    pub(crate) close_open_gap_detection_filter: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReturnComponentsStageActivityCalibrationContext {
    pub(crate) modeled_open_event_rate_per_hour: f64,
    pub(crate) representative_interarrival_hours: f64,
}

pub(crate) fn resolve_activity_calibration_context(
    activity_input: &ReturnComponentsStageActivityInput,
) -> Option<ReturnComponentsStageActivityCalibrationContext> {
    let hawkes_payload = activity_input.hawkes_payload()?;
    let selected_candidate = hawkes_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == hawkes_payload.selected_candidate_id)?;

    let modeled_open_event_rate_per_hour = selected_candidate
        .implied_metrics
        .model_implied_open_event_rate_per_hour;
    let representative_interarrival_hours = selected_candidate
        .source_regime
        .open_session_mean_interarrival_seconds
        / 3_600.0;

    if !modeled_open_event_rate_per_hour.is_finite()
        || modeled_open_event_rate_per_hour <= 0.0
        || !representative_interarrival_hours.is_finite()
        || representative_interarrival_hours <= 0.0
    {
        return None;
    }

    Some(ReturnComponentsStageActivityCalibrationContext {
        modeled_open_event_rate_per_hour,
        representative_interarrival_hours,
    })
}

pub(crate) fn sort_candidate_generation_signals(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    derived_close_open_gap_candidates: &[ReturnComponentsDetectedCloseOpenGapCandidate],
    fitted_signal_partition_layer: signal_partition_fit::FittedSignalPartitionLayer,
    fitted_surprise_episode_selection_layer: &surprise_episode_selection_fit::FittedSurpriseEpisodeSelectionLayer,
) -> ReturnComponentsStageSortedSignalInputs {
    let mut known_return_samples = candidate_generation_input
        .realized_known_news_return_samples
        .clone();
    known_return_samples.sort_by_key(|sample| sample.event_timestamp_utc_nanos);

    let mut observed_open_session_returns = candidate_generation_input
        .observed_open_session_returns
        .clone();
    observed_open_session_returns.sort_by_key(|observation| observation.return_timestamp_utc_nanos);

    let mut intraday_jump_candidates = candidate_generation_input
        .detected_intraday_jump_candidates
        .clone();
    intraday_jump_candidates.sort_by_key(|candidate| candidate.jump_timestamp_utc_nanos);

    let mut close_open_gap_candidates = derived_close_open_gap_candidates.to_vec();
    close_open_gap_candidates.sort_by_key(|candidate| {
        (
            candidate.close_timestamp_utc_nanos,
            candidate.open_timestamp_utc_nanos,
        )
    });

    let combined_gap_magnitudes = {
        let mut magnitudes =
            Vec::with_capacity(known_return_samples.len() + close_open_gap_candidates.len());

        magnitudes.extend(
            known_return_samples
                .iter()
                .map(|sample| sample.closed_window_gap_return.abs()),
        );
        magnitudes.extend(
            close_open_gap_candidates
                .iter()
                .map(|candidate| candidate.gap_return.abs()),
        );

        magnitudes
    };

    let surprise_jump_episodes = fitted_surprise_episode_selection_layer
        .surprise_jump_episodes
        .clone();
    let selected_surprise_jump_episodes = fitted_surprise_episode_selection_layer
        .selected_surprise_jump_episodes
        .clone();
    let exogenous_jump_timestamps: BTreeSet<i64> = selected_surprise_jump_episodes
        .iter()
        .flat_map(|episode| episode.member_jump_timestamps_utc_nanos.iter().copied())
        .collect();
    let known_news_attributed_open_return_timestamps: BTreeSet<i64> = known_return_samples
        .iter()
        .filter_map(|sample| sample.attributed_open_return_timestamp_utc_nanos)
        .collect();

    let mut continuous_fit_returns = observed_open_session_returns
        .iter()
        .copied()
        .filter(|observation| {
            !known_news_attributed_open_return_timestamps
                .contains(&observation.return_timestamp_utc_nanos)
                && !exogenous_jump_timestamps.contains(&observation.return_timestamp_utc_nanos)
                && observation.log_return.abs()
                    <= fitted_signal_partition_layer.continuous_return_filter_threshold
        })
        .collect::<Vec<_>>();

    if continuous_fit_returns.is_empty() {
        continuous_fit_returns = observed_open_session_returns
            .iter()
            .copied()
            .filter(|observation| {
                observation.log_return.abs()
                    <= fitted_signal_partition_layer.continuous_return_filter_threshold
            })
            .collect();
    }
    if continuous_fit_returns.is_empty() {
        continuous_fit_returns = observed_open_session_returns.clone();
    }

    ReturnComponentsStageSortedSignalInputs {
        known_return_samples,
        observed_open_session_returns,
        continuous_fit_returns,
        intraday_jump_candidates,
        surprise_jump_episodes,
        selected_surprise_jump_episodes: selected_surprise_jump_episodes.clone(),
        close_open_gap_candidates,
        surprise_open_jump_magnitudes: selected_surprise_jump_episodes
            .iter()
            .map(|episode| episode.cumulative_jump_return.abs())
            .collect(),
        combined_gap_magnitudes,
        source_window: candidate_generation_input.known_news_calendar.source_window,
    }
}

pub(crate) fn summarize_candidate_generation_signals(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContext>,
) -> (
    ReturnComponentsStageObservedSignalSummaryStatistics,
    continuous_fit::ReturnComponentsStageContinuousReturnCalibrationSample,
) {
    let source_window_duration_hours = sorted_signals
        .source_window
        .duration_hours()
        .max(f64::EPSILON);

    let continuous_fit_business_time_scales =
        derive_continuous_fit_business_time_scales(sorted_signals, activity_calibration_context);

    let continuous_return_calibration_sample =
        continuous_fit::build_continuous_return_calibration_sample(
            sorted_signals,
            continuous_fit_business_time_scales.as_slice(),
            candidate_generation_input.open_session_sell_share_proxy,
        );

    let mean_continuous_business_time_scale =
        continuous_fit::mean_continuous_return_calibration_business_time_scale(
            &continuous_return_calibration_sample,
        )
        .max(
            smallest_positive_value(
                continuous_fit::continuous_return_calibration_sample_business_time_scales(
                    &continuous_return_calibration_sample,
                ),
            )
            .unwrap_or_else(strictly_positive_machine_value),
        );

    let continuous_distribution =
        continuous_fit::summarize_observed_continuous_return_calibration_sample(
            &continuous_return_calibration_sample,
        );

    let observed_continuous_log_price_span = compute_observed_log_price_span(
        sorted_signals
            .continuous_fit_returns
            .iter()
            .map(|observation| {
                (
                    observation.return_timestamp_utc_nanos,
                    observation.log_return,
                )
            })
            .collect::<Vec<_>>()
            .as_slice(),
    );

    let observed_log_price_span = compute_observed_log_price_span(
        sorted_signals
            .observed_open_session_returns
            .iter()
            .map(|observation| {
                (
                    observation.return_timestamp_utc_nanos,
                    observation.log_return,
                )
            })
            .chain(
                sorted_signals
                    .close_open_gap_candidates
                    .iter()
                    .map(|candidate| (candidate.open_timestamp_utc_nanos, candidate.gap_return)),
            )
            .collect::<Vec<_>>()
            .as_slice(),
    );

    let known_activity_coupling_target = arithmetic_mean(
        &sorted_signals
            .known_return_samples
            .iter()
            .map(|sample| {
                activity_coupling_target_from_post_shock_lift(sample.post_shock_activity_lift)
            })
            .collect::<Vec<_>>(),
    );

    let surprise_activity_coupling_target = arithmetic_mean(
        &sorted_signals
            .selected_surprise_jump_episodes
            .iter()
            .map(|episode| {
                activity_coupling_target_from_post_shock_lift(episode.post_shock_activity_lift)
            })
            .collect::<Vec<_>>(),
    );

    (
        ReturnComponentsStageObservedSignalSummaryStatistics {
            known_calendar_event_count: candidate_generation_input
                .known_news_calendar
                .scheduled_event_timestamps_utc_nanos
                .len(),
            known_realized_sample_count: sorted_signals.known_return_samples.len(),
            source_window_duration_hours,
            observed_open_session_return_count: sorted_signals.observed_open_session_returns.len(),
            continuous_fit_return_count: continuous_fit::continuous_return_calibration_sample_count(
                &continuous_return_calibration_sample,
            ),
            intraday_jump_candidate_count: sorted_signals.intraday_jump_candidates.len(),
            intraday_jump_episode_count: sorted_signals.surprise_jump_episodes.len(),
            selected_surprise_jump_episode_count: sorted_signals
                .selected_surprise_jump_episodes
                .len(),
            close_open_gap_candidate_count: sorted_signals.close_open_gap_candidates.len(),
            close_open_gap_episode_count: sorted_signals.close_open_gap_candidates.len(),
            open_session_sell_share_proxy: candidate_generation_input.open_session_sell_share_proxy,
            mean_continuous_business_time_scale,
            continuous_return_mean: continuous_distribution.mean_return,
            continuous_return_volatility: continuous_distribution.volatility,
            continuous_return_skewness: continuous_distribution.skewness,
            continuous_return_excess_kurtosis: continuous_distribution.excess_kurtosis,
            observed_continuous_log_price_span,
            observed_log_price_span,
            mean_abs_known_open_jump_return: arithmetic_mean(
                sorted_signals
                    .known_return_samples
                    .iter()
                    .map(|sample| sample.open_window_jump_return.abs())
                    .collect::<Vec<_>>()
                    .as_slice(),
            ),
            mean_abs_known_closed_gap_return: arithmetic_mean(
                sorted_signals
                    .known_return_samples
                    .iter()
                    .map(|sample| sample.closed_window_gap_return.abs())
                    .collect::<Vec<_>>()
                    .as_slice(),
            ),
            mean_abs_surprise_open_jump_return: arithmetic_mean(
                &sorted_signals
                    .selected_surprise_jump_episodes
                    .iter()
                    .map(|episode| episode.cumulative_jump_return.abs())
                    .collect::<Vec<_>>(),
            ),
            mean_abs_surprise_closed_gap_return: arithmetic_mean(
                sorted_signals
                    .close_open_gap_candidates
                    .iter()
                    .map(|candidate| candidate.gap_return.abs())
                    .collect::<Vec<_>>()
                    .as_slice(),
            ),
            mean_known_post_shock_activity_lift: arithmetic_mean(
                sorted_signals
                    .known_return_samples
                    .iter()
                    .map(|sample| sample.post_shock_activity_lift)
                    .collect::<Vec<_>>()
                    .as_slice(),
            ),
            mean_surprise_post_shock_activity_lift: arithmetic_mean(
                &sorted_signals
                    .selected_surprise_jump_episodes
                    .iter()
                    .map(|episode| episode.post_shock_activity_lift)
                    .collect::<Vec<_>>(),
            ),
            known_activity_coupling_target,
            surprise_activity_coupling_target,
            known_news_coverage_ratio: sorted_signals.known_return_samples.len() as f64
                / candidate_generation_input
                    .known_news_calendar
                    .scheduled_event_timestamps_utc_nanos
                    .len()
                    .max(1) as f64,
            surprise_arrival_intensity_per_hour: sorted_signals
                .selected_surprise_jump_episodes
                .len() as f64
                / source_window_duration_hours,
        },
        continuous_return_calibration_sample,
    )
}

pub(super) fn derive_continuous_fit_business_time_scales(
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContext>,
) -> Vec<f64> {
    let Some(activity_calibration_context) = activity_calibration_context else {
        return vec![1.0; sorted_signals.continuous_fit_returns.len()];
    };

    let representative_interarrival_hours =
        activity_calibration_context.representative_interarrival_hours;

    let fallback_business_time_scale = activity_calibration_context
        .modeled_open_event_rate_per_hour
        * representative_interarrival_hours;

    let mut business_time_scale_by_timestamp = BTreeMap::new();

    for observation in &sorted_signals.observed_open_session_returns {
        let business_time_scale = activity_calibration_context.modeled_open_event_rate_per_hour
            * observation.elapsed_same_session_hours;

        business_time_scale_by_timestamp
            .insert(observation.return_timestamp_utc_nanos, business_time_scale);
    }

    sorted_signals
        .continuous_fit_returns
        .iter()
        .map(|observation| {
            business_time_scale_by_timestamp
                .get(&observation.return_timestamp_utc_nanos)
                .copied()
                .unwrap_or(fallback_business_time_scale)
        })
        .collect()
}

fn activity_coupling_target_from_post_shock_lift(post_shock_activity_lift: f64) -> f64 {
    if !post_shock_activity_lift.is_finite() {
        return 0.0;
    }

    (post_shock_activity_lift - 1.0).max(0.0)
}

fn compute_observed_log_price_span(timestamped_returns: &[(i64, f64)]) -> f64 {
    if timestamped_returns.is_empty() {
        return 0.0;
    }

    let mut sorted_returns = timestamped_returns.to_vec();
    sorted_returns.sort_by_key(|(timestamp_utc_nanos, _)| *timestamp_utc_nanos);

    let mut cumulative_return = 0.0_f64;
    let mut minimum_cumulative_return = 0.0_f64;
    let mut maximum_cumulative_return = 0.0_f64;

    for (_, return_increment) in sorted_returns {
        cumulative_return += return_increment;
        minimum_cumulative_return = minimum_cumulative_return.min(cumulative_return);
        maximum_cumulative_return = maximum_cumulative_return.max(cumulative_return);
    }

    (maximum_cumulative_return - minimum_cumulative_return).max(0.0)
}
