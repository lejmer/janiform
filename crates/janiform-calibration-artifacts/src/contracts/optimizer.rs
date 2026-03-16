use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::CalibrationStage;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationOptimizerSettings {
    pub solver_name: String,
    pub max_iterations: usize,
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub constraint_penalty_weight: f64,
}

impl CalibrationOptimizerSettings {
    pub fn try_new(
        solver_name: impl Into<String>,
        max_iterations: usize,
        absolute_tolerance: f64,
        relative_tolerance: f64,
        constraint_penalty_weight: f64,
    ) -> Result<Self, CalibrationOptimizerSettingsError> {
        let optimizer_settings = Self {
            solver_name: solver_name.into(),
            max_iterations,
            absolute_tolerance,
            relative_tolerance,
            constraint_penalty_weight,
        };
        optimizer_settings.validate()?;
        Ok(optimizer_settings)
    }

    pub fn validate(&self) -> Result<(), CalibrationOptimizerSettingsError> {
        if self.solver_name.trim().is_empty() {
            return Err(CalibrationOptimizerSettingsError::MissingSolverName);
        }
        if self.max_iterations == 0 {
            return Err(CalibrationOptimizerSettingsError::InvalidMaxIterations {
                value: self.max_iterations,
            });
        }
        if !self.absolute_tolerance.is_finite() || self.absolute_tolerance <= 0.0 {
            return Err(
                CalibrationOptimizerSettingsError::InvalidAbsoluteTolerance {
                    value: self.absolute_tolerance,
                },
            );
        }
        if !self.relative_tolerance.is_finite() || self.relative_tolerance <= 0.0 {
            return Err(
                CalibrationOptimizerSettingsError::InvalidRelativeTolerance {
                    value: self.relative_tolerance,
                },
            );
        }
        if !self.constraint_penalty_weight.is_finite() || self.constraint_penalty_weight < 0.0 {
            return Err(
                CalibrationOptimizerSettingsError::InvalidConstraintPenaltyWeight {
                    value: self.constraint_penalty_weight,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOptimizerSettingsError {
    MissingSolverName,
    InvalidMaxIterations { value: usize },
    InvalidAbsoluteTolerance { value: f64 },
    InvalidRelativeTolerance { value: f64 },
    InvalidConstraintPenaltyWeight { value: f64 },
}

impl Display for CalibrationOptimizerSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSolverName => {
                write!(formatter, "solver_name must be a non-empty string")
            }
            Self::InvalidMaxIterations { value } => write!(
                formatter,
                "max_iterations must be strictly positive, found {value}"
            ),
            Self::InvalidAbsoluteTolerance { value } => write!(
                formatter,
                "absolute_tolerance must be finite and strictly positive, found {value}"
            ),
            Self::InvalidRelativeTolerance { value } => write!(
                formatter,
                "relative_tolerance must be finite and strictly positive, found {value}"
            ),
            Self::InvalidConstraintPenaltyWeight { value } => write!(
                formatter,
                "constraint_penalty_weight must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for CalibrationOptimizerSettingsError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationStageSolverSettings {
    pub stage: CalibrationStage,
    pub optimizer_settings: CalibrationOptimizerSettings,
}
