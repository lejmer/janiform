use std::path::PathBuf;

use chrono::{DateTime, Utc};
use janiform::model::factors::hourly::NANOS_PER_HOUR;
use janiform::model::hawkes::HawkesExcitationState;
use janiform::simulation::{MeanReversionBoundaryState, SeededTickEventStateSnapshotCapture};
use janiform_foundations::time::deserialize_utc_datetime;
use serde::Deserialize;
use serde_json::Value;

use super::artifacts::{DEFAULT_SIMULATION_EXPORT_BATCH_SIZE, SimulationOutputColumns};
use super::direct::{DirectSimulationExportPlan, DirectSimulationJob};

const DEFAULT_SIMULATION_RUN_ID: &str = "simulation";

/// Shared workflow metadata for simulation execution.
#[derive(Debug, Default, Deserialize)]
pub struct SimulationWorkflowMetadata {
    /// Optional stable workflow run id shown in terminal summaries.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Optional request artifact path when the boundary loaded the request from disk.
    #[serde(default)]
    pub request_path: Option<PathBuf>,
    /// Optional primary output path.
    #[serde(default)]
    pub output_path: Option<PathBuf>,
}

impl SimulationWorkflowMetadata {
    /// Return the configured run id, or the default simulation run id.
    pub fn run_id(&self) -> &str {
        self.run_id.as_deref().unwrap_or(DEFAULT_SIMULATION_RUN_ID)
    }

    /// Fill in the default simulation run id when no explicit id was provided.
    pub fn into_defaulted_run_id(mut self) -> Self {
        if self.run_id.is_none() {
            self.run_id = Some(DEFAULT_SIMULATION_RUN_ID.to_string());
        }

        self
    }
}

/// Workflow request for simulation execution.
pub struct SimulationWorkflowRequest {
    /// Shared workflow metadata.
    pub metadata: SimulationWorkflowMetadata,
    /// Direct simulation job.
    pub job: DirectSimulationJob,
    /// Optional export plan for writing direct simulation outputs.
    pub export_plan: Option<DirectSimulationExportPlan>,
}

const DEFAULT_SIMULATION_INTERVAL_DURATION_NANOS: i64 = NANOS_PER_HOUR;
const DEFAULT_SEEDED_DEBUG_TRACE_MAX_RECORDS_PER_CHUNK: usize = 1024;

/// Complete simulation-from-model-artifact workflow request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelArtifactSimulationWorkflowRequest {
    /// Shared workflow metadata.
    #[serde(flatten)]
    pub metadata: SimulationWorkflowMetadata,
    /// Persisted `JaniformModel` payload.
    pub model_payload: Value,
    /// Simulation kind selected by the public API.
    pub simulation_kind: ModelArtifactSimulationKind,
    /// Deterministic run seed.
    pub seed: u64,
    /// First simulated interval start.
    #[serde(deserialize_with = "deserialize_utc_datetime")]
    pub start_time_utc: DateTime<Utc>,
    /// Number of simulated intervals.
    pub horizon_interval_count: usize,
    /// Runtime interval width in UTC nanoseconds.
    #[serde(default = "default_simulation_interval_duration_nanos")]
    pub interval_duration_nanos: i64,
    /// Output column contract.
    pub output_columns: SimulationOutputColumns,
    /// Optional output reference price.
    #[serde(default = "default_reference_price")]
    pub reference_price: f64,
    /// Batch size for exported simulation records.
    #[serde(default = "default_simulation_export_batch_size")]
    pub export_batch_size: usize,
    /// Optional continuation observations used to estimate missing initial state.
    pub continuation_data: Option<ModelArtifactSimulationContinuationData>,
    /// Optional explicit initial Hawkes state.
    pub initial_excitation_state: Option<HawkesExcitationState>,
    /// Optional explicit initial mean-reversion boundary state.
    pub initial_mean_reversion_boundary_state: Option<MeanReversionBoundaryState>,
    /// Enable seeded debug trace collection.
    #[serde(default)]
    pub debug_trace_enabled: bool,
    /// Seeded debug trace sampling stride.
    #[serde(default = "default_sample_every_nth_transition")]
    pub sample_every_nth_transition: u64,
    /// Seeded debug trace chunk size.
    #[serde(default = "default_max_records_per_chunk")]
    pub max_records_per_chunk: usize,
    /// Seeded state snapshot capture mode.
    #[serde(default)]
    pub state_snapshot_capture: SeededTickEventStateSnapshotCapture,
}

/// Model-artifact simulation kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ModelArtifactSimulationKind {
    /// Seeded tick-event simulation.
    SeededTick,
    /// Coarse interval simulation.
    CoarseInterval,
}

/// Boundary payload for continuation-based current state estimation.
#[derive(Debug, Deserialize)]
pub struct ModelArtifactSimulationContinuationData {
    /// Observation timestamp column in UTC nanoseconds.
    pub observation_timestamp_utc_nanos: Vec<i64>,
    /// Observed trade prices.
    pub observation_trade_prices: Vec<f64>,
    /// Observed trade sides encoded as `buy` or `sell`.
    pub observation_trade_sides: Vec<String>,
    /// Optional current spot price used for observed log-price derivation.
    pub observed_price: Option<f64>,
}

fn default_simulation_interval_duration_nanos() -> i64 {
    DEFAULT_SIMULATION_INTERVAL_DURATION_NANOS
}

fn default_reference_price() -> f64 {
    1.0
}

fn default_simulation_export_batch_size() -> usize {
    DEFAULT_SIMULATION_EXPORT_BATCH_SIZE
}

fn default_sample_every_nth_transition() -> u64 {
    1
}

fn default_max_records_per_chunk() -> usize {
    DEFAULT_SEEDED_DEBUG_TRACE_MAX_RECORDS_PER_CHUNK
}
