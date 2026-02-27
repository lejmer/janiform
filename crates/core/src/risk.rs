//! Risk-sampling adapters and public contracts for sufficient-statistics paths.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

use crate::affine_oracle::{
    ExponentialAffineTransformEquationSpecification,
    ExponentialAffineTransformEquationSpecificationError, ExponentialAffineTransformInput,
    ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError,
    ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_optional_settings,
};
use crate::hawkes::HawkesExcitationState;
use crate::nig::{NigSharedPlusShiftConfiguration, NigSharedPlusShiftConfigurationError};

pub const SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.run_request.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.path.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.path_record.v1";
pub const SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY: &str =
    "explicit_interval_aligned_s_h_and_nu_h_vectors_without_implicit_resampling.v1";

/// Per-interval deterministic factors used by the sufficient-statistics risk sampler.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskIntervalFactors {
    pub s_h: Box<[f64]>,
    pub nu_h: Box<[f64]>,
}

impl SufficientStatisticsRiskIntervalFactors {
    pub fn try_new(
        s_h: Vec<f64>,
        nu_h: Vec<f64>,
    ) -> Result<Self, SufficientStatisticsRiskIntervalFactorsError> {
        let interval_factors = Self {
            s_h: s_h.into_boxed_slice(),
            nu_h: nu_h.into_boxed_slice(),
        };
        interval_factors.validate()?;
        Ok(interval_factors)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsRiskIntervalFactorsError> {
        let interval_count = self.s_h.len();
        if interval_count == 0 {
            return Err(SufficientStatisticsRiskIntervalFactorsError::MissingIntervals);
        }

        if self.nu_h.len() != interval_count {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h",
                    expected_length: interval_count,
                    actual_length: self.nu_h.len(),
                },
            );
        }

        for (interval_index, value) in self.s_h.iter().copied().enumerate() {
            if !value.is_finite() || value < 0.0 {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InvalidSeasonalityFactor {
                        interval_index,
                        value,
                    },
                );
            }
        }

        for (interval_index, value) in self.nu_h.iter().copied().enumerate() {
            if !value.is_finite() || value < 0.0 {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InvalidNewsIntensityFactor {
                        interval_index,
                        value,
                    },
                );
            }
        }

        Ok(())
    }

    pub const fn policy() -> &'static str {
        SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskIntervalFactorsError {
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
}

impl Display for SufficientStatisticsRiskIntervalFactorsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
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
        }
    }
}

impl Error for SufficientStatisticsRiskIntervalFactorsError {}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskModelInputs {
    pub equation_specification: ExponentialAffineTransformEquationSpecification,
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    pub initial_excitation_state: HawkesExcitationState,
}

impl SufficientStatisticsRiskModelInputs {
    pub fn try_new(
        equation_specification: ExponentialAffineTransformEquationSpecification,
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
        initial_excitation_state: HawkesExcitationState,
    ) -> Result<Self, SufficientStatisticsRiskModelInputsError> {
        let model_inputs = Self {
            equation_specification,
            nig_shared_plus_shift_configuration,
            initial_excitation_state,
        };
        model_inputs.validate()?;
        Ok(model_inputs)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsRiskModelInputsError> {
        self.equation_specification.validate().map_err(|source| {
            SufficientStatisticsRiskModelInputsError::InvalidEquationSpecification { source }
        })?;

        self.nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .map_err(|source| {
                SufficientStatisticsRiskModelInputsError::InvalidNigSharedPlusShiftConfiguration {
                    source,
                }
            })?;

        validate_excitation_state(self.initial_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsRiskModelInputsError::InvalidInitialState { field, value }
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskModelInputsError {
    InvalidEquationSpecification {
        source: ExponentialAffineTransformEquationSpecificationError,
    },
    InvalidNigSharedPlusShiftConfiguration {
        source: NigSharedPlusShiftConfigurationError,
    },
    InvalidInitialState {
        field: &'static str,
        value: f64,
    },
}

impl Display for SufficientStatisticsRiskModelInputsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
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
        }
    }
}

impl Error for SufficientStatisticsRiskModelInputsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidEquationSpecification { source } => Some(source),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => Some(source),
            _ => None,
        }
    }
}

/// Public run request contract for sufficient-statistics risk simulation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskRunRequest {
    pub seed: u64,
    pub horizon_interval_count: usize,
    pub interval_delta: f64,
    pub model_inputs: SufficientStatisticsRiskModelInputs,
    pub interval_factors: SufficientStatisticsRiskIntervalFactors,
}

impl SufficientStatisticsRiskRunRequest {
    pub fn try_new(
        seed: u64,
        horizon_interval_count: usize,
        interval_delta: f64,
        model_inputs: SufficientStatisticsRiskModelInputs,
        interval_factors: SufficientStatisticsRiskIntervalFactors,
    ) -> Result<Self, SufficientStatisticsRiskRunRequestError> {
        let run_request = Self {
            seed,
            horizon_interval_count,
            interval_delta,
            model_inputs,
            interval_factors,
        };
        run_request.validate()?;
        Ok(run_request)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsRiskRunRequestError> {
        if self.horizon_interval_count == 0 {
            return Err(
                SufficientStatisticsRiskRunRequestError::InvalidHorizonIntervalCount { value: 0 },
            );
        }

        if !self.interval_delta.is_finite() || self.interval_delta <= 0.0 {
            return Err(
                SufficientStatisticsRiskRunRequestError::InvalidIntervalDelta {
                    value: self.interval_delta,
                },
            );
        }

        self.model_inputs.validate().map_err(|source| {
            SufficientStatisticsRiskRunRequestError::InvalidModelInputs { source }
        })?;

        self.interval_factors.validate().map_err(|source| {
            SufficientStatisticsRiskRunRequestError::InvalidIntervalFactors { source }
        })?;

        let available_interval_count = self.interval_factors.s_h.len();
        if self.horizon_interval_count > available_interval_count {
            return Err(
                SufficientStatisticsRiskRunRequestError::HorizonExceedsAvailableIntervals {
                    requested_interval_count: self.horizon_interval_count,
                    available_interval_count,
                },
            );
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskRunRequestError {
    InvalidHorizonIntervalCount {
        value: usize,
    },
    InvalidIntervalDelta {
        value: f64,
    },
    InvalidModelInputs {
        source: SufficientStatisticsRiskModelInputsError,
    },
    InvalidIntervalFactors {
        source: SufficientStatisticsRiskIntervalFactorsError,
    },
    HorizonExceedsAvailableIntervals {
        requested_interval_count: usize,
        available_interval_count: usize,
    },
}

impl Display for SufficientStatisticsRiskRunRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be strictly positive, found {value}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidModelInputs { source } => {
                write!(formatter, "model_inputs payload is invalid: {source}")
            }
            Self::InvalidIntervalFactors { source } => {
                write!(formatter, "interval_factors payload is invalid: {source}")
            }
            Self::HorizonExceedsAvailableIntervals {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "horizon_interval_count ({requested_interval_count}) exceeds available interval_factors entries ({available_interval_count})"
            ),
        }
    }
}

impl Error for SufficientStatisticsRiskRunRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidModelInputs { source } => Some(source),
            Self::InvalidIntervalFactors { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsIntervalStatistics {
    pub a_plus: f64,
    pub a_minus: f64,
    pub n_plus: u64,
    pub n_minus: u64,
}

impl SufficientStatisticsIntervalStatistics {
    pub fn try_new(
        a_plus: f64,
        a_minus: f64,
        n_plus: u64,
        n_minus: u64,
    ) -> Result<Self, SufficientStatisticsIntervalStatisticsError> {
        let interval_statistics = Self {
            a_plus,
            a_minus,
            n_plus,
            n_minus,
        };
        interval_statistics.validate()?;
        Ok(interval_statistics)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsIntervalStatisticsError> {
        if !self.a_plus.is_finite() || self.a_plus < 0.0 {
            return Err(SufficientStatisticsIntervalStatisticsError::InvalidAPlus {
                value: self.a_plus,
            });
        }
        if !self.a_minus.is_finite() || self.a_minus < 0.0 {
            return Err(SufficientStatisticsIntervalStatisticsError::InvalidAMinus {
                value: self.a_minus,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsIntervalStatisticsError {
    InvalidAPlus { value: f64 },
    InvalidAMinus { value: f64 },
}

impl Display for SufficientStatisticsIntervalStatisticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAPlus { value } => {
                write!(
                    formatter,
                    "a_plus must be finite and non-negative, found {value}"
                )
            }
            Self::InvalidAMinus { value } => {
                write!(
                    formatter,
                    "a_minus must be finite and non-negative, found {value}"
                )
            }
        }
    }
}

impl Error for SufficientStatisticsIntervalStatisticsError {}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskReturnComponents {
    pub buy_side_return: f64,
    pub sell_side_return: f64,
    pub trading_time_return: f64,
    pub gap_return: f64,
    pub total_return: f64,
}

impl SufficientStatisticsRiskReturnComponents {
    pub fn try_new(
        buy_side_return: f64,
        sell_side_return: f64,
        trading_time_return: f64,
        gap_return: f64,
        total_return: f64,
    ) -> Result<Self, SufficientStatisticsRiskReturnComponentsError> {
        let return_components = Self {
            buy_side_return,
            sell_side_return,
            trading_time_return,
            gap_return,
            total_return,
        };
        return_components.validate()?;
        Ok(return_components)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsRiskReturnComponentsError> {
        validate_finite_component("buy_side_return", self.buy_side_return)?;
        validate_finite_component("sell_side_return", self.sell_side_return)?;
        validate_finite_component("trading_time_return", self.trading_time_return)?;
        validate_finite_component("gap_return", self.gap_return)?;
        validate_finite_component("total_return", self.total_return)?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskReturnComponentsError {
    InvalidFiniteComponent { field: &'static str, value: f64 },
}

impl Display for SufficientStatisticsRiskReturnComponentsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFiniteComponent { field, value } => {
                write!(formatter, "{field} must be finite, found {value}")
            }
        }
    }
}

impl Error for SufficientStatisticsRiskReturnComponentsError {}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskPathRecord {
    pub schema_version: &'static str,
    pub interval_index: usize,
    pub sufficient_statistics: SufficientStatisticsIntervalStatistics,
    pub return_components: SufficientStatisticsRiskReturnComponents,
}

impl SufficientStatisticsRiskPathRecord {
    pub fn try_new(
        interval_index: usize,
        sufficient_statistics: SufficientStatisticsIntervalStatistics,
        return_components: SufficientStatisticsRiskReturnComponents,
    ) -> Result<Self, SufficientStatisticsRiskPathRecordError> {
        let path_record = Self {
            schema_version: SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION,
            interval_index,
            sufficient_statistics,
            return_components,
        };
        path_record.validate()?;
        Ok(path_record)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsRiskPathRecordError> {
        self.sufficient_statistics.validate().map_err(|source| {
            SufficientStatisticsRiskPathRecordError::InvalidSufficientStatistics { source }
        })?;
        self.return_components.validate().map_err(|source| {
            SufficientStatisticsRiskPathRecordError::InvalidReturnComponents { source }
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskPathRecordError {
    InvalidSufficientStatistics {
        source: SufficientStatisticsIntervalStatisticsError,
    },
    InvalidReturnComponents {
        source: SufficientStatisticsRiskReturnComponentsError,
    },
}

impl Display for SufficientStatisticsRiskPathRecordError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSufficientStatistics { source } => {
                write!(formatter, "sufficient_statistics is invalid: {source}")
            }
            Self::InvalidReturnComponents { source } => {
                write!(formatter, "return_components is invalid: {source}")
            }
        }
    }
}

impl Error for SufficientStatisticsRiskPathRecordError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSufficientStatistics { source } => Some(source),
            Self::InvalidReturnComponents { source } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskPath {
    pub schema_version: &'static str,
    pub run_request_schema_version: &'static str,
    pub seed: u64,
    pub interval_delta: f64,
    pub simulated_interval_count: usize,
    pub initial_excitation_state: HawkesExcitationState,
    pub terminal_excitation_state: HawkesExcitationState,
    pub records: Vec<SufficientStatisticsRiskPathRecord>,
}

impl SufficientStatisticsRiskPath {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        seed: u64,
        interval_delta: f64,
        simulated_interval_count: usize,
        initial_excitation_state: HawkesExcitationState,
        terminal_excitation_state: HawkesExcitationState,
        records: Vec<SufficientStatisticsRiskPathRecord>,
    ) -> Result<Self, SufficientStatisticsRiskPathError> {
        let path = Self {
            schema_version: SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION,
            run_request_schema_version: SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION,
            seed,
            interval_delta,
            simulated_interval_count,
            initial_excitation_state,
            terminal_excitation_state,
            records,
        };
        path.validate()?;
        Ok(path)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsRiskPathError> {
        if !self.interval_delta.is_finite() || self.interval_delta <= 0.0 {
            return Err(SufficientStatisticsRiskPathError::InvalidIntervalDelta {
                value: self.interval_delta,
            });
        }

        if self.simulated_interval_count == 0 {
            return Err(
                SufficientStatisticsRiskPathError::InvalidSimulatedIntervalCount { value: 0 },
            );
        }

        if self.records.len() != self.simulated_interval_count {
            return Err(SufficientStatisticsRiskPathError::RecordCountMismatch {
                simulated_interval_count: self.simulated_interval_count,
                record_count: self.records.len(),
            });
        }

        validate_excitation_state(self.initial_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsRiskPathError::InvalidInitialState { field, value }
        })?;
        validate_excitation_state(self.terminal_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsRiskPathError::InvalidTerminalState { field, value }
        })?;

        for (record_position, record) in self.records.iter().enumerate() {
            if record.interval_index != record_position {
                return Err(
                    SufficientStatisticsRiskPathError::NonSequentialIntervalIndex {
                        record_position,
                        record_interval_index: record.interval_index,
                    },
                );
            }

            record.validate().map_err(|source| {
                SufficientStatisticsRiskPathError::InvalidRecord {
                    record_position,
                    source,
                }
            })?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskPathError {
    InvalidIntervalDelta {
        value: f64,
    },
    InvalidSimulatedIntervalCount {
        value: usize,
    },
    RecordCountMismatch {
        simulated_interval_count: usize,
        record_count: usize,
    },
    InvalidInitialState {
        field: &'static str,
        value: f64,
    },
    InvalidTerminalState {
        field: &'static str,
        value: f64,
    },
    NonSequentialIntervalIndex {
        record_position: usize,
        record_interval_index: usize,
    },
    InvalidRecord {
        record_position: usize,
        source: SufficientStatisticsRiskPathRecordError,
    },
}

impl Display for SufficientStatisticsRiskPathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidSimulatedIntervalCount { value } => write!(
                formatter,
                "simulated_interval_count must be strictly positive, found {value}"
            ),
            Self::RecordCountMismatch {
                simulated_interval_count,
                record_count,
            } => write!(
                formatter,
                "simulated_interval_count ({simulated_interval_count}) must match records length ({record_count})"
            ),
            Self::InvalidInitialState { field, value } => write!(
                formatter,
                "initial_excitation_state.{field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidTerminalState { field, value } => write!(
                formatter,
                "terminal_excitation_state.{field} must be finite and non-negative, found {value}"
            ),
            Self::NonSequentialIntervalIndex {
                record_position,
                record_interval_index,
            } => write!(
                formatter,
                "records[{record_position}].interval_index must equal {record_position}, found {record_interval_index}"
            ),
            Self::InvalidRecord {
                record_position,
                source,
            } => write!(formatter, "records[{record_position}] is invalid: {source}"),
        }
    }
}

impl Error for SufficientStatisticsRiskPathError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRecord { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskIntervalFactorsPayload {
    s_h: Vec<f64>,
    nu_h: Vec<f64>,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskIntervalFactors {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskIntervalFactorsPayload::deserialize(deserializer)?;
        Self::try_new(payload.s_h, payload.nu_h).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskModelInputsPayload {
    equation_specification: ExponentialAffineTransformEquationSpecification,
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    initial_excitation_state: HawkesExcitationState,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskModelInputs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskModelInputsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.equation_specification,
            payload.nig_shared_plus_shift_configuration,
            payload.initial_excitation_state,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskRunRequestPayload {
    seed: u64,
    horizon_interval_count: usize,
    interval_delta: f64,
    model_inputs: SufficientStatisticsRiskModelInputs,
    interval_factors: SufficientStatisticsRiskIntervalFactors,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskRunRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskRunRequestPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.seed,
            payload.horizon_interval_count,
            payload.interval_delta,
            payload.model_inputs,
            payload.interval_factors,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsIntervalStatisticsPayload {
    a_plus: f64,
    a_minus: f64,
    n_plus: u64,
    n_minus: u64,
}

impl<'de> Deserialize<'de> for SufficientStatisticsIntervalStatistics {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsIntervalStatisticsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.a_plus,
            payload.a_minus,
            payload.n_plus,
            payload.n_minus,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskReturnComponentsPayload {
    buy_side_return: f64,
    sell_side_return: f64,
    trading_time_return: f64,
    gap_return: f64,
    total_return: f64,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskReturnComponents {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskReturnComponentsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.buy_side_return,
            payload.sell_side_return,
            payload.trading_time_return,
            payload.gap_return,
            payload.total_return,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathRecordPayload {
    #[serde(default)]
    schema_version: Option<String>,
    interval_index: usize,
    sufficient_statistics: SufficientStatisticsIntervalStatistics,
    return_components: SufficientStatisticsRiskReturnComponents,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskPathRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskPathRecordPayload::deserialize(deserializer)?;

        if let Some(schema_version) = payload.schema_version.as_deref()
            && schema_version != SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION
        {
            return Err(serde::de::Error::custom(format!(
                "schema_version must be {}, found {schema_version}",
                SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION
            )));
        }

        Self::try_new(
            payload.interval_index,
            payload.sufficient_statistics,
            payload.return_components,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathPayload {
    #[serde(default)]
    schema_version: Option<String>,
    #[serde(default)]
    run_request_schema_version: Option<String>,
    seed: u64,
    interval_delta: f64,
    simulated_interval_count: usize,
    initial_excitation_state: HawkesExcitationState,
    terminal_excitation_state: HawkesExcitationState,
    records: Vec<SufficientStatisticsRiskPathRecord>,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskPathPayload::deserialize(deserializer)?;

        if let Some(schema_version) = payload.schema_version.as_deref()
            && schema_version != SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION
        {
            return Err(serde::de::Error::custom(format!(
                "schema_version must be {}, found {schema_version}",
                SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION
            )));
        }

        if let Some(run_request_schema_version) = payload.run_request_schema_version.as_deref()
            && run_request_schema_version != SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
        {
            return Err(serde::de::Error::custom(format!(
                "run_request_schema_version must be {}, found {run_request_schema_version}",
                SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
            )));
        }

        Self::try_new(
            payload.seed,
            payload.interval_delta,
            payload.simulated_interval_count,
            payload.initial_excitation_state,
            payload.terminal_excitation_state,
            payload.records,
        )
        .map_err(serde::de::Error::custom)
    }
}

fn validate_finite_component(
    field: &'static str,
    value: f64,
) -> Result<(), SufficientStatisticsRiskReturnComponentsError> {
    if !value.is_finite() {
        return Err(
            SufficientStatisticsRiskReturnComponentsError::InvalidFiniteComponent { field, value },
        );
    }
    Ok(())
}

fn validate_excitation_state(state: HawkesExcitationState) -> Result<(), (&'static str, f64)> {
    validate_non_negative_finite_component("y_plus_plus", state.y_plus_plus)?;
    validate_non_negative_finite_component("y_plus_minus", state.y_plus_minus)?;
    validate_non_negative_finite_component("y_minus_plus", state.y_minus_plus)?;
    validate_non_negative_finite_component("y_minus_minus", state.y_minus_minus)?;

    Ok(())
}

fn validate_non_negative_finite_component(
    field: &'static str,
    value: f64,
) -> Result<(), (&'static str, f64)> {
    if !value.is_finite() || value < 0.0 {
        return Err((field, value));
    }
    Ok(())
}

/// Solve one-interval exponential-affine transform for sufficient-statistics risk sampling.
///
/// This adapter stays conversion-focused and reuses the same shared solver entrypoint as pricing.
pub fn solve_exponential_affine_transform_for_risk_sampling(
    transform_input: ExponentialAffineTransformInput,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(transform_input, None)
}

/// Solve one-interval exponential-affine transform for risk sampling with explicit settings.
pub fn solve_exponential_affine_transform_for_risk_sampling_with_settings(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(
        transform_input,
        Some(solver_settings),
    )
}

#[cfg(test)]
mod tests {
    use crate::affine_oracle::ExponentialAffineTransformEquationSpecification;
    use crate::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
    use crate::news::GammaSeverityMarkConfiguration;
    use crate::nig::NigSharedPlusShiftConfiguration;

    use super::{
        SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
        SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION,
        SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION,
        SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION,
        SufficientStatisticsIntervalStatistics, SufficientStatisticsRiskIntervalFactors,
        SufficientStatisticsRiskModelInputs, SufficientStatisticsRiskPath,
        SufficientStatisticsRiskPathError, SufficientStatisticsRiskPathRecord,
        SufficientStatisticsRiskReturnComponents, SufficientStatisticsRiskRunRequest,
        SufficientStatisticsRiskRunRequestError,
    };

    #[test]
    fn run_request_round_trip_serialization_preserves_contract_shape() {
        let run_request = build_valid_run_request();

        let serialized =
            serde_json::to_value(&run_request).expect("run request payload should serialize");
        assert_eq!(serialized["seed"], serde_json::json!(17));
        assert_eq!(serialized["horizon_interval_count"], serde_json::json!(2));
        assert_eq!(serialized["interval_delta"], serde_json::json!(1.0));
        assert_eq!(
            SufficientStatisticsRiskRunRequest::schema_version(),
            SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
        );
        assert_eq!(
            SufficientStatisticsRiskIntervalFactors::policy(),
            SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY
        );

        let parsed: SufficientStatisticsRiskRunRequest =
            serde_json::from_value(serialized).expect("serialized payload should deserialize");
        assert_eq!(parsed, run_request);
    }

    #[test]
    fn path_round_trip_serialization_preserves_interval_statistics_and_return_components() {
        let record = SufficientStatisticsRiskPathRecord::try_new(
            0,
            SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 3, 2)
                .expect("statistics should be valid"),
            SufficientStatisticsRiskReturnComponents::try_new(0.04, -0.03, 0.01, -0.02, -0.01)
                .expect("return components should be valid"),
        )
        .expect("record should be valid");
        let path = SufficientStatisticsRiskPath::try_new(
            17,
            1.0,
            1,
            build_initial_state(),
            build_initial_state(),
            vec![record],
        )
        .expect("path should be valid");

        let serialized = serde_json::to_value(&path).expect("path payload should serialize");
        assert_eq!(
            serialized["schema_version"],
            serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION)
        );
        assert_eq!(
            serialized["records"][0]["schema_version"],
            serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION)
        );
        assert_eq!(
            serialized["records"][0]["sufficient_statistics"]["n_plus"],
            serde_json::json!(3)
        );
        assert_eq!(
            serialized["records"][0]["return_components"]["total_return"],
            serde_json::json!(-0.01)
        );

        let parsed: SufficientStatisticsRiskPath =
            serde_json::from_value(serialized).expect("serialized path should deserialize");
        assert_eq!(parsed, path);
    }

    #[test]
    fn run_request_validation_rejects_invalid_values_with_structured_errors() {
        let invalid_horizon = SufficientStatisticsRiskRunRequest::try_new(
            7,
            0,
            1.0,
            build_model_inputs(),
            build_interval_factors(),
        )
        .expect_err("zero horizon must fail validation");
        assert_eq!(
            invalid_horizon,
            SufficientStatisticsRiskRunRequestError::InvalidHorizonIntervalCount { value: 0 }
        );

        let invalid_interval_delta = SufficientStatisticsRiskRunRequest::try_new(
            7,
            1,
            0.0,
            build_model_inputs(),
            build_interval_factors(),
        )
        .expect_err("non-positive interval delta must fail validation");
        assert_eq!(
            invalid_interval_delta,
            SufficientStatisticsRiskRunRequestError::InvalidIntervalDelta { value: 0.0 }
        );
    }

    #[test]
    fn deserialization_rejects_incomplete_or_malformed_request_payloads() {
        let missing_model_inputs_payload = serde_json::json!({
            "seed": 17,
            "horizon_interval_count": 2,
            "interval_delta": 1.0,
            "interval_factors": {
                "s_h": [1.1, 0.9],
                "nu_h": [0.2, 0.1]
            }
        });
        let missing_model_inputs_error =
            serde_json::from_value::<SufficientStatisticsRiskRunRequest>(
                missing_model_inputs_payload,
            )
            .expect_err("missing model_inputs must fail deserialization");

        assert!(
            missing_model_inputs_error
                .to_string()
                .contains("missing field `model_inputs`")
        );

        let malformed_factors_payload = serde_json::json!({
            "seed": 17,
            "horizon_interval_count": 2,
            "interval_delta": 1.0,
            "model_inputs": {
                "equation_specification": {
                    "hawkes_kernel_parameters": {
                        "lambda_bar": 0.8,
                        "beta": 1.6,
                        "alpha": {
                            "plus_plus": 0.2,
                            "plus_minus": 0.1,
                            "minus_plus": 0.1,
                            "minus_minus": 0.25
                        },
                        "rho": {
                            "plus_plus": 0.08,
                            "plus_minus": 0.04,
                            "minus_plus": 0.05,
                            "minus_minus": 0.1
                        }
                    },
                    "gamma_severity_mark_configuration": {
                        "shape_k": 1.8,
                        "scale_theta": 0.7
                    }
                },
                "nig_shared_plus_shift_configuration": {
                    "alpha": 2.3,
                    "beta_0": -0.8,
                    "delta": 0.7,
                    "mu": 0.1,
                    "delta_beta": -0.2
                },
                "initial_excitation_state": {
                    "y_plus_plus": 0.2,
                    "y_plus_minus": 0.1,
                    "y_minus_plus": 0.3,
                    "y_minus_minus": 0.4
                }
            },
            "interval_factors": {
                "s_h": [1.1, 0.9],
                "nu_h": [0.2]
            }
        });
        let malformed_factors_error =
            serde_json::from_value::<SufficientStatisticsRiskRunRequest>(malformed_factors_payload)
                .expect_err("mismatched interval_factors lengths must fail deserialization");

        assert!(
            malformed_factors_error
                .to_string()
                .contains("interval_factors.nu_h length must match s_h length")
        );
    }

    #[test]
    fn path_validation_rejects_non_sequential_interval_indices() {
        let first_record = SufficientStatisticsRiskPathRecord::try_new(
            1,
            SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 3, 2)
                .expect("statistics should be valid"),
            SufficientStatisticsRiskReturnComponents::try_new(0.04, -0.03, 0.01, -0.02, -0.01)
                .expect("return components should be valid"),
        )
        .expect("record should be valid");

        let error = SufficientStatisticsRiskPath::try_new(
            17,
            1.0,
            1,
            build_initial_state(),
            build_initial_state(),
            vec![first_record],
        )
        .expect_err("non-sequential interval index must fail validation");
        assert_eq!(
            error,
            SufficientStatisticsRiskPathError::NonSequentialIntervalIndex {
                record_position: 0,
                record_interval_index: 1,
            }
        );
    }

    fn build_valid_run_request() -> SufficientStatisticsRiskRunRequest {
        SufficientStatisticsRiskRunRequest::try_new(
            17,
            2,
            1.0,
            build_model_inputs(),
            build_interval_factors(),
        )
        .expect("run request should be valid")
    }

    fn build_model_inputs() -> SufficientStatisticsRiskModelInputs {
        let kernel_parameters = HawkesKernelParameters::try_new(
            0.8,
            1.6,
            HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.1,
                minus_plus: 0.1,
                minus_minus: 0.25,
            },
            HawkesKernelLoadings {
                plus_plus: 0.08,
                plus_minus: 0.04,
                minus_plus: 0.05,
                minus_minus: 0.1,
            },
        )
        .expect("kernel parameters should be valid");
        let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
            kernel_parameters,
            GammaSeverityMarkConfiguration::try_new(1.8, 0.7).expect("gamma marks should be valid"),
        )
        .expect("equation specification should be valid");

        SufficientStatisticsRiskModelInputs::try_new(
            equation_specification,
            NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
                .expect("nig configuration should be valid"),
            build_initial_state(),
        )
        .expect("model inputs should be valid")
    }

    fn build_interval_factors() -> SufficientStatisticsRiskIntervalFactors {
        SufficientStatisticsRiskIntervalFactors::try_new(vec![1.1, 0.9], vec![0.2, 0.1])
            .expect("interval factors should be valid")
    }

    fn build_initial_state() -> HawkesExcitationState {
        HawkesExcitationState {
            y_plus_plus: 0.2,
            y_plus_minus: 0.1,
            y_minus_plus: 0.3,
            y_minus_minus: 0.4,
        }
    }
}
