use janiform_foundations::linear_algebra::non_negative_least_squares_3;
use janiform_foundations::time::{NANOS_PER_HOUR, duration_nanos_to_hours};

use super::metrics::long_variance_interval_side_event_counts;
use super::observations::VarianceObservation;
use super::policy::LongVarianceFeedbackLoadings;
use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;

#[derive(Debug, Clone, Copy)]
pub(crate) struct VarianceFeedbackEstimate {
    pub(crate) loadings: LongVarianceFeedbackLoadings,
    pub(crate) candidate_pair_count: usize,
    pub(crate) energy_pair_count: usize,
    pub(crate) activity_pair_count: usize,
}

/// Joint predictive initializer for event and return-energy feedback.
/// Shocks are spread uniformly within each measured interval. Predictors precede the response
/// window. Full-model refinement owns the endogenous feedback correction to the
/// approximate covariance initializer.
pub(crate) fn estimate_variance_feedback(
    observations: &[VarianceObservation],
    normalized: &[VarianceObservation],
    activity: &[JointRefinementAggregatedIntervalObservation],
    rate: f64,
) -> VarianceFeedbackEstimate {
    let (total_energy, total_hours) =
        observations
            .iter()
            .fold((0.0, 0.0), |(energy, hours), row| {
                (
                    energy + row.energy,
                    hours + duration_nanos_to_hours(row.end - row.start),
                )
            });

    let mean_energy = if total_hours > 0.0 {
        total_energy / total_hours
    } else {
        0.0
    };

    let mut estimate = VarianceFeedbackEstimate {
        loadings: LongVarianceFeedbackLoadings {
            stationary_return_energy_rate_per_hour: mean_energy,
            return_energy_loading: 0.0,
            buy_event_loading: 0.0,
            sell_event_loading: 0.0,
        },
        candidate_pair_count: observations.len().saturating_sub(1),
        energy_pair_count: 0,
        activity_pair_count: 0,
    };

    if !rate.is_finite() || rate <= 0.0 || !mean_energy.is_finite() || mean_energy <= 0.0 {
        return estimate;
    }

    let (rows, response, activity_pair_count) =
        feedback_predictors(normalized, activity, rate, mean_energy);

    estimate.activity_pair_count = activity_pair_count;
    estimate.energy_pair_count = rows.len();

    if let Some([buy, sell, energy]) = non_negative_least_squares_3(&rows, &response) {
        estimate.loadings.buy_event_loading = buy;
        estimate.loadings.sell_event_loading = sell;
        estimate.loadings.return_energy_loading = energy;
    }

    estimate
}

fn feedback_predictors(
    normalized: &[VarianceObservation],
    activity: &[JointRefinementAggregatedIntervalObservation],
    rate: f64,
    mean_energy: f64,
) -> (Vec<[f64; 3]>, Vec<f64>, usize) {
    let mut activity_pair_count = 0;

    let side_means = mean_hourly_activity(activity);

    let mut state = [0.0; 3];
    let mut previous_end: Option<i64> = None;
    let mut previous_activity = false;
    let mut activity_position = 0;
    let mut rows = Vec::new();
    let mut response = Vec::new();

    for observation in normalized {
        let duration = duration_nanos_to_hours(observation.end - observation.start);
        let decay = (-rate * duration).exp();
        let average_decay = -(-rate * duration).exp_m1() / (rate * duration);
        let counts = if observation.end - observation.start == NANOS_PER_HOUR {
            aligned_activity(activity, &mut activity_position, observation.start)
        } else {
            None
        };

        let contiguous = previous_end == Some(observation.start);

        if contiguous {
            let mut row = state.map(|value| value * average_decay);

            if !previous_activity {
                row[0] = 0.0;
                row[1] = 0.0;
            } else {
                activity_pair_count += 1;
            }

            rows.push(row);
            response.push(observation.energy - 1.0);
        } else {
            state = [0.0; 3];
        }

        let centered_counts =
            counts.map(|counts| [counts[0] - side_means[0], counts[1] - side_means[1]]);

        let energy_signal = (observation.energy - 1.0) * mean_energy * duration;

        state[2] = decay * state[2] + average_decay * energy_signal;

        match centered_counts {
            Some(counts) => {
                state[0] = decay * state[0] + average_decay * counts[0];
                state[1] = decay * state[1] + average_decay * counts[1];
            }
            None => {
                state[0] = 0.0;
                state[1] = 0.0;
            }
        }

        previous_activity = counts.is_some();
        previous_end = Some(observation.end);
    }

    (rows, response, activity_pair_count)
}

fn mean_hourly_activity(activity: &[JointRefinementAggregatedIntervalObservation]) -> [f64; 2] {
    let mut totals = [0.0; 2];
    let mut count = 0usize;

    for interval in activity.iter().filter(|interval| {
        interval
            .interval_end_utc_nanos
            .checked_sub(interval.interval_start_utc_nanos)
            == Some(NANOS_PER_HOUR)
    }) {
        let counts = long_variance_interval_side_event_counts(*interval);

        totals[0] += counts.buy_count;
        totals[1] += counts.sell_count;

        count += 1;
    }

    if count > 0 {
        totals.map(|sum| sum / count as f64)
    } else {
        [0.0; 2]
    }
}

fn aligned_activity(
    intervals: &[JointRefinementAggregatedIntervalObservation],
    position: &mut usize,
    start: i64,
) -> Option<[f64; 2]> {
    while intervals
        .get(*position)
        .is_some_and(|interval| interval.interval_start_utc_nanos < start)
    {
        *position += 1;
    }

    let interval = intervals.get(*position)?;
    if interval.interval_start_utc_nanos != start
        || interval.interval_end_utc_nanos.checked_sub(start) != Some(NANOS_PER_HOUR)
    {
        return None;
    }

    let counts = long_variance_interval_side_event_counts(*interval);

    Some([counts.buy_count, counts.sell_count])
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::NANOS_PER_HOUR;

    use super::estimate_variance_feedback;
    use crate::stages::long_variance::observations::VarianceObservation;

    #[test]
    fn feedback_keeps_coarse_activity_absent_and_resets_at_missing_hours() {
        let observations = [0, 1, 3, 4].map(|hour| VarianceObservation {
            start: hour * NANOS_PER_HOUR,
            end: (hour + 1) * NANOS_PER_HOUR,
            energy: 0.5,
            sampling_count: 1,
        });

        let result = estimate_variance_feedback(
            &observations,
            &super::super::covariance::normalized_variance(&observations),
            &[],
            0.2,
        );

        assert_eq!(
            (
                result.candidate_pair_count,
                result.energy_pair_count,
                result.activity_pair_count
            ),
            (3, 2, 0),
        );

        assert_eq!(
            (
                result.loadings.buy_event_loading,
                result.loadings.sell_event_loading,
                result.loadings.return_energy_loading,
                result.loadings.stationary_return_energy_rate_per_hour
            ),
            (0.0, 0.0, 0.0, 0.5),
        );
    }
}
