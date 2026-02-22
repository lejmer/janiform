//! Shared exogenous news process configuration and deterministic intensity model.
//!
//! This module defines one typed configuration payload that is reusable by
//! simulation, pricing, and calibration artifact persistence.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

pub const NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION: &str = "news.process.configuration.v1";
pub const NEWS_INTENSITY_BASELINE_POLICY: &str =
    "closed_window_share_times_baseline_intensity_times_deterministic_multiplier.v1";
pub const NEWS_INTENSITY_DETERMINISTIC_INPUT_POLICY: &str =
    "interval_indexed_non_negative_multiplier_with_default_one.v1";

/// Deterministic intensity policy configuration for `nu_news(t)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NewsIntensityConfiguration {
    pub closed_window_baseline_intensity: f64,
}

impl NewsIntensityConfiguration {
    pub const fn new(closed_window_baseline_intensity: f64) -> Self {
        Self {
            closed_window_baseline_intensity,
        }
    }

    pub fn try_new(closed_window_baseline_intensity: f64) -> Result<Self, NewsIntensityError> {
        let configuration = Self::new(closed_window_baseline_intensity);
        configuration.validate()?;
        Ok(configuration)
    }

    pub fn validate(self) -> Result<(), NewsIntensityError> {
        if !self.closed_window_baseline_intensity.is_finite()
            || self.closed_window_baseline_intensity < 0.0
        {
            return Err(NewsIntensityError::InvalidClosedWindowBaselineIntensity {
                value: self.closed_window_baseline_intensity,
            });
        }

        Ok(())
    }

    /// Evaluate deterministic interval intensity from a closed-window share and multiplier.
    ///
    /// Formula:
    /// `nu_news = closed_window_share * closed_window_baseline_intensity * deterministic_time_multiplier`.
    pub fn evaluate_with_deterministic_time_multiplier(
        self,
        closed_window_share: f64,
        deterministic_time_multiplier: f64,
    ) -> Result<f64, NewsDeterministicIntensityEvaluationError> {
        if !closed_window_share.is_finite() || !(0.0..=1.0).contains(&closed_window_share) {
            return Err(
                NewsDeterministicIntensityEvaluationError::InvalidClosedWindowShare {
                    value: closed_window_share,
                },
            );
        }
        if !deterministic_time_multiplier.is_finite() || deterministic_time_multiplier < 0.0 {
            return Err(
                NewsDeterministicIntensityEvaluationError::InvalidDeterministicTimeMultiplier {
                    value: deterministic_time_multiplier,
                },
            );
        }

        let interval_intensity = closed_window_share
            * self.closed_window_baseline_intensity
            * deterministic_time_multiplier;
        if !interval_intensity.is_finite() || interval_intensity < 0.0 {
            return Err(
                NewsDeterministicIntensityEvaluationError::InvalidEvaluatedIntensity {
                    value: interval_intensity,
                },
            );
        }

        Ok(interval_intensity)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GammaSeverityMarkConfiguration {
    pub shape_k: f64,
    pub scale_theta: f64,
}

impl GammaSeverityMarkConfiguration {
    pub const fn new(shape_k: f64, scale_theta: f64) -> Self {
        Self {
            shape_k,
            scale_theta,
        }
    }

    fn validate(self) -> Result<(), NewsMarkConfigurationError> {
        if !self.shape_k.is_finite() || self.shape_k <= 0.0 {
            return Err(NewsMarkConfigurationError::InvalidGammaShapeK {
                value: self.shape_k,
            });
        }
        if !self.scale_theta.is_finite() || self.scale_theta <= 0.0 {
            return Err(NewsMarkConfigurationError::InvalidGammaScaleTheta {
                value: self.scale_theta,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoubleExponentialBaseShockConfiguration {
    pub probability_positive: f64,
    pub eta_plus: f64,
    pub eta_minus: f64,
}

impl DoubleExponentialBaseShockConfiguration {
    pub const fn new(probability_positive: f64, eta_plus: f64, eta_minus: f64) -> Self {
        Self {
            probability_positive,
            eta_plus,
            eta_minus,
        }
    }

    fn validate(self) -> Result<(), NewsMarkConfigurationError> {
        if !self.probability_positive.is_finite()
            || self.probability_positive <= 0.0
            || self.probability_positive >= 1.0
        {
            return Err(NewsMarkConfigurationError::InvalidPositiveJumpProbability {
                value: self.probability_positive,
            });
        }
        if !self.eta_plus.is_finite() || self.eta_plus <= 0.0 {
            return Err(NewsMarkConfigurationError::InvalidEtaPlus {
                value: self.eta_plus,
            });
        }
        if !self.eta_minus.is_finite() || self.eta_minus <= 0.0 {
            return Err(NewsMarkConfigurationError::InvalidEtaMinus {
                value: self.eta_minus,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoupledGapMarkConfiguration {
    pub sigma_g: f64,
}

impl CoupledGapMarkConfiguration {
    pub const fn new(sigma_g: f64) -> Self {
        Self { sigma_g }
    }

    fn validate(self) -> Result<(), NewsMarkConfigurationError> {
        if !self.sigma_g.is_finite() || self.sigma_g < 0.0 {
            return Err(NewsMarkConfigurationError::InvalidSigmaG {
                value: self.sigma_g,
            });
        }

        Ok(())
    }
}

/// Canonical shared news process configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewsProcessConfiguration {
    pub schema_version: &'static str,
    pub intensity: NewsIntensityConfiguration,
    pub gamma_severity_marks: GammaSeverityMarkConfiguration,
    pub double_exponential_base_shock: DoubleExponentialBaseShockConfiguration,
    pub coupled_gap_marks: CoupledGapMarkConfiguration,
}

impl NewsProcessConfiguration {
    pub fn try_new(
        intensity: NewsIntensityConfiguration,
        gamma_severity_marks: GammaSeverityMarkConfiguration,
        double_exponential_base_shock: DoubleExponentialBaseShockConfiguration,
        coupled_gap_marks: CoupledGapMarkConfiguration,
    ) -> Result<Self, NewsProcessConfigurationError> {
        Self::try_new_with_schema_version(
            NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
            intensity,
            gamma_severity_marks,
            double_exponential_base_shock,
            coupled_gap_marks,
        )
    }

    fn try_new_with_schema_version(
        schema_version: &str,
        intensity: NewsIntensityConfiguration,
        gamma_severity_marks: GammaSeverityMarkConfiguration,
        double_exponential_base_shock: DoubleExponentialBaseShockConfiguration,
        coupled_gap_marks: CoupledGapMarkConfiguration,
    ) -> Result<Self, NewsProcessConfigurationError> {
        if schema_version != NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION {
            return Err(NewsProcessConfigurationError::UnsupportedSchemaVersion {
                value: schema_version.to_string(),
            });
        }

        intensity.validate().map_err(|source| {
            NewsProcessConfigurationError::InvalidIntensityConfiguration { source }
        })?;
        gamma_severity_marks.validate().map_err(|source| {
            NewsProcessConfigurationError::InvalidGammaSeverityMarkConfiguration { source }
        })?;
        double_exponential_base_shock.validate().map_err(|source| {
            NewsProcessConfigurationError::InvalidBaseShockConfiguration { source }
        })?;
        coupled_gap_marks.validate().map_err(|source| {
            NewsProcessConfigurationError::InvalidCoupledGapMarkConfiguration { source }
        })?;

        Ok(Self {
            schema_version: NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
            intensity,
            gamma_severity_marks,
            double_exponential_base_shock,
            coupled_gap_marks,
        })
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
    intensity: NewsIntensityConfiguration,
    gamma_severity_marks: GammaSeverityMarkConfiguration,
    double_exponential_base_shock: DoubleExponentialBaseShockConfiguration,
    coupled_gap_marks: CoupledGapMarkConfiguration,
}

impl<'de> Deserialize<'de> for NewsProcessConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = NewsProcessConfigurationPayload::deserialize(deserializer)?;
        Self::try_new_with_schema_version(
            &payload.schema_version,
            payload.intensity,
            payload.gamma_severity_marks,
            payload.double_exponential_base_shock,
            payload.coupled_gap_marks,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsIntensityError {
    InvalidClosedWindowBaselineIntensity { value: f64 },
}

impl Display for NewsIntensityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidClosedWindowBaselineIntensity { value } => write!(
                formatter,
                "closed_window_baseline_intensity must be a finite non-negative value, found {value}"
            ),
        }
    }
}

impl Error for NewsIntensityError {}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsMarkConfigurationError {
    InvalidGammaShapeK { value: f64 },
    InvalidGammaScaleTheta { value: f64 },
    InvalidPositiveJumpProbability { value: f64 },
    InvalidEtaPlus { value: f64 },
    InvalidEtaMinus { value: f64 },
    InvalidSigmaG { value: f64 },
}

impl Display for NewsMarkConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidGammaShapeK { value } => {
                write!(
                    formatter,
                    "gamma severity shape_k must be finite and strictly positive, found {value}"
                )
            }
            Self::InvalidGammaScaleTheta { value } => write!(
                formatter,
                "gamma severity scale_theta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidPositiveJumpProbability { value } => write!(
                formatter,
                "double_exponential_base_shock.probability_positive must be finite and strictly between 0 and 1, found {value}"
            ),
            Self::InvalidEtaPlus { value } => write!(
                formatter,
                "double_exponential_base_shock.eta_plus must be finite and strictly positive, found {value}"
            ),
            Self::InvalidEtaMinus { value } => write!(
                formatter,
                "double_exponential_base_shock.eta_minus must be finite and strictly positive, found {value}"
            ),
            Self::InvalidSigmaG { value } => write!(
                formatter,
                "coupled_gap_marks.sigma_g must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for NewsMarkConfigurationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsProcessConfigurationError {
    UnsupportedSchemaVersion { value: String },
    InvalidIntensityConfiguration { source: NewsIntensityError },
    InvalidGammaSeverityMarkConfiguration { source: NewsMarkConfigurationError },
    InvalidBaseShockConfiguration { source: NewsMarkConfigurationError },
    InvalidCoupledGapMarkConfiguration { source: NewsMarkConfigurationError },
}

impl Display for NewsProcessConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { value } => write!(
                formatter,
                "unsupported news process configuration schema version `{value}`, expected `{NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION}`"
            ),
            Self::InvalidIntensityConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidGammaSeverityMarkConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidBaseShockConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidCoupledGapMarkConfiguration { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for NewsProcessConfigurationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidIntensityConfiguration { source } => Some(source),
            Self::InvalidGammaSeverityMarkConfiguration { source } => Some(source),
            Self::InvalidBaseShockConfiguration { source } => Some(source),
            Self::InvalidCoupledGapMarkConfiguration { source } => Some(source),
            Self::UnsupportedSchemaVersion { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsDeterministicIntensityEvaluationError {
    InvalidClosedWindowShare { value: f64 },
    InvalidDeterministicTimeMultiplier { value: f64 },
    InvalidEvaluatedIntensity { value: f64 },
}

impl Display for NewsDeterministicIntensityEvaluationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidClosedWindowShare { value } => write!(
                formatter,
                "closed_window_share must be finite and in [0, 1], found {value}"
            ),
            Self::InvalidDeterministicTimeMultiplier { value } => write!(
                formatter,
                "deterministic_time_multiplier must be finite and non-negative, found {value}"
            ),
            Self::InvalidEvaluatedIntensity { value } => write!(
                formatter,
                "evaluated deterministic news intensity must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for NewsDeterministicIntensityEvaluationError {}

#[cfg(test)]
mod tests {
    use super::{
        CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
        GammaSeverityMarkConfiguration, NEWS_INTENSITY_BASELINE_POLICY,
        NEWS_INTENSITY_DETERMINISTIC_INPUT_POLICY, NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
        NewsIntensityConfiguration, NewsProcessConfiguration,
    };
    use serde_json::json;

    #[test]
    fn news_process_configuration_round_trip_preserves_payload() {
        let configuration = NewsProcessConfiguration::try_new(
            NewsIntensityConfiguration::try_new(0.75)
                .expect("valid intensity payload should construct"),
            GammaSeverityMarkConfiguration::new(1.8, 0.9),
            DoubleExponentialBaseShockConfiguration::new(0.35, 2.1, 3.0),
            CoupledGapMarkConfiguration::new(0.6),
        )
        .expect("valid process configuration should construct");

        let serialized =
            serde_json::to_string(&configuration).expect("configuration payload should serialize");
        let parsed: NewsProcessConfiguration =
            serde_json::from_str(&serialized).expect("serialized payload should deserialize");

        assert_eq!(parsed, configuration);
        assert_eq!(
            parsed.schema_version,
            NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION
        );
    }

    #[test]
    fn news_process_configuration_deserialization_reports_missing_required_field() {
        let payload = json!({
            "schema_version": NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
            "gamma_severity_marks": {
                "shape_k": 1.8,
                "scale_theta": 0.9
            },
            "double_exponential_base_shock": {
                "probability_positive": 0.35,
                "eta_plus": 2.1,
                "eta_minus": 3.0
            },
            "coupled_gap_marks": {
                "sigma_g": 0.6
            }
        });

        let parse_result = serde_json::from_value::<NewsProcessConfiguration>(payload);
        let error_text = parse_result
            .expect_err("missing intensity field must fail")
            .to_string();

        assert!(
            error_text.contains("missing field `intensity`"),
            "unexpected error text: {error_text}"
        );
    }

    #[test]
    fn news_process_configuration_deserialization_reports_invalid_field_value() {
        let payload = json!({
            "schema_version": NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
            "intensity": {
                "closed_window_baseline_intensity": -0.1
            },
            "gamma_severity_marks": {
                "shape_k": 1.8,
                "scale_theta": 0.9
            },
            "double_exponential_base_shock": {
                "probability_positive": 0.35,
                "eta_plus": 2.1,
                "eta_minus": 3.0
            },
            "coupled_gap_marks": {
                "sigma_g": 0.6
            }
        });

        let parse_result = serde_json::from_value::<NewsProcessConfiguration>(payload);
        let error_text = parse_result
            .expect_err("invalid baseline intensity must fail")
            .to_string();

        assert!(
            error_text
                .contains("closed_window_baseline_intensity must be a finite non-negative value"),
            "unexpected error text: {error_text}"
        );
    }

    #[test]
    fn deterministic_intensity_model_matches_closed_window_policy_equation() {
        let intensity_configuration = NewsIntensityConfiguration::try_new(0.8)
            .expect("valid baseline intensity should construct");

        let interval_intensity = intensity_configuration
            .evaluate_with_deterministic_time_multiplier(0.75, 1.5)
            .expect("valid deterministic intensity inputs should evaluate");

        assert!((interval_intensity - 0.9).abs() <= 1e-12);
        assert_eq!(
            NEWS_INTENSITY_BASELINE_POLICY,
            "closed_window_share_times_baseline_intensity_times_deterministic_multiplier.v1"
        );
        assert_eq!(
            NEWS_INTENSITY_DETERMINISTIC_INPUT_POLICY,
            "interval_indexed_non_negative_multiplier_with_default_one.v1"
        );
    }

    #[test]
    fn deterministic_intensity_model_rejects_out_of_domain_inputs() {
        let intensity_configuration = NewsIntensityConfiguration::new(0.8);

        let closed_share_error = intensity_configuration
            .evaluate_with_deterministic_time_multiplier(1.1, 1.0)
            .expect_err("closed_window_share above 1 must fail");
        assert_eq!(
            closed_share_error.to_string(),
            "closed_window_share must be finite and in [0, 1], found 1.1"
        );

        let multiplier_error = intensity_configuration
            .evaluate_with_deterministic_time_multiplier(0.5, -0.2)
            .expect_err("negative deterministic_time_multiplier must fail");
        assert_eq!(
            multiplier_error.to_string(),
            "deterministic_time_multiplier must be finite and non-negative, found -0.2"
        );
    }
}
