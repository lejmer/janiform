use std::error::Error;
use std::fmt::{Display, Formatter};

use super::contracts::{MeanReversionBoundaryState, MeanReversionBoundaryStateValidationError};

#[derive(Debug, Clone, PartialEq)]
pub enum MeanReversionBoundaryStateEvolutionError {
    InvalidBoundaryState {
        source: MeanReversionBoundaryStateValidationError,
    },
    InvalidDeviation {
        value: f64,
    },
    InvalidDeviationDecayFactor {
        value: f64,
    },
    InvalidKappa {
        value: f64,
    },
    InvalidIntervalDelta {
        value: f64,
    },
    InvalidCoreTotalReturn {
        value: f64,
    },
    InvalidIntervalEndAnchorValue {
        value: f64,
    },
    InvalidCurrentAnchorValue {
        value: f64,
    },
    InvalidEvolvedDeviation {
        value: f64,
    },
}

impl Display for MeanReversionBoundaryStateEvolutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBoundaryState { source } => {
                write!(formatter, "boundary_state is invalid: {source}")
            }
            Self::InvalidDeviation { value } => {
                write!(
                    formatter,
                    "deviation must be a finite number, found {value}"
                )
            }
            Self::InvalidDeviationDecayFactor { value } => write!(
                formatter,
                "deviation_decay_factor must be a finite non-negative number, found {value}"
            ),
            Self::InvalidKappa { value } => {
                write!(
                    formatter,
                    "kappa must be a finite non-negative number, found {value}"
                )
            }
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be a finite strictly positive number, found {value}"
            ),
            Self::InvalidCoreTotalReturn { value } => write!(
                formatter,
                "core_total_return must be a finite number, found {value}"
            ),
            Self::InvalidIntervalEndAnchorValue { value } => write!(
                formatter,
                "interval_end_anchor_value must be a finite number, found {value}"
            ),
            Self::InvalidCurrentAnchorValue { value } => write!(
                formatter,
                "current_anchor_value must be a finite number, found {value}"
            ),
            Self::InvalidEvolvedDeviation { value } => write!(
                formatter,
                "evolved deviation must be a finite number, found {value}"
            ),
        }
    }
}

impl Error for MeanReversionBoundaryStateEvolutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidBoundaryState { source } => Some(source),
            Self::InvalidDeviation { .. }
            | Self::InvalidDeviationDecayFactor { .. }
            | Self::InvalidKappa { .. }
            | Self::InvalidIntervalDelta { .. }
            | Self::InvalidCoreTotalReturn { .. }
            | Self::InvalidIntervalEndAnchorValue { .. }
            | Self::InvalidCurrentAnchorValue { .. }
            | Self::InvalidEvolvedDeviation { .. } => None,
        }
    }
}

/// Apply the canonical mean-reversion deviation recursion with a precomputed decay factor.
pub fn evolve_mean_reversion_deviation_with_decay_on_interval_grid(
    deviation: f64,
    deviation_decay_factor: f64,
    core_total_return: f64,
    current_anchor_value: f64,
    interval_end_anchor_value: f64,
) -> Result<f64, MeanReversionBoundaryStateEvolutionError> {
    if !deviation.is_finite() {
        return Err(MeanReversionBoundaryStateEvolutionError::InvalidDeviation {
            value: deviation,
        });
    }

    if !deviation_decay_factor.is_finite() || deviation_decay_factor < 0.0 {
        return Err(
            MeanReversionBoundaryStateEvolutionError::InvalidDeviationDecayFactor {
                value: deviation_decay_factor,
            },
        );
    }

    if !core_total_return.is_finite() {
        return Err(
            MeanReversionBoundaryStateEvolutionError::InvalidCoreTotalReturn {
                value: core_total_return,
            },
        );
    }

    if !current_anchor_value.is_finite() {
        return Err(
            MeanReversionBoundaryStateEvolutionError::InvalidCurrentAnchorValue {
                value: current_anchor_value,
            },
        );
    }

    if !interval_end_anchor_value.is_finite() {
        return Err(
            MeanReversionBoundaryStateEvolutionError::InvalidIntervalEndAnchorValue {
                value: interval_end_anchor_value,
            },
        );
    }

    let evolved_deviation = deviation_decay_factor.mul_add(
        deviation,
        core_total_return + current_anchor_value - interval_end_anchor_value,
    );
    if !evolved_deviation.is_finite() {
        return Err(
            MeanReversionBoundaryStateEvolutionError::InvalidEvolvedDeviation {
                value: evolved_deviation,
            },
        );
    }

    Ok(evolved_deviation)
}

/// Apply the canonical interval-grid anchor-deviation update and return the new boundary state.
pub fn evolve_mean_reversion_boundary_state_on_interval_grid(
    boundary_state: MeanReversionBoundaryState,
    kappa: f64,
    interval_delta: f64,
    core_total_return: f64,
    interval_end_anchor_value: f64,
) -> Result<MeanReversionBoundaryState, MeanReversionBoundaryStateEvolutionError> {
    boundary_state.validate().map_err(|source| {
        MeanReversionBoundaryStateEvolutionError::InvalidBoundaryState { source }
    })?;

    if !kappa.is_finite() || kappa < 0.0 {
        return Err(MeanReversionBoundaryStateEvolutionError::InvalidKappa { value: kappa });
    }

    if !interval_delta.is_finite() || interval_delta < 0.0 {
        return Err(
            MeanReversionBoundaryStateEvolutionError::InvalidIntervalDelta {
                value: interval_delta,
            },
        );
    }

    let deviation_decay_factor = (-kappa * interval_delta).exp();

    let deviation = evolve_mean_reversion_deviation_with_decay_on_interval_grid(
        boundary_state.deviation,
        deviation_decay_factor,
        core_total_return,
        boundary_state.anchor_value,
        interval_end_anchor_value,
    )?;

    let observed_log_price = interval_end_anchor_value + deviation;

    MeanReversionBoundaryState::try_new(observed_log_price, interval_end_anchor_value, deviation)
        .map_err(|source| MeanReversionBoundaryStateEvolutionError::InvalidBoundaryState { source })
}
