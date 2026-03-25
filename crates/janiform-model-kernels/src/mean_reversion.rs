use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::numeric::validate_finite_number;
use serde::{Deserialize, Serialize};

const BOUNDARY_STATE_INVARIANT_TOLERANCE: f64 = 1e-12;

/// Canonical anchor-family selector for the observed mean-reversion layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeanReversionAnchorModelKind {
    Disabled,
    Constant,
    DeterministicIntervalGrid,
}

/// Persisted anchor specification for the observed mean-reversion layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeanReversionAnchorModel {
    pub anchor_model_kind: MeanReversionAnchorModelKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_interval_start_utc_nanos: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_interval_end_utc_nanos: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchor_values: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_alignment_policy: Option<String>,
}

impl MeanReversionAnchorModel {
    pub fn disabled() -> Self {
        Self {
            anchor_model_kind: MeanReversionAnchorModelKind::Disabled,
            anchor_value: None,
            source_interval_start_utc_nanos: Vec::new(),
            source_interval_end_utc_nanos: Vec::new(),
            anchor_values: Vec::new(),
            source_alignment_policy: None,
        }
    }

    pub fn constant(anchor_value: f64) -> Result<Self, MeanReversionAnchorModelValidationError> {
        let anchor_model = Self {
            anchor_model_kind: MeanReversionAnchorModelKind::Constant,
            anchor_value: Some(anchor_value),
            source_interval_start_utc_nanos: Vec::new(),
            source_interval_end_utc_nanos: Vec::new(),
            anchor_values: Vec::new(),
            source_alignment_policy: None,
        };
        anchor_model.validate()?;
        Ok(anchor_model)
    }

    pub fn deterministic_interval_grid(
        source_interval_start_utc_nanos: Vec<i64>,
        source_interval_end_utc_nanos: Vec<i64>,
        anchor_values: Vec<f64>,
        source_alignment_policy: impl Into<String>,
    ) -> Result<Self, MeanReversionAnchorModelValidationError> {
        let anchor_model = Self {
            anchor_model_kind: MeanReversionAnchorModelKind::DeterministicIntervalGrid,
            anchor_value: None,
            source_interval_start_utc_nanos,
            source_interval_end_utc_nanos,
            anchor_values,
            source_alignment_policy: Some(source_alignment_policy.into()),
        };

        anchor_model.validate()?;

        Ok(anchor_model)
    }

    pub fn validate(&self) -> Result<(), MeanReversionAnchorModelValidationError> {
        match self.anchor_model_kind {
            MeanReversionAnchorModelKind::Disabled => {
                if self.anchor_value.is_some() {
                    return Err(
                        MeanReversionAnchorModelValidationError::UnexpectedFieldForAnchorModelKind {
                            anchor_model_kind: self.anchor_model_kind,
                            field: "anchor_value",
                        },
                    );
                }

                validate_absent_deterministic_interval_grid(self)?;
            }
            MeanReversionAnchorModelKind::Constant => {
                let anchor_value = self.anchor_value.ok_or(
                    MeanReversionAnchorModelValidationError::MissingFieldForAnchorModelKind {
                        anchor_model_kind: self.anchor_model_kind,
                        field: "anchor_value",
                    },
                )?;

                validate_finite_number(anchor_value, "anchor_value").map_err(|source| {
                    MeanReversionAnchorModelValidationError::NonFiniteField {
                        field: source.field,
                        value: source.value.to_string(),
                    }
                })?;
                validate_absent_deterministic_interval_grid(self)?;
            }
            MeanReversionAnchorModelKind::DeterministicIntervalGrid => {
                if self.anchor_value.is_some() {
                    return Err(
                        MeanReversionAnchorModelValidationError::UnexpectedFieldForAnchorModelKind {
                            anchor_model_kind: self.anchor_model_kind,
                            field: "anchor_value",
                        },
                    );
                }

                let source_alignment_policy = self.source_alignment_policy.as_ref().ok_or(
                    MeanReversionAnchorModelValidationError::MissingFieldForAnchorModelKind {
                        anchor_model_kind: self.anchor_model_kind,
                        field: "source_alignment_policy",
                    },
                )?;
                if source_alignment_policy.trim().is_empty() {
                    return Err(MeanReversionAnchorModelValidationError::EmptyStringField {
                        field: "source_alignment_policy",
                    });
                }

                if self.source_interval_start_utc_nanos.is_empty() {
                    return Err(
                        MeanReversionAnchorModelValidationError::MissingFieldForAnchorModelKind {
                            anchor_model_kind: self.anchor_model_kind,
                            field: "source_interval_start_utc_nanos",
                        },
                    );
                }
                if self.source_interval_end_utc_nanos.is_empty() {
                    return Err(
                        MeanReversionAnchorModelValidationError::MissingFieldForAnchorModelKind {
                            anchor_model_kind: self.anchor_model_kind,
                            field: "source_interval_end_utc_nanos",
                        },
                    );
                }
                if self.anchor_values.is_empty() {
                    return Err(
                        MeanReversionAnchorModelValidationError::MissingFieldForAnchorModelKind {
                            anchor_model_kind: self.anchor_model_kind,
                            field: "anchor_values",
                        },
                    );
                }

                let expected_length = self.source_interval_start_utc_nanos.len();
                if self.source_interval_end_utc_nanos.len() != expected_length {
                    return Err(
                        MeanReversionAnchorModelValidationError::DeterministicIntervalGridLengthMismatch {
                            field: "source_interval_end_utc_nanos",
                            expected: expected_length,
                            found: self.source_interval_end_utc_nanos.len(),
                        },
                    );
                }
                if self.anchor_values.len() != expected_length {
                    return Err(
                        MeanReversionAnchorModelValidationError::DeterministicIntervalGridLengthMismatch {
                            field: "anchor_values",
                            expected: expected_length,
                            found: self.anchor_values.len(),
                        },
                    );
                }

                let mut previous_interval_end_utc_nanos: Option<i64> = None;
                for interval_index in 0..expected_length {
                    let interval_start_utc_nanos =
                        self.source_interval_start_utc_nanos[interval_index];

                    let interval_end_utc_nanos = self.source_interval_end_utc_nanos[interval_index];
                    if interval_end_utc_nanos <= interval_start_utc_nanos {
                        return Err(
                            MeanReversionAnchorModelValidationError::InvalidIntervalBoundary {
                                interval_index,
                                start_utc_nanos: interval_start_utc_nanos,
                                end_utc_nanos: interval_end_utc_nanos,
                            },
                        );
                    }

                    if let Some(previous_end_utc_nanos) = previous_interval_end_utc_nanos
                        && interval_start_utc_nanos < previous_end_utc_nanos
                    {
                        return Err(
                            MeanReversionAnchorModelValidationError::OverlappingIntervalBoundary {
                                interval_index,
                                previous_end_utc_nanos,
                                start_utc_nanos: interval_start_utc_nanos,
                            },
                        );
                    }

                    validate_finite_number(self.anchor_values[interval_index], "anchor_values")
                        .map_err(|source| {
                            MeanReversionAnchorModelValidationError::NonFiniteField {
                                field: source.field,
                                value: source.value.to_string(),
                            }
                        })?;

                    previous_interval_end_utc_nanos = Some(interval_end_utc_nanos);
                }
            }
        }

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
    pub fn disabled() -> Self {
        Self {
            kappa: 0.0,
            anchor_model: MeanReversionAnchorModel::disabled(),
        }
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

    pub fn is_enabled(&self) -> bool {
        self.anchor_model.anchor_model_kind != MeanReversionAnchorModelKind::Disabled
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

/// Runtime-aligned anchor values shared by seeded and coarse execution planning.
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
                    expected: expected_length,
                    found: self.interval_deltas.len(),
                    field: "interval_deltas",
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
            | Self::InvalidIntervalEndAnchorValue { .. } => None,
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

    let deviation = (-kappa * interval_delta).exp() * boundary_state.deviation
        + core_total_return
        + boundary_state.anchor_value
        - interval_end_anchor_value;
    let observed_log_price = interval_end_anchor_value + deviation;

    MeanReversionBoundaryState::try_new(observed_log_price, interval_end_anchor_value, deviation)
        .map_err(|source| MeanReversionBoundaryStateEvolutionError::InvalidBoundaryState { source })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeanReversionAnchorModelValidationError {
    MissingFieldForAnchorModelKind {
        anchor_model_kind: MeanReversionAnchorModelKind,
        field: &'static str,
    },
    UnexpectedFieldForAnchorModelKind {
        anchor_model_kind: MeanReversionAnchorModelKind,
        field: &'static str,
    },
    EmptyStringField {
        field: &'static str,
    },
    DeterministicIntervalGridLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    InvalidIntervalBoundary {
        interval_index: usize,
        start_utc_nanos: i64,
        end_utc_nanos: i64,
    },
    OverlappingIntervalBoundary {
        interval_index: usize,
        previous_end_utc_nanos: i64,
        start_utc_nanos: i64,
    },
    NonFiniteField {
        field: &'static str,
        value: String,
    },
}

impl Display for MeanReversionAnchorModelValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingFieldForAnchorModelKind {
                anchor_model_kind,
                field,
            } => write!(
                formatter,
                "anchor_model `{anchor_model_kind:?}` requires field `{field}`"
            ),
            Self::UnexpectedFieldForAnchorModelKind {
                anchor_model_kind,
                field,
            } => write!(
                formatter,
                "anchor_model `{anchor_model_kind:?}` must not set field `{field}`"
            ),
            Self::EmptyStringField { field } => {
                write!(formatter, "{field} must be a non-empty string")
            }
            Self::DeterministicIntervalGridLengthMismatch {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "{field} must contain {expected} values, found {found}"
            ),
            Self::InvalidIntervalBoundary {
                interval_index,
                start_utc_nanos,
                end_utc_nanos,
            } => write!(
                formatter,
                "deterministic interval-grid anchor interval {interval_index} must satisfy end > start, found start={start_utc_nanos} end={end_utc_nanos}"
            ),
            Self::OverlappingIntervalBoundary {
                interval_index,
                previous_end_utc_nanos,
                start_utc_nanos,
            } => write!(
                formatter,
                "deterministic interval-grid anchor interval {interval_index} overlaps the previous interval, previous_end={previous_end_utc_nanos} start={start_utc_nanos}"
            ),
            Self::NonFiniteField { field, value } => {
                write!(formatter, "{field} must be a finite number, found {value}")
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

fn validate_absent_deterministic_interval_grid(
    anchor_model: &MeanReversionAnchorModel,
) -> Result<(), MeanReversionAnchorModelValidationError> {
    for field in [
        (
            "source_interval_start_utc_nanos",
            !anchor_model.source_interval_start_utc_nanos.is_empty(),
        ),
        (
            "source_interval_end_utc_nanos",
            !anchor_model.source_interval_end_utc_nanos.is_empty(),
        ),
        ("anchor_values", !anchor_model.anchor_values.is_empty()),
        (
            "source_alignment_policy",
            anchor_model.source_alignment_policy.is_some(),
        ),
    ] {
        if field.1 {
            return Err(
                MeanReversionAnchorModelValidationError::UnexpectedFieldForAnchorModelKind {
                    anchor_model_kind: anchor_model.anchor_model_kind,
                    field: field.0,
                },
            );
        }
    }

    Ok(())
}
