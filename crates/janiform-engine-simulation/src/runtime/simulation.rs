use chrono::{DateTime, Utc};
use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessStream, SimulationRandomnessSubstreamDomain,
};
use janiform_foundations::time::{duration_nanos_to_hours, elapsed_utc_nanos_to_hours};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelParameters, evolve_hawkes_state,
};
use janiform_model_kernels::long_variance::{LongVarianceParameters, LongVarianceState};
use janiform_model_kernels::nig::NigBuySellDriverParameters;
use tracing::debug;

use crate::artifacts::{
    SeededFinePathMetadata, SeededFinePathRecord, SeededFinePathTransitionKind,
    SeededTickEventDebugTraceTransitionKind, SeededTickEventSide, SeededTickEventTapeMetadata,
    SeededTickSimulationNewsEventMarketState, SeededTickSimulationNewsRoutingTotals,
    SeededTickSimulationStreamCompletion, SeededTickSimulationTransitionConsumer,
    SeededTickSimulationTransitionEmission,
};
use crate::config::{SeededTickEventStateSnapshotCapture, SeededTickSimulationRunConfig};
use crate::constants::{
    SEEDED_TICK_EVENT_DETERMINISM_POLICY, SEEDED_TICK_EVENT_ORDERING_POLICY,
    SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION, SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY,
    SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
};
use crate::error::SeededTickSimulationRunError;
use crate::long_variance::{
    LongVarianceTransitionSample, apply_long_variance_event_jump, sample_long_variance_transition,
};
use crate::progress::{SimulationProgressEvent, SimulationProgressPhase, emit_progress_event};
use janiform_affine_oracle::sufficient_statistics::SufficientStatisticsReturnComponents;

use super::debug_trace::{
    SeededTickEventDebugTraceCaptureInput, SeededTickEventDebugTraceCollector,
};
use super::endogenous::{
    EndogenousIntervalSchedulingInput, schedule_endogenous_interval_events_with_event_time_feedback,
};
use super::market_state::SeededSimulationMarketStateResolver;
use super::metadata::{
    build_event_state_snapshot, build_seeded_fine_path_metadata,
    build_seeded_tick_event_tape_metadata,
};
use super::news::{
    ExogenousIntervalNewsSchedulingInput, accumulate_news_routing_totals,
    derive_known_news_interval_slice_bounds, schedule_exogenous_interval_news_updates,
};
use super::observed_state::SeededObservedState;
use super::results::estimate_seeded_result_capacities;
use super::trading_time::{
    TransitionTradingTimeReturnInput, build_interval_open_market_segments,
    integrate_open_market_elapsed_hours, sample_transition_trading_time_return,
};
use super::updates::{
    IntervalStochasticUpdate, is_public_seeded_fine_path_record,
    map_seeded_tick_side_to_hawkes_side, map_side_to_transition_kind,
    merge_ordered_interval_updates,
};
use super::validation::map_transition_error_to_seeded_simulation_run_error;

pub(crate) fn stream_seeded_tick_event_simulation_with_randomness_factory_internal<F, C>(
    run_config: SeededTickSimulationRunConfig,
    randomness_factory: &F,
    consumer: &mut C,
    mut observer: Option<&mut dyn FnMut(SimulationProgressEvent)>,
) -> Result<SeededTickSimulationStreamCompletion, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
    C: SeededTickSimulationTransitionConsumer,
{
    run_config.validate()?;

    let state_snapshot_capture = run_config.options.state_snapshot_capture;
    let lambda_bar = run_config.model_inputs.kernel_parameters.lambda_bar;

    let driver_parameters = run_config
        .model_inputs
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .map_err(
            |source| SeededTickSimulationRunError::InvalidNigSharedPlusShiftConfiguration {
                source: Box::new(source),
            },
        )?;

    let long_variance_stationary_mean_reversion_level = run_config
        .model_inputs
        .long_variance_parameters
        .stationary_mean_reversion_level_for_hawkes_kernel(
            run_config.model_inputs.kernel_parameters,
        )
        .map_err(
            |source| SeededTickSimulationRunError::InvalidLongVarianceStationaryLevel {
                source: Box::new(source),
            },
        )?;

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

    debug!(
        seed = run_config.seed,
        horizon_interval_count = run_config.horizon_interval_count,
        estimated_transition_capacity,
        estimated_event_capacity,
        "reserving seeded simulation result buffers"
    );

    consumer
        .reserve_transition_capacity(estimated_transition_capacity, estimated_event_capacity)?;

    let interval_duration_nanos = run_config
        .model_inputs
        .hourly_factor_contract
        .interval_duration_nanos;

    emit_progress_event(
        &mut observer,
        SimulationProgressEvent::new(
            SimulationProgressPhase::SeededTickIntervals,
            0,
            run_config.horizon_interval_count,
            interval_duration_nanos,
            Some(0),
        ),
    );

    let fine_path_state = SeededTickSimulationStaticState {
        lambda_bar,
        mean_reversion_kappa: run_config
            .model_inputs
            .mean_reversion_fitted_parameters
            .kappa,
        driver_parameters,
        long_variance_parameters: run_config.model_inputs.long_variance_parameters,
        long_variance_stationary_mean_reversion_level,
        state_snapshot_capture,
        fine_path_metadata,
        event_tape_metadata,
    };

    let known_news_timestamps = &run_config
        .model_inputs
        .news_process_configuration
        .arrivals
        .known
        .scheduled_event_timestamps_utc_nanos;

    let mut runtime_state = initialize_runtime_state(&run_config, lambda_bar)?;

    for (interval_index, interval_open_market_segment) in interval_open_market_segments
        .iter()
        .enumerate()
        .take(run_config.horizon_interval_count)
    {
        let interval_inputs = build_interval_simulation_inputs(
            &run_config,
            interval_index,
            known_news_timestamps,
            interval_open_market_segment.as_slice(),
            &mut runtime_state.next_known_scheduled_news_timestamp_position,
        )?;

        process_interval(
            &run_config,
            randomness_factory,
            consumer,
            &market_state_resolver,
            &fine_path_state,
            &mut runtime_state,
            interval_inputs,
        )?;

        emit_progress_event(
            &mut observer,
            SimulationProgressEvent::new(
                SimulationProgressPhase::SeededTickIntervals,
                interval_index + 1,
                run_config.horizon_interval_count,
                interval_duration_nanos,
                Some(runtime_state.next_event_index),
            ),
        );
    }

    Ok(build_stream_completion(
        run_config,
        fine_path_state,
        runtime_state,
    ))
}

#[derive(Clone, Copy)]
struct SeededTickSimulationStaticState {
    lambda_bar: f64,
    mean_reversion_kappa: f64,
    driver_parameters: NigBuySellDriverParameters,
    long_variance_parameters: LongVarianceParameters,
    long_variance_stationary_mean_reversion_level: f64,
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
    fine_path_metadata: SeededFinePathMetadata,
    event_tape_metadata: SeededTickEventTapeMetadata,
}

struct SeededTickSimulationRuntimeState {
    next_event_index: u64,
    next_transition_index: u64,
    next_state: HawkesExcitationState,
    long_variance_state: LongVarianceState,
    news_routing_totals: SeededTickSimulationNewsRoutingTotals,
    next_known_scheduled_news_timestamp_position: usize,
    pending_open_jump_return: f64,
    pending_closed_gap_return: f64,
    observed_state: SeededObservedState,
    debug_trace_collector: Option<SeededTickEventDebugTraceCollector>,
}

struct IntervalSimulationInputs<'a> {
    interval_index: usize,
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    interval_duration_nanos: i64,
    interval_duration_in_hours: f64,
    interval_seasonality_factor: f64,
    interval_news_intensity: f64,
    interval_end_anchor_value: f64,
    interval_end_pressure_state: f64,
    interval_known_news_event_timestamps_utc_nanos: &'a [i64],
    interval_open_market_segments: &'a [(i64, i64)],
}

fn initialize_runtime_state(
    run_config: &SeededTickSimulationRunConfig,
    lambda_bar: f64,
) -> Result<SeededTickSimulationRuntimeState, SeededTickSimulationRunError> {
    Ok(SeededTickSimulationRuntimeState {
        next_event_index: 0,
        next_transition_index: 0,
        next_state: run_config.model_inputs.initial_excitation_state,
        long_variance_state: run_config.model_inputs.initial_long_variance_state,
        news_routing_totals: SeededTickSimulationNewsRoutingTotals::default(),
        next_known_scheduled_news_timestamp_position: 0,
        pending_open_jump_return: 0.0,
        pending_closed_gap_return: 0.0,
        observed_state: SeededObservedState::from_boundary_state(
            &run_config
                .model_inputs
                .initial_mean_reversion_boundary_state,
        ),
        debug_trace_collector: if run_config.options.debug_trace_enabled {
            Some(SeededTickEventDebugTraceCollector::new(
                run_config.options.debug_trace_options,
                lambda_bar,
            )?)
        } else {
            None
        },
    })
}

fn build_interval_simulation_inputs<'a>(
    run_config: &'a SeededTickSimulationRunConfig,
    interval_index: usize,
    known_news_timestamps: &'a [i64],
    interval_open_market_segments: &'a [(i64, i64)],
    next_known_scheduled_news_timestamp_position: &mut usize,
) -> Result<IntervalSimulationInputs<'a>, SeededTickSimulationRunError> {
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

    let (
        interval_known_news_start_position,
        interval_known_news_end_position,
        next_interval_known_news_timestamp_position,
    ) = derive_known_news_interval_slice_bounds(
        known_news_timestamps,
        *next_known_scheduled_news_timestamp_position,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
    );

    *next_known_scheduled_news_timestamp_position = next_interval_known_news_timestamp_position;

    let interval_duration_nanos = interval_end_utc_nanos - interval_start_utc_nanos;

    Ok(IntervalSimulationInputs {
        interval_index,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        interval_duration_nanos,
        interval_duration_in_hours: duration_nanos_to_hours(interval_duration_nanos),
        interval_seasonality_factor: run_config.model_inputs.hourly_factor_contract.s_h
            [interval_index],
        interval_news_intensity: run_config.model_inputs.hourly_factor_contract.nu_h
            [interval_index]
            .max(0.0),
        interval_end_anchor_value: run_config
            .model_inputs
            .mean_reversion_runtime_interval_anchors
            .end_anchor_values[interval_index],
        interval_end_pressure_state: run_config
            .model_inputs
            .mean_reversion_runtime_interval_anchors
            .end_pressure_states[interval_index],
        interval_known_news_event_timestamps_utc_nanos: &known_news_timestamps
            [interval_known_news_start_position..interval_known_news_end_position],
        interval_open_market_segments,
    })
}

fn process_interval<F, C>(
    run_config: &SeededTickSimulationRunConfig,
    randomness_factory: &F,
    consumer: &mut C,
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    fine_path_state: &SeededTickSimulationStaticState,
    runtime_state: &mut SeededTickSimulationRuntimeState,
    interval_inputs: IntervalSimulationInputs<'_>,
) -> Result<(), SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
    C: SeededTickSimulationTransitionConsumer,
{
    let interval_updates = schedule_interval_stochastic_updates(
        interval_inputs.interval_index,
        &interval_inputs,
        run_config,
        runtime_state.next_state,
        run_config.model_inputs.kernel_parameters,
        market_state_resolver,
        randomness_factory,
    )?;

    let mut previous_timestamp_utc_nanos = interval_inputs.interval_start_utc_nanos;

    for interval_update in interval_updates {
        previous_timestamp_utc_nanos = process_interval_update(IntervalUpdateProcessingInput {
            run_config,
            randomness_factory,
            consumer,
            market_state_resolver,
            fine_path_state,
            runtime_state,
            interval_inputs: &interval_inputs,
            previous_timestamp_utc_nanos,
            interval_update,
        })?;
    }

    emit_interval_boundary_transition(IntervalBoundaryTransitionInput {
        run_config,
        randomness_factory,
        consumer,
        market_state_resolver,
        fine_path_state,
        runtime_state,
        interval_inputs: &interval_inputs,
        previous_timestamp_utc_nanos,
    })
}

fn schedule_interval_stochastic_updates<F>(
    interval_index: usize,
    interval_inputs: &IntervalSimulationInputs<'_>,
    run_config: &SeededTickSimulationRunConfig,
    interval_start_state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
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
        ExogenousIntervalNewsSchedulingInput {
            interval_index,
            interval_start_utc_nanos: interval_inputs.interval_start_utc_nanos,
            interval_duration_nanos: interval_inputs.interval_duration_nanos,
            interval_duration_in_hours: interval_inputs.interval_duration_in_hours,
            interval_news_intensity: interval_inputs.interval_news_intensity,
            run_config,
            interval_known_news_event_timestamps_utc_nanos: interval_inputs
                .interval_known_news_event_timestamps_utc_nanos,
            market_state_resolver,
        },
        randomness_factory,
    )?;

    endogenous_auxiliary_stream.next_unit_interval_f64();

    let endogenous_interval_updates = schedule_endogenous_interval_events_with_event_time_feedback(
        EndogenousIntervalSchedulingInput {
            interval_start_state,
            kernel_parameters,
            interval_seasonality_factor: interval_inputs.interval_seasonality_factor,
            interval_start_utc_nanos: interval_inputs.interval_start_utc_nanos,
            interval_end_utc_nanos: interval_inputs.interval_end_utc_nanos,
            exogenous_interval_updates: &exogenous_interval_updates,
            market_state_resolver,
        },
        &mut endogenous_wait_stream,
        &mut endogenous_acceptance_stream,
        &mut endogenous_side_stream,
    )?;

    merge_ordered_interval_updates(endogenous_interval_updates, exogenous_interval_updates)
}

fn sample_seeded_transition_long_variance<F>(
    run_config: &SeededTickSimulationRunConfig,
    randomness_factory: &F,
    fine_path_state: &SeededTickSimulationStaticState,
    runtime_state: &SeededTickSimulationRuntimeState,
    interval_index: usize,
    elapsed_open_hours: f64,
) -> Result<LongVarianceTransitionSample, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    sample_long_variance_transition(
        runtime_state.long_variance_state,
        fine_path_state.long_variance_parameters,
        fine_path_state.long_variance_stationary_mean_reversion_level,
        elapsed_open_hours,
        &mut randomness_factory.create_substream(
            run_config.seed,
            SimulationRandomnessSubstreamDomain::LongVarianceDiffusion,
            runtime_state.next_transition_index,
        ),
    )
    .map_err(
        |source| SeededTickSimulationRunError::LongVarianceTransitionFailure {
            interval_index,
            source: Box::new(source),
        },
    )
}

struct IntervalUpdateProcessingInput<'a, 'registry, 'interval, F, C> {
    run_config: &'a SeededTickSimulationRunConfig,
    randomness_factory: &'a F,
    consumer: &'a mut C,
    market_state_resolver: &'a SeededSimulationMarketStateResolver<'registry>,
    fine_path_state: &'a SeededTickSimulationStaticState,
    runtime_state: &'a mut SeededTickSimulationRuntimeState,
    interval_inputs: &'a IntervalSimulationInputs<'interval>,
    previous_timestamp_utc_nanos: i64,
    interval_update: IntervalStochasticUpdate,
}

fn process_interval_update<F, C>(
    input: IntervalUpdateProcessingInput<'_, '_, '_, F, C>,
) -> Result<i64, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
    C: SeededTickSimulationTransitionConsumer,
{
    let IntervalUpdateProcessingInput {
        run_config,
        randomness_factory,
        consumer,
        market_state_resolver,
        fine_path_state,
        runtime_state,
        interval_inputs,
        previous_timestamp_utc_nanos,
        interval_update,
    } = input;

    let elapsed_hours_since_previous_update = elapsed_utc_nanos_to_hours(
        previous_timestamp_utc_nanos,
        interval_update.timestamp_utc_nanos(),
    );

    let current_market_state = market_state_resolver
        .resolve_market_state_at_nanos(interval_update.timestamp_utc_nanos())?;

    let (open_jump_return, closed_gap_return) =
        realize_pending_jump_returns(runtime_state, current_market_state);

    let elapsed_open_hours_since_previous_update = integrate_open_market_elapsed_hours(
        interval_inputs.interval_open_market_segments,
        previous_timestamp_utc_nanos,
        interval_update.timestamp_utc_nanos(),
    );

    let long_variance_transition = sample_seeded_transition_long_variance(
        run_config,
        randomness_factory,
        fine_path_state,
        runtime_state,
        interval_inputs.interval_index,
        elapsed_open_hours_since_previous_update,
    )?;

    let trading_time_return =
        sample_transition_trading_time_return(TransitionTradingTimeReturnInput {
            driver_parameters: fine_path_state.driver_parameters,
            state: runtime_state.next_state,
            lambda_bar: fine_path_state.lambda_bar,
            beta: run_config.model_inputs.kernel_parameters.beta,
            seasonality_factor: interval_inputs.interval_seasonality_factor,
            return_clock_exposure_parameters: run_config
                .model_inputs
                .return_clock_exposure_parameters,
            baseline_long_variance_clock_scale: long_variance_transition.baseline_clock_scale,
            interval_open_market_segments: interval_inputs.interval_open_market_segments,
            previous_timestamp_utc_nanos,
            timestamp_utc_nanos: interval_update.timestamp_utc_nanos(),
            interval_index: interval_inputs.interval_index,
            randomness_stream: randomness_factory.create_substream(
                run_config.seed,
                SimulationRandomnessSubstreamDomain::CoarseTradingTimeReturn,
                runtime_state.next_transition_index,
            ),
        })?;

    let raw_total_return =
        trading_time_return.trading_time_return + open_jump_return + closed_gap_return;

    let projected_core_increment = runtime_state.observed_state.project_core_increment(
        fine_path_state.mean_reversion_kappa,
        elapsed_open_hours_since_previous_update,
        raw_total_return,
        run_config.model_inputs.observed_price_tick_size,
        interval_inputs.interval_index,
    )?;

    let total_return = projected_core_increment.observed_core_increment;

    let return_components = SufficientStatisticsReturnComponents::try_new(
        trading_time_return.buy_side_return,
        trading_time_return.sell_side_return,
        trading_time_return.trading_time_return + total_return - raw_total_return,
        open_jump_return,
        closed_gap_return,
        total_return,
    )
    .map_err(
        |source| SeededTickSimulationRunError::InvalidReturnComponents {
            interval_index: interval_inputs.interval_index,
            source: Box::new(source),
        },
    )?;

    runtime_state.observed_state = runtime_state.observed_state.apply_projected_core_increment(
        fine_path_state.mean_reversion_kappa,
        elapsed_open_hours_since_previous_update,
        projected_core_increment,
    );

    let transition_step = interval_update.transition_step(elapsed_hours_since_previous_update);

    let event_index_for_update = match interval_update {
        IntervalStochasticUpdate::EndogenousEvent { .. } => Some(runtime_state.next_event_index),
        IntervalStochasticUpdate::ExogenousNews { .. } => None,
    };

    let previous_state = runtime_state.next_state;

    runtime_state.next_state = evolve_hawkes_state(
        runtime_state.next_state,
        run_config.model_inputs.kernel_parameters,
        transition_step,
    )
    .map_err(map_transition_error_to_seeded_simulation_run_error)?;

    runtime_state.long_variance_state = long_variance_transition.terminal_state;

    if let IntervalStochasticUpdate::EndogenousEvent { side, .. } = interval_update {
        runtime_state.long_variance_state = apply_long_variance_event_jump(
            runtime_state.long_variance_state,
            fine_path_state.long_variance_parameters,
            map_seeded_tick_side_to_hawkes_side(side),
        )
        .map_err(
            |source| SeededTickSimulationRunError::LongVarianceTransitionFailure {
                interval_index: interval_inputs.interval_index,
                source: Box::new(source),
            },
        )?;
    }

    if let Some(news_routing_update) = interval_update.news_routing_update() {
        runtime_state.news_routing_totals =
            accumulate_news_routing_totals(runtime_state.news_routing_totals, news_routing_update)?;
    }

    capture_debug_transition(
        runtime_state.debug_trace_collector.as_mut(),
        SeededTickEventDebugTraceCaptureInput {
            transition_index: runtime_state.next_transition_index,
            interval_index: interval_inputs.interval_index,
            previous_timestamp_utc_nanos,
            timestamp_utc_nanos: interval_update.timestamp_utc_nanos(),
            elapsed_hours_since_previous_update,
            transition_kind: interval_update.transition_kind(),
            generation_order: Some(interval_update.generation_order()),
            event_index: event_index_for_update,
            news_transition_payload: interval_update.trace_news_transition_payload(),
            news_routing_totals: runtime_state.news_routing_totals,
            seasonality_factor: interval_inputs.interval_seasonality_factor,
            interval_news_intensity: interval_inputs.interval_news_intensity,
            state_before_transition: previous_state,
            state_after_transition: runtime_state.next_state,
        },
    );

    let transition_index = runtime_state.next_transition_index;

    runtime_state.next_transition_index = runtime_state.next_transition_index.saturating_add(1);

    let state_snapshot = build_event_state_snapshot(
        runtime_state.next_state,
        fine_path_state.lambda_bar,
        fine_path_state.state_snapshot_capture,
    );

    let (transition_kind, side, news_arrival_component) =
        apply_interval_update_side_effects(runtime_state, interval_update);

    let fine_path_record = SeededFinePathRecord {
        transition_index,
        timestamp_utc_nanos: interval_update.timestamp_utc_nanos(),
        transition_kind,
        side,
        news_arrival_component,
        market_state: SeededTickSimulationNewsEventMarketState::from_market_state(
            current_market_state,
        ),
        return_components,
        core_total_return_increment: total_return,
        observed_log_price: runtime_state.observed_state.observed_log_price,
        anchor_value: runtime_state.observed_state.anchor_value,
        pressure_state: runtime_state.observed_state.pressure_state,
        deviation_from_anchor: runtime_state.observed_state.deviation_from_anchor,
        cumulative_core_log_return: runtime_state.observed_state.cumulative_core_log_return,
        state_snapshot,
    };

    let emission = SeededTickSimulationTransitionEmission {
        is_public_fine_path_record: is_public_seeded_fine_path_record(&fine_path_record),
        is_event_projection_record: matches!(
            fine_path_record.transition_kind,
            SeededFinePathTransitionKind::BuyTrade | SeededFinePathTransitionKind::SellTrade
        ),
    };

    consumer.consume_transition(fine_path_record, emission)?;

    Ok(interval_update.timestamp_utc_nanos())
}

struct IntervalBoundaryTransitionInput<'a, 'registry, 'interval, F, C> {
    run_config: &'a SeededTickSimulationRunConfig,
    randomness_factory: &'a F,
    consumer: &'a mut C,
    market_state_resolver: &'a SeededSimulationMarketStateResolver<'registry>,
    fine_path_state: &'a SeededTickSimulationStaticState,
    runtime_state: &'a mut SeededTickSimulationRuntimeState,
    interval_inputs: &'a IntervalSimulationInputs<'interval>,
    previous_timestamp_utc_nanos: i64,
}

fn emit_interval_boundary_transition<F, C>(
    input: IntervalBoundaryTransitionInput<'_, '_, '_, F, C>,
) -> Result<(), SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
    C: SeededTickSimulationTransitionConsumer,
{
    let IntervalBoundaryTransitionInput {
        run_config,
        randomness_factory,
        consumer,
        market_state_resolver,
        fine_path_state,
        runtime_state,
        interval_inputs,
        previous_timestamp_utc_nanos,
    } = input;

    let elapsed_hours_to_interval_end = elapsed_utc_nanos_to_hours(
        previous_timestamp_utc_nanos,
        interval_inputs.interval_end_utc_nanos,
    );

    let previous_state = runtime_state.next_state;

    let boundary_market_state = market_state_resolver
        .resolve_market_state_at_nanos(interval_inputs.interval_end_utc_nanos)?;

    let (open_jump_return, closed_gap_return) =
        realize_pending_jump_returns(runtime_state, boundary_market_state);

    let elapsed_open_hours_to_interval_end = integrate_open_market_elapsed_hours(
        interval_inputs.interval_open_market_segments,
        previous_timestamp_utc_nanos,
        interval_inputs.interval_end_utc_nanos,
    );

    let long_variance_transition = sample_seeded_transition_long_variance(
        run_config,
        randomness_factory,
        fine_path_state,
        runtime_state,
        interval_inputs.interval_index,
        elapsed_open_hours_to_interval_end,
    )?;

    let trading_time_return =
        sample_transition_trading_time_return(TransitionTradingTimeReturnInput {
            driver_parameters: fine_path_state.driver_parameters,
            state: previous_state,
            lambda_bar: fine_path_state.lambda_bar,
            beta: run_config.model_inputs.kernel_parameters.beta,
            seasonality_factor: interval_inputs.interval_seasonality_factor,
            return_clock_exposure_parameters: run_config
                .model_inputs
                .return_clock_exposure_parameters,
            baseline_long_variance_clock_scale: long_variance_transition.baseline_clock_scale,
            interval_open_market_segments: interval_inputs.interval_open_market_segments,
            previous_timestamp_utc_nanos,
            timestamp_utc_nanos: interval_inputs.interval_end_utc_nanos,
            interval_index: interval_inputs.interval_index,
            randomness_stream: randomness_factory.create_substream(
                run_config.seed,
                SimulationRandomnessSubstreamDomain::CoarseTradingTimeReturn,
                runtime_state.next_transition_index,
            ),
        })?;

    let raw_total_return =
        trading_time_return.trading_time_return + open_jump_return + closed_gap_return;

    let projected_core_increment = runtime_state.observed_state.project_core_increment(
        fine_path_state.mean_reversion_kappa,
        elapsed_open_hours_to_interval_end,
        raw_total_return,
        run_config.model_inputs.observed_price_tick_size,
        interval_inputs.interval_index,
    )?;

    let total_return = projected_core_increment.observed_core_increment;

    let return_components = SufficientStatisticsReturnComponents::try_new(
        trading_time_return.buy_side_return,
        trading_time_return.sell_side_return,
        trading_time_return.trading_time_return + total_return - raw_total_return,
        open_jump_return,
        closed_gap_return,
        total_return,
    )
    .map_err(
        |source| SeededTickSimulationRunError::InvalidReturnComponents {
            interval_index: interval_inputs.interval_index,
            source: Box::new(source),
        },
    )?;

    runtime_state.observed_state = runtime_state
        .observed_state
        .apply_projected_core_increment(
            fine_path_state.mean_reversion_kappa,
            elapsed_open_hours_to_interval_end,
            projected_core_increment,
        )
        .apply_anchor_reset(
            interval_inputs.interval_end_anchor_value,
            interval_inputs.interval_end_pressure_state,
        );

    runtime_state.next_state = evolve_hawkes_state(
        runtime_state.next_state,
        run_config.model_inputs.kernel_parameters,
        janiform_model_kernels::hawkes::HawkesTransitionStep::new(
            elapsed_hours_to_interval_end,
            None,
            None,
        ),
    )
    .map_err(map_transition_error_to_seeded_simulation_run_error)?;

    runtime_state.long_variance_state = long_variance_transition.terminal_state;

    capture_debug_transition(
        runtime_state.debug_trace_collector.as_mut(),
        SeededTickEventDebugTraceCaptureInput {
            transition_index: runtime_state.next_transition_index,
            interval_index: interval_inputs.interval_index,
            previous_timestamp_utc_nanos,
            timestamp_utc_nanos: interval_inputs.interval_end_utc_nanos,
            elapsed_hours_since_previous_update: elapsed_hours_to_interval_end,
            transition_kind: SeededTickEventDebugTraceTransitionKind::IntervalBoundaryDrift,
            generation_order: None,
            event_index: None,
            news_transition_payload: None,
            news_routing_totals: runtime_state.news_routing_totals,
            seasonality_factor: interval_inputs.interval_seasonality_factor,
            interval_news_intensity: interval_inputs.interval_news_intensity,
            state_before_transition: previous_state,
            state_after_transition: runtime_state.next_state,
        },
    );

    let boundary_record = SeededFinePathRecord {
        transition_index: runtime_state.next_transition_index,
        timestamp_utc_nanos: interval_inputs.interval_end_utc_nanos,
        transition_kind: SeededFinePathTransitionKind::IntervalBoundaryDrift,
        side: None,
        news_arrival_component: None,
        market_state: SeededTickSimulationNewsEventMarketState::from_market_state(
            boundary_market_state,
        ),
        return_components,
        core_total_return_increment: total_return,
        observed_log_price: runtime_state.observed_state.observed_log_price,
        anchor_value: runtime_state.observed_state.anchor_value,
        pressure_state: runtime_state.observed_state.pressure_state,
        deviation_from_anchor: runtime_state.observed_state.deviation_from_anchor,
        cumulative_core_log_return: runtime_state.observed_state.cumulative_core_log_return,
        state_snapshot: build_event_state_snapshot(
            runtime_state.next_state,
            fine_path_state.lambda_bar,
            fine_path_state.state_snapshot_capture,
        ),
    };

    let boundary_emission = SeededTickSimulationTransitionEmission {
        is_public_fine_path_record: is_public_seeded_fine_path_record(&boundary_record),
        is_event_projection_record: false,
    };

    consumer.consume_transition(boundary_record, boundary_emission)?;

    runtime_state.next_transition_index = runtime_state.next_transition_index.saturating_add(1);

    Ok(())
}

fn capture_debug_transition(
    debug_trace_collector: Option<&mut SeededTickEventDebugTraceCollector>,
    capture_input: SeededTickEventDebugTraceCaptureInput,
) {
    if let Some(trace_collector) = debug_trace_collector {
        trace_collector.capture_transition(capture_input);
    }
}

fn apply_interval_update_side_effects(
    runtime_state: &mut SeededTickSimulationRuntimeState,
    interval_update: IntervalStochasticUpdate,
) -> (
    SeededFinePathTransitionKind,
    Option<SeededTickEventSide>,
    Option<janiform_model_kernels::news::NewsArrivalComponent>,
) {
    match interval_update {
        IntervalStochasticUpdate::EndogenousEvent {
            timestamp_utc_nanos: _,
            side,
            ..
        } => {
            runtime_state.next_event_index = runtime_state.next_event_index.saturating_add(1);
            (map_side_to_transition_kind(side), Some(side), None)
        }
        IntervalStochasticUpdate::ExogenousNews {
            timestamp_utc_nanos: _,
            news_arrival_component,
            open_window_return_jump_increment,
            closed_window_gap_increment,
            ..
        } => {
            runtime_state.pending_open_jump_return += open_window_return_jump_increment;
            runtime_state.pending_closed_gap_return += closed_window_gap_increment;
            (
                SeededFinePathTransitionKind::ExogenousNews,
                None,
                Some(news_arrival_component),
            )
        }
    }
}

fn realize_pending_jump_returns(
    runtime_state: &mut SeededTickSimulationRuntimeState,
    market_state: janiform_market_time::calendar::MarketState,
) -> (f64, f64) {
    if market_state != janiform_market_time::calendar::MarketState::Open {
        return (0.0, 0.0);
    }

    let open_jump_return = runtime_state.pending_open_jump_return;
    let closed_gap_return = runtime_state.pending_closed_gap_return;

    runtime_state.pending_open_jump_return = 0.0;
    runtime_state.pending_closed_gap_return = 0.0;

    (open_jump_return, closed_gap_return)
}

fn build_stream_completion(
    run_config: SeededTickSimulationRunConfig,
    fine_path_state: SeededTickSimulationStaticState,
    runtime_state: SeededTickSimulationRuntimeState,
) -> SeededTickSimulationStreamCompletion {
    let horizon_end_utc_nanos = run_config
        .model_inputs
        .hourly_factor_contract
        .interval_end_utc_nanos[run_config.horizon_interval_count - 1];

    SeededTickSimulationStreamCompletion {
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
        fine_path_metadata: fine_path_state.fine_path_metadata,
        event_tape_metadata: fine_path_state.event_tape_metadata,
        final_excitation_state: runtime_state.next_state,
        news_routing_totals: runtime_state.news_routing_totals,
        cumulative_core_log_return: runtime_state.observed_state.cumulative_core_log_return,
        debug_trace_enabled: run_config.options.debug_trace_enabled,
        debug_trace_output: runtime_state
            .debug_trace_collector
            .map(SeededTickEventDebugTraceCollector::finish),
    }
}
