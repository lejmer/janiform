//! Seeded simulation event tape and debug trace artifacts.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::calendar::MarketState;
use crate::hawkes::HawkesExcitationState;
use crate::news::{NewsArrivalComponent, NewsReturnJumpRoutingTarget};

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
    pub timestamp_utc_nanos: i64,
    pub side: SeededTickEventSide,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_snapshot: Option<SeededTickEventStateSnapshot>,
}

/// Machine-readable event tape metadata for deterministic serialization and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SeededTickEventTapeMetadata {
    pub schema_version: &'static str,
    pub event_record_schema_version: &'static str,
    pub state_snapshot_schema_version: &'static str,
    pub event_ordering_policy: &'static str,
    pub timestamp_representation_policy: &'static str,
    pub side_encoding_policy: &'static str,
    pub state_snapshot_capture: SeededTickEventStateSnapshotCapture,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_snapshot_capture_point_policy: Option<&'static str>,
}

/// Canonical serializable event tape payload for deterministic run outputs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededTickEventTapeOutput<'a> {
    pub schema_version: &'static str,
    pub metadata: SeededTickEventTapeMetadata,
    pub event_tape: &'a [SeededTickEventRecord],
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
    pub previous_timestamp_utc_nanos: i64,
    pub timestamp_utc_nanos: i64,
    pub elapsed_hours_since_previous_update: f64,
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

impl Default for SeededTickSimulationNewsRoutingTotals {
    fn default() -> Self {
        Self {
            cumulative_activity_coupling_j_t_increment: 0.0,
            cumulative_open_window_return_jump_m_t_increment: 0.0,
            cumulative_closed_window_gap_g_t_increment: 0.0,
        }
    }
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
    pub event_tape_metadata: SeededTickEventTapeMetadata,
    pub event_path: Vec<SeededTickEventRecord>,
    pub final_excitation_state: HawkesExcitationState,
    pub news_routing_totals: SeededTickSimulationNewsRoutingTotals,
    pub debug_trace_enabled: bool,
    pub debug_trace_output: Option<SeededTickEventDebugTraceOutput>,
}

impl SeededTickSimulationRunResult {
    /// Build the canonical machine-readable event tape payload.
    pub fn event_tape_output(&self) -> SeededTickEventTapeOutput<'_> {
        SeededTickEventTapeOutput {
            schema_version: self.schema_version,
            metadata: self.event_tape_metadata,
            event_tape: &self.event_path,
        }
    }

    /// Serialize the canonical event tape payload to deterministic JSON.
    pub fn serialize_event_tape_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.event_tape_output())
    }

    /// Serialize the optional debug trace artifact to deterministic JSON.
    pub fn serialize_debug_trace_json(&self) -> Result<Option<String>, serde_json::Error> {
        self.debug_trace_output
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
    }
}
