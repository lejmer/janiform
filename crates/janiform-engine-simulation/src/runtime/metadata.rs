use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel;

use crate::artifacts::{
    SeededFinePathMetadata, SeededTickEventDebugTraceMetadata, SeededTickEventStateSnapshot,
    SeededTickEventTapeMetadata,
};
use crate::config::{SeededTickEventStateSnapshotCapture, SeededTickSimulationDebugTraceOptions};
use crate::constants::{
    SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNKING_POLICY,
    SEEDED_TICK_EVENT_DEBUG_TRACE_METADATA_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_RECORD_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_SAMPLING_POLICY,
    SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY, SEEDED_TICK_EVENT_ORDERING_POLICY,
    SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION, SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY,
    SEEDED_TICK_EVENT_STATE_CARRYOVER_POLICY, SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY,
};

pub(crate) const fn build_event_state_snapshot(
    state: HawkesExcitationState,
    lambda_bar: f64,
    snapshot_capture: SeededTickEventStateSnapshotCapture,
) -> Option<SeededTickEventStateSnapshot> {
    match snapshot_capture {
        SeededTickEventStateSnapshotCapture::Disabled => None,
        SeededTickEventStateSnapshotCapture::PostEventTransition => {
            Some(SeededTickEventStateSnapshot::from_state(state, lambda_bar))
        }
    }
}

pub(crate) fn build_seeded_tick_event_tape_metadata(
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
    timestamp_batching_observation_model: &TimestampBatchingObservationModel,
) -> SeededTickEventTapeMetadata {
    SeededTickEventTapeMetadata {
        schema_version: SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
        event_record_schema_version: SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION,
        state_snapshot_schema_version: SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION,
        event_ordering_policy: SEEDED_TICK_EVENT_ORDERING_POLICY,
        timestamp_representation_policy: SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY,
        // Metadata is persisted independently from the run config.
        timestamp_batching_observation_model: timestamp_batching_observation_model.clone(),
        side_encoding_policy: SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY,
        state_snapshot_capture,
        state_snapshot_capture_point_policy: state_snapshot_capture.capture_point_policy(),
    }
}

pub(crate) fn build_seeded_fine_path_metadata(
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
    timestamp_batching_observation_model: &TimestampBatchingObservationModel,
) -> SeededFinePathMetadata {
    SeededFinePathMetadata {
        schema_version: SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
        path_record_schema_version: SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION,
        event_projection_record_schema_version: SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION,
        transition_ordering_policy: SEEDED_TICK_EVENT_ORDERING_POLICY,
        timestamp_representation_policy: SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY,
        // Metadata is persisted independently from the run config.
        timestamp_batching_observation_model: timestamp_batching_observation_model.clone(),
        side_encoding_policy: SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY,
        state_snapshot_capture,
        state_snapshot_capture_point_policy: state_snapshot_capture.capture_point_policy(),
    }
}

pub(crate) const fn build_seeded_tick_event_debug_trace_metadata(
    debug_trace_options: SeededTickSimulationDebugTraceOptions,
) -> SeededTickEventDebugTraceMetadata {
    SeededTickEventDebugTraceMetadata {
        schema_version: SEEDED_TICK_EVENT_DEBUG_TRACE_METADATA_SCHEMA_VERSION,
        trace_record_schema_version: SEEDED_TICK_EVENT_DEBUG_TRACE_RECORD_SCHEMA_VERSION,
        trace_chunk_schema_version: SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION,
        transition_ordering_policy: SEEDED_TICK_EVENT_ORDERING_POLICY,
        state_carryover_policy: SEEDED_TICK_EVENT_STATE_CARRYOVER_POLICY,
        news_market_state_routing_policy: SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY,
        sampling_policy: SEEDED_TICK_EVENT_DEBUG_TRACE_SAMPLING_POLICY,
        chunking_policy: SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNKING_POLICY,
        sample_every_nth_transition: debug_trace_options.sample_every_nth_transition,
        max_records_per_chunk: debug_trace_options.max_records_per_chunk,
        interval_boundary_drift_always_captured: true,
    }
}
