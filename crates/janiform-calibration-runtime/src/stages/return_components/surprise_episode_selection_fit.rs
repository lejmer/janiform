use super::contracts::{
    ReturnComponentsDetectedIntradayJumpCandidate,
    ReturnComponentsStageSurpriseEpisodeSelectionPolicy, ReturnComponentsSurpriseJumpEpisode,
};
use super::quantile_binning_fit::deterministic_quantile_values;

const RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_SELECTION_POLICY: &str =
    "quantile_thresholded_intraday_jump_episode_selection_with_activity_and_magnitude_evidence.v1";
const RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_MERGE_GAP_QUANTILE: f64 = 0.25;
const RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_MAGNITUDE_QUANTILE: f64 = 0.75;
const RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_ACTIVITY_QUANTILE: f64 = 0.75;
const RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_COMPOSITE_QUANTILE: f64 = 0.95;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FittedSurpriseEpisodeSelectionLayer {
    pub(crate) policy: ReturnComponentsStageSurpriseEpisodeSelectionPolicy,
    pub(crate) surprise_jump_episodes: Vec<ReturnComponentsSurpriseJumpEpisode>,
    pub(crate) selected_surprise_jump_episodes: Vec<ReturnComponentsSurpriseJumpEpisode>,
}

pub(crate) fn fit_surprise_episode_selection_layer(
    intraday_jump_candidates: &[ReturnComponentsDetectedIntradayJumpCandidate],
    continuous_filter_threshold: f64,
) -> FittedSurpriseEpisodeSelectionLayer {
    let merge_gap_threshold_nanos = fit_merge_gap_threshold_nanos(intraday_jump_candidates);

    let surprise_jump_episodes =
        derive_surprise_jump_episodes(intraday_jump_candidates, merge_gap_threshold_nanos);

    let (normalized_magnitude_threshold, positive_activity_excess_threshold, composite_threshold) =
        fit_selection_thresholds(
            surprise_jump_episodes.as_slice(),
            continuous_filter_threshold,
        );

    let selected_surprise_jump_episodes = select_exogenous_surprise_jump_episodes(
        surprise_jump_episodes.as_slice(),
        normalized_magnitude_threshold,
        positive_activity_excess_threshold,
        composite_threshold,
        continuous_filter_threshold,
    );

    FittedSurpriseEpisodeSelectionLayer {
        policy: ReturnComponentsStageSurpriseEpisodeSelectionPolicy {
            fit_policy: RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_SELECTION_POLICY,
            merge_gap_quantile: RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_MERGE_GAP_QUANTILE,
            merge_gap_threshold_nanos,
            normalized_magnitude_quantile:
                RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_MAGNITUDE_QUANTILE,
            normalized_magnitude_threshold,
            positive_activity_excess_quantile:
                RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_ACTIVITY_QUANTILE,
            positive_activity_excess_threshold,
            composite_score_quantile: RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_COMPOSITE_QUANTILE,
            composite_score_threshold: composite_threshold,
        },
        surprise_jump_episodes,
        selected_surprise_jump_episodes,
    }
}

fn fit_merge_gap_threshold_nanos(
    intraday_jump_candidates: &[ReturnComponentsDetectedIntradayJumpCandidate],
) -> i64 {
    if intraday_jump_candidates.len() < 2 {
        return 0;
    }

    let mut sorted_candidates = intraday_jump_candidates.to_vec();
    sorted_candidates.sort_by_key(|candidate| candidate.jump_timestamp_utc_nanos);
    let consecutive_gap_sample: Vec<f64> = sorted_candidates
        .windows(2)
        .filter_map(|candidate_pair| {
            let gap_nanos = candidate_pair[1].jump_timestamp_utc_nanos
                - candidate_pair[0].jump_timestamp_utc_nanos;
            if gap_nanos > 0 {
                Some(gap_nanos as f64)
            } else {
                None
            }
        })
        .collect();

    deterministic_quantile_values(
        consecutive_gap_sample.as_slice(),
        &[RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_MERGE_GAP_QUANTILE],
    )
    .first()
    .copied()
    .unwrap_or(0.0) as i64
}

fn derive_surprise_jump_episodes(
    intraday_jump_candidates: &[ReturnComponentsDetectedIntradayJumpCandidate],
    merge_gap_threshold_nanos: i64,
) -> Vec<ReturnComponentsSurpriseJumpEpisode> {
    if intraday_jump_candidates.is_empty() {
        return Vec::new();
    }

    let mut sorted_candidates = intraday_jump_candidates.to_vec();
    sorted_candidates.sort_by_key(|candidate| candidate.jump_timestamp_utc_nanos);

    let mut episodes = Vec::new();
    let mut current_episode = ReturnComponentsSurpriseJumpEpisode {
        member_jump_timestamps_utc_nanos: vec![sorted_candidates[0].jump_timestamp_utc_nanos],
        terminal_timestamp_utc_nanos: sorted_candidates[0].jump_timestamp_utc_nanos,
        cumulative_jump_return: sorted_candidates[0].jump_return,
        post_shock_activity_lift: sorted_candidates[0].post_shock_activity_lift,
    };

    for candidate in sorted_candidates.iter().skip(1) {
        let candidate_gap_nanos =
            candidate.jump_timestamp_utc_nanos - current_episode.terminal_timestamp_utc_nanos;
        if merge_gap_threshold_nanos > 0 && candidate_gap_nanos <= merge_gap_threshold_nanos {
            current_episode
                .member_jump_timestamps_utc_nanos
                .push(candidate.jump_timestamp_utc_nanos);
            current_episode.terminal_timestamp_utc_nanos = candidate.jump_timestamp_utc_nanos;
            current_episode.cumulative_jump_return += candidate.jump_return;
            current_episode.post_shock_activity_lift = current_episode
                .post_shock_activity_lift
                .max(candidate.post_shock_activity_lift);
            continue;
        }

        episodes.push(current_episode);
        current_episode = ReturnComponentsSurpriseJumpEpisode {
            member_jump_timestamps_utc_nanos: vec![candidate.jump_timestamp_utc_nanos],
            terminal_timestamp_utc_nanos: candidate.jump_timestamp_utc_nanos,
            cumulative_jump_return: candidate.jump_return,
            post_shock_activity_lift: candidate.post_shock_activity_lift,
        };
    }
    episodes.push(current_episode);

    episodes
}

fn fit_selection_thresholds(
    surprise_jump_episodes: &[ReturnComponentsSurpriseJumpEpisode],
    continuous_filter_threshold: f64,
) -> (f64, f64, f64) {
    if surprise_jump_episodes.is_empty() {
        return (1.0, 0.0, 0.0);
    }

    let normalized_magnitude_scores: Vec<f64> = surprise_jump_episodes
        .iter()
        .map(|episode| episode.cumulative_jump_return.abs())
        .map(|magnitude| magnitude / continuous_filter_threshold.max(f64::EPSILON))
        .collect();
    let positive_activity_excesses: Vec<f64> = surprise_jump_episodes
        .iter()
        .map(|episode| positive_activity_excess(episode.post_shock_activity_lift))
        .filter(|value| *value > 0.0)
        .collect();
    if positive_activity_excesses.is_empty() {
        return (1.0, 0.0, 0.0);
    }

    let normalized_magnitude_threshold = deterministic_quantile_values(
        normalized_magnitude_scores.as_slice(),
        &[RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_MAGNITUDE_QUANTILE],
    )
    .first()
    .copied()
    .unwrap_or(1.0)
    .max(1.0);

    let positive_activity_excess_threshold = deterministic_quantile_values(
        positive_activity_excesses.as_slice(),
        &[RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_ACTIVITY_QUANTILE],
    )
    .first()
    .copied()
    .unwrap_or(0.0);

    let composite_scores: Vec<f64> = surprise_jump_episodes
        .iter()
        .zip(normalized_magnitude_scores.iter())
        .filter_map(|(episode, magnitude_score)| {
            let activity_excess = positive_activity_excess(episode.post_shock_activity_lift);
            if activity_excess <= 0.0 {
                return None;
            }

            Some(magnitude_score.ln_1p() * activity_excess.ln_1p())
        })
        .collect();

    if composite_scores.is_empty() {
        return (
            normalized_magnitude_threshold,
            positive_activity_excess_threshold,
            0.0,
        );
    }

    let composite_threshold = deterministic_quantile_values(
        composite_scores.as_slice(),
        &[RETURN_COMPONENTS_STAGE_SURPRISE_EPISODE_COMPOSITE_QUANTILE],
    )
    .first()
    .copied()
    .unwrap_or(0.0);
    (
        normalized_magnitude_threshold,
        positive_activity_excess_threshold,
        composite_threshold,
    )
}

fn select_exogenous_surprise_jump_episodes(
    surprise_jump_episodes: &[ReturnComponentsSurpriseJumpEpisode],
    normalized_magnitude_threshold: f64,
    positive_activity_excess_threshold: f64,
    composite_threshold: f64,
    continuous_filter_threshold: f64,
) -> Vec<ReturnComponentsSurpriseJumpEpisode> {
    surprise_jump_episodes
        .iter()
        .filter_map(|episode| {
            let normalized_magnitude_score = episode.cumulative_jump_return.abs()
                / continuous_filter_threshold.max(f64::EPSILON);

            let activity_excess = positive_activity_excess(episode.post_shock_activity_lift);

            let composite_score = if activity_excess <= 0.0 {
                0.0
            } else {
                normalized_magnitude_score.ln_1p() * activity_excess.ln_1p()
            };

            if normalized_magnitude_score < normalized_magnitude_threshold
                || activity_excess < positive_activity_excess_threshold
                || composite_score < composite_threshold
            {
                return None;
            }

            Some(episode.clone())
        })
        .collect()
}

fn positive_activity_excess(post_shock_activity_lift: f64) -> f64 {
    if !post_shock_activity_lift.is_finite() {
        return 0.0;
    }

    (post_shock_activity_lift - 1.0).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::fit_surprise_episode_selection_layer;
    use crate::stages::return_components::ReturnComponentsDetectedIntradayJumpCandidate;

    #[test]
    fn empty_input_collapses_episode_selection_layer() {
        let fitted_layer = fit_surprise_episode_selection_layer(&[], 1.0e-4);

        assert_eq!(fitted_layer.surprise_jump_episodes, Vec::new());
        assert_eq!(fitted_layer.selected_surprise_jump_episodes, Vec::new());
        assert_eq!(fitted_layer.policy.merge_gap_threshold_nanos, 0);
    }

    #[test]
    fn high_evidence_episode_is_retained() {
        let fitted_layer = fit_surprise_episode_selection_layer(
            &[
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: 10,
                    jump_return: 1.4e-3,
                    post_shock_activity_lift: 4.5,
                },
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: 12,
                    jump_return: 9.0e-4,
                    post_shock_activity_lift: 4.0,
                },
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: 100,
                    jump_return: 2.0e-4,
                    post_shock_activity_lift: 1.1,
                },
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: 400,
                    jump_return: -1.5e-4,
                    post_shock_activity_lift: 1.05,
                },
            ],
            1.0e-4,
        );

        assert_eq!(fitted_layer.surprise_jump_episodes.len(), 2);
        assert_eq!(fitted_layer.selected_surprise_jump_episodes.len(), 1);
        assert_eq!(
            fitted_layer.selected_surprise_jump_episodes[0].member_jump_timestamps_utc_nanos,
            vec![10, 12, 100]
        );
    }
}
