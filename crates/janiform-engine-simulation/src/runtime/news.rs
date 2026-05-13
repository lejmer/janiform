use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessStream, SimulationRandomnessSubstreamDomain,
};
use janiform_foundations::time::utc_datetime_from_nanos;
use janiform_model_kernels::news::{
    NewsArrivalComponent, NewsComponentMarkScaleConfiguration, NewsMarkInterfaceError,
    SharedNewsAccumulatorState, SharedNewsEventTimestampedInput,
    dispatch_shared_news_events_with_resolved_market_states,
};

use crate::artifacts::SeededTickSimulationNewsRoutingTotals;
use crate::config::SeededTickSimulationRunConfig;
use crate::constants::MAX_STOCHASTIC_UPDATES_PER_INTERVAL;
use crate::error::SeededTickSimulationRunError;

use super::buffers::try_reserve_simulation_buffer;
use super::market_state::SeededSimulationMarketStateResolver;
use super::updates::{IntervalStochasticUpdate, SeededTickSimulationNewsRoutingUpdate};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct IntervalNewsArrivalDecomposition {
    pub(crate) known_component_intensity: f64,
    pub(crate) surprise_component_intensity: f64,
}

#[derive(Clone, Copy)]
pub(crate) struct ExogenousIntervalNewsSchedulingInput<'a, 'registry> {
    pub(crate) interval_index: usize,
    pub(crate) interval_start_utc_nanos: i64,
    pub(crate) interval_duration_nanos: i64,
    pub(crate) interval_duration_in_hours: f64,
    pub(crate) interval_news_intensity: f64,
    pub(crate) run_config: &'a SeededTickSimulationRunConfig,
    pub(crate) interval_known_news_event_timestamps_utc_nanos: &'a [i64],
    pub(crate) market_state_resolver: &'a SeededSimulationMarketStateResolver<'registry>,
}

pub(crate) fn schedule_exogenous_interval_news_updates<F>(
    input: ExogenousIntervalNewsSchedulingInput<'_, '_>,
    randomness_factory: &F,
) -> Result<Vec<IntervalStochasticUpdate>, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let ExogenousIntervalNewsSchedulingInput {
        interval_index,
        interval_start_utc_nanos,
        interval_duration_nanos,
        interval_duration_in_hours,
        interval_news_intensity,
        run_config,
        interval_known_news_event_timestamps_utc_nanos,
        market_state_resolver,
    } = input;

    let event_inputs = build_interval_news_event_inputs(
        IntervalNewsEventInputBuildInput {
            interval_index,
            interval_start_utc_nanos,
            interval_duration_nanos,
            interval_duration_in_hours,
            interval_news_intensity,
            run_config,
            interval_known_news_event_timestamps_utc_nanos,
        },
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
            source: Box::new(source),
        },
    )?;

    let mut exogenous_interval_updates =
        try_reserve_simulation_buffer("exogenous_interval_updates", dispatch_output.records.len())?;
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct IntervalNewsEventInputBuildInput<'a> {
    pub(crate) interval_index: usize,
    pub(crate) interval_start_utc_nanos: i64,
    pub(crate) interval_duration_nanos: i64,
    pub(crate) interval_duration_in_hours: f64,
    pub(crate) interval_news_intensity: f64,
    pub(crate) run_config: &'a SeededTickSimulationRunConfig,
    pub(crate) interval_known_news_event_timestamps_utc_nanos: &'a [i64],
}

pub(crate) fn build_interval_news_event_inputs<F>(
    input: IntervalNewsEventInputBuildInput<'_>,
    randomness_factory: &F,
) -> Result<Vec<SharedNewsEventTimestampedInput>, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let IntervalNewsEventInputBuildInput {
        interval_index,
        interval_start_utc_nanos,
        interval_duration_nanos,
        interval_duration_in_hours,
        interval_news_intensity,
        run_config,
        interval_known_news_event_timestamps_utc_nanos,
    } = input;

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

    let event_input_capacity = usize::try_from(
        interval_known_news_event_count.saturating_add(interval_surprise_news_count),
    )
    .unwrap_or(usize::MAX);

    let mut event_inputs: Vec<SharedNewsEventTimestampedInput> =
        try_reserve_simulation_buffer("interval_news_event_inputs", event_input_capacity)?;

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
                    source: Box::new(source),
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
                    source: Box::new(source),
                }
            })?,
        );

        next_generation_order = next_generation_order.saturating_add(1);
    }

    Ok(event_inputs)
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
        if utc_datetime_from_nanos(timestamp_utc_nanos).is_none() {
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
    randomness_stream: &mut dyn SimulationRandomnessStream,
    _interval_index: usize,
) -> Result<SharedNewsEventTimestampedInput, NewsMarkInterfaceError> {
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

pub(crate) fn accumulate_news_routing_totals(
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
