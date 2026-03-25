//! Seeded simulation runtime and transition scheduling.

use chrono::{DateTime, Datelike, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use std::cmp::Ordering;
use std::str::FromStr;

use crate::calendar::{
    CalendarDate, ExchangeCalendarDefinition, ExchangeCalendarRegistry, MarketState, SessionTime,
    TradingWeekday,
};
use crate::risk::SufficientStatisticsRiskReturnComponents;
use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessStream, SimulationRandomnessSubstreamDomain,
    SplitMix64SimulationRandomnessFactory,
};
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelParameters, HawkesTransitionError,
    HawkesTransitionStep, evolve_hawkes_state,
};
use janiform_model_kernels::news::{
    NewsArrivalComponent, NewsComponentMarkScaleConfiguration, NewsReturnJumpRoutingTarget,
    SharedNewsAccumulatorState, SharedNewsEventMarketStateResolutionError,
    SharedNewsEventMarketStateResolver, SharedNewsEventTimestampedInput,
    dispatch_shared_news_events_with_resolved_market_states,
};
use janiform_model_kernels::return_decomposition::ReturnDecompositionValidationError;

use super::artifacts::{
    SeededFinePathMetadata, SeededFinePathRecord, SeededFinePathTransitionKind,
    SeededTickEventDebugTraceChunk, SeededTickEventDebugTraceMetadata,
    SeededTickEventDebugTraceOutput, SeededTickEventDebugTraceRecord,
    SeededTickEventDebugTraceTransitionKind, SeededTickEventSide, SeededTickEventStateSnapshot,
    SeededTickEventTapeMetadata, SeededTickSimulationNewsEventMarketState,
    SeededTickSimulationNewsRoutingTotals, SeededTickSimulationRunResult,
};
use super::config::{
    SeededTickEventStateSnapshotCapture, SeededTickSimulationDebugTraceOptions,
    SeededTickSimulationRunConfig,
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

    let driver_parameters =
        run_config
            .model_inputs
            .nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .map_err(|source| {
                SeededTickSimulationRunError::InvalidNigSharedPlusShiftConfiguration { source }
            })?;

    let fine_path_metadata = build_seeded_fine_path_metadata(state_snapshot_capture);
    let event_tape_metadata = build_seeded_tick_event_tape_metadata(state_snapshot_capture);

    let market_state_resolver = SeededSimulationMarketStateResolver::new(
        &run_config.model_inputs.calendar_registry,
        run_config
            .model_inputs
            .hourly_factor_contract
            .exchange_code
            .as_str(),
    )?;
    let interval_open_market_segments = build_interval_open_market_segments(
        &market_state_resolver,
        &run_config
            .model_inputs
            .hourly_factor_contract
            .interval_start_utc_nanos[..run_config.horizon_interval_count],
        &run_config
            .model_inputs
            .hourly_factor_contract
            .interval_end_utc_nanos[..run_config.horizon_interval_count],
    )?;
    let (estimated_transition_capacity, estimated_event_capacity) =
        estimate_seeded_result_capacities(&run_config);

    let mut fine_path: Vec<SeededFinePathRecord> =
        Vec::with_capacity(estimated_transition_capacity);
    let mut public_transition_positions: Vec<usize> =
        Vec::with_capacity(estimated_transition_capacity);
    let mut event_transition_positions: Vec<usize> = Vec::with_capacity(estimated_event_capacity);

    let mut next_event_index: u64 = 0;
    let mut next_transition_index: u64 = 0;
    let mut next_state = run_config.model_inputs.initial_excitation_state;
    let mut news_routing_totals = SeededTickSimulationNewsRoutingTotals::default();

    let known_scheduled_news_event_timestamps_utc_nanos = &run_config
        .model_inputs
        .news_process_configuration
        .arrivals
        .known
        .scheduled_event_timestamps_utc_nanos;

    let mut next_known_scheduled_news_timestamp_position = 0usize;

    let mut pending_open_jump_return = 0.0_f64;
    let mut pending_closed_gap_return = 0.0_f64;

    let mean_reversion_kappa = run_config
        .model_inputs
        .mean_reversion_fitted_parameters
        .kappa;

    let mut observed_state = SeededObservedState::from_boundary_state(
        &run_config
            .model_inputs
            .initial_mean_reversion_boundary_state,
    );

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
        let interval_end_anchor_value = run_config
            .model_inputs
            .mean_reversion_runtime_interval_anchors
            .end_anchor_values[interval_index];

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
            interval_index,
            interval_start_utc_nanos,
            interval_duration_nanos,
            interval_duration_in_hours,
            &run_config,
            &known_scheduled_news_event_timestamps_utc_nanos
                [interval_known_news_start_position..interval_known_news_end_position],
            next_state,
            run_config.model_inputs.kernel_parameters,
            interval_seasonality_factor,
            interval_news_intensity,
            &market_state_resolver,
            randomness_factory,
        )?;

        let mut previous_timestamp_utc_nanos = interval_start_utc_nanos;
        for interval_update in interval_updates {
            let elapsed_hours_since_previous_update = derive_elapsed_hours(
                previous_timestamp_utc_nanos,
                interval_update.timestamp_utc_nanos(),
            );

            let current_market_state = market_state_resolver
                .resolve_market_state_at_nanos(interval_update.timestamp_utc_nanos())?;

            let open_jump_return = if current_market_state == MarketState::Open {
                let realized = pending_open_jump_return;
                pending_open_jump_return = 0.0;
                realized
            } else {
                0.0
            };

            let closed_gap_return = if current_market_state == MarketState::Open {
                let realized = pending_closed_gap_return;
                pending_closed_gap_return = 0.0;
                realized
            } else {
                0.0
            };

            let trading_time_return = sample_transition_trading_time_return(
                driver_parameters,
                next_state,
                lambda_bar,
                run_config.model_inputs.kernel_parameters.beta,
                interval_seasonality_factor,
                interval_open_market_segments[interval_index].as_slice(),
                previous_timestamp_utc_nanos,
                interval_update.timestamp_utc_nanos(),
                interval_index,
                randomness_factory.create_substream(
                    run_config.seed,
                    SimulationRandomnessSubstreamDomain::CoarseTradingTimeReturn,
                    next_transition_index,
                ),
            )?;

            let total_return =
                trading_time_return.trading_time_return + open_jump_return + closed_gap_return;

            let return_components = SufficientStatisticsRiskReturnComponents::try_new(
                trading_time_return.buy_side_return,
                trading_time_return.sell_side_return,
                trading_time_return.trading_time_return,
                open_jump_return,
                closed_gap_return,
                total_return,
            )
            .map_err(|source| {
                SeededTickSimulationRunError::InvalidReturnComponents {
                    interval_index,
                    source,
                }
            })?;

            observed_state = observed_state.apply_core_increment(
                mean_reversion_kappa,
                elapsed_hours_since_previous_update,
                total_return,
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

            let state_snapshot =
                build_event_state_snapshot(next_state, lambda_bar, state_snapshot_capture);

            let (transition_kind, side, news_arrival_component) = match interval_update {
                IntervalStochasticUpdate::EndogenousEvent {
                    timestamp_utc_nanos,
                    side,
                    ..
                } => {
                    event_transition_positions.push(fine_path.len());
                    next_event_index = next_event_index.saturating_add(1);
                    previous_timestamp_utc_nanos = timestamp_utc_nanos;
                    (map_side_to_transition_kind(side), Some(side), None)
                }
                IntervalStochasticUpdate::ExogenousNews {
                    timestamp_utc_nanos,
                    news_arrival_component,
                    open_window_return_jump_increment,
                    closed_window_gap_increment,
                    ..
                } => {
                    pending_open_jump_return += open_window_return_jump_increment;
                    pending_closed_gap_return += closed_window_gap_increment;
                    previous_timestamp_utc_nanos = timestamp_utc_nanos;
                    (
                        SeededFinePathTransitionKind::ExogenousNews,
                        None,
                        Some(news_arrival_component),
                    )
                }
            };

            let fine_path_record = SeededFinePathRecord {
                transition_index: next_transition_index - 1,
                timestamp_utc_nanos: interval_update.timestamp_utc_nanos(),
                transition_kind,
                side,
                news_arrival_component,
                market_state: SeededTickSimulationNewsEventMarketState::from_market_state(
                    current_market_state,
                ),
                return_components,
                core_total_return_increment: total_return,
                observed_log_price: observed_state.observed_log_price,
                anchor_value: observed_state.anchor_value,
                deviation_from_anchor: observed_state.deviation_from_anchor,
                cumulative_core_log_return: observed_state.cumulative_core_log_return,
                state_snapshot,
            };

            if is_public_seeded_fine_path_record(&fine_path_record) {
                public_transition_positions.push(fine_path.len());
            }

            fine_path.push(fine_path_record);
        }

        let elapsed_hours_to_interval_end =
            derive_elapsed_hours(previous_timestamp_utc_nanos, interval_end_utc_nanos);

        let previous_state = next_state;
        let boundary_market_state =
            market_state_resolver.resolve_market_state_at_nanos(interval_end_utc_nanos)?;

        let open_jump_return = if boundary_market_state == MarketState::Open {
            let realized = pending_open_jump_return;
            pending_open_jump_return = 0.0;
            realized
        } else {
            0.0
        };

        let closed_gap_return = if boundary_market_state == MarketState::Open {
            let realized = pending_closed_gap_return;
            pending_closed_gap_return = 0.0;
            realized
        } else {
            0.0
        };

        let trading_time_return = sample_transition_trading_time_return(
            driver_parameters,
            previous_state,
            lambda_bar,
            run_config.model_inputs.kernel_parameters.beta,
            interval_seasonality_factor,
            interval_open_market_segments[interval_index].as_slice(),
            previous_timestamp_utc_nanos,
            interval_end_utc_nanos,
            interval_index,
            randomness_factory.create_substream(
                run_config.seed,
                SimulationRandomnessSubstreamDomain::CoarseTradingTimeReturn,
                next_transition_index,
            ),
        )?;

        let total_return =
            trading_time_return.trading_time_return + open_jump_return + closed_gap_return;

        let return_components = SufficientStatisticsRiskReturnComponents::try_new(
            trading_time_return.buy_side_return,
            trading_time_return.sell_side_return,
            trading_time_return.trading_time_return,
            open_jump_return,
            closed_gap_return,
            total_return,
        )
        .map_err(
            |source| SeededTickSimulationRunError::InvalidReturnComponents {
                interval_index,
                source,
            },
        )?;

        observed_state = observed_state
            .apply_core_increment(
                mean_reversion_kappa,
                elapsed_hours_to_interval_end,
                total_return,
            )
            .apply_anchor_reset(interval_end_anchor_value);

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

        let fine_path_record = SeededFinePathRecord {
            transition_index: next_transition_index,
            timestamp_utc_nanos: interval_end_utc_nanos,
            transition_kind: SeededFinePathTransitionKind::IntervalBoundaryDrift,
            side: None,
            news_arrival_component: None,
            market_state: SeededTickSimulationNewsEventMarketState::from_market_state(
                boundary_market_state,
            ),
            return_components,
            core_total_return_increment: total_return,
            observed_log_price: observed_state.observed_log_price,
            anchor_value: observed_state.anchor_value,
            deviation_from_anchor: observed_state.deviation_from_anchor,
            cumulative_core_log_return: observed_state.cumulative_core_log_return,
            state_snapshot: build_event_state_snapshot(
                next_state,
                lambda_bar,
                state_snapshot_capture,
            ),
        };

        public_transition_positions.push(fine_path.len());
        fine_path.push(fine_path_record);

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
        fine_path_metadata,
        fine_path,
        public_transition_positions,
        event_tape_metadata,
        event_transition_positions,
        final_excitation_state: next_state,
        news_routing_totals,
        cumulative_core_log_return: observed_state.cumulative_core_log_return,
        debug_trace_enabled: run_config.options.debug_trace_enabled,
        debug_trace_output: debug_trace_collector.map(SeededTickEventDebugTraceCollector::finish),
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TradingTimeReturnComponents {
    buy_side_return: f64,
    sell_side_return: f64,
    trading_time_return: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct OpenMarketElapsedTimeWeights {
    open_elapsed_hours: f64,
    open_decay_weight: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SeededObservedState {
    cumulative_core_log_return: f64,
    observed_log_price: f64,
    anchor_value: f64,
    deviation_from_anchor: f64,
}

impl SeededObservedState {
    fn from_boundary_state(
        boundary_state: &janiform_model_kernels::mean_reversion::MeanReversionBoundaryState,
    ) -> Self {
        Self {
            cumulative_core_log_return: 0.0,
            observed_log_price: boundary_state.observed_log_price,
            anchor_value: boundary_state.anchor_value,
            deviation_from_anchor: boundary_state.deviation,
        }
    }

    fn apply_core_increment(
        self,
        kappa: f64,
        elapsed_hours_since_previous_update: f64,
        core_total_return_increment: f64,
    ) -> Self {
        let deviation_from_anchor = (-kappa * elapsed_hours_since_previous_update).exp()
            * self.deviation_from_anchor
            + core_total_return_increment;

        Self {
            cumulative_core_log_return: self.cumulative_core_log_return
                + core_total_return_increment,
            observed_log_price: self.anchor_value + deviation_from_anchor,
            anchor_value: self.anchor_value,
            deviation_from_anchor,
        }
    }

    fn apply_anchor_reset(self, next_anchor_value: f64) -> Self {
        let deviation_from_anchor =
            self.deviation_from_anchor + self.anchor_value - next_anchor_value;

        Self {
            cumulative_core_log_return: self.cumulative_core_log_return,
            observed_log_price: next_anchor_value + deviation_from_anchor,
            anchor_value: next_anchor_value,
            deviation_from_anchor,
        }
    }
}

type IntervalOpenMarketSegments = Vec<(i64, i64)>;

fn sample_transition_trading_time_return<R>(
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    state: HawkesExcitationState,
    lambda_bar: f64,
    beta: f64,
    seasonality_factor: f64,
    interval_open_market_segments: &[(i64, i64)],
    previous_timestamp_utc_nanos: i64,
    timestamp_utc_nanos: i64,
    interval_index: usize,
    mut randomness_stream: R,
) -> Result<TradingTimeReturnComponents, SeededTickSimulationRunError>
where
    R: SimulationRandomnessStream,
{
    let open_market_elapsed_time_weights = integrate_open_market_elapsed_time_weights(
        interval_open_market_segments,
        previous_timestamp_utc_nanos,
        timestamp_utc_nanos,
        beta,
    );
    let integrated_buy_intensity = integrate_side_intensity(
        state,
        lambda_bar,
        seasonality_factor,
        open_market_elapsed_time_weights,
        true,
    );
    let integrated_sell_intensity = integrate_side_intensity(
        state,
        lambda_bar,
        seasonality_factor,
        open_market_elapsed_time_weights,
        false,
    );

    validate_integrated_intensity("a_plus", integrated_buy_intensity, interval_index)?;
    validate_integrated_intensity("a_minus", integrated_sell_intensity, interval_index)?;

    let trading_time_contributions = driver_parameters
        .sample_trading_time_return_increment(
            integrated_buy_intensity,
            integrated_sell_intensity,
            &mut randomness_stream,
        )
        .map_err(
            |source| SeededTickSimulationRunError::TradingTimeSamplingFailure {
                interval_index,
                source,
            },
        )?;

    Ok(TradingTimeReturnComponents {
        buy_side_return: trading_time_contributions.buy_side_increment,
        sell_side_return: -trading_time_contributions.sell_side_increment,
        trading_time_return: trading_time_contributions.combined_increment,
    })
}

fn validate_integrated_intensity(
    field: &'static str,
    value: f64,
    interval_index: usize,
) -> Result<(), SeededTickSimulationRunError> {
    if !value.is_finite() || value < 0.0 {
        return Err(SeededTickSimulationRunError::InvalidReturnDecomposition {
            interval_index,
            source: ReturnDecompositionValidationError::DomainViolation {
                field,
                constraint:
                    janiform_model_kernels::return_decomposition::ReturnDecompositionConstraint::FiniteAndNonNegative,
                value,
            },
        });
    }

    Ok(())
}

fn integrate_side_intensity(
    state: HawkesExcitationState,
    lambda_bar: f64,
    seasonality_factor: f64,
    open_market_elapsed_time_weights: OpenMarketElapsedTimeWeights,
    plus_side: bool,
) -> f64 {
    if open_market_elapsed_time_weights.open_elapsed_hours <= 0.0 || seasonality_factor <= 0.0 {
        return 0.0;
    }

    let deseasonalized_baseline = lambda_bar * open_market_elapsed_time_weights.open_elapsed_hours;

    let endogenous_component = if plus_side {
        (state.y_plus_plus + state.y_plus_minus)
            * open_market_elapsed_time_weights.open_decay_weight
    } else {
        (state.y_minus_plus + state.y_minus_minus)
            * open_market_elapsed_time_weights.open_decay_weight
    };

    seasonality_factor * (deseasonalized_baseline + endogenous_component)
}

fn integrate_open_market_elapsed_time_weights(
    interval_open_market_segments: &[(i64, i64)],
    previous_timestamp_utc_nanos: i64,
    timestamp_utc_nanos: i64,
    beta: f64,
) -> OpenMarketElapsedTimeWeights {
    if timestamp_utc_nanos <= previous_timestamp_utc_nanos {
        return OpenMarketElapsedTimeWeights {
            open_elapsed_hours: 0.0,
            open_decay_weight: 0.0,
        };
    }

    let mut open_elapsed_hours = 0.0;
    let mut open_decay_weight = 0.0;

    for (open_timestamp_utc_nanos, close_timestamp_utc_nanos) in interval_open_market_segments {
        let overlap_start_utc_nanos = previous_timestamp_utc_nanos.max(*open_timestamp_utc_nanos);
        let overlap_end_utc_nanos = timestamp_utc_nanos.min(*close_timestamp_utc_nanos);
        if overlap_end_utc_nanos <= overlap_start_utc_nanos {
            continue;
        }

        let overlap_start_hours =
            derive_elapsed_hours(previous_timestamp_utc_nanos, overlap_start_utc_nanos);
        let overlap_end_hours =
            derive_elapsed_hours(previous_timestamp_utc_nanos, overlap_end_utc_nanos);

        let overlap_open_hours = overlap_end_hours - overlap_start_hours;

        open_elapsed_hours += overlap_open_hours;
        open_decay_weight += if beta > 0.0 {
            ((-beta * overlap_start_hours).exp() - (-beta * overlap_end_hours).exp()) / beta
        } else {
            overlap_open_hours
        };
    }

    OpenMarketElapsedTimeWeights {
        open_elapsed_hours,
        open_decay_weight,
    }
}

fn build_interval_open_market_segments(
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    interval_start_utc_nanos: &[i64],
    interval_end_utc_nanos: &[i64],
) -> Result<Vec<IntervalOpenMarketSegments>, SeededTickSimulationRunError> {
    interval_start_utc_nanos
        .iter()
        .copied()
        .zip(interval_end_utc_nanos.iter().copied())
        .map(|(interval_start_utc_nanos, interval_end_utc_nanos)| {
            interval_open_market_segments_for_range(
                market_state_resolver,
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            )
        })
        .collect()
}

fn interval_open_market_segments_for_range(
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> Result<IntervalOpenMarketSegments, SeededTickSimulationRunError> {
    if interval_end_utc_nanos <= interval_start_utc_nanos {
        return Ok(Vec::new());
    }

    let start_timestamp_utc = timestamp_utc_from_nanos(interval_start_utc_nanos).ok_or(
        SeededTickSimulationRunError::InvalidKnownScheduledNewsTimestampUtcNanos {
            value: interval_start_utc_nanos,
        },
    )?;
    let end_timestamp_utc = timestamp_utc_from_nanos(interval_end_utc_nanos - 1).ok_or(
        SeededTickSimulationRunError::InvalidKnownScheduledNewsTimestampUtcNanos {
            value: interval_end_utc_nanos - 1,
        },
    )?;

    let start_local_date = CalendarDate::from_local_timestamp(
        &start_timestamp_utc.with_timezone(&market_state_resolver.timezone),
    );
    let end_local_date = CalendarDate::from_local_timestamp(
        &end_timestamp_utc.with_timezone(&market_state_resolver.timezone),
    );

    let mut current_date = NaiveDate::from_ymd_opt(
        start_local_date.year,
        start_local_date.month as u32,
        start_local_date.day as u32,
    )
    .ok_or(
        SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
            exchange_code: market_state_resolver.exchange_code.to_string(),
            timezone_name: market_state_resolver.timezone.name().to_string(),
            local_date: start_local_date.to_string(),
            local_time: "00:00".to_string(),
        },
    )?;

    let last_date = NaiveDate::from_ymd_opt(
        end_local_date.year,
        end_local_date.month as u32,
        end_local_date.day as u32,
    )
    .ok_or(
        SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
            exchange_code: market_state_resolver.exchange_code.to_string(),
            timezone_name: market_state_resolver.timezone.name().to_string(),
            local_date: end_local_date.to_string(),
            local_time: "00:00".to_string(),
        },
    )?;

    let mut open_market_segments = Vec::new();

    while current_date <= last_date {
        let local_date = CalendarDate {
            year: current_date.year(),
            month: current_date.month() as u8,
            day: current_date.day() as u8,
        };
        let local_weekday = TradingWeekday::from_chrono_weekday(current_date.weekday());

        for session_window in market_state_resolver
            .definition
            .session_windows_for_local_date(local_date, local_weekday)
        {
            let open_timestamp_utc_nanos = session_boundary_timestamp_utc_nanos(
                market_state_resolver,
                local_date,
                session_window.open_time(),
                false,
            )?;
            let close_timestamp_utc_nanos = session_boundary_timestamp_utc_nanos(
                market_state_resolver,
                local_date,
                session_window.close_time(),
                true,
            )?;

            let overlap_start_utc_nanos = interval_start_utc_nanos.max(open_timestamp_utc_nanos);
            let overlap_end_utc_nanos = interval_end_utc_nanos.min(close_timestamp_utc_nanos);
            if overlap_end_utc_nanos > overlap_start_utc_nanos {
                open_market_segments.push((overlap_start_utc_nanos, overlap_end_utc_nanos));
            }
        }

        current_date = current_date.succ_opt().ok_or(
            SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
                exchange_code: market_state_resolver.exchange_code.to_string(),
                timezone_name: market_state_resolver.timezone.name().to_string(),
                local_date: local_date.to_string(),
                local_time: "24:00".to_string(),
            },
        )?;
    }

    Ok(open_market_segments)
}

fn session_boundary_timestamp_utc_nanos(
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    local_date: CalendarDate,
    session_time: SessionTime,
    prefer_latest_ambiguous_timestamp: bool,
) -> Result<i64, SeededTickSimulationRunError> {
    let session_boundary_local = local_datetime_for_session_boundary(
        market_state_resolver,
        local_date,
        session_time,
        prefer_latest_ambiguous_timestamp,
    )?;

    session_boundary_local
        .with_timezone(&Utc)
        .timestamp_nanos_opt()
        .ok_or(
            SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
                exchange_code: market_state_resolver.exchange_code.to_string(),
                timezone_name: market_state_resolver.timezone.name().to_string(),
                local_date: local_date.to_string(),
                local_time: session_time.to_string(),
            },
        )
}

fn local_datetime_for_session_boundary(
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    local_date: CalendarDate,
    session_time: SessionTime,
    prefer_latest_ambiguous_timestamp: bool,
) -> Result<DateTime<Tz>, SeededTickSimulationRunError> {
    let (resolved_year, resolved_month, resolved_day, resolved_hour, resolved_minute) =
        if session_time.hour() == 24 && session_time.minute() == 0 {
            let next_day = NaiveDate::from_ymd_opt(
                local_date.year,
                local_date.month as u32,
                local_date.day as u32,
            )
            .and_then(|date| date.succ_opt())
            .ok_or(
                SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
                    exchange_code: market_state_resolver.exchange_code.to_string(),
                    timezone_name: market_state_resolver.timezone.name().to_string(),
                    local_date: local_date.to_string(),
                    local_time: session_time.to_string(),
                },
            )?;

            (
                next_day.year(),
                next_day.month(),
                next_day.day(),
                0_u32,
                0_u32,
            )
        } else {
            (
                local_date.year,
                local_date.month as u32,
                local_date.day as u32,
                session_time.hour() as u32,
                session_time.minute() as u32,
            )
        };

    match market_state_resolver.timezone.with_ymd_and_hms(
        resolved_year,
        resolved_month,
        resolved_day,
        resolved_hour,
        resolved_minute,
        0,
    ) {
        LocalResult::Single(timestamp_local) => Ok(timestamp_local),
        LocalResult::Ambiguous(earliest_timestamp_local, latest_timestamp_local) => {
            if prefer_latest_ambiguous_timestamp {
                Ok(latest_timestamp_local)
            } else {
                Ok(earliest_timestamp_local)
            }
        }
        LocalResult::None => Err(
            SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
                exchange_code: market_state_resolver.exchange_code.to_string(),
                timezone_name: market_state_resolver.timezone.name().to_string(),
                local_date: local_date.to_string(),
                local_time: session_time.to_string(),
            },
        ),
    }
}

fn timestamp_utc_from_nanos(timestamp_utc_nanos: i64) -> Option<DateTime<Utc>> {
    let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
    let nanoseconds = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;

    DateTime::<Utc>::from_timestamp(seconds, nanoseconds)
}

fn map_side_to_transition_kind(side: SeededTickEventSide) -> SeededFinePathTransitionKind {
    match side {
        SeededTickEventSide::Buy => SeededFinePathTransitionKind::BuyTrade,
        SeededTickEventSide::Sell => SeededFinePathTransitionKind::SellTrade,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum IntervalStochasticUpdate {
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

fn is_public_seeded_fine_path_record(record: &SeededFinePathRecord) -> bool {
    !matches!(
        (record.transition_kind, record.market_state),
        (
            SeededFinePathTransitionKind::ExogenousNews,
            SeededTickSimulationNewsEventMarketState::Closed,
        )
    )
}

fn estimate_seeded_result_capacities(run_config: &SeededTickSimulationRunConfig) -> (usize, usize) {
    let known_news_count = run_config
        .model_inputs
        .news_process_configuration
        .arrivals
        .known
        .scheduled_event_timestamps_utc_nanos
        .len();

    let base_transition_count = run_config.horizon_interval_count;

    let mut estimated_endogenous_event_count = 0_usize;
    let mut estimated_surprise_news_count = 0_usize;

    for interval_index in 0..run_config.horizon_interval_count {
        let interval_start_utc_nanos = run_config
            .model_inputs
            .hourly_factor_contract
            .interval_start_utc_nanos[interval_index];
        let interval_end_utc_nanos = run_config
            .model_inputs
            .hourly_factor_contract
            .interval_end_utc_nanos[interval_index];
        let interval_duration_in_hours =
            (interval_end_utc_nanos - interval_start_utc_nanos) as f64 / ONE_HOUR_NANOS as f64;

        let seasonality_factor = run_config.model_inputs.hourly_factor_contract.s_h[interval_index];
        let interval_news_intensity =
            run_config.model_inputs.hourly_factor_contract.nu_h[interval_index].max(0.0);

        let endogenous_baseline = (2.0
            * run_config
                .model_inputs
                .kernel_parameters
                .lambda_bar
                .max(0.0)
            * seasonality_factor.max(0.0)
            * interval_duration_in_hours.max(0.0))
        .ceil();

        estimated_endogenous_event_count = estimated_endogenous_event_count.saturating_add(
            endogenous_baseline.min(MAX_STOCHASTIC_UPDATES_PER_INTERVAL as f64) as usize,
        );

        let surprise_expected_count = interval_news_intensity * interval_duration_in_hours.max(0.0);
        estimated_surprise_news_count = estimated_surprise_news_count.saturating_add(
            surprise_expected_count
                .ceil()
                .min(MAX_STOCHASTIC_UPDATES_PER_INTERVAL as f64) as usize,
        );
    }

    let estimated_transition_capacity = base_transition_count
        .saturating_add(known_news_count)
        .saturating_add(estimated_surprise_news_count)
        .saturating_add(estimated_endogenous_event_count);

    (
        estimated_transition_capacity,
        estimated_endogenous_event_count,
    )
}

fn merge_ordered_interval_updates(
    endogenous_interval_updates: Vec<IntervalStochasticUpdate>,
    exogenous_interval_updates: Vec<IntervalStochasticUpdate>,
) -> Vec<IntervalStochasticUpdate> {
    let mut endogenous_iterator = endogenous_interval_updates.into_iter();
    let mut exogenous_iterator = exogenous_interval_updates.into_iter();

    let mut next_endogenous = endogenous_iterator.next();
    let mut next_exogenous = exogenous_iterator.next();

    let mut merged_updates: Vec<IntervalStochasticUpdate> = Vec::with_capacity(
        endogenous_iterator
            .len()
            .saturating_add(exogenous_iterator.len())
            .saturating_add(next_endogenous.is_some() as usize)
            .saturating_add(next_exogenous.is_some() as usize),
    );

    loop {
        match (&next_endogenous, &next_exogenous) {
            (Some(endogenous_update), Some(exogenous_update)) => {
                if compare_interval_updates(endogenous_update, exogenous_update)
                    != Ordering::Greater
                {
                    merged_updates.push(next_endogenous.take().expect("endogenous update exists"));
                    next_endogenous = endogenous_iterator.next();
                } else {
                    merged_updates.push(next_exogenous.take().expect("exogenous update exists"));
                    next_exogenous = exogenous_iterator.next();
                }
            }
            (Some(_), None) => {
                merged_updates.push(next_endogenous.take().expect("endogenous update exists"));
                merged_updates.extend(endogenous_iterator);
                return merged_updates;
            }
            (None, Some(_)) => {
                merged_updates.push(next_exogenous.take().expect("exogenous update exists"));
                merged_updates.extend(exogenous_iterator);
                return merged_updates;
            }
            (None, None) => return merged_updates,
        }
    }
}

fn compare_interval_updates(
    left: &IntervalStochasticUpdate,
    right: &IntervalStochasticUpdate,
) -> Ordering {
    left.ordering_key().cmp(&right.ordering_key())
}

struct SeededSimulationMarketStateResolver<'a> {
    definition: &'a ExchangeCalendarDefinition,
    exchange_code: &'a str,
    timezone: Tz,
}

impl<'a> SeededSimulationMarketStateResolver<'a> {
    fn new(
        calendar_registry: &'a ExchangeCalendarRegistry,
        exchange_code: &'a str,
    ) -> Result<Self, SeededTickSimulationRunError> {
        let definition =
            calendar_registry
                .get_required(exchange_code)
                .map_err(|source| {
                    SeededTickSimulationRunError::NewsMarketStateResolutionFailed {
                exchange_code: exchange_code.to_string(),
                timestamp_utc_nanos: 0,
                source: match source {
                    crate::calendar::ExchangeCalendarRegistryLookupError::MissingExchangeId {
                        exchange_code,
                    } => crate::calendar::ExchangeCalendarResolveError::MissingExchangeId {
                        exchange_code,
                    },
                },
            }
                })?;

        let timezone = Tz::from_str(definition.timezone_name()).map_err(|_| {
            SeededTickSimulationRunError::NewsMarketStateResolutionFailed {
                exchange_code: exchange_code.to_string(),
                timestamp_utc_nanos: 0,
                source: crate::calendar::ExchangeCalendarResolveError::InvalidTimezoneName {
                    exchange_code: exchange_code.to_string(),
                    timezone_name: definition.timezone_name().to_string(),
                },
            }
        })?;

        Ok(Self {
            definition,
            exchange_code,
            timezone,
        })
    }

    fn resolve_market_state_at_nanos(
        &self,
        timestamp_utc_nanos: i64,
    ) -> Result<MarketState, SeededTickSimulationRunError> {
        let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
        let nanoseconds = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;

        let timestamp_utc = DateTime::<Utc>::from_timestamp(seconds, nanoseconds).ok_or(
            SeededTickSimulationRunError::InvalidKnownScheduledNewsTimestampUtcNanos {
                value: timestamp_utc_nanos,
            },
        )?;

        Ok(self
            .definition
            .resolve_market_state_at_utc_with_timezone(self.timezone, timestamp_utc))
    }
}

impl SharedNewsEventMarketStateResolver for SeededSimulationMarketStateResolver<'_> {
    fn resolve_market_state_at_utc(
        &self,
        exchange_code: &str,
        event_timestamp_utc: DateTime<Utc>,
    ) -> Result<MarketState, SharedNewsEventMarketStateResolutionError> {
        if exchange_code != self.exchange_code {
            return Err(SharedNewsEventMarketStateResolutionError {
                reason: format!(
                    "cached seeded market-state resolver is bound to exchange `{}` but received `{exchange_code}`",
                    self.exchange_code
                ),
            });
        }

        Ok(self
            .definition
            .resolve_market_state_at_utc_with_timezone(self.timezone, event_timestamp_utc))
    }
}

fn schedule_interval_stochastic_updates<F>(
    interval_index: usize,
    interval_start_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_duration_in_hours: f64,
    run_config: &SeededTickSimulationRunConfig,
    interval_known_news_event_timestamps_utc_nanos: &[i64],
    interval_start_state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    interval_seasonality_factor: f64,
    interval_news_intensity: f64,
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    randomness_factory: &F,
) -> Result<Vec<IntervalStochasticUpdate>, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let mut endogenous_side_stream = randomness_factory.create_substream(
        run_config.seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount,
        interval_index as u64,
    );
    let mut endogenous_auxiliary_stream = randomness_factory.create_substream(
        run_config.seed,
        SimulationRandomnessSubstreamDomain::EndogenousSellEventCount,
        interval_index as u64,
    );
    let mut endogenous_wait_stream = randomness_factory.create_substream(
        run_config.seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp,
        interval_index as u64,
    );
    let mut endogenous_acceptance_stream = randomness_factory.create_substream(
        run_config.seed,
        SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp,
        interval_index as u64,
    );

    let exogenous_interval_updates = schedule_exogenous_interval_news_updates(
        interval_index,
        interval_start_utc_nanos,
        interval_duration_nanos,
        interval_duration_in_hours,
        interval_news_intensity,
        run_config,
        interval_known_news_event_timestamps_utc_nanos,
        market_state_resolver,
        randomness_factory,
    )?;

    let _ = endogenous_auxiliary_stream.next_unit_interval_f64();

    let interval_end_utc_nanos = interval_start_utc_nanos + interval_duration_nanos;

    let endogenous_interval_updates = schedule_endogenous_interval_events_with_event_time_feedback(
        interval_start_state,
        kernel_parameters,
        interval_seasonality_factor,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        &exogenous_interval_updates,
        market_state_resolver,
        &mut endogenous_wait_stream,
        &mut endogenous_acceptance_stream,
        &mut endogenous_side_stream,
    )?;

    Ok(merge_ordered_interval_updates(
        endogenous_interval_updates,
        exogenous_interval_updates,
    ))
}

fn schedule_exogenous_interval_news_updates<F>(
    interval_index: usize,
    interval_start_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_duration_in_hours: f64,
    interval_news_intensity: f64,
    run_config: &SeededTickSimulationRunConfig,
    interval_known_news_event_timestamps_utc_nanos: &[i64],
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    randomness_factory: &F,
) -> Result<Vec<IntervalStochasticUpdate>, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let event_inputs = build_interval_news_event_inputs(
        interval_index,
        interval_start_utc_nanos,
        interval_duration_nanos,
        interval_duration_in_hours,
        interval_news_intensity,
        run_config,
        interval_known_news_event_timestamps_utc_nanos,
        randomness_factory,
    )?;

    let dispatch_output = dispatch_shared_news_events_with_resolved_market_states(
        market_state_resolver,
        market_state_resolver.exchange_code,
        SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
        event_inputs.as_slice(),
    )
    .map_err(
        |source| SeededTickSimulationRunError::SharedNewsDispatchFailure {
            interval_index,
            source,
        },
    )?;

    let mut exogenous_interval_updates = Vec::with_capacity(dispatch_output.records.len());
    for dispatch_record in dispatch_output.records {
        exogenous_interval_updates.push(IntervalStochasticUpdate::ExogenousNews {
            timestamp_utc_nanos: dispatch_record.event_timestamp_utc_nanos,
            news_arrival_component: dispatch_record.news_arrival_component,
            activity_coupling_mark: dispatch_record.j_t_increment,
            return_jump_mark: dispatch_record.coupled_return_jump_mark,
            market_state: dispatch_record.market_state,
            return_jump_routing_target: dispatch_record.return_jump_routing_target,
            open_window_return_jump_increment: dispatch_record.m_t_increment,
            closed_window_gap_increment: dispatch_record.g_t_increment,
            generation_order: dispatch_record.event_generation_order,
        });
    }

    exogenous_interval_updates.sort_by_key(IntervalStochasticUpdate::ordering_key);

    Ok(exogenous_interval_updates)
}

pub(crate) fn build_interval_news_event_inputs<F>(
    interval_index: usize,
    interval_start_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_duration_in_hours: f64,
    interval_news_intensity: f64,
    run_config: &SeededTickSimulationRunConfig,
    interval_known_news_event_timestamps_utc_nanos: &[i64],
    randomness_factory: &F,
) -> Result<Vec<SharedNewsEventTimestampedInput>, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let mut news_count_stream = randomness_factory.create_substream(
        run_config.seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsCount,
        interval_index as u64,
    );
    let mut news_timestamp_stream = randomness_factory.create_substream(
        run_config.seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp,
        interval_index as u64,
    );
    let mut news_mark_stream = randomness_factory.create_substream(
        run_config.seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsMarks,
        interval_index as u64,
    );

    let interval_known_news_event_count =
        u64::try_from(interval_known_news_event_timestamps_utc_nanos.len()).unwrap_or(u64::MAX);

    let interval_news_arrival_decomposition = derive_interval_news_arrival_decomposition(
        interval_news_intensity,
        interval_duration_in_hours,
        interval_known_news_event_count,
    );

    let interval_surprise_news_expected_count = interval_news_arrival_decomposition
        .surprise_component_intensity
        * interval_duration_in_hours;

    let interval_surprise_news_count = sample_count_from_expectation(
        interval_surprise_news_expected_count,
        &mut news_count_stream,
    );

    let mut event_inputs: Vec<SharedNewsEventTimestampedInput> = Vec::with_capacity(
        (interval_known_news_event_count + interval_surprise_news_count) as usize,
    );

    let mark_pipeline = run_config
        .model_inputs
        .news_process_configuration
        .default_mark_pipeline();

    let mut next_generation_order = 0_u64;

    for &known_news_timestamp_utc_nanos in interval_known_news_event_timestamps_utc_nanos {
        event_inputs.push(
            build_timestamped_news_event_input(
                &mark_pipeline,
                NewsArrivalComponent::Known,
                known_news_timestamp_utc_nanos,
                next_generation_order,
                run_config
                    .model_inputs
                    .news_process_configuration
                    .known_component_mark_scales,
                &mut news_mark_stream,
                interval_index,
            )
            .map_err(|source| {
                SeededTickSimulationRunError::NewsMarkSamplingFailure {
                    interval_index,
                    source,
                }
            })?,
        );

        next_generation_order = next_generation_order.saturating_add(1);
    }

    for _surprise_news_position in 0..interval_surprise_news_count {
        let timestamp_utc_nanos = sample_interval_timestamp_utc_nanos(
            interval_start_utc_nanos,
            interval_duration_nanos,
            &mut news_timestamp_stream,
        );

        event_inputs.push(
            build_timestamped_news_event_input(
                &mark_pipeline,
                NewsArrivalComponent::Surprise,
                timestamp_utc_nanos,
                next_generation_order,
                run_config
                    .model_inputs
                    .news_process_configuration
                    .surprise_component_mark_scales,
                &mut news_mark_stream,
                interval_index,
            )
            .map_err(|source| {
                SeededTickSimulationRunError::NewsMarkSamplingFailure {
                    interval_index,
                    source,
                }
            })?,
        );

        next_generation_order = next_generation_order.saturating_add(1);
    }

    Ok(event_inputs)
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
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    endogenous_wait_stream: &mut WaitStream,
    endogenous_acceptance_stream: &mut AcceptanceStream,
    endogenous_side_stream: &mut SideStream,
) -> Result<Vec<IntervalStochasticUpdate>, SeededTickSimulationRunError>
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
                )
                .map_err(map_transition_error_to_seeded_simulation_run_error)?;

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
                )
                .map_err(map_transition_error_to_seeded_simulation_run_error)?;

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
                )
                .map_err(map_transition_error_to_seeded_simulation_run_error)?;

                simulated_timestamp_utc_nanos = segment_end_utc_nanos;

                break;
            }

            let candidate_state = apply_hawkes_drift(
                simulated_state,
                kernel_parameters,
                simulated_timestamp_utc_nanos,
                candidate_timestamp_utc_nanos,
            )
            .map_err(map_transition_error_to_seeded_simulation_run_error)?;

            let candidate_market_state = market_state_resolver
                .resolve_market_state_at_nanos(candidate_timestamp_utc_nanos)?;
            if candidate_market_state != MarketState::Open {
                simulated_state = candidate_state;
                simulated_timestamp_utc_nanos = candidate_timestamp_utc_nanos;

                continue;
            }

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
                )
                .map_err(map_transition_error_to_seeded_simulation_run_error)?;

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
            )
            .map_err(map_transition_error_to_seeded_simulation_run_error)?;

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
                )
                .map_err(map_transition_error_to_seeded_simulation_run_error)?;
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

pub(crate) fn compute_max_effective_endogenous_branching_ratio(
    kernel_parameters: HawkesKernelParameters,
    seasonality_factors: &[f64],
) -> Option<(f64, f64, f64)> {
    let max_seasonality_factor = seasonality_factors
        .iter()
        .copied()
        .reduce(f64::max)?
        .max(0.0);

    let scaled_plus_plus = kernel_parameters.alpha.plus_plus / kernel_parameters.beta;
    let scaled_plus_minus = kernel_parameters.alpha.plus_minus / kernel_parameters.beta;
    let scaled_minus_plus = kernel_parameters.alpha.minus_plus / kernel_parameters.beta;
    let scaled_minus_minus = kernel_parameters.alpha.minus_minus / kernel_parameters.beta;
    let trace = scaled_plus_plus + scaled_minus_minus;

    let discriminant = ((scaled_plus_plus - scaled_minus_minus).powi(2)
        + 4.0 * scaled_plus_minus * scaled_minus_plus)
        .sqrt();

    let spectral_radius_alpha_over_beta = 0.5 * (trace + discriminant);
    let effective_branching_ratio = max_seasonality_factor * spectral_radius_alpha_over_beta;

    Some((
        max_seasonality_factor,
        spectral_radius_alpha_over_beta,
        effective_branching_ratio,
    ))
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

const fn build_seeded_fine_path_metadata(
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
) -> SeededFinePathMetadata {
    SeededFinePathMetadata {
        schema_version: SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
        path_record_schema_version: SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION,
        event_projection_record_schema_version: SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION,
        transition_ordering_policy: SEEDED_TICK_EVENT_ORDERING_POLICY,
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

pub(crate) fn build_timestamped_news_event_input(
    mark_pipeline: &janiform_model_kernels::news::DefaultNewsMarkPipeline,
    news_arrival_component: NewsArrivalComponent,
    event_timestamp_utc_nanos: i64,
    event_generation_order: u64,
    mark_scales: NewsComponentMarkScaleConfiguration,
    randomness_stream: &mut dyn janiform_foundations::randomness::SimulationRandomnessStream,
    _interval_index: usize,
) -> Result<SharedNewsEventTimestampedInput, janiform_model_kernels::news::NewsMarkInterfaceError> {
    let sampled_mark_payload = mark_pipeline.sample_coupled_marks(randomness_stream)?;

    Ok(SharedNewsEventTimestampedInput {
        event_timestamp_utc_nanos,
        event_generation_order,
        news_arrival_component,
        shared_severity_mark: sampled_mark_payload.shared_severity_mark
            * mark_scales.activity_coupling_scale,
        coupled_return_jump_mark: sampled_mark_payload
            .coupled_gap_mark_construction
            .gap_jump_mark
            * mark_scales.return_jump_scale,
    })
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

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{
        SeededSimulationMarketStateResolver, build_interval_open_market_segments,
        integrate_open_market_elapsed_time_weights,
    };
    use crate::calendar::{
        ExchangeCalendarArtifact, ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact,
        RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
    };

    fn build_full_weekday_registry() -> ExchangeCalendarRegistry {
        let regular_sessions = [
            TradingWeekday::Monday,
            TradingWeekday::Tuesday,
            TradingWeekday::Wednesday,
            TradingWeekday::Thursday,
            TradingWeekday::Friday,
        ]
        .into_iter()
        .map(|weekday| RegularSessionArtifact {
            weekday,
            windows: vec![SessionWindowArtifact {
                label: "full_day".to_string(),
                open_time: "00:00".to_string(),
                close_time: "24:00".to_string(),
            }],
        })
        .collect();

        ExchangeCalendarRegistry::from_artifact(ExchangeCalendarRegistryArtifact {
            schema_version: "calendar.registry.v1".to_string(),
            artifact_date: "2025-01-01".to_string(),
            exchanges: vec![ExchangeCalendarArtifact {
                schema_version: "exchange.calendar.v1".to_string(),
                exchange_code: "TEST".to_string(),
                exchange_name: "Test Exchange".to_string(),
                timezone_name: Some("UTC".to_string()),
                rule_metadata: crate::calendar::CalendarRuleMetadataArtifact {
                    holiday_calendar_code: "NONE".to_string(),
                    early_close_rule_set: None,
                },
                regular_sessions,
                holiday_dates: Vec::new(),
                early_close_sessions: Vec::new(),
            }],
        })
        .expect("test calendar registry should validate")
    }

    #[test]
    fn open_market_elapsed_time_weights_zero_closed_weekend_interval() {
        let calendar_registry = build_full_weekday_registry();

        let market_state_resolver =
            SeededSimulationMarketStateResolver::new(&calendar_registry, "TEST")
                .expect("market-state resolver should load");

        let start_timestamp_utc_nanos = Utc
            .with_ymd_and_hms(2025, 3, 22, 12, 0, 0)
            .single()
            .expect("start timestamp should exist")
            .timestamp_nanos_opt()
            .expect("start timestamp should fit in nanos");
        let end_timestamp_utc_nanos = Utc
            .with_ymd_and_hms(2025, 3, 22, 13, 0, 0)
            .single()
            .expect("end timestamp should exist")
            .timestamp_nanos_opt()
            .expect("end timestamp should fit in nanos");

        let interval_open_market_segments = build_interval_open_market_segments(
            &market_state_resolver,
            &[start_timestamp_utc_nanos],
            &[end_timestamp_utc_nanos],
        )
        .expect("interval open-market segments should build");

        let weights = integrate_open_market_elapsed_time_weights(
            interval_open_market_segments[0].as_slice(),
            start_timestamp_utc_nanos,
            end_timestamp_utc_nanos,
            1.0,
        );

        assert_eq!(weights.open_elapsed_hours, 0.0);
        assert_eq!(weights.open_decay_weight, 0.0);
    }
}
