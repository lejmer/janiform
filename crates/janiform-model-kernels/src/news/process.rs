use serde::Deserialize;
use serde::Serialize;

use super::configuration::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, NewsArrivalConfiguration, NewsComponentMarkScaleConfiguration,
    NewsIntensityConfiguration, SharedNewsMarkLawConfiguration,
};
use super::errors::NewsProcessConfigurationError;
use super::interface::{ComposableNewsMarkPipeline, NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION};

/// Canonical shared news process configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewsProcessConfiguration {
    pub schema_version: &'static str,
    pub arrivals: NewsArrivalConfiguration,
    pub shared_mark_law: SharedNewsMarkLawConfiguration,
    pub known_component_mark_scales: NewsComponentMarkScaleConfiguration,
    pub surprise_component_mark_scales: NewsComponentMarkScaleConfiguration,
}

pub type DefaultNewsMarkPipeline = ComposableNewsMarkPipeline<
    NewsArrivalConfiguration,
    GammaSeverityMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
    CoupledGapMarkConfiguration,
>;

impl NewsProcessConfiguration {
    pub fn try_new(
        arrivals: NewsArrivalConfiguration,
        shared_mark_law: SharedNewsMarkLawConfiguration,
        known_component_mark_scales: NewsComponentMarkScaleConfiguration,
        surprise_component_mark_scales: NewsComponentMarkScaleConfiguration,
    ) -> Result<Self, NewsProcessConfigurationError> {
        Self::try_new_with_schema_version(
            NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
            arrivals,
            shared_mark_law,
            known_component_mark_scales,
            surprise_component_mark_scales,
        )
    }

    fn try_new_with_schema_version(
        schema_version: &str,
        arrivals: NewsArrivalConfiguration,
        shared_mark_law: SharedNewsMarkLawConfiguration,
        known_component_mark_scales: NewsComponentMarkScaleConfiguration,
        surprise_component_mark_scales: NewsComponentMarkScaleConfiguration,
    ) -> Result<Self, NewsProcessConfigurationError> {
        if schema_version != NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION {
            return Err(NewsProcessConfigurationError::UnsupportedSchemaVersion {
                value: schema_version.to_string(),
            });
        }

        arrivals.validate().map_err(|source| {
            NewsProcessConfigurationError::InvalidArrivalConfiguration { source }
        })?;
        shared_mark_law.validate()?;
        known_component_mark_scales.validate().map_err(|source| {
            NewsProcessConfigurationError::InvalidKnownComponentMarkScales { source }
        })?;
        surprise_component_mark_scales
            .validate()
            .map_err(|source| {
                NewsProcessConfigurationError::InvalidSurpriseComponentMarkScales { source }
            })?;

        Ok(Self {
            schema_version: NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
            arrivals,
            shared_mark_law,
            known_component_mark_scales,
            surprise_component_mark_scales,
        })
    }

    pub fn validate(&self) -> Result<(), NewsProcessConfigurationError> {
        Self::try_new_with_schema_version(
            self.schema_version,
            self.arrivals.clone(),
            self.shared_mark_law,
            self.known_component_mark_scales,
            self.surprise_component_mark_scales,
        )
        .map(|_| ())
    }

    /// Build the default composable interface wiring.
    ///
    /// Component mapping:
    /// - arrival intensity: known-schedule and surprise-background component model
    /// - severity sampler: Gamma(shape, scale)
    /// - base-shock sampler: Kou double-exponential
    /// - coupled constructor: `sigma_g * sqrt(U) * epsilon`
    pub fn default_mark_pipeline(&self) -> DefaultNewsMarkPipeline {
        DefaultNewsMarkPipeline::new(
            self.arrivals.clone(),
            self.shared_mark_law.gamma_severity_marks,
            self.shared_mark_law.double_exponential_base_shock,
            self.shared_mark_law.coupled_gap_marks,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewsIntensityConfigurationPayload {
    closed_window_baseline_intensity: f64,
}

impl<'de> Deserialize<'de> for NewsIntensityConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = NewsIntensityConfigurationPayload::deserialize(deserializer)?;
        Self::try_new(payload.closed_window_baseline_intensity).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewsProcessConfigurationPayload {
    schema_version: String,
    arrivals: NewsArrivalConfiguration,
    shared_mark_law: SharedNewsMarkLawConfiguration,
    known_component_mark_scales: NewsComponentMarkScaleConfiguration,
    surprise_component_mark_scales: NewsComponentMarkScaleConfiguration,
}

impl<'de> Deserialize<'de> for NewsProcessConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = NewsProcessConfigurationPayload::deserialize(deserializer)?;
        Self::try_new_with_schema_version(
            &payload.schema_version,
            payload.arrivals,
            payload.shared_mark_law,
            payload.known_component_mark_scales,
            payload.surprise_component_mark_scales,
        )
        .map_err(serde::de::Error::custom)
    }
}
