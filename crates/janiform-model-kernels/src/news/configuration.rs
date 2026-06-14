use serde::Deserialize;
use serde::Serialize;
use statrs::function::gamma::ln_gamma;

use janiform_foundations::randomness::{SimulationRandomnessStream, sample_standard_normal_draw};
use janiform_foundations::time::{NANOS_PER_HOUR_F64, utc_datetime_from_nanos};

use super::errors::{
    GammaNormalNewsMarkConstructionError, GammaSeverityMarkSamplingError,
    NewsActivityLoadingRateError, NewsArrivalConfigurationError,
    NewsComponentIntensityEvaluationError, NewsComponentMarkScaleConfigurationError,
    NewsMarkConfigurationError, NewsProcessConfigurationError,
};
use super::interface::{
    GAMMA_NORMAL_NEWS_MARK_CONSTRUCTION_POLICY, NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY,
    NewsArrivalIntensityEvaluationInput, NewsArrivalIntensityEvaluationOutput,
    NewsArrivalIntensityGenerator, NewsMarkInterfaceError, NewsMarkInterfaceStage,
    NewsReturnShockMarkSampler, NewsSeverityMarkSampler,
};
use super::sampling::sample_gamma_shape_scale;

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
            if utc_datetime_from_nanos(timestamp_utc_nanos).is_none() {
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
    pub open_baseline_intensity: f64,
    pub closed_baseline_intensity: f64,
}

impl SurpriseNewsArrivalConfiguration {
    pub fn validate(self) -> Result<(), NewsArrivalConfigurationError> {
        if !self.open_baseline_intensity.is_finite() || self.open_baseline_intensity < 0.0 {
            return Err(
                NewsArrivalConfigurationError::InvalidSurpriseOpenBaselineIntensity {
                    value: self.open_baseline_intensity,
                },
            );
        }

        if !self.closed_baseline_intensity.is_finite() || self.closed_baseline_intensity < 0.0 {
            return Err(
                NewsArrivalConfigurationError::InvalidSurpriseClosedBaselineIntensity {
                    value: self.closed_baseline_intensity,
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

        if utc_datetime_from_nanos(intensity_input.interval_start_utc_nanos).is_none() {
            return Err(
                NewsComponentIntensityEvaluationError::InvalidIntervalStartTimestampUtcNanos {
                    value: intensity_input.interval_start_utc_nanos,
                },
            );
        }

        if utc_datetime_from_nanos(intensity_input.interval_end_utc_nanos).is_none() {
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

        if !intensity_input.surprise_open_window_share.is_finite()
            || !(0.0..=1.0).contains(&intensity_input.surprise_open_window_share)
        {
            return Err(
                NewsComponentIntensityEvaluationError::InvalidSurpriseOpenWindowShare {
                    value: intensity_input.surprise_open_window_share,
                },
            );
        }

        if !intensity_input.surprise_closed_window_share.is_finite()
            || !(0.0..=1.0).contains(&intensity_input.surprise_closed_window_share)
        {
            return Err(
                NewsComponentIntensityEvaluationError::InvalidSurpriseClosedWindowShare {
                    value: intensity_input.surprise_closed_window_share,
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

        let surprise_open_component_intensity = self.surprise.open_baseline_intensity
            * intensity_input.surprise_open_window_share
            * intensity_input.surprise_deterministic_time_multiplier;
        if !surprise_open_component_intensity.is_finite() || surprise_open_component_intensity < 0.0
        {
            return Err(
                NewsComponentIntensityEvaluationError::InvalidSurpriseComponentIntensity {
                    value: surprise_open_component_intensity,
                },
            );
        }

        let surprise_closed_component_intensity = self.surprise.closed_baseline_intensity
            * intensity_input.surprise_closed_window_share
            * intensity_input.surprise_deterministic_time_multiplier;
        if !surprise_closed_component_intensity.is_finite()
            || surprise_closed_component_intensity < 0.0
        {
            return Err(
                NewsComponentIntensityEvaluationError::InvalidSurpriseComponentIntensity {
                    value: surprise_closed_component_intensity,
                },
            );
        }

        let surprise_component_intensity =
            surprise_open_component_intensity + surprise_closed_component_intensity;
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
            surprise_open_component_intensity,
            surprise_closed_component_intensity,
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
}

impl SharedNewsMarkLawConfiguration {
    pub fn validate(self) -> Result<(), NewsProcessConfigurationError> {
        self.gamma_severity_marks.validate().map_err(|source| {
            NewsProcessConfigurationError::InvalidGammaSeverityMarkConfiguration { source }
        })?;

        Ok(())
    }
}

/// Component-level coupled Gamma-normal mark parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewsComponentMarkScaleConfiguration {
    pub activity_coupling_scale: f64,
    pub return_jump_drift: f64,
    pub return_jump_volatility: f64,
}

impl NewsComponentMarkScaleConfiguration {
    pub const fn neutral() -> Self {
        Self {
            activity_coupling_scale: 0.0,
            return_jump_drift: 0.0,
            return_jump_volatility: 0.0,
        }
    }

    pub fn validate(self) -> Result<(), NewsComponentMarkScaleConfigurationError> {
        if !self.activity_coupling_scale.is_finite() || self.activity_coupling_scale < 0.0 {
            return Err(
                NewsComponentMarkScaleConfigurationError::InvalidActivityCouplingScale {
                    value: self.activity_coupling_scale,
                },
            );
        }

        if !self.return_jump_drift.is_finite() {
            return Err(
                NewsComponentMarkScaleConfigurationError::InvalidReturnJumpDrift {
                    value: self.return_jump_drift,
                },
            );
        }

        if !self.return_jump_volatility.is_finite() || self.return_jump_volatility < 0.0 {
            return Err(
                NewsComponentMarkScaleConfigurationError::InvalidReturnJumpVolatility {
                    value: self.return_jump_volatility,
                },
            );
        }

        if self.activity_coupling_scale == 0.0
            && (self.return_jump_drift != 0.0 || self.return_jump_volatility != 0.0)
        {
            return Err(
                NewsComponentMarkScaleConfigurationError::ReturnJumpWithoutActivityCoupling {
                    return_jump_drift: self.return_jump_drift,
                    return_jump_volatility: self.return_jump_volatility,
                },
            );
        }

        Ok(())
    }

    pub fn construct_gamma_normal_mark(
        self,
        shared_severity_mark: f64,
        shared_return_shock_mark: f64,
    ) -> Result<GammaNormalNewsMarkConstruction, GammaNormalNewsMarkConstructionError> {
        self.validate().map_err(|source| {
            GammaNormalNewsMarkConstructionError::InvalidComponentMarkScaleConfiguration { source }
        })?;

        if !shared_severity_mark.is_finite() || shared_severity_mark < 0.0 {
            return Err(
                GammaNormalNewsMarkConstructionError::InvalidSharedSeverityMark {
                    value: shared_severity_mark,
                },
            );
        }

        if !shared_return_shock_mark.is_finite() {
            return Err(
                GammaNormalNewsMarkConstructionError::InvalidStandardNormalMark {
                    value: shared_return_shock_mark,
                },
            );
        }

        let activity_shock_mark = self.activity_coupling_scale * shared_severity_mark;
        if !activity_shock_mark.is_finite() || activity_shock_mark < 0.0 {
            return Err(
                GammaNormalNewsMarkConstructionError::InvalidConstructedActivityShockMark {
                    value: activity_shock_mark,
                },
            );
        }

        let return_jump_mark = self.return_jump_drift * shared_severity_mark
            + self.return_jump_volatility * shared_severity_mark.sqrt() * shared_return_shock_mark;
        if !return_jump_mark.is_finite() {
            return Err(
                GammaNormalNewsMarkConstructionError::InvalidConstructedReturnJumpMark {
                    value: return_jump_mark,
                },
            );
        }

        Ok(GammaNormalNewsMarkConstruction {
            shared_severity_mark,
            shared_return_shock_mark,
            activity_coupling_scale: self.activity_coupling_scale,
            return_jump_drift: self.return_jump_drift,
            return_jump_volatility: self.return_jump_volatility,
            activity_shock_mark,
            return_jump_mark,
            construction_policy: GAMMA_NORMAL_NEWS_MARK_CONSTRUCTION_POLICY,
        })
    }
}

/// Derive the expected news activity-loading rate from component intensities and mark scales.
pub fn expected_news_activity_loading_rate(
    known_news_intensity: f64,
    surprise_news_intensity: f64,
    known_component_mark_scales: NewsComponentMarkScaleConfiguration,
    surprise_component_mark_scales: NewsComponentMarkScaleConfiguration,
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
) -> Result<f64, NewsActivityLoadingRateError> {
    if !known_news_intensity.is_finite() || known_news_intensity < 0.0 {
        return Err(NewsActivityLoadingRateError::InvalidKnownIntensity {
            value: known_news_intensity,
        });
    }

    if !surprise_news_intensity.is_finite() || surprise_news_intensity < 0.0 {
        return Err(NewsActivityLoadingRateError::InvalidSurpriseIntensity {
            value: surprise_news_intensity,
        });
    }

    known_component_mark_scales.validate().map_err(|source| {
        NewsActivityLoadingRateError::InvalidKnownComponentMarkScales { source }
    })?;

    surprise_component_mark_scales
        .validate()
        .map_err(
            |source| NewsActivityLoadingRateError::InvalidSurpriseComponentMarkScales { source },
        )?;

    let expected_severity_mark = gamma_severity_mark_configuration
        .expected_severity_mark()
        .map_err(
            |source| NewsActivityLoadingRateError::InvalidGammaSeverityMarkConfiguration { source },
        )?;

    let expected_known_loading_rate = known_news_intensity
        * known_component_mark_scales.activity_coupling_scale
        * expected_severity_mark;

    let expected_surprise_loading_rate = surprise_news_intensity
        * surprise_component_mark_scales.activity_coupling_scale
        * expected_severity_mark;

    let expected_loading_rate = expected_known_loading_rate + expected_surprise_loading_rate;
    if !expected_loading_rate.is_finite() || expected_loading_rate < 0.0 {
        return Err(NewsActivityLoadingRateError::InvalidDerivedRate {
            value: expected_loading_rate,
        });
    }

    Ok(expected_loading_rate)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GammaSeverityMarkConfiguration {
    pub shape_k: f64,
}

impl GammaSeverityMarkConfiguration {
    pub const fn new(shape_k: f64) -> Self {
        Self { shape_k }
    }

    pub fn try_new(shape_k: f64) -> Result<Self, NewsMarkConfigurationError> {
        let configuration = Self::new(shape_k);
        configuration.validate()?;
        Ok(configuration)
    }

    pub fn validate(self) -> Result<(), NewsMarkConfigurationError> {
        if !self.shape_k.is_finite() || self.shape_k <= 0.0 {
            return Err(NewsMarkConfigurationError::InvalidGammaShapeK {
                value: self.shape_k,
            });
        }

        Ok(())
    }

    /// Returns the unit-mean Gamma scale implied by a validated shape parameter.
    pub const fn derived_unit_mean_scale(self) -> f64 {
        1.0 / self.shape_k
    }

    pub fn expected_severity_mark(self) -> Result<f64, NewsMarkConfigurationError> {
        self.validate()?;
        Ok(1.0)
    }

    pub fn expected_square_root_severity_mark(self) -> Result<f64, NewsMarkConfigurationError> {
        self.validate()?;

        let unit_mean_scale = self.derived_unit_mean_scale();
        let log_expected_square_root =
            0.5 * unit_mean_scale.ln() + ln_gamma(self.shape_k + 0.5) - ln_gamma(self.shape_k);

        let expected_square_root = log_expected_square_root.exp();
        if !expected_square_root.is_finite() || expected_square_root <= 0.0 {
            return Err(NewsMarkConfigurationError::InvalidGammaShapeK {
                value: self.shape_k,
            });
        }

        Ok(expected_square_root)
    }

    /// Sample one unit-mean severity mark `U ~ Gamma(k_U, 1 / k_U)`.
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

        let sampled_mark = sample_gamma_shape_scale(
            self.shape_k,
            self.derived_unit_mean_scale(),
            randomness_stream,
        );
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StandardNormalNewsReturnShockSampler;

impl NewsReturnShockMarkSampler for StandardNormalNewsReturnShockSampler {
    fn sample_shared_return_shock_mark(
        &self,
        randomness_stream: &mut dyn SimulationRandomnessStream,
    ) -> Result<f64, NewsMarkInterfaceError> {
        let sampled_mark = sample_standard_normal_draw(randomness_stream);
        if !sampled_mark.is_finite() {
            return Err(NewsMarkInterfaceError::component_rejected(
                NewsMarkInterfaceStage::ReturnShockMarkSampling,
                format!("standard normal sampling produced non-finite mark {sampled_mark}"),
            ));
        }

        Ok(sampled_mark)
    }
}

/// Deterministic output for coupled Gamma-normal mark construction from shared `U` and `Z_N`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GammaNormalNewsMarkConstruction {
    pub shared_severity_mark: f64,
    pub shared_return_shock_mark: f64,
    pub activity_coupling_scale: f64,
    pub return_jump_drift: f64,
    pub return_jump_volatility: f64,
    pub activity_shock_mark: f64,
    pub return_jump_mark: f64,
    pub construction_policy: &'static str,
}
