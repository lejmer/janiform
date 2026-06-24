use janiform_foundations::randomness::SimulationRandomnessStream;
use janiform_model_kernels::timestamp_batching_surface::{
    TimestampBatchingMode, TimestampBatchingSurface,
};

#[derive(Debug, Default)]
pub(super) struct TimestampBatchingIntervalObservationState {
    previous_endogenous_observed_timestamp_utc_nanos: Option<i64>,
}

pub(super) struct TimestampBatchingUpdateObservationInput<'a, R>
where
    R: SimulationRandomnessStream,
{
    pub(super) timestamp_batching_surface: &'a TimestampBatchingSurface,
    pub(super) randomness_stream: &'a mut R,
    pub(super) interval_start_utc_nanos: i64,
    pub(super) interval_end_utc_nanos: i64,
    pub(super) state: &'a mut TimestampBatchingIntervalObservationState,
    pub(super) bucketed_observed_timestamp_utc_nanos: i64,
    pub(super) apply_timestamp_batching: bool,
}

pub(super) fn observed_timestamp_for_update<R>(
    input: TimestampBatchingUpdateObservationInput<'_, R>,
) -> i64
where
    R: SimulationRandomnessStream,
{
    let TimestampBatchingUpdateObservationInput {
        timestamp_batching_surface,
        randomness_stream,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        state,
        bucketed_observed_timestamp_utc_nanos,
        apply_timestamp_batching,
    } = input;

    if !apply_timestamp_batching
        || timestamp_batching_surface.mode == TimestampBatchingMode::Disabled
    {
        return bucketed_observed_timestamp_utc_nanos;
    }

    let observed_timestamp_utc_nanos = if should_continue_same_timestamp_batch(
        timestamp_batching_surface,
        randomness_stream,
        state,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
    ) {
        state
            .previous_endogenous_observed_timestamp_utc_nanos
            .unwrap_or(bucketed_observed_timestamp_utc_nanos)
    } else {
        bucketed_observed_timestamp_utc_nanos
    };

    state.previous_endogenous_observed_timestamp_utc_nanos = Some(observed_timestamp_utc_nanos);

    observed_timestamp_utc_nanos
}

fn should_continue_same_timestamp_batch<R>(
    timestamp_batching_surface: &TimestampBatchingSurface,
    randomness_stream: &mut R,
    state: &TimestampBatchingIntervalObservationState,
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> bool
where
    R: SimulationRandomnessStream,
{
    let Some(previous_observed_timestamp_utc_nanos) =
        state.previous_endogenous_observed_timestamp_utc_nanos
    else {
        return false;
    };

    if previous_observed_timestamp_utc_nanos < interval_start_utc_nanos
        || previous_observed_timestamp_utc_nanos >= interval_end_utc_nanos
    {
        return false;
    }

    let Some(continuation_probability) =
        timestamp_batching_surface.same_timestamp_continuation_probability()
    else {
        return false;
    };

    if continuation_probability <= 0.0 {
        return false;
    }

    if continuation_probability >= 1.0 {
        return true;
    }

    randomness_stream.next_unit_interval_f64() < continuation_probability
}
