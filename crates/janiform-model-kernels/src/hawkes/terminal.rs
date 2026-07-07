use super::parameters::HawkesKernelParameters;
use super::state::{
    HawkesEventSide, HawkesExcitationState, HawkesNewsSeverityMarkError, HawkesTransitionError,
};

/// Event-time weighted contribution to interval-end Hawkes excitation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HawkesIntervalTerminalStateContribution {
    /// Excitation contribution remaining at the interval boundary.
    pub state_contribution: HawkesExcitationState,
}

impl HawkesIntervalTerminalStateContribution {
    /// Construct a validated terminal-state contribution.
    pub fn try_new(
        state_contribution: HawkesExcitationState,
    ) -> Result<Self, HawkesTransitionError> {
        validate_hawkes_state_for_transition_contribution(state_contribution)?;

        Ok(Self { state_contribution })
    }

    /// Return the empty contribution.
    pub const fn empty() -> Self {
        Self {
            state_contribution: HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
        }
    }

    /// Add another terminal-state contribution.
    pub fn plus(self, other: Self) -> Result<Self, HawkesTransitionError> {
        let state_contribution = self
            .state_contribution
            .add_componentwise(other.state_contribution)
            .map_err(
                |source| HawkesTransitionError::InvalidTerminalStateContribution {
                    field: source.field,
                    value: source.value,
                },
            )?;

        Ok(Self { state_contribution })
    }
}

/// Compute an endogenous event's interval-end Hawkes contribution from its remaining interval time.
pub fn hawkes_endogenous_event_terminal_state_contribution_for_remaining_time(
    kernel_parameters: HawkesKernelParameters,
    event_side: HawkesEventSide,
    remaining_interval_delta: f64,
) -> Result<HawkesIntervalTerminalStateContribution, HawkesTransitionError> {
    validate_hawkes_terminal_contribution_inputs(kernel_parameters.beta, remaining_interval_delta)?;

    let decay = (-kernel_parameters.beta * remaining_interval_delta).exp();
    let alpha = kernel_parameters.alpha;

    let state_contribution = match event_side {
        HawkesEventSide::Buy => HawkesExcitationState {
            y_plus_plus: alpha.plus_plus * decay,
            y_plus_minus: 0.0,
            y_minus_plus: alpha.minus_plus * decay,
            y_minus_minus: 0.0,
        },
        HawkesEventSide::Sell => HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: alpha.plus_minus * decay,
            y_minus_plus: 0.0,
            y_minus_minus: alpha.minus_minus * decay,
        },
    };

    HawkesIntervalTerminalStateContribution::try_new(state_contribution)
}

/// Compute an exogenous news loading's interval-end Hawkes contribution.
pub fn hawkes_exogenous_news_terminal_state_contribution_for_remaining_time(
    kernel_parameters: HawkesKernelParameters,
    news_severity_mark: f64,
    remaining_interval_delta: f64,
) -> Result<HawkesIntervalTerminalStateContribution, HawkesTransitionError> {
    validate_hawkes_terminal_contribution_inputs(kernel_parameters.beta, remaining_interval_delta)?;

    if !news_severity_mark.is_finite() || news_severity_mark < 0.0 {
        return Err(HawkesTransitionError::InvalidNewsSeverityMark {
            source: HawkesNewsSeverityMarkError {
                value: news_severity_mark,
            },
        });
    }

    let decay = (-kernel_parameters.beta * remaining_interval_delta).exp();
    let rho = kernel_parameters.rho;

    HawkesIntervalTerminalStateContribution::try_new(HawkesExcitationState {
        y_plus_plus: rho.plus_plus * news_severity_mark * decay,
        y_plus_minus: rho.plus_minus * news_severity_mark * decay,
        y_minus_plus: rho.minus_plus * news_severity_mark * decay,
        y_minus_minus: rho.minus_minus * news_severity_mark * decay,
    })
}

/// Resolve interval-end Hawkes state from the initial state and terminal contributions.
pub fn resolve_hawkes_interval_terminal_state(
    initial_state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    interval_delta: f64,
    terminal_state_contribution: HawkesIntervalTerminalStateContribution,
) -> Result<HawkesExcitationState, HawkesTransitionError> {
    validate_hawkes_state_for_transition_initial_state(initial_state)?;
    validate_hawkes_terminal_contribution_inputs(kernel_parameters.beta, interval_delta)?;

    terminal_state_contribution
        .state_contribution
        .validate()
        .map_err(
            |source| HawkesTransitionError::InvalidTerminalStateContribution {
                field: source.field,
                value: source.value,
            },
        )?;

    let drifted_initial_state =
        initial_state.apply_shared_decay_drift(kernel_parameters.beta, interval_delta);

    let terminal_state = drifted_initial_state
        .add_componentwise(terminal_state_contribution.state_contribution)
        .map_err(|source| HawkesTransitionError::InvalidState {
            field: source.field,
            value: source.value,
        })?;

    Ok(terminal_state)
}

fn validate_hawkes_terminal_contribution_inputs(
    beta: f64,
    remaining_interval_delta: f64,
) -> Result<(), HawkesTransitionError> {
    if !remaining_interval_delta.is_finite() || remaining_interval_delta < 0.0 {
        return Err(HawkesTransitionError::InvalidDeltaT {
            value: remaining_interval_delta,
        });
    }

    if !beta.is_finite() || beta <= 0.0 {
        return Err(HawkesTransitionError::InvalidBeta { value: beta });
    }

    Ok(())
}

fn validate_hawkes_state_for_transition_initial_state(
    state: HawkesExcitationState,
) -> Result<(), HawkesTransitionError> {
    state
        .validate()
        .map_err(|source| HawkesTransitionError::InvalidState {
            field: source.field,
            value: source.value,
        })
}

fn validate_hawkes_state_for_transition_contribution(
    state: HawkesExcitationState,
) -> Result<(), HawkesTransitionError> {
    state.validate().map_err(
        |source| HawkesTransitionError::InvalidTerminalStateContribution {
            field: source.field,
            value: source.value,
        },
    )
}
