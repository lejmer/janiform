use janiform_foundations::numeric::validate_finite_number;
use janiform_foundations::randomness::SimulationRandomnessStream;

use super::contracts::{MarketPressureState, MarketPressureTransitionRates};
use super::errors::MarketPressureStateEvolutionError;
use super::evolution::validate_transition_input;
use super::interval::{
    MarketPressureIntervalTransitionInput, MarketPressureSampledIntervalTransition,
};

/// Sample one CTMC path and its integrated signed side-balance exposure.
pub fn sample_market_pressure_interval_transition<R>(
    input: MarketPressureIntervalTransitionInput<'_>,
    randomness_stream: &mut R,
) -> Result<MarketPressureSampledIntervalTransition, MarketPressureStateEvolutionError>
where
    R: SimulationRandomnessStream + ?Sized,
{
    validate_transition_input(&input)?;

    let probabilities = input
        .initial_state
        .normalized_probabilities()
        .map_err(MarketPressureStateEvolutionError::InvalidState)?;

    let mut state_position = sample_state_position(&probabilities, randomness_stream);

    let mut remaining_delta = input.interval_delta;
    let mut integrated_side_imbalance = 0.0;

    while remaining_delta > 0.0 {
        let exit_rate = state_exit_rate(&input.parameters.transition_rates, state_position);
        if exit_rate == 0.0 {
            integrated_side_imbalance +=
                input.parameters.side_imbalance_levels[state_position] * remaining_delta;
            break;
        }

        let waiting_time = sample_exponential_wait_time(exit_rate, randomness_stream);
        if waiting_time >= remaining_delta {
            integrated_side_imbalance +=
                input.parameters.side_imbalance_levels[state_position] * remaining_delta;
            break;
        }

        integrated_side_imbalance +=
            input.parameters.side_imbalance_levels[state_position] * waiting_time;

        remaining_delta -= waiting_time;

        state_position = sample_next_state_position(
            &input.parameters.transition_rates,
            state_position,
            randomness_stream.next_unit_interval_f64(),
        );
    }

    validate_finite_number(integrated_side_imbalance, "integrated_side_imbalance").map_err(
        |source| MarketPressureStateEvolutionError::InvalidIntegratedSideImbalance {
            value: source.value,
        },
    )?;

    let signed_balance_exposure = integrated_side_imbalance * input.signed_balance_exposure_scale;

    validate_finite_number(signed_balance_exposure, "signed_balance_exposure").map_err(
        |source| MarketPressureStateEvolutionError::InvalidSignedBalanceExposure {
            value: source.value,
        },
    )?;

    let anchor_increment =
        input.parameters.anchor_drift_per_signed_balance * signed_balance_exposure;

    validate_finite_number(anchor_increment, "market_pressure_anchor_increment").map_err(
        |source| MarketPressureStateEvolutionError::InvalidAnchorIncrement {
            value: source.value,
        },
    )?;

    let terminal_state =
        MarketPressureState::from_state_position(input.parameters.state_count(), state_position)
            .map_err(MarketPressureStateEvolutionError::InvalidState)?;

    Ok(MarketPressureSampledIntervalTransition {
        terminal_state,
        integrated_side_imbalance,
        signed_balance_exposure,
        anchor_increment,
    })
}

fn sample_state_position<R>(probabilities: &[f64], randomness_stream: &mut R) -> usize
where
    R: SimulationRandomnessStream + ?Sized,
{
    let draw = randomness_stream.next_unit_interval_f64();

    let mut cumulative_probability = 0.0;

    for (state_position, probability) in probabilities.iter().copied().enumerate() {
        cumulative_probability += probability;

        if draw < cumulative_probability {
            return state_position;
        }
    }

    probabilities.len() - 1
}

fn state_exit_rate(transition_rates: &MarketPressureTransitionRates, state_position: usize) -> f64 {
    let upward_rate = transition_rates
        .upward_transition_rates
        .get(state_position)
        .copied()
        .unwrap_or(0.0);

    let downward_rate = state_position
        .checked_sub(1)
        .and_then(|position| transition_rates.downward_transition_rates.get(position))
        .copied()
        .unwrap_or(0.0);

    upward_rate + downward_rate
}

fn sample_next_state_position(
    transition_rates: &MarketPressureTransitionRates,
    state_position: usize,
    transition_draw: f64,
) -> usize {
    let upward_rate = transition_rates
        .upward_transition_rates
        .get(state_position)
        .copied()
        .unwrap_or(0.0);

    let downward_rate = state_position
        .checked_sub(1)
        .and_then(|position| transition_rates.downward_transition_rates.get(position))
        .copied()
        .unwrap_or(0.0);

    let exit_rate = upward_rate + downward_rate;

    if exit_rate == 0.0 {
        return state_position;
    }

    let upward_probability = upward_rate / exit_rate;
    if transition_draw < upward_probability {
        state_position + 1
    } else {
        state_position - 1
    }
}

fn sample_exponential_wait_time<R>(rate: f64, randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
{
    if rate <= 0.0 {
        return f64::INFINITY;
    }

    let unit_draw = randomness_stream
        .next_unit_interval_f64()
        .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);

    -unit_draw.ln() / rate
}
