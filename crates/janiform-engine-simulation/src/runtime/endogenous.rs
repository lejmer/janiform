use janiform_foundations::randomness::SimulationRandomnessStream;
use janiform_foundations::time::elapsed_utc_nanos_to_hours;
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelParameters, HawkesTransitionError, HawkesTransitionStep,
    evolve_hawkes_state,
};

use crate::artifacts::SeededTickEventSide;
use crate::constants::{MAX_STOCHASTIC_UPDATES_PER_INTERVAL, NANOS_PER_HOUR};
use crate::error::SeededTickSimulationRunError;
use janiform_market_time::calendar::MarketState;

use super::buffers::{push_simulation_buffer_value, try_reserve_simulation_buffer};
use super::market_state::SeededSimulationMarketStateResolver;
use super::updates::{IntervalStochasticUpdate, map_seeded_tick_side_to_hawkes_side};
use super::validation::map_transition_error_to_seeded_simulation_run_error;

#[derive(Clone, Copy)]
pub(crate) struct EndogenousIntervalSchedulingInput<'a, 'registry> {
    pub(crate) interval_start_state: HawkesExcitationState,
    pub(crate) kernel_parameters: HawkesKernelParameters,
    pub(crate) interval_seasonality_factor: f64,
    pub(crate) interval_start_utc_nanos: i64,
    pub(crate) interval_end_utc_nanos: i64,
    pub(crate) exogenous_interval_updates: &'a [IntervalStochasticUpdate],
    pub(crate) market_state_resolver: &'a SeededSimulationMarketStateResolver<'registry>,
}

pub(crate) fn schedule_endogenous_interval_events_with_event_time_feedback<
    WaitStream,
    AcceptanceStream,
    SideStream,
>(
    input: EndogenousIntervalSchedulingInput<'_, '_>,
    endogenous_wait_stream: &mut WaitStream,
    endogenous_acceptance_stream: &mut AcceptanceStream,
    endogenous_side_stream: &mut SideStream,
) -> Result<Vec<IntervalStochasticUpdate>, SeededTickSimulationRunError>
where
    WaitStream: SimulationRandomnessStream,
    AcceptanceStream: SimulationRandomnessStream,
    SideStream: SimulationRandomnessStream,
{
    let EndogenousIntervalSchedulingInput {
        interval_start_state,
        kernel_parameters,
        interval_seasonality_factor,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        exogenous_interval_updates,
        market_state_resolver,
    } = input;

    let interval_duration_in_hours =
        elapsed_utc_nanos_to_hours(interval_start_utc_nanos, interval_end_utc_nanos);

    let estimated_endogenous_capacity = estimate_endogenous_interval_event_capacity(
        kernel_parameters.lambda_bar,
        interval_seasonality_factor,
        interval_duration_in_hours,
    );

    let mut endogenous_interval_updates: Vec<IntervalStochasticUpdate> =
        try_reserve_simulation_buffer(
            "endogenous_interval_updates",
            estimated_endogenous_capacity,
        )?;

    let mut scheduling_state = EndogenousSchedulingState {
        simulated_state: interval_start_state,
        simulated_timestamp_utc_nanos: interval_start_utc_nanos,
        next_exogenous_update_position: 0,
    };

    let baseline_total_intensity =
        (2.0 * interval_seasonality_factor * kernel_parameters.lambda_bar).max(0.0);

    while scheduling_state.simulated_timestamp_utc_nanos < interval_end_utc_nanos {
        let segment_end_utc_nanos = next_segment_end_utc_nanos(
            exogenous_interval_updates,
            scheduling_state.next_exogenous_update_position,
            interval_end_utc_nanos,
        );

        simulate_endogenous_segment(
            EndogenousSegmentSimulationInput {
                scheduling_state: &mut scheduling_state,
                kernel_parameters,
                interval_seasonality_factor,
                segment_end_utc_nanos,
                baseline_total_intensity,
                market_state_resolver,
                endogenous_interval_updates: &mut endogenous_interval_updates,
            },
            endogenous_wait_stream,
            endogenous_acceptance_stream,
            endogenous_side_stream,
        )?;

        if segment_end_utc_nanos == interval_end_utc_nanos
            || endogenous_interval_updates.len() >= MAX_STOCHASTIC_UPDATES_PER_INTERVAL as usize
        {
            break;
        }

        apply_exogenous_activity_coupling_marks(
            &mut scheduling_state,
            kernel_parameters,
            exogenous_interval_updates,
            segment_end_utc_nanos,
        )?;
    }

    Ok(endogenous_interval_updates)
}

pub(crate) fn estimate_endogenous_interval_event_capacity(
    lambda_bar: f64,
    seasonality_factor: f64,
    interval_duration_in_hours: f64,
) -> usize {
    (2.0 * lambda_bar.max(0.0) * seasonality_factor.max(0.0) * interval_duration_in_hours.max(0.0))
        .ceil()
        .min(MAX_STOCHASTIC_UPDATES_PER_INTERVAL as f64) as usize
}

#[derive(Debug, Clone, Copy)]
struct EndogenousSchedulingState {
    simulated_state: HawkesExcitationState,
    simulated_timestamp_utc_nanos: i64,
    next_exogenous_update_position: usize,
}

struct EndogenousSegmentSimulationInput<'a, 'registry> {
    scheduling_state: &'a mut EndogenousSchedulingState,
    kernel_parameters: HawkesKernelParameters,
    interval_seasonality_factor: f64,
    segment_end_utc_nanos: i64,
    baseline_total_intensity: f64,
    market_state_resolver: &'a SeededSimulationMarketStateResolver<'registry>,
    endogenous_interval_updates: &'a mut Vec<IntervalStochasticUpdate>,
}

fn simulate_endogenous_segment<WaitStream, AcceptanceStream, SideStream>(
    input: EndogenousSegmentSimulationInput<'_, '_>,
    endogenous_wait_stream: &mut WaitStream,
    endogenous_acceptance_stream: &mut AcceptanceStream,
    endogenous_side_stream: &mut SideStream,
) -> Result<(), SeededTickSimulationRunError>
where
    WaitStream: SimulationRandomnessStream,
    AcceptanceStream: SimulationRandomnessStream,
    SideStream: SimulationRandomnessStream,
{
    let EndogenousSegmentSimulationInput {
        scheduling_state,
        kernel_parameters,
        interval_seasonality_factor,
        segment_end_utc_nanos,
        baseline_total_intensity,
        market_state_resolver,
        endogenous_interval_updates,
    } = input;

    while scheduling_state.simulated_timestamp_utc_nanos < segment_end_utc_nanos
        && endogenous_interval_updates.len() < MAX_STOCHASTIC_UPDATES_PER_INTERVAL as usize
    {
        let (_, _, current_total_intensity) = project_non_negative_intensities(
            scheduling_state.simulated_state,
            kernel_parameters.lambda_bar,
            interval_seasonality_factor,
        );

        if current_total_intensity <= 0.0 {
            drift_state_to_timestamp(scheduling_state, kernel_parameters, segment_end_utc_nanos)?;
            break;
        }

        let proposal_upper_bound_total_intensity =
            current_total_intensity.max(baseline_total_intensity);

        let candidate_timestamp_utc_nanos = sample_candidate_timestamp_utc_nanos(
            scheduling_state.simulated_timestamp_utc_nanos,
            segment_end_utc_nanos,
            proposal_upper_bound_total_intensity,
            endogenous_wait_stream,
        );

        let Some(candidate_timestamp_utc_nanos) = candidate_timestamp_utc_nanos else {
            drift_state_to_timestamp(scheduling_state, kernel_parameters, segment_end_utc_nanos)?;
            break;
        };

        let candidate_state = apply_hawkes_drift(
            scheduling_state.simulated_state,
            kernel_parameters,
            scheduling_state.simulated_timestamp_utc_nanos,
            candidate_timestamp_utc_nanos,
        )
        .map_err(map_transition_error_to_seeded_simulation_run_error)?;

        let candidate_market_state =
            market_state_resolver.resolve_market_state_at_nanos(candidate_timestamp_utc_nanos)?;
        if candidate_market_state != MarketState::Open {
            scheduling_state.simulated_state = candidate_state;
            scheduling_state.simulated_timestamp_utc_nanos = candidate_timestamp_utc_nanos;
            continue;
        }

        let (candidate_lambda_plus, candidate_lambda_minus, candidate_total_intensity) =
            project_non_negative_intensities(
                candidate_state,
                kernel_parameters.lambda_bar,
                interval_seasonality_factor,
            );

        if accept_endogenous_event(
            candidate_total_intensity,
            proposal_upper_bound_total_intensity,
            endogenous_acceptance_stream.next_unit_interval_f64(),
        ) {
            let side = sample_endogenous_event_side_from_intensities(
                candidate_lambda_plus,
                candidate_lambda_minus,
                endogenous_side_stream.next_unit_interval_f64(),
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

            let generation_order = endogenous_interval_updates.len() as u64;

            push_simulation_buffer_value(
                "endogenous_interval_updates",
                endogenous_interval_updates,
                IntervalStochasticUpdate::EndogenousEvent {
                    timestamp_utc_nanos: candidate_timestamp_utc_nanos,
                    side,
                    generation_order,
                },
            )?;

            scheduling_state.simulated_state = state_after_event;
        } else {
            scheduling_state.simulated_state = candidate_state;
        }

        scheduling_state.simulated_timestamp_utc_nanos = candidate_timestamp_utc_nanos;
    }

    if scheduling_state.simulated_timestamp_utc_nanos < segment_end_utc_nanos {
        drift_state_to_timestamp(scheduling_state, kernel_parameters, segment_end_utc_nanos)?;
    }

    Ok(())
}

fn next_segment_end_utc_nanos(
    exogenous_interval_updates: &[IntervalStochasticUpdate],
    next_exogenous_update_position: usize,
    interval_end_utc_nanos: i64,
) -> i64 {
    exogenous_interval_updates
        .get(next_exogenous_update_position)
        .map(IntervalStochasticUpdate::timestamp_utc_nanos)
        .unwrap_or(interval_end_utc_nanos)
        .min(interval_end_utc_nanos)
}

fn sample_candidate_timestamp_utc_nanos<R>(
    simulated_timestamp_utc_nanos: i64,
    segment_end_utc_nanos: i64,
    proposal_upper_bound_total_intensity: f64,
    endogenous_wait_stream: &mut R,
) -> Option<i64>
where
    R: SimulationRandomnessStream,
{
    let waiting_time_hours = sample_exponential_wait_time_hours(
        proposal_upper_bound_total_intensity,
        endogenous_wait_stream,
    );

    let waiting_time_nanos = convert_wait_time_hours_to_positive_nanos(waiting_time_hours);
    if waiting_time_nanos == i64::MAX {
        return None;
    }

    let candidate_timestamp_utc_nanos =
        simulated_timestamp_utc_nanos.saturating_add(waiting_time_nanos);

    if candidate_timestamp_utc_nanos >= segment_end_utc_nanos {
        None
    } else {
        Some(candidate_timestamp_utc_nanos)
    }
}

fn accept_endogenous_event(
    candidate_total_intensity: f64,
    proposal_upper_bound_total_intensity: f64,
    acceptance_draw: f64,
) -> bool {
    let acceptance_probability = if proposal_upper_bound_total_intensity > 0.0 {
        (candidate_total_intensity / proposal_upper_bound_total_intensity).clamp(0.0, 1.0)
    } else {
        0.0
    };

    acceptance_draw <= acceptance_probability
}

fn apply_exogenous_activity_coupling_marks(
    scheduling_state: &mut EndogenousSchedulingState,
    kernel_parameters: HawkesKernelParameters,
    exogenous_interval_updates: &[IntervalStochasticUpdate],
    segment_end_utc_nanos: i64,
) -> Result<(), SeededTickSimulationRunError> {
    while scheduling_state.next_exogenous_update_position < exogenous_interval_updates.len()
        && exogenous_interval_updates[scheduling_state.next_exogenous_update_position]
            .timestamp_utc_nanos()
            == segment_end_utc_nanos
    {
        if let IntervalStochasticUpdate::ExogenousNews {
            activity_coupling_mark,
            ..
        } = exogenous_interval_updates[scheduling_state.next_exogenous_update_position]
        {
            scheduling_state.simulated_state = evolve_hawkes_state(
                scheduling_state.simulated_state,
                kernel_parameters,
                HawkesTransitionStep::new(0.0, None, Some(activity_coupling_mark)),
            )
            .map_err(map_transition_error_to_seeded_simulation_run_error)?;
        }

        scheduling_state.next_exogenous_update_position = scheduling_state
            .next_exogenous_update_position
            .saturating_add(1);
    }

    Ok(())
}

fn drift_state_to_timestamp(
    scheduling_state: &mut EndogenousSchedulingState,
    kernel_parameters: HawkesKernelParameters,
    timestamp_utc_nanos: i64,
) -> Result<(), SeededTickSimulationRunError> {
    scheduling_state.simulated_state = apply_hawkes_drift(
        scheduling_state.simulated_state,
        kernel_parameters,
        scheduling_state.simulated_timestamp_utc_nanos,
        timestamp_utc_nanos,
    )
    .map_err(map_transition_error_to_seeded_simulation_run_error)?;

    scheduling_state.simulated_timestamp_utc_nanos = timestamp_utc_nanos;

    Ok(())
}

fn apply_hawkes_drift(
    state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    previous_timestamp_utc_nanos: i64,
    timestamp_utc_nanos: i64,
) -> Result<HawkesExcitationState, HawkesTransitionError> {
    let elapsed_hours =
        elapsed_utc_nanos_to_hours(previous_timestamp_utc_nanos, timestamp_utc_nanos);

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
    let projected_intensities = state.project_deseasonalized_intensities(lambda_bar);
    let baseline = lambda_bar * seasonality_factor.max(0.0);

    let lambda_plus = (baseline + projected_intensities.lambda_plus - lambda_bar).max(0.0);
    let lambda_minus = (baseline + projected_intensities.lambda_minus - lambda_bar).max(0.0);

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

    let wait_time_nanos = (wait_time_hours * NANOS_PER_HOUR as f64).ceil();
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
