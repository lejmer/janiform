//! Coarse-interval simulation workflow contracts and time-indexed path artifacts.

mod constants;
mod metadata;
mod model_inputs;
mod path;
mod record;
mod request;
mod time_index;

pub use constants::{
    COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY, COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY,
    COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY,
    COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY,
    COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY, COARSE_SIMULATION_INTERVAL_TIMING_POLICY,
    COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION,
    COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
    COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION, COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION,
    COARSE_SIMULATION_PATH_SCHEMA_VERSION, COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
};
pub use model_inputs::{CoarseSimulationModelInputs, CoarseSimulationModelInputsError};
pub use path::{
    CoarseSimulationPath, CoarseSimulationPathDiagnostics, CoarseSimulationPathError,
    CoarseSimulationPathOutputSchemaMetadata, CoarseSimulationPathProvenance,
};
pub use record::{CoarseSimulationPathRecord, CoarseSimulationPathRecordError};
pub use request::{CoarseSimulationRunRequest, CoarseSimulationRunRequestError};
pub(super) use time_index::IntervalDeltaGridError;
