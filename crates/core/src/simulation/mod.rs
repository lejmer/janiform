//! Simulation-side hourly factor loading through the shared provider interface.

mod api;
mod artifacts;
mod coarse;
mod coarse_runtime;
mod config;
mod constants;
mod error;
mod runtime;

pub use api::{
    evolve_hawkes_state, load_hourly_factor_contract,
    project_hawkes_intensities_simulation_interval, sample_nig_trading_time_increment,
    sample_nig_trading_time_increment_with_edge_mode,
};
pub use artifacts::{
    SeededFinePathMetadata, SeededFinePathOutput, SeededFinePathRecord,
    SeededFinePathTransitionKind, SeededTickEventDebugTraceChunk,
    SeededTickEventDebugTraceMetadata, SeededTickEventDebugTraceOutput,
    SeededTickEventDebugTraceRecord, SeededTickEventDebugTraceTransitionKind,
    SeededTickEventRecord, SeededTickEventSide, SeededTickEventStateSnapshot,
    SeededTickEventTapeMetadata, SeededTickEventTapeOutput,
    SeededTickSimulationNewsEventMarketState, SeededTickSimulationNewsRoutingTotals,
    SeededTickSimulationRunResult,
};
pub use coarse::{
    COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY, COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY,
    COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY,
    COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY,
    COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY, COARSE_SIMULATION_INTERVAL_TIMING_POLICY,
    COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION,
    COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
    COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION, COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION,
    COARSE_SIMULATION_PATH_SCHEMA_VERSION, COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
    CoarseSimulationModelInputs, CoarseSimulationModelInputsError, CoarseSimulationPath,
    CoarseSimulationPathDiagnostics, CoarseSimulationPathError,
    CoarseSimulationPathOutputSchemaMetadata, CoarseSimulationPathProvenance,
    CoarseSimulationPathRecord, CoarseSimulationPathRecordError, CoarseSimulationRunRequest,
    CoarseSimulationRunRequestError,
};
pub use coarse_runtime::{
    CoarseSimulationRunError, run_coarse_simulation, run_coarse_simulation_with_randomness_factory,
};
pub use config::{
    SeededTickEventStateSnapshotCapture, SeededTickSimulationDebugTraceOptions,
    SeededTickSimulationModelInputs, SeededTickSimulationRunConfig, SeededTickSimulationRunOptions,
};
pub use constants::{
    SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNKING_POLICY,
    SEEDED_TICK_EVENT_DEBUG_TRACE_METADATA_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_RECORD_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_SAMPLING_POLICY, SEEDED_TICK_EVENT_DEBUG_TRACE_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DETERMINISM_POLICY, SEEDED_TICK_EVENT_FACTOR_CONTROL_POLICY,
    SEEDED_TICK_EVENT_INTERVAL_BOUNDARY_POLICY, SEEDED_TICK_EVENT_INTRA_INTERVAL_SCHEDULING_POLICY,
    SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY, SEEDED_TICK_EVENT_ORDERING_POLICY,
    SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION, SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY,
    SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
    SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION, SEEDED_TICK_EVENT_RUN_CONFIG_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY, SEEDED_TICK_EVENT_STATE_CARRYOVER_POLICY,
    SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY,
    SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY,
};
pub use error::SeededTickSimulationRunError;
pub use runtime::{
    run_seeded_tick_event_simulation, run_seeded_tick_event_simulation_with_randomness_factory,
};

#[cfg(test)]
mod tests;
