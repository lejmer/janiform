use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::numeric::{FiniteNumberValidationError, PositiveNumberValidationError};
use janiform_foundations::randomness::{SimulationRandomnessRng, SimulationRandomnessStream};
use janiform_model_kernels::hawkes::HawkesEventSide;
use janiform_model_kernels::long_variance::{
    LongVarianceParameters, LongVarianceParametersError, LongVarianceState, LongVarianceStateError,
    LongVarianceStateTransitionError, return_energy_increment_from_log_return,
};
use rand::distributions::Distribution;
use statrs::distribution::{ChiSquared, Poisson};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LongVarianceTransitionSample {
    pub(crate) baseline_clock_scale: f64,
    pub(crate) terminal_state: LongVarianceState,
}

pub(crate) fn sample_long_variance_transition<R>(
    state: LongVarianceState,
    parameters: LongVarianceParameters,
    stationary_mean_reversion_level: f64,
    elapsed_hours: f64,
    randomness_stream: &mut R,
) -> Result<LongVarianceTransitionSample, LongVarianceTransitionSamplingError>
where
    R: SimulationRandomnessStream,
{
    state
        .validate()
        .map_err(|source| LongVarianceTransitionSamplingError::InvalidInitialState { source })?;

    parameters.validate().map_err(|source| {
        LongVarianceTransitionSamplingError::InvalidLongVarianceParameters { source }
    })?;

    janiform_foundations::numeric::validate_positive_number(
        stationary_mean_reversion_level,
        "stationary_mean_reversion_level",
    )
    .map_err(|source| {
        LongVarianceTransitionSamplingError::InvalidStationaryMeanReversionLevel { source }
    })?;

    janiform_foundations::numeric::validate_finite_number(elapsed_hours, "elapsed_hours")
        .map_err(|source| LongVarianceTransitionSamplingError::InvalidElapsedHours { source })?;

    if elapsed_hours < 0.0 {
        return Err(LongVarianceTransitionSamplingError::NegativeElapsedHours {
            value: elapsed_hours,
        });
    }

    let baseline_clock_scale = state
        .baseline_clock_scale(parameters, stationary_mean_reversion_level, elapsed_hours)
        .map_err(|source| LongVarianceTransitionSamplingError::StateTransitionFailure { source })?;

    let terminal_state = sample_cir_terminal_state(
        state,
        parameters,
        stationary_mean_reversion_level,
        elapsed_hours,
        randomness_stream,
    )?;

    Ok(LongVarianceTransitionSample {
        baseline_clock_scale,
        terminal_state,
    })
}

pub(crate) fn apply_long_variance_event_jump(
    state: LongVarianceState,
    parameters: LongVarianceParameters,
    event_side: HawkesEventSide,
) -> Result<LongVarianceState, LongVarianceTransitionSamplingError> {
    state
        .apply_event_jump(parameters, event_side)
        .map_err(|source| LongVarianceTransitionSamplingError::StateTransitionFailure { source })
}

pub(crate) fn apply_long_variance_return_energy_jump(
    state: LongVarianceState,
    parameters: LongVarianceParameters,
    return_energy_increment: f64,
) -> Result<LongVarianceState, LongVarianceTransitionSamplingError> {
    state
        .apply_return_energy_jump(parameters, return_energy_increment)
        .map_err(|source| LongVarianceTransitionSamplingError::StateTransitionFailure { source })
}

pub(crate) fn return_energy_increment_from_generated_log_return(
    generated_log_return: f64,
) -> Result<f64, LongVarianceTransitionSamplingError> {
    return_energy_increment_from_log_return(generated_log_return)
        .map_err(|source| LongVarianceTransitionSamplingError::StateTransitionFailure { source })
}

pub(crate) fn apply_long_variance_event_count_jumps(
    state: LongVarianceState,
    parameters: LongVarianceParameters,
    buy_event_count: u64,
    sell_event_count: u64,
) -> Result<LongVarianceState, LongVarianceTransitionSamplingError> {
    state
        .validate()
        .map_err(|source| LongVarianceTransitionSamplingError::InvalidInitialState { source })?;

    parameters.validate().map_err(|source| {
        LongVarianceTransitionSamplingError::InvalidLongVarianceParameters { source }
    })?;

    let terminal_level = state.level
        + parameters.buy_event_loading * buy_event_count as f64
        + parameters.sell_event_loading * sell_event_count as f64;

    LongVarianceState::try_new(terminal_level)
        .map_err(|source| LongVarianceTransitionSamplingError::InvalidTerminalState { source })
}

fn sample_cir_terminal_state<R>(
    state: LongVarianceState,
    parameters: LongVarianceParameters,
    stationary_mean_reversion_level: f64,
    elapsed_hours: f64,
    randomness_stream: &mut R,
) -> Result<LongVarianceState, LongVarianceTransitionSamplingError>
where
    R: SimulationRandomnessStream,
{
    if elapsed_hours == 0.0 || parameters.vol_of_vol == 0.0 {
        return state
            .apply_mean_reversion_drift(parameters, stationary_mean_reversion_level, elapsed_hours)
            .map_err(
                |source| LongVarianceTransitionSamplingError::StateTransitionFailure { source },
            );
    }

    let mean_reversion_rate = parameters.mean_reversion_rate;
    let volatility_squared = parameters.vol_of_vol * parameters.vol_of_vol;
    let decay = (-mean_reversion_rate * elapsed_hours).exp();
    let one_minus_decay = 1.0 - decay;

    if one_minus_decay == 0.0 {
        return state
            .apply_mean_reversion_drift(parameters, stationary_mean_reversion_level, elapsed_hours)
            .map_err(
                |source| LongVarianceTransitionSamplingError::StateTransitionFailure { source },
            );
    }

    // The CIR endpoint law is a scaled noncentral chi-squared distribution.
    let scale = volatility_squared * one_minus_decay / (4.0 * mean_reversion_rate);

    let degrees_of_freedom =
        4.0 * mean_reversion_rate * stationary_mean_reversion_level / volatility_squared;

    let noncentrality =
        4.0 * mean_reversion_rate * decay * state.level / (volatility_squared * one_minus_decay);

    let poisson_count = sample_poisson_count(0.5 * noncentrality, randomness_stream)?;
    let chi_squared_freedom = degrees_of_freedom + 2.0 * poisson_count as f64;

    let chi_squared = ChiSquared::new(chi_squared_freedom).map_err(|source| {
        LongVarianceTransitionSamplingError::DistributionBuildFailure {
            distribution_name: "chi_squared",
            source: source.to_string(),
        }
    })?;

    let mut rng = SimulationRandomnessRng::new(randomness_stream);

    let terminal_level = scale * chi_squared.sample(&mut rng);

    if !terminal_level.is_finite() {
        return Err(
            LongVarianceTransitionSamplingError::InvalidDistributionDraw {
                distribution_name: "chi_squared",
                value: terminal_level,
            },
        );
    }

    LongVarianceState::try_new(terminal_level)
        .map_err(|source| LongVarianceTransitionSamplingError::InvalidTerminalState { source })
}

fn sample_poisson_count<R>(
    intensity: f64,
    randomness_stream: &mut R,
) -> Result<u64, LongVarianceTransitionSamplingError>
where
    R: SimulationRandomnessStream,
{
    if intensity == 0.0 {
        return Ok(0);
    }

    let poisson = Poisson::new(intensity).map_err(|source| {
        LongVarianceTransitionSamplingError::DistributionBuildFailure {
            distribution_name: "poisson",
            source: source.to_string(),
        }
    })?;

    let mut rng = SimulationRandomnessRng::new(randomness_stream);

    let sampled_count: u64 = poisson.sample(&mut rng);

    Ok(sampled_count)
}

/// Long-variance transition failure raised by seeded or coarse simulation.
#[derive(Debug, Clone, PartialEq)]
pub enum LongVarianceTransitionSamplingError {
    InvalidInitialState {
        source: LongVarianceStateError,
    },
    InvalidLongVarianceParameters {
        source: LongVarianceParametersError,
    },
    InvalidStationaryMeanReversionLevel {
        source: PositiveNumberValidationError,
    },
    InvalidElapsedHours {
        source: FiniteNumberValidationError,
    },
    NegativeElapsedHours {
        value: f64,
    },
    DistributionBuildFailure {
        distribution_name: &'static str,
        source: String,
    },
    InvalidDistributionDraw {
        distribution_name: &'static str,
        value: f64,
    },
    InvalidTerminalState {
        source: LongVarianceStateError,
    },
    StateTransitionFailure {
        source: LongVarianceStateTransitionError,
    },
}

impl Display for LongVarianceTransitionSamplingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInitialState { source } => {
                write!(
                    formatter,
                    "initial long-variance state is invalid: {source}"
                )
            }
            Self::InvalidLongVarianceParameters { source } => {
                write!(formatter, "long-variance parameters are invalid: {source}")
            }
            Self::InvalidStationaryMeanReversionLevel { source } => write!(
                formatter,
                "stationary_mean_reversion_level is invalid: {source}"
            ),
            Self::InvalidElapsedHours { source } => {
                write!(formatter, "elapsed_hours is invalid: {source}")
            }
            Self::NegativeElapsedHours { value } => {
                write!(
                    formatter,
                    "elapsed_hours must be non-negative, found {value}"
                )
            }
            Self::DistributionBuildFailure {
                distribution_name,
                source,
            } => write!(
                formatter,
                "failed to build long-variance {distribution_name} distribution: {source}"
            ),
            Self::InvalidDistributionDraw {
                distribution_name,
                value,
            } => write!(
                formatter,
                "long-variance {distribution_name} distribution produced invalid draw {value}"
            ),
            Self::InvalidTerminalState { source } => {
                write!(
                    formatter,
                    "terminal long-variance state is invalid: {source}"
                )
            }
            Self::StateTransitionFailure { source } => {
                write!(formatter, "long-variance state transition failed: {source}")
            }
        }
    }
}

impl Error for LongVarianceTransitionSamplingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidInitialState { source } => Some(source),
            Self::InvalidLongVarianceParameters { source } => Some(source),
            Self::InvalidStationaryMeanReversionLevel { source } => Some(source),
            Self::InvalidElapsedHours { source } => Some(source),
            Self::InvalidTerminalState { source } => Some(source),
            Self::StateTransitionFailure { source } => Some(source),
            Self::NegativeElapsedHours { .. }
            | Self::DistributionBuildFailure { .. }
            | Self::InvalidDistributionDraw { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
    use janiform_model_kernels::long_variance::{LongVarianceParameters, LongVarianceState};

    use super::{
        LongVarianceTransitionSample, apply_long_variance_event_count_jumps,
        return_energy_increment_from_generated_log_return, sample_long_variance_transition,
    };

    #[test]
    fn neutral_long_variance_transition_keeps_unit_clock_and_state() {
        let mut stream = SplitMix64SimulationRandomnessStream::from_seed(7);

        let transition = sample_long_variance_transition(
            LongVarianceState::unit(),
            LongVarianceParameters::neutral(),
            1.0,
            12.0,
            &mut stream,
        )
        .expect("neutral transition should sample");

        assert_eq!(
            transition,
            LongVarianceTransitionSample {
                baseline_clock_scale: 1.0,
                terminal_state: LongVarianceState::unit(),
            }
        );
    }

    #[test]
    fn elevated_initial_state_increases_baseline_clock_scale() {
        let mut stream = SplitMix64SimulationRandomnessStream::from_seed(11);
        let parameters = LongVarianceParameters::try_new(2.0, 0.0, 0.0, 0.0, 0.0, 0.0)
            .expect("parameters should build");

        let transition = sample_long_variance_transition(
            LongVarianceState::try_new(2.0).expect("state should build"),
            parameters,
            1.0,
            1.0,
            &mut stream,
        )
        .expect("transition should sample");

        assert!(transition.baseline_clock_scale > 1.0);
        assert!(transition.terminal_state.level > 1.0);
    }

    #[test]
    fn event_count_jumps_apply_side_specific_loadings() {
        let parameters = LongVarianceParameters::try_new(2.0, 0.0, 0.0, 0.0, 0.25, 0.5)
            .expect("parameters should build");

        let state =
            apply_long_variance_event_count_jumps(LongVarianceState::unit(), parameters, 2, 3)
                .expect("event count jumps should apply");

        assert_eq!(state.level, 1.0 + 2.0 * 0.25 + 3.0 * 0.5);
    }

    #[test]
    fn generated_return_energy_uses_squared_log_return_contract() {
        assert_eq!(
            return_energy_increment_from_generated_log_return(-0.25)
                .expect("finite generated return should produce energy"),
            0.0625
        );
    }
}
