//! Seeded simulation runtime and transition scheduling.

use chrono::{DateTime, Utc};

use crate::calendar::{ExchangeCalendarRegistry, MarketState};
use crate::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelParameters, HawkesTransitionError,
    HawkesTransitionStep, evolve_hawkes_state,
};
use crate::news::{
    CoupledGapMarkConstructionError, DoubleExponentialBaseShockSamplingError,
    GammaSeverityMarkSamplingError, NewsArrivalComponent, NewsReturnJumpRoutingTarget,
};
use crate::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessStream, SimulationRandomnessSubstreamDomain,
    SplitMix64SimulationRandomnessFactory,
};

use super::artifacts::{
    SeededTickEventDebugTraceChunk, SeededTickEventDebugTraceMetadata,
    SeededTickEventDebugTraceOutput, SeededTickEventDebugTraceRecord,
    SeededTickEventDebugTraceTransitionKind, SeededTickEventRecord, SeededTickEventSide,
    SeededTickEventStateSnapshot, SeededTickEventTapeMetadata,
    SeededTickSimulationNewsEventMarketState, SeededTickSimulationNewsRoutingTotals,
    SeededTickSimulationRunResult,
};
use super::config::{
    SeededTickEventStateSnapshotCapture, SeededTickSimulationDebugTraceOptions,
    SeededTickSimulationNewsMarkScales, SeededTickSimulationRunConfig,
};
use super::constants::{
    MAX_STOCHASTIC_UPDATES_PER_INTERVAL, ONE_HOUR_NANOS, ONE_SECOND_NANOS,
    SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNKING_POLICY,
    SEEDED_TICK_EVENT_DEBUG_TRACE_METADATA_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_RECORD_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_SAMPLING_POLICY, SEEDED_TICK_EVENT_DEBUG_TRACE_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DETERMINISM_POLICY, SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY,
    SEEDED_TICK_EVENT_ORDERING_POLICY, SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY,
    SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
    SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION, SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY,
    SEEDED_TICK_EVENT_STATE_CARRYOVER_POLICY, SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY,
    SEEDED_TICK_EXOGENOUS_NEWS_SHARED_BASE_SHOCK_CONFIGURATION,
    SEEDED_TICK_EXOGENOUS_NEWS_SHARED_COUPLED_RETURN_JUMP_CONFIGURATION,
    SEEDED_TICK_EXOGENOUS_NEWS_SHARED_GAMMA_CONFIGURATION,
};
use super::error::SeededTickSimulationRunError;

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
        let interval_news_intensity =
            run_config.model_inputs.hourly_factor_contract.nu_h[interval_index].max(0.0);
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
            next_state,
            run_config.model_inputs.kernel_parameters,
            interval_seasonality_factor,
            interval_news_intensity,
            &known_scheduled_news_event_timestamps_utc_nanos
                [interval_known_news_start_position..interval_known_news_end_position],
            run_config.model_inputs.known_news_mark_scales,
            run_config.model_inputs.surprise_news_mark_scales,
            &run_config.model_inputs.calendar_registry,
            run_config
                .model_inputs
                .hourly_factor_contract
                .exchange_code
                .as_str(),
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
    interval_start_state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    interval_seasonality_factor: f64,
    interval_news_intensity: f64,
    interval_known_news_event_timestamps_utc_nanos: &[i64],
    known_news_mark_scales: SeededTickSimulationNewsMarkScales,
    surprise_news_mark_scales: SeededTickSimulationNewsMarkScales,
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    randomness_factory: &F,
) -> Result<Vec<IntervalStochasticUpdate>, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let mut endogenous_side_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount,
        interval_index as u64,
    );
    let mut endogenous_auxiliary_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousSellEventCount,
        interval_index as u64,
    );
    let mut endogenous_wait_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp,
        interval_index as u64,
    );
    let mut endogenous_acceptance_stream = randomness_factory.create_substream(
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
    let interval_known_news_event_count =
        u64::try_from(interval_known_news_event_timestamps_utc_nanos.len()).unwrap_or(u64::MAX);
    let interval_news_arrival_decomposition = derive_interval_news_arrival_decomposition(
        interval_news_intensity,
        interval_duration_in_hours,
        interval_known_news_event_count,
    );
    // Keep component assumptions explicit
    // - known arrivals are fixed UTC anchors from the provided calendar slice
    // - surprise arrivals are sampled independently from a Poisson count using nu_surprise
    // - nu_news follows the documented superposition nu_known + nu_surprise
    let interval_surprise_news_expected_count = interval_news_arrival_decomposition
        .surprise_component_intensity
        * interval_duration_in_hours;
    let interval_surprise_news_count = sample_count_from_expectation(
        interval_surprise_news_expected_count,
        &mut news_count_stream,
    );
    let mut exogenous_interval_updates: Vec<IntervalStochasticUpdate> = Vec::with_capacity(
        (interval_known_news_event_count + interval_surprise_news_count) as usize,
    );
    let mut next_exogenous_generation_order = 0_u64;

    for &known_news_timestamp_utc_nanos in interval_known_news_event_timestamps_utc_nanos {
        let (activity_coupling_mark, return_jump_mark) = sample_exogenous_news_marks(
            &mut news_severity_stream,
            &mut news_return_jump_stream,
            known_news_mark_scales,
        )?;
        let market_state = resolve_news_event_market_state(
            calendar_registry,
            exchange_code,
            known_news_timestamp_utc_nanos,
        )?;
        let (
            return_jump_routing_target,
            open_window_return_jump_increment,
            closed_window_gap_increment,
        ) = resolve_news_return_jump_routing(market_state, return_jump_mark)?;

        exogenous_interval_updates.push(IntervalStochasticUpdate::ExogenousNews {
            timestamp_utc_nanos: known_news_timestamp_utc_nanos,
            news_arrival_component: NewsArrivalComponent::Known,
            activity_coupling_mark,
            return_jump_mark,
            market_state,
            return_jump_routing_target,
            open_window_return_jump_increment,
            closed_window_gap_increment,
            generation_order: next_exogenous_generation_order,
        });
        next_exogenous_generation_order = next_exogenous_generation_order.saturating_add(1);
    }

    for _surprise_news_position in 0..interval_surprise_news_count {
        let timestamp_utc_nanos = sample_interval_timestamp_utc_nanos(
            interval_start_utc_nanos,
            interval_duration_nanos,
            &mut news_timestamp_stream,
        );
        let (activity_coupling_mark, return_jump_mark) = sample_exogenous_news_marks(
            &mut news_severity_stream,
            &mut news_return_jump_stream,
            surprise_news_mark_scales,
        )?;
        let market_state =
            resolve_news_event_market_state(calendar_registry, exchange_code, timestamp_utc_nanos)?;
        let (
            return_jump_routing_target,
            open_window_return_jump_increment,
            closed_window_gap_increment,
        ) = resolve_news_return_jump_routing(market_state, return_jump_mark)?;

        exogenous_interval_updates.push(IntervalStochasticUpdate::ExogenousNews {
            timestamp_utc_nanos,
            news_arrival_component: NewsArrivalComponent::Surprise,
            activity_coupling_mark,
            return_jump_mark,
            market_state,
            return_jump_routing_target,
            open_window_return_jump_increment,
            closed_window_gap_increment,
            generation_order: next_exogenous_generation_order,
        });

        next_exogenous_generation_order = next_exogenous_generation_order.saturating_add(1);
    }

    exogenous_interval_updates.sort_by_key(IntervalStochasticUpdate::ordering_key);

    let _ = endogenous_auxiliary_stream.next_unit_interval_f64();

    let interval_end_utc_nanos = interval_start_utc_nanos + interval_duration_nanos;
    let mut endogenous_interval_updates =
        schedule_endogenous_interval_events_with_event_time_feedback(
            interval_start_state,
            kernel_parameters,
            interval_seasonality_factor,
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            &exogenous_interval_updates,
            &mut endogenous_wait_stream,
            &mut endogenous_acceptance_stream,
            &mut endogenous_side_stream,
        )
        .map_err(map_transition_error_to_seeded_simulation_run_error)?;

    let mut interval_updates: Vec<IntervalStochasticUpdate> = Vec::with_capacity(
        endogenous_interval_updates
            .len()
            .saturating_add(exogenous_interval_updates.len()),
    );
    interval_updates.append(&mut endogenous_interval_updates);
    interval_updates.extend(exogenous_interval_updates);
    interval_updates.sort_by_key(IntervalStochasticUpdate::ordering_key);

    Ok(interval_updates)
}

fn schedule_endogenous_interval_events_with_event_time_feedback<
    WaitStream,
    AcceptanceStream,
    SideStream,
>(
    interval_start_state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    interval_seasonality_factor: f64,
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    exogenous_interval_updates: &[IntervalStochasticUpdate],
    endogenous_wait_stream: &mut WaitStream,
    endogenous_acceptance_stream: &mut AcceptanceStream,
    endogenous_side_stream: &mut SideStream,
) -> Result<Vec<IntervalStochasticUpdate>, HawkesTransitionError>
where
    WaitStream: SimulationRandomnessStream,
    AcceptanceStream: SimulationRandomnessStream,
    SideStream: SimulationRandomnessStream,
{
    let mut endogenous_interval_updates: Vec<IntervalStochasticUpdate> = Vec::new();
    let mut simulated_state = interval_start_state;
    let mut simulated_timestamp_utc_nanos = interval_start_utc_nanos;
    let mut next_exogenous_update_position = 0_usize;
    let baseline_total_intensity =
        (2.0 * interval_seasonality_factor * kernel_parameters.lambda_bar).max(0.0);

    while simulated_timestamp_utc_nanos < interval_end_utc_nanos {
        let next_exogenous_timestamp_utc_nanos = exogenous_interval_updates
            .get(next_exogenous_update_position)
            .map(IntervalStochasticUpdate::timestamp_utc_nanos)
            .unwrap_or(interval_end_utc_nanos);
        let segment_end_utc_nanos = next_exogenous_timestamp_utc_nanos.min(interval_end_utc_nanos);

        while simulated_timestamp_utc_nanos < segment_end_utc_nanos
            && endogenous_interval_updates.len() < MAX_STOCHASTIC_UPDATES_PER_INTERVAL as usize
        {
            let (current_lambda_plus, current_lambda_minus, current_total_intensity) =
                project_non_negative_intensities(
                    simulated_state,
                    kernel_parameters.lambda_bar,
                    interval_seasonality_factor,
                );
            if current_total_intensity <= 0.0 {
                simulated_state = apply_hawkes_drift(
                    simulated_state,
                    kernel_parameters,
                    simulated_timestamp_utc_nanos,
                    segment_end_utc_nanos,
                )?;
                simulated_timestamp_utc_nanos = segment_end_utc_nanos;
                break;
            }

            let proposal_upper_bound_total_intensity =
                current_total_intensity.max(baseline_total_intensity);
            let waiting_time_hours = sample_exponential_wait_time_hours(
                proposal_upper_bound_total_intensity,
                endogenous_wait_stream,
            );
            let waiting_time_nanos = convert_wait_time_hours_to_positive_nanos(waiting_time_hours);
            if waiting_time_nanos == i64::MAX {
                simulated_state = apply_hawkes_drift(
                    simulated_state,
                    kernel_parameters,
                    simulated_timestamp_utc_nanos,
                    segment_end_utc_nanos,
                )?;
                simulated_timestamp_utc_nanos = segment_end_utc_nanos;
                break;
            }

            let candidate_timestamp_utc_nanos =
                simulated_timestamp_utc_nanos.saturating_add(waiting_time_nanos);
            if candidate_timestamp_utc_nanos >= segment_end_utc_nanos {
                simulated_state = apply_hawkes_drift(
                    simulated_state,
                    kernel_parameters,
                    simulated_timestamp_utc_nanos,
                    segment_end_utc_nanos,
                )?;
                simulated_timestamp_utc_nanos = segment_end_utc_nanos;
                break;
            }

            let candidate_state = apply_hawkes_drift(
                simulated_state,
                kernel_parameters,
                simulated_timestamp_utc_nanos,
                candidate_timestamp_utc_nanos,
            )?;
            let (candidate_lambda_plus, candidate_lambda_minus, candidate_total_intensity) =
                project_non_negative_intensities(
                    candidate_state,
                    kernel_parameters.lambda_bar,
                    interval_seasonality_factor,
                );

            let acceptance_probability = if proposal_upper_bound_total_intensity > 0.0 {
                (candidate_total_intensity / proposal_upper_bound_total_intensity).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let acceptance_draw = endogenous_acceptance_stream.next_unit_interval_f64();
            if acceptance_draw <= acceptance_probability {
                let side_draw = endogenous_side_stream.next_unit_interval_f64();
                let side = sample_endogenous_event_side_from_intensities(
                    candidate_lambda_plus,
                    candidate_lambda_minus,
                    side_draw,
                );
                let state_after_event = evolve_hawkes_state(
                    candidate_state,
                    kernel_parameters,
                    HawkesTransitionStep::new(
                        0.0,
                        Some(map_seeded_tick_side_to_hawkes_side(side)),
                        None,
                    ),
                )?;

                endogenous_interval_updates.push(IntervalStochasticUpdate::EndogenousEvent {
                    timestamp_utc_nanos: candidate_timestamp_utc_nanos,
                    side,
                    generation_order: endogenous_interval_updates.len() as u64,
                });
                simulated_state = state_after_event;
            } else {
                simulated_state = candidate_state;
            }

            simulated_timestamp_utc_nanos = candidate_timestamp_utc_nanos;
            let _ = (current_lambda_plus, current_lambda_minus);
        }

        if simulated_timestamp_utc_nanos < segment_end_utc_nanos {
            simulated_state = apply_hawkes_drift(
                simulated_state,
                kernel_parameters,
                simulated_timestamp_utc_nanos,
                segment_end_utc_nanos,
            )?;
            simulated_timestamp_utc_nanos = segment_end_utc_nanos;
        }

        if segment_end_utc_nanos == interval_end_utc_nanos {
            break;
        }

        while next_exogenous_update_position < exogenous_interval_updates.len()
            && exogenous_interval_updates[next_exogenous_update_position].timestamp_utc_nanos()
                == segment_end_utc_nanos
        {
            if let IntervalStochasticUpdate::ExogenousNews {
                activity_coupling_mark,
                ..
            } = exogenous_interval_updates[next_exogenous_update_position]
            {
                simulated_state = evolve_hawkes_state(
                    simulated_state,
                    kernel_parameters,
                    HawkesTransitionStep::new(0.0, None, Some(activity_coupling_mark)),
                )?;
            }
            next_exogenous_update_position = next_exogenous_update_position.saturating_add(1);
        }

        if endogenous_interval_updates.len() >= MAX_STOCHASTIC_UPDATES_PER_INTERVAL as usize {
            break;
        }
    }

    Ok(endogenous_interval_updates)
}

fn apply_hawkes_drift(
    state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    previous_timestamp_utc_nanos: i64,
    timestamp_utc_nanos: i64,
) -> Result<HawkesExcitationState, HawkesTransitionError> {
    let elapsed_hours = derive_elapsed_hours(previous_timestamp_utc_nanos, timestamp_utc_nanos);
    evolve_hawkes_state(
        state,
        kernel_parameters,
        HawkesTransitionStep::new(elapsed_hours, None, None),
    )
}

fn project_non_negative_intensities(
    state: HawkesExcitationState,
    lambda_bar: f64,
    seasonality_factor: f64,
) -> (f64, f64, f64) {
    let projected_intensities = state
        .project_intensities(lambda_bar, seasonality_factor)
        .seasonality_scaled;

    let lambda_plus = projected_intensities.lambda_plus.max(0.0);
    let lambda_minus = projected_intensities.lambda_minus.max(0.0);
    let lambda_plus = if lambda_plus.is_finite() {
        lambda_plus
    } else {
        0.0
    };
    let lambda_minus = if lambda_minus.is_finite() {
        lambda_minus
    } else {
        0.0
    };

    (lambda_plus, lambda_minus, lambda_plus + lambda_minus)
}

fn sample_exponential_wait_time_hours<R>(intensity_per_hour: f64, randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream,
{
    if !intensity_per_hour.is_finite() || intensity_per_hour <= 0.0 {
        return f64::INFINITY;
    }

    let raw_uniform_draw = randomness_stream.next_unit_interval_f64();
    if !raw_uniform_draw.is_finite() || raw_uniform_draw <= 0.0 {
        return f64::INFINITY;
    }

    let clamped_uniform_draw = raw_uniform_draw.min(1.0 - f64::EPSILON);
    -clamped_uniform_draw.ln() / intensity_per_hour
}

fn convert_wait_time_hours_to_positive_nanos(wait_time_hours: f64) -> i64 {
    if !wait_time_hours.is_finite() || wait_time_hours < 0.0 {
        return i64::MAX;
    }

    let wait_time_nanos = (wait_time_hours * ONE_HOUR_NANOS as f64).ceil();
    if !wait_time_nanos.is_finite() || wait_time_nanos >= i64::MAX as f64 {
        return i64::MAX;
    }

    (wait_time_nanos as i64).max(1)
}

fn sample_endogenous_event_side_from_intensities(
    lambda_plus: f64,
    lambda_minus: f64,
    side_draw: f64,
) -> SeededTickEventSide {
    let total_intensity = lambda_plus + lambda_minus;
    if total_intensity <= 0.0 {
        return SeededTickEventSide::Buy;
    }

    let buy_probability = (lambda_plus / total_intensity).clamp(0.0, 1.0);
    if side_draw < buy_probability {
        SeededTickEventSide::Buy
    } else {
        SeededTickEventSide::Sell
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct IntervalNewsArrivalDecomposition {
    pub(crate) known_component_intensity: f64,
    pub(crate) surprise_component_intensity: f64,
}

pub(crate) fn derive_interval_news_arrival_decomposition(
    interval_news_intensity: f64,
    interval_duration_in_hours: f64,
    interval_known_news_event_count: u64,
) -> IntervalNewsArrivalDecomposition {
    let interval_known_component_intensity = if interval_duration_in_hours > 0.0 {
        interval_known_news_event_count as f64 / interval_duration_in_hours
    } else {
        0.0
    };
    let interval_surprise_component_intensity =
        (interval_news_intensity - interval_known_component_intensity).max(0.0);

    IntervalNewsArrivalDecomposition {
        known_component_intensity: interval_known_component_intensity,
        surprise_component_intensity: interval_surprise_component_intensity,
    }
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

fn resolve_news_event_market_state(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    timestamp_utc_nanos: i64,
) -> Result<MarketState, SeededTickSimulationRunError> {
    let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
    let nanoseconds = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;
    let timestamp_utc = DateTime::<Utc>::from_timestamp(seconds, nanoseconds).ok_or(
        SeededTickSimulationRunError::InvalidKnownScheduledNewsTimestampUtcNanos {
            value: timestamp_utc_nanos,
        },
    )?;

    calendar_registry
        .resolve_market_state_at_utc(exchange_code, timestamp_utc)
        .map_err(
            |source| SeededTickSimulationRunError::NewsMarketStateResolutionFailed {
                exchange_code: exchange_code.to_string(),
                timestamp_utc_nanos,
                source,
            },
        )
}

pub(crate) fn derive_known_news_interval_slice_bounds(
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

pub(crate) fn validate_known_scheduled_news_timestamps(
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

pub(crate) fn sample_interval_timestamp_utc_nanos<R>(
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

pub(crate) fn sample_count_from_expectation<R>(
    expected_count: f64,
    randomness_stream: &mut R,
) -> u64
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

pub(crate) fn sample_exogenous_news_marks<SeverityStream, ReturnJumpStream>(
    severity_randomness_stream: &mut SeverityStream,
    return_jump_randomness_stream: &mut ReturnJumpStream,
    mark_scales: SeededTickSimulationNewsMarkScales,
) -> Result<(f64, f64), SeededTickSimulationRunError>
where
    SeverityStream: SimulationRandomnessStream,
    ReturnJumpStream: SimulationRandomnessStream,
{
    let shared_severity_mark = SEEDED_TICK_EXOGENOUS_NEWS_SHARED_GAMMA_CONFIGURATION
        .sample_severity_mark(severity_randomness_stream)
        .map_err(map_gamma_severity_sampling_error_to_seeded_simulation_run_error)?;
    let shared_base_shock_mark = SEEDED_TICK_EXOGENOUS_NEWS_SHARED_BASE_SHOCK_CONFIGURATION
        .sample_base_shock_mark(return_jump_randomness_stream)
        .map_err(map_base_shock_sampling_error_to_seeded_simulation_run_error)?;
    let shared_coupled_return_jump_mark =
        SEEDED_TICK_EXOGENOUS_NEWS_SHARED_COUPLED_RETURN_JUMP_CONFIGURATION
            .construct_gap_jump_mark(shared_severity_mark, shared_base_shock_mark)
            .map_err(map_coupled_mark_construction_error_to_seeded_simulation_run_error)?
            .gap_jump_mark;

    let activity_coupling_mark = mark_scales.activity_coupling_scale * shared_severity_mark;
    if !activity_coupling_mark.is_finite() || activity_coupling_mark < 0.0 {
        return Err(
            SeededTickSimulationRunError::InvalidGeneratedNewsActivityCouplingMark {
                value: activity_coupling_mark,
            },
        );
    }

    let return_jump_mark = mark_scales.return_jump_scale * shared_coupled_return_jump_mark;
    if !return_jump_mark.is_finite() {
        return Err(
            SeededTickSimulationRunError::InvalidGeneratedNewsReturnJumpMark {
                value: return_jump_mark,
            },
        );
    }

    Ok((activity_coupling_mark, return_jump_mark))
}

fn map_gamma_severity_sampling_error_to_seeded_simulation_run_error(
    source: GammaSeverityMarkSamplingError,
) -> SeededTickSimulationRunError {
    match source {
        GammaSeverityMarkSamplingError::InvalidConfiguration { .. } => {
            SeededTickSimulationRunError::InvalidGeneratedNewsActivityCouplingMark {
                value: f64::NAN,
            }
        }
        GammaSeverityMarkSamplingError::InvalidSampledMark { value } => {
            SeededTickSimulationRunError::InvalidGeneratedNewsActivityCouplingMark { value }
        }
    }
}

fn map_base_shock_sampling_error_to_seeded_simulation_run_error(
    source: DoubleExponentialBaseShockSamplingError,
) -> SeededTickSimulationRunError {
    match source {
        DoubleExponentialBaseShockSamplingError::InvalidConfiguration { .. } => {
            SeededTickSimulationRunError::InvalidGeneratedNewsReturnJumpMark { value: f64::NAN }
        }
        DoubleExponentialBaseShockSamplingError::InvalidSampledMark { value } => {
            SeededTickSimulationRunError::InvalidGeneratedNewsReturnJumpMark { value }
        }
    }
}

fn map_coupled_mark_construction_error_to_seeded_simulation_run_error(
    source: CoupledGapMarkConstructionError,
) -> SeededTickSimulationRunError {
    match source {
        CoupledGapMarkConstructionError::InvalidConfiguration { .. } => {
            SeededTickSimulationRunError::InvalidGeneratedNewsReturnJumpMark { value: f64::NAN }
        }
        CoupledGapMarkConstructionError::InvalidSharedSeverityMark { value } => {
            SeededTickSimulationRunError::InvalidGeneratedNewsActivityCouplingMark { value }
        }
        CoupledGapMarkConstructionError::InvalidSharedBaseShockMark { value } => {
            SeededTickSimulationRunError::InvalidGeneratedNewsReturnJumpMark { value }
        }
        CoupledGapMarkConstructionError::InvalidConstructedGapMark { value } => {
            SeededTickSimulationRunError::InvalidGeneratedNewsReturnJumpMark { value }
        }
    }
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

pub(crate) fn validate_news_return_jump_routing_state(
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

pub(crate) fn validate_hourly_factor_field_length(
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

pub(crate) fn validate_hourly_factor_interval_coverage(
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
