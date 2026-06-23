use janiform_foundations::linear_algebra::dot_product;
use janiform_foundations::numeric::{validate_finite_number, validate_non_negative_number};
use nalgebra::{DMatrix, DVector};

use super::contracts::{
    MarketPressureParameters, MarketPressureState, MarketPressureTransitionRates,
};
use super::errors::{
    MarketPressureCtmcTransitionError, MarketPressureStateEvolutionError,
    MarketPressureStateValidationError,
};
use super::interval::{
    MarketPressureIntervalTransition, MarketPressureIntervalTransitionInput,
    MarketPressureIntervalTransitionMoments,
};

/// Resolve conditional first moments for the latent side-balance CTMC interval.
pub fn market_pressure_interval_transition_moments(
    input: MarketPressureIntervalTransitionInput<'_>,
) -> Result<MarketPressureIntervalTransitionMoments, MarketPressureStateEvolutionError> {
    validate_transition_input(&input)?;

    let probabilities = input
        .initial_state
        .normalized_probabilities()
        .map_err(MarketPressureStateEvolutionError::InvalidState)?;

    let (terminal_probabilities, integrated_state_probabilities) =
        evolve_probability_vector_and_integral(
            &input.parameters.transition_rates,
            probabilities,
            input.interval_delta,
        )
        .map_err(MarketPressureStateEvolutionError::InvalidCtmcTransition)?;

    let terminal_state = MarketPressureState::try_new(terminal_probabilities)
        .map_err(MarketPressureStateEvolutionError::InvalidState)?;

    let integrated_side_imbalance = dot_product(
        &integrated_state_probabilities,
        &input.parameters.side_imbalance_levels,
    )
    .ok_or({
        MarketPressureStateEvolutionError::InvalidState(
            MarketPressureStateValidationError::StateCountMismatch {
                expected_state_count: input.parameters.side_imbalance_levels.len(),
                actual_state_count: integrated_state_probabilities.len(),
            },
        )
    })?;

    validate_finite_number(integrated_side_imbalance, "integrated_side_imbalance").map_err(
        |source| MarketPressureStateEvolutionError::InvalidIntegratedSideImbalance {
            value: source.value,
        },
    )?;

    let signed_balance_exposure_mean =
        integrated_side_imbalance * input.signed_balance_exposure_scale;

    validate_finite_number(signed_balance_exposure_mean, "signed_balance_exposure_mean").map_err(
        |source| MarketPressureStateEvolutionError::InvalidSignedBalanceExposure {
            value: source.value,
        },
    )?;

    let anchor_increment_mean =
        input.parameters.anchor_drift_per_signed_balance * signed_balance_exposure_mean;

    validate_finite_number(anchor_increment_mean, "anchor_increment_mean").map_err(|source| {
        MarketPressureStateEvolutionError::InvalidAnchorIncrement {
            value: source.value,
        }
    })?;

    Ok(MarketPressureIntervalTransitionMoments {
        terminal_state_mean: terminal_state,
        integrated_side_imbalance_mean: integrated_side_imbalance,
        signed_balance_exposure_mean,
        anchor_increment_mean,
    })
}

/// Resolve terminal state, signed exposure, and anchor increment for the conditional mean path.
pub fn resolve_market_pressure_interval_transition(
    input: MarketPressureIntervalTransitionInput<'_>,
) -> Result<MarketPressureIntervalTransition, MarketPressureStateEvolutionError> {
    let moments = market_pressure_interval_transition_moments(input)?;

    Ok(MarketPressureIntervalTransition {
        terminal_state: moments.terminal_state_mean,
        integrated_side_imbalance: moments.integrated_side_imbalance_mean,
        signed_balance_exposure: moments.signed_balance_exposure_mean,
        anchor_increment: moments.anchor_increment_mean,
    })
}

/// Evolve the latent side-balance state over a non-negative interval.
pub fn evolve_market_pressure_state(
    parameters: &MarketPressureParameters,
    state: &MarketPressureState,
    interval_delta: f64,
) -> Result<MarketPressureState, MarketPressureStateEvolutionError> {
    resolve_market_pressure_interval_transition(MarketPressureIntervalTransitionInput {
        parameters,
        initial_state: state,
        interval_delta,
        signed_balance_exposure_scale: 1.0,
    })
    .map(|transition| transition.terminal_state)
}

/// Compute the conditional mean of the integrated signed side imbalance.
pub fn integrate_market_pressure_side_imbalance_mean(
    parameters: &MarketPressureParameters,
    state: &MarketPressureState,
    interval_delta: f64,
) -> Result<f64, MarketPressureStateEvolutionError> {
    resolve_market_pressure_interval_transition(MarketPressureIntervalTransitionInput {
        parameters,
        initial_state: state,
        interval_delta,
        signed_balance_exposure_scale: 1.0,
    })
    .map(|transition| transition.integrated_side_imbalance)
}

/// Project the conditional mean side-balance anchor increment over an interval.
pub fn project_market_pressure_anchor_increment_mean(
    parameters: &MarketPressureParameters,
    state: &MarketPressureState,
    interval_delta: f64,
    signed_balance_exposure_scale: f64,
) -> Result<f64, MarketPressureStateEvolutionError> {
    resolve_market_pressure_interval_transition(MarketPressureIntervalTransitionInput {
        parameters,
        initial_state: state,
        interval_delta,
        signed_balance_exposure_scale,
    })
    .map(|transition| transition.anchor_increment)
}

/// Return the conditional buy-side probability implied by a side-imbalance value.
pub fn buy_probability_from_side_imbalance(
    side_imbalance: f64,
) -> Result<f64, MarketPressureStateEvolutionError> {
    validate_finite_number(side_imbalance, "side_imbalance").map_err(|source| {
        MarketPressureStateEvolutionError::InvalidExpectedSideImbalance {
            value: source.value,
        }
    })?;

    if !(-1.0..=1.0).contains(&side_imbalance) {
        return Err(
            MarketPressureStateEvolutionError::ExpectedSideImbalanceOutOfRange {
                value: side_imbalance,
            },
        );
    }

    Ok(0.5 * (1.0 + side_imbalance))
}

/// Condition state weights on one observed buy/sell event side.
pub fn condition_market_pressure_state_on_event_side(
    parameters: &MarketPressureParameters,
    state: &MarketPressureState,
    observed_buy_side: bool,
) -> Result<MarketPressureState, MarketPressureStateEvolutionError> {
    parameters
        .validate()
        .map_err(MarketPressureStateEvolutionError::InvalidParameters)?;

    state
        .validate_for_parameters(parameters)
        .map_err(MarketPressureStateEvolutionError::InvalidState)?;

    let probabilities = state
        .normalized_probabilities()
        .map_err(MarketPressureStateEvolutionError::InvalidState)?;

    let mut likelihood_weighted_probabilities = Vec::with_capacity(probabilities.len());
    let mut likelihood_sum = 0.0;

    for (state_probability, &side_imbalance) in probabilities
        .iter()
        .zip(parameters.side_imbalance_levels.iter())
    {
        let buy_probability = buy_probability_from_side_imbalance(side_imbalance)?;

        let side_likelihood = if observed_buy_side {
            buy_probability
        } else {
            1.0 - buy_probability
        };

        let likelihood_weighted_probability = state_probability * side_likelihood;

        validate_non_negative_number(
            likelihood_weighted_probability,
            "likelihood_weighted_probability",
        )
        .map_err(|source| {
            MarketPressureStateEvolutionError::InvalidObservationLikelihood {
                value: source.value,
            }
        })?;

        likelihood_sum += likelihood_weighted_probability;
        likelihood_weighted_probabilities.push(likelihood_weighted_probability);
    }

    if !likelihood_sum.is_finite() || likelihood_sum <= 0.0 {
        return Err(
            MarketPressureStateEvolutionError::InvalidObservationLikelihood {
                value: likelihood_sum,
            },
        );
    }

    let normalized_probabilities = likelihood_weighted_probabilities
        .into_iter()
        .map(|probability| probability / likelihood_sum)
        .collect::<Vec<_>>();

    MarketPressureState::try_new(normalized_probabilities)
        .map_err(MarketPressureStateEvolutionError::InvalidState)
}

/// Combine buy- and sell-side exposure into the total scale used for signed side balance.
pub fn signed_balance_exposure_scale_from_side_exposures(
    buy_side_exposure: f64,
    sell_side_exposure: f64,
) -> Result<f64, MarketPressureStateEvolutionError> {
    validate_non_negative_number(buy_side_exposure, "buy_side_exposure").map_err(|source| {
        MarketPressureStateEvolutionError::InvalidSignedBalanceExposureScale {
            value: source.value,
        }
    })?;

    validate_non_negative_number(sell_side_exposure, "sell_side_exposure").map_err(|source| {
        MarketPressureStateEvolutionError::InvalidSignedBalanceExposureScale {
            value: source.value,
        }
    })?;

    let exposure_scale = buy_side_exposure + sell_side_exposure;

    validate_non_negative_number(exposure_scale, "signed_balance_exposure_scale").map_err(
        |source| MarketPressureStateEvolutionError::InvalidSignedBalanceExposureScale {
            value: source.value,
        },
    )?;

    Ok(exposure_scale)
}

/// Split total side exposure according to a signed side-imbalance value.
pub fn side_exposures_from_total_and_imbalance(
    total_side_exposure: f64,
    side_imbalance: f64,
) -> Result<(f64, f64), MarketPressureStateEvolutionError> {
    validate_non_negative_number(total_side_exposure, "total_side_exposure").map_err(|source| {
        MarketPressureStateEvolutionError::InvalidSignedBalanceExposureScale {
            value: source.value,
        }
    })?;

    let buy_probability = buy_probability_from_side_imbalance(side_imbalance)?;
    let buy_side_exposure = total_side_exposure * buy_probability;
    let sell_side_exposure = total_side_exposure - buy_side_exposure;

    validate_non_negative_number(buy_side_exposure, "buy_side_exposure").map_err(|source| {
        MarketPressureStateEvolutionError::InvalidSignedBalanceExposureScale {
            value: source.value,
        }
    })?;

    validate_non_negative_number(sell_side_exposure, "sell_side_exposure").map_err(|source| {
        MarketPressureStateEvolutionError::InvalidSignedBalanceExposureScale {
            value: source.value,
        }
    })?;

    Ok((buy_side_exposure, sell_side_exposure))
}

pub(crate) fn validate_transition_input(
    input: &MarketPressureIntervalTransitionInput<'_>,
) -> Result<(), MarketPressureStateEvolutionError> {
    input
        .parameters
        .validate()
        .map_err(MarketPressureStateEvolutionError::InvalidParameters)?;

    input
        .initial_state
        .validate_for_parameters(input.parameters)
        .map_err(MarketPressureStateEvolutionError::InvalidState)?;

    validate_non_negative_number(input.interval_delta, "interval_delta").map_err(|source| {
        MarketPressureStateEvolutionError::InvalidIntervalDelta {
            value: source.value,
        }
    })?;

    validate_non_negative_number(
        input.signed_balance_exposure_scale,
        "signed_balance_exposure_scale",
    )
    .map_err(|source| {
        MarketPressureStateEvolutionError::InvalidSignedBalanceExposureScale {
            value: source.value,
        }
    })?;

    Ok(())
}

fn evolve_probability_vector_and_integral(
    transition_rates: &MarketPressureTransitionRates,
    initial_probabilities: Vec<f64>,
    interval_delta: f64,
) -> Result<(Vec<f64>, Vec<f64>), MarketPressureCtmcTransitionError> {
    let state_count = initial_probabilities.len();
    let generator = build_column_generator_matrix(transition_rates, state_count)?;

    if interval_delta == 0.0 {
        return Ok((initial_probabilities, vec![0.0; state_count]));
    }

    let mut augmented_generator = DMatrix::<f64>::zeros(2 * state_count, 2 * state_count);

    for row_position in 0..state_count {
        for column_position in 0..state_count {
            let value = generator[(row_position, column_position)];

            augmented_generator[(row_position, column_position)] = value;
            augmented_generator[(row_position + state_count, column_position + state_count)] =
                value;
        }

        augmented_generator[(row_position + state_count, row_position)] = 1.0;
    }

    let transition = (augmented_generator * interval_delta).exp();

    let mut initial = DVector::<f64>::zeros(2 * state_count);

    for (state_position, probability) in initial_probabilities.iter().copied().enumerate() {
        initial[state_position] = probability;
    }

    let output = transition * initial;

    let terminal_probabilities = output.rows(0, state_count).iter().copied().collect();
    let integrated_state_probabilities = output
        .rows(state_count, state_count)
        .iter()
        .copied()
        .collect();

    Ok((terminal_probabilities, integrated_state_probabilities))
}

fn build_column_generator_matrix(
    transition_rates: &MarketPressureTransitionRates,
    state_count: usize,
) -> Result<DMatrix<f64>, MarketPressureCtmcTransitionError> {
    transition_rates
        .validate_for_state_count(state_count)
        .map_err(|source| MarketPressureCtmcTransitionError::InvalidTransitionRates { source })?;

    let mut generator = DMatrix::<f64>::zeros(state_count, state_count);

    for state_position in 0..state_count {
        let mut exit_rate = 0.0;

        if state_position + 1 < state_count {
            let upward_rate = transition_rates.upward_transition_rates[state_position];

            generator[(state_position + 1, state_position)] = upward_rate;

            exit_rate += upward_rate;
        }

        if state_position > 0 {
            let downward_rate = transition_rates.downward_transition_rates[state_position - 1];

            generator[(state_position - 1, state_position)] = downward_rate;

            exit_rate += downward_rate;
        }

        generator[(state_position, state_position)] = -exit_rate;
    }

    Ok(generator)
}
