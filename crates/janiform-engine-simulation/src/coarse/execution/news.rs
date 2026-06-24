use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessSubstreamDomain,
};
use janiform_foundations::time::elapsed_utc_nanos_to_hours;
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesIntervalTerminalStateContribution, HawkesTransitionError,
    hawkes_exogenous_news_terminal_state_contribution_for_remaining_time,
};
use janiform_model_kernels::news::{
    NewsArrivalComponent, SharedNewsAccumulatorState, SharedNewsEventTimestampedInput,
    dispatch_shared_news_events_with_resolved_market_states,
};

use crate::core::{
    build_timestamped_news_event_input, sample_count_from_expectation,
    sample_interval_timestamp_utc_nanos,
};

use super::super::contracts::CoarseSimulationRunRequest;
use super::error::CoarseSimulationRunError;

const KNOWN_NEWS_INTENSITY_ALIGNMENT_TOLERANCE: f64 = 1.0e-12;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct IntervalDirectJumpTotals {
    pub(super) terminal_hawkes_state_contribution: HawkesIntervalTerminalStateContribution,
    pub(super) open_jump_return: f64,
    pub(super) closed_gap_return: f64,
}

pub(super) fn sample_interval_direct_jump_totals<F>(
    run_request: &CoarseSimulationRunRequest,
    calendar_registry: &janiform_market_time::calendar::ExchangeCalendarRegistry,
    interval_index: usize,
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    interval_known_news_timestamp_slice: &[i64],
    randomness_factory: &F,
) -> Result<IntervalDirectJumpTotals, CoarseSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let model_inputs = &run_request.model_inputs;

    model_inputs
        .news_process_configuration
        .validate()
        .map_err(
            |source| CoarseSimulationRunError::NewsProcessConfigurationFailure {
                interval_index,
                source: Box::new(source),
            },
        )?;

    let mut news_count_stream = randomness_factory.create_substream(
        run_request.seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsCount,
        interval_index as u64,
    );

    let mut news_timestamp_stream = randomness_factory.create_substream(
        run_request.seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp,
        interval_index as u64,
    );

    let mut news_mark_stream = randomness_factory.create_substream(
        run_request.seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsMarks,
        interval_index as u64,
    );

    let mut event_inputs: Vec<SharedNewsEventTimestampedInput> =
        Vec::with_capacity(interval_known_news_timestamp_slice.len());

    let mut next_generation_order = 0_u64;

    let mark_pipeline = model_inputs
        .news_process_configuration
        .default_mark_pipeline();

    for &known_news_timestamp_utc_nanos in interval_known_news_timestamp_slice {
        event_inputs.push(
            build_timestamped_news_event_input(
                &mark_pipeline,
                NewsArrivalComponent::Known,
                known_news_timestamp_utc_nanos,
                next_generation_order,
                model_inputs
                    .news_process_configuration
                    .known_component_mark_scales,
                &mut news_mark_stream,
                interval_index,
            )
            .map_err(|source| CoarseSimulationRunError::NewsMarkSamplingFailure {
                interval_index,
                source: Box::new(source),
            })?,
        );

        next_generation_order = next_generation_order.saturating_add(1);
    }

    let interval_surprise_expected_count =
        run_request.interval_factor_grids.surprise_news_intensity[interval_index]
            * run_request.interval_delta;

    let interval_surprise_news_count =
        sample_count_from_expectation(interval_surprise_expected_count, &mut news_count_stream);

    let interval_duration_nanos = interval_end_utc_nanos - interval_start_utc_nanos;

    for _ in 0..interval_surprise_news_count {
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
                model_inputs
                    .news_process_configuration
                    .surprise_component_mark_scales,
                &mut news_mark_stream,
                interval_index,
            )
            .map_err(|source| CoarseSimulationRunError::NewsMarkSamplingFailure {
                interval_index,
                source: Box::new(source),
            })?,
        );

        next_generation_order = next_generation_order.saturating_add(1);
    }

    let dispatch_output = dispatch_shared_news_events_with_resolved_market_states(
        calendar_registry,
        &model_inputs.exchange_code,
        SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
        event_inputs.as_slice(),
    )
    .map_err(
        |source| CoarseSimulationRunError::SharedNewsDispatchFailure {
            interval_index,
            source: Box::new(source),
        },
    )?;

    let terminal_hawkes_state_contribution = resolve_interval_news_terminal_hawkes_contribution(
        run_request,
        interval_index,
        interval_end_utc_nanos,
        dispatch_output.records.as_slice(),
    )?;

    Ok(IntervalDirectJumpTotals {
        terminal_hawkes_state_contribution,
        open_jump_return: dispatch_output.final_state.m_t,
        closed_gap_return: dispatch_output.final_state.g_t,
    })
}

fn resolve_interval_news_terminal_hawkes_contribution(
    run_request: &CoarseSimulationRunRequest,
    interval_index: usize,
    interval_end_utc_nanos: i64,
    news_records: &[janiform_model_kernels::news::SharedNewsEventDispatchRecord],
) -> Result<HawkesIntervalTerminalStateContribution, CoarseSimulationRunError> {
    let mut terminal_contribution = HawkesIntervalTerminalStateContribution::empty();

    for news_record in news_records {
        let remaining_interval_delta = elapsed_utc_nanos_to_hours(
            news_record.event_timestamp_utc_nanos,
            interval_end_utc_nanos,
        );

        let event_contribution =
            hawkes_exogenous_news_terminal_state_contribution_for_remaining_time(
                run_request
                    .model_inputs
                    .sufficient_statistics_model_inputs
                    .equation_specification
                    .hawkes_kernel_parameters,
                news_record.j_t_increment,
                remaining_interval_delta,
            )
            .map_err(|source| {
                CoarseSimulationRunError::NewsBoundaryStateUpdateFailure {
                    interval_index,
                    source: Box::new(source),
                }
            })?;

        terminal_contribution =
            terminal_contribution
                .plus(event_contribution)
                .map_err(
                    |source| CoarseSimulationRunError::NewsBoundaryStateUpdateFailure {
                        interval_index,
                        source: Box::new(source),
                    },
                )?;
    }

    Ok(terminal_contribution)
}

pub(super) fn apply_interval_news_terminal_contribution(
    interval_index: usize,
    terminal_state_before_news: HawkesExcitationState,
    news_terminal_contribution: HawkesIntervalTerminalStateContribution,
) -> Result<HawkesExcitationState, CoarseSimulationRunError> {
    terminal_state_before_news
        .add_componentwise(news_terminal_contribution.state_contribution)
        .map_err(
            |source| CoarseSimulationRunError::NewsBoundaryStateUpdateFailure {
                interval_index,
                source: Box::new(HawkesTransitionError::InvalidState {
                    field: source.field,
                    value: source.value,
                }),
            },
        )
}

pub(super) fn validate_known_news_factor_alignment(
    interval_index: usize,
    interval_delta: f64,
    expected_known_news_intensity: f64,
    known_news_event_count: usize,
) -> Result<(), CoarseSimulationRunError> {
    let derived_known_news_intensity = known_news_event_count as f64 / interval_delta;
    if (expected_known_news_intensity - derived_known_news_intensity).abs()
        > KNOWN_NEWS_INTENSITY_ALIGNMENT_TOLERANCE
    {
        return Err(CoarseSimulationRunError::KnownNewsFactorAlignmentFailure {
            interval_index,
            expected_known_news_intensity,
            derived_known_news_intensity,
        });
    }

    Ok(())
}
