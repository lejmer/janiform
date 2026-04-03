use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::numeric::validate_finite_number;
use serde::{Deserialize, Serialize};

const BOUNDARY_STATE_INVARIANT_TOLERANCE: f64 = 1e-12;
const ZERO_GAIN_PRESSURE_LEVEL_REFERENCE_INTERVAL_HOURS: f64 = 1.0;

/// Persisted anchor specification for the observed mean-reversion layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeanReversionAnchorModel {
    pub reference_interval_hours: f64,
    pub pressure_persistence_per_reference_interval: f64,
    pub pressure_level_gain: f64,
}

impl MeanReversionAnchorModel {
    pub fn endogenous_core_pressure_level(
        reference_interval_hours: f64,
        pressure_persistence_per_reference_interval: f64,
        pressure_level_gain: f64,
    ) -> Self {
        Self {
            reference_interval_hours,
            pressure_persistence_per_reference_interval,
            pressure_level_gain,
        }
    }

    pub fn zero_gain_endogenous_core_pressure_level() -> Self {
        Self::endogenous_core_pressure_level(
            ZERO_GAIN_PRESSURE_LEVEL_REFERENCE_INTERVAL_HOURS,
            0.0,
            0.0,
        )
    }

    pub fn validate(&self) -> Result<(), MeanReversionAnchorModelValidationError> {
        validate_finite_number(self.reference_interval_hours, "reference_interval_hours").map_err(
            |source| MeanReversionAnchorModelValidationError::NonFiniteField {
                field: source.field,
                value: source.value.to_string(),
            },
        )?;
        if self.reference_interval_hours <= 0.0 {
            return Err(MeanReversionAnchorModelValidationError::NonPositiveField {
                field: "reference_interval_hours",
                value: self.reference_interval_hours,
            });
        }

        validate_finite_number(
            self.pressure_persistence_per_reference_interval,
            "pressure_persistence_per_reference_interval",
        )
        .map_err(
            |source| MeanReversionAnchorModelValidationError::NonFiniteField {
                field: source.field,
                value: source.value.to_string(),
            },
        )?;

        if !(0.0..=1.0).contains(&self.pressure_persistence_per_reference_interval) {
            return Err(
                MeanReversionAnchorModelValidationError::OutOfUnitIntervalField {
                    field: "pressure_persistence_per_reference_interval",
                    value: self.pressure_persistence_per_reference_interval,
                },
            );
        }

        validate_finite_number(self.pressure_level_gain, "pressure_level_gain").map_err(
            |source| MeanReversionAnchorModelValidationError::NonFiniteField {
                field: source.field,
                value: source.value.to_string(),
            },
        )?;

        Ok(())
    }
}

/// Fitted parameter block for the observed mean-reversion layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeanReversionFittedParameters {
    pub kappa: f64,
    pub anchor_model: MeanReversionAnchorModel,
}

impl MeanReversionFittedParameters {
    pub fn endogenous_core_pressure_level(
        kappa: f64,
        reference_interval_hours: f64,
        pressure_persistence_per_reference_interval: f64,
        pressure_level_gain: f64,
    ) -> Result<Self, MeanReversionFittedParametersValidationError> {
        Self::try_new(
            kappa,
            MeanReversionAnchorModel::endogenous_core_pressure_level(
                reference_interval_hours,
                pressure_persistence_per_reference_interval,
                pressure_level_gain,
            ),
        )
    }

    pub fn flat_anchor_mean_reversion(
        kappa: f64,
    ) -> Result<Self, MeanReversionFittedParametersValidationError> {
        Self::try_new(
            kappa,
            MeanReversionAnchorModel::zero_gain_endogenous_core_pressure_level(),
        )
    }

    pub fn try_new(
        kappa: f64,
        anchor_model: MeanReversionAnchorModel,
    ) -> Result<Self, MeanReversionFittedParametersValidationError> {
        let fitted_parameters = Self {
            kappa,
            anchor_model,
        };

        fitted_parameters.validate()?;

        Ok(fitted_parameters)
    }

    pub const fn is_enabled(&self) -> bool {
        true
    }

    pub fn validate(&self) -> Result<(), MeanReversionFittedParametersValidationError> {
        if !self.kappa.is_finite() || self.kappa < 0.0 {
            return Err(MeanReversionFittedParametersValidationError::InvalidKappa {
                value: self.kappa,
            });
        }

        self.anchor_model
            .validate()
            .map_err(MeanReversionFittedParametersValidationError::InvalidAnchorModel)?;

        Ok(())
    }
}

/// Runtime-aligned anchor and pressure states shared by seeded and coarse execution planning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeanReversionRuntimeIntervalAnchors {
    pub interval_alignment_policy: String,
    pub start_anchor_value: f64,
    pub start_pressure_state: f64,
    pub end_anchor_values: Vec<f64>,
    pub end_pressure_states: Vec<f64>,
    pub interval_deltas: Vec<f64>,
}

impl MeanReversionRuntimeIntervalAnchors {
    pub fn try_new(
        interval_alignment_policy: impl Into<String>,
        start_anchor_value: f64,
        start_pressure_state: f64,
        end_anchor_values: Vec<f64>,
        end_pressure_states: Vec<f64>,
        interval_deltas: Vec<f64>,
    ) -> Result<Self, MeanReversionRuntimeIntervalAnchorsValidationError> {
        let runtime_interval_anchors = Self {
            interval_alignment_policy: interval_alignment_policy.into(),
            start_anchor_value,
            start_pressure_state,
            end_anchor_values,
            end_pressure_states,
            interval_deltas,
        };

        runtime_interval_anchors.validate()?;

        Ok(runtime_interval_anchors)
    }

    pub fn validate(&self) -> Result<(), MeanReversionRuntimeIntervalAnchorsValidationError> {
        if self.interval_alignment_policy.trim().is_empty() {
            return Err(
                MeanReversionRuntimeIntervalAnchorsValidationError::EmptyIntervalAlignmentPolicy,
            );
        }

        validate_finite_number(self.start_anchor_value, "start_anchor_value").map_err(
            |source| MeanReversionRuntimeIntervalAnchorsValidationError::NonFiniteField {
                field: source.field,
                value: source.value.to_string(),
            },
        )?;
        validate_finite_number(self.start_pressure_state, "start_pressure_state").map_err(
            |source| MeanReversionRuntimeIntervalAnchorsValidationError::NonFiniteField {
                field: source.field,
                value: source.value.to_string(),
            },
        )?;

        let expected_length = self.end_anchor_values.len();
        if self.end_pressure_states.len() != expected_length {
            return Err(
                MeanReversionRuntimeIntervalAnchorsValidationError::LengthMismatch {
                    field: "end_pressure_states",
                    expected: expected_length,
                    found: self.end_pressure_states.len(),
                },
            );
        }
        if self.interval_deltas.len() != expected_length {
            return Err(
                MeanReversionRuntimeIntervalAnchorsValidationError::LengthMismatch {
                    field: "interval_deltas",
                    expected: expected_length,
                    found: self.interval_deltas.len(),
                },
            );
        }

        for (interval_index, anchor_value) in self.end_anchor_values.iter().enumerate() {
            validate_finite_number(*anchor_value, "end_anchor_values").map_err(|source| {
                MeanReversionRuntimeIntervalAnchorsValidationError::NonFiniteField {
                    field: source.field,
                    value: source.value.to_string(),
                }
            })?;

            let pressure_state = self.end_pressure_states[interval_index];
            validate_finite_number(pressure_state, "end_pressure_states").map_err(|source| {
                MeanReversionRuntimeIntervalAnchorsValidationError::NonFiniteField {
                    field: source.field,
                    value: source.value.to_string(),
                }
            })?;

            let interval_delta = self.interval_deltas[interval_index];
            if !interval_delta.is_finite() || interval_delta <= 0.0 {
                return Err(
                    MeanReversionRuntimeIntervalAnchorsValidationError::InvalidIntervalDelta {
                        interval_index,
                        value: interval_delta,
                    },
                );
            }
        }

        Ok(())
    }
}

/// Observed-state boundary payload carried between anchored runtime intervals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeanReversionBoundaryState {
    pub observed_log_price: f64,
    pub anchor_value: f64,
    pub pressure_state: f64,
    pub deviation: f64,
}

impl MeanReversionBoundaryState {
    pub fn try_new(
        observed_log_price: f64,
        anchor_value: f64,
        pressure_state: f64,
        deviation: f64,
    ) -> Result<Self, MeanReversionBoundaryStateValidationError> {
        let boundary_state = Self {
            observed_log_price,
            anchor_value,
            pressure_state,
            deviation,
        };

        boundary_state.validate()?;

        Ok(boundary_state)
    }

    pub fn validate(&self) -> Result<(), MeanReversionBoundaryStateValidationError> {
        validate_finite_number(self.observed_log_price, "observed_log_price").map_err(
            |source| MeanReversionBoundaryStateValidationError::NonFiniteField {
                field: source.field,
                value: source.value.to_string(),
            },
        )?;
        validate_finite_number(self.anchor_value, "anchor_value").map_err(|source| {
            MeanReversionBoundaryStateValidationError::NonFiniteField {
                field: source.field,
                value: source.value.to_string(),
            }
        })?;
        validate_finite_number(self.pressure_state, "pressure_state").map_err(|source| {
            MeanReversionBoundaryStateValidationError::NonFiniteField {
                field: source.field,
                value: source.value.to_string(),
            }
        })?;
        validate_finite_number(self.deviation, "deviation").map_err(|source| {
            MeanReversionBoundaryStateValidationError::NonFiniteField {
                field: source.field,
                value: source.value.to_string(),
            }
        })?;

        let expected_deviation = self.observed_log_price - self.anchor_value;
        if (self.deviation - expected_deviation).abs() > BOUNDARY_STATE_INVARIANT_TOLERANCE {
            return Err(
                MeanReversionBoundaryStateValidationError::DeviationInvariantViolation {
                    observed_log_price: self.observed_log_price,
                    anchor_value: self.anchor_value,
                    deviation: self.deviation,
                    expected_deviation,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MeanReversionBoundaryStateEvolutionError {
    InvalidBoundaryState {
        source: MeanReversionBoundaryStateValidationError,
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
    InvalidIntervalEndPressureState {
        value: f64,
    },
}

impl Display for MeanReversionBoundaryStateEvolutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBoundaryState { source } => {
                write!(formatter, "boundary_state is invalid: {source}")
            }
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
            Self::InvalidIntervalEndPressureState { value } => write!(
                formatter,
                "interval_end_pressure_state must be a finite number, found {value}"
            ),
        }
    }
}

impl Error for MeanReversionBoundaryStateEvolutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidBoundaryState { source } => Some(source),
            Self::InvalidKappa { .. }
            | Self::InvalidIntervalDelta { .. }
            | Self::InvalidCoreTotalReturn { .. }
            | Self::InvalidIntervalEndAnchorValue { .. }
            | Self::InvalidIntervalEndPressureState { .. } => None,
        }
    }
}

/// Apply the canonical interval-grid anchor-deviation update and return the new boundary state.
pub fn evolve_mean_reversion_boundary_state_on_interval_grid(
    boundary_state: MeanReversionBoundaryState,
    kappa: f64,
    interval_delta: f64,
    core_total_return: f64,
    interval_end_anchor_value: f64,
    interval_end_pressure_state: f64,
) -> Result<MeanReversionBoundaryState, MeanReversionBoundaryStateEvolutionError> {
    boundary_state.validate().map_err(|source| {
        MeanReversionBoundaryStateEvolutionError::InvalidBoundaryState { source }
    })?;

    if !kappa.is_finite() || kappa < 0.0 {
        return Err(MeanReversionBoundaryStateEvolutionError::InvalidKappa { value: kappa });
    }
    if !interval_delta.is_finite() || interval_delta <= 0.0 {
        return Err(
            MeanReversionBoundaryStateEvolutionError::InvalidIntervalDelta {
                value: interval_delta,
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
    if !interval_end_anchor_value.is_finite() {
        return Err(
            MeanReversionBoundaryStateEvolutionError::InvalidIntervalEndAnchorValue {
                value: interval_end_anchor_value,
            },
        );
    }
    if !interval_end_pressure_state.is_finite() {
        return Err(
            MeanReversionBoundaryStateEvolutionError::InvalidIntervalEndPressureState {
                value: interval_end_pressure_state,
            },
        );
    }

    let deviation = (-kappa * interval_delta).exp() * boundary_state.deviation
        + core_total_return
        + boundary_state.anchor_value
        - interval_end_anchor_value;
    let observed_log_price = interval_end_anchor_value + deviation;

    MeanReversionBoundaryState::try_new(
        observed_log_price,
        interval_end_anchor_value,
        interval_end_pressure_state,
        deviation,
    )
    .map_err(|source| MeanReversionBoundaryStateEvolutionError::InvalidBoundaryState { source })
}

/// Apply the canonical endogenous pressure-to-level update over one interval.
pub fn evolve_mean_reversion_boundary_state_with_endogenous_pressure(
    boundary_state: MeanReversionBoundaryState,
    fitted_parameters: &MeanReversionFittedParameters,
    interval_delta: f64,
    core_total_return: f64,
    core_expected_increment: f64,
) -> Result<MeanReversionBoundaryState, MeanReversionBoundaryStateEvolutionError> {
    boundary_state.validate().map_err(|source| {
        MeanReversionBoundaryStateEvolutionError::InvalidBoundaryState { source }
    })?;

    fitted_parameters.validate().map_err(|source| {
        MeanReversionBoundaryStateEvolutionError::InvalidBoundaryState {
            source: MeanReversionBoundaryStateValidationError::NonFiniteField {
                field: "fitted_parameters",
                value: source.to_string(),
            },
        }
    })?;

    let interval_end_pressure_state = evolve_mean_reversion_pressure_state(
        &fitted_parameters.anchor_model,
        boundary_state.pressure_state,
        core_expected_increment,
        interval_delta,
    )
    .map_err(|source| match source {
        MeanReversionPressureStateEvolutionError::InvalidAnchorModel { source } => {
            MeanReversionBoundaryStateEvolutionError::InvalidBoundaryState {
                source: MeanReversionBoundaryStateValidationError::NonFiniteField {
                    field: "anchor_model",
                    value: source.to_string(),
                },
            }
        }
        MeanReversionPressureStateEvolutionError::InvalidPreviousPressureState { value }
        | MeanReversionPressureStateEvolutionError::InvalidCoreExpectedIncrement { value }
        | MeanReversionPressureStateEvolutionError::InvalidIntervalDelta { value } => {
            MeanReversionBoundaryStateEvolutionError::InvalidIntervalEndPressureState { value }
        }
    })?;

    let interval_end_anchor_value = boundary_state.anchor_value
        + fitted_parameters.anchor_model.pressure_level_gain
            * (interval_end_pressure_state - boundary_state.pressure_state);

    evolve_mean_reversion_boundary_state_on_interval_grid(
        boundary_state,
        fitted_parameters.kappa,
        interval_delta,
        core_total_return,
        interval_end_anchor_value,
        interval_end_pressure_state,
    )
}

#[derive(Debug, Clone, PartialEq)]
pub enum MeanReversionAnchorModelValidationError {
    NonFiniteField { field: &'static str, value: String },
    NonPositiveField { field: &'static str, value: f64 },
    OutOfUnitIntervalField { field: &'static str, value: f64 },
}

impl Display for MeanReversionAnchorModelValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteField { field, value } => {
                write!(formatter, "{field} must be a finite number, found {value}")
            }
            Self::NonPositiveField { field, value } => {
                write!(
                    formatter,
                    "{field} must be strictly positive, found {value}"
                )
            }
            Self::OutOfUnitIntervalField { field, value } => {
                write!(formatter, "{field} must lie in [0, 1], found {value}")
            }
        }
    }
}

impl Error for MeanReversionAnchorModelValidationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum MeanReversionFittedParametersValidationError {
    InvalidKappa { value: f64 },
    InvalidAnchorModel(MeanReversionAnchorModelValidationError),
}

impl Display for MeanReversionFittedParametersValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKappa { value } => {
                write!(
                    formatter,
                    "kappa must be a finite non-negative number, found {value}"
                )
            }
            Self::InvalidAnchorModel(source) => {
                write!(formatter, "anchor_model is invalid: {source}")
            }
        }
    }
}

impl Error for MeanReversionFittedParametersValidationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum MeanReversionPressureStateEvolutionError {
    InvalidAnchorModel {
        source: MeanReversionAnchorModelValidationError,
    },
    InvalidPreviousPressureState {
        value: f64,
    },
    InvalidCoreExpectedIncrement {
        value: f64,
    },
    InvalidIntervalDelta {
        value: f64,
    },
}

impl Display for MeanReversionPressureStateEvolutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAnchorModel { source } => {
                write!(formatter, "anchor_model is invalid: {source}")
            }
            Self::InvalidPreviousPressureState { value } => write!(
                formatter,
                "previous_pressure_state must be finite, found {value}"
            ),
            Self::InvalidCoreExpectedIncrement { value } => write!(
                formatter,
                "core_expected_increment must be finite, found {value}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be finite and strictly positive, found {value}"
            ),
        }
    }
}

impl Error for MeanReversionPressureStateEvolutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidAnchorModel { source } => Some(source),
            Self::InvalidPreviousPressureState { .. }
            | Self::InvalidCoreExpectedIncrement { .. }
            | Self::InvalidIntervalDelta { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MeanReversionAnchorPathMaterializationError {
    InvalidAnchorModel {
        source: MeanReversionAnchorModelValidationError,
    },
    InvalidStartAnchorValue {
        value: f64,
    },
    InvalidStartPressureState {
        value: f64,
    },
    LengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    InvalidCoreExpectedIncrement {
        interval_index: usize,
        value: f64,
    },
    InvalidIntervalDelta {
        interval_index: usize,
        value: f64,
    },
    InvalidRuntimeIntervalAnchors {
        source: MeanReversionRuntimeIntervalAnchorsValidationError,
    },
}

impl Display for MeanReversionAnchorPathMaterializationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAnchorModel { source } => {
                write!(formatter, "anchor_model is invalid: {source}")
            }
            Self::InvalidStartAnchorValue { value } => write!(
                formatter,
                "start_anchor_value must be a finite number, found {value}"
            ),
            Self::InvalidStartPressureState { value } => write!(
                formatter,
                "start_pressure_state must be a finite number, found {value}"
            ),
            Self::LengthMismatch {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "{field} must contain {expected} values, found {found}"
            ),
            Self::InvalidCoreExpectedIncrement {
                interval_index,
                value,
            } => write!(
                formatter,
                "core_expected_increments[{interval_index}] must be finite, found {value}"
            ),
            Self::InvalidIntervalDelta {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_deltas[{interval_index}] must be finite and strictly positive, found {value}"
            ),
            Self::InvalidRuntimeIntervalAnchors { source } => {
                write!(
                    formatter,
                    "materialized runtime anchors are invalid: {source}"
                )
            }
        }
    }
}

impl Error for MeanReversionAnchorPathMaterializationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidAnchorModel { source } => Some(source),
            Self::InvalidRuntimeIntervalAnchors { source } => Some(source),
            Self::InvalidStartAnchorValue { .. }
            | Self::InvalidStartPressureState { .. }
            | Self::LengthMismatch { .. }
            | Self::InvalidCoreExpectedIncrement { .. }
            | Self::InvalidIntervalDelta { .. } => None,
        }
    }
}

pub fn evolve_mean_reversion_pressure_state(
    anchor_model: &MeanReversionAnchorModel,
    previous_pressure_state: f64,
    core_expected_increment: f64,
    interval_delta: f64,
) -> Result<f64, MeanReversionPressureStateEvolutionError> {
    anchor_model.validate().map_err(|source| {
        MeanReversionPressureStateEvolutionError::InvalidAnchorModel { source }
    })?;

    if !previous_pressure_state.is_finite() {
        return Err(
            MeanReversionPressureStateEvolutionError::InvalidPreviousPressureState {
                value: previous_pressure_state,
            },
        );
    }
    if !core_expected_increment.is_finite() {
        return Err(
            MeanReversionPressureStateEvolutionError::InvalidCoreExpectedIncrement {
                value: core_expected_increment,
            },
        );
    }
    if !interval_delta.is_finite() || interval_delta <= 0.0 {
        return Err(
            MeanReversionPressureStateEvolutionError::InvalidIntervalDelta {
                value: interval_delta,
            },
        );
    }

    let reference_interval_hours = anchor_model.reference_interval_hours;
    let pressure_persistence = anchor_model
        .pressure_persistence_per_reference_interval
        .powf(interval_delta / reference_interval_hours);

    Ok(pressure_persistence * previous_pressure_state
        + (1.0 - pressure_persistence) * core_expected_increment)
}

pub fn materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments(
    interval_alignment_policy: impl Into<String>,
    start_anchor_value: f64,
    start_pressure_state: f64,
    anchor_model: &MeanReversionAnchorModel,
    core_expected_increments: &[f64],
    interval_deltas: &[f64],
) -> Result<MeanReversionRuntimeIntervalAnchors, MeanReversionAnchorPathMaterializationError> {
    anchor_model.validate().map_err(|source| {
        MeanReversionAnchorPathMaterializationError::InvalidAnchorModel { source }
    })?;

    if !start_anchor_value.is_finite() {
        return Err(
            MeanReversionAnchorPathMaterializationError::InvalidStartAnchorValue {
                value: start_anchor_value,
            },
        );
    }
    if !start_pressure_state.is_finite() {
        return Err(
            MeanReversionAnchorPathMaterializationError::InvalidStartPressureState {
                value: start_pressure_state,
            },
        );
    }
    if core_expected_increments.len() != interval_deltas.len() {
        return Err(
            MeanReversionAnchorPathMaterializationError::LengthMismatch {
                field: "interval_deltas",
                expected: core_expected_increments.len(),
                found: interval_deltas.len(),
            },
        );
    }

    let mut end_anchor_values = Vec::with_capacity(core_expected_increments.len());
    let mut end_pressure_states = Vec::with_capacity(core_expected_increments.len());
    let mut current_anchor_value = start_anchor_value;
    let mut current_pressure_state = start_pressure_state;

    for (interval_index, (&core_expected_increment, &interval_delta)) in core_expected_increments
        .iter()
        .zip(interval_deltas.iter())
        .enumerate()
    {
        if !core_expected_increment.is_finite() {
            return Err(
                MeanReversionAnchorPathMaterializationError::InvalidCoreExpectedIncrement {
                    interval_index,
                    value: core_expected_increment,
                },
            );
        }
        if !interval_delta.is_finite() || interval_delta <= 0.0 {
            return Err(
                MeanReversionAnchorPathMaterializationError::InvalidIntervalDelta {
                    interval_index,
                    value: interval_delta,
                },
            );
        }

        let next_pressure_state = evolve_mean_reversion_pressure_state(
            anchor_model,
            current_pressure_state,
            core_expected_increment,
            interval_delta,
        )
        .map_err(|source| match source {
            MeanReversionPressureStateEvolutionError::InvalidAnchorModel { source } => {
                MeanReversionAnchorPathMaterializationError::InvalidAnchorModel { source }
            }
            MeanReversionPressureStateEvolutionError::InvalidPreviousPressureState { value }
            | MeanReversionPressureStateEvolutionError::InvalidCoreExpectedIncrement { value } => {
                MeanReversionAnchorPathMaterializationError::InvalidCoreExpectedIncrement {
                    interval_index,
                    value,
                }
            }
            MeanReversionPressureStateEvolutionError::InvalidIntervalDelta { value } => {
                MeanReversionAnchorPathMaterializationError::InvalidIntervalDelta {
                    interval_index,
                    value,
                }
            }
        })?;

        current_anchor_value +=
            anchor_model.pressure_level_gain * (next_pressure_state - current_pressure_state);

        current_pressure_state = next_pressure_state;

        end_anchor_values.push(current_anchor_value);
        end_pressure_states.push(current_pressure_state);
    }

    MeanReversionRuntimeIntervalAnchors::try_new(
        interval_alignment_policy,
        start_anchor_value,
        start_pressure_state,
        end_anchor_values,
        end_pressure_states,
        interval_deltas.to_vec(),
    )
    .map_err(|source| {
        MeanReversionAnchorPathMaterializationError::InvalidRuntimeIntervalAnchors { source }
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum MeanReversionRuntimeIntervalAnchorsValidationError {
    EmptyIntervalAlignmentPolicy,
    LengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    InvalidIntervalDelta {
        interval_index: usize,
        value: f64,
    },
    NonFiniteField {
        field: &'static str,
        value: String,
    },
}

impl Display for MeanReversionRuntimeIntervalAnchorsValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIntervalAlignmentPolicy => {
                write!(
                    formatter,
                    "interval_alignment_policy must be a non-empty string"
                )
            }
            Self::LengthMismatch {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "{field} must contain {expected} values, found {found}"
            ),
            Self::InvalidIntervalDelta {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_deltas[{interval_index}] must be finite and strictly positive, found {value}"
            ),
            Self::NonFiniteField { field, value } => {
                write!(formatter, "{field} must be a finite number, found {value}")
            }
        }
    }
}

impl Error for MeanReversionRuntimeIntervalAnchorsValidationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum MeanReversionBoundaryStateValidationError {
    NonFiniteField {
        field: &'static str,
        value: String,
    },
    DeviationInvariantViolation {
        observed_log_price: f64,
        anchor_value: f64,
        deviation: f64,
        expected_deviation: f64,
    },
}

impl Display for MeanReversionBoundaryStateValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteField { field, value } => {
                write!(formatter, "{field} must be a finite number, found {value}")
            }
            Self::DeviationInvariantViolation {
                observed_log_price,
                anchor_value,
                deviation,
                expected_deviation,
            } => write!(
                formatter,
                "deviation must equal observed_log_price - anchor_value, found deviation={deviation} observed_log_price={observed_log_price} anchor_value={anchor_value} expected_deviation={expected_deviation}"
            ),
        }
    }
}

impl Error for MeanReversionBoundaryStateValidationError {}
