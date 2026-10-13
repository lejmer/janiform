use std::collections::BTreeMap;

use chrono::{DateTime, Timelike, Utc};
use janiform_foundations::numeric::jackknife_standard_error_from_leave_one_out_values;
use janiform_foundations::time::{NANOS_PER_DAY, duration_nanos_to_hours};
use serde::{Deserialize, Serialize};

use super::observations::VarianceObservation;

// Each sampling group has decay, persistent amplitude, and observation noise.
// More than three temporal blocks are necessary for a full-rank three-parameter jackknife.
const PERSISTENCE_FIT_PARAMETER_COUNT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct VarianceCovariance {
    pub(crate) lag_hours: f64,
    pub(crate) window_hours: f64,
    pub(crate) sampling_count: usize,
    pub(crate) pair_count: usize,
    pub(crate) value: Option<f64>,
    pub(crate) standard_error: Option<f64>,
}

/// Remove the deterministic UTC-hour energy profile, retaining unit mean variance.
pub(crate) fn normalized_variance(
    observations: &[VarianceObservation],
) -> Vec<VarianceObservation> {
    let mut buckets = BTreeMap::<(i64, usize, u32), (f64, usize)>::new();

    for observation in observations {
        let Some(key) = observation_bucket(observation) else {
            continue;
        };

        let (sum, count) = buckets.entry(key).or_default();

        *sum += observation.energy;
        *count += 1;
    }

    observations
        .iter()
        .filter_map(|observation| {
            let (sum, count) = buckets.get(&observation_bucket(observation)?)?;
            let mean = sum / *count as f64;

            if !mean.is_finite() || mean <= 0.0 {
                return None;
            }

            Some(VarianceObservation {
                energy: observation.energy / mean,
                ..*observation
            })
        })
        .collect()
}

fn observation_bucket(observation: &VarianceObservation) -> Option<(i64, usize, u32)> {
    let duration = observation.end.checked_sub(observation.start)?;
    let hour = DateTime::<Utc>::from_timestamp_nanos(observation.start).hour();

    (duration > 0 && observation.energy.is_finite()).then_some((
        duration,
        observation.sampling_count,
        hour,
    ))
}

/// Exact timestamp pairs preserve closed sessions and missing observations.
/// Observed uncertainty uses nonoverlapping blocks at least one day and twice the lag.
/// The hour-of-day normalizer is held fixed for these conditional standard errors.
pub(crate) fn variance_covariances(
    observations: &[VarianceObservation],
    estimate_uncertainty: bool,
) -> Vec<VarianceCovariance> {
    covariances_from_normalized_variance(&normalized_variance(observations), estimate_uncertainty)
}

/// Consume the normalized observations shared with feedback estimation.
pub(crate) fn covariances_from_normalized_variance(
    observations: &[VarianceObservation],
    estimate_uncertainty: bool,
) -> Vec<VarianceCovariance> {
    let mut groups = BTreeMap::<(i64, usize), Vec<&VarianceObservation>>::new();

    for observation in observations {
        if let Some((duration, sampling_count, _)) = observation_bucket(observation) {
            groups
                .entry((duration, sampling_count))
                .or_default()
                .push(observation);
        }
    }

    groups
        .into_iter()
        .filter(|(_, group)| !estimate_uncertainty || group.len() > PERSISTENCE_FIT_PARAMETER_COUNT)
        .flat_map(|((duration, sampling_count), group)| {
            let span = group
                .first()
                .zip(group.last())
                .and_then(|(first, last)| last.start.checked_sub(first.start))
                .unwrap_or(0);

            // Geometric spacing covers the observed span with logarithmic work per group.
            std::iter::once(0)
                .chain(
                    std::iter::successors(Some(1_i64), |lag| lag.checked_mul(2)).take_while(
                        move |lag| lag.checked_mul(duration).is_some_and(|lag| lag <= span),
                    ),
                )
                .map(move |lag| {
                    covariance_at_lag(&group, duration, sampling_count, lag, estimate_uncertainty)
                })
        })
        .collect()
}

/// Algebraic and temporal uncertainty requirements shared by estimation and refinement.
pub(crate) fn supports_persistence_fit(covariances: &[VarianceCovariance]) -> bool {
    let supported = |moment: &&VarianceCovariance| {
        moment.value.is_some_and(f64::is_finite)
            && moment
                .standard_error
                .is_some_and(|error| error.is_finite() && error > 0.0)
    };

    covariances
        .iter()
        .filter(supported)
        .any(|moment| moment.lag_hours == 0.0)
        && covariances
            .iter()
            .filter(supported)
            .filter(|moment| moment.lag_hours > 0.0)
            .count()
            >= 2
}

fn covariance_at_lag(
    observations: &[&VarianceObservation],
    duration: i64,
    sampling_count: usize,
    lag: i64,
    estimate_uncertainty: bool,
) -> VarianceCovariance {
    let mut position = 0;
    let mut sum = 0.0;
    let mut count = 0;
    let mut blocks = Vec::<(f64, usize)>::new();

    let lag_nanos = lag.checked_mul(duration);

    let block_nanos = lag_nanos
        .and_then(|lag| lag.checked_mul(2))
        .map(|span| span.max(NANOS_PER_DAY));

    let mut previous_block = None;

    for current in observations {
        let Some(target) = lag_nanos.and_then(|lag| current.start.checked_sub(lag)) else {
            continue;
        };

        while observations
            .get(position)
            .is_some_and(|previous| previous.start < target)
        {
            position += 1;
        }

        let Some(previous) = observations
            .get(position)
            .filter(|previous| previous.start == target)
        else {
            continue;
        };

        let product = (previous.energy - 1.0) * (current.energy - 1.0);

        sum += product;
        count += 1;

        if estimate_uncertainty && let Some(block_nanos) = block_nanos {
            let block = current.start.div_euclid(block_nanos);

            if previous_block != Some(block) {
                blocks.push((0.0, 0));
                previous_block = Some(block);
            }

            if let Some((block_sum, block_count)) = blocks.last_mut() {
                *block_sum += product;
                *block_count += 1;
            }
        }
    }

    let value = (count > 0)
        .then(|| sum / count as f64)
        .filter(|value| value.is_finite());

    // Delete-block estimates respect unequal block sizes and overlapping lag pairs.
    let deleted = blocks
        .iter()
        .filter(|(_, block_count)| count > *block_count)
        .map(|(block_sum, block_count)| (sum - block_sum) / (count - block_count) as f64)
        .collect::<Vec<_>>();

    let standard_error = if deleted.len() > PERSISTENCE_FIT_PARAMETER_COUNT {
        jackknife_standard_error_from_leave_one_out_values(&deleted)
    } else {
        None
    };

    VarianceCovariance {
        lag_hours: lag as f64 * duration_nanos_to_hours(duration),
        window_hours: duration_nanos_to_hours(duration),
        sampling_count,
        pair_count: count,
        value,
        standard_error,
    }
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::NANOS_PER_HOUR;

    use super::{normalized_variance, variance_covariances};
    use crate::stages::long_variance::observations::VarianceObservation;

    #[test]
    fn variance_covariance_uses_elapsed_time_across_gaps() {
        let observations = [0, 1, 3, 4].map(|hour| VarianceObservation {
            start: hour * NANOS_PER_HOUR,
            end: (hour + 1) * NANOS_PER_HOUR,
            energy: 1.0,
            sampling_count: 12,
        });

        let summary = variance_covariances(&observations, false);
        let counts = summary.iter().map(|row| row.pair_count).collect::<Vec<_>>();

        assert_eq!(counts, vec![4, 2, 1, 1]);
        assert_eq!(summary[1].value, Some(0.0));
        assert_eq!(summary[1].standard_error, None);
    }

    #[test]
    fn variance_covariance_normalization_preserves_day_to_day_signal() {
        let observations = (0..48)
            .map(|hour| VarianceObservation {
                start: hour * NANOS_PER_HOUR,
                end: (hour + 1) * NANOS_PER_HOUR,
                energy: (hour % 24 + 1) as f64
                    * if hour < 24 {
                        1.0
                    } else {
                        3.0
                    },
                sampling_count: 12,
            })
            .collect::<Vec<_>>();

        let normalized = normalized_variance(&observations);

        assert_eq!(
            normalized.iter().map(|row| row.energy).collect::<Vec<_>>(),
            [vec![0.5; 24], vec![1.5; 24]].concat()
        );

        let summary = variance_covariances(&observations, true);

        assert_eq!(
            (
                summary[0].value,
                summary
                    .iter()
                    .find(|moment| moment.lag_hours == 32.0)
                    .unwrap()
                    .value
            ),
            (Some(0.25), Some(-0.25))
        );
    }
}
