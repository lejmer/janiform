use janiform_model_kernels::hawkes::HawkesExcitationState;

use crate::artifacts::{
    SeededTickEventDebugTraceChunk, SeededTickEventDebugTraceOutput,
    SeededTickEventDebugTraceRecord, SeededTickEventDebugTraceTransitionKind,
    SeededTickEventStateSnapshot, SeededTickSimulationNewsEventMarketState,
    SeededTickSimulationNewsRoutingTotals,
};
use crate::config::SeededTickSimulationDebugTraceOptions;
use crate::constants::{
    SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_SCHEMA_VERSION,
};
use crate::error::SeededTickSimulationRunError;

use super::buffers::try_reserve_simulation_buffer;
use super::metadata::build_seeded_tick_event_debug_trace_metadata;
use super::updates::SeededTickSimulationTraceNewsTransitionPayload;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SeededTickEventDebugTraceCaptureInput {
    pub(crate) transition_index: u64,
    pub(crate) interval_index: usize,
    pub(crate) previous_timestamp_utc_nanos: i64,
    pub(crate) timestamp_utc_nanos: i64,
    pub(crate) elapsed_hours_since_previous_update: f64,
    pub(crate) transition_kind: SeededTickEventDebugTraceTransitionKind,
    pub(crate) generation_order: Option<u64>,
    pub(crate) event_index: Option<u64>,
    pub(crate) news_transition_payload: Option<SeededTickSimulationTraceNewsTransitionPayload>,
    pub(crate) news_routing_totals: SeededTickSimulationNewsRoutingTotals,
    pub(crate) seasonality_factor: f64,
    pub(crate) interval_news_intensity: f64,
    pub(crate) state_before_transition: HawkesExcitationState,
    pub(crate) state_after_transition: HawkesExcitationState,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SeededTickEventDebugTraceCollector {
    lambda_bar: f64,
    options: SeededTickSimulationDebugTraceOptions,
    trace_chunks: Vec<SeededTickEventDebugTraceChunk>,
    pending_chunk_records: Vec<SeededTickEventDebugTraceRecord>,
}

impl SeededTickEventDebugTraceCollector {
    pub(crate) fn new(
        options: SeededTickSimulationDebugTraceOptions,
        lambda_bar: f64,
    ) -> Result<Self, SeededTickSimulationRunError> {
        Ok(Self {
            lambda_bar,
            options,
            trace_chunks: Vec::new(),
            pending_chunk_records: try_reserve_simulation_buffer(
                "debug_trace_pending_chunk_records",
                options.max_records_per_chunk,
            )?,
        })
    }

    pub(crate) fn capture_transition(&mut self, input: SeededTickEventDebugTraceCaptureInput) {
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

    pub(crate) fn finish(mut self) -> SeededTickEventDebugTraceOutput {
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

        transition_index.is_multiple_of(self.options.sample_every_nth_transition)
    }

    fn flush_pending_chunk(&mut self) {
        if self.pending_chunk_records.is_empty() {
            return;
        }

        let Some(first_record) = self.pending_chunk_records.first() else {
            return;
        };
        let Some(last_record) = self.pending_chunk_records.last() else {
            return;
        };

        let start_transition_index = first_record.transition_index;
        let end_transition_index = last_record.transition_index;

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
