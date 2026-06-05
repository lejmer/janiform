use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::CalibrationStage;

/// Calibration search effort policy.
///
/// The effort policy controls how much restart search work Janiform performs
/// after stage warm starts are available. Presets are named policies rather
/// than optimizer-specific magic numbers: the runtime derives concrete poll
/// counts from the active parameter dimension and uses these multipliers to
/// size the asynchronous restart pool.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CalibrationEffortOptions {
    pub restart_candidate_budget: usize,
    pub restart_pool_total_budget_scale: f64,
    pub restart_pool_max_candidate_budget_scale: f64,
    pub restart_pool_minimum_poll_rounds: usize,
    pub restart_pool_rescue_poll_interval: usize,
}

impl CalibrationEffortOptions {
    pub const fn fast() -> Self {
        Self {
            restart_candidate_budget: 10,
            restart_pool_total_budget_scale: 0.65,
            restart_pool_max_candidate_budget_scale: 1.25,
            restart_pool_minimum_poll_rounds: 1,
            restart_pool_rescue_poll_interval: 2,
        }
    }

    pub const fn standard() -> Self {
        Self {
            restart_candidate_budget: 15,
            restart_pool_total_budget_scale: 1.0,
            restart_pool_max_candidate_budget_scale: 2.0,
            restart_pool_minimum_poll_rounds: 1,
            restart_pool_rescue_poll_interval: 2,
        }
    }

    pub const fn thorough() -> Self {
        Self {
            restart_candidate_budget: 24,
            restart_pool_total_budget_scale: 1.75,
            restart_pool_max_candidate_budget_scale: 3.0,
            restart_pool_minimum_poll_rounds: 2,
            restart_pool_rescue_poll_interval: 2,
        }
    }

    pub fn validate(&self) -> Result<(), CalibrationEffortOptionsError> {
        if self.restart_candidate_budget == 0 {
            return Err(
                CalibrationEffortOptionsError::InvalidRestartCandidateBudget {
                    value: self.restart_candidate_budget,
                },
            );
        }

        if !self.restart_pool_total_budget_scale.is_finite()
            || self.restart_pool_total_budget_scale <= 0.0
        {
            return Err(CalibrationEffortOptionsError::InvalidTotalBudgetScale {
                value: self.restart_pool_total_budget_scale,
            });
        }

        if !self.restart_pool_max_candidate_budget_scale.is_finite()
            || self.restart_pool_max_candidate_budget_scale <= 0.0
        {
            return Err(
                CalibrationEffortOptionsError::InvalidMaxCandidateBudgetScale {
                    value: self.restart_pool_max_candidate_budget_scale,
                },
            );
        }

        if self.restart_pool_minimum_poll_rounds == 0 {
            return Err(CalibrationEffortOptionsError::InvalidMinimumPollRounds {
                value: self.restart_pool_minimum_poll_rounds,
            });
        }

        if self.restart_pool_rescue_poll_interval == 0 {
            return Err(CalibrationEffortOptionsError::InvalidRescuePollInterval {
                value: self.restart_pool_rescue_poll_interval,
            });
        }

        Ok(())
    }
}

pub const FAST_CALIBRATION_EFFORT: CalibrationEffortOptions = CalibrationEffortOptions::fast();
pub const STANDARD_CALIBRATION_EFFORT: CalibrationEffortOptions =
    CalibrationEffortOptions::standard();
pub const THOROUGH_CALIBRATION_EFFORT: CalibrationEffortOptions =
    CalibrationEffortOptions::thorough();

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationEffortOptionsError {
    InvalidRestartCandidateBudget { value: usize },
    InvalidTotalBudgetScale { value: f64 },
    InvalidMaxCandidateBudgetScale { value: f64 },
    InvalidMinimumPollRounds { value: usize },
    InvalidRescuePollInterval { value: usize },
}

impl Display for CalibrationEffortOptionsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRestartCandidateBudget { value } => write!(
                formatter,
                "restart_candidate_budget must be strictly positive, found {value}"
            ),
            Self::InvalidTotalBudgetScale { value } => write!(
                formatter,
                "restart_pool_total_budget_scale must be finite and strictly positive, found {value}"
            ),
            Self::InvalidMaxCandidateBudgetScale { value } => write!(
                formatter,
                "restart_pool_max_candidate_budget_scale must be finite and strictly positive, found {value}"
            ),
            Self::InvalidMinimumPollRounds { value } => write!(
                formatter,
                "restart_pool_minimum_poll_rounds must be strictly positive, found {value}"
            ),
            Self::InvalidRescuePollInterval { value } => write!(
                formatter,
                "restart_pool_rescue_poll_interval must be strictly positive, found {value}"
            ),
        }
    }
}

impl Error for CalibrationEffortOptionsError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationOptimizerSettings {
    pub solver_name: String,
    pub max_iterations: usize,
    pub cpu_count: usize,
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub constraint_penalty_weight: f64,
}

impl CalibrationOptimizerSettings {
    pub fn try_new(
        solver_name: impl Into<String>,
        max_iterations: usize,
        cpu_count: usize,
        absolute_tolerance: f64,
        relative_tolerance: f64,
        constraint_penalty_weight: f64,
    ) -> Result<Self, CalibrationOptimizerSettingsError> {
        let optimizer_settings = Self {
            solver_name: solver_name.into(),
            max_iterations,
            cpu_count,
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

#[cfg(test)]
mod tests {
    use super::CalibrationOptimizerSettings;

    #[test]
    fn optimizer_settings_accept_zero_cpu_count_as_auto_budget() {
        let optimizer_settings =
            CalibrationOptimizerSettings::try_new("lbfgs", 10, 0, 1.0e-6, 1.0e-6, 1.0)
                .expect("zero cpu_count is an automatic CPU-budget request");

        assert_eq!(optimizer_settings.cpu_count, 0);
    }
}
