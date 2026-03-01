//! Simulation schema and policy constants.

use crate::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration,
};
use crate::randomness::{
    SIMULATION_RANDOMNESS_STREAM_POLICY, SIMULATION_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
};

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
pub const SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY: &str = "calendar_resolve_market_state_at_utc_routes_open_news_return_jump_to_open_m_channel_and_closed_news_return_jump_to_closed_g_channel.v1";
pub const SEEDED_TICK_EVENT_DEBUG_TRACE_SAMPLING_POLICY: &str =
    "capture_every_nth_transition_with_mandatory_interval_boundary_drift_records.v1";
pub const SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNKING_POLICY: &str =
    "fixed_max_records_per_chunk_preserving_transition_order.v1";

pub const SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY: &str = SIMULATION_RANDOMNESS_STREAM_POLICY;
pub const SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY: &str =
    SIMULATION_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY;

pub(crate) const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;
pub(crate) const ONE_SECOND_NANOS: i64 = 1_000_000_000;
pub(crate) const MAX_STOCHASTIC_UPDATES_PER_INTERVAL: u64 = 1_000_000;
pub(crate) const DEFAULT_DEBUG_TRACE_SAMPLE_EVERY_NTH_TRANSITION: u64 = 1;
pub(crate) const DEFAULT_DEBUG_TRACE_MAX_RECORDS_PER_CHUNK: usize = 1_024;
// Seeded simulation now follows the shared contract pipeline:
// U ~ Gamma(k, theta), epsilon ~ Kou(p, eta_plus, eta_minus),
// Z = sigma_g * sqrt(U) * epsilon.
// These defaults keep a neutral baseline while component scales remain explicit.
pub(crate) const SEEDED_TICK_EXOGENOUS_NEWS_SHARED_GAMMA_CONFIGURATION:
    GammaSeverityMarkConfiguration = GammaSeverityMarkConfiguration::new(1.0, 1.0);
pub(crate) const SEEDED_TICK_EXOGENOUS_NEWS_SHARED_BASE_SHOCK_CONFIGURATION:
    DoubleExponentialBaseShockConfiguration =
    DoubleExponentialBaseShockConfiguration::new(0.5, 1.0, 1.0);
pub(crate) const SEEDED_TICK_EXOGENOUS_NEWS_SHARED_COUPLED_RETURN_JUMP_CONFIGURATION:
    CoupledGapMarkConfiguration = CoupledGapMarkConfiguration::new(1.0);
