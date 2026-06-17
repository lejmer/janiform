use janiform_model_kernels::timestamp_observation::{
    TIMESTAMP_BATCHING_INFERRED_BUCKET_SOURCE, TimestampBatchingObservationModel,
};

use super::ObservationModelsOpenSessionTimestampTransition;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TimestampBatchingObservationFit {
    pub(super) positive_timestamp_quantum_nanos: Option<i64>,
    pub(super) same_timestamp_continuation_count: u64,
    pub(super) same_session_observation_transition_count: u64,
}

pub(super) fn timestamp_batching_observation_fit(
    transitions: &[ObservationModelsOpenSessionTimestampTransition],
) -> TimestampBatchingObservationFit {
    let mut fit = TimestampBatchingObservationFit::default();

    for transition in transitions {
        let timestamp_delta_nanos =
            transition.current_timestamp_utc_nanos - transition.previous_timestamp_utc_nanos;

        if timestamp_delta_nanos < 0 {
            continue;
        }

        fit.same_session_observation_transition_count = fit
            .same_session_observation_transition_count
            .saturating_add(1);

        if timestamp_delta_nanos == 0 {
            fit.same_timestamp_continuation_count =
                fit.same_timestamp_continuation_count.saturating_add(1);

            continue;
        }

        fit.positive_timestamp_quantum_nanos = Some(match fit.positive_timestamp_quantum_nanos {
            Some(current_quantum_nanos) => {
                gcd_positive_i64(current_quantum_nanos, timestamp_delta_nanos)
            }
            None => timestamp_delta_nanos,
        });
    }

    fit
}

pub(super) fn timestamp_batching_observation_model_from_fit(
    fit: &TimestampBatchingObservationFit,
) -> Result<TimestampBatchingObservationModel, String> {
    if fit.same_timestamp_continuation_count == 0 {
        return Ok(TimestampBatchingObservationModel::disabled());
    }

    let Some(bucket_width_nanos) = fit.positive_timestamp_quantum_nanos else {
        return Ok(TimestampBatchingObservationModel::disabled());
    };

    TimestampBatchingObservationModel::fixed_bucket_floor(
        bucket_width_nanos,
        TIMESTAMP_BATCHING_INFERRED_BUCKET_SOURCE,
        fit.same_timestamp_continuation_count,
        fit.same_session_observation_transition_count,
    )
    .map_err(|source| source.to_string())
}

fn gcd_positive_i64(mut left_value: i64, mut right_value: i64) -> i64 {
    while right_value != 0 {
        let remainder = left_value % right_value;

        left_value = right_value;
        right_value = remainder;
    }

    left_value
}

#[cfg(test)]
mod tests {
    use super::{
        timestamp_batching_observation_fit, timestamp_batching_observation_model_from_fit,
    };
    use crate::stages::observation_models::ObservationModelsOpenSessionTimestampTransition;
    use janiform_model_kernels::timestamp_observation::{
        TimestampBatchingObservationMode, TimestampBatchingObservationModel,
    };

    #[test]
    fn timestamp_batching_fit_infers_gcd_quantum_and_continuation_counts() {
        let transitions = [
            timestamp_transition(1_000, 1_000),
            timestamp_transition(1_000, 1_250),
            timestamp_transition(1_250, 1_750),
            timestamp_transition(1_750, 1_750),
        ];

        let fit = timestamp_batching_observation_fit(&transitions);
        let model = timestamp_batching_observation_model_from_fit(&fit)
            .expect("timestamp batching model should construct");

        assert_eq!(
            model.mode,
            TimestampBatchingObservationMode::FixedBucketFloor
        );
        assert_eq!(model.bucket_width_nanos, Some(250));
        assert_eq!(model.same_timestamp_continuation_count, Some(2));
        assert_eq!(model.same_session_observation_transition_count, Some(4));
    }

    #[test]
    fn timestamp_batching_fit_disables_when_no_duplicate_timestamp_exists() {
        let transitions = [
            timestamp_transition(1_000, 1_250),
            timestamp_transition(1_250, 1_500),
        ];

        let fit = timestamp_batching_observation_fit(&transitions);
        let model = timestamp_batching_observation_model_from_fit(&fit)
            .expect("disabled timestamp batching model should construct");

        assert_eq!(model, TimestampBatchingObservationModel::disabled());
    }

    const fn timestamp_transition(
        previous_timestamp_utc_nanos: i64,
        current_timestamp_utc_nanos: i64,
    ) -> ObservationModelsOpenSessionTimestampTransition {
        ObservationModelsOpenSessionTimestampTransition {
            previous_timestamp_utc_nanos,
            current_timestamp_utc_nanos,
        }
    }
}
