use crate::stages::mean_reversion::{
    FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY, FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY,
    OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY,
    mean_reversion_flat_anchor_required_source_observation_count,
    mean_reversion_full_model_required_source_observation_count,
};

pub(super) struct PredictiveValidationWindow {
    pub(super) training_observation_count: usize,
    pub(super) start_transition_index: usize,
    pub(super) end_transition_index_exclusive: usize,
}

pub(super) fn minimum_training_observation_count(candidate_family: &str) -> usize {
    match candidate_family {
        FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY => {
            mean_reversion_full_model_required_source_observation_count()
        }
        FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY => {
            mean_reversion_flat_anchor_required_source_observation_count()
        }
        OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY => {
            mean_reversion_full_model_required_source_observation_count()
        }
        _ => 2,
    }
}

pub(super) fn rolling_origin_refit_training_observation_counts(
    observation_count: usize,
    minimum_training_observation_count: usize,
) -> Vec<usize> {
    let final_training_observation_count = observation_count.saturating_sub(1);
    if final_training_observation_count < minimum_training_observation_count {
        return Vec::new();
    }

    let mut training_observation_counts = Vec::new();
    let mut training_observation_count = minimum_training_observation_count.max(2);

    while training_observation_count < final_training_observation_count {
        training_observation_counts.push(training_observation_count);

        let next_training_observation_count = training_observation_count.saturating_mul(2);
        if next_training_observation_count <= training_observation_count {
            break;
        }

        training_observation_count = next_training_observation_count;
    }

    if training_observation_counts.last().copied() != Some(final_training_observation_count) {
        training_observation_counts.push(final_training_observation_count);
    }

    training_observation_counts
}

pub(super) fn predictive_validation_window(
    refit_training_observation_counts: &[usize],
    refit_position: usize,
    training_observation_count: usize,
    observation_count: usize,
) -> Option<PredictiveValidationWindow> {
    let start_transition_index = training_observation_count.saturating_sub(1);

    let end_transition_index_exclusive = refit_training_observation_counts
        .get(refit_position.saturating_add(1))
        .copied()
        .unwrap_or(observation_count)
        .saturating_sub(1)
        .min(observation_count.saturating_sub(1));

    if start_transition_index >= end_transition_index_exclusive {
        return None;
    }

    Some(PredictiveValidationWindow {
        training_observation_count,
        start_transition_index,
        end_transition_index_exclusive,
    })
}

#[cfg(test)]
mod tests {
    use super::rolling_origin_refit_training_observation_counts;

    #[test]
    fn rolling_origin_refit_schedule_is_bounded_for_coarse_history() {
        let training_observation_counts =
            rolling_origin_refit_training_observation_counts(43_000, 3);

        assert_eq!(training_observation_counts.first().copied(), Some(3));
        assert_eq!(training_observation_counts.last().copied(), Some(42_999));
        assert_eq!(training_observation_counts.len(), 15);

        assert!(training_observation_counts.windows(2).all(|window| {
            let [left, right] = window else {
                return false;
            };

            left < right
        }));
    }
}
