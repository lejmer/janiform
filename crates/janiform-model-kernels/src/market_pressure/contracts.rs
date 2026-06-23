use janiform_foundations::linear_algebra::dot_product;
use janiform_foundations::numeric::{
    validate_finite_number, validate_non_negative_number, validate_positive_number,
};
use serde::{Deserialize, Serialize};

use super::errors::{
    MarketPressureParametersValidationError, MarketPressureStateEvolutionError,
    MarketPressureStateValidationError, MarketPressureTransitionRatesValidationError,
};

/// Nearest-neighbor CTMC transition rates over ordered market-pressure support.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketPressureTransitionRates {
    /// Transition rates from state `i` to state `i + 1`.
    pub upward_transition_rates: Vec<f64>,
    /// Transition rates from state `i + 1` to state `i`.
    pub downward_transition_rates: Vec<f64>,
}

impl MarketPressureTransitionRates {
    /// Construct transition rates without validation.
    pub fn new(upward_transition_rates: Vec<f64>, downward_transition_rates: Vec<f64>) -> Self {
        Self {
            upward_transition_rates,
            downward_transition_rates,
        }
    }

    /// Return the transition-rate contract for a one-state neutral chain.
    pub const fn single_state() -> Self {
        Self {
            upward_transition_rates: Vec::new(),
            downward_transition_rates: Vec::new(),
        }
    }

    /// Validate transition rates for a support with `state_count` ordered states.
    pub fn validate_for_state_count(
        &self,
        state_count: usize,
    ) -> Result<(), MarketPressureTransitionRatesValidationError> {
        if state_count == 0 {
            return Err(MarketPressureTransitionRatesValidationError::EmptyStateSupport);
        }

        let expected_rate_count = state_count - 1;
        if self.upward_transition_rates.len() != expected_rate_count {
            return Err(
                MarketPressureTransitionRatesValidationError::InvalidUpwardRateCount {
                    expected: expected_rate_count,
                    actual: self.upward_transition_rates.len(),
                },
            );
        }

        if self.downward_transition_rates.len() != expected_rate_count {
            return Err(
                MarketPressureTransitionRatesValidationError::InvalidDownwardRateCount {
                    expected: expected_rate_count,
                    actual: self.downward_transition_rates.len(),
                },
            );
        }

        for (rate_position, rate) in self.upward_transition_rates.iter().enumerate() {
            validate_non_negative_number(*rate, "upward_transition_rates").map_err(|source| {
                MarketPressureTransitionRatesValidationError::InvalidUpwardRate {
                    rate_position,
                    value: source.value,
                }
            })?;
        }

        for (rate_position, rate) in self.downward_transition_rates.iter().enumerate() {
            validate_non_negative_number(*rate, "downward_transition_rates").map_err(|source| {
                MarketPressureTransitionRatesValidationError::InvalidDownwardRate {
                    rate_position,
                    value: source.value,
                }
            })?;
        }

        Ok(())
    }
}

/// Fitted parameters for latent side-balance market pressure.
///
/// The support values are ordered signed buy/sell imbalances in `[-1, 1]`. Conditional on an
/// event and state `k`, the buy-side probability is `(1 + side_imbalance_levels[k]) / 2`; the
/// sell-side probability is the complementary mass. The CTMC changes only the side composition
/// and does not change total event-arrival activity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketPressureParameters {
    /// Ordered side-balance support points.
    pub side_imbalance_levels: Vec<f64>,
    /// Birth-death transition rates for the latent side-balance CTMC.
    pub transition_rates: MarketPressureTransitionRates,
    /// Log-anchor drift loading per unit expected signed side-balance exposure.
    pub anchor_drift_per_signed_balance: f64,
}

impl MarketPressureParameters {
    /// Construct latent side-balance parameters from validated support and dynamics contracts.
    pub fn try_new(
        side_imbalance_levels: Vec<f64>,
        transition_rates: MarketPressureTransitionRates,
        anchor_drift_per_signed_balance: f64,
    ) -> Result<Self, MarketPressureParametersValidationError> {
        let parameters = Self {
            side_imbalance_levels,
            transition_rates,
            anchor_drift_per_signed_balance,
        };

        parameters.validate()?;

        Ok(parameters)
    }

    /// Return parameters whose market pressure has no side or anchor effect.
    pub fn no_anchor_impact() -> Self {
        Self {
            side_imbalance_levels: vec![0.0],
            transition_rates: MarketPressureTransitionRates::single_state(),
            anchor_drift_per_signed_balance: 0.0,
        }
    }

    /// Return the number of latent pressure states.
    pub fn state_count(&self) -> usize {
        self.side_imbalance_levels.len()
    }

    /// Validate the latent side-balance parameter contract.
    pub fn validate(&self) -> Result<(), MarketPressureParametersValidationError> {
        if self.side_imbalance_levels.is_empty() {
            return Err(MarketPressureParametersValidationError::EmptySideImbalanceSupport);
        }

        let mut previous_level = None;
        for (state_position, level) in self.side_imbalance_levels.iter().copied().enumerate() {
            validate_finite_number(level, "side_imbalance_levels").map_err(|source| {
                MarketPressureParametersValidationError::InvalidSideImbalanceLevel {
                    state_position,
                    value: source.value,
                }
            })?;

            if !(-1.0..=1.0).contains(&level) {
                return Err(
                    MarketPressureParametersValidationError::SideImbalanceLevelOutOfRange {
                        state_position,
                        value: level,
                    },
                );
            }

            if let Some(previous_level) = previous_level
                && level <= previous_level
            {
                return Err(
                    MarketPressureParametersValidationError::SideImbalanceLevelsNotIncreasing {
                        previous_state_position: state_position - 1,
                        previous_value: previous_level,
                        state_position,
                        value: level,
                    },
                );
            }

            previous_level = Some(level);
        }

        let state_count = self.state_count();

        self.transition_rates
            .validate_for_state_count(state_count)
            .map_err(|source| MarketPressureParametersValidationError::InvalidTransitionRates {
                source,
            })?;

        validate_finite_number(
            self.anchor_drift_per_signed_balance,
            "anchor_drift_per_signed_balance",
        )
        .map_err(|source| {
            MarketPressureParametersValidationError::InvalidAnchorDriftPerSignedBalance {
                value: source.value,
            }
        })?;

        Ok(())
    }
}

/// Boundary value for latent side-balance state weights.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketPressureState {
    /// Non-negative finite posterior weights over `MarketPressureParameters::side_imbalance_levels`.
    pub state_weights: Vec<f64>,
}

impl MarketPressureState {
    /// Construct a validated market-pressure boundary state from non-negative state weights.
    pub fn try_new(state_weights: Vec<f64>) -> Result<Self, MarketPressureStateValidationError> {
        let state = Self { state_weights };

        state.validate()?;

        Ok(state)
    }

    /// Return a one-state neutral market-pressure boundary.
    pub fn neutral() -> Self {
        Self {
            state_weights: vec![1.0],
        }
    }

    /// Return a neutral prior for the supplied side-imbalance support.
    ///
    /// If the support contains or brackets zero, the returned state has zero expected side
    /// imbalance. If zero is outside the support, the prior is concentrated on the support point
    /// closest to zero because the fitted support cannot represent a zero side-balance state.
    pub fn neutral_for_parameters(
        parameters: &MarketPressureParameters,
    ) -> Result<Self, MarketPressureStateValidationError> {
        parameters
            .validate()
            .map_err(MarketPressureStateValidationError::InvalidParameters)?;

        let state_count = parameters.state_count();
        if state_count == 1 {
            return Self::from_state_position(state_count, 0);
        }

        if let Some(zero_state_position) = parameters
            .side_imbalance_levels
            .iter()
            .position(|side_imbalance| *side_imbalance == 0.0)
        {
            return Self::from_state_position(state_count, zero_state_position);
        }

        for (upper_state_position, upper_side_imbalance) in
            parameters.side_imbalance_levels.iter().copied().enumerate()
        {
            if upper_side_imbalance <= 0.0 {
                continue;
            }

            if upper_state_position == 0 {
                return Self::from_state_position(state_count, upper_state_position);
            }

            let lower_state_position = upper_state_position - 1;
            let lower_side_imbalance = parameters.side_imbalance_levels[lower_state_position];

            let support_span = upper_side_imbalance - lower_side_imbalance;

            let lower_weight = upper_side_imbalance / support_span;
            let upper_weight = -lower_side_imbalance / support_span;

            let mut state_weights = vec![0.0; state_count];

            state_weights[lower_state_position] = lower_weight;
            state_weights[upper_state_position] = upper_weight;

            return Self::try_new(state_weights);
        }

        Self::from_state_position(state_count, state_count - 1)
    }

    /// Return a deterministic boundary concentrated on `state_position`.
    pub fn from_state_position(
        state_count: usize,
        state_position: usize,
    ) -> Result<Self, MarketPressureStateValidationError> {
        if state_count == 0 {
            return Err(MarketPressureStateValidationError::EmptyStateWeights);
        }

        if state_position >= state_count {
            return Err(
                MarketPressureStateValidationError::StatePositionOutOfRange {
                    state_position,
                    state_count,
                },
            );
        }

        let mut state_weights = vec![0.0; state_count];
        state_weights[state_position] = 1.0;

        Self::try_new(state_weights)
    }

    /// Validate the market-pressure state contract.
    pub fn validate(&self) -> Result<(), MarketPressureStateValidationError> {
        if self.state_weights.is_empty() {
            return Err(MarketPressureStateValidationError::EmptyStateWeights);
        }

        for (state_position, weight) in self.state_weights.iter().copied().enumerate() {
            validate_non_negative_number(weight, "state_weights").map_err(|source| {
                MarketPressureStateValidationError::InvalidStateWeight {
                    state_position,
                    value: source.value,
                }
            })?;
        }

        let weight_sum = self.state_weight_sum();

        validate_positive_number(weight_sum, "state_weight_sum").map_err(|source| {
            MarketPressureStateValidationError::InvalidStateWeightSum {
                value: source.value,
            }
        })?;

        Ok(())
    }

    /// Validate that state weights match the parameter support size.
    pub fn validate_for_parameters(
        &self,
        parameters: &MarketPressureParameters,
    ) -> Result<(), MarketPressureStateValidationError> {
        self.validate()?;

        let expected_state_count = parameters.state_count();
        let actual_state_count = self.state_weights.len();
        if actual_state_count != expected_state_count {
            return Err(MarketPressureStateValidationError::StateCountMismatch {
                expected_state_count,
                actual_state_count,
            });
        }

        Ok(())
    }

    /// Return normalized probabilities in state-support order.
    pub fn normalized_probabilities(&self) -> Result<Vec<f64>, MarketPressureStateValidationError> {
        self.validate()?;

        let weight_sum = self.state_weight_sum();

        Ok(self
            .state_weights
            .iter()
            .map(|weight| *weight / weight_sum)
            .collect())
    }

    /// Return expected signed side imbalance under this state and `parameters`.
    pub fn expected_side_imbalance(
        &self,
        parameters: &MarketPressureParameters,
    ) -> Result<f64, MarketPressureStateEvolutionError> {
        self.validate_for_parameters(parameters)
            .map_err(MarketPressureStateEvolutionError::InvalidState)?;

        let probabilities = self
            .normalized_probabilities()
            .map_err(MarketPressureStateEvolutionError::InvalidState)?;

        let expected_side_imbalance =
            dot_product(&probabilities, &parameters.side_imbalance_levels).ok_or({
                MarketPressureStateEvolutionError::InvalidState(
                    MarketPressureStateValidationError::StateCountMismatch {
                        expected_state_count: parameters.side_imbalance_levels.len(),
                        actual_state_count: probabilities.len(),
                    },
                )
            })?;
        validate_finite_number(expected_side_imbalance, "expected_side_imbalance").map_err(
            |source| MarketPressureStateEvolutionError::InvalidExpectedSideImbalance {
                value: source.value,
            },
        )?;

        Ok(expected_side_imbalance)
    }

    fn state_weight_sum(&self) -> f64 {
        self.state_weights.iter().sum()
    }
}
