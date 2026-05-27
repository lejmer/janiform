use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::long_variance::{
    LongVarianceState, LongVarianceStateError, LongVarianceStationaryLevelError,
};

use super::super::contracts::{
    ExponentialAffineTransformActivityLoadings, ExponentialAffineTransformActivityLoadingsError,
    ExponentialAffineTransformInput, ExponentialAffineTransformInputError,
    ExponentialAffineTransformOutputError,
};
use super::super::solver::{
    ExponentialAffineTransformGuardFailure, ExponentialAffineTransformSolverSettingsError,
};

/// Tangent of the transform input with respect to one scalar parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExponentialAffineTransformInputTangent {
    pub activity_loadings: ExponentialAffineTransformActivityLoadings,
    pub z_plus: ComplexScalar,
    pub z_minus: ComplexScalar,
}

impl ExponentialAffineTransformInputTangent {
    /// Returns a tangent with no direct perturbation.
    pub fn zero() -> Self {
        Self {
            activity_loadings: ExponentialAffineTransformActivityLoadings::new(
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            z_plus: ComplexScalar::new(0.0, 0.0),
            z_minus: ComplexScalar::new(0.0, 0.0),
        }
    }

    /// Validates all tangent loadings and direct terminal jump tangents.
    pub fn validate(self) -> Result<(), ExponentialAffineTransformInputTangentError> {
        self.activity_loadings.validate().map_err(|source| {
            ExponentialAffineTransformInputTangentError::InvalidActivityLoadings { source }
        })?;

        ensure_finite_complex_tangent("z_plus", self.z_plus)?;
        ensure_finite_complex_tangent("z_minus", self.z_minus)?;

        Ok(())
    }
}

/// Validation failure for an affine transform input tangent.
#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformInputTangentError {
    InvalidActivityLoadings {
        source: ExponentialAffineTransformActivityLoadingsError,
    },
    InvalidComplexField {
        field: &'static str,
        value: ComplexScalar,
    },
}

impl Display for ExponentialAffineTransformInputTangentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidActivityLoadings { source } => {
                write!(formatter, "activity loading tangent is invalid: {source}")
            }
            Self::InvalidComplexField { field, value } => write!(
                formatter,
                "{field} tangent must be finite as a complex scalar, found {value:?}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformInputTangentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidActivityLoadings { source } => Some(source),
            Self::InvalidComplexField { .. } => None,
        }
    }
}

/// One validated interval input in a schedule-level affine cumulant request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExponentialAffineScheduleIntervalInput {
    pub transform_input: ExponentialAffineTransformInput,
    pub argument_tangent: ExponentialAffineTransformInputTangent,
    pub buy_activity_premium_tangent: ExponentialAffineTransformInputTangent,
    pub sell_activity_premium_tangent: ExponentialAffineTransformInputTangent,
}

impl ExponentialAffineScheduleIntervalInput {
    /// Validates the base transform input and all tangent inputs.
    pub fn validate(self) -> Result<(), ExponentialAffineScheduleCumulantError> {
        self.transform_input
            .validate()
            .map_err(|source| ExponentialAffineScheduleCumulantError::InvalidInput { source })?;

        self.argument_tangent.validate().map_err(|source| {
            ExponentialAffineScheduleCumulantError::InvalidInputTangent {
                tangent: "argument",
                source,
            }
        })?;

        self.buy_activity_premium_tangent
            .validate()
            .map_err(
                |source| ExponentialAffineScheduleCumulantError::InvalidInputTangent {
                    tangent: "buy_activity_premium",
                    source,
                },
            )?;

        self.sell_activity_premium_tangent
            .validate()
            .map_err(
                |source| ExponentialAffineScheduleCumulantError::InvalidInputTangent {
                    tangent: "sell_activity_premium",
                    source,
                },
            )?;

        Ok(())
    }
}

/// Full schedule cumulant request with a common initial Hawkes state.
#[derive(Debug, Clone, PartialEq)]
pub struct ExponentialAffineScheduleCumulantRequest {
    pub initial_state: HawkesExcitationState,
    pub initial_long_variance_state: LongVarianceState,
    pub intervals: Vec<ExponentialAffineScheduleIntervalInput>,
}

impl ExponentialAffineScheduleCumulantRequest {
    /// Validates the initial state and interval schedule.
    pub fn validate(&self) -> Result<(), ExponentialAffineScheduleCumulantError> {
        validate_initial_state(self.initial_state)?;

        self.initial_long_variance_state
            .validate()
            .map_err(|source| {
                ExponentialAffineScheduleCumulantError::InvalidInitialLongVarianceState { source }
            })?;

        if self.intervals.is_empty() {
            return Err(ExponentialAffineScheduleCumulantError::EmptySchedule);
        }

        for interval in &self.intervals {
            interval.validate()?;
        }

        Ok(())
    }
}

/// Schedule cumulant value and tangent derivatives evaluated at the initial state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExponentialAffineScheduleCumulantOutput {
    pub exponent: ComplexScalar,
    pub argument_derivative: ComplexScalar,
    pub buy_activity_premium_derivative: ComplexScalar,
    pub sell_activity_premium_derivative: ComplexScalar,
    pub interval_count: usize,
    pub completed_step_count: usize,
    pub rhs_evaluation_count: usize,
}

/// Interval-count progress for schedule-level affine cumulant evaluation.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ExponentialAffineScheduleCumulantProgress {
    pub completed_interval_count: usize,
    pub total_interval_count: usize,
    pub current_interval_position: Option<usize>,
}

/// Failure returned by schedule-level affine cumulant evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineScheduleCumulantError {
    EmptySchedule,
    InvalidInitialStateComponent {
        field: &'static str,
        value: f64,
    },
    InvalidInitialLongVarianceState {
        source: LongVarianceStateError,
    },
    InvalidStationaryIntensityMultipliers,
    InvalidLongVarianceStationaryLevel {
        source: LongVarianceStationaryLevelError,
    },
    InvalidInput {
        source: ExponentialAffineTransformInputError,
    },
    InvalidInputTangent {
        tangent: &'static str,
        source: ExponentialAffineTransformInputTangentError,
    },
    InvalidSolverSettings {
        source: ExponentialAffineTransformSolverSettingsError,
    },
    InvalidTerminalLoading {
        source: ExponentialAffineTransformOutputError,
    },
    GuardFailure {
        interval_position: usize,
        guard_failure: ExponentialAffineTransformGuardFailure,
    },
    InvalidOutput {
        source: ExponentialAffineTransformOutputError,
    },
    NonFiniteDerivative {
        label: &'static str,
        value: ComplexScalar,
    },
}

impl Display for ExponentialAffineScheduleCumulantError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySchedule => write!(formatter, "schedule cumulant requires intervals"),
            Self::InvalidInitialStateComponent { field, value } => write!(
                formatter,
                "initial_state.{field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidInitialLongVarianceState { source } => write!(
                formatter,
                "initial_long_variance_state is invalid: {source}"
            ),
            Self::InvalidStationaryIntensityMultipliers => write!(
                formatter,
                "schedule Hawkes kernel does not admit stationary intensity multipliers"
            ),
            Self::InvalidLongVarianceStationaryLevel { source } => write!(
                formatter,
                "schedule long-variance stationary level is invalid: {source}"
            ),
            Self::InvalidInput { source } => {
                write!(formatter, "schedule interval input is invalid: {source}")
            }
            Self::InvalidInputTangent { tangent, source } => write!(
                formatter,
                "schedule interval {tangent} tangent is invalid: {source}"
            ),
            Self::InvalidSolverSettings { source } => {
                write!(
                    formatter,
                    "schedule interval solver settings are invalid: {source}"
                )
            }
            Self::InvalidTerminalLoading { source } => {
                write!(formatter, "schedule terminal loading is invalid: {source}")
            }
            Self::GuardFailure {
                interval_position,
                guard_failure,
            } => write!(
                formatter,
                "schedule affine integration guard failed at interval_position={interval_position}: {:?}",
                guard_failure.code
            ),
            Self::InvalidOutput { source } => {
                write!(formatter, "schedule affine output is invalid: {source}")
            }
            Self::NonFiniteDerivative { label, value } => {
                write!(
                    formatter,
                    "schedule derivative {label} is non-finite: {value:?}"
                )
            }
        }
    }
}

impl Error for ExponentialAffineScheduleCumulantError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidInput { source } => Some(source),
            Self::InvalidInitialLongVarianceState { source } => Some(source),
            Self::InvalidLongVarianceStationaryLevel { source } => Some(source),
            Self::InvalidInputTangent { source, .. } => Some(source),
            Self::InvalidSolverSettings { source } => Some(source),
            Self::InvalidTerminalLoading { source } => Some(source),
            Self::InvalidOutput { source } => Some(source),
            _ => None,
        }
    }
}

pub(super) fn validate_finite_output_derivative(
    label: &'static str,
    value: ComplexScalar,
) -> Result<(), ExponentialAffineScheduleCumulantError> {
    if !value.is_finite() {
        return Err(ExponentialAffineScheduleCumulantError::NonFiniteDerivative { label, value });
    }

    Ok(())
}

fn validate_initial_state(
    state: HawkesExcitationState,
) -> Result<(), ExponentialAffineScheduleCumulantError> {
    validate_initial_state_component("y_plus_plus", state.y_plus_plus)?;
    validate_initial_state_component("y_plus_minus", state.y_plus_minus)?;
    validate_initial_state_component("y_minus_plus", state.y_minus_plus)?;
    validate_initial_state_component("y_minus_minus", state.y_minus_minus)?;

    Ok(())
}

fn validate_initial_state_component(
    field: &'static str,
    value: f64,
) -> Result<(), ExponentialAffineScheduleCumulantError> {
    if !value.is_finite() || value < 0.0 {
        return Err(
            ExponentialAffineScheduleCumulantError::InvalidInitialStateComponent { field, value },
        );
    }

    Ok(())
}

fn ensure_finite_complex_tangent(
    field: &'static str,
    value: ComplexScalar,
) -> Result<(), ExponentialAffineTransformInputTangentError> {
    if !value.is_finite() {
        return Err(
            ExponentialAffineTransformInputTangentError::InvalidComplexField { field, value },
        );
    }

    Ok(())
}
