//! Simulation-side hourly factor loading through the shared provider interface.

use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::calendar::MarketState;
use crate::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesIntensityProjection, HawkesKernelParameters,
    HawkesSeasonalityIntervalError, HawkesTransitionError, HawkesTransitionStep,
};
use crate::hourly_factors::HourlyFactorContractOutput;
use crate::news::{NewsArrivalComponent, NewsReturnJumpRoutingTarget};
use crate::nig::{
    NigBuySellDriverParameters, NigTradingTimeIncrementContributions,
    NigTradingTimeIncrementSamplingError,
};
use crate::randomness::{
    SIMULATION_RANDOMNESS_STREAM_POLICY, SIMULATION_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
    SimulationRandomnessFactory, SimulationRandomnessStream, SimulationRandomnessSubstreamDomain,
    SplitMix64SimulationRandomnessFactory,
};
use crate::return_decomposition::{ReturnDecomposition, ReturnDecompositionEdgeMode};

pub const SEEDED_TICK_EVENT_RUN_CONFIG_SCHEMA_VERSION: &str =
    "simulation.seeded_tick_event.run_config.v1";
pub const SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION: &str = "simulation.seeded_tick_event.path.v1";
pub const SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION: &str = "simulation.seeded_tick_event.record.v1";
pub const SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION: &str =
    "simulation.seeded_tick_event.tape_metadata.v1";
pub const SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION: &str =
    "simulation.seeded_tick_event.state_snapshot.v1";
pub const SEEDED_TICK_EVENT_DEBUG_TRACE_SCHEMA_VERSION: &str =
    "simulation.seeded_tick_event.debug_trace.v1";
pub const SEEDED_TICK_EVENT_DEBUG_TRACE_METADATA_SCHEMA_VERSION: &str =
    "simulation.seeded_tick_event.debug_trace_metadata.v1";
pub const SEEDED_TICK_EVENT_DEBUG_TRACE_RECORD_SCHEMA_VERSION: &str =
    "simulation.seeded_tick_event.debug_trace_record.v1";
pub const SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION: &str =
    "simulation.seeded_tick_event.debug_trace_chunk.v1";
pub const SEEDED_TICK_EVENT_DETERMINISM_POLICY: &str =
    "stable_seed_and_run_config_with_ordered_event_path.v1";

/// Ordering policy for all emitted stochastic updates and recorded buy/sell arrivals.
///
/// Tie-break sequence:
/// 1. lower timestamp first
/// 2. endogenous events before exogenous news
/// 3. buy before sell when endogenous timestamps tie
/// 4. known before surprise for exogenous news timestamp ties
/// 5. generation order fallback for exact key collisions
pub const SEEDED_TICK_EVENT_ORDERING_POLICY: &str = "timestamp_then_endogenous_before_exogenous_then_buy_before_sell_then_known_before_surprise_then_generation_order.v1";
pub const SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY: &str =
    "event_time_timestamp_utc_nanos_without_bucket_identifier.v1";
pub const SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY: &str = "lowercase_buy_or_sell.v1";
pub const SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY: &str =
    "post_event_transition_pre_following_same_timestamp_updates.v1";
pub const SEEDED_TICK_EVENT_INTERVAL_BOUNDARY_POLICY: &str =
    "hourly_factor_start_inclusive_end_exclusive_with_strict_consecutive_coverage.v1";
pub const SEEDED_TICK_EVENT_STATE_CARRYOVER_POLICY: &str =
    "full_4d_hawkes_excitation_state_carryover_across_interval_boundaries.v1";
pub const SEEDED_TICK_EVENT_FACTOR_CONTROL_POLICY: &str =
    "interval_aligned_s_h_nu_h_control_intensity_only_without_intra_interval_grid.v1";
pub const SEEDED_TICK_EVENT_INTRA_INTERVAL_SCHEDULING_POLICY: &str =
    "event_time_updates_with_sequential_hawkes_transition_composition.v1";
pub const SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY: &str = "positive_interval_s_h_routes_news_return_jump_to_open_m_channel_zero_interval_s_h_routes_news_return_jump_to_closed_g_channel.v1";
pub const SEEDED_TICK_EVENT_DEBUG_TRACE_SAMPLING_POLICY: &str =
    "capture_every_nth_transition_with_mandatory_interval_boundary_drift_records.v1";
pub const SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNKING_POLICY: &str =
    "fixed_max_records_per_chunk_preserving_transition_order.v1";

pub const SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY: &str = SIMULATION_RANDOMNESS_STREAM_POLICY;
pub const SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY: &str =
    SIMULATION_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY;

const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;
const ONE_SECOND_NANOS: i64 = 1_000_000_000;
const MAX_STOCHASTIC_UPDATES_PER_INTERVAL: u64 = 1_000_000;
const DEFAULT_DEBUG_TRACE_SAMPLE_EVERY_NTH_TRANSITION: u64 = 1;
const DEFAULT_DEBUG_TRACE_MAX_RECORDS_PER_CHUNK: usize = 1_024;

/// State snapshot capture mode for per-event diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeededTickEventStateSnapshotCapture {
    Disabled,
    PostEventTransition,
}

impl SeededTickEventStateSnapshotCapture {
    const fn capture_point_policy(self) -> Option<&'static str> {
        match self {
            Self::Disabled => None,
            Self::PostEventTransition => {
                Some(SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY)
            }
        }
    }
}

impl Default for SeededTickEventStateSnapshotCapture {
    fn default() -> Self {
        Self::Disabled
    }
}

/// Optional debug trace controls for seeded simulation diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeededTickSimulationDebugTraceOptions {
    /// Capture every Nth transition record in deterministic transition order.
    ///
    /// Interval-boundary drift transitions are always recorded.
    pub sample_every_nth_transition: u64,

    /// Maximum number of records in each trace chunk.
    pub max_records_per_chunk: usize,
}

impl SeededTickSimulationDebugTraceOptions {
    fn validate(self) -> Result<(), SeededTickSimulationRunError> {
        if self.sample_every_nth_transition == 0 {
            return Err(
                SeededTickSimulationRunError::InvalidDebugTraceSamplingStride {
                    value: self.sample_every_nth_transition,
                },
            );
        }

        if self.max_records_per_chunk == 0 {
            return Err(SeededTickSimulationRunError::InvalidDebugTraceChunkSize {
                value: self.max_records_per_chunk,
            });
        }

        Ok(())
    }
}

impl Default for SeededTickSimulationDebugTraceOptions {
    fn default() -> Self {
        Self {
            sample_every_nth_transition: DEFAULT_DEBUG_TRACE_SAMPLE_EVERY_NTH_TRANSITION,
            max_records_per_chunk: DEFAULT_DEBUG_TRACE_MAX_RECORDS_PER_CHUNK,
        }
    }
}

/// Optional run settings for seeded tick-event simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeededTickSimulationRunOptions {
    /// Enable optional debug trace artifact emission.
    pub debug_trace_enabled: bool,

    /// Optional debug trace sampling and chunking controls.
    pub debug_trace_options: SeededTickSimulationDebugTraceOptions,

    /// Control optional Hawkes state snapshot capture on each event tape record.
    pub state_snapshot_capture: SeededTickEventStateSnapshotCapture,
}

impl Default for SeededTickSimulationRunOptions {
    fn default() -> Self {
        Self {
            debug_trace_enabled: false,
            debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }
    }
}

/// Per-component mark scaling controls for seeded simulation news arrivals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeededTickSimulationNewsMarkScales {
    pub activity_coupling_scale: f64,
    pub return_jump_scale: f64,
}

impl SeededTickSimulationNewsMarkScales {
    const fn default_unit() -> Self {
        Self {
            activity_coupling_scale: 1.0,
            return_jump_scale: 1.0,
        }
    }

    fn validate(self, component_label: &'static str) -> Result<(), SeededTickSimulationRunError> {
        if !self.activity_coupling_scale.is_finite() || self.activity_coupling_scale < 0.0 {
            return Err(
                SeededTickSimulationRunError::InvalidNewsComponentActivityCouplingScale {
                    component: component_label,
                    value: self.activity_coupling_scale,
                },
            );
        }
        if !self.return_jump_scale.is_finite() || self.return_jump_scale < 0.0 {
            return Err(
                SeededTickSimulationRunError::InvalidNewsComponentReturnJumpScale {
                    component: component_label,
                    value: self.return_jump_scale,
                },
            );
        }

        Ok(())
    }
}

impl Default for SeededTickSimulationNewsMarkScales {
    fn default() -> Self {
        Self::default_unit()
    }
}

/// Required model inputs for seeded tick-event simulation runs.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTickSimulationModelInputs {
    /// Canonical Hawkes kernel parameter payload.
    pub kernel_parameters: HawkesKernelParameters,

    /// Excitation state carried into the first simulated interval.
    pub initial_excitation_state: HawkesExcitationState,

    /// Interval-aligned hourly factors (`s_h`, `nu_h`) used by the simulator.
    pub hourly_factor_contract: HourlyFactorContractOutput,

    /// Known scheduled news arrivals in UTC nanos.
    pub known_scheduled_news_event_timestamps_utc_nanos: Vec<i64>,

    /// Known-component mark scaling controls for activity coupling and return jumps.
    pub known_news_mark_scales: SeededTickSimulationNewsMarkScales,

    /// Surprise-component mark scaling controls for activity coupling and return jumps.
    pub surprise_news_mark_scales: SeededTickSimulationNewsMarkScales,
}

/// Typed public run contract for deterministic seeded tick-event simulation.
///
/// Required fields:
/// - `seed`
/// - `start_time_utc`
/// - `horizon_interval_count`
/// - `model_inputs`
///
/// Optional fields:
/// - `options`, defaults to `SeededTickSimulationRunOptions::default()`
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTickSimulationRunConfig {
    pub seed: u64,
    pub start_time_utc: DateTime<Utc>,
    pub horizon_interval_count: usize,
    pub model_inputs: SeededTickSimulationModelInputs,
    pub options: SeededTickSimulationRunOptions,
}

impl SeededTickSimulationRunConfig {
    /// Build and validate one seeded run configuration payload.
    ///
    /// Failure modes:
    /// - invalid horizon value (`horizon_interval_count == 0`)
    /// - missing or incomplete hourly factor interval vectors
    /// - mismatched hourly factor diagnostics interval count
    /// - non-consecutive or invalid hourly factor interval bounds in simulated horizon
    /// - start time not aligned to first hourly factor interval start
    /// - requested horizon exceeds available factor intervals
    pub fn try_new(
        seed: u64,
        start_time_utc: DateTime<Utc>,
        horizon_interval_count: usize,
        model_inputs: SeededTickSimulationModelInputs,
        options: Option<SeededTickSimulationRunOptions>,
    ) -> Result<Self, SeededTickSimulationRunError> {
        let run_config = Self {
            seed,
            start_time_utc,
            horizon_interval_count,
            model_inputs,
            options: options.unwrap_or_default(),
        };

        run_config.validate()?;

        Ok(run_config)
    }

    pub fn validate(&self) -> Result<(), SeededTickSimulationRunError> {
        if self.horizon_interval_count == 0 {
            return Err(SeededTickSimulationRunError::InvalidHorizonIntervalCount { value: 0 });
        }

        let hourly_factor_contract = &self.model_inputs.hourly_factor_contract;
        let expected_interval_count = hourly_factor_contract.interval_indices.len();
        if expected_interval_count == 0 {
            return Err(SeededTickSimulationRunError::MissingHourlyFactorIntervals);
        }

        if hourly_factor_contract.diagnostics.interval_count != expected_interval_count {
            return Err(
                SeededTickSimulationRunError::HourlyFactorDiagnosticsIntervalCountMismatch {
                    expected_interval_count,
                    diagnostics_interval_count: hourly_factor_contract.diagnostics.interval_count,
                },
            );
        }

        validate_hourly_factor_field_length(
            "interval_start_utc_nanos",
            expected_interval_count,
            hourly_factor_contract.interval_start_utc_nanos.len(),
        )?;
        validate_hourly_factor_field_length(
            "interval_end_utc_nanos",
            expected_interval_count,
            hourly_factor_contract.interval_end_utc_nanos.len(),
        )?;
        validate_hourly_factor_field_length(
            "s_h",
            expected_interval_count,
            hourly_factor_contract.s_h.len(),
        )?;
        validate_hourly_factor_field_length(
            "nu_h",
            expected_interval_count,
            hourly_factor_contract.nu_h.len(),
        )?;

        if self.horizon_interval_count > expected_interval_count {
            return Err(
                SeededTickSimulationRunError::HorizonExceedsAvailableIntervals {
                    requested_interval_count: self.horizon_interval_count,
                    available_interval_count: expected_interval_count,
                },
            );
        }

        let start_time_utc_nanos = self.start_time_utc.timestamp_nanos_opt().ok_or(
            SeededTickSimulationRunError::StartTimeOutOfNanosecondRange {
                value: self.start_time_utc,
            },
        )?;
        let first_interval_start_utc_nanos = hourly_factor_contract.interval_start_utc_nanos[0];

        if start_time_utc_nanos != first_interval_start_utc_nanos {
            return Err(
                SeededTickSimulationRunError::StartTimeDoesNotMatchHourlyFactorCoverage {
                    start_time_utc_nanos,
                    first_interval_start_utc_nanos,
                },
            );
        }

        validate_hourly_factor_interval_coverage(
            &hourly_factor_contract.interval_start_utc_nanos,
            &hourly_factor_contract.interval_end_utc_nanos,
            self.horizon_interval_count,
        )?;

        validate_known_scheduled_news_timestamps(
            &self
                .model_inputs
                .known_scheduled_news_event_timestamps_utc_nanos,
        )?;
        self.model_inputs.known_news_mark_scales.validate("known")?;
        self.model_inputs
            .surprise_news_mark_scales
            .validate("surprise")?;

        if self.options.debug_trace_enabled {
            self.options.debug_trace_options.validate()?;
        }

        Ok(())
    }
}

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
    const fn from_state(state: HawkesExcitationState, lambda_bar: f64) -> Self {
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
    const fn from_market_state(market_state: MarketState) -> Self {
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

/// Structured simulator run failure with explicit validation diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub enum SeededTickSimulationRunError {
    InvalidHorizonIntervalCount {
        value: usize,
    },
    MissingHourlyFactorIntervals,
    HourlyFactorFieldLengthMismatch {
        field_name: &'static str,
        expected_interval_count: usize,
        observed_interval_count: usize,
    },
    HourlyFactorDiagnosticsIntervalCountMismatch {
        expected_interval_count: usize,
        diagnostics_interval_count: usize,
    },
    HorizonExceedsAvailableIntervals {
        requested_interval_count: usize,
        available_interval_count: usize,
    },
    StartTimeOutOfNanosecondRange {
        value: DateTime<Utc>,
    },
    StartTimeDoesNotMatchHourlyFactorCoverage {
        start_time_utc_nanos: i64,
        first_interval_start_utc_nanos: i64,
    },
    InvalidHourlyFactorIntervalBounds {
        interval_index: usize,
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
    NonConsecutiveHourlyFactorCoverage {
        previous_interval_index: usize,
        previous_interval_end_utc_nanos: i64,
        current_interval_index: usize,
        current_interval_start_utc_nanos: i64,
    },
    InvalidDebugTraceSamplingStride {
        value: u64,
    },
    InvalidDebugTraceChunkSize {
        value: usize,
    },
    InvalidKnownScheduledNewsTimestampUtcNanos {
        value: i64,
    },
    KnownScheduledNewsTimestampsNotStrictlyIncreasing {
        previous_timestamp_utc_nanos: i64,
        current_timestamp_utc_nanos: i64,
    },
    InvalidNewsComponentActivityCouplingScale {
        component: &'static str,
        value: f64,
    },
    InvalidNewsComponentReturnJumpScale {
        component: &'static str,
        value: f64,
    },
    InvalidGeneratedNewsActivityCouplingMark {
        value: f64,
    },
    InvalidGeneratedNewsReturnJumpMark {
        value: f64,
    },
    InvalidNewsRoutingStateViolation {
        market_state: MarketState,
        open_window_return_jump_increment: f64,
        closed_window_gap_increment: f64,
    },
    InvalidAccumulatedNewsRoutingTotal {
        field: &'static str,
        value: f64,
    },
    InvalidTransitionStep {
        source: HawkesTransitionError,
    },
}

impl Display for SeededTickSimulationRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be greater than 0, found {value}"
            ),
            Self::MissingHourlyFactorIntervals => write!(
                formatter,
                "hourly_factor_contract.interval_count must be greater than 0"
            ),
            Self::HourlyFactorFieldLengthMismatch {
                field_name,
                expected_interval_count,
                observed_interval_count,
            } => write!(
                formatter,
                "hourly_factor_contract.{field_name} length must match interval_count ({expected_interval_count}), found {observed_interval_count}"
            ),
            Self::HourlyFactorDiagnosticsIntervalCountMismatch {
                expected_interval_count,
                diagnostics_interval_count,
            } => write!(
                formatter,
                "hourly_factor_contract.diagnostics.interval_count must match interval_count ({expected_interval_count}), found {diagnostics_interval_count}"
            ),
            Self::HorizonExceedsAvailableIntervals {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "horizon_interval_count ({requested_interval_count}) exceeds hourly factor interval_count ({available_interval_count})"
            ),
            Self::StartTimeOutOfNanosecondRange { value } => write!(
                formatter,
                "start_time_utc cannot be represented in nanoseconds, found {value}"
            ),
            Self::StartTimeDoesNotMatchHourlyFactorCoverage {
                start_time_utc_nanos,
                first_interval_start_utc_nanos,
            } => write!(
                formatter,
                "start_time_utc nanos ({start_time_utc_nanos}) must match hourly factor first interval start ({first_interval_start_utc_nanos})"
            ),
            Self::InvalidHourlyFactorIntervalBounds {
                interval_index,
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "hourly_factor_contract interval {interval_index} has invalid bounds, start={interval_start_utc_nanos}, end={interval_end_utc_nanos}"
            ),
            Self::NonConsecutiveHourlyFactorCoverage {
                previous_interval_index,
                previous_interval_end_utc_nanos,
                current_interval_index,
                current_interval_start_utc_nanos,
            } => write!(
                formatter,
                "hourly_factor_contract interval coverage must be consecutive, interval {previous_interval_index} end ({previous_interval_end_utc_nanos}) must equal interval {current_interval_index} start ({current_interval_start_utc_nanos})"
            ),
            Self::InvalidDebugTraceSamplingStride { value } => write!(
                formatter,
                "debug trace sample_every_nth_transition must be greater than 0, found {value}"
            ),
            Self::InvalidDebugTraceChunkSize { value } => write!(
                formatter,
                "debug trace max_records_per_chunk must be greater than 0, found {value}"
            ),
            Self::InvalidKnownScheduledNewsTimestampUtcNanos { value } => write!(
                formatter,
                "known_scheduled_news_event_timestamps_utc_nanos must map to valid UTC timestamp nanos, found {value}"
            ),
            Self::KnownScheduledNewsTimestampsNotStrictlyIncreasing {
                previous_timestamp_utc_nanos,
                current_timestamp_utc_nanos,
            } => write!(
                formatter,
                "known_scheduled_news_event_timestamps_utc_nanos must be strictly increasing, found previous={previous_timestamp_utc_nanos} and current={current_timestamp_utc_nanos}"
            ),
            Self::InvalidNewsComponentActivityCouplingScale { component, value } => write!(
                formatter,
                "{component} news activity_coupling_scale must be finite and non-negative, found {value}"
            ),
            Self::InvalidNewsComponentReturnJumpScale { component, value } => write!(
                formatter,
                "{component} news return_jump_scale must be finite and non-negative, found {value}"
            ),
            Self::InvalidGeneratedNewsActivityCouplingMark { value } => write!(
                formatter,
                "generated exogenous news activity-coupling mark must be finite and non-negative, found {value}"
            ),
            Self::InvalidGeneratedNewsReturnJumpMark { value } => write!(
                formatter,
                "generated exogenous news return-jump mark must be finite, found {value}"
            ),
            Self::InvalidNewsRoutingStateViolation {
                market_state,
                open_window_return_jump_increment,
                closed_window_gap_increment,
            } => write!(
                formatter,
                "invalid news routing state for market_state={market_state}, open_window_return_jump_increment={open_window_return_jump_increment}, closed_window_gap_increment={closed_window_gap_increment}"
            ),
            Self::InvalidAccumulatedNewsRoutingTotal { field, value } => write!(
                formatter,
                "accumulated news routing total {field} must remain finite, found {value}"
            ),
            Self::InvalidTransitionStep { source } => {
                write!(
                    formatter,
                    "failed to apply canonical Hawkes transition step: {source}"
                )
            }
        }
    }
}

impl Error for SeededTickSimulationRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidTransitionStep { source } => Some(source),
            _ => None,
        }
    }
}

/// Run deterministic seeded tick-event simulation using the default splitmix64 randomness policy.
pub fn run_seeded_tick_event_simulation(
    run_config: SeededTickSimulationRunConfig,
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;
    run_seeded_tick_event_simulation_with_randomness_factory(run_config, &randomness_factory)
}

/// Run deterministic seeded tick-event simulation with an injected randomness factory.
///
/// This hook keeps randomness centralized in one abstraction while enabling fixed fixture
/// streams for deterministic testing and reference-sequence generation.
pub fn run_seeded_tick_event_simulation_with_randomness_factory<F>(
    run_config: SeededTickSimulationRunConfig,
    randomness_factory: &F,
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    run_config.validate()?;

    let state_snapshot_capture = run_config.options.state_snapshot_capture;
    let lambda_bar = run_config.model_inputs.kernel_parameters.lambda_bar;
    let event_tape_metadata = build_seeded_tick_event_tape_metadata(state_snapshot_capture);
    let mut event_path: Vec<SeededTickEventRecord> = Vec::new();
    let mut next_event_index: u64 = 0;
    let mut next_transition_index: u64 = 0;
    let mut next_state = run_config.model_inputs.initial_excitation_state;
    let mut news_routing_totals = SeededTickSimulationNewsRoutingTotals::default();
    let known_scheduled_news_event_timestamps_utc_nanos = &run_config
        .model_inputs
        .known_scheduled_news_event_timestamps_utc_nanos;
    let mut next_known_scheduled_news_timestamp_position = 0usize;
    let mut debug_trace_collector = run_config.options.debug_trace_enabled.then(|| {
        SeededTickEventDebugTraceCollector::new(run_config.options.debug_trace_options, lambda_bar)
    });

    for interval_index in 0..run_config.horizon_interval_count {
        let interval_start_utc_nanos = run_config
            .model_inputs
            .hourly_factor_contract
            .interval_start_utc_nanos[interval_index];
        let interval_end_utc_nanos = run_config
            .model_inputs
            .hourly_factor_contract
            .interval_end_utc_nanos[interval_index];

        if interval_end_utc_nanos <= interval_start_utc_nanos {
            return Err(
                SeededTickSimulationRunError::InvalidHourlyFactorIntervalBounds {
                    interval_index,
                    interval_start_utc_nanos,
                    interval_end_utc_nanos,
                },
            );
        }

        let interval_duration_nanos = interval_end_utc_nanos - interval_start_utc_nanos;
        let interval_duration_in_hours = interval_duration_nanos as f64 / ONE_HOUR_NANOS as f64;
        let interval_seasonality_factor =
            run_config.model_inputs.hourly_factor_contract.s_h[interval_index];
        let interval_projection = next_state.project_intensities(
            run_config.model_inputs.kernel_parameters.lambda_bar,
            interval_seasonality_factor,
        );
        let interval_news_intensity =
            run_config.model_inputs.hourly_factor_contract.nu_h[interval_index].max(0.0);
        let interval_market_state =
            derive_interval_market_state_for_news_routing(interval_seasonality_factor);
        let (
            interval_known_news_start_position,
            interval_known_news_end_position,
            next_interval_known_news_timestamp_position,
        ) = derive_known_news_interval_slice_bounds(
            known_scheduled_news_event_timestamps_utc_nanos,
            next_known_scheduled_news_timestamp_position,
            interval_start_utc_nanos,
            interval_end_utc_nanos,
        );
        next_known_scheduled_news_timestamp_position = next_interval_known_news_timestamp_position;

        let interval_updates = schedule_interval_stochastic_updates(
            run_config.seed,
            interval_index,
            interval_start_utc_nanos,
            interval_duration_nanos,
            interval_duration_in_hours,
            interval_projection,
            interval_news_intensity,
            &known_scheduled_news_event_timestamps_utc_nanos
                [interval_known_news_start_position..interval_known_news_end_position],
            run_config.model_inputs.known_news_mark_scales,
            run_config.model_inputs.surprise_news_mark_scales,
            interval_market_state,
            randomness_factory,
        )?;

        let mut previous_timestamp_utc_nanos = interval_start_utc_nanos;
        for interval_update in interval_updates {
            // Intra-interval updates are processed in event time and each update composes
            // one canonical transition step onto the carried Hawkes state.
            let elapsed_hours_since_previous_update = derive_elapsed_hours(
                previous_timestamp_utc_nanos,
                interval_update.timestamp_utc_nanos(),
            );
            let transition_step =
                interval_update.transition_step(elapsed_hours_since_previous_update);
            let event_index_for_update = match interval_update {
                IntervalStochasticUpdate::EndogenousEvent { .. } => Some(next_event_index),
                IntervalStochasticUpdate::ExogenousNews { .. } => None,
            };
            let previous_state = next_state;

            next_state = evolve_hawkes_state(
                next_state,
                run_config.model_inputs.kernel_parameters,
                transition_step,
            )
            .map_err(map_transition_error_to_seeded_simulation_run_error)?;

            if let Some(news_routing_update) = interval_update.news_routing_update() {
                news_routing_totals =
                    accumulate_news_routing_totals(news_routing_totals, news_routing_update)?;
            }

            if let Some(trace_collector) = debug_trace_collector.as_mut() {
                trace_collector.capture_transition(SeededTickEventDebugTraceCaptureInput {
                    transition_index: next_transition_index,
                    interval_index,
                    previous_timestamp_utc_nanos,
                    timestamp_utc_nanos: interval_update.timestamp_utc_nanos(),
                    elapsed_hours_since_previous_update,
                    transition_kind: interval_update.transition_kind(),
                    generation_order: Some(interval_update.generation_order()),
                    event_index: event_index_for_update,
                    news_transition_payload: interval_update.trace_news_transition_payload(),
                    news_routing_totals,
                    seasonality_factor: interval_seasonality_factor,
                    interval_news_intensity,
                    state_before_transition: previous_state,
                    state_after_transition: next_state,
                });
            }

            next_transition_index = next_transition_index.saturating_add(1);

            match interval_update {
                IntervalStochasticUpdate::EndogenousEvent {
                    timestamp_utc_nanos,
                    side,
                    ..
                } => {
                    event_path.push(SeededTickEventRecord {
                        event_index: next_event_index,
                        timestamp_utc_nanos,
                        side,
                        state_snapshot: build_event_state_snapshot(
                            next_state,
                            lambda_bar,
                            state_snapshot_capture,
                        ),
                    });
                    next_event_index = next_event_index.saturating_add(1);
                    previous_timestamp_utc_nanos = timestamp_utc_nanos;
                }
                IntervalStochasticUpdate::ExogenousNews {
                    timestamp_utc_nanos,
                    ..
                } => {
                    previous_timestamp_utc_nanos = timestamp_utc_nanos;
                }
            }
        }

        let elapsed_hours_to_interval_end =
            derive_elapsed_hours(previous_timestamp_utc_nanos, interval_end_utc_nanos);
        let previous_state = next_state;

        next_state = evolve_hawkes_state(
            next_state,
            run_config.model_inputs.kernel_parameters,
            HawkesTransitionStep::new(elapsed_hours_to_interval_end, None, None),
        )
        .map_err(map_transition_error_to_seeded_simulation_run_error)?;

        if let Some(trace_collector) = debug_trace_collector.as_mut() {
            trace_collector.capture_transition(SeededTickEventDebugTraceCaptureInput {
                transition_index: next_transition_index,
                interval_index,
                previous_timestamp_utc_nanos,
                timestamp_utc_nanos: interval_end_utc_nanos,
                elapsed_hours_since_previous_update: elapsed_hours_to_interval_end,
                transition_kind: SeededTickEventDebugTraceTransitionKind::IntervalBoundaryDrift,
                generation_order: None,
                event_index: None,
                news_transition_payload: None,
                news_routing_totals,
                seasonality_factor: interval_seasonality_factor,
                interval_news_intensity,
                state_before_transition: previous_state,
                state_after_transition: next_state,
            });
        }

        next_transition_index = next_transition_index.saturating_add(1);
    }

    let horizon_end_utc_nanos = run_config
        .model_inputs
        .hourly_factor_contract
        .interval_end_utc_nanos[run_config.horizon_interval_count - 1];

    Ok(SeededTickSimulationRunResult {
        schema_version: SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION,
        determinism_policy: SEEDED_TICK_EVENT_DETERMINISM_POLICY,
        event_ordering_policy: SEEDED_TICK_EVENT_ORDERING_POLICY,
        randomness_stream_policy: SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY,
        randomness_substream_derivation_policy:
            SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
        seed: run_config.seed,
        start_time_utc: run_config.start_time_utc,
        end_time_utc: DateTime::<Utc>::from_timestamp_nanos(horizon_end_utc_nanos),
        simulated_interval_count: run_config.horizon_interval_count,
        event_tape_metadata,
        event_path,
        final_excitation_state: next_state,
        news_routing_totals,
        debug_trace_enabled: run_config.options.debug_trace_enabled,
        debug_trace_output: debug_trace_collector.map(SeededTickEventDebugTraceCollector::finish),
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum IntervalStochasticUpdate {
    EndogenousEvent {
        timestamp_utc_nanos: i64,
        side: SeededTickEventSide,
        generation_order: u64,
    },
    ExogenousNews {
        timestamp_utc_nanos: i64,
        news_arrival_component: NewsArrivalComponent,
        activity_coupling_mark: f64,
        return_jump_mark: f64,
        market_state: MarketState,
        return_jump_routing_target: NewsReturnJumpRoutingTarget,
        open_window_return_jump_increment: f64,
        closed_window_gap_increment: f64,
        generation_order: u64,
    },
}

impl IntervalStochasticUpdate {
    const fn timestamp_utc_nanos(&self) -> i64 {
        match *self {
            Self::EndogenousEvent {
                timestamp_utc_nanos,
                ..
            } => timestamp_utc_nanos,
            Self::ExogenousNews {
                timestamp_utc_nanos,
                ..
            } => timestamp_utc_nanos,
        }
    }

    const fn ordering_key(&self) -> (i64, u8, u8, u8, u64) {
        match *self {
            Self::EndogenousEvent {
                timestamp_utc_nanos,
                side,
                generation_order,
            } => (
                timestamp_utc_nanos,
                0,
                match side {
                    SeededTickEventSide::Buy => 0,
                    SeededTickEventSide::Sell => 1,
                },
                0,
                generation_order,
            ),
            Self::ExogenousNews {
                timestamp_utc_nanos,
                news_arrival_component,
                generation_order,
                ..
            } => (
                timestamp_utc_nanos,
                1,
                0,
                match news_arrival_component {
                    NewsArrivalComponent::Known => 0,
                    NewsArrivalComponent::Surprise => 1,
                },
                generation_order,
            ),
        }
    }

    const fn transition_step(
        &self,
        elapsed_hours_since_previous_update: f64,
    ) -> HawkesTransitionStep {
        match *self {
            Self::EndogenousEvent { side, .. } => HawkesTransitionStep::new(
                elapsed_hours_since_previous_update,
                Some(map_seeded_tick_side_to_hawkes_side(side)),
                None,
            ),
            Self::ExogenousNews {
                activity_coupling_mark,
                ..
            } => HawkesTransitionStep::new(
                elapsed_hours_since_previous_update,
                None,
                Some(activity_coupling_mark),
            ),
        }
    }

    const fn generation_order(&self) -> u64 {
        match *self {
            Self::EndogenousEvent {
                generation_order, ..
            } => generation_order,
            Self::ExogenousNews {
                generation_order, ..
            } => generation_order,
        }
    }

    const fn transition_kind(&self) -> SeededTickEventDebugTraceTransitionKind {
        match *self {
            Self::EndogenousEvent { side, .. } => match side {
                SeededTickEventSide::Buy => {
                    SeededTickEventDebugTraceTransitionKind::EndogenousBuyEvent
                }
                SeededTickEventSide::Sell => {
                    SeededTickEventDebugTraceTransitionKind::EndogenousSellEvent
                }
            },
            Self::ExogenousNews { .. } => SeededTickEventDebugTraceTransitionKind::ExogenousNews,
        }
    }

    const fn news_routing_update(&self) -> Option<SeededTickSimulationNewsRoutingUpdate> {
        match *self {
            Self::EndogenousEvent { .. } => None,
            Self::ExogenousNews {
                activity_coupling_mark,
                open_window_return_jump_increment,
                closed_window_gap_increment,
                ..
            } => Some(SeededTickSimulationNewsRoutingUpdate {
                activity_coupling_j_t_increment: activity_coupling_mark,
                open_window_return_jump_m_t_increment: open_window_return_jump_increment,
                closed_window_gap_g_t_increment: closed_window_gap_increment,
            }),
        }
    }

    const fn trace_news_transition_payload(
        &self,
    ) -> Option<SeededTickSimulationTraceNewsTransitionPayload> {
        match *self {
            Self::EndogenousEvent { .. } => None,
            Self::ExogenousNews {
                news_arrival_component,
                activity_coupling_mark,
                return_jump_mark,
                market_state,
                return_jump_routing_target,
                open_window_return_jump_increment,
                closed_window_gap_increment,
                ..
            } => Some(SeededTickSimulationTraceNewsTransitionPayload {
                news_arrival_component,
                activity_coupling_mark,
                return_jump_mark,
                market_state,
                return_jump_routing_target,
                open_window_return_jump_increment,
                closed_window_gap_increment,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SeededTickSimulationNewsRoutingUpdate {
    activity_coupling_j_t_increment: f64,
    open_window_return_jump_m_t_increment: f64,
    closed_window_gap_g_t_increment: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SeededTickSimulationTraceNewsTransitionPayload {
    news_arrival_component: NewsArrivalComponent,
    activity_coupling_mark: f64,
    return_jump_mark: f64,
    market_state: MarketState,
    return_jump_routing_target: NewsReturnJumpRoutingTarget,
    open_window_return_jump_increment: f64,
    closed_window_gap_increment: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SeededTickEventDebugTraceCaptureInput {
    transition_index: u64,
    interval_index: usize,
    previous_timestamp_utc_nanos: i64,
    timestamp_utc_nanos: i64,
    elapsed_hours_since_previous_update: f64,
    transition_kind: SeededTickEventDebugTraceTransitionKind,
    generation_order: Option<u64>,
    event_index: Option<u64>,
    news_transition_payload: Option<SeededTickSimulationTraceNewsTransitionPayload>,
    news_routing_totals: SeededTickSimulationNewsRoutingTotals,
    seasonality_factor: f64,
    interval_news_intensity: f64,
    state_before_transition: HawkesExcitationState,
    state_after_transition: HawkesExcitationState,
}

#[derive(Debug, Clone, PartialEq)]
struct SeededTickEventDebugTraceCollector {
    lambda_bar: f64,
    options: SeededTickSimulationDebugTraceOptions,
    trace_chunks: Vec<SeededTickEventDebugTraceChunk>,
    pending_chunk_records: Vec<SeededTickEventDebugTraceRecord>,
}

impl SeededTickEventDebugTraceCollector {
    fn new(options: SeededTickSimulationDebugTraceOptions, lambda_bar: f64) -> Self {
        Self {
            lambda_bar,
            options,
            trace_chunks: Vec::new(),
            pending_chunk_records: Vec::with_capacity(options.max_records_per_chunk),
        }
    }

    fn capture_transition(&mut self, input: SeededTickEventDebugTraceCaptureInput) {
        if !self.should_capture_transition(input.transition_index, input.transition_kind) {
            return;
        }

        self.pending_chunk_records
            .push(SeededTickEventDebugTraceRecord {
                transition_index: input.transition_index,
                interval_index: input.interval_index,
                previous_timestamp_utc_nanos: input.previous_timestamp_utc_nanos,
                timestamp_utc_nanos: input.timestamp_utc_nanos,
                elapsed_hours_since_previous_update: input.elapsed_hours_since_previous_update,
                transition_kind: input.transition_kind,
                generation_order: input.generation_order,
                event_index: input.event_index,
                exogenous_news_arrival_component: input
                    .news_transition_payload
                    .map(|payload| payload.news_arrival_component),
                exogenous_news_activity_coupling_mark: input
                    .news_transition_payload
                    .map(|payload| payload.activity_coupling_mark),
                exogenous_news_return_jump_mark: input
                    .news_transition_payload
                    .map(|payload| payload.return_jump_mark),
                exogenous_news_market_state: input.news_transition_payload.map(|payload| {
                    SeededTickSimulationNewsEventMarketState::from_market_state(
                        payload.market_state,
                    )
                }),
                exogenous_news_return_jump_routing_target: input
                    .news_transition_payload
                    .map(|payload| payload.return_jump_routing_target),
                exogenous_news_open_window_return_jump_increment: input
                    .news_transition_payload
                    .map(|payload| payload.open_window_return_jump_increment),
                exogenous_news_closed_window_gap_increment: input
                    .news_transition_payload
                    .map(|payload| payload.closed_window_gap_increment),
                cumulative_news_activity_coupling_j_t_increment: input
                    .news_routing_totals
                    .cumulative_activity_coupling_j_t_increment,
                cumulative_open_window_return_jump_m_t_increment: input
                    .news_routing_totals
                    .cumulative_open_window_return_jump_m_t_increment,
                cumulative_closed_window_gap_g_t_increment: input
                    .news_routing_totals
                    .cumulative_closed_window_gap_g_t_increment,
                seasonality_factor: input.seasonality_factor,
                interval_news_intensity: input.interval_news_intensity,
                state_before_transition: SeededTickEventStateSnapshot::from_state(
                    input.state_before_transition,
                    self.lambda_bar,
                ),
                state_after_transition: SeededTickEventStateSnapshot::from_state(
                    input.state_after_transition,
                    self.lambda_bar,
                ),
            });

        if self.pending_chunk_records.len() >= self.options.max_records_per_chunk {
            self.flush_pending_chunk();
        }
    }

    fn finish(mut self) -> SeededTickEventDebugTraceOutput {
        self.flush_pending_chunk();

        let captured_transition_count = self
            .trace_chunks
            .iter()
            .map(|trace_chunk| trace_chunk.records.len())
            .sum();

        SeededTickEventDebugTraceOutput {
            schema_version: SEEDED_TICK_EVENT_DEBUG_TRACE_SCHEMA_VERSION,
            metadata: build_seeded_tick_event_debug_trace_metadata(self.options),
            captured_transition_count,
            trace_chunks: self.trace_chunks,
        }
    }

    fn should_capture_transition(
        &self,
        transition_index: u64,
        transition_kind: SeededTickEventDebugTraceTransitionKind,
    ) -> bool {
        if matches!(
            transition_kind,
            SeededTickEventDebugTraceTransitionKind::IntervalBoundaryDrift
        ) {
            return true;
        }

        transition_index % self.options.sample_every_nth_transition == 0
    }

    fn flush_pending_chunk(&mut self) {
        if self.pending_chunk_records.is_empty() {
            return;
        }

        let start_transition_index = self
            .pending_chunk_records
            .first()
            .expect("non-empty trace chunk should contain first record")
            .transition_index;
        let end_transition_index = self
            .pending_chunk_records
            .last()
            .expect("non-empty trace chunk should contain last record")
            .transition_index;
        let records = std::mem::take(&mut self.pending_chunk_records);
        let chunk_index = self.trace_chunks.len();

        self.trace_chunks.push(SeededTickEventDebugTraceChunk {
            schema_version: SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION,
            chunk_index,
            start_transition_index,
            end_transition_index,
            records,
        });
    }
}

fn schedule_interval_stochastic_updates<F>(
    seed: u64,
    interval_index: usize,
    interval_start_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_duration_in_hours: f64,
    interval_projection: HawkesIntensityProjection,
    interval_news_intensity: f64,
    interval_known_news_event_timestamps_utc_nanos: &[i64],
    known_news_mark_scales: SeededTickSimulationNewsMarkScales,
    surprise_news_mark_scales: SeededTickSimulationNewsMarkScales,
    interval_market_state: MarketState,
    randomness_factory: &F,
) -> Result<Vec<IntervalStochasticUpdate>, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let mut buy_event_count_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount,
        interval_index as u64,
    );
    let mut sell_event_count_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousSellEventCount,
        interval_index as u64,
    );
    let mut buy_event_timestamp_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp,
        interval_index as u64,
    );
    let mut sell_event_timestamp_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp,
        interval_index as u64,
    );
    let mut news_count_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsCount,
        interval_index as u64,
    );
    let mut news_timestamp_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp,
        interval_index as u64,
    );
    let mut news_severity_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity,
        interval_index as u64,
    );
    let mut news_return_jump_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsReturnJump,
        interval_index as u64,
    );

    let interval_buy_intensity = interval_projection.seasonality_scaled.lambda_plus.max(0.0);
    let interval_sell_intensity = interval_projection.seasonality_scaled.lambda_minus.max(0.0);

    let interval_buy_event_count = sample_count_from_expectation(
        interval_buy_intensity * interval_duration_in_hours,
        &mut buy_event_count_stream,
    );
    let interval_sell_event_count = sample_count_from_expectation(
        interval_sell_intensity * interval_duration_in_hours,
        &mut sell_event_count_stream,
    );
    let interval_total_news_expected_count = interval_news_intensity * interval_duration_in_hours;
    let interval_known_news_event_count =
        u64::try_from(interval_known_news_event_timestamps_utc_nanos.len()).unwrap_or(u64::MAX);
    let interval_surprise_news_expected_count =
        (interval_total_news_expected_count - interval_known_news_event_count as f64).max(0.0);
    let interval_surprise_news_count = sample_count_from_expectation(
        interval_surprise_news_expected_count,
        &mut news_count_stream,
    );

    let mut interval_updates: Vec<IntervalStochasticUpdate> = Vec::with_capacity(
        (interval_buy_event_count
            + interval_sell_event_count
            + interval_known_news_event_count
            + interval_surprise_news_count) as usize,
    );
    let mut next_generation_order = 0_u64;

    for _buy_event_position in 0..interval_buy_event_count {
        let timestamp_utc_nanos = sample_interval_timestamp_utc_nanos(
            interval_start_utc_nanos,
            interval_duration_nanos,
            &mut buy_event_timestamp_stream,
        );

        interval_updates.push(IntervalStochasticUpdate::EndogenousEvent {
            timestamp_utc_nanos,
            side: SeededTickEventSide::Buy,
            generation_order: next_generation_order,
        });
        next_generation_order = next_generation_order.saturating_add(1);
    }

    for _sell_event_position in 0..interval_sell_event_count {
        let timestamp_utc_nanos = sample_interval_timestamp_utc_nanos(
            interval_start_utc_nanos,
            interval_duration_nanos,
            &mut sell_event_timestamp_stream,
        );

        interval_updates.push(IntervalStochasticUpdate::EndogenousEvent {
            timestamp_utc_nanos,
            side: SeededTickEventSide::Sell,
            generation_order: next_generation_order,
        });
        next_generation_order = next_generation_order.saturating_add(1);
    }

    for &known_news_timestamp_utc_nanos in interval_known_news_event_timestamps_utc_nanos {
        let activity_coupling_mark = sample_exogenous_news_activity_coupling_mark(
            &mut news_severity_stream,
            known_news_mark_scales.activity_coupling_scale,
        )?;
        let return_jump_mark = sample_exogenous_news_return_jump_mark(
            activity_coupling_mark,
            &mut news_return_jump_stream,
            known_news_mark_scales.return_jump_scale,
        )?;
        let (
            return_jump_routing_target,
            open_window_return_jump_increment,
            closed_window_gap_increment,
        ) = resolve_news_return_jump_routing(interval_market_state, return_jump_mark)?;

        interval_updates.push(IntervalStochasticUpdate::ExogenousNews {
            timestamp_utc_nanos: known_news_timestamp_utc_nanos,
            news_arrival_component: NewsArrivalComponent::Known,
            activity_coupling_mark,
            return_jump_mark,
            market_state: interval_market_state,
            return_jump_routing_target,
            open_window_return_jump_increment,
            closed_window_gap_increment,
            generation_order: next_generation_order,
        });
        next_generation_order = next_generation_order.saturating_add(1);
    }

    for _surprise_news_position in 0..interval_surprise_news_count {
        let timestamp_utc_nanos = sample_interval_timestamp_utc_nanos(
            interval_start_utc_nanos,
            interval_duration_nanos,
            &mut news_timestamp_stream,
        );
        let activity_coupling_mark = sample_exogenous_news_activity_coupling_mark(
            &mut news_severity_stream,
            surprise_news_mark_scales.activity_coupling_scale,
        )?;
        let return_jump_mark = sample_exogenous_news_return_jump_mark(
            activity_coupling_mark,
            &mut news_return_jump_stream,
            surprise_news_mark_scales.return_jump_scale,
        )?;
        let (
            return_jump_routing_target,
            open_window_return_jump_increment,
            closed_window_gap_increment,
        ) = resolve_news_return_jump_routing(interval_market_state, return_jump_mark)?;

        interval_updates.push(IntervalStochasticUpdate::ExogenousNews {
            timestamp_utc_nanos,
            news_arrival_component: NewsArrivalComponent::Surprise,
            activity_coupling_mark,
            return_jump_mark,
            market_state: interval_market_state,
            return_jump_routing_target,
            open_window_return_jump_increment,
            closed_window_gap_increment,
            generation_order: next_generation_order,
        });
        next_generation_order = next_generation_order.saturating_add(1);
    }

    interval_updates.sort_by_key(IntervalStochasticUpdate::ordering_key);
    Ok(interval_updates)
}

fn map_transition_error_to_seeded_simulation_run_error(
    source: HawkesTransitionError,
) -> SeededTickSimulationRunError {
    match source {
        HawkesTransitionError::InvalidNewsSeverityMark { source } => {
            SeededTickSimulationRunError::InvalidGeneratedNewsActivityCouplingMark {
                value: source.value,
            }
        }
        _ => SeededTickSimulationRunError::InvalidTransitionStep { source },
    }
}

const fn build_event_state_snapshot(
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

const fn build_seeded_tick_event_tape_metadata(
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
) -> SeededTickEventTapeMetadata {
    SeededTickEventTapeMetadata {
        schema_version: SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
        event_record_schema_version: SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION,
        state_snapshot_schema_version: SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION,
        event_ordering_policy: SEEDED_TICK_EVENT_ORDERING_POLICY,
        timestamp_representation_policy: SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY,
        side_encoding_policy: SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY,
        state_snapshot_capture,
        state_snapshot_capture_point_policy: state_snapshot_capture.capture_point_policy(),
    }
}

const fn build_seeded_tick_event_debug_trace_metadata(
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

fn derive_elapsed_hours(previous_timestamp_utc_nanos: i64, timestamp_utc_nanos: i64) -> f64 {
    (timestamp_utc_nanos - previous_timestamp_utc_nanos) as f64 / ONE_HOUR_NANOS as f64
}

const fn derive_interval_market_state_for_news_routing(
    interval_seasonality_factor: f64,
) -> MarketState {
    if interval_seasonality_factor > 0.0 {
        MarketState::Open
    } else {
        MarketState::Closed
    }
}

fn derive_known_news_interval_slice_bounds(
    known_scheduled_news_event_timestamps_utc_nanos: &[i64],
    known_scheduled_news_start_position: usize,
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> (usize, usize, usize) {
    let mut next_known_scheduled_news_position = known_scheduled_news_start_position;
    while next_known_scheduled_news_position < known_scheduled_news_event_timestamps_utc_nanos.len()
        && known_scheduled_news_event_timestamps_utc_nanos[next_known_scheduled_news_position]
            < interval_start_utc_nanos
    {
        next_known_scheduled_news_position = next_known_scheduled_news_position.saturating_add(1);
    }

    let interval_known_news_start_position = next_known_scheduled_news_position;
    while next_known_scheduled_news_position < known_scheduled_news_event_timestamps_utc_nanos.len()
        && known_scheduled_news_event_timestamps_utc_nanos[next_known_scheduled_news_position]
            < interval_end_utc_nanos
    {
        next_known_scheduled_news_position = next_known_scheduled_news_position.saturating_add(1);
    }

    (
        interval_known_news_start_position,
        next_known_scheduled_news_position,
        next_known_scheduled_news_position,
    )
}

fn validate_known_scheduled_news_timestamps(
    known_scheduled_news_event_timestamps_utc_nanos: &[i64],
) -> Result<(), SeededTickSimulationRunError> {
    let mut previous_timestamp_utc_nanos: Option<i64> = None;
    for &timestamp_utc_nanos in known_scheduled_news_event_timestamps_utc_nanos {
        let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
        let nanoseconds = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;
        if DateTime::<Utc>::from_timestamp(seconds, nanoseconds).is_none() {
            return Err(
                SeededTickSimulationRunError::InvalidKnownScheduledNewsTimestampUtcNanos {
                    value: timestamp_utc_nanos,
                },
            );
        }

        if let Some(previous_timestamp_utc_nanos_value) = previous_timestamp_utc_nanos
            && timestamp_utc_nanos <= previous_timestamp_utc_nanos_value
        {
            return Err(
                SeededTickSimulationRunError::KnownScheduledNewsTimestampsNotStrictlyIncreasing {
                    previous_timestamp_utc_nanos: previous_timestamp_utc_nanos_value,
                    current_timestamp_utc_nanos: timestamp_utc_nanos,
                },
            );
        }
        previous_timestamp_utc_nanos = Some(timestamp_utc_nanos);
    }

    Ok(())
}

fn sample_interval_timestamp_utc_nanos<R>(
    interval_start_utc_nanos: i64,
    interval_duration_nanos: i64,
    randomness_stream: &mut R,
) -> i64
where
    R: SimulationRandomnessStream,
{
    if interval_duration_nanos <= 1 {
        return interval_start_utc_nanos;
    }

    let timestamp_offset_nanos = (randomness_stream.next_unit_interval_f64()
        * interval_duration_nanos as f64)
        .floor() as i64;

    interval_start_utc_nanos + timestamp_offset_nanos.min(interval_duration_nanos - 1)
}

fn sample_count_from_expectation<R>(expected_count: f64, randomness_stream: &mut R) -> u64
where
    R: SimulationRandomnessStream,
{
    if !expected_count.is_finite() || expected_count <= 0.0 {
        return 0;
    }

    if expected_count >= u64::MAX as f64 {
        return MAX_STOCHASTIC_UPDATES_PER_INTERVAL;
    }

    let floored_count = expected_count.floor() as u64;
    let fractional_count = expected_count - floored_count as f64;

    let sampled_count = if randomness_stream.next_unit_interval_f64() < fractional_count {
        floored_count.saturating_add(1)
    } else {
        floored_count
    };

    sampled_count.min(MAX_STOCHASTIC_UPDATES_PER_INTERVAL)
}

fn sample_exogenous_news_activity_coupling_mark<R>(
    randomness_stream: &mut R,
    activity_coupling_scale: f64,
) -> Result<f64, SeededTickSimulationRunError>
where
    R: SimulationRandomnessStream,
{
    let sampled_unscaled_mark = sample_exogenous_news_severity_mark(randomness_stream);
    let sampled_mark = activity_coupling_scale * sampled_unscaled_mark;
    if !sampled_mark.is_finite() || sampled_mark < 0.0 {
        return Err(
            SeededTickSimulationRunError::InvalidGeneratedNewsActivityCouplingMark {
                value: sampled_mark,
            },
        );
    }

    Ok(sampled_mark)
}

fn sample_exogenous_news_return_jump_mark<R>(
    activity_coupling_mark: f64,
    randomness_stream: &mut R,
    return_jump_scale: f64,
) -> Result<f64, SeededTickSimulationRunError>
where
    R: SimulationRandomnessStream,
{
    let sign_selector = randomness_stream.next_unit_interval_f64();
    let sign = if sign_selector < 0.5 { -1.0 } else { 1.0 };
    let magnitude = sample_exogenous_news_severity_mark(randomness_stream);
    let sampled_mark = return_jump_scale * activity_coupling_mark.sqrt() * sign * magnitude;
    if !sampled_mark.is_finite() {
        return Err(
            SeededTickSimulationRunError::InvalidGeneratedNewsReturnJumpMark {
                value: sampled_mark,
            },
        );
    }

    Ok(sampled_mark)
}

fn sample_exogenous_news_severity_mark<R>(randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream,
{
    let unit_draw = randomness_stream.next_unit_interval_f64();
    -(1.0 - unit_draw).ln()
}

fn resolve_news_return_jump_routing(
    market_state: MarketState,
    return_jump_mark: f64,
) -> Result<(NewsReturnJumpRoutingTarget, f64, f64), SeededTickSimulationRunError> {
    let (
        return_jump_routing_target,
        open_window_return_jump_increment,
        closed_window_gap_increment,
    ) = match market_state {
        MarketState::Open => (
            NewsReturnJumpRoutingTarget::OpenWindowDirectJumpM,
            return_jump_mark,
            0.0,
        ),
        MarketState::Closed => (
            NewsReturnJumpRoutingTarget::ClosedWindowGapG,
            0.0,
            return_jump_mark,
        ),
    };

    validate_news_return_jump_routing_state(
        market_state,
        open_window_return_jump_increment,
        closed_window_gap_increment,
    )?;

    Ok((
        return_jump_routing_target,
        open_window_return_jump_increment,
        closed_window_gap_increment,
    ))
}

fn validate_news_return_jump_routing_state(
    market_state: MarketState,
    open_window_return_jump_increment: f64,
    closed_window_gap_increment: f64,
) -> Result<(), SeededTickSimulationRunError> {
    let routing_state_is_valid = match market_state {
        MarketState::Open => closed_window_gap_increment == 0.0,
        MarketState::Closed => open_window_return_jump_increment == 0.0,
    };
    if !routing_state_is_valid
        || !open_window_return_jump_increment.is_finite()
        || !closed_window_gap_increment.is_finite()
    {
        return Err(
            SeededTickSimulationRunError::InvalidNewsRoutingStateViolation {
                market_state,
                open_window_return_jump_increment,
                closed_window_gap_increment,
            },
        );
    }

    Ok(())
}

fn accumulate_news_routing_totals(
    current_totals: SeededTickSimulationNewsRoutingTotals,
    routing_update: SeededTickSimulationNewsRoutingUpdate,
) -> Result<SeededTickSimulationNewsRoutingTotals, SeededTickSimulationRunError> {
    let next_totals = SeededTickSimulationNewsRoutingTotals {
        cumulative_activity_coupling_j_t_increment: current_totals
            .cumulative_activity_coupling_j_t_increment
            + routing_update.activity_coupling_j_t_increment,
        cumulative_open_window_return_jump_m_t_increment: current_totals
            .cumulative_open_window_return_jump_m_t_increment
            + routing_update.open_window_return_jump_m_t_increment,
        cumulative_closed_window_gap_g_t_increment: current_totals
            .cumulative_closed_window_gap_g_t_increment
            + routing_update.closed_window_gap_g_t_increment,
    };
    if !next_totals
        .cumulative_activity_coupling_j_t_increment
        .is_finite()
    {
        return Err(
            SeededTickSimulationRunError::InvalidAccumulatedNewsRoutingTotal {
                field: "cumulative_activity_coupling_j_t_increment",
                value: next_totals.cumulative_activity_coupling_j_t_increment,
            },
        );
    }
    if !next_totals
        .cumulative_open_window_return_jump_m_t_increment
        .is_finite()
    {
        return Err(
            SeededTickSimulationRunError::InvalidAccumulatedNewsRoutingTotal {
                field: "cumulative_open_window_return_jump_m_t_increment",
                value: next_totals.cumulative_open_window_return_jump_m_t_increment,
            },
        );
    }
    if !next_totals
        .cumulative_closed_window_gap_g_t_increment
        .is_finite()
    {
        return Err(
            SeededTickSimulationRunError::InvalidAccumulatedNewsRoutingTotal {
                field: "cumulative_closed_window_gap_g_t_increment",
                value: next_totals.cumulative_closed_window_gap_g_t_increment,
            },
        );
    }

    Ok(next_totals)
}

const fn map_seeded_tick_side_to_hawkes_side(side: SeededTickEventSide) -> HawkesEventSide {
    match side {
        SeededTickEventSide::Buy => HawkesEventSide::Buy,
        SeededTickEventSide::Sell => HawkesEventSide::Sell,
    }
}

fn validate_hourly_factor_field_length(
    field_name: &'static str,
    expected_interval_count: usize,
    observed_interval_count: usize,
) -> Result<(), SeededTickSimulationRunError> {
    if observed_interval_count != expected_interval_count {
        return Err(
            SeededTickSimulationRunError::HourlyFactorFieldLengthMismatch {
                field_name,
                expected_interval_count,
                observed_interval_count,
            },
        );
    }

    Ok(())
}

fn validate_hourly_factor_interval_coverage(
    interval_start_utc_nanos: &[i64],
    interval_end_utc_nanos: &[i64],
    simulated_interval_count: usize,
) -> Result<(), SeededTickSimulationRunError> {
    for interval_index in 0..simulated_interval_count {
        let current_interval_start_utc_nanos = interval_start_utc_nanos[interval_index];
        let current_interval_end_utc_nanos = interval_end_utc_nanos[interval_index];

        if current_interval_end_utc_nanos <= current_interval_start_utc_nanos {
            return Err(
                SeededTickSimulationRunError::InvalidHourlyFactorIntervalBounds {
                    interval_index,
                    interval_start_utc_nanos: current_interval_start_utc_nanos,
                    interval_end_utc_nanos: current_interval_end_utc_nanos,
                },
            );
        }

        if interval_index == 0 {
            continue;
        }

        let previous_interval_index = interval_index - 1;
        let previous_interval_end_utc_nanos = interval_end_utc_nanos[previous_interval_index];
        if current_interval_start_utc_nanos != previous_interval_end_utc_nanos {
            return Err(
                SeededTickSimulationRunError::NonConsecutiveHourlyFactorCoverage {
                    previous_interval_index,
                    previous_interval_end_utc_nanos,
                    current_interval_index: interval_index,
                    current_interval_start_utc_nanos,
                },
            );
        }
    }

    Ok(())
}

/// Resolve interval-aligned hourly factors for the simulation path.
pub use crate::hourly_factor_provider::provide_hourly_factor_contract as load_hourly_factor_contract;

/// Project state to interval intensities for the fine-scale simulation path.
pub fn project_hawkes_intensities_simulation_interval(
    state: HawkesExcitationState,
    parameters: HawkesKernelParameters,
    hourly_factor_contract: &HourlyFactorContractOutput,
    interval_index: usize,
) -> Result<HawkesIntensityProjection, HawkesSeasonalityIntervalError> {
    state.project_intensities_for_hourly_interval(
        parameters.lambda_bar,
        hourly_factor_contract,
        interval_index,
    )
}

/// Sample one trading-time return increment for simulation through the shared NIG driver API.
pub fn sample_nig_trading_time_increment<R>(
    driver_parameters: NigBuySellDriverParameters,
    return_decomposition: ReturnDecomposition,
    randomness_stream: &mut R,
) -> Result<NigTradingTimeIncrementContributions, NigTradingTimeIncrementSamplingError>
where
    R: SimulationRandomnessStream,
{
    crate::return_decomposition::sample_nig_trading_time_increment_with_edge_mode(
        driver_parameters,
        return_decomposition,
        ReturnDecompositionEdgeMode::Disabled,
        randomness_stream,
    )
}

/// Sample one simulation trading-time increment with optional decomposition edge-mode gating.
pub use crate::return_decomposition::sample_nig_trading_time_increment_with_edge_mode;

/// Apply one simulation-path Hawkes state transition using canonical call order.
pub use crate::hawkes::evolve_hawkes_state;

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use serde_json::Value;

    use crate::calendar::MarketState;
    use crate::hawkes::{
        HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters, HawkesTransitionStep,
    };
    use crate::hourly_factors::{
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD, HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        HOURLY_FACTOR_FALL_BACK_POLICY, HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
        HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY, HOURLY_FACTOR_SPRING_FORWARD_POLICY,
        HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY, HourlyFactorContractDiagnostics,
        HourlyFactorContractOutput, HourlyFactorMaterializationMetadata,
    };
    use crate::news::{NewsArrivalComponent, NewsReturnJumpRoutingTarget};
    use crate::nig::{
        NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY, NigSharedPlusShiftConfiguration,
    };
    use crate::randomness::{
        SimulationRandomnessFactory, SimulationRandomnessStream,
        SimulationRandomnessSubstreamDomain, SplitMix64SimulationRandomnessStream,
    };
    use crate::return_decomposition::{
        ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionInput,
    };
    use crate::simulation::{
        SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION,
        SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNKING_POLICY,
        SEEDED_TICK_EVENT_DEBUG_TRACE_METADATA_SCHEMA_VERSION,
        SEEDED_TICK_EVENT_DEBUG_TRACE_RECORD_SCHEMA_VERSION,
        SEEDED_TICK_EVENT_DEBUG_TRACE_SAMPLING_POLICY,
        SEEDED_TICK_EVENT_DEBUG_TRACE_SCHEMA_VERSION, SEEDED_TICK_EVENT_DETERMINISM_POLICY,
        SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY, SEEDED_TICK_EVENT_ORDERING_POLICY,
        SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION, SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY,
        SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
        SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION, SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY,
        SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY,
        SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION,
        SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
        SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY, SeededTickEventDebugTraceTransitionKind,
        SeededTickEventSide, SeededTickEventStateSnapshotCapture,
        SeededTickSimulationDebugTraceOptions, SeededTickSimulationModelInputs,
        SeededTickSimulationNewsEventMarketState, SeededTickSimulationNewsMarkScales,
        SeededTickSimulationRunConfig, SeededTickSimulationRunError,
        SeededTickSimulationRunOptions, run_seeded_tick_event_simulation,
        run_seeded_tick_event_simulation_with_randomness_factory,
        sample_nig_trading_time_increment, sample_nig_trading_time_increment_with_edge_mode,
    };

    const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;

    #[test]
    fn trading_time_assembler_buy_only_edge_mode_keeps_sell_and_gap_components_zero() {
        let driver_parameters = valid_nig_driver_parameters();
        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.9,
            a_minus: 0.7,
            open_jump: 0.0,
            closed_gap: -0.2,
        })
        .expect("buy-only decomposition should be valid");
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(42_101);

        let contributions = sample_nig_trading_time_increment_with_edge_mode(
            driver_parameters,
            decomposition,
            ReturnDecompositionEdgeMode::BuyOnly,
            &mut randomness_stream,
        )
        .expect("buy-only assembly should sample successfully");

        assert_eq!(
            contributions.policy,
            NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY
        );
        assert_float_close(contributions.sell_side_increment, 0.0);
        assert_float_close(
            contributions.combined_increment,
            contributions.buy_side_increment,
        );
    }

    #[test]
    fn trading_time_assembler_sell_only_edge_mode_applies_minus_sign_to_combined_increment() {
        let driver_parameters = valid_nig_driver_parameters();
        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.6,
            a_minus: 0.9,
            open_jump: 0.0,
            closed_gap: -0.1,
        })
        .expect("sell-only decomposition should be valid");
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(42_102);

        let contributions = sample_nig_trading_time_increment_with_edge_mode(
            driver_parameters,
            decomposition,
            ReturnDecompositionEdgeMode::SellOnly,
            &mut randomness_stream,
        )
        .expect("sell-only assembly should sample successfully");

        assert_eq!(
            contributions.policy,
            NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY
        );
        assert_float_close(contributions.buy_side_increment, 0.0);
        assert_float_close(
            contributions.combined_increment,
            -contributions.sell_side_increment,
        );
    }

    #[test]
    fn trading_time_assembler_gap_only_edge_mode_zeroes_trading_components() {
        let driver_parameters = valid_nig_driver_parameters();
        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.9,
            a_minus: 0.8,
            open_jump: 0.0,
            closed_gap: -0.3,
        })
        .expect("gap-only decomposition should be valid");
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(42_104);

        let contributions = sample_nig_trading_time_increment_with_edge_mode(
            driver_parameters,
            decomposition,
            ReturnDecompositionEdgeMode::ClosedGapOnly,
            &mut randomness_stream,
        )
        .expect("gap-only assembly should sample successfully");

        assert_eq!(
            contributions.policy,
            NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY
        );
        assert_float_close(contributions.buy_side_increment, 0.0);
        assert_float_close(contributions.sell_side_increment, 0.0);
        assert_float_close(contributions.combined_increment, 0.0);
    }

    #[test]
    fn trading_time_assembler_is_seed_deterministic() {
        let driver_parameters = valid_nig_driver_parameters();
        let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.8,
            a_minus: 0.5,
            open_jump: 0.0,
            closed_gap: 0.0,
        })
        .expect("mixed decomposition should be valid");
        let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(42_103);
        let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(42_103);

        let first_contributions =
            sample_nig_trading_time_increment(driver_parameters, decomposition, &mut first_stream)
                .expect("first seeded assembly should sample successfully");
        let second_contributions =
            sample_nig_trading_time_increment(driver_parameters, decomposition, &mut second_stream)
                .expect("second seeded assembly should sample successfully");

        assert_eq!(first_contributions, second_contributions);
    }

    fn valid_nig_driver_parameters() -> crate::nig::NigBuySellDriverParameters {
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("test shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("test shared-plus-shift payload should derive drivers")
    }

    #[test]
    fn seeded_simulation_run_executes_with_valid_config() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            9_101,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
                debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect("valid run config should initialize");

        let first_result = run_seeded_tick_event_simulation(run_config.clone())
            .expect("valid run should produce deterministic output");
        let second_result = run_seeded_tick_event_simulation(run_config)
            .expect("same run config should produce deterministic output");

        assert_eq!(first_result, second_result);
        assert_eq!(
            first_result.schema_version,
            SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION
        );
        assert_eq!(
            first_result.determinism_policy,
            SEEDED_TICK_EVENT_DETERMINISM_POLICY
        );
        assert_eq!(
            first_result.event_ordering_policy,
            SEEDED_TICK_EVENT_ORDERING_POLICY
        );
        assert_eq!(
            first_result.randomness_stream_policy,
            SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY
        );
        assert_eq!(
            first_result.randomness_substream_derivation_policy,
            SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY
        );
        assert_eq!(first_result.seed, 9_101);
        assert_eq!(first_result.simulated_interval_count, 2);
        assert!(first_result.debug_trace_enabled);
        assert!(!first_result.event_path.is_empty());
        assert!(is_non_decreasing_timestamp_path(&first_result));
    }

    #[test]
    fn seeded_simulation_run_diverges_with_different_seeds() {
        let first_run_config = SeededTickSimulationRunConfig::try_new(
            9_101,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            None,
        )
        .expect("first run config should initialize");
        let second_run_config = SeededTickSimulationRunConfig::try_new(
            9_102,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            None,
        )
        .expect("second run config should initialize");

        let first_result = run_seeded_tick_event_simulation(first_run_config)
            .expect("first deterministic run should complete");
        let second_result = run_seeded_tick_event_simulation(second_run_config)
            .expect("second deterministic run should complete");

        assert_ne!(first_result.event_path, second_result.event_path);
    }

    #[test]
    fn seeded_simulation_run_is_independent_from_debug_trace_flag() {
        let run_config_without_debug = SeededTickSimulationRunConfig::try_new(
            8_001,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: false,
                debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect("run config without debug trace should initialize");
        let run_config_with_debug = SeededTickSimulationRunConfig::try_new(
            8_001,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
                debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect("run config with debug trace should initialize");

        let result_without_debug = run_seeded_tick_event_simulation(run_config_without_debug)
            .expect("run without debug trace should complete");
        let result_with_debug = run_seeded_tick_event_simulation(run_config_with_debug)
            .expect("run with debug trace should complete");

        assert_eq!(
            result_without_debug.event_path,
            result_with_debug.event_path
        );
        assert_eq!(
            result_without_debug.final_excitation_state,
            result_with_debug.final_excitation_state
        );
        assert!(!result_without_debug.debug_trace_enabled);
        assert!(result_with_debug.debug_trace_enabled);
        assert!(result_without_debug.debug_trace_output.is_none());
        assert!(
            result_with_debug
                .debug_trace_output
                .as_ref()
                .is_some_and(
                    |debug_trace_output| debug_trace_output.captured_transition_count >= 2
                )
        );
    }

    #[test]
    fn seeded_simulation_run_rejects_invalid_debug_trace_sampling_stride() {
        let error = SeededTickSimulationRunConfig::try_new(
            8_011,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            valid_model_inputs(),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
                debug_trace_options: SeededTickSimulationDebugTraceOptions {
                    sample_every_nth_transition: 0,
                    max_records_per_chunk: 128,
                },
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect_err("debug trace with zero sampling stride must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::InvalidDebugTraceSamplingStride { value: 0 }
        ));
    }

    #[test]
    fn seeded_simulation_run_rejects_invalid_debug_trace_chunk_size() {
        let error = SeededTickSimulationRunConfig::try_new(
            8_012,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            valid_model_inputs(),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
                debug_trace_options: SeededTickSimulationDebugTraceOptions {
                    sample_every_nth_transition: 1,
                    max_records_per_chunk: 0,
                },
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect_err("debug trace with zero chunk size must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::InvalidDebugTraceChunkSize { value: 0 }
        ));
    }

    #[test]
    fn seeded_simulation_debug_trace_schema_exposes_required_fields() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            8_013,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(1.0, 1.0, 0.0),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
                debug_trace_options: SeededTickSimulationDebugTraceOptions {
                    sample_every_nth_transition: 1,
                    max_records_per_chunk: 1,
                },
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect("debug trace schema run config should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.25],
            vec![0.5],
            vec![0.0],
            vec![],
            vec![],
            vec![],
        );

        let run_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect("debug trace schema run should complete");

        let serialized_debug_trace = run_result
            .serialize_debug_trace_json()
            .expect("debug trace serialization should succeed")
            .expect("debug trace payload should exist when debug mode is enabled");

        let debug_trace_payload: Value =
            serde_json::from_str(&serialized_debug_trace).expect("debug trace JSON should parse");

        assert_eq!(
            debug_trace_payload["schema_version"],
            SEEDED_TICK_EVENT_DEBUG_TRACE_SCHEMA_VERSION
        );
        assert_eq!(
            debug_trace_payload["metadata"]["schema_version"],
            SEEDED_TICK_EVENT_DEBUG_TRACE_METADATA_SCHEMA_VERSION
        );
        assert_eq!(
            debug_trace_payload["metadata"]["trace_record_schema_version"],
            SEEDED_TICK_EVENT_DEBUG_TRACE_RECORD_SCHEMA_VERSION
        );
        assert_eq!(
            debug_trace_payload["metadata"]["trace_chunk_schema_version"],
            SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION
        );
        assert_eq!(
            debug_trace_payload["metadata"]["news_market_state_routing_policy"],
            SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY
        );
        assert_eq!(
            debug_trace_payload["metadata"]["sampling_policy"],
            SEEDED_TICK_EVENT_DEBUG_TRACE_SAMPLING_POLICY
        );
        assert_eq!(
            debug_trace_payload["metadata"]["chunking_policy"],
            SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNKING_POLICY
        );
        assert_eq!(
            debug_trace_payload["metadata"]["sample_every_nth_transition"],
            1
        );
        assert_eq!(debug_trace_payload["metadata"]["max_records_per_chunk"], 1);
        assert_eq!(
            debug_trace_payload["metadata"]["interval_boundary_drift_always_captured"],
            true
        );

        let trace_chunks = debug_trace_payload["trace_chunks"]
            .as_array()
            .expect("trace_chunks should serialize as an array");

        assert!(!trace_chunks.is_empty());

        let first_chunk = trace_chunks[0]
            .as_object()
            .expect("trace chunk should serialize as an object");

        for required_chunk_field in [
            "schema_version",
            "chunk_index",
            "start_transition_index",
            "end_transition_index",
            "records",
        ] {
            assert!(first_chunk.contains_key(required_chunk_field));
        }

        let first_record = first_chunk["records"][0]
            .as_object()
            .expect("trace record should serialize as an object");

        for required_record_field in [
            "transition_index",
            "interval_index",
            "previous_timestamp_utc_nanos",
            "timestamp_utc_nanos",
            "elapsed_hours_since_previous_update",
            "transition_kind",
            "cumulative_news_activity_coupling_j_t_increment",
            "cumulative_open_window_return_jump_m_t_increment",
            "cumulative_closed_window_gap_g_t_increment",
            "seasonality_factor",
            "interval_news_intensity",
            "state_before_transition",
            "state_after_transition",
        ] {
            assert!(first_record.contains_key(required_record_field));
        }
    }

    #[test]
    fn seeded_simulation_debug_trace_captures_transition_ordering_and_carryover_context() {
        let start_utc_nanos = 1_704_067_200_000_000_000;

        let run_config = SeededTickSimulationRunConfig::try_new(
            8_014,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            1,
            one_interval_model_inputs(1.0, 1.0, 1.0),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
                debug_trace_options: SeededTickSimulationDebugTraceOptions {
                    sample_every_nth_transition: 1,
                    max_records_per_chunk: 8,
                },
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect("debug trace correctness run config should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.3],
            vec![0.3],
            vec![0.0],
            vec![0.3],
            vec![0.5],
            vec![],
        );

        let run_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect("debug trace correctness run should complete");

        let debug_trace_output = run_result
            .debug_trace_output
            .as_ref()
            .expect("debug trace output should exist");

        let trace_records: Vec<_> = debug_trace_output
            .trace_chunks
            .iter()
            .flat_map(|trace_chunk| trace_chunk.records.iter())
            .collect();

        assert_eq!(trace_records.len(), 4);
        assert_eq!(
            trace_records[0].transition_kind,
            SeededTickEventDebugTraceTransitionKind::EndogenousBuyEvent
        );
        assert_eq!(
            trace_records[1].transition_kind,
            SeededTickEventDebugTraceTransitionKind::EndogenousSellEvent
        );
        assert_eq!(
            trace_records[2].transition_kind,
            SeededTickEventDebugTraceTransitionKind::ExogenousNews
        );
        assert_eq!(
            trace_records[3].transition_kind,
            SeededTickEventDebugTraceTransitionKind::IntervalBoundaryDrift
        );

        let expected_update_timestamp_utc_nanos =
            start_utc_nanos + ((ONE_HOUR_NANOS as f64 * 0.3).floor() as i64);
        let expected_interval_end_utc_nanos = start_utc_nanos + ONE_HOUR_NANOS;
        let expected_elapsed_hours_to_update =
            super::derive_elapsed_hours(start_utc_nanos, expected_update_timestamp_utc_nanos);
        let expected_elapsed_hours_to_interval_end = super::derive_elapsed_hours(
            expected_update_timestamp_utc_nanos,
            expected_interval_end_utc_nanos,
        );

        assert_eq!(
            trace_records[0].timestamp_utc_nanos,
            expected_update_timestamp_utc_nanos
        );
        assert_eq!(
            trace_records[1].timestamp_utc_nanos,
            expected_update_timestamp_utc_nanos
        );
        assert_eq!(
            trace_records[2].timestamp_utc_nanos,
            expected_update_timestamp_utc_nanos
        );
        assert_eq!(
            trace_records[3].timestamp_utc_nanos,
            expected_interval_end_utc_nanos
        );
        assert_float_close(
            trace_records[0].elapsed_hours_since_previous_update,
            expected_elapsed_hours_to_update,
        );
        assert_float_close(trace_records[1].elapsed_hours_since_previous_update, 0.0);
        assert_float_close(trace_records[2].elapsed_hours_since_previous_update, 0.0);
        assert_float_close(
            trace_records[3].elapsed_hours_since_previous_update,
            expected_elapsed_hours_to_interval_end,
        );

        assert_eq!(trace_records[0].generation_order, Some(0));
        assert_eq!(trace_records[1].generation_order, Some(1));
        assert_eq!(trace_records[2].generation_order, Some(2));
        assert_eq!(trace_records[3].generation_order, None);
        assert_eq!(trace_records[0].event_index, Some(0));
        assert_eq!(trace_records[1].event_index, Some(1));
        assert_eq!(trace_records[2].event_index, None);
        assert_eq!(trace_records[3].event_index, None);
        assert_float_close(
            trace_records[2]
                .exogenous_news_activity_coupling_mark
                .expect("exogenous transition should include news activity-coupling mark"),
            -(1.0 - 0.5_f64).ln(),
        );
        assert_eq!(
            trace_records[2].exogenous_news_arrival_component,
            Some(NewsArrivalComponent::Surprise)
        );
        assert_eq!(
            trace_records[2].exogenous_news_market_state,
            Some(SeededTickSimulationNewsEventMarketState::Open)
        );
        assert_eq!(
            trace_records[2].exogenous_news_return_jump_routing_target,
            Some(NewsReturnJumpRoutingTarget::OpenWindowDirectJumpM)
        );
        assert!(
            trace_records[0]
                .exogenous_news_activity_coupling_mark
                .is_none()
        );
        assert!(
            trace_records[1]
                .exogenous_news_activity_coupling_mark
                .is_none()
        );
        assert!(
            trace_records[3]
                .exogenous_news_activity_coupling_mark
                .is_none()
        );

        for trace_window in trace_records.windows(2) {
            assert_state_snapshot_close(
                trace_window[0].state_after_transition,
                trace_window[1].state_before_transition,
            );
        }

        assert_eq!(run_result.event_path.len(), 2);
        assert_eq!(run_result.event_path[0].event_index, 0);
        assert_eq!(run_result.event_path[0].side, SeededTickEventSide::Buy);
        assert_eq!(run_result.event_path[1].event_index, 1);
        assert_eq!(run_result.event_path[1].side, SeededTickEventSide::Sell);
    }

    #[test]
    fn seeded_simulation_debug_trace_sampling_and_chunking_controls_limit_payload_size() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            8_015,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(3.0, 3.0, 0.0),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
                debug_trace_options: SeededTickSimulationDebugTraceOptions {
                    sample_every_nth_transition: 2,
                    max_records_per_chunk: 2,
                },
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect("debug trace sampling and chunking run config should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.1, 0.2, 0.3],
            vec![0.4, 0.5, 0.6],
            vec![0.0],
            vec![],
            vec![],
            vec![],
        );

        let run_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect("debug trace sampling and chunking run should complete");

        let debug_trace_output = run_result
            .debug_trace_output
            .as_ref()
            .expect("debug trace output should exist");

        assert_eq!(debug_trace_output.captured_transition_count, 4);
        assert_eq!(debug_trace_output.trace_chunks.len(), 2);
        assert_eq!(debug_trace_output.trace_chunks[0].records.len(), 2);
        assert_eq!(debug_trace_output.trace_chunks[1].records.len(), 2);

        let sampled_transition_indices: Vec<u64> = debug_trace_output
            .trace_chunks
            .iter()
            .flat_map(|trace_chunk| trace_chunk.records.iter())
            .map(|trace_record| trace_record.transition_index)
            .collect();

        assert_eq!(sampled_transition_indices, vec![0, 2, 4, 6]);
    }

    #[test]
    fn seeded_simulation_run_accepts_injected_randomness_factory() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            5_003,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            None,
        )
        .expect("run config should initialize");

        let fixture_factory = FixtureRandomnessFactory;

        let first_result = run_seeded_tick_event_simulation_with_randomness_factory(
            run_config.clone(),
            &fixture_factory,
        )
        .expect("injected fixture randomness run should complete");

        let second_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &fixture_factory)
                .expect("second injected fixture randomness run should complete");

        assert_eq!(first_result.event_path, second_result.event_path);
        assert!(!first_result.event_path.is_empty());
        assert!(is_non_decreasing_timestamp_path(&first_result));
    }

    #[test]
    fn seeded_simulation_run_rejects_zero_horizon() {
        let error = SeededTickSimulationRunConfig::try_new(
            42,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            0,
            valid_model_inputs(),
            None,
        )
        .expect_err("zero horizon must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::InvalidHorizonIntervalCount { value: 0 }
        ));
    }

    #[test]
    fn seeded_simulation_run_rejects_incomplete_hourly_factor_payload() {
        let mut model_inputs = valid_model_inputs();
        model_inputs.hourly_factor_contract.s_h.pop();

        let error = SeededTickSimulationRunConfig::try_new(
            77,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            model_inputs,
            None,
        )
        .expect_err("incomplete hourly factor vectors must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::HourlyFactorFieldLengthMismatch {
                field_name: "s_h",
                expected_interval_count: 2,
                observed_interval_count: 1,
            }
        ));
    }

    #[test]
    fn seeded_simulation_run_rejects_non_consecutive_hourly_factor_coverage() {
        let mut model_inputs = valid_model_inputs();
        let start_utc_nanos = 1_704_067_200_000_000_000;

        model_inputs.hourly_factor_contract.interval_start_utc_nanos[1] += ONE_HOUR_NANOS;
        model_inputs.hourly_factor_contract.interval_end_utc_nanos[1] += ONE_HOUR_NANOS;
        model_inputs.hourly_factor_contract.request_end_utc_nanos += ONE_HOUR_NANOS;
        model_inputs
            .hourly_factor_contract
            .diagnostics
            .last_interval_end_utc_nanos += ONE_HOUR_NANOS;

        let error = SeededTickSimulationRunConfig::try_new(
            88,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            2,
            model_inputs,
            None,
        )
        .expect_err("non-consecutive interval coverage must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::NonConsecutiveHourlyFactorCoverage {
                previous_interval_index: 0,
                previous_interval_end_utc_nanos,
                current_interval_index: 1,
                current_interval_start_utc_nanos,
            } if previous_interval_end_utc_nanos == start_utc_nanos + ONE_HOUR_NANOS
                && current_interval_start_utc_nanos == start_utc_nanos + 2 * ONE_HOUR_NANOS
        ));
    }

    #[test]
    fn seeded_simulation_preserves_full_state_carryover_across_hour_boundaries() {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let model_inputs = two_interval_zero_event_carryover_model_inputs();
        let kernel_parameters = model_inputs.kernel_parameters;

        let one_hour_config = SeededTickSimulationRunConfig::try_new(
            10_052,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            1,
            model_inputs.clone(),
            None,
        )
        .expect("one-hour carryover run config should initialize");

        let two_hour_config = SeededTickSimulationRunConfig::try_new(
            10_052,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            2,
            model_inputs,
            None,
        )
        .expect("two-hour carryover run config should initialize");

        let one_hour_result = run_seeded_tick_event_simulation(one_hour_config)
            .expect("one-hour carryover run should complete");
        let two_hour_result = run_seeded_tick_event_simulation(two_hour_config)
            .expect("two-hour carryover run should complete");

        let expected_two_hour_final_state = super::evolve_hawkes_state(
            one_hour_result.final_excitation_state,
            kernel_parameters,
            HawkesTransitionStep::new(1.0, None, None),
        )
        .expect("deterministic carryover drift step should succeed");

        assert!(one_hour_result.event_path.is_empty());
        assert!(two_hour_result.event_path.is_empty());
        assert_float_close(
            two_hour_result.final_excitation_state.y_plus_plus,
            expected_two_hour_final_state.y_plus_plus,
        );
        assert_float_close(
            two_hour_result.final_excitation_state.y_plus_minus,
            expected_two_hour_final_state.y_plus_minus,
        );
        assert_float_close(
            two_hour_result.final_excitation_state.y_minus_plus,
            expected_two_hour_final_state.y_minus_plus,
        );
        assert_float_close(
            two_hour_result.final_excitation_state.y_minus_minus,
            expected_two_hour_final_state.y_minus_minus,
        );
    }

    #[test]
    fn seeded_simulation_factor_changes_apply_on_hour_boundaries_only() {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let boundary_utc_nanos = start_utc_nanos + ONE_HOUR_NANOS;

        let baseline_run_config = SeededTickSimulationRunConfig::try_new(
            10_053,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            2,
            two_interval_constant_intensity_model_inputs(1.0),
            None,
        )
        .expect("baseline boundary-factor run config should initialize");

        let muted_second_hour_run_config = SeededTickSimulationRunConfig::try_new(
            10_053,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            2,
            two_interval_constant_intensity_model_inputs(0.0),
            None,
        )
        .expect("muted boundary-factor run config should initialize");

        let baseline_result = run_seeded_tick_event_simulation(baseline_run_config)
            .expect("baseline boundary-factor run should complete");
        let muted_second_hour_result =
            run_seeded_tick_event_simulation(muted_second_hour_run_config)
                .expect("muted boundary-factor run should complete");

        assert_eq!(baseline_result.event_path.len(), 8);
        assert_eq!(muted_second_hour_result.event_path.len(), 4);
        assert!(
            muted_second_hour_result
                .event_path
                .iter()
                .all(|event_record| event_record.timestamp_utc_nanos < boundary_utc_nanos)
        );
        assert_eq!(
            &baseline_result.event_path[..muted_second_hour_result.event_path.len()],
            muted_second_hour_result.event_path.as_slice()
        );
    }

    #[test]
    fn seeded_simulation_scheduler_interleaves_buy_and_sell_arrivals() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            1_007,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(2.0, 2.0, 0.0),
            None,
        )
        .expect("interleaved schedule run config should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.75, 0.25],
            vec![0.5, 0.1],
            vec![0.0],
            vec![],
            vec![],
            vec![],
        );
        let run_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect("interleaved schedule run should complete");

        let observed_sequence: Vec<(i64, SeededTickEventSide)> = run_result
            .event_path
            .iter()
            .map(|event_record| (event_record.timestamp_utc_nanos, event_record.side))
            .collect();

        let interval_start_utc_nanos = 1_704_067_200_000_000_000;
        let expected_sequence = vec![
            (
                interval_start_utc_nanos + ONE_HOUR_NANOS / 10,
                SeededTickEventSide::Sell,
            ),
            (
                interval_start_utc_nanos + ONE_HOUR_NANOS / 4,
                SeededTickEventSide::Buy,
            ),
            (
                interval_start_utc_nanos + ONE_HOUR_NANOS / 2,
                SeededTickEventSide::Sell,
            ),
            (
                interval_start_utc_nanos + 3 * ONE_HOUR_NANOS / 4,
                SeededTickEventSide::Buy,
            ),
        ];

        assert_eq!(observed_sequence, expected_sequence);
        assert!(is_non_decreasing_timestamp_path(&run_result));
    }

    #[test]
    fn seeded_simulation_scheduler_applies_buy_before_sell_tie_ordering_policy() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            2_008,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(1.0, 1.0, 0.0),
            None,
        )
        .expect("tie-ordering run config should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.3],
            vec![0.3],
            vec![0.0],
            vec![],
            vec![],
            vec![],
        );
        let run_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect("tie-ordering run should complete");

        assert_eq!(run_result.event_path.len(), 2);
        assert_eq!(
            run_result.event_path[0].timestamp_utc_nanos,
            run_result.event_path[1].timestamp_utc_nanos
        );
        assert_eq!(run_result.event_path[0].side, SeededTickEventSide::Buy);
        assert_eq!(run_result.event_path[1].side, SeededTickEventSide::Sell);
        assert_eq!(
            run_result.event_ordering_policy,
            SEEDED_TICK_EVENT_ORDERING_POLICY
        );
    }

    #[test]
    fn seeded_simulation_scheduler_applies_known_before_surprise_tie_ordering_policy() {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let tied_timestamp_utc_nanos =
            start_utc_nanos + ((ONE_HOUR_NANOS as f64 * 0.3).floor() as i64);

        let mut model_inputs = one_interval_model_inputs(1.0, 1.0, 2.0);
        model_inputs.known_scheduled_news_event_timestamps_utc_nanos =
            vec![tied_timestamp_utc_nanos];

        let run_config = SeededTickSimulationRunConfig::try_new(
            2_009,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            1,
            model_inputs,
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
                debug_trace_options: SeededTickSimulationDebugTraceOptions {
                    sample_every_nth_transition: 1,
                    max_records_per_chunk: 16,
                },
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect("known-surprise tie-ordering run config should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.3],
            vec![0.3],
            vec![0.0],
            vec![0.3],
            vec![0.4, 0.6],
            vec![0.75, 0.2, 0.25, 0.3],
        );
        let run_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect("known-surprise tie-ordering run should complete");

        let trace_records = run_result
            .debug_trace_output
            .as_ref()
            .expect("debug trace output should exist")
            .trace_chunks
            .iter()
            .flat_map(|trace_chunk| trace_chunk.records.iter())
            .collect::<Vec<_>>();

        assert_eq!(trace_records.len(), 5);
        assert_eq!(
            trace_records[0].transition_kind,
            SeededTickEventDebugTraceTransitionKind::EndogenousBuyEvent
        );
        assert_eq!(
            trace_records[1].transition_kind,
            SeededTickEventDebugTraceTransitionKind::EndogenousSellEvent
        );
        assert_eq!(
            trace_records[2].exogenous_news_arrival_component,
            Some(NewsArrivalComponent::Known)
        );
        assert_eq!(
            trace_records[3].exogenous_news_arrival_component,
            Some(NewsArrivalComponent::Surprise)
        );
        assert_eq!(trace_records[2].generation_order, Some(2));
        assert_eq!(trace_records[3].generation_order, Some(3));
        assert_eq!(
            trace_records[2].timestamp_utc_nanos,
            trace_records[3].timestamp_utc_nanos
        );
        assert_eq!(
            trace_records[2].timestamp_utc_nanos,
            tied_timestamp_utc_nanos
        );

        assert_eq!(run_result.event_path.len(), 2);
        assert_eq!(run_result.event_path[0].side, SeededTickEventSide::Buy);
        assert_eq!(run_result.event_path[1].side, SeededTickEventSide::Sell);
    }

    #[test]
    fn seeded_simulation_news_routing_applies_open_window_to_m_and_closed_window_to_g() {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let mut model_inputs = valid_model_inputs();
        model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
            0.5,
            1.5,
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
        )
        .expect("routing kernel fixture should be valid");
        model_inputs.initial_excitation_state = HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        };
        model_inputs.hourly_factor_contract.s_h = vec![1.0, 0.0];
        model_inputs.hourly_factor_contract.nu_h = vec![0.0, 0.0];
        model_inputs.known_scheduled_news_event_timestamps_utc_nanos = vec![
            start_utc_nanos + ONE_HOUR_NANOS / 5,
            start_utc_nanos + ONE_HOUR_NANOS + ONE_HOUR_NANOS / 5,
        ];

        let run_config = SeededTickSimulationRunConfig::try_new(
            2_010,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            2,
            model_inputs,
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
                debug_trace_options: SeededTickSimulationDebugTraceOptions {
                    sample_every_nth_transition: 1,
                    max_records_per_chunk: 16,
                },
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
            }),
        )
        .expect("open-closed routing run config should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![1.0, 1.0],
            vec![1.0, 1.0],
            vec![],
            vec![],
            vec![1.0, 1.0],
            vec![],
            vec![0.5, 0.8],
            vec![0.75, 0.5, 0.25, 0.5],
        );
        let run_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect("open-closed routing run should complete");

        let trace_records = run_result
            .debug_trace_output
            .as_ref()
            .expect("debug trace output should exist")
            .trace_chunks
            .iter()
            .flat_map(|trace_chunk| trace_chunk.records.iter())
            .filter(|trace_record| {
                trace_record.transition_kind
                    == SeededTickEventDebugTraceTransitionKind::ExogenousNews
            })
            .collect::<Vec<_>>();

        assert_eq!(trace_records.len(), 2);
        assert_eq!(
            trace_records[0].exogenous_news_market_state,
            Some(SeededTickSimulationNewsEventMarketState::Open)
        );
        assert_eq!(
            trace_records[0].exogenous_news_return_jump_routing_target,
            Some(NewsReturnJumpRoutingTarget::OpenWindowDirectJumpM)
        );
        assert_eq!(
            trace_records[1].exogenous_news_market_state,
            Some(SeededTickSimulationNewsEventMarketState::Closed)
        );
        assert_eq!(
            trace_records[1].exogenous_news_return_jump_routing_target,
            Some(NewsReturnJumpRoutingTarget::ClosedWindowGapG)
        );

        let first_activity = -(1.0 - 0.5_f64).ln();
        let first_return_jump = first_activity.sqrt() * -(1.0 - 0.5_f64).ln();
        let second_activity = -(1.0 - 0.5_f64).ln();
        let second_return_jump = second_activity.sqrt() * -(1.0 - 0.5_f64).ln();

        assert_float_close(
            run_result
                .news_routing_totals
                .cumulative_activity_coupling_j_t_increment,
            first_activity + second_activity,
        );
        assert_float_close(
            run_result
                .news_routing_totals
                .cumulative_open_window_return_jump_m_t_increment,
            first_return_jump,
        );
        assert_float_close(
            run_result
                .news_routing_totals
                .cumulative_closed_window_gap_g_t_increment,
            second_return_jump,
        );
    }

    #[test]
    fn seeded_simulation_rejects_invalid_generated_news_return_jump_mark() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            2_011,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(0.5, 0.5, 1.0),
            None,
        )
        .expect("invalid return-jump mark run config should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![1.0],
            vec![1.0],
            vec![],
            vec![],
            vec![0.0],
            vec![0.3],
            vec![0.5],
            vec![0.75, f64::NAN],
        );
        let error =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect_err("invalid generated return-jump mark must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::InvalidGeneratedNewsReturnJumpMark { value } if value.is_nan()
        ));
    }

    #[test]
    fn seeded_simulation_rejects_invalid_news_routing_state_violations() {
        let error = super::validate_news_return_jump_routing_state(MarketState::Open, 0.1, 0.2)
            .expect_err("open-window routing with non-zero closed-gap increment must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::InvalidNewsRoutingStateViolation {
                market_state: MarketState::Open,
                open_window_return_jump_increment,
                closed_window_gap_increment,
            } if (open_window_return_jump_increment - 0.1).abs() <= 1e-12
                && (closed_window_gap_increment - 0.2).abs() <= 1e-12
        ));
    }

    #[test]
    fn seeded_simulation_boundary_adjacent_event_times_are_not_snapped_to_hour_edges() {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let boundary_utc_nanos = start_utc_nanos + ONE_HOUR_NANOS;

        let run_config = SeededTickSimulationRunConfig::try_new(
            10_054,
            DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
            2,
            two_interval_boundary_adjacent_model_inputs(),
            None,
        )
        .expect("boundary-adjacent run config should initialize");

        let run_result = run_seeded_tick_event_simulation_with_randomness_factory(
            run_config,
            &BoundaryAdjacentRandomnessFactory,
        )
        .expect("boundary-adjacent run should complete");

        assert_eq!(run_result.event_path.len(), 2);

        let first_timestamp_utc_nanos = run_result.event_path[0].timestamp_utc_nanos;
        let second_timestamp_utc_nanos = run_result.event_path[1].timestamp_utc_nanos;

        assert!(first_timestamp_utc_nanos < boundary_utc_nanos);
        assert!(boundary_utc_nanos - first_timestamp_utc_nanos <= 4);
        assert!(second_timestamp_utc_nanos > boundary_utc_nanos);
        assert!(second_timestamp_utc_nanos - boundary_utc_nanos <= 4);
        assert_ne!(first_timestamp_utc_nanos, boundary_utc_nanos);
        assert_ne!(second_timestamp_utc_nanos, boundary_utc_nanos);
    }

    #[test]
    fn seeded_simulation_state_snapshot_captures_post_event_transition_boundary() {
        let mut model_inputs = one_interval_model_inputs(0.5, 0.5, 0.0);
        model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
            0.5,
            1.5,
            HawkesKernelLoadings {
                plus_plus: 0.4,
                plus_minus: 0.0,
                minus_plus: 0.2,
                minus_minus: 0.0,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
        )
        .expect("snapshot boundary kernel fixture should be valid");
        model_inputs.initial_excitation_state = HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        };

        let run_config = SeededTickSimulationRunConfig::try_new(
            7_047,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            model_inputs,
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: false,
                debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::PostEventTransition,
            }),
        )
        .expect("snapshot boundary run config should initialize");
        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![1.0],
            vec![0.0],
            vec![],
            vec![0.0],
            vec![],
            vec![],
            vec![],
        );
        let run_result =
            run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
                .expect("snapshot boundary run should complete");

        assert_eq!(run_result.event_path.len(), 1);
        assert_eq!(
            run_result.event_tape_metadata.state_snapshot_capture,
            SeededTickEventStateSnapshotCapture::PostEventTransition
        );
        assert_eq!(
            run_result
                .event_tape_metadata
                .state_snapshot_capture_point_policy,
            Some(SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY)
        );

        let snapshot = run_result.event_path[0]
            .state_snapshot
            .expect("snapshot capture must populate state_snapshot");

        assert_float_close(snapshot.y_plus_plus, 0.4);
        assert_float_close(snapshot.y_plus_minus, 0.0);
        assert_float_close(snapshot.y_minus_plus, 0.2);
        assert_float_close(snapshot.y_minus_minus, 0.0);
        assert_float_close(snapshot.deseasonalized_lambda_plus, 0.9);
        assert_float_close(snapshot.deseasonalized_lambda_minus, 0.7);
    }

    #[test]
    fn seeded_simulation_event_tape_schema_exposes_required_and_optional_fields() {
        let run_config_without_snapshot = SeededTickSimulationRunConfig::try_new(
            8_048,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(1.0, 0.5, 0.0),
            None,
        )
        .expect("schema run config without snapshot should initialize");

        let scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![1.0],
            vec![0.25],
            vec![],
            vec![0.0],
            vec![],
            vec![],
            vec![],
        );

        let run_result_without_snapshot = run_seeded_tick_event_simulation_with_randomness_factory(
            run_config_without_snapshot,
            &scripted_factory,
        )
        .expect("schema run without snapshot should complete");

        let serialized_without_snapshot = run_result_without_snapshot
            .serialize_event_tape_json()
            .expect("event tape serialization without snapshot should succeed");
        let payload_without_snapshot: Value = serde_json::from_str(&serialized_without_snapshot)
            .expect("serialized event tape payload should parse");

        assert_eq!(
            payload_without_snapshot["schema_version"],
            SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["schema_version"],
            SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["event_record_schema_version"],
            SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["state_snapshot_schema_version"],
            SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["timestamp_representation_policy"],
            SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["side_encoding_policy"],
            SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY
        );
        assert_eq!(
            payload_without_snapshot["metadata"]["state_snapshot_capture"],
            "disabled"
        );
        assert!(
            payload_without_snapshot["metadata"]
                .get("state_snapshot_capture_point_policy")
                .is_none()
        );

        let serialized_events_without_snapshot = payload_without_snapshot["event_tape"]
            .as_array()
            .expect("event_tape must serialize as an array");
        assert_eq!(serialized_events_without_snapshot.len(), 1);

        let first_event_without_snapshot = serialized_events_without_snapshot[0]
            .as_object()
            .expect("event record must serialize as an object");

        for required_field in ["event_index", "timestamp_utc_nanos", "side"] {
            assert!(first_event_without_snapshot.contains_key(required_field));
        }

        assert!(!first_event_without_snapshot.contains_key("state_snapshot"));
        assert!(!first_event_without_snapshot.contains_key("interval_index"));

        let run_config_with_snapshot = SeededTickSimulationRunConfig::try_new(
            8_048,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(1.0, 0.5, 0.0),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: false,
                debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::PostEventTransition,
            }),
        )
        .expect("schema run config with snapshot should initialize");

        let run_result_with_snapshot = run_seeded_tick_event_simulation_with_randomness_factory(
            run_config_with_snapshot,
            &scripted_factory,
        )
        .expect("schema run with snapshot should complete");

        let serialized_with_snapshot = run_result_with_snapshot
            .serialize_event_tape_json()
            .expect("event tape serialization with snapshot should succeed");
        let payload_with_snapshot: Value = serde_json::from_str(&serialized_with_snapshot)
            .expect("serialized event tape payload should parse");

        assert_eq!(
            payload_with_snapshot["metadata"]["state_snapshot_capture"],
            "post_event_transition"
        );
        assert_eq!(
            payload_with_snapshot["metadata"]["state_snapshot_capture_point_policy"],
            SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY
        );

        let first_event_with_snapshot = payload_with_snapshot["event_tape"][0]
            .as_object()
            .expect("event record with snapshot must serialize as an object");
        let state_snapshot = first_event_with_snapshot
            .get("state_snapshot")
            .expect("event record must include state_snapshot when capture is enabled")
            .as_object()
            .expect("state_snapshot must serialize as an object");

        for required_snapshot_field in [
            "y_plus_plus",
            "y_plus_minus",
            "y_minus_plus",
            "y_minus_minus",
            "deseasonalized_lambda_plus",
            "deseasonalized_lambda_minus",
        ] {
            assert!(state_snapshot.contains_key(required_snapshot_field));
        }
    }

    #[test]
    fn seeded_simulation_event_tape_serialization_is_deterministic() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            9_049,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: false,
                debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
                state_snapshot_capture: SeededTickEventStateSnapshotCapture::PostEventTransition,
            }),
        )
        .expect("serialization determinism run config should initialize");

        let first_result = run_seeded_tick_event_simulation(run_config.clone())
            .expect("first serialization determinism run should complete");
        let second_result = run_seeded_tick_event_simulation(run_config)
            .expect("second serialization determinism run should complete");

        let first_json = first_result
            .serialize_event_tape_json()
            .expect("first event tape serialization should succeed");
        let second_json = second_result
            .serialize_event_tape_json()
            .expect("second event tape serialization should succeed");

        assert_eq!(first_json, second_json);
    }

    #[test]
    fn seeded_simulation_tied_and_near_tied_timestamps_serialize_deterministically() {
        let tie_run_config = SeededTickSimulationRunConfig::try_new(
            10_050,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(1.0, 1.0, 0.0),
            None,
        )
        .expect("tie serialization run config should initialize");

        let tie_scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.3],
            vec![0.3],
            vec![0.0],
            vec![],
            vec![],
            vec![],
        );

        let tie_result = run_seeded_tick_event_simulation_with_randomness_factory(
            tie_run_config,
            &tie_scripted_factory,
        )
        .expect("tie serialization run should complete");

        let tie_payload: Value = serde_json::from_str(
            &tie_result
                .serialize_event_tape_json()
                .expect("tie event tape serialization should succeed"),
        )
        .expect("tie serialized payload should parse");

        let tie_events = tie_payload["event_tape"]
            .as_array()
            .expect("tie event_tape must serialize as an array");

        assert_eq!(tie_events.len(), 2);
        assert_eq!(
            tie_events[0]["timestamp_utc_nanos"],
            tie_events[1]["timestamp_utc_nanos"]
        );
        assert_eq!(tie_events[0]["side"], "buy");
        assert_eq!(tie_events[1]["side"], "sell");
        assert_eq!(
            tie_payload["metadata"]["timestamp_representation_policy"],
            SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY
        );
        assert!(tie_events[0].get("interval_index").is_none());
        assert!(tie_events[1].get("interval_index").is_none());

        let near_tie_run_config = SeededTickSimulationRunConfig::try_new(
            10_051,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            one_interval_model_inputs(1.0, 1.0, 0.0),
            None,
        )
        .expect("near-tie serialization run config should initialize");

        let near_tie_scripted_factory = ScriptedRandomnessFactory::new(
            vec![0.0],
            vec![0.0],
            vec![0.300000001],
            vec![0.3],
            vec![0.0],
            vec![],
            vec![],
            vec![],
        );

        let near_tie_result = run_seeded_tick_event_simulation_with_randomness_factory(
            near_tie_run_config,
            &near_tie_scripted_factory,
        )
        .expect("near-tie serialization run should complete");

        let near_tie_payload: Value = serde_json::from_str(
            &near_tie_result
                .serialize_event_tape_json()
                .expect("near-tie event tape serialization should succeed"),
        )
        .expect("near-tie serialized payload should parse");

        let near_tie_events = near_tie_payload["event_tape"]
            .as_array()
            .expect("near-tie event_tape must serialize as an array");

        assert_eq!(near_tie_events.len(), 2);

        let first_timestamp = near_tie_events[0]["timestamp_utc_nanos"]
            .as_i64()
            .expect("first near-tie timestamp should parse");
        let second_timestamp = near_tie_events[1]["timestamp_utc_nanos"]
            .as_i64()
            .expect("second near-tie timestamp should parse");

        assert!(first_timestamp < second_timestamp);
        assert_eq!(near_tie_events[0]["side"], "sell");
        assert_eq!(near_tie_events[1]["side"], "buy");
        assert!(
            !near_tie_events[0]
                .as_object()
                .is_some_and(|entry| entry.contains_key("bucket"))
        );
        assert!(
            !near_tie_events[1]
                .as_object()
                .is_some_and(|entry| entry.contains_key("bucket"))
        );
    }

    fn is_non_decreasing_timestamp_path(run_result: &super::SeededTickSimulationRunResult) -> bool {
        run_result.event_path.windows(2).all(|event_window| {
            event_window[0].timestamp_utc_nanos <= event_window[1].timestamp_utc_nanos
        })
    }

    fn assert_state_snapshot_close(
        observed: super::SeededTickEventStateSnapshot,
        expected: super::SeededTickEventStateSnapshot,
    ) {
        assert_float_close(observed.y_plus_plus, expected.y_plus_plus);
        assert_float_close(observed.y_plus_minus, expected.y_plus_minus);
        assert_float_close(observed.y_minus_plus, expected.y_minus_plus);
        assert_float_close(observed.y_minus_minus, expected.y_minus_minus);
        assert_float_close(
            observed.deseasonalized_lambda_plus,
            expected.deseasonalized_lambda_plus,
        );
        assert_float_close(
            observed.deseasonalized_lambda_minus,
            expected.deseasonalized_lambda_minus,
        );
    }

    fn assert_float_close(observed: f64, expected: f64) {
        assert!(
            (observed - expected).abs() <= 1e-12,
            "float mismatch, expected {expected}, observed {observed}"
        );
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FixtureRandomnessFactory;

    impl SimulationRandomnessFactory for FixtureRandomnessFactory {
        type Stream = FixtureRandomnessStream;

        fn create_substream(
            &self,
            _root_seed: u64,
            substream_domain: SimulationRandomnessSubstreamDomain,
            substream_index: u64,
        ) -> Self::Stream {
            FixtureRandomnessStream {
                substream_domain,
                substream_index,
                draw_count: 0,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FixtureRandomnessStream {
        substream_domain: SimulationRandomnessSubstreamDomain,
        substream_index: u64,
        draw_count: u64,
    }

    impl SimulationRandomnessStream for FixtureRandomnessStream {
        fn next_u64(&mut self) -> u64 {
            self.draw_count = self.draw_count.saturating_add(1);

            match self.substream_domain {
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount => 0,
                SimulationRandomnessSubstreamDomain::EndogenousSellEventCount => {
                    u64::MAX.saturating_sub(1)
                }
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp => {
                    self.substream_index.wrapping_add(self.draw_count)
                }
                SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp => {
                    self.substream_index.wrapping_add(self.draw_count)
                }
                SimulationRandomnessSubstreamDomain::ExogenousNewsCount => 0,
                SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp => 0,
                SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity => 0,
                SimulationRandomnessSubstreamDomain::ExogenousNewsReturnJump => 0,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct ScriptedRandomnessFactory {
        buy_count_draws: Vec<f64>,
        sell_count_draws: Vec<f64>,
        buy_timestamp_draws: Vec<f64>,
        sell_timestamp_draws: Vec<f64>,
        news_count_draws: Vec<f64>,
        news_timestamp_draws: Vec<f64>,
        news_severity_draws: Vec<f64>,
        news_return_jump_draws: Vec<f64>,
    }

    impl ScriptedRandomnessFactory {
        fn new(
            buy_count_draws: Vec<f64>,
            sell_count_draws: Vec<f64>,
            buy_timestamp_draws: Vec<f64>,
            sell_timestamp_draws: Vec<f64>,
            news_count_draws: Vec<f64>,
            news_timestamp_draws: Vec<f64>,
            news_severity_draws: Vec<f64>,
            news_return_jump_draws: Vec<f64>,
        ) -> Self {
            Self {
                buy_count_draws,
                sell_count_draws,
                buy_timestamp_draws,
                sell_timestamp_draws,
                news_count_draws,
                news_timestamp_draws,
                news_severity_draws,
                news_return_jump_draws,
            }
        }
    }

    impl SimulationRandomnessFactory for ScriptedRandomnessFactory {
        type Stream = ScriptedRandomnessStream;

        fn create_substream(
            &self,
            _root_seed: u64,
            substream_domain: SimulationRandomnessSubstreamDomain,
            _substream_index: u64,
        ) -> Self::Stream {
            let draws = match substream_domain {
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount => {
                    self.buy_count_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::EndogenousSellEventCount => {
                    self.sell_count_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp => {
                    self.buy_timestamp_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp => {
                    self.sell_timestamp_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::ExogenousNewsCount => {
                    self.news_count_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp => {
                    self.news_timestamp_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity => {
                    self.news_severity_draws.clone()
                }
                SimulationRandomnessSubstreamDomain::ExogenousNewsReturnJump => {
                    self.news_return_jump_draws.clone()
                }
            };

            ScriptedRandomnessStream::new(draws)
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct ScriptedRandomnessStream {
        draws: Vec<f64>,
        position: usize,
    }

    impl ScriptedRandomnessStream {
        fn new(draws: Vec<f64>) -> Self {
            Self { draws, position: 0 }
        }
    }

    impl SimulationRandomnessStream for ScriptedRandomnessStream {
        fn next_u64(&mut self) -> u64 {
            let unit_draw = self.next_unit_interval_f64();
            (unit_draw.clamp(0.0, 1.0) * u64::MAX as f64).floor() as u64
        }

        fn next_unit_interval_f64(&mut self) -> f64 {
            let next_draw = self.draws.get(self.position).copied().unwrap_or(0.0);
            self.position = self.position.saturating_add(1);
            next_draw.clamp(0.0, 1.0)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct BoundaryAdjacentRandomnessFactory;

    impl SimulationRandomnessFactory for BoundaryAdjacentRandomnessFactory {
        type Stream = ScriptedRandomnessStream;

        fn create_substream(
            &self,
            _root_seed: u64,
            substream_domain: SimulationRandomnessSubstreamDomain,
            substream_index: u64,
        ) -> Self::Stream {
            let draws = match substream_domain {
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount => vec![0.0],
                SimulationRandomnessSubstreamDomain::EndogenousSellEventCount => vec![0.0],
                SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp => {
                    if substream_index == 0 {
                        vec![0.999_999_999_999]
                    } else {
                        vec![0.000_000_000_001]
                    }
                }
                SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp => vec![],
                SimulationRandomnessSubstreamDomain::ExogenousNewsCount => vec![0.0],
                SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp => vec![],
                SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity => vec![],
                SimulationRandomnessSubstreamDomain::ExogenousNewsReturnJump => vec![],
            };

            ScriptedRandomnessStream::new(draws)
        }
    }

    fn one_interval_model_inputs(
        lambda_plus: f64,
        lambda_minus: f64,
        news_intensity: f64,
    ) -> SeededTickSimulationModelInputs {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let interval_start_utc_nanos = vec![start_utc_nanos];
        let interval_end_utc_nanos = vec![start_utc_nanos + ONE_HOUR_NANOS];

        SeededTickSimulationModelInputs {
            kernel_parameters: HawkesKernelParameters::try_new(
                0.5,
                1.5,
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
            )
            .expect("one-interval kernel parameter fixture should be valid"),
            initial_excitation_state: HawkesExcitationState {
                y_plus_plus: lambda_plus - 0.5,
                y_plus_minus: 0.0,
                y_minus_plus: lambda_minus - 0.5,
                y_minus_minus: 0.0,
            },
            hourly_factor_contract: HourlyFactorContractOutput {
                schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
                exchange_code: "XNYS".to_string(),
                exchange_timezone_name: "America/New_York".to_string(),
                request_start_utc_nanos: start_utc_nanos,
                request_end_utc_nanos: start_utc_nanos + ONE_HOUR_NANOS,
                interval_indices: vec![0],
                interval_start_utc_nanos: interval_start_utc_nanos.clone(),
                interval_end_utc_nanos: interval_end_utc_nanos.clone(),
                interval_start_exchange_local: vec!["2024-01-22T09:00:00-05:00".to_string()],
                interval_end_exchange_local: vec!["2024-01-22T10:00:00-05:00".to_string()],
                s_h: vec![1.0],
                nu_h: vec![news_intensity],
                metadata: HourlyFactorMaterializationMetadata {
                    seasonality_materialization_method:
                        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD.to_string(),
                    news_intensity_materialization_method:
                        DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD.to_string(),
                    interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
                    interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
                    timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
                    spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
                    fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
                },
                diagnostics: HourlyFactorContractDiagnostics {
                    interval_count: 1,
                    first_interval_start_utc_nanos: interval_start_utc_nanos[0],
                    last_interval_end_utc_nanos: interval_end_utc_nanos[0],
                    skipped_local_hour_interval_count: 0,
                    repeated_local_hour_interval_count: 0,
                },
            },
            known_scheduled_news_event_timestamps_utc_nanos: Vec::new(),
            known_news_mark_scales: SeededTickSimulationNewsMarkScales::default(),
            surprise_news_mark_scales: SeededTickSimulationNewsMarkScales::default(),
        }
    }

    fn two_interval_zero_event_carryover_model_inputs() -> SeededTickSimulationModelInputs {
        let mut model_inputs = valid_model_inputs();

        model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
            0.5,
            1.25,
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
        )
        .expect("zero-event carryover kernel parameter fixture should be valid");

        model_inputs.initial_excitation_state = HawkesExcitationState {
            y_plus_plus: 1.6,
            y_plus_minus: 0.8,
            y_minus_plus: 1.2,
            y_minus_minus: 0.4,
        };

        model_inputs.hourly_factor_contract.s_h = vec![0.0, 0.0];
        model_inputs.hourly_factor_contract.nu_h = vec![0.0, 0.0];

        model_inputs
    }

    fn two_interval_constant_intensity_model_inputs(
        second_interval_seasonality_factor: f64,
    ) -> SeededTickSimulationModelInputs {
        let mut model_inputs = valid_model_inputs();

        model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
            2.0,
            1.0,
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
        )
        .expect("constant-intensity kernel parameter fixture should be valid");

        model_inputs.initial_excitation_state = HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        };

        model_inputs.hourly_factor_contract.s_h = vec![1.0, second_interval_seasonality_factor];
        model_inputs.hourly_factor_contract.nu_h = vec![0.0, 0.0];

        model_inputs
    }

    fn two_interval_boundary_adjacent_model_inputs() -> SeededTickSimulationModelInputs {
        let mut model_inputs = valid_model_inputs();

        model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
            1.0,
            1.0,
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
        )
        .expect("boundary-adjacent kernel parameter fixture should be valid");

        model_inputs.initial_excitation_state = HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: -10.0,
            y_minus_minus: 0.0,
        };

        model_inputs.hourly_factor_contract.s_h = vec![1.0, 1.0];
        model_inputs.hourly_factor_contract.nu_h = vec![0.0, 0.0];

        model_inputs
    }

    fn valid_model_inputs() -> SeededTickSimulationModelInputs {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let interval_start_utc_nanos = vec![start_utc_nanos, start_utc_nanos + ONE_HOUR_NANOS];
        let interval_end_utc_nanos = vec![
            start_utc_nanos + ONE_HOUR_NANOS,
            start_utc_nanos + 2 * ONE_HOUR_NANOS,
        ];

        SeededTickSimulationModelInputs {
            kernel_parameters: HawkesKernelParameters::try_new(
                0.8,
                1.5,
                HawkesKernelLoadings {
                    plus_plus: 0.3,
                    plus_minus: 0.15,
                    minus_plus: 0.1,
                    minus_minus: 0.25,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.05,
                    plus_minus: 0.02,
                    minus_plus: 0.03,
                    minus_minus: 0.06,
                },
            )
            .expect("kernel parameter fixture should be valid"),
            initial_excitation_state: HawkesExcitationState {
                y_plus_plus: 0.2,
                y_plus_minus: 0.1,
                y_minus_plus: 0.15,
                y_minus_minus: 0.3,
            },
            hourly_factor_contract: HourlyFactorContractOutput {
                schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
                exchange_code: "XNYS".to_string(),
                exchange_timezone_name: "America/New_York".to_string(),
                request_start_utc_nanos: start_utc_nanos,
                request_end_utc_nanos: start_utc_nanos + 2 * ONE_HOUR_NANOS,
                interval_indices: vec![0, 1],
                interval_start_utc_nanos: interval_start_utc_nanos.clone(),
                interval_end_utc_nanos: interval_end_utc_nanos.clone(),
                interval_start_exchange_local: vec![
                    "2024-01-22T09:00:00-05:00".to_string(),
                    "2024-01-22T10:00:00-05:00".to_string(),
                ],
                interval_end_exchange_local: vec![
                    "2024-01-22T10:00:00-05:00".to_string(),
                    "2024-01-22T11:00:00-05:00".to_string(),
                ],
                s_h: vec![0.95, 1.05],
                nu_h: vec![0.02, 0.03],
                metadata: HourlyFactorMaterializationMetadata {
                    seasonality_materialization_method:
                        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD.to_string(),
                    news_intensity_materialization_method:
                        DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD.to_string(),
                    interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
                    interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
                    timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
                    spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
                    fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
                },
                diagnostics: HourlyFactorContractDiagnostics {
                    interval_count: 2,
                    first_interval_start_utc_nanos: interval_start_utc_nanos[0],
                    last_interval_end_utc_nanos: interval_end_utc_nanos[1],
                    skipped_local_hour_interval_count: 0,
                    repeated_local_hour_interval_count: 0,
                },
            },
            known_scheduled_news_event_timestamps_utc_nanos: Vec::new(),
            known_news_mark_scales: SeededTickSimulationNewsMarkScales::default(),
            surprise_news_mark_scales: SeededTickSimulationNewsMarkScales::default(),
        }
    }
}
