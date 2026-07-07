use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::numeric::{
    validate_finite_number, validate_non_negative_number, validate_positive_number,
};
use serde::{Deserialize, Serialize};

/// Fitted parameter block for temporary observed-state mean reversion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeanReversionFittedParameters {
    pub kappa: f64,
}

impl MeanReversionFittedParameters {
    pub fn try_new(kappa: f64) -> Result<Self, MeanReversionFittedParametersValidationError> {
        let fitted_parameters = Self { kappa };

        fitted_parameters.validate()?;

        Ok(fitted_parameters)
    }

    pub const fn zero_kappa() -> Self {
        Self { kappa: 0.0 }
    }

    pub const fn is_enabled(&self) -> bool {
        true
    }

    pub fn validate(&self) -> Result<(), MeanReversionFittedParametersValidationError> {
        validate_non_negative_number(self.kappa, "kappa").map_err(|source| {
            MeanReversionFittedParametersValidationError::InvalidKappa {
                value: source.value,
            }
        })
    }
}

/// Runtime-aligned anchor states shared by seeded and coarse execution planning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeanReversionRuntimeIntervalAnchors {
    pub interval_alignment_policy: String,
    pub start_anchor_value: f64,
    pub end_anchor_values: Vec<f64>,
    pub interval_deltas: Vec<f64>,
}

impl MeanReversionRuntimeIntervalAnchors {
    pub fn try_new(
        interval_alignment_policy: impl Into<String>,
        start_anchor_value: f64,
        end_anchor_values: Vec<f64>,
        interval_deltas: Vec<f64>,
    ) -> Result<Self, MeanReversionRuntimeIntervalAnchorsValidationError> {
        let runtime_interval_anchors = Self {
            interval_alignment_policy: interval_alignment_policy.into(),
            start_anchor_value,
            end_anchor_values,
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

        let expected_length = self.end_anchor_values.len();
        if self.interval_deltas.len() != expected_length {
            return Err(
                MeanReversionRuntimeIntervalAnchorsValidationError::LengthMismatch {
                    field: "interval_deltas",
                    expected: expected_length,
                    found: self.interval_deltas.len(),
                },
            );
        }

        for (interval_index, (anchor_value, interval_delta)) in self
            .end_anchor_values
            .iter()
            .zip(self.interval_deltas.iter())
            .enumerate()
        {
            validate_finite_number(*anchor_value, "end_anchor_values").map_err(|source| {
                MeanReversionRuntimeIntervalAnchorsValidationError::NonFiniteField {
                    field: source.field,
                    value: source.value.to_string(),
                }
            })?;

            validate_positive_number(*interval_delta, "interval_deltas").map_err(|source| {
                MeanReversionRuntimeIntervalAnchorsValidationError::InvalidIntervalDelta {
                    interval_index,
                    value: source.value,
                }
            })?;
        }

        Ok(())
    }
}

/// Observed-state boundary payload carried between anchored runtime intervals.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeanReversionBoundaryState {
    pub observed_log_price: f64,
    pub anchor_value: f64,
    pub deviation: f64,
}

impl MeanReversionBoundaryState {
    pub fn try_new(
        observed_log_price: f64,
        anchor_value: f64,
        deviation: f64,
    ) -> Result<Self, MeanReversionBoundaryStateValidationError> {
        let boundary_state = Self {
            observed_log_price,
            anchor_value,
            deviation,
        };

        boundary_state.validate()?;

        Ok(boundary_state)
    }

    pub fn from_observed_and_anchor(
        observed_log_price: f64,
        anchor_value: f64,
    ) -> Result<Self, MeanReversionBoundaryStateValidationError> {
        let deviation = observed_log_price - anchor_value;

        Self::try_new(observed_log_price, anchor_value, deviation)
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

        validate_finite_number(self.deviation, "deviation").map_err(|source| {
            MeanReversionBoundaryStateValidationError::NonFiniteField {
                field: source.field,
                value: source.value.to_string(),
            }
        })?;

        let expected_deviation = self.observed_log_price - self.anchor_value;
        if self.deviation != expected_deviation {
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

pub fn materialize_mean_reversion_runtime_interval_anchors_from_anchor_increments(
    interval_alignment_policy: impl Into<String>,
    start_anchor_value: f64,
    anchor_increments: &[f64],
    interval_deltas: &[f64],
) -> Result<MeanReversionRuntimeIntervalAnchors, MeanReversionAnchorPathMaterializationError> {
    validate_finite_number(start_anchor_value, "start_anchor_value").map_err(|source| {
        MeanReversionAnchorPathMaterializationError::InvalidStartAnchorValue {
            value: source.value,
        }
    })?;

    if anchor_increments.len() != interval_deltas.len() {
        return Err(
            MeanReversionAnchorPathMaterializationError::LengthMismatch {
                field: "interval_deltas",
                expected: anchor_increments.len(),
                found: interval_deltas.len(),
            },
        );
    }

    let mut end_anchor_values = Vec::with_capacity(anchor_increments.len());
    let mut current_anchor_value = start_anchor_value;

    for (interval_index, (anchor_increment, interval_delta)) in anchor_increments
        .iter()
        .zip(interval_deltas.iter())
        .enumerate()
    {
        validate_finite_number(*anchor_increment, "anchor_increments").map_err(|source| {
            MeanReversionAnchorPathMaterializationError::InvalidAnchorIncrement {
                interval_index,
                value: source.value,
            }
        })?;

        validate_positive_number(*interval_delta, "interval_deltas").map_err(|source| {
            MeanReversionAnchorPathMaterializationError::InvalidIntervalDelta {
                interval_index,
                value: source.value,
            }
        })?;

        current_anchor_value += anchor_increment;
        validate_finite_number(current_anchor_value, "end_anchor_values").map_err(|source| {
            MeanReversionAnchorPathMaterializationError::InvalidAnchorValue {
                interval_index,
                value: source.value,
            }
        })?;

        end_anchor_values.push(current_anchor_value);
    }

    MeanReversionRuntimeIntervalAnchors::try_new(
        interval_alignment_policy,
        start_anchor_value,
        end_anchor_values,
        interval_deltas.to_vec(),
    )
    .map_err(|source| {
        MeanReversionAnchorPathMaterializationError::InvalidRuntimeIntervalAnchors { source }
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum MeanReversionFittedParametersValidationError {
    InvalidKappa { value: f64 },
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
        }
    }
}

impl Error for MeanReversionFittedParametersValidationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum MeanReversionAnchorPathMaterializationError {
    InvalidStartAnchorValue {
        value: f64,
    },
    LengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    InvalidAnchorIncrement {
        interval_index: usize,
        value: f64,
    },
    InvalidAnchorValue {
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
            Self::InvalidStartAnchorValue { value } => write!(
                formatter,
                "start_anchor_value must be a finite number, found {value}"
            ),
            Self::LengthMismatch {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "{field} must contain {expected} values, found {found}"
            ),
            Self::InvalidAnchorIncrement {
                interval_index,
                value,
            } => write!(
                formatter,
                "anchor_increments[{interval_index}] must be finite, found {value}"
            ),
            Self::InvalidAnchorValue {
                interval_index,
                value,
            } => write!(
                formatter,
                "end_anchor_values[{interval_index}] must be finite, found {value}"
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
            Self::InvalidRuntimeIntervalAnchors { source } => Some(source),
            Self::InvalidStartAnchorValue { .. }
            | Self::LengthMismatch { .. }
            | Self::InvalidAnchorIncrement { .. }
            | Self::InvalidAnchorValue { .. }
            | Self::InvalidIntervalDelta { .. } => None,
        }
    }
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
