use serde::Deserialize;
use serde::Serialize;
use statrs::function::gamma::ln_gamma;

use janiform_foundations::randomness::SimulationRandomnessStream;
use janiform_foundations::time::NANOS_PER_HOUR_F64;

use super::errors::{
    CoupledGapMarkConstructionError, DoubleExponentialBaseShockSamplingError,
    GammaSeverityMarkSamplingError, NewsArrivalConfigurationError,
    NewsComponentIntensityEvaluationError, NewsComponentMarkScaleConfigurationError,
    NewsDeterministicIntensityEvaluationError, NewsIntensityError, NewsMarkConfigurationError,
    NewsProcessConfigurationError,
};
use super::interface::{
    COUPLED_GAP_MARK_CONSTRUCTION_POLICY, NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY,
    NewsArrivalIntensityEvaluationInput, NewsArrivalIntensityEvaluationOutput,
    NewsArrivalIntensityGenerator, NewsBaseShockMarkSampler, NewsCoupledMarkConstructor,
    NewsMarkInterfaceError, NewsMarkInterfaceStage, NewsSeverityMarkSampler,
};
use super::sampling::{
    sample_exponential_with_rate, sample_gamma_shape_scale, utc_timestamp_from_nanos,
};

/// Known scheduled-news arrival configuration with explicit UTC anchors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownNewsArrivalConfiguration {
    pub scheduled_event_timestamps_utc_nanos: Vec<i64>,
}

impl KnownNewsArrivalConfiguration {
    pub fn validate(&self) -> Result<(), NewsArrivalConfigurationError> {
        let mut previous_timestamp_utc_nanos: Option<i64> = None;
        for &timestamp_utc_nanos in &self.scheduled_event_timestamps_utc_nanos {
            if utc_timestamp_from_nanos(timestamp_utc_nanos).is_none() {
                return Err(
                    NewsArrivalConfigurationError::InvalidKnownScheduledTimestampUtcNanos {
                        value: timestamp_utc_nanos,
                    },
                );
            }

            if let Some(previous_timestamp_utc_nanos_value) = previous_timestamp_utc_nanos
                && timestamp_utc_nanos <= previous_timestamp_utc_nanos_value
            {
                return Err(
                    NewsArrivalConfigurationError::KnownScheduledTimestampsNotStrictlyIncreasing {
                        previous_timestamp_utc_nanos: previous_timestamp_utc_nanos_value,
                        current_timestamp_utc_nanos: timestamp_utc_nanos,
                    },
                );
            }

            previous_timestamp_utc_nanos = Some(timestamp_utc_nanos);
        }

        Ok(())
    }

    fn count_scheduled_arrivals_in_interval(
        &self,
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    ) -> usize {
        self.scheduled_event_timestamps_utc_nanos
            .iter()
            .filter(|&&timestamp_utc_nanos| {
                timestamp_utc_nanos >= interval_start_utc_nanos
                    && timestamp_utc_nanos < interval_end_utc_nanos
            })
            .count()
    }
}

/// Surprise-news deterministic background intensity configuration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurpriseNewsArrivalConfiguration {
    pub baseline_intensity: f64,
}

impl SurpriseNewsArrivalConfiguration {
    pub fn validate(self) -> Result<(), NewsArrivalConfigurationError> {
        if !self.baseline_intensity.is_finite() || self.baseline_intensity < 0.0 {
            return Err(
                NewsArrivalConfigurationError::InvalidSurpriseBaselineIntensity {
                    value: self.baseline_intensity,
                },
            );
        }

        Ok(())
    }
}

/// Componentized known and surprise arrival configuration for the news process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewsArrivalConfiguration {
    pub known: KnownNewsArrivalConfiguration,
    pub surprise: SurpriseNewsArrivalConfiguration,
}

impl NewsArrivalConfiguration {
    pub fn validate(&self) -> Result<(), NewsArrivalConfigurationError> {
        self.known.validate()?;
        self.surprise.validate()?;
        Ok(())
    }

    pub fn evaluate_with_deterministic_time_multiplier(
        &self,
        intensity_input: NewsArrivalIntensityEvaluationInput,
    ) -> Result<NewsArrivalIntensityEvaluationOutput, NewsComponentIntensityEvaluationError> {
        self.validate().map_err(|source| {
            NewsComponentIntensityEvaluationError::InvalidConfiguration { source }
        })?;

        if intensity_input.interval_end_utc_nanos <= intensity_input.interval_start_utc_nanos {
            return Err(
                NewsComponentIntensityEvaluationError::NonIncreasingInterval {
                    interval_start_utc_nanos: intensity_input.interval_start_utc_nanos,
                    interval_end_utc_nanos: intensity_input.interval_end_utc_nanos,
                },
            );
        }
        if utc_timestamp_from_nanos(intensity_input.interval_start_utc_nanos).is_none() {
            return Err(
                NewsComponentIntensityEvaluationError::InvalidIntervalStartTimestampUtcNanos {
                    value: intensity_input.interval_start_utc_nanos,
                },
            );
        }
        if utc_timestamp_from_nanos(intensity_input.interval_end_utc_nanos).is_none() {
            return Err(
                NewsComponentIntensityEvaluationError::InvalidIntervalEndTimestampUtcNanos {
                    value: intensity_input.interval_end_utc_nanos,
                },
            );
        }
        if !intensity_input
            .surprise_deterministic_time_multiplier
            .is_finite()
            || intensity_input.surprise_deterministic_time_multiplier < 0.0
        {
            return Err(
                NewsComponentIntensityEvaluationError::InvalidSurpriseDeterministicTimeMultiplier {
                    value: intensity_input.surprise_deterministic_time_multiplier,
                },
            );
        }

        let known_scheduled_arrival_count_usize = self.known.count_scheduled_arrivals_in_interval(
            intensity_input.interval_start_utc_nanos,
            intensity_input.interval_end_utc_nanos,
        );
        let known_scheduled_arrival_count = u64::try_from(known_scheduled_arrival_count_usize)
            .map_err(|_| {
                NewsComponentIntensityEvaluationError::KnownScheduledArrivalCountOverflow {
                    value: known_scheduled_arrival_count_usize,
                }
            })?;
        let interval_duration_in_hours = (intensity_input.interval_end_utc_nanos
            - intensity_input.interval_start_utc_nanos)
            as f64
            / NANOS_PER_HOUR_F64;
        let known_component_intensity =
            known_scheduled_arrival_count as f64 / interval_duration_in_hours;
        if !known_component_intensity.is_finite() || known_component_intensity < 0.0 {
            return Err(
                NewsComponentIntensityEvaluationError::InvalidKnownComponentIntensity {
                    value: known_component_intensity,
                },
            );
        }

        let surprise_component_intensity = self.surprise.baseline_intensity
            * intensity_input.surprise_deterministic_time_multiplier;
        if !surprise_component_intensity.is_finite() || surprise_component_intensity < 0.0 {
            return Err(
                NewsComponentIntensityEvaluationError::InvalidSurpriseComponentIntensity {
                    value: surprise_component_intensity,
                },
            );
        }

        let total_intensity = known_component_intensity + surprise_component_intensity;
        if !total_intensity.is_finite() || total_intensity < 0.0 {
            return Err(
                NewsComponentIntensityEvaluationError::InvalidTotalIntensity {
                    value: total_intensity,
                },
            );
        }

        Ok(NewsArrivalIntensityEvaluationOutput {
            known_scheduled_arrival_count,
            known_component_intensity,
            surprise_component_intensity,
            total_intensity,
            intensity_policy: NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY,
        })
    }
}

impl NewsArrivalIntensityGenerator for NewsArrivalConfiguration {
    fn generate_arrival_intensity(
        &self,
        intensity_input: NewsArrivalIntensityEvaluationInput,
    ) -> Result<NewsArrivalIntensityEvaluationOutput, NewsMarkInterfaceError> {
        self.evaluate_with_deterministic_time_multiplier(intensity_input)
            .map_err(|source| {
                NewsMarkInterfaceError::component_rejected(
                    NewsMarkInterfaceStage::ArrivalIntensityGeneration,
                    source.to_string(),
                )
            })
    }
}

/// Shared mark-law parameters consumed across known and surprise components.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedNewsMarkLawConfiguration {
    pub gamma_severity_marks: GammaSeverityMarkConfiguration,
    pub double_exponential_base_shock: DoubleExponentialBaseShockConfiguration,
    pub coupled_gap_marks: CoupledGapMarkConfiguration,
}

impl SharedNewsMarkLawConfiguration {
    pub fn validate(self) -> Result<(), NewsProcessConfigurationError> {
        self.gamma_severity_marks.validate().map_err(|source| {
            NewsProcessConfigurationError::InvalidGammaSeverityMarkConfiguration { source }
        })?;

        self.double_exponential_base_shock
            .validate()
            .map_err(
                |source| NewsProcessConfigurationError::InvalidBaseShockConfiguration { source },
            )?;

        self.coupled_gap_marks.validate().map_err(|source| {
            NewsProcessConfigurationError::InvalidCoupledGapMarkConfiguration { source }
        })?;

        Ok(())
    }
}

/// Component-level mark scaling applied to shared mark-law draws.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewsComponentMarkScaleConfiguration {
    pub activity_coupling_scale: f64,
    pub return_jump_scale: f64,
}

impl NewsComponentMarkScaleConfiguration {
    pub fn validate(self) -> Result<(), NewsComponentMarkScaleConfigurationError> {
        if !self.activity_coupling_scale.is_finite() || self.activity_coupling_scale < 0.0 {
            return Err(
                NewsComponentMarkScaleConfigurationError::InvalidActivityCouplingScale {
                    value: self.activity_coupling_scale,
                },
            );
        }

        if !self.return_jump_scale.is_finite() || self.return_jump_scale < 0.0 {
            return Err(
                NewsComponentMarkScaleConfigurationError::InvalidReturnJumpScale {
                    value: self.return_jump_scale,
                },
            );
        }

        Ok(())
    }
}

/// Legacy deterministic intensity policy kept for hourly factor materialization contracts.
///
/// `NewsProcessConfiguration` now uses `NewsArrivalConfiguration` for known and surprise
/// components directly.
///
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

    pub fn try_new(shape_k: f64, scale_theta: f64) -> Result<Self, NewsMarkConfigurationError> {
        let configuration = Self::new(shape_k, scale_theta);
        configuration.validate()?;
        Ok(configuration)
    }

    pub fn validate(self) -> Result<(), NewsMarkConfigurationError> {
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

    pub fn expected_severity_mark(self) -> Result<f64, NewsMarkConfigurationError> {
        self.validate()?;
        Ok(self.shape_k * self.scale_theta)
    }

    pub fn expected_square_root_severity_mark(self) -> Result<f64, NewsMarkConfigurationError> {
        self.validate()?;

        let log_expected_square_root =
            0.5 * self.scale_theta.ln() + ln_gamma(self.shape_k + 0.5) - ln_gamma(self.shape_k);

        let expected_square_root = log_expected_square_root.exp();
        if !expected_square_root.is_finite() || expected_square_root <= 0.0 {
            return Err(NewsMarkConfigurationError::InvalidGammaScaleTheta {
                value: self.scale_theta,
            });
        }

        Ok(expected_square_root)
    }

    /// Sample one severity mark `U ~ Gamma(k_U, theta_U)` using shape-scale parameters.
    ///
    /// This API is intentionally independent from news-arrival generation so downstream
    /// consumers can reuse one sampled mark across Hawkes loading and coupled gap construction.
    pub fn sample_severity_mark<R>(
        self,
        randomness_stream: &mut R,
    ) -> Result<f64, GammaSeverityMarkSamplingError>
    where
        R: SimulationRandomnessStream + ?Sized,
    {
        self.validate()
            .map_err(|source| GammaSeverityMarkSamplingError::InvalidConfiguration { source })?;

        let sampled_mark =
            sample_gamma_shape_scale(self.shape_k, self.scale_theta, randomness_stream);
        if !sampled_mark.is_finite() || sampled_mark < 0.0 {
            return Err(GammaSeverityMarkSamplingError::InvalidSampledMark {
                value: sampled_mark,
            });
        }

        Ok(sampled_mark)
    }
}

impl NewsSeverityMarkSampler for GammaSeverityMarkConfiguration {
    fn sample_shared_severity_mark(
        &self,
        randomness_stream: &mut dyn SimulationRandomnessStream,
    ) -> Result<f64, NewsMarkInterfaceError> {
        self.sample_severity_mark(randomness_stream)
            .map_err(|source| {
                NewsMarkInterfaceError::component_rejected(
                    NewsMarkInterfaceStage::SeverityMarkSampling,
                    source.to_string(),
                )
            })
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

    pub fn try_new(
        probability_positive: f64,
        eta_plus: f64,
        eta_minus: f64,
    ) -> Result<Self, NewsMarkConfigurationError> {
        let configuration = Self::new(probability_positive, eta_plus, eta_minus);
        configuration.validate()?;
        Ok(configuration)
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

    pub fn expected_base_shock_mark(self) -> Result<f64, NewsMarkConfigurationError> {
        self.validate()?;

        Ok(self.probability_positive / self.eta_plus
            - (1.0 - self.probability_positive) / self.eta_minus)
    }

    /// Sample one Kou-style base shock mark `epsilon`.
    ///
    /// Policy for v0.1:
    /// - The mixture probability `p` is constant and equals `probability_positive`
    /// - Positive shocks follow `+Exp(eta_plus)`
    /// - Negative shocks follow `-Exp(eta_minus)`
    ///
    /// This sampler remains independent from severity-mark generation so callers can
    /// reuse `epsilon` in closed-market gap mark construction `Z = sigma_g * sqrt(U) * epsilon`.
    pub fn sample_base_shock_mark<R>(
        self,
        randomness_stream: &mut R,
    ) -> Result<f64, DoubleExponentialBaseShockSamplingError>
    where
        R: SimulationRandomnessStream + ?Sized,
    {
        self.validate().map_err(|source| {
            DoubleExponentialBaseShockSamplingError::InvalidConfiguration { source }
        })?;

        let acceptance_draw = randomness_stream
            .next_unit_interval_f64()
            .clamp(0.0, 1.0 - f64::EPSILON);

        let sampled_mark = if acceptance_draw < self.probability_positive {
            sample_exponential_with_rate(self.eta_plus, randomness_stream)
        } else {
            -sample_exponential_with_rate(self.eta_minus, randomness_stream)
        };

        if !sampled_mark.is_finite() {
            return Err(
                DoubleExponentialBaseShockSamplingError::InvalidSampledMark {
                    value: sampled_mark,
                },
            );
        }

        Ok(sampled_mark)
    }
}

impl NewsBaseShockMarkSampler for DoubleExponentialBaseShockConfiguration {
    fn sample_shared_base_shock_mark(
        &self,
        randomness_stream: &mut dyn SimulationRandomnessStream,
    ) -> Result<f64, NewsMarkInterfaceError> {
        self.sample_base_shock_mark(randomness_stream)
            .map_err(|source| {
                NewsMarkInterfaceError::component_rejected(
                    NewsMarkInterfaceStage::BaseShockMarkSampling,
                    source.to_string(),
                )
            })
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

    pub fn expected_gap_jump_mark(
        self,
        expected_square_root_severity_mark: f64,
        expected_base_shock_mark: f64,
    ) -> Result<f64, CoupledGapMarkConstructionError> {
        self.validate()
            .map_err(|source| CoupledGapMarkConstructionError::InvalidConfiguration { source })?;

        if !expected_square_root_severity_mark.is_finite()
            || expected_square_root_severity_mark < 0.0
        {
            return Err(CoupledGapMarkConstructionError::InvalidSharedSeverityMark {
                value: expected_square_root_severity_mark,
            });
        }

        if !expected_base_shock_mark.is_finite() {
            return Err(
                CoupledGapMarkConstructionError::InvalidSharedBaseShockMark {
                    value: expected_base_shock_mark,
                },
            );
        }

        let expected_gap_jump_mark =
            self.sigma_g * expected_square_root_severity_mark * expected_base_shock_mark;
        if !expected_gap_jump_mark.is_finite() {
            return Err(CoupledGapMarkConstructionError::InvalidConstructedGapMark {
                value: expected_gap_jump_mark,
            });
        }

        Ok(expected_gap_jump_mark)
    }

    /// Construct one coupled gap jump mark from shared severity and base-shock marks.
    ///
    /// Formula:
    /// `Z = sigma_g * sqrt(U) * epsilon`
    ///
    /// The output keeps all construction inputs so diagnostics and deterministic replay can
    /// recover the exact mark-construction payload without re-sampling `U` or `epsilon`.
    pub fn construct_gap_jump_mark(
        self,
        shared_severity_mark: f64,
        shared_base_shock_mark: f64,
    ) -> Result<CoupledGapMarkConstruction, CoupledGapMarkConstructionError> {
        self.validate()
            .map_err(|source| CoupledGapMarkConstructionError::InvalidConfiguration { source })?;

        if !shared_severity_mark.is_finite() || shared_severity_mark < 0.0 {
            return Err(CoupledGapMarkConstructionError::InvalidSharedSeverityMark {
                value: shared_severity_mark,
            });
        }

        if !shared_base_shock_mark.is_finite() {
            return Err(
                CoupledGapMarkConstructionError::InvalidSharedBaseShockMark {
                    value: shared_base_shock_mark,
                },
            );
        }

        let gap_jump_mark = self.sigma_g * shared_severity_mark.sqrt() * shared_base_shock_mark;
        if !gap_jump_mark.is_finite() {
            return Err(CoupledGapMarkConstructionError::InvalidConstructedGapMark {
                value: gap_jump_mark,
            });
        }

        Ok(CoupledGapMarkConstruction {
            shared_severity_mark,
            shared_base_shock_mark,
            sigma_g: self.sigma_g,
            gap_jump_mark,
            construction_policy: COUPLED_GAP_MARK_CONSTRUCTION_POLICY,
        })
    }
}

impl NewsCoupledMarkConstructor for CoupledGapMarkConfiguration {
    fn construct_coupled_mark(
        &self,
        shared_severity_mark: f64,
        shared_base_shock_mark: f64,
    ) -> Result<CoupledGapMarkConstruction, NewsMarkInterfaceError> {
        self.construct_gap_jump_mark(shared_severity_mark, shared_base_shock_mark)
            .map_err(|source| {
                NewsMarkInterfaceError::component_rejected(
                    NewsMarkInterfaceStage::CoupledMarkConstruction,
                    source.to_string(),
                )
            })
    }
}

/// Deterministic output for coupled gap-mark construction from shared `U` and `epsilon`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CoupledGapMarkConstruction {
    pub shared_severity_mark: f64,
    pub shared_base_shock_mark: f64,
    pub sigma_g: f64,
    pub gap_jump_mark: f64,
    pub construction_policy: &'static str,
}
