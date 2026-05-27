use serde::{Deserialize, Serialize};

use crate::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformNewsIntensityInput,
};
use janiform_foundations::time::duration_hours_to_nanos;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::long_variance::{
    LongVarianceParameters, LongVarianceParametersError, LongVarianceState, LongVarianceStateError,
};
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionBoundaryStateValidationError,
    MeanReversionFittedParameters, MeanReversionFittedParametersValidationError,
    MeanReversionRuntimeIntervalAnchors, MeanReversionRuntimeIntervalAnchorsValidationError,
};
use janiform_model_kernels::nig::{
    NigSharedPlusShiftConfiguration, NigSharedPlusShiftConfigurationError,
};

use super::constants::{
    DEFAULT_INTRADAY_SUFFICIENT_STATISTICS_INTERVAL_DELTA, NEWS_INTENSITY_COMPONENT_SUM_TOLERANCE,
    SUFFICIENT_STATISTICS_INTERVAL_FACTOR_POLICY,
};
use super::validation::validate_excitation_state;

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsIntervalFactorsError {
    MissingIntervals,
    FieldLengthMismatch {
        field: &'static str,
        expected_length: usize,
        actual_length: usize,
    },
    InvalidSeasonalityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidKnownNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidSurpriseNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    InconsistentNewsIntensityAggregation {
        interval_index: usize,
        nu_h_total: f64,
        nu_h_known: f64,
        nu_h_surprise: f64,
    },
    InvalidIntervalDelta {
        value: f64,
    },
}

impl std::fmt::Display for SufficientStatisticsIntervalFactorsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingIntervals => write!(
                formatter,
                "interval_factors must provide at least one interval"
            ),
            Self::FieldLengthMismatch {
                field,
                expected_length,
                actual_length,
            } => write!(
                formatter,
                "interval_factors.{field} length must match s_h length ({expected_length}), found {actual_length}"
            ),
            Self::InvalidSeasonalityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.s_h[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.nu_h[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidKnownNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.nu_h_known[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidSurpriseNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.nu_h_surprise[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InconsistentNewsIntensityAggregation {
                interval_index,
                nu_h_total,
                nu_h_known,
                nu_h_surprise,
            } => write!(
                formatter,
                "interval_factors.nu_h[{interval_index}] must match nu_h_known + nu_h_surprise within tolerance {NEWS_INTENSITY_COMPONENT_SUM_TOLERANCE}, found total={nu_h_total}, known={nu_h_known}, surprise={nu_h_surprise}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_factors.interval_delta must be finite, strictly positive, and map to an exact integer UTC nanosecond duration when provided, found {value}"
            ),
        }
    }
}

impl std::error::Error for SufficientStatisticsIntervalFactorsError {}

/// Per-interval deterministic factors used by sufficient-statistics workflows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsIntervalFactors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_delta: Option<f64>,
    pub s_h: Box<[f64]>,
    #[serde(skip_serializing)]
    pub nu_h: Box<[f64]>,
    pub nu_h_known: Box<[f64]>,
    pub nu_h_surprise: Box<[f64]>,
}

impl SufficientStatisticsIntervalFactors {
    pub fn try_new(
        s_h: Vec<f64>,
        nu_h_known: Vec<f64>,
        nu_h_surprise: Vec<f64>,
        interval_delta: Option<f64>,
    ) -> Result<Self, SufficientStatisticsIntervalFactorsError> {
        if s_h.is_empty() {
            return Err(SufficientStatisticsIntervalFactorsError::MissingIntervals);
        }
        if nu_h_known.len() != s_h.len() {
            return Err(
                SufficientStatisticsIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_known",
                    expected_length: s_h.len(),
                    actual_length: nu_h_known.len(),
                },
            );
        }
        if nu_h_surprise.len() != s_h.len() {
            return Err(
                SufficientStatisticsIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_surprise",
                    expected_length: s_h.len(),
                    actual_length: nu_h_surprise.len(),
                },
            );
        }

        let nu_h: Vec<f64> = nu_h_known
            .iter()
            .zip(nu_h_surprise.iter())
            .map(|(known_nu_h, surprise_nu_h)| known_nu_h + surprise_nu_h)
            .collect();
        let interval_factors = Self {
            interval_delta,
            s_h: s_h.into_boxed_slice(),
            nu_h: nu_h.into_boxed_slice(),
            nu_h_known: nu_h_known.into_boxed_slice(),
            nu_h_surprise: nu_h_surprise.into_boxed_slice(),
        };
        interval_factors.validate()?;
        Ok(interval_factors)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsIntervalFactorsError> {
        let interval_count = self.s_h.len();
        if interval_count == 0 {
            return Err(SufficientStatisticsIntervalFactorsError::MissingIntervals);
        }

        for (interval_index, value) in self.s_h.iter().copied().enumerate() {
            if !value.is_finite() || value < 0.0 {
                return Err(
                    SufficientStatisticsIntervalFactorsError::InvalidSeasonalityFactor {
                        interval_index,
                        value,
                    },
                );
            }
        }

        for (interval_index, value) in self.nu_h.iter().copied().enumerate() {
            if !value.is_finite() || value < 0.0 {
                return Err(
                    SufficientStatisticsIntervalFactorsError::InvalidNewsIntensityFactor {
                        interval_index,
                        value,
                    },
                );
            }
        }

        if self.nu_h_known.len() != interval_count {
            return Err(
                SufficientStatisticsIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_known",
                    expected_length: interval_count,
                    actual_length: self.nu_h_known.len(),
                },
            );
        }
        if self.nu_h_surprise.len() != interval_count {
            return Err(
                SufficientStatisticsIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_surprise",
                    expected_length: interval_count,
                    actual_length: self.nu_h_surprise.len(),
                },
            );
        }
        if self.nu_h.len() != interval_count {
            return Err(
                SufficientStatisticsIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h",
                    expected_length: interval_count,
                    actual_length: self.nu_h.len(),
                },
            );
        }

        for interval_index in 0..interval_count {
            let known_nu_h = self.nu_h_known[interval_index];
            if !known_nu_h.is_finite() || known_nu_h < 0.0 {
                return Err(
                    SufficientStatisticsIntervalFactorsError::InvalidKnownNewsIntensityFactor {
                        interval_index,
                        value: known_nu_h,
                    },
                );
            }

            let surprise_nu_h = self.nu_h_surprise[interval_index];
            if !surprise_nu_h.is_finite() || surprise_nu_h < 0.0 {
                return Err(
                    SufficientStatisticsIntervalFactorsError::InvalidSurpriseNewsIntensityFactor {
                        interval_index,
                        value: surprise_nu_h,
                    },
                );
            }

            let total_nu_h = self.nu_h[interval_index];
            let component_sum = known_nu_h + surprise_nu_h;
            if !component_sum.is_finite()
                || (component_sum - total_nu_h).abs() > NEWS_INTENSITY_COMPONENT_SUM_TOLERANCE
            {
                return Err(
                    SufficientStatisticsIntervalFactorsError::InconsistentNewsIntensityAggregation {
                        interval_index,
                        nu_h_total: total_nu_h,
                        nu_h_known: known_nu_h,
                        nu_h_surprise: surprise_nu_h,
                    },
                );
            }
        }

        if let Some(interval_delta) = self.interval_delta
            && duration_hours_to_nanos(interval_delta).is_err()
        {
            return Err(
                SufficientStatisticsIntervalFactorsError::InvalidIntervalDelta {
                    value: interval_delta,
                },
            );
        }

        Ok(())
    }

    pub const fn policy() -> &'static str {
        SUFFICIENT_STATISTICS_INTERVAL_FACTOR_POLICY
    }

    pub fn resolved_interval_delta(&self) -> f64 {
        self.interval_delta
            .unwrap_or(DEFAULT_INTRADAY_SUFFICIENT_STATISTICS_INTERVAL_DELTA)
    }

    pub fn news_intensity_input_at(
        &self,
        interval_index: usize,
    ) -> ExponentialAffineTransformNewsIntensityInput {
        let news_intensity_input = ExponentialAffineTransformNewsIntensityInput {
            known_nu_h: self.nu_h_known[interval_index],
            surprise_nu_h: self.nu_h_surprise[interval_index],
        };

        debug_assert!(
            news_intensity_input.validate().is_ok(),
            "validated interval factors must produce valid news intensity inputs"
        );

        news_intensity_input
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsModelInputsError {
    InvalidEquationSpecification {
        source: crate::affine_oracle::ExponentialAffineTransformEquationSpecificationError,
    },
    InvalidNigSharedPlusShiftConfiguration {
        source: NigSharedPlusShiftConfigurationError,
    },
    InvalidInitialState {
        field: &'static str,
        value: f64,
    },
    InvalidLongVarianceParameters {
        source: LongVarianceParametersError,
    },
    InvalidInitialLongVarianceState {
        source: LongVarianceStateError,
    },
    InvalidMeanReversionFittedParameters {
        source: MeanReversionFittedParametersValidationError,
    },
    InvalidMeanReversionRuntimeIntervalAnchors {
        source: MeanReversionRuntimeIntervalAnchorsValidationError,
    },
    InvalidMeanReversionBoundaryState {
        source: MeanReversionBoundaryStateValidationError,
    },
    MeanReversionBoundaryStateStartAnchorMismatch {
        boundary_anchor_value: f64,
        runtime_start_anchor_value: f64,
    },
}

impl std::fmt::Display for SufficientStatisticsModelInputsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEquationSpecification { source } => {
                write!(
                    formatter,
                    "model_inputs.equation_specification is invalid: {source}"
                )
            }
            Self::InvalidNigSharedPlusShiftConfiguration { source } => write!(
                formatter,
                "model_inputs.nig_shared_plus_shift_configuration is invalid: {source}"
            ),
            Self::InvalidInitialState { field, value } => write!(
                formatter,
                "model_inputs.initial_excitation_state.{field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidLongVarianceParameters { source } => {
                write!(
                    formatter,
                    "model_inputs.long_variance_parameters is invalid: {source}"
                )
            }
            Self::InvalidInitialLongVarianceState { source } => write!(
                formatter,
                "model_inputs.initial_long_variance_state is invalid: {source}"
            ),
            Self::InvalidMeanReversionFittedParameters { source } => write!(
                formatter,
                "model_inputs.mean_reversion_fitted_parameters is invalid: {source}"
            ),
            Self::InvalidMeanReversionRuntimeIntervalAnchors { source } => write!(
                formatter,
                "model_inputs.mean_reversion_runtime_interval_anchors is invalid: {source}"
            ),
            Self::InvalidMeanReversionBoundaryState { source } => write!(
                formatter,
                "model_inputs.initial_mean_reversion_boundary_state is invalid: {source}"
            ),
            Self::MeanReversionBoundaryStateStartAnchorMismatch {
                boundary_anchor_value,
                runtime_start_anchor_value,
            } => write!(
                formatter,
                "model_inputs.initial_mean_reversion_boundary_state.anchor_value ({boundary_anchor_value}) must match model_inputs.mean_reversion_runtime_interval_anchors.start_anchor_value ({runtime_start_anchor_value})"
            ),
        }
    }
}

impl std::error::Error for SufficientStatisticsModelInputsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidEquationSpecification { source } => Some(source),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => Some(source),
            Self::InvalidLongVarianceParameters { source } => Some(source),
            Self::InvalidInitialLongVarianceState { source } => Some(source),
            Self::InvalidMeanReversionFittedParameters { source } => Some(source),
            Self::InvalidMeanReversionRuntimeIntervalAnchors { source } => Some(source),
            Self::InvalidMeanReversionBoundaryState { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsModelInputs {
    pub equation_specification: ExponentialAffineTransformEquationSpecification,
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    pub initial_excitation_state: HawkesExcitationState,
    pub long_variance_parameters: LongVarianceParameters,
    pub initial_long_variance_state: LongVarianceState,
    pub mean_reversion_fitted_parameters: MeanReversionFittedParameters,
    pub mean_reversion_runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors,
    pub initial_mean_reversion_boundary_state: MeanReversionBoundaryState,
}

impl SufficientStatisticsModelInputs {
    pub fn try_new(model_inputs: Self) -> Result<Self, SufficientStatisticsModelInputsError> {
        model_inputs.validate()?;

        Ok(model_inputs)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsModelInputsError> {
        self.equation_specification.validate().map_err(|source| {
            SufficientStatisticsModelInputsError::InvalidEquationSpecification { source }
        })?;

        self.nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .map_err(|source| {
                SufficientStatisticsModelInputsError::InvalidNigSharedPlusShiftConfiguration {
                    source,
                }
            })?;

        validate_excitation_state(self.initial_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsModelInputsError::InvalidInitialState { field, value }
        })?;

        self.long_variance_parameters.validate().map_err(|source| {
            SufficientStatisticsModelInputsError::InvalidLongVarianceParameters { source }
        })?;

        self.initial_long_variance_state
            .validate()
            .map_err(|source| {
                SufficientStatisticsModelInputsError::InvalidInitialLongVarianceState { source }
            })?;

        self.mean_reversion_fitted_parameters
            .validate()
            .map_err(|source| {
                SufficientStatisticsModelInputsError::InvalidMeanReversionFittedParameters {
                    source,
                }
            })?;

        self.mean_reversion_runtime_interval_anchors
            .validate()
            .map_err(|source| {
                SufficientStatisticsModelInputsError::InvalidMeanReversionRuntimeIntervalAnchors {
                    source,
                }
            })?;

        self.initial_mean_reversion_boundary_state
            .validate()
            .map_err(|source| {
                SufficientStatisticsModelInputsError::InvalidMeanReversionBoundaryState { source }
            })?;

        if self.initial_mean_reversion_boundary_state.anchor_value
            != self
                .mean_reversion_runtime_interval_anchors
                .start_anchor_value
        {
            return Err(
                SufficientStatisticsModelInputsError::MeanReversionBoundaryStateStartAnchorMismatch {
                    boundary_anchor_value: self.initial_mean_reversion_boundary_state.anchor_value,
                    runtime_start_anchor_value: self
                        .mean_reversion_runtime_interval_anchors
                        .start_anchor_value,
                },
            );
        }

        if self.initial_mean_reversion_boundary_state.pressure_state
            != self
                .mean_reversion_runtime_interval_anchors
                .start_pressure_state
        {
            return Err(
                SufficientStatisticsModelInputsError::MeanReversionBoundaryStateStartAnchorMismatch {
                    boundary_anchor_value: self.initial_mean_reversion_boundary_state.pressure_state,
                    runtime_start_anchor_value: self
                        .mean_reversion_runtime_interval_anchors
                        .start_pressure_state,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsIntervalFactorsPayload {
    #[serde(default)]
    interval_delta: Option<f64>,
    s_h: Vec<f64>,
    nu_h_known: Vec<f64>,
    nu_h_surprise: Vec<f64>,
}

impl<'de> Deserialize<'de> for SufficientStatisticsIntervalFactors {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsIntervalFactorsPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.s_h,
            payload.nu_h_known,
            payload.nu_h_surprise,
            payload.interval_delta,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsModelInputsPayload {
    equation_specification: ExponentialAffineTransformEquationSpecification,
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    initial_excitation_state: HawkesExcitationState,
    long_variance_parameters: LongVarianceParameters,
    initial_long_variance_state: LongVarianceState,
    mean_reversion_fitted_parameters: MeanReversionFittedParameters,
    mean_reversion_runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors,
    initial_mean_reversion_boundary_state: MeanReversionBoundaryState,
}

impl<'de> Deserialize<'de> for SufficientStatisticsModelInputs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsModelInputsPayload::deserialize(deserializer)?;

        Self::try_new(Self {
            equation_specification: payload.equation_specification,
            nig_shared_plus_shift_configuration: payload.nig_shared_plus_shift_configuration,
            initial_excitation_state: payload.initial_excitation_state,
            long_variance_parameters: payload.long_variance_parameters,
            initial_long_variance_state: payload.initial_long_variance_state,
            mean_reversion_fitted_parameters: payload.mean_reversion_fitted_parameters,
            mean_reversion_runtime_interval_anchors: payload
                .mean_reversion_runtime_interval_anchors,
            initial_mean_reversion_boundary_state: payload.initial_mean_reversion_boundary_state,
        })
        .map_err(serde::de::Error::custom)
    }
}
