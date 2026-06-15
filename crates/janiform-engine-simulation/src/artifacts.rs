//! Seeded simulation fine-path, event-tape projection, and debug trace artifacts.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::mem::size_of;

use janiform_affine_oracle::sufficient_statistics::SufficientStatisticsReturnComponents;
use janiform_market_time::calendar::MarketState;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::news::{NewsArrivalComponent, NewsReturnJumpRoutingTarget};
use janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel;

use super::config::SeededTickEventStateSnapshotCapture;

/// Side label for one simulated buy or sell event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeededTickEventSide {
    Buy,
    Sell,
}

/// Optional Hawkes state snapshot attached to one event tape record.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SeededTickEventStateSnapshot {
    pub y_plus_plus: f64,
    pub y_plus_minus: f64,
    pub y_minus_plus: f64,
    pub y_minus_minus: f64,
    pub deseasonalized_lambda_plus: f64,
    pub deseasonalized_lambda_minus: f64,
}

impl SeededTickEventStateSnapshot {
    pub(crate) const fn from_state(state: HawkesExcitationState, lambda_bar: f64) -> Self {
        let deseasonalized_intensities = state.project_deseasonalized_intensities(lambda_bar);

        Self {
            y_plus_plus: state.y_plus_plus,
            y_plus_minus: state.y_plus_minus,
            y_minus_plus: state.y_minus_plus,
            y_minus_minus: state.y_minus_minus,
            deseasonalized_lambda_plus: deseasonalized_intensities.lambda_plus,
            deseasonalized_lambda_minus: deseasonalized_intensities.lambda_minus,
        }
    }
}

/// One deterministic event-path record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededTickEventRecord {
    pub event_index: u64,
    pub latent_event_time_utc_nanos: i64,
    pub observed_timestamp_utc_nanos: i64,
    pub side: SeededTickEventSide,
    pub core_total_return_increment: f64,
    pub observed_log_price: f64,
    pub anchor_value: f64,
    pub pressure_state: f64,
    pub deviation_from_anchor: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_snapshot: Option<SeededTickEventStateSnapshot>,
}

/// Public transition kind emitted by the canonical seeded fine path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeededFinePathTransitionKind {
    BuyTrade,
    SellTrade,
    ExogenousNews,
    IntervalBoundaryDrift,
}

/// Machine-readable metadata for one serialized fine path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeededFinePathMetadata {
    pub schema_version: &'static str,
    pub path_record_schema_version: &'static str,
    pub event_projection_record_schema_version: &'static str,
    pub transition_ordering_policy: &'static str,
    pub timestamp_representation_policy: &'static str,
    pub timestamp_batching_observation_model: TimestampBatchingObservationModel,
    pub side_encoding_policy: &'static str,
    pub state_snapshot_capture: SeededTickEventStateSnapshotCapture,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_snapshot_capture_point_policy: Option<&'static str>,
}

/// One ordered fine-path transition with realized return decomposition.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededFinePathRecord {
    pub transition_index: u64,
    pub latent_transition_time_utc_nanos: i64,
    pub observed_timestamp_utc_nanos: i64,
    pub transition_kind: SeededFinePathTransitionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<SeededTickEventSide>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub news_arrival_component: Option<NewsArrivalComponent>,
    pub market_state: SeededTickSimulationNewsEventMarketState,
    pub return_components: SufficientStatisticsReturnComponents,
    pub core_total_return_increment: f64,
    pub observed_log_price: f64,
    pub anchor_value: f64,
    pub pressure_state: f64,
    pub deviation_from_anchor: f64,
    pub cumulative_core_log_return: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_snapshot: Option<SeededTickEventStateSnapshot>,
}

/// Canonical serializable fine-path payload for deterministic run outputs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededFinePathOutput<'a> {
    pub schema_version: &'static str,
    pub metadata: SeededFinePathMetadata,
    pub fine_path: &'a [SeededFinePathRecord],
}

/// Machine-readable event tape metadata for deterministic serialization and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeededTickEventTapeMetadata {
    pub schema_version: &'static str,
    pub event_record_schema_version: &'static str,
    pub state_snapshot_schema_version: &'static str,
    pub event_ordering_policy: &'static str,
    pub timestamp_representation_policy: &'static str,
    pub timestamp_batching_observation_model: TimestampBatchingObservationModel,
    pub side_encoding_policy: &'static str,
    pub state_snapshot_capture: SeededTickEventStateSnapshotCapture,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_snapshot_capture_point_policy: Option<&'static str>,
}

/// Canonical serializable event tape payload for deterministic run outputs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededTickEventTapeOutput {
    pub schema_version: &'static str,
    pub metadata: SeededTickEventTapeMetadata,
    pub event_tape: Vec<SeededTickEventRecord>,
}

/// Transition kind emitted by debug trace records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeededTickEventDebugTraceTransitionKind {
    EndogenousBuyEvent,
    EndogenousSellEvent,
    ExogenousNews,
    IntervalBoundaryDrift,
}

/// News market-state label captured in seeded simulation debug traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeededTickSimulationNewsEventMarketState {
    Open,
    Closed,
}

impl SeededTickSimulationNewsEventMarketState {
    pub(crate) const fn from_market_state(market_state: MarketState) -> Self {
        match market_state {
            MarketState::Open => Self::Open,
            MarketState::Closed => Self::Closed,
        }
    }
}

/// Per-transition diagnostic record emitted in debug trace mode.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededTickEventDebugTraceRecord {
    pub transition_index: u64,
    pub interval_index: usize,
    pub previous_latent_transition_time_utc_nanos: i64,
    pub latent_transition_time_utc_nanos: i64,
    pub observed_timestamp_utc_nanos: i64,
    pub elapsed_hours_since_previous_latent_update: f64,
    pub transition_kind: SeededTickEventDebugTraceTransitionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_order: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exogenous_news_arrival_component: Option<NewsArrivalComponent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exogenous_news_activity_coupling_mark: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exogenous_news_return_jump_mark: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exogenous_news_market_state: Option<SeededTickSimulationNewsEventMarketState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exogenous_news_return_jump_routing_target: Option<NewsReturnJumpRoutingTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exogenous_news_open_window_return_jump_increment: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exogenous_news_closed_window_gap_increment: Option<f64>,
    pub cumulative_news_activity_coupling_j_t_increment: f64,
    pub cumulative_open_window_return_jump_m_t_increment: f64,
    pub cumulative_closed_window_gap_g_t_increment: f64,
    pub seasonality_factor: f64,
    pub interval_news_intensity: f64,
    pub state_before_transition: SeededTickEventStateSnapshot,
    pub state_after_transition: SeededTickEventStateSnapshot,
}

/// Chunk container for bounded-size debug trace payloads.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededTickEventDebugTraceChunk {
    pub schema_version: &'static str,
    pub chunk_index: usize,
    pub start_transition_index: u64,
    pub end_transition_index: u64,
    pub records: Vec<SeededTickEventDebugTraceRecord>,
}

/// Machine-readable debug trace metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SeededTickEventDebugTraceMetadata {
    pub schema_version: &'static str,
    pub trace_record_schema_version: &'static str,
    pub trace_chunk_schema_version: &'static str,
    pub transition_ordering_policy: &'static str,
    pub state_carryover_policy: &'static str,
    pub news_market_state_routing_policy: &'static str,
    pub sampling_policy: &'static str,
    pub chunking_policy: &'static str,
    pub sample_every_nth_transition: u64,
    pub max_records_per_chunk: usize,
    pub interval_boundary_drift_always_captured: bool,
}

/// Canonical serializable debug trace payload.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededTickEventDebugTraceOutput {
    pub schema_version: &'static str,
    pub metadata: SeededTickEventDebugTraceMetadata,
    pub captured_transition_count: usize,
    pub trace_chunks: Vec<SeededTickEventDebugTraceChunk>,
}

/// Cumulative news-routing diagnostics exposed on simulator outputs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SeededTickSimulationNewsRoutingTotals {
    pub cumulative_activity_coupling_j_t_increment: f64,
    pub cumulative_open_window_return_jump_m_t_increment: f64,
    pub cumulative_closed_window_gap_g_t_increment: f64,
}

/// Interarrival summary for one timestamp axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SeededTickSimulationInterarrivalDiagnostics {
    pub interarrival_count: usize,
    pub zero_interarrival_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_positive_interarrival_nanos: Option<i64>,
}

/// Timing diagnostics separating latent event arrivals from observed timestamps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeededTickSimulationTimingDiagnostics {
    pub schema_version: &'static str,
    pub timestamp_batching_observation_model: TimestampBatchingObservationModel,
    pub latent_event_interarrivals: SeededTickSimulationInterarrivalDiagnostics,
    pub observed_timestamp_interarrivals: SeededTickSimulationInterarrivalDiagnostics,
    pub latent_interval_ownership_policy: &'static str,
    pub observed_timestamp_interval_ownership_policy: &'static str,
    pub coarse_interval_compatibility_policy: String,
}

impl Default for SeededTickSimulationNewsRoutingTotals {
    fn default() -> Self {
        Self {
            cumulative_activity_coupling_j_t_increment: 0.0,
            cumulative_open_window_return_jump_m_t_increment: 0.0,
            cumulative_closed_window_gap_g_t_increment: 0.0,
        }
    }
}

/// Approximate retained-memory diagnostics for the canonical native seeded result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SeededTickSimulationNativeDiagnostics {
    pub transition_count: usize,
    pub public_transition_count: usize,
    pub event_count: usize,
    pub estimated_fine_path_bytes: usize,
    pub estimated_public_projection_index_bytes: usize,
    pub estimated_event_projection_index_bytes: usize,
    pub estimated_debug_trace_bytes: usize,
    pub estimated_total_retained_bytes: usize,
}

/// Visibility flags for one streamed transition emitted by the seeded simulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeededTickSimulationTransitionEmission {
    pub is_public_fine_path_record: bool,
    pub is_event_projection_record: bool,
}

/// Final deterministic metadata produced by one streamed seeded simulation pass.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTickSimulationStreamCompletion {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub event_ordering_policy: &'static str,
    pub randomness_stream_policy: &'static str,
    pub randomness_substream_derivation_policy: &'static str,
    pub seed: u64,
    pub start_time_utc: DateTime<Utc>,
    pub end_time_utc: DateTime<Utc>,
    pub simulated_interval_count: usize,
    pub fine_path_metadata: SeededFinePathMetadata,
    pub event_tape_metadata: SeededTickEventTapeMetadata,
    pub timing_diagnostics: SeededTickSimulationTimingDiagnostics,
    pub final_excitation_state: HawkesExcitationState,
    pub news_routing_totals: SeededTickSimulationNewsRoutingTotals,
    pub cumulative_core_log_return: f64,
    pub debug_trace_enabled: bool,
    pub debug_trace_output: Option<SeededTickEventDebugTraceOutput>,
}

/// Consumer interface for streamed deterministic seeded transitions.
pub trait SeededTickSimulationTransitionConsumer {
    /// Reserve retained storage using the simulator's deterministic capacity estimate.
    fn reserve_transition_capacity(
        &mut self,
        _estimated_transition_capacity: usize,
        _estimated_event_capacity: usize,
    ) -> Result<(), crate::error::SeededTickSimulationRunError> {
        Ok(())
    }

    /// Consume one deterministic transition in canonical simulation order.
    fn consume_transition(
        &mut self,
        record: SeededFinePathRecord,
        emission: SeededTickSimulationTransitionEmission,
    ) -> Result<(), crate::error::SeededTickSimulationRunError>;
}

/// Deterministic seeded event-path output object.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTickSimulationRunResult {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub event_ordering_policy: &'static str,
    pub randomness_stream_policy: &'static str,
    pub randomness_substream_derivation_policy: &'static str,
    pub seed: u64,
    pub start_time_utc: DateTime<Utc>,
    pub end_time_utc: DateTime<Utc>,
    pub simulated_interval_count: usize,
    pub fine_path_metadata: SeededFinePathMetadata,
    pub fine_path: Vec<SeededFinePathRecord>,
    pub public_transition_positions: Vec<usize>,
    pub event_tape_metadata: SeededTickEventTapeMetadata,
    pub event_transition_positions: Vec<usize>,
    pub timing_diagnostics: SeededTickSimulationTimingDiagnostics,
    pub final_excitation_state: HawkesExcitationState,
    pub news_routing_totals: SeededTickSimulationNewsRoutingTotals,
    pub cumulative_core_log_return: f64,
    pub debug_trace_enabled: bool,
    pub debug_trace_output: Option<SeededTickEventDebugTraceOutput>,
}

impl SeededTickSimulationRunResult {
    /// Number of canonical fine-path transitions retained by the run result.
    pub fn transition_count(&self) -> usize {
        self.fine_path.len()
    }

    /// Number of public fine-path transitions exposed through the Python surface.
    pub fn public_transition_count(&self) -> usize {
        self.public_transition_positions.len()
    }

    /// Number of endogenous event projections derivable from the canonical fine path.
    pub fn event_count(&self) -> usize {
        self.event_transition_positions.len()
    }

    /// Borrow one contiguous canonical fine-path chunk without duplicating records.
    pub fn fine_path_chunk(
        &self,
        start_index: usize,
        max_records: usize,
    ) -> &[SeededFinePathRecord] {
        if max_records == 0 || start_index >= self.fine_path.len() {
            return &[];
        }

        let end_index = start_index
            .saturating_add(max_records)
            .min(self.fine_path.len());

        &self.fine_path[start_index..end_index]
    }

    /// Project one contiguous public fine-path chunk from the canonical fine path.
    pub fn public_fine_path_chunk(
        &self,
        start_index: usize,
        max_records: usize,
    ) -> Vec<SeededFinePathRecord> {
        if max_records == 0 || start_index >= self.public_transition_positions.len() {
            return Vec::new();
        }

        let end_index = start_index
            .saturating_add(max_records)
            .min(self.public_transition_positions.len());
        let mut public_records: Vec<SeededFinePathRecord> =
            Vec::with_capacity(end_index - start_index);
        for public_index in start_index..end_index {
            let transition_position = self.public_transition_positions[public_index];
            public_records.push(self.fine_path[transition_position].clone());
        }

        public_records
    }

    /// Project one endogenous event record from the canonical fine path.
    pub fn event_record(&self, event_index: usize) -> Option<SeededTickEventRecord> {
        let transition_position = *self.event_transition_positions.get(event_index)?;
        let fine_path_record = self.fine_path.get(transition_position)?;
        let side = fine_path_record.side?;

        Some(SeededTickEventRecord {
            event_index: event_index as u64,
            latent_event_time_utc_nanos: fine_path_record.latent_transition_time_utc_nanos,
            observed_timestamp_utc_nanos: fine_path_record.observed_timestamp_utc_nanos,
            side,
            core_total_return_increment: fine_path_record.core_total_return_increment,
            observed_log_price: fine_path_record.observed_log_price,
            anchor_value: fine_path_record.anchor_value,
            pressure_state: fine_path_record.pressure_state,
            deviation_from_anchor: fine_path_record.deviation_from_anchor,
            state_snapshot: fine_path_record.state_snapshot,
        })
    }

    /// Project one contiguous event chunk from the canonical fine path.
    pub fn event_chunk(
        &self,
        start_index: usize,
        max_records: usize,
    ) -> Vec<SeededTickEventRecord> {
        if max_records == 0 || start_index >= self.event_transition_positions.len() {
            return Vec::new();
        }

        let end_index = start_index
            .saturating_add(max_records)
            .min(self.event_transition_positions.len());
        let mut event_records: Vec<SeededTickEventRecord> =
            Vec::with_capacity(end_index - start_index);
        for event_index in start_index..end_index {
            if let Some(event_record) = self.event_record(event_index) {
                event_records.push(event_record);
            }
        }

        event_records
    }

    /// Project the full endogenous event path from the canonical fine path on demand.
    pub fn projected_event_path(&self) -> Vec<SeededTickEventRecord> {
        self.event_chunk(0, self.event_count())
    }

    /// Approximate native retained-memory diagnostics for one run result.
    pub fn native_diagnostics(&self) -> SeededTickSimulationNativeDiagnostics {
        let estimated_fine_path_bytes = self
            .fine_path
            .capacity()
            .saturating_mul(size_of::<SeededFinePathRecord>());
        let estimated_public_projection_index_bytes = self
            .public_transition_positions
            .capacity()
            .saturating_mul(size_of::<usize>());
        let estimated_event_projection_index_bytes = self
            .event_transition_positions
            .capacity()
            .saturating_mul(size_of::<usize>());
        let estimated_debug_trace_bytes = self
            .debug_trace_output
            .as_ref()
            .map(|debug_trace_output| {
                debug_trace_output
                    .trace_chunks
                    .iter()
                    .map(|trace_chunk| {
                        trace_chunk
                            .records
                            .capacity()
                            .saturating_mul(size_of::<SeededTickEventDebugTraceRecord>())
                    })
                    .sum::<usize>()
            })
            .unwrap_or(0);

        SeededTickSimulationNativeDiagnostics {
            transition_count: self.transition_count(),
            public_transition_count: self.public_transition_count(),
            event_count: self.event_count(),
            estimated_fine_path_bytes,
            estimated_public_projection_index_bytes,
            estimated_event_projection_index_bytes,
            estimated_debug_trace_bytes,
            estimated_total_retained_bytes: estimated_fine_path_bytes
                .saturating_add(estimated_public_projection_index_bytes)
                .saturating_add(estimated_event_projection_index_bytes)
                .saturating_add(estimated_debug_trace_bytes),
        }
    }

    /// Build the canonical machine-readable fine-path payload.
    pub fn fine_path_output(&self) -> SeededFinePathOutput<'_> {
        SeededFinePathOutput {
            schema_version: self.schema_version,
            metadata: self.fine_path_metadata.clone(),
            fine_path: &self.fine_path,
        }
    }

    /// Serialize the canonical fine-path payload to deterministic JSON.
    pub fn serialize_fine_path_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.fine_path_output())
    }

    /// Build the canonical machine-readable event tape payload.
    pub fn event_tape_output(&self) -> SeededTickEventTapeOutput {
        SeededTickEventTapeOutput {
            schema_version: self.schema_version,
            metadata: self.event_tape_metadata.clone(),
            event_tape: self.projected_event_path(),
        }
    }

    /// Serialize the canonical event tape payload to deterministic JSON.
    pub fn serialize_event_tape_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.event_tape_output())
    }

    /// Serialize the timing diagnostics artifact to deterministic JSON.
    pub fn serialize_timing_diagnostics_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.timing_diagnostics)
    }

    /// Serialize the optional debug trace artifact to deterministic JSON.
    pub fn serialize_debug_trace_json(&self) -> Result<Option<String>, serde_json::Error> {
        self.debug_trace_output
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
    }
}
