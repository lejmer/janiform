use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsModelInputs, SufficientStatisticsModelInputsError,
};
use janiform_market_time::calendar::{
    ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact, ExchangeCalendarRegistryLoadError,
    ExchangeCalendarRegistryLookupError,
};
use janiform_model_kernels::long_variance::{
    LongVarianceParameters, LongVarianceParametersError, LongVarianceState, LongVarianceStateError,
};
use janiform_model_kernels::news::{NewsProcessConfiguration, NewsProcessConfigurationError};
use janiform_model_kernels::return_clock::{
    ReturnClockExposureParameters, ReturnClockExposureParametersError,
};
use janiform_model_kernels::timestamp_batching_surface::{
    TimestampBatchingSurface, TimestampBatchingSurfaceError,
};

use crate::core::is_valid_observed_price_tick_size;

/// Simulation-specific coarse model inputs that extend the shared sufficient-statistics sampler state
/// with canonical news-process and calendar-routing configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseSimulationModelInputs {
    pub sufficient_statistics_model_inputs: SufficientStatisticsModelInputs,
    pub news_process_configuration: NewsProcessConfiguration,
    pub return_clock_exposure_parameters: ReturnClockExposureParameters,
    pub long_variance_parameters: LongVarianceParameters,
    pub initial_long_variance_state: LongVarianceState,
    pub observed_price_tick_size: Option<f64>,
    pub timestamp_batching_surface: TimestampBatchingSurface,
    pub calendar_registry_artifact: ExchangeCalendarRegistryArtifact,
    pub exchange_code: String,
}

impl CoarseSimulationModelInputs {
    pub fn try_new(model_inputs: Self) -> Result<Self, CoarseSimulationModelInputsError> {
        model_inputs.validate()?;

        Ok(model_inputs)
    }

    pub fn validate(&self) -> Result<(), CoarseSimulationModelInputsError> {
        self.sufficient_statistics_model_inputs
            .validate()
            .map_err(|source| {
                CoarseSimulationModelInputsError::InvalidSufficientStatisticsModelInputs { source }
            })?;

        self.news_process_configuration
            .validate()
            .map_err(|source| {
                CoarseSimulationModelInputsError::InvalidNewsProcessConfiguration { source }
            })?;

        self.return_clock_exposure_parameters
            .validate()
            .map_err(|source| {
                CoarseSimulationModelInputsError::InvalidReturnClockExposureParameters { source }
            })?;

        self.long_variance_parameters.validate().map_err(|source| {
            CoarseSimulationModelInputsError::InvalidLongVarianceParameters { source }
        })?;

        self.initial_long_variance_state
            .validate()
            .map_err(|source| {
                CoarseSimulationModelInputsError::InvalidInitialLongVarianceState { source }
            })?;

        if let Some(tick_size) = self.observed_price_tick_size
            && !is_valid_observed_price_tick_size(tick_size)
        {
            return Err(
                CoarseSimulationModelInputsError::InvalidObservedPriceTickSize { value: tick_size },
            );
        }

        self.timestamp_batching_surface
            .validate()
            .map_err(|source| {
                CoarseSimulationModelInputsError::InvalidTimestampBatchingSurface { source }
            })?;

        if self.exchange_code.trim().is_empty() {
            return Err(CoarseSimulationModelInputsError::InvalidExchangeCode {
                value: self.exchange_code.clone(),
            });
        }

        let calendar_registry = self.load_calendar_registry().map_err(|source| {
            CoarseSimulationModelInputsError::InvalidCalendarRegistry { source }
        })?;

        calendar_registry
            .get_required(&self.exchange_code)
            .map_err(|source| {
                CoarseSimulationModelInputsError::ExchangeCodeMissingFromCalendarRegistry {
                    exchange_code: self.exchange_code.clone(),
                    source,
                }
            })?;

        Ok(())
    }

    pub fn load_calendar_registry(
        &self,
    ) -> Result<ExchangeCalendarRegistry, ExchangeCalendarRegistryLoadError> {
        ExchangeCalendarRegistry::from_artifact(self.calendar_registry_artifact.clone())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationModelInputsError {
    InvalidSufficientStatisticsModelInputs {
        source: SufficientStatisticsModelInputsError,
    },
    InvalidNewsProcessConfiguration {
        source: NewsProcessConfigurationError,
    },
    InvalidReturnClockExposureParameters {
        source: ReturnClockExposureParametersError,
    },
    InvalidLongVarianceParameters {
        source: LongVarianceParametersError,
    },
    InvalidInitialLongVarianceState {
        source: LongVarianceStateError,
    },
    InvalidObservedPriceTickSize {
        value: f64,
    },
    InvalidTimestampBatchingSurface {
        source: TimestampBatchingSurfaceError,
    },
    InvalidCalendarRegistry {
        source: ExchangeCalendarRegistryLoadError,
    },
    InvalidExchangeCode {
        value: String,
    },
    ExchangeCodeMissingFromCalendarRegistry {
        exchange_code: String,
        source: ExchangeCalendarRegistryLookupError,
    },
}

impl Display for CoarseSimulationModelInputsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSufficientStatisticsModelInputs { source } => {
                write!(
                    formatter,
                    "sufficient_statistics_model_inputs are invalid: {source}"
                )
            }
            Self::InvalidNewsProcessConfiguration { source } => {
                write!(formatter, "news_process_configuration is invalid: {source}")
            }
            Self::InvalidReturnClockExposureParameters { source } => {
                write!(
                    formatter,
                    "return_clock_exposure_parameters are invalid: {source}"
                )
            }
            Self::InvalidLongVarianceParameters { source } => {
                write!(formatter, "long_variance_parameters are invalid: {source}")
            }
            Self::InvalidInitialLongVarianceState { source } => {
                write!(
                    formatter,
                    "initial_long_variance_state is invalid: {source}"
                )
            }
            Self::InvalidObservedPriceTickSize { value } => write!(
                formatter,
                "observed_price_tick_size must be finite and strictly positive when provided, found {value}"
            ),
            Self::InvalidTimestampBatchingSurface { source } => {
                write!(formatter, "timestamp_batching_surface is invalid: {source}")
            }
            Self::InvalidCalendarRegistry { source } => {
                write!(formatter, "calendar_registry_artifact is invalid: {source}")
            }
            Self::InvalidExchangeCode { value } => {
                write!(
                    formatter,
                    "exchange_code must be a non-empty string, found `{value}`"
                )
            }
            Self::ExchangeCodeMissingFromCalendarRegistry {
                exchange_code,
                source,
            } => write!(
                formatter,
                "exchange_code `{exchange_code}` must exist in calendar_registry_artifact: {source}"
            ),
        }
    }
}

impl Error for CoarseSimulationModelInputsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSufficientStatisticsModelInputs { source } => Some(source),
            Self::InvalidNewsProcessConfiguration { source } => Some(source),
            Self::InvalidReturnClockExposureParameters { source } => Some(source),
            Self::InvalidLongVarianceParameters { source } => Some(source),
            Self::InvalidInitialLongVarianceState { source } => Some(source),
            Self::InvalidTimestampBatchingSurface { source } => Some(source),
            Self::InvalidCalendarRegistry { source } => Some(source),
            Self::ExchangeCodeMissingFromCalendarRegistry { source, .. } => Some(source),
            Self::InvalidObservedPriceTickSize { .. } | Self::InvalidExchangeCode { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoarseSimulationModelInputsPayload {
    sufficient_statistics_model_inputs: SufficientStatisticsModelInputs,
    news_process_configuration: NewsProcessConfiguration,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
    long_variance_parameters: LongVarianceParameters,
    initial_long_variance_state: LongVarianceState,
    observed_price_tick_size: Option<f64>,
    timestamp_batching_surface: TimestampBatchingSurface,
    calendar_registry_artifact: ExchangeCalendarRegistryArtifact,
    exchange_code: String,
}

impl<'de> Deserialize<'de> for CoarseSimulationModelInputs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = CoarseSimulationModelInputsPayload::deserialize(deserializer)?;

        Self::try_new(Self {
            sufficient_statistics_model_inputs: payload.sufficient_statistics_model_inputs,
            news_process_configuration: payload.news_process_configuration,
            return_clock_exposure_parameters: payload.return_clock_exposure_parameters,
            long_variance_parameters: payload.long_variance_parameters,
            initial_long_variance_state: payload.initial_long_variance_state,
            observed_price_tick_size: payload.observed_price_tick_size,
            timestamp_batching_surface: payload.timestamp_batching_surface,
            calendar_registry_artifact: payload.calendar_registry_artifact,
            exchange_code: payload.exchange_code,
        })
        .map_err(serde::de::Error::custom)
    }
}
