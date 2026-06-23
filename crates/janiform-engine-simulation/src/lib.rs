//! Simulation-side interval factor grid loading through the shared provider interface.

mod artifacts;
mod coarse;
mod config;
mod constants;
mod core;
mod error;
mod hawkes;
mod long_variance;
mod progress;
mod returns;
mod seeded;

pub use artifacts::{
    SeededFinePathMetadata, SeededFinePathOutput, SeededFinePathRecord,
    SeededFinePathTransitionKind, SeededTickEventDebugTraceChunk,
    SeededTickEventDebugTraceMetadata, SeededTickEventDebugTraceOutput,
    SeededTickEventDebugTraceRecord, SeededTickEventDebugTraceTransitionKind,
    SeededTickEventRecord, SeededTickEventSide, SeededTickEventStateSnapshot,
    SeededTickEventTapeMetadata, SeededTickEventTapeOutput,
    SeededTickSimulationInterarrivalDiagnostics, SeededTickSimulationNativeDiagnostics,
    SeededTickSimulationNewsEventMarketState, SeededTickSimulationNewsRoutingTotals,
    SeededTickSimulationRunResult, SeededTickSimulationStreamCompletion,
    SeededTickSimulationTimingDiagnostics, SeededTickSimulationTransitionConsumer,
    SeededTickSimulationTransitionEmission,
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
    CoarseSimulationPathRecord, CoarseSimulationPathRecordError, CoarseSimulationRunError,
    CoarseSimulationRunRequest, CoarseSimulationRunRequestError, run_coarse_simulation,
    run_coarse_simulation_with_observer, run_coarse_simulation_with_randomness_factory,
    run_coarse_simulation_with_randomness_factory_and_observer,
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
    SEEDED_TICK_EVENT_LATENT_INTERVAL_OWNERSHIP_POLICY,
    SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY,
    SEEDED_TICK_EVENT_OBSERVED_TIMESTAMP_INTERVAL_OWNERSHIP_POLICY,
    SEEDED_TICK_EVENT_ORDERING_POLICY, SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY,
    SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
    SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION, SEEDED_TICK_EVENT_RUN_CONFIG_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY, SEEDED_TICK_EVENT_STATE_CARRYOVER_POLICY,
    SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY,
    SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY,
    SEEDED_TICK_EVENT_TIMING_DIAGNOSTICS_SCHEMA_VERSION,
};
pub use core::project_core_increment_sequence_to_tick_grid;
pub use error::SeededTickSimulationRunError;
pub use hawkes::project_hawkes_intensities_simulation_interval;
pub use janiform_market_time::interval_factor_grids::provide_interval_factor_grid as load_interval_factor_grid;
pub use janiform_model_kernels::hawkes::evolve_hawkes_state;
pub use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorPathMaterializationError, MeanReversionBoundaryState,
    MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
    materialize_mean_reversion_runtime_interval_anchors_from_anchor_increments,
};
pub use long_variance::LongVarianceTransitionSamplingError;
pub use progress::{SimulationProgressEvent, SimulationProgressPhase};
pub use returns::{
    sample_nig_trading_time_increment, sample_nig_trading_time_increment_with_edge_mode,
};
pub use seeded::{
    estimate_seeded_result_capacities, run_seeded_tick_event_simulation,
    run_seeded_tick_event_simulation_with_observer,
    run_seeded_tick_event_simulation_with_randomness_factory,
    run_seeded_tick_event_simulation_with_randomness_factory_and_observer,
    stream_seeded_tick_event_simulation, stream_seeded_tick_event_simulation_with_observer,
    stream_seeded_tick_event_simulation_with_randomness_factory,
    stream_seeded_tick_event_simulation_with_randomness_factory_and_observer,
};
