//! Shared exogenous news process configuration and deterministic intensity model.
//!
//! This module defines one typed configuration payload that is reusable by
//! simulation, pricing, and calibration artifact persistence.

use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::Serialize;

use crate::calendar::{
    ExchangeCalendarRegistry, ExchangeCalendarResolveError, MARKET_STATE_RESOLUTION_POLICY,
    MarketState, SESSION_WINDOW_BOUNDARY_POLICY,
};
use crate::randomness::SimulationRandomnessStream;

pub const NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION: &str = "news.process.configuration.v1";
pub const NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY: &str =
    "known_schedule_count_plus_surprise_baseline_times_deterministic_multiplier.v1";
pub const NEWS_INTENSITY_BASELINE_POLICY: &str =
    "closed_window_share_times_baseline_intensity_times_deterministic_multiplier.v1";
pub const NEWS_INTENSITY_DETERMINISTIC_INPUT_POLICY: &str =
    "interval_indexed_non_negative_multiplier_with_default_one.v1";
pub const GAMMA_SEVERITY_MARK_SAMPLING_POLICY: &str =
    "marsaglia_tsang_shape_scale_with_shape_less_than_one_boost.v1";
pub const DOUBLE_EXPONENTIAL_BASE_SHOCK_SAMPLING_POLICY: &str =
    "kou_two_sided_exponential_with_constant_mixture_probability.v1";
pub const COUPLED_GAP_MARK_CONSTRUCTION_POLICY: &str =
    "sigma_g_times_square_root_shared_severity_mark_times_shared_base_shock_mark.v1";
pub const SHARED_NEWS_EVENT_DISPATCH_ORDERING_POLICY: &str =
    "event_timestamp_then_generation_order.v1";
pub const SHARED_NEWS_EVENT_DISPATCH_JT_UPDATE_POLICY: &str =
    "every_news_event_adds_shared_severity_mark_to_jt.v1";
pub const SHARED_NEWS_EVENT_DISPATCH_MT_GATING_POLICY: &str =
    "apply_shared_return_jump_mark_only_for_open_market_state.v1";
pub const SHARED_NEWS_EVENT_DISPATCH_GT_GATING_POLICY: &str =
    "apply_shared_return_jump_mark_only_for_closed_market_state.v1";
pub const SHARED_NEWS_EVENT_DISPATCH_RETURN_ROUTING_POLICY: &str =
    "open_market_state_routes_to_mt_closed_market_state_routes_to_gt.v1";
pub const SHARED_NEWS_EVENT_MARKET_STATE_DEPENDENCY: &str = MARKET_STATE_RESOLUTION_POLICY;
pub const SHARED_NEWS_EVENT_BOUNDARY_POLICY: &str = SESSION_WINDOW_BOUNDARY_POLICY;
pub const COMPOSABLE_NEWS_MARK_INTERFACE_POLICY: &str =
    "component_arrival_intensity_then_shared_mark_sampling_then_coupled_mark_construction.v1";

const ONE_SECOND_NANOS: i64 = 1_000_000_000;

/// Explicit component stages in the composable news-mark generation lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsMarkInterfaceStage {
    ArrivalIntensityGeneration,
    SeverityMarkSampling,
    BaseShockMarkSampling,
    CoupledMarkConstruction,
}

/// Stable interface error for composable arrival and mark-generation components.
#[derive(Debug, Clone, PartialEq)]
pub enum NewsMarkInterfaceError {
    ComponentRejected {
        stage: NewsMarkInterfaceStage,
        reason: String,
    },
    InvalidGeneratedKnownArrivalIntensity {
        value: f64,
    },
    InvalidGeneratedSurpriseArrivalIntensity {
        value: f64,
    },
    InvalidGeneratedTotalArrivalIntensity {
        value: f64,
    },
    InvalidSampledSeverityMark {
        value: f64,
    },
    InvalidSampledBaseShockMark {
        value: f64,
    },
    InvalidConstructedGapMark {
        value: f64,
    },
    ConstructedSeverityMarkMismatch {
        expected: f64,
        observed: f64,
    },
    ConstructedBaseShockMarkMismatch {
        expected: f64,
        observed: f64,
    },
}

impl NewsMarkInterfaceError {
    fn component_rejected(
        stage: NewsMarkInterfaceStage,
        reason: impl Into<String>,
    ) -> NewsMarkInterfaceError {
        Self::ComponentRejected {
            stage,
            reason: reason.into(),
        }
    }
}

impl Display for NewsMarkInterfaceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ComponentRejected { stage, reason } => {
                write!(
                    formatter,
                    "composable news-mark component at stage `{stage:?}` rejected input: {reason}"
                )
            }
            Self::InvalidGeneratedKnownArrivalIntensity { value } => write!(
                formatter,
                "composable known-news arrival-intensity output must be finite and non-negative, found {value}"
            ),
            Self::InvalidGeneratedSurpriseArrivalIntensity { value } => write!(
                formatter,
                "composable surprise-news arrival-intensity output must be finite and non-negative, found {value}"
            ),
            Self::InvalidGeneratedTotalArrivalIntensity { value } => write!(
                formatter,
                "composable total news arrival-intensity output must be finite and non-negative, found {value}"
            ),
            Self::InvalidSampledSeverityMark { value } => write!(
                formatter,
                "composable news severity sample must be finite and non-negative, found {value}"
            ),
            Self::InvalidSampledBaseShockMark { value } => write!(
                formatter,
                "composable news base-shock sample must be finite, found {value}"
            ),
            Self::InvalidConstructedGapMark { value } => write!(
                formatter,
                "composable coupled gap mark must be finite, found {value}"
            ),
            Self::ConstructedSeverityMarkMismatch { expected, observed } => write!(
                formatter,
                "composable coupled-mark constructor must preserve the shared severity mark, expected {expected} and observed {observed}"
            ),
            Self::ConstructedBaseShockMarkMismatch { expected, observed } => write!(
                formatter,
                "composable coupled-mark constructor must preserve the shared base-shock mark, expected {expected} and observed {observed}"
            ),
        }
    }
}

impl Error for NewsMarkInterfaceError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewsArrivalComponent {
    Known,
    Surprise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewsReturnJumpRoutingTarget {
    OpenWindowDirectJumpM,
    ClosedWindowGapG,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NewsArrivalIntensityEvaluationInput {
    pub interval_start_utc_nanos: i64,
    pub interval_end_utc_nanos: i64,
    pub surprise_deterministic_time_multiplier: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewsArrivalIntensityEvaluationOutput {
    pub known_scheduled_arrival_count: u64,
    pub known_component_intensity: f64,
    pub surprise_component_intensity: f64,
    pub total_intensity: f64,
    pub intensity_policy: &'static str,
}

/// Interface boundary for deterministic news-arrival intensity generation.
pub trait NewsArrivalIntensityGenerator {
    fn generate_arrival_intensity(
        &self,
        intensity_input: NewsArrivalIntensityEvaluationInput,
    ) -> Result<NewsArrivalIntensityEvaluationOutput, NewsMarkInterfaceError>;
}

/// Interface boundary for shared severity-mark sampling.
pub trait NewsSeverityMarkSampler {
    fn sample_shared_severity_mark(
        &self,
        randomness_stream: &mut dyn SimulationRandomnessStream,
    ) -> Result<f64, NewsMarkInterfaceError>;
}

/// Interface boundary for shared base-shock sampling.
pub trait NewsBaseShockMarkSampler {
    fn sample_shared_base_shock_mark(
        &self,
        randomness_stream: &mut dyn SimulationRandomnessStream,
    ) -> Result<f64, NewsMarkInterfaceError>;
}

/// Interface boundary for coupled mark construction from shared severity and base-shock marks.
pub trait NewsCoupledMarkConstructor {
    fn construct_coupled_mark(
        &self,
        shared_severity_mark: f64,
        shared_base_shock_mark: f64,
    ) -> Result<CoupledGapMarkConstruction, NewsMarkInterfaceError>;
}

/// Fully composed mark payload for one exogenous news arrival.
///
/// Ownership and lifecycle:
/// - This payload owns plain values only and does not borrow from any generator component.
/// - Callers may persist it for replay or diagnostics independent from generator lifetime.
/// - `shared_severity_mark` is the same value used by both Hawkes loading and gap construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposableNewsMarkPayload {
    pub shared_severity_mark: f64,
    pub shared_base_shock_mark: f64,
    pub coupled_gap_mark_construction: CoupledGapMarkConstruction,
}

/// Minimal composable pipeline for arrival intensity and coupled news-mark generation.
///
/// This struct owns component implementations and orchestrates one deterministic flow:
/// 1. Arrival intensity generation
/// 2. Shared severity-mark sampling
/// 3. Shared base-shock sampling
/// 4. Coupled mark construction
///
/// The pipeline enforces output invariants even for alternative component implementations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposableNewsMarkPipeline<
    ArrivalIntensityGenerator,
    SeverityMarkSampler,
    BaseShockMarkSampler,
    CoupledMarkConstructor,
> {
    arrival_intensity_generator: ArrivalIntensityGenerator,
    severity_mark_sampler: SeverityMarkSampler,
    base_shock_mark_sampler: BaseShockMarkSampler,
    coupled_mark_constructor: CoupledMarkConstructor,
}

impl<ArrivalIntensityGenerator, SeverityMarkSampler, BaseShockMarkSampler, CoupledMarkConstructor>
    ComposableNewsMarkPipeline<
        ArrivalIntensityGenerator,
        SeverityMarkSampler,
        BaseShockMarkSampler,
        CoupledMarkConstructor,
    >
where
    ArrivalIntensityGenerator: NewsArrivalIntensityGenerator,
    SeverityMarkSampler: NewsSeverityMarkSampler,
    BaseShockMarkSampler: NewsBaseShockMarkSampler,
    CoupledMarkConstructor: NewsCoupledMarkConstructor,
{
    pub const fn new(
        arrival_intensity_generator: ArrivalIntensityGenerator,
        severity_mark_sampler: SeverityMarkSampler,
        base_shock_mark_sampler: BaseShockMarkSampler,
        coupled_mark_constructor: CoupledMarkConstructor,
    ) -> Self {
        Self {
            arrival_intensity_generator,
            severity_mark_sampler,
            base_shock_mark_sampler,
            coupled_mark_constructor,
        }
    }

    pub fn generate_arrival_intensity(
        &self,
        intensity_input: NewsArrivalIntensityEvaluationInput,
    ) -> Result<NewsArrivalIntensityEvaluationOutput, NewsMarkInterfaceError> {
        let generated_arrival_intensity = self
            .arrival_intensity_generator
            .generate_arrival_intensity(intensity_input)?;
        if !generated_arrival_intensity
            .known_component_intensity
            .is_finite()
            || generated_arrival_intensity.known_component_intensity < 0.0
        {
            return Err(
                NewsMarkInterfaceError::InvalidGeneratedKnownArrivalIntensity {
                    value: generated_arrival_intensity.known_component_intensity,
                },
            );
        }
        if !generated_arrival_intensity
            .surprise_component_intensity
            .is_finite()
            || generated_arrival_intensity.surprise_component_intensity < 0.0
        {
            return Err(
                NewsMarkInterfaceError::InvalidGeneratedSurpriseArrivalIntensity {
                    value: generated_arrival_intensity.surprise_component_intensity,
                },
            );
        }
        if !generated_arrival_intensity.total_intensity.is_finite()
            || generated_arrival_intensity.total_intensity < 0.0
        {
            return Err(
                NewsMarkInterfaceError::InvalidGeneratedTotalArrivalIntensity {
                    value: generated_arrival_intensity.total_intensity,
                },
            );
        }

        Ok(generated_arrival_intensity)
    }

    pub fn sample_coupled_marks(
        &self,
        randomness_stream: &mut dyn SimulationRandomnessStream,
    ) -> Result<ComposableNewsMarkPayload, NewsMarkInterfaceError> {
        let shared_severity_mark = self
            .severity_mark_sampler
            .sample_shared_severity_mark(randomness_stream)?;
        if !shared_severity_mark.is_finite() || shared_severity_mark < 0.0 {
            return Err(NewsMarkInterfaceError::InvalidSampledSeverityMark {
                value: shared_severity_mark,
            });
        }

        let shared_base_shock_mark = self
            .base_shock_mark_sampler
            .sample_shared_base_shock_mark(randomness_stream)?;
        if !shared_base_shock_mark.is_finite() {
            return Err(NewsMarkInterfaceError::InvalidSampledBaseShockMark {
                value: shared_base_shock_mark,
            });
        }

        let coupled_gap_mark_construction = self
            .coupled_mark_constructor
            .construct_coupled_mark(shared_severity_mark, shared_base_shock_mark)?;
        if coupled_gap_mark_construction.shared_severity_mark.to_bits()
            != shared_severity_mark.to_bits()
        {
            return Err(NewsMarkInterfaceError::ConstructedSeverityMarkMismatch {
                expected: shared_severity_mark,
                observed: coupled_gap_mark_construction.shared_severity_mark,
            });
        }
        if coupled_gap_mark_construction
            .shared_base_shock_mark
            .to_bits()
            != shared_base_shock_mark.to_bits()
        {
            return Err(NewsMarkInterfaceError::ConstructedBaseShockMarkMismatch {
                expected: shared_base_shock_mark,
                observed: coupled_gap_mark_construction.shared_base_shock_mark,
            });
        }
        if !coupled_gap_mark_construction.gap_jump_mark.is_finite() {
            return Err(NewsMarkInterfaceError::InvalidConstructedGapMark {
                value: coupled_gap_mark_construction.gap_jump_mark,
            });
        }

        Ok(ComposableNewsMarkPayload {
            shared_severity_mark,
            shared_base_shock_mark,
            coupled_gap_mark_construction,
        })
    }
}

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
            if let Some(previous_timestamp_utc_nanos_value) = previous_timestamp_utc_nanos {
                if timestamp_utc_nanos <= previous_timestamp_utc_nanos_value {
                    return Err(
                        NewsArrivalConfigurationError::KnownScheduledTimestampsNotStrictlyIncreasing {
                            previous_timestamp_utc_nanos: previous_timestamp_utc_nanos_value,
                            current_timestamp_utc_nanos: timestamp_utc_nanos,
                        },
                    );
                }
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
        let known_component_intensity = known_scheduled_arrival_count as f64;
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

/// Running shared-news accumulators for Hawkes shock loading `J_t`,
/// open-window direct jumps `M_t`, and closed-window gap jumps `G_t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SharedNewsAccumulatorState {
    pub j_t: f64,
    pub m_t: f64,
    pub g_t: f64,
}

impl SharedNewsAccumulatorState {
    pub const fn new(j_t: f64, m_t: f64, g_t: f64) -> Self {
        Self { j_t, m_t, g_t }
    }
}

/// Shared dispatch input for one exogenous news event.
///
/// Dispatch ordering is deterministic and follows:
/// 1. lower `event_timestamp_utc_nanos`
/// 2. lower `event_generation_order`
///
/// Every event contributes to `J_t` using `shared_severity_mark`.
/// `coupled_return_jump_mark` contributes to `M_t` when `market_state == open`
/// and to `G_t` when `market_state == closed`.
/// `market_state` should come from canonical calendar resolver output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SharedNewsEventDispatchInput {
    pub event_timestamp_utc_nanos: i64,
    pub event_generation_order: u64,
    pub news_arrival_component: NewsArrivalComponent,
    pub market_state: MarketState,
    pub shared_severity_mark: f64,
    pub coupled_return_jump_mark: f64,
}

impl SharedNewsEventDispatchInput {
    const fn ordering_key(self) -> (i64, u64) {
        (self.event_timestamp_utc_nanos, self.event_generation_order)
    }
}

/// Shared news-event payload where market state is resolved from the canonical
/// calendar service before dispatch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SharedNewsEventTimestampedInput {
    pub event_timestamp_utc_nanos: i64,
    pub event_generation_order: u64,
    pub news_arrival_component: NewsArrivalComponent,
    pub shared_severity_mark: f64,
    pub coupled_return_jump_mark: f64,
}

/// Resolve market state for shared news events from the canonical calendar service.
///
/// The resolver uses `resolve_market_state_at_utc` and inherits
/// `SESSION_WINDOW_BOUNDARY_POLICY`, where session windows are `[open, close)`.
pub fn resolve_shared_news_event_market_states(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    event_inputs: &[SharedNewsEventTimestampedInput],
) -> Result<Vec<SharedNewsEventDispatchInput>, SharedNewsEventDispatchError> {
    let mut resolved_event_inputs: Vec<SharedNewsEventDispatchInput> =
        Vec::with_capacity(event_inputs.len());

    for event_input in event_inputs {
        let event_timestamp_utc = utc_timestamp_from_nanos(event_input.event_timestamp_utc_nanos)
            .ok_or(
            SharedNewsEventDispatchError::InvalidEventTimestampUtcNanos {
                event_generation_order: event_input.event_generation_order,
                value: event_input.event_timestamp_utc_nanos,
            },
        )?;
        let market_state = calendar_registry
            .resolve_market_state_at_utc(exchange_code, event_timestamp_utc)
            .map_err(
                |source| SharedNewsEventDispatchError::MarketStateResolutionFailed {
                    event_generation_order: event_input.event_generation_order,
                    exchange_code: exchange_code.to_string(),
                    source,
                },
            )?;

        resolved_event_inputs.push(SharedNewsEventDispatchInput {
            event_timestamp_utc_nanos: event_input.event_timestamp_utc_nanos,
            event_generation_order: event_input.event_generation_order,
            news_arrival_component: event_input.news_arrival_component,
            market_state,
            shared_severity_mark: event_input.shared_severity_mark,
            coupled_return_jump_mark: event_input.coupled_return_jump_mark,
        });
    }

    Ok(resolved_event_inputs)
}

/// Per-event dispatch record produced by shared `J_t`, `M_t`, and `G_t` updates.
#[derive(Debug, Clone, PartialEq)]
pub struct SharedNewsEventDispatchRecord {
    pub event_timestamp_utc_nanos: i64,
    pub event_generation_order: u64,
    pub news_arrival_component: NewsArrivalComponent,
    pub market_state: MarketState,
    pub return_jump_routing_target: NewsReturnJumpRoutingTarget,
    pub shared_severity_mark: f64,
    pub coupled_return_jump_mark: f64,
    pub j_t_increment: f64,
    pub m_t_increment: f64,
    pub g_t_increment: f64,
    pub j_t_after_event: f64,
    pub m_t_after_event: f64,
    pub g_t_after_event: f64,
}

/// Shared dispatch output reusable by simulation and coarse-time engines.
#[derive(Debug, Clone, PartialEq)]
pub struct SharedNewsEventDispatchOutput {
    pub ordering_policy: &'static str,
    pub j_t_update_policy: &'static str,
    pub m_t_gating_policy: &'static str,
    pub g_t_gating_policy: &'static str,
    pub return_jump_routing_policy: &'static str,
    pub market_state_dependency: &'static str,
    pub market_state_boundary_policy: &'static str,
    pub records: Vec<SharedNewsEventDispatchRecord>,
    pub final_state: SharedNewsAccumulatorState,
}

/// Structured failures emitted by shared news-event dispatch.
#[derive(Debug, Clone, PartialEq)]
pub enum SharedNewsEventDispatchError {
    InvalidInitialJtAccumulator {
        value: f64,
    },
    InvalidInitialMtAccumulator {
        value: f64,
    },
    InvalidInitialGtAccumulator {
        value: f64,
    },
    InvalidEventTimestampUtcNanos {
        event_generation_order: u64,
        value: i64,
    },
    MarketStateResolutionFailed {
        event_generation_order: u64,
        exchange_code: String,
        source: ExchangeCalendarResolveError,
    },
    InvalidSeverityMark {
        event_generation_order: u64,
        value: f64,
    },
    InvalidReturnJumpMark {
        event_generation_order: u64,
        value: f64,
    },
    InvalidAccumulatedJt {
        event_generation_order: u64,
        value: f64,
    },
    InvalidAccumulatedMt {
        event_generation_order: u64,
        value: f64,
    },
    InvalidAccumulatedGt {
        event_generation_order: u64,
        value: f64,
    },
}

impl Display for SharedNewsEventDispatchError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInitialJtAccumulator { value } => write!(
                formatter,
                "initial J_t accumulator must be finite, found {value}"
            ),
            Self::InvalidInitialMtAccumulator { value } => write!(
                formatter,
                "initial M_t accumulator must be finite, found {value}"
            ),
            Self::InvalidInitialGtAccumulator { value } => write!(
                formatter,
                "initial G_t accumulator must be finite, found {value}"
            ),
            Self::InvalidEventTimestampUtcNanos {
                event_generation_order,
                value,
            } => write!(
                formatter,
                "event timestamp nanos for generation order {event_generation_order} must map to a valid UTC timestamp, found {value}"
            ),
            Self::MarketStateResolutionFailed {
                event_generation_order,
                exchange_code,
                source,
            } => write!(
                formatter,
                "failed to resolve market state for exchange `{exchange_code}` at event generation order {event_generation_order}: {source}"
            ),
            Self::InvalidSeverityMark {
                event_generation_order,
                value,
            } => write!(
                formatter,
                "shared severity mark for event generation order {event_generation_order} must be finite and non-negative, found {value}"
            ),
            Self::InvalidReturnJumpMark {
                event_generation_order,
                value,
            } => write!(
                formatter,
                "shared return jump mark for event generation order {event_generation_order} must be finite, found {value}"
            ),
            Self::InvalidAccumulatedJt {
                event_generation_order,
                value,
            } => write!(
                formatter,
                "accumulated J_t after event generation order {event_generation_order} must be finite, found {value}"
            ),
            Self::InvalidAccumulatedMt {
                event_generation_order,
                value,
            } => write!(
                formatter,
                "accumulated M_t after event generation order {event_generation_order} must be finite, found {value}"
            ),
            Self::InvalidAccumulatedGt {
                event_generation_order,
                value,
            } => write!(
                formatter,
                "accumulated G_t after event generation order {event_generation_order} must be finite, found {value}"
            ),
        }
    }
}

impl Error for SharedNewsEventDispatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MarketStateResolutionFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Dispatch shared news events through one canonical `J_t` plus `M_t` plus `G_t` update path.
///
/// Event processing order is deterministic and documented by
/// `SHARED_NEWS_EVENT_DISPATCH_ORDERING_POLICY`.
pub fn dispatch_shared_news_events(
    initial_state: SharedNewsAccumulatorState,
    event_inputs: &[SharedNewsEventDispatchInput],
) -> Result<SharedNewsEventDispatchOutput, SharedNewsEventDispatchError> {
    validate_initial_dispatch_state(initial_state)?;

    let mut ordered_event_inputs: Vec<SharedNewsEventDispatchInput> = event_inputs.to_vec();
    ordered_event_inputs.sort_by_key(|event_input| event_input.ordering_key());

    let mut next_state = initial_state;
    let mut dispatch_records: Vec<SharedNewsEventDispatchRecord> =
        Vec::with_capacity(ordered_event_inputs.len());

    for event_input in ordered_event_inputs {
        let dispatch_record = dispatch_one_shared_news_event(event_input, &mut next_state)?;
        dispatch_records.push(dispatch_record);
    }

    Ok(SharedNewsEventDispatchOutput {
        ordering_policy: SHARED_NEWS_EVENT_DISPATCH_ORDERING_POLICY,
        j_t_update_policy: SHARED_NEWS_EVENT_DISPATCH_JT_UPDATE_POLICY,
        m_t_gating_policy: SHARED_NEWS_EVENT_DISPATCH_MT_GATING_POLICY,
        g_t_gating_policy: SHARED_NEWS_EVENT_DISPATCH_GT_GATING_POLICY,
        return_jump_routing_policy: SHARED_NEWS_EVENT_DISPATCH_RETURN_ROUTING_POLICY,
        market_state_dependency: SHARED_NEWS_EVENT_MARKET_STATE_DEPENDENCY,
        market_state_boundary_policy: SHARED_NEWS_EVENT_BOUNDARY_POLICY,
        records: dispatch_records,
        final_state: next_state,
    })
}

/// Resolve canonical market states and dispatch shared news events through one path.
pub fn dispatch_shared_news_events_with_resolved_market_states(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    initial_state: SharedNewsAccumulatorState,
    event_inputs: &[SharedNewsEventTimestampedInput],
) -> Result<SharedNewsEventDispatchOutput, SharedNewsEventDispatchError> {
    let resolved_event_inputs =
        resolve_shared_news_event_market_states(calendar_registry, exchange_code, event_inputs)?;
    dispatch_shared_news_events(initial_state, &resolved_event_inputs)
}

fn validate_initial_dispatch_state(
    initial_state: SharedNewsAccumulatorState,
) -> Result<(), SharedNewsEventDispatchError> {
    if !initial_state.j_t.is_finite() {
        return Err(SharedNewsEventDispatchError::InvalidInitialJtAccumulator {
            value: initial_state.j_t,
        });
    }
    if !initial_state.m_t.is_finite() {
        return Err(SharedNewsEventDispatchError::InvalidInitialMtAccumulator {
            value: initial_state.m_t,
        });
    }
    if !initial_state.g_t.is_finite() {
        return Err(SharedNewsEventDispatchError::InvalidInitialGtAccumulator {
            value: initial_state.g_t,
        });
    }

    Ok(())
}

fn dispatch_one_shared_news_event(
    event_input: SharedNewsEventDispatchInput,
    next_state: &mut SharedNewsAccumulatorState,
) -> Result<SharedNewsEventDispatchRecord, SharedNewsEventDispatchError> {
    if !event_input.shared_severity_mark.is_finite() || event_input.shared_severity_mark < 0.0 {
        return Err(SharedNewsEventDispatchError::InvalidSeverityMark {
            event_generation_order: event_input.event_generation_order,
            value: event_input.shared_severity_mark,
        });
    }
    if !event_input.coupled_return_jump_mark.is_finite() {
        return Err(SharedNewsEventDispatchError::InvalidReturnJumpMark {
            event_generation_order: event_input.event_generation_order,
            value: event_input.coupled_return_jump_mark,
        });
    }

    let j_t_increment = event_input.shared_severity_mark;
    let (return_jump_routing_target, m_t_increment, g_t_increment) = match event_input.market_state
    {
        MarketState::Open => (
            NewsReturnJumpRoutingTarget::OpenWindowDirectJumpM,
            event_input.coupled_return_jump_mark,
            0.0,
        ),
        MarketState::Closed => (
            NewsReturnJumpRoutingTarget::ClosedWindowGapG,
            0.0,
            event_input.coupled_return_jump_mark,
        ),
    };

    next_state.j_t += j_t_increment;
    if !next_state.j_t.is_finite() {
        return Err(SharedNewsEventDispatchError::InvalidAccumulatedJt {
            event_generation_order: event_input.event_generation_order,
            value: next_state.j_t,
        });
    }

    next_state.m_t += m_t_increment;
    if !next_state.m_t.is_finite() {
        return Err(SharedNewsEventDispatchError::InvalidAccumulatedMt {
            event_generation_order: event_input.event_generation_order,
            value: next_state.m_t,
        });
    }

    next_state.g_t += g_t_increment;
    if !next_state.g_t.is_finite() {
        return Err(SharedNewsEventDispatchError::InvalidAccumulatedGt {
            event_generation_order: event_input.event_generation_order,
            value: next_state.g_t,
        });
    }

    Ok(SharedNewsEventDispatchRecord {
        event_timestamp_utc_nanos: event_input.event_timestamp_utc_nanos,
        event_generation_order: event_input.event_generation_order,
        news_arrival_component: event_input.news_arrival_component,
        market_state: event_input.market_state,
        return_jump_routing_target,
        shared_severity_mark: event_input.shared_severity_mark,
        coupled_return_jump_mark: event_input.coupled_return_jump_mark,
        j_t_increment,
        m_t_increment,
        g_t_increment,
        j_t_after_event: next_state.j_t,
        m_t_after_event: next_state.m_t,
        g_t_after_event: next_state.g_t,
    })
}

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

#[derive(Debug, Clone, PartialEq)]
pub enum NewsArrivalConfigurationError {
    InvalidKnownScheduledTimestampUtcNanos {
        value: i64,
    },
    KnownScheduledTimestampsNotStrictlyIncreasing {
        previous_timestamp_utc_nanos: i64,
        current_timestamp_utc_nanos: i64,
    },
    InvalidSurpriseBaselineIntensity {
        value: f64,
    },
}

impl Display for NewsArrivalConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKnownScheduledTimestampUtcNanos { value } => write!(
                formatter,
                "known.scheduled_event_timestamps_utc_nanos contains an invalid UTC timestamp nanos value {value}"
            ),
            Self::KnownScheduledTimestampsNotStrictlyIncreasing {
                previous_timestamp_utc_nanos,
                current_timestamp_utc_nanos,
            } => write!(
                formatter,
                "known.scheduled_event_timestamps_utc_nanos must be strictly increasing, found previous={previous_timestamp_utc_nanos} and current={current_timestamp_utc_nanos}"
            ),
            Self::InvalidSurpriseBaselineIntensity { value } => write!(
                formatter,
                "surprise.baseline_intensity must be a finite non-negative value, found {value}"
            ),
        }
    }
}

impl Error for NewsArrivalConfigurationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsComponentIntensityEvaluationError {
    InvalidConfiguration {
        source: NewsArrivalConfigurationError,
    },
    NonIncreasingInterval {
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
    },
    InvalidIntervalStartTimestampUtcNanos {
        value: i64,
    },
    InvalidIntervalEndTimestampUtcNanos {
        value: i64,
    },
    InvalidSurpriseDeterministicTimeMultiplier {
        value: f64,
    },
    KnownScheduledArrivalCountOverflow {
        value: usize,
    },
    InvalidKnownComponentIntensity {
        value: f64,
    },
    InvalidSurpriseComponentIntensity {
        value: f64,
    },
    InvalidTotalIntensity {
        value: f64,
    },
}

impl Display for NewsComponentIntensityEvaluationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration { source } => write!(formatter, "{source}"),
            Self::NonIncreasingInterval {
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            } => write!(
                formatter,
                "arrival-intensity interval must be increasing, found start={interval_start_utc_nanos} and end={interval_end_utc_nanos}"
            ),
            Self::InvalidIntervalStartTimestampUtcNanos { value } => write!(
                formatter,
                "interval_start_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::InvalidIntervalEndTimestampUtcNanos { value } => write!(
                formatter,
                "interval_end_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::InvalidSurpriseDeterministicTimeMultiplier { value } => write!(
                formatter,
                "surprise_deterministic_time_multiplier must be finite and non-negative, found {value}"
            ),
            Self::KnownScheduledArrivalCountOverflow { value } => write!(
                formatter,
                "known scheduled arrival count overflowed u64 conversion, found {value}"
            ),
            Self::InvalidKnownComponentIntensity { value } => write!(
                formatter,
                "known-component arrival intensity must be finite and non-negative, found {value}"
            ),
            Self::InvalidSurpriseComponentIntensity { value } => write!(
                formatter,
                "surprise-component arrival intensity must be finite and non-negative, found {value}"
            ),
            Self::InvalidTotalIntensity { value } => write!(
                formatter,
                "total arrival intensity must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for NewsComponentIntensityEvaluationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfiguration { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NewsComponentMarkScaleConfigurationError {
    InvalidActivityCouplingScale { value: f64 },
    InvalidReturnJumpScale { value: f64 },
}

impl Display for NewsComponentMarkScaleConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidActivityCouplingScale { value } => write!(
                formatter,
                "activity_coupling_scale must be a finite non-negative value, found {value}"
            ),
            Self::InvalidReturnJumpScale { value } => write!(
                formatter,
                "return_jump_scale must be a finite non-negative value, found {value}"
            ),
        }
    }
}

impl Error for NewsComponentMarkScaleConfigurationError {}

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
    UnsupportedSchemaVersion {
        value: String,
    },
    InvalidArrivalConfiguration {
        source: NewsArrivalConfigurationError,
    },
    InvalidGammaSeverityMarkConfiguration {
        source: NewsMarkConfigurationError,
    },
    InvalidBaseShockConfiguration {
        source: NewsMarkConfigurationError,
    },
    InvalidCoupledGapMarkConfiguration {
        source: NewsMarkConfigurationError,
    },
    InvalidKnownComponentMarkScales {
        source: NewsComponentMarkScaleConfigurationError,
    },
    InvalidSurpriseComponentMarkScales {
        source: NewsComponentMarkScaleConfigurationError,
    },
}

impl Display for NewsProcessConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { value } => write!(
                formatter,
                "unsupported news process configuration schema version `{value}`, expected `{NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION}`"
            ),
            Self::InvalidArrivalConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidGammaSeverityMarkConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidBaseShockConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidCoupledGapMarkConfiguration { source } => write!(formatter, "{source}"),
            Self::InvalidKnownComponentMarkScales { source } => write!(formatter, "{source}"),
            Self::InvalidSurpriseComponentMarkScales { source } => write!(formatter, "{source}"),
        }
    }
}

impl Error for NewsProcessConfigurationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidArrivalConfiguration { source } => Some(source),
            Self::InvalidGammaSeverityMarkConfiguration { source } => Some(source),
            Self::InvalidBaseShockConfiguration { source } => Some(source),
            Self::InvalidCoupledGapMarkConfiguration { source } => Some(source),
            Self::InvalidKnownComponentMarkScales { source } => Some(source),
            Self::InvalidSurpriseComponentMarkScales { source } => Some(source),
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

#[derive(Debug, Clone, PartialEq)]
pub enum GammaSeverityMarkSamplingError {
    InvalidConfiguration { source: NewsMarkConfigurationError },
    InvalidSampledMark { value: f64 },
}

impl Display for GammaSeverityMarkSamplingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration { source } => {
                write!(
                    formatter,
                    "gamma severity mark configuration is invalid for sampling: {source}"
                )
            }
            Self::InvalidSampledMark { value } => write!(
                formatter,
                "gamma severity sampling produced a non-finite or negative mark, found {value}"
            ),
        }
    }
}

impl Error for GammaSeverityMarkSamplingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfiguration { source } => Some(source),
            Self::InvalidSampledMark { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DoubleExponentialBaseShockSamplingError {
    InvalidConfiguration { source: NewsMarkConfigurationError },
    InvalidSampledMark { value: f64 },
}

impl Display for DoubleExponentialBaseShockSamplingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration { source } => {
                write!(
                    formatter,
                    "double-exponential base-shock configuration is invalid for sampling: {source}"
                )
            }
            Self::InvalidSampledMark { value } => write!(
                formatter,
                "double-exponential base-shock sampling produced a non-finite mark, found {value}"
            ),
        }
    }
}

impl Error for DoubleExponentialBaseShockSamplingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfiguration { source } => Some(source),
            Self::InvalidSampledMark { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoupledGapMarkConstructionError {
    InvalidConfiguration { source: NewsMarkConfigurationError },
    InvalidSharedSeverityMark { value: f64 },
    InvalidSharedBaseShockMark { value: f64 },
    InvalidConstructedGapMark { value: f64 },
}

impl Display for CoupledGapMarkConstructionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration { source } => {
                write!(
                    formatter,
                    "coupled gap-mark configuration is invalid for construction: {source}"
                )
            }
            Self::InvalidSharedSeverityMark { value } => write!(
                formatter,
                "shared severity mark must be finite and non-negative, found {value}"
            ),
            Self::InvalidSharedBaseShockMark { value } => write!(
                formatter,
                "shared base-shock mark must be finite, found {value}"
            ),
            Self::InvalidConstructedGapMark { value } => write!(
                formatter,
                "constructed coupled gap mark must be finite, found {value}"
            ),
        }
    }
}

impl Error for CoupledGapMarkConstructionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfiguration { source } => Some(source),
            Self::InvalidSharedSeverityMark { .. }
            | Self::InvalidSharedBaseShockMark { .. }
            | Self::InvalidConstructedGapMark { .. } => None,
        }
    }
}

fn sample_gamma_shape_scale<R>(shape_k: f64, scale_theta: f64, randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
{
    sample_gamma_unit_scale(shape_k, randomness_stream) * scale_theta
}

fn sample_gamma_unit_scale<R>(shape_k: f64, randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
{
    if shape_k < 1.0 {
        let boosted_sample = sample_gamma_shape_at_least_one(shape_k + 1.0, randomness_stream);
        let uniform_draw = randomness_stream
            .next_unit_interval_f64()
            .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);
        return boosted_sample * uniform_draw.powf(1.0 / shape_k);
    }

    sample_gamma_shape_at_least_one(shape_k, randomness_stream)
}

fn sample_gamma_shape_at_least_one<R>(shape_k: f64, randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
{
    let d = shape_k - (1.0 / 3.0);
    let c = (9.0 * d).sqrt().recip();

    loop {
        let standard_normal_draw = sample_standard_normal_draw(randomness_stream);
        let one_plus_cx = 1.0 + c * standard_normal_draw;
        if one_plus_cx <= 0.0 {
            continue;
        }

        let v = one_plus_cx * one_plus_cx * one_plus_cx;
        let uniform_draw = randomness_stream
            .next_unit_interval_f64()
            .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);

        if uniform_draw < 1.0 - 0.0331 * standard_normal_draw.powi(4) {
            return d * v;
        }

        if uniform_draw.ln()
            < 0.5 * standard_normal_draw * standard_normal_draw + d * (1.0 - v + v.ln())
        {
            return d * v;
        }
    }
}

fn sample_standard_normal_draw<R>(randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
{
    let first_uniform_draw = randomness_stream
        .next_unit_interval_f64()
        .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);
    let second_uniform_draw = randomness_stream
        .next_unit_interval_f64()
        .clamp(0.0, 1.0 - f64::EPSILON);

    let radius = (-2.0 * first_uniform_draw.ln()).sqrt();
    let angle = 2.0 * std::f64::consts::PI * second_uniform_draw;

    radius * angle.cos()
}

fn sample_exponential_with_rate<R>(rate: f64, randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
{
    let uniform_draw = randomness_stream
        .next_unit_interval_f64()
        .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);

    -uniform_draw.ln() / rate
}

fn utc_timestamp_from_nanos(timestamp_utc_nanos: i64) -> Option<DateTime<Utc>> {
    let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
    let subsecond_nanos = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;

    DateTime::from_timestamp(seconds, subsecond_nanos)
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::{
        COMPOSABLE_NEWS_MARK_INTERFACE_POLICY, COUPLED_GAP_MARK_CONSTRUCTION_POLICY,
        ComposableNewsMarkPipeline, CoupledGapMarkConfiguration, CoupledGapMarkConstruction,
        CoupledGapMarkConstructionError, DOUBLE_EXPONENTIAL_BASE_SHOCK_SAMPLING_POLICY,
        DefaultNewsMarkPipeline, DoubleExponentialBaseShockConfiguration,
        DoubleExponentialBaseShockSamplingError, GAMMA_SEVERITY_MARK_SAMPLING_POLICY,
        GammaSeverityMarkConfiguration, GammaSeverityMarkSamplingError,
        NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY, NEWS_INTENSITY_BASELINE_POLICY,
        NEWS_INTENSITY_DETERMINISTIC_INPUT_POLICY, NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
        NewsArrivalComponent, NewsArrivalConfiguration, NewsArrivalIntensityEvaluationInput,
        NewsArrivalIntensityEvaluationOutput, NewsArrivalIntensityGenerator,
        NewsBaseShockMarkSampler, NewsComponentMarkScaleConfiguration, NewsCoupledMarkConstructor,
        NewsIntensityConfiguration, NewsMarkConfigurationError, NewsMarkInterfaceError,
        NewsMarkInterfaceStage, NewsProcessConfiguration, NewsSeverityMarkSampler,
        SHARED_NEWS_EVENT_BOUNDARY_POLICY, SHARED_NEWS_EVENT_DISPATCH_GT_GATING_POLICY,
        SHARED_NEWS_EVENT_DISPATCH_JT_UPDATE_POLICY, SHARED_NEWS_EVENT_DISPATCH_MT_GATING_POLICY,
        SHARED_NEWS_EVENT_DISPATCH_ORDERING_POLICY, SHARED_NEWS_EVENT_MARKET_STATE_DEPENDENCY,
        SharedNewsAccumulatorState, SharedNewsEventDispatchInput, SharedNewsEventTimestampedInput,
        SharedNewsMarkLawConfiguration, dispatch_shared_news_events,
        dispatch_shared_news_events_with_resolved_market_states,
        resolve_shared_news_event_market_states,
    };
    use crate::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader, MarketState};
    use crate::randomness::{SimulationRandomnessStream, SplitMix64SimulationRandomnessStream};
    use chrono::{DateTime, Utc};
    use serde_json::json;

    const MOCK_COUPLED_MARK_POLICY: &str = "test.mock.coupled.mark.policy.v1";

    #[derive(Debug, Clone, Copy)]
    struct MockArrivalIntensityGenerator {
        generated_arrival_intensity: NewsArrivalIntensityEvaluationOutput,
    }

    impl NewsArrivalIntensityGenerator for MockArrivalIntensityGenerator {
        fn generate_arrival_intensity(
            &self,
            _intensity_input: NewsArrivalIntensityEvaluationInput,
        ) -> Result<NewsArrivalIntensityEvaluationOutput, NewsMarkInterfaceError> {
            Ok(self.generated_arrival_intensity)
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct MockSeverityMarkSampler {
        sampled_severity_mark: f64,
    }

    impl NewsSeverityMarkSampler for MockSeverityMarkSampler {
        fn sample_shared_severity_mark(
            &self,
            _randomness_stream: &mut dyn SimulationRandomnessStream,
        ) -> Result<f64, NewsMarkInterfaceError> {
            Ok(self.sampled_severity_mark)
        }
    }

    #[derive(Debug, Clone)]
    struct RejectingSeverityMarkSampler {
        rejection_reason: String,
    }

    impl NewsSeverityMarkSampler for RejectingSeverityMarkSampler {
        fn sample_shared_severity_mark(
            &self,
            _randomness_stream: &mut dyn SimulationRandomnessStream,
        ) -> Result<f64, NewsMarkInterfaceError> {
            Err(NewsMarkInterfaceError::component_rejected(
                NewsMarkInterfaceStage::SeverityMarkSampling,
                self.rejection_reason.clone(),
            ))
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct MockBaseShockMarkSampler {
        sampled_base_shock_mark: f64,
    }

    impl NewsBaseShockMarkSampler for MockBaseShockMarkSampler {
        fn sample_shared_base_shock_mark(
            &self,
            _randomness_stream: &mut dyn SimulationRandomnessStream,
        ) -> Result<f64, NewsMarkInterfaceError> {
            Ok(self.sampled_base_shock_mark)
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct MockCoupledMarkConstructor {
        severity_mark_override: Option<f64>,
        base_shock_mark_override: Option<f64>,
        gap_jump_mark_override: Option<f64>,
    }

    impl NewsCoupledMarkConstructor for MockCoupledMarkConstructor {
        fn construct_coupled_mark(
            &self,
            shared_severity_mark: f64,
            shared_base_shock_mark: f64,
        ) -> Result<CoupledGapMarkConstruction, NewsMarkInterfaceError> {
            let constructed_severity_mark =
                self.severity_mark_override.unwrap_or(shared_severity_mark);
            let constructed_base_shock_mark = self
                .base_shock_mark_override
                .unwrap_or(shared_base_shock_mark);
            let gap_jump_mark = self
                .gap_jump_mark_override
                .unwrap_or(constructed_severity_mark.sqrt() * constructed_base_shock_mark);

            Ok(CoupledGapMarkConstruction {
                shared_severity_mark: constructed_severity_mark,
                shared_base_shock_mark: constructed_base_shock_mark,
                sigma_g: 1.0,
                gap_jump_mark,
                construction_policy: MOCK_COUPLED_MARK_POLICY,
            })
        }
    }

    fn sample_arrival_configuration() -> NewsArrivalConfiguration {
        NewsArrivalConfiguration {
            known: super::KnownNewsArrivalConfiguration {
                scheduled_event_timestamps_utc_nanos: vec![
                    1_710_000_000_000_000_000,
                    1_710_000_000_000_000_500,
                    1_710_000_000_000_001_000,
                ],
            },
            surprise: super::SurpriseNewsArrivalConfiguration {
                baseline_intensity: 0.75,
            },
        }
    }

    fn sample_shared_mark_law_configuration() -> SharedNewsMarkLawConfiguration {
        SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration::new(1.8, 0.9),
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::new(
                0.35, 2.1, 3.0,
            ),
            coupled_gap_marks: CoupledGapMarkConfiguration::new(0.6),
        }
    }

    fn sample_process_configuration() -> NewsProcessConfiguration {
        NewsProcessConfiguration::try_new(
            sample_arrival_configuration(),
            sample_shared_mark_law_configuration(),
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 1.0,
                return_jump_scale: 1.0,
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 1.0,
                return_jump_scale: 1.0,
            },
        )
        .expect("valid process configuration should construct")
    }

    fn sample_arrival_intensity_input() -> NewsArrivalIntensityEvaluationInput {
        NewsArrivalIntensityEvaluationInput {
            interval_start_utc_nanos: 1_710_000_000_000_000_000,
            interval_end_utc_nanos: 1_710_000_000_000_001_000,
            surprise_deterministic_time_multiplier: 1.25,
        }
    }

    fn mock_arrival_intensity_output(
        known_component_intensity: f64,
        surprise_component_intensity: f64,
    ) -> NewsArrivalIntensityEvaluationOutput {
        let known_scheduled_arrival_count =
            if known_component_intensity.is_finite() && known_component_intensity >= 0.0 {
                known_component_intensity.floor() as u64
            } else {
                0
            };
        NewsArrivalIntensityEvaluationOutput {
            known_scheduled_arrival_count,
            known_component_intensity,
            surprise_component_intensity,
            total_intensity: known_component_intensity + surprise_component_intensity,
            intensity_policy: NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY,
        }
    }

    #[test]
    fn news_process_configuration_round_trip_preserves_payload() {
        let configuration = sample_process_configuration();

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
            "shared_mark_law": {
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
            },
            "known_component_mark_scales": {
                "activity_coupling_scale": 1.0,
                "return_jump_scale": 1.0
            },
            "surprise_component_mark_scales": {
                "activity_coupling_scale": 1.0,
                "return_jump_scale": 1.0
            }
        });

        let parse_result = serde_json::from_value::<NewsProcessConfiguration>(payload);
        let error_text = parse_result
            .expect_err("missing arrivals field must fail")
            .to_string();

        assert!(
            error_text.contains("missing field `arrivals`"),
            "unexpected error text: {error_text}"
        );
    }

    #[test]
    fn news_process_configuration_deserialization_reports_invalid_field_value() {
        let payload = json!({
            "schema_version": NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
            "arrivals": {
                "known": {
                    "scheduled_event_timestamps_utc_nanos": [1710000000000000000_i64]
                },
                "surprise": {
                    "baseline_intensity": -0.1
                }
            },
            "shared_mark_law": {
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
            },
            "known_component_mark_scales": {
                "activity_coupling_scale": 1.0,
                "return_jump_scale": 1.0
            },
            "surprise_component_mark_scales": {
                "activity_coupling_scale": 1.0,
                "return_jump_scale": 1.0
            }
        });

        let parse_result = serde_json::from_value::<NewsProcessConfiguration>(payload);
        let error_text = parse_result
            .expect_err("invalid baseline intensity must fail")
            .to_string();

        assert!(
            error_text.contains("surprise.baseline_intensity must be a finite non-negative value"),
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

    #[test]
    fn default_composable_pipeline_wires_gamma_and_double_exponential_components() {
        let configuration = sample_process_configuration();
        let pipeline: DefaultNewsMarkPipeline = configuration.default_mark_pipeline();
        let intensity_input = sample_arrival_intensity_input();

        let generated_arrival_intensity = pipeline
            .generate_arrival_intensity(intensity_input)
            .expect("default pipeline should evaluate arrival intensity");
        let expected_arrival_intensity = configuration
            .arrivals
            .evaluate_with_deterministic_time_multiplier(intensity_input)
            .expect("deterministic intensity inputs should evaluate");

        assert_eq!(generated_arrival_intensity, expected_arrival_intensity);
        assert_eq!(
            generated_arrival_intensity.intensity_policy,
            NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY
        );
        assert_eq!(
            COMPOSABLE_NEWS_MARK_INTERFACE_POLICY,
            "component_arrival_intensity_then_shared_mark_sampling_then_coupled_mark_construction.v1"
        );

        let mut pipeline_stream = SplitMix64SimulationRandomnessStream::from_seed(90_011);
        let sampled_payload = pipeline
            .sample_coupled_marks(&mut pipeline_stream)
            .expect("default pipeline should sample coupled marks");

        let mut expected_stream = SplitMix64SimulationRandomnessStream::from_seed(90_011);
        let expected_severity_mark = configuration
            .shared_mark_law
            .gamma_severity_marks
            .sample_severity_mark(&mut expected_stream)
            .expect("gamma severity sampler should succeed");
        let expected_base_shock_mark = configuration
            .shared_mark_law
            .double_exponential_base_shock
            .sample_base_shock_mark(&mut expected_stream)
            .expect("double-exponential base-shock sampler should succeed");
        let expected_construction = configuration
            .shared_mark_law
            .coupled_gap_marks
            .construct_gap_jump_mark(expected_severity_mark, expected_base_shock_mark)
            .expect("coupled gap-mark constructor should succeed");

        assert_eq!(sampled_payload.shared_severity_mark, expected_severity_mark);
        assert_eq!(
            sampled_payload.shared_base_shock_mark,
            expected_base_shock_mark
        );
        assert_eq!(
            sampled_payload.coupled_gap_mark_construction,
            expected_construction
        );
    }

    #[test]
    fn composable_pipeline_supports_pluggable_component_implementations() {
        let pipeline = ComposableNewsMarkPipeline::new(
            MockArrivalIntensityGenerator {
                generated_arrival_intensity: mock_arrival_intensity_output(1.0, 0.4),
            },
            MockSeverityMarkSampler {
                sampled_severity_mark: 2.25,
            },
            MockBaseShockMarkSampler {
                sampled_base_shock_mark: -0.8,
            },
            MockCoupledMarkConstructor {
                severity_mark_override: None,
                base_shock_mark_override: None,
                gap_jump_mark_override: Some(-1.2),
            },
        );
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(10);

        let generated_arrival_intensity = pipeline
            .generate_arrival_intensity(sample_arrival_intensity_input())
            .expect("mock arrival generator should produce intensity");
        let sampled_payload = pipeline
            .sample_coupled_marks(&mut randomness_stream)
            .expect("mock component wiring should generate coupled marks");

        assert!((generated_arrival_intensity.total_intensity - 1.4).abs() <= 1e-12);
        assert_eq!(sampled_payload.shared_severity_mark, 2.25);
        assert_eq!(sampled_payload.shared_base_shock_mark, -0.8);
        assert_eq!(
            sampled_payload.coupled_gap_mark_construction.gap_jump_mark,
            -1.2
        );
        assert_eq!(
            sampled_payload
                .coupled_gap_mark_construction
                .construction_policy,
            MOCK_COUPLED_MARK_POLICY
        );
    }

    #[test]
    fn composable_pipeline_contract_rejects_invalid_arrival_intensity_output() {
        let pipeline = ComposableNewsMarkPipeline::new(
            MockArrivalIntensityGenerator {
                generated_arrival_intensity: mock_arrival_intensity_output(f64::NAN, 0.1),
            },
            MockSeverityMarkSampler {
                sampled_severity_mark: 1.0,
            },
            MockBaseShockMarkSampler {
                sampled_base_shock_mark: 1.0,
            },
            MockCoupledMarkConstructor {
                severity_mark_override: None,
                base_shock_mark_override: None,
                gap_jump_mark_override: None,
            },
        );

        let error = pipeline
            .generate_arrival_intensity(sample_arrival_intensity_input())
            .expect_err("non-finite arrival intensity must fail contract validation");
        assert!(matches!(
            error,
            NewsMarkInterfaceError::InvalidGeneratedKnownArrivalIntensity { value }
            if value.is_nan()
        ));
    }

    #[test]
    fn composable_pipeline_contract_rejects_invalid_severity_and_base_shock_outputs() {
        let severity_violation_pipeline = ComposableNewsMarkPipeline::new(
            MockArrivalIntensityGenerator {
                generated_arrival_intensity: mock_arrival_intensity_output(0.7, 0.3),
            },
            MockSeverityMarkSampler {
                sampled_severity_mark: -0.01,
            },
            MockBaseShockMarkSampler {
                sampled_base_shock_mark: 1.0,
            },
            MockCoupledMarkConstructor {
                severity_mark_override: None,
                base_shock_mark_override: None,
                gap_jump_mark_override: None,
            },
        );

        let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(11);
        let severity_error = severity_violation_pipeline
            .sample_coupled_marks(&mut first_stream)
            .expect_err("negative severity mark must fail contract validation");

        assert!(matches!(
            severity_error,
            NewsMarkInterfaceError::InvalidSampledSeverityMark { value } if value == -0.01
        ));

        let base_shock_violation_pipeline = ComposableNewsMarkPipeline::new(
            MockArrivalIntensityGenerator {
                generated_arrival_intensity: mock_arrival_intensity_output(0.7, 0.3),
            },
            MockSeverityMarkSampler {
                sampled_severity_mark: 1.0,
            },
            MockBaseShockMarkSampler {
                sampled_base_shock_mark: f64::INFINITY,
            },
            MockCoupledMarkConstructor {
                severity_mark_override: None,
                base_shock_mark_override: None,
                gap_jump_mark_override: None,
            },
        );

        let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(12);
        let base_shock_error = base_shock_violation_pipeline
            .sample_coupled_marks(&mut second_stream)
            .expect_err("non-finite base-shock mark must fail contract validation");

        assert!(matches!(
            base_shock_error,
            NewsMarkInterfaceError::InvalidSampledBaseShockMark { value }
            if value.is_infinite()
        ));
    }

    #[test]
    fn composable_pipeline_contract_rejects_coupled_mark_output_violations() {
        let severity_mismatch_pipeline = ComposableNewsMarkPipeline::new(
            MockArrivalIntensityGenerator {
                generated_arrival_intensity: mock_arrival_intensity_output(0.7, 0.3),
            },
            MockSeverityMarkSampler {
                sampled_severity_mark: 2.0,
            },
            MockBaseShockMarkSampler {
                sampled_base_shock_mark: -0.5,
            },
            MockCoupledMarkConstructor {
                severity_mark_override: Some(3.0),
                base_shock_mark_override: None,
                gap_jump_mark_override: Some(-0.75),
            },
        );

        let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(13);
        let severity_mismatch_error = severity_mismatch_pipeline
            .sample_coupled_marks(&mut first_stream)
            .expect_err("severity mismatch must fail contract validation");

        assert!(matches!(
            severity_mismatch_error,
            NewsMarkInterfaceError::ConstructedSeverityMarkMismatch { expected, observed }
            if expected == 2.0 && observed == 3.0
        ));

        let non_finite_gap_pipeline = ComposableNewsMarkPipeline::new(
            MockArrivalIntensityGenerator {
                generated_arrival_intensity: mock_arrival_intensity_output(0.7, 0.3),
            },
            MockSeverityMarkSampler {
                sampled_severity_mark: 2.0,
            },
            MockBaseShockMarkSampler {
                sampled_base_shock_mark: -0.5,
            },
            MockCoupledMarkConstructor {
                severity_mark_override: None,
                base_shock_mark_override: None,
                gap_jump_mark_override: Some(f64::NAN),
            },
        );

        let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(14);
        let non_finite_gap_error = non_finite_gap_pipeline
            .sample_coupled_marks(&mut second_stream)
            .expect_err("non-finite coupled gap mark must fail contract validation");

        assert!(matches!(
            non_finite_gap_error,
            NewsMarkInterfaceError::InvalidConstructedGapMark { value } if value.is_nan()
        ));
    }

    #[test]
    fn composable_pipeline_propagates_component_rejections_from_pluggable_components() {
        let pipeline = ComposableNewsMarkPipeline::new(
            MockArrivalIntensityGenerator {
                generated_arrival_intensity: mock_arrival_intensity_output(0.7, 0.3),
            },
            RejectingSeverityMarkSampler {
                rejection_reason: "mock rejection for severity stage".to_string(),
            },
            MockBaseShockMarkSampler {
                sampled_base_shock_mark: 1.0,
            },
            MockCoupledMarkConstructor {
                severity_mark_override: None,
                base_shock_mark_override: None,
                gap_jump_mark_override: None,
            },
        );

        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(15);

        let error = pipeline
            .sample_coupled_marks(&mut randomness_stream)
            .expect_err("component rejection should propagate");

        assert!(matches!(
            error,
            NewsMarkInterfaceError::ComponentRejected {
                stage: NewsMarkInterfaceStage::SeverityMarkSampling,
                reason,
            } if reason == "mock rejection for severity stage"
        ));
    }

    #[test]
    fn gamma_severity_configuration_try_new_rejects_invalid_parameters() {
        let invalid_shape_error = GammaSeverityMarkConfiguration::try_new(0.0, 1.0)
            .expect_err("shape_k must be strictly positive");
        assert!(matches!(
            invalid_shape_error,
            NewsMarkConfigurationError::InvalidGammaShapeK { value } if value == 0.0
        ));

        let invalid_scale_error = GammaSeverityMarkConfiguration::try_new(1.0, -0.5)
            .expect_err("scale_theta must be strictly positive");
        assert!(matches!(
            invalid_scale_error,
            NewsMarkConfigurationError::InvalidGammaScaleTheta { value } if value == -0.5
        ));
    }

    #[test]
    fn gamma_severity_sampler_rejects_invalid_configuration_at_sampling_time() {
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(44_101);
        let invalid_configuration = GammaSeverityMarkConfiguration::new(-1.0, 0.8);

        let sampling_error = invalid_configuration
            .sample_severity_mark(&mut randomness_stream)
            .expect_err("invalid shape parameter must fail fast before sampling");

        assert!(matches!(
            sampling_error,
            GammaSeverityMarkSamplingError::InvalidConfiguration {
                source: NewsMarkConfigurationError::InvalidGammaShapeK { value },
            } if value == -1.0
        ));
    }

    #[test]
    fn gamma_severity_sampler_outputs_finite_non_negative_marks() {
        let configuration = GammaSeverityMarkConfiguration::try_new(2.25, 0.6)
            .expect("valid gamma severity configuration should construct");
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(55_101);

        let mut sampled_marks: Vec<f64> = Vec::with_capacity(2_048);
        for _sample_index in 0..2_048 {
            let sampled_mark = configuration
                .sample_severity_mark(&mut randomness_stream)
                .expect("sampling with valid parameters should succeed");
            sampled_marks.push(sampled_mark);
        }

        assert!(sampled_marks.iter().all(|sample| sample.is_finite()));
        assert!(sampled_marks.iter().all(|sample| *sample >= 0.0));
    }

    #[test]
    fn gamma_severity_sampler_is_seed_reproducible() {
        let configuration = GammaSeverityMarkConfiguration::try_new(1.7, 1.2)
            .expect("valid gamma severity configuration should construct");
        let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(77_201);
        let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(77_201);
        let mut different_seed_stream = SplitMix64SimulationRandomnessStream::from_seed(77_202);

        let first_draws: Vec<f64> = (0..64)
            .map(|_| {
                configuration
                    .sample_severity_mark(&mut first_stream)
                    .expect("first seeded stream should sample")
            })
            .collect();
        let second_draws: Vec<f64> = (0..64)
            .map(|_| {
                configuration
                    .sample_severity_mark(&mut second_stream)
                    .expect("second seeded stream should sample")
            })
            .collect();
        let different_seed_draws: Vec<f64> = (0..64)
            .map(|_| {
                configuration
                    .sample_severity_mark(&mut different_seed_stream)
                    .expect("different seeded stream should sample")
            })
            .collect();

        assert_eq!(first_draws, second_draws);
        assert_ne!(first_draws, different_seed_draws);
        assert_eq!(
            GAMMA_SEVERITY_MARK_SAMPLING_POLICY,
            "marsaglia_tsang_shape_scale_with_shape_less_than_one_boost.v1"
        );
    }

    #[test]
    fn gamma_severity_sampler_mean_tracks_shape_scale_expectation() {
        let shape_k = 2.4;
        let scale_theta = 0.75;
        let expected_mean = shape_k * scale_theta;
        let sample_count = 40_000;

        let configuration = GammaSeverityMarkConfiguration::try_new(shape_k, scale_theta)
            .expect("valid gamma severity configuration should construct");
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(91_502);

        let mut sample_sum = 0.0;
        for _sample_index in 0..sample_count {
            sample_sum += configuration
                .sample_severity_mark(&mut randomness_stream)
                .expect("sampling with valid parameters should succeed");
        }
        let sample_mean = sample_sum / sample_count as f64;

        let relative_error = (sample_mean - expected_mean).abs() / expected_mean;
        assert!(
            relative_error <= 0.03,
            "sample mean should stay close to expectation, expected_mean={expected_mean}, sample_mean={sample_mean}, relative_error={relative_error}"
        );
    }

    #[test]
    fn double_exponential_base_shock_try_new_rejects_invalid_parameters() {
        let invalid_probability_low =
            DoubleExponentialBaseShockConfiguration::try_new(0.0, 2.1, 3.0)
                .expect_err("probability_positive must be strictly between 0 and 1");
        assert!(matches!(
            invalid_probability_low,
            NewsMarkConfigurationError::InvalidPositiveJumpProbability { value } if value == 0.0
        ));

        let invalid_probability_high =
            DoubleExponentialBaseShockConfiguration::try_new(1.0, 2.1, 3.0)
                .expect_err("probability_positive must be strictly between 0 and 1");
        assert!(matches!(
            invalid_probability_high,
            NewsMarkConfigurationError::InvalidPositiveJumpProbability { value } if value == 1.0
        ));

        let invalid_eta_plus = DoubleExponentialBaseShockConfiguration::try_new(0.35, -0.2, 3.0)
            .expect_err("eta_plus must be strictly positive");
        assert!(matches!(
            invalid_eta_plus,
            NewsMarkConfigurationError::InvalidEtaPlus { value } if value == -0.2
        ));

        let invalid_eta_minus = DoubleExponentialBaseShockConfiguration::try_new(0.35, 2.1, 0.0)
            .expect_err("eta_minus must be strictly positive");
        assert!(matches!(
            invalid_eta_minus,
            NewsMarkConfigurationError::InvalidEtaMinus { value } if value == 0.0
        ));
    }

    #[test]
    fn double_exponential_base_shock_sampler_rejects_invalid_configuration_at_sampling_time() {
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(44_102);
        let invalid_configuration = DoubleExponentialBaseShockConfiguration::new(1.2, 2.1, 3.0);

        let sampling_error = invalid_configuration
            .sample_base_shock_mark(&mut randomness_stream)
            .expect_err("invalid p parameter must fail fast before sampling");

        assert!(matches!(
            sampling_error,
            DoubleExponentialBaseShockSamplingError::InvalidConfiguration {
                source: NewsMarkConfigurationError::InvalidPositiveJumpProbability { value },
            } if value == 1.2
        ));
    }

    #[test]
    fn double_exponential_base_shock_sampler_outputs_two_sided_tail_shape() {
        let probability_positive = 0.35;
        let eta_plus = 1.2;
        let eta_minus = 2.4;
        let sample_count = 80_000;
        let expected_positive_mean = 1.0 / eta_plus;
        let expected_negative_absolute_mean = 1.0 / eta_minus;

        let configuration = DoubleExponentialBaseShockConfiguration::try_new(
            probability_positive,
            eta_plus,
            eta_minus,
        )
        .expect("valid base-shock configuration should construct");
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(98_113);

        let mut positive_count = 0_usize;
        let mut negative_count = 0_usize;
        let mut positive_sum = 0.0;
        let mut negative_absolute_sum = 0.0;

        for _sample_index in 0..sample_count {
            let sampled_mark = configuration
                .sample_base_shock_mark(&mut randomness_stream)
                .expect("sampling with valid base-shock parameters should succeed");
            assert!(sampled_mark.is_finite());

            if sampled_mark >= 0.0 {
                positive_count += 1;
                positive_sum += sampled_mark;
            } else {
                negative_count += 1;
                negative_absolute_sum += -sampled_mark;
            }
        }

        assert!(positive_count > 0);
        assert!(negative_count > 0);

        let positive_frequency = positive_count as f64 / sample_count as f64;
        let positive_mean = positive_sum / positive_count as f64;
        let negative_absolute_mean = negative_absolute_sum / negative_count as f64;

        assert!(
            (positive_frequency - probability_positive).abs() <= 0.015,
            "positive-frequency sanity check failed, expected_probability={probability_positive}, observed_probability={positive_frequency}"
        );
        assert!(
            (positive_mean - expected_positive_mean).abs() / expected_positive_mean <= 0.06,
            "positive-tail mean sanity check failed, expected_mean={expected_positive_mean}, observed_mean={positive_mean}"
        );
        assert!(
            (negative_absolute_mean - expected_negative_absolute_mean).abs()
                / expected_negative_absolute_mean
                <= 0.06,
            "negative-tail mean sanity check failed, expected_mean={expected_negative_absolute_mean}, observed_mean={negative_absolute_mean}"
        );
        assert!(
            positive_mean > negative_absolute_mean,
            "tail-shape sanity check failed, expected positive tail to be heavier for eta_plus={eta_plus} and eta_minus={eta_minus}, observed positive_mean={positive_mean}, negative_absolute_mean={negative_absolute_mean}"
        );
        assert_eq!(
            DOUBLE_EXPONENTIAL_BASE_SHOCK_SAMPLING_POLICY,
            "kou_two_sided_exponential_with_constant_mixture_probability.v1"
        );
    }

    #[test]
    fn double_exponential_base_shock_sampler_is_seed_reproducible() {
        let configuration = DoubleExponentialBaseShockConfiguration::try_new(0.35, 1.8, 2.7)
            .expect("valid base-shock configuration should construct");

        let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(66_777);
        let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(66_777);
        let mut different_seed_stream = SplitMix64SimulationRandomnessStream::from_seed(66_778);

        let first_draws: Vec<f64> = (0..64)
            .map(|_| {
                configuration
                    .sample_base_shock_mark(&mut first_stream)
                    .expect("first seeded stream should sample")
            })
            .collect();
        let second_draws: Vec<f64> = (0..64)
            .map(|_| {
                configuration
                    .sample_base_shock_mark(&mut second_stream)
                    .expect("second seeded stream should sample")
            })
            .collect();
        let different_seed_draws: Vec<f64> = (0..64)
            .map(|_| {
                configuration
                    .sample_base_shock_mark(&mut different_seed_stream)
                    .expect("different seeded stream should sample")
            })
            .collect();

        assert_eq!(first_draws, second_draws);
        assert_ne!(first_draws, different_seed_draws);
    }

    #[test]
    fn coupled_gap_mark_constructor_rejects_invalid_sigma_configuration() {
        let construction_error = CoupledGapMarkConfiguration::new(-0.01)
            .construct_gap_jump_mark(1.25, -0.5)
            .expect_err("negative sigma_g must fail gap-mark construction");
        assert!(matches!(
            construction_error,
            CoupledGapMarkConstructionError::InvalidConfiguration {
                source: NewsMarkConfigurationError::InvalidSigmaG { value }
            } if value == -0.01
        ));

        let non_finite_configuration_error = CoupledGapMarkConfiguration::new(f64::INFINITY)
            .construct_gap_jump_mark(1.25, -0.5)
            .expect_err("non-finite sigma_g must fail gap-mark construction");
        assert!(matches!(
            non_finite_configuration_error,
            CoupledGapMarkConstructionError::InvalidConfiguration {
                source: NewsMarkConfigurationError::InvalidSigmaG { value }
            } if value.is_infinite()
        ));
    }

    #[test]
    fn coupled_gap_mark_constructor_deterministically_scales_shared_marks() {
        let construction = CoupledGapMarkConfiguration::new(0.3)
            .construct_gap_jump_mark(4.0, -1.25)
            .expect("valid coupled-mark payload must construct");

        assert!((construction.gap_jump_mark - (-0.75)).abs() <= 1e-12);
        assert_eq!(construction.shared_severity_mark, 4.0);
        assert_eq!(construction.shared_base_shock_mark, -1.25);
        assert_eq!(construction.sigma_g, 0.3);
        assert_eq!(
            construction.construction_policy,
            COUPLED_GAP_MARK_CONSTRUCTION_POLICY
        );
    }

    #[test]
    fn coupled_gap_mark_constructor_preserves_shared_severity_for_hawkes_and_gap_consumers() {
        let shared_severity_mark = 2.25;
        let shared_base_shock_mark = 0.8;
        let sigma_g = 0.4;

        let construction = CoupledGapMarkConfiguration::new(sigma_g)
            .construct_gap_jump_mark(shared_severity_mark, shared_base_shock_mark)
            .expect("valid shared marks must construct");

        let hawkes_consumer_severity_mark = construction.shared_severity_mark;
        let replayed_gap_jump_mark = construction.sigma_g
            * hawkes_consumer_severity_mark.sqrt()
            * construction.shared_base_shock_mark;

        assert_eq!(hawkes_consumer_severity_mark, shared_severity_mark);
        assert!(
            (construction.gap_jump_mark - replayed_gap_jump_mark).abs() <= 1e-12,
            "gap consumer must use the same shared severity mark used by Hawkes loading"
        );
    }

    #[test]
    fn shared_news_event_dispatch_maps_each_news_event_to_one_jt_increment() {
        let event_inputs = vec![
            SharedNewsEventDispatchInput {
                event_timestamp_utc_nanos: 300,
                event_generation_order: 2,
                news_arrival_component: NewsArrivalComponent::Surprise,
                market_state: MarketState::Open,
                shared_severity_mark: 1.25,
                coupled_return_jump_mark: -0.3,
            },
            SharedNewsEventDispatchInput {
                event_timestamp_utc_nanos: 100,
                event_generation_order: 0,
                news_arrival_component: NewsArrivalComponent::Known,
                market_state: MarketState::Closed,
                shared_severity_mark: 0.4,
                coupled_return_jump_mark: -0.2,
            },
            SharedNewsEventDispatchInput {
                event_timestamp_utc_nanos: 200,
                event_generation_order: 1,
                news_arrival_component: NewsArrivalComponent::Surprise,
                market_state: MarketState::Closed,
                shared_severity_mark: 0.8,
                coupled_return_jump_mark: 0.1,
            },
        ];

        let dispatch_output = dispatch_shared_news_events(
            SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
            &event_inputs,
        )
        .expect("valid shared dispatch inputs should dispatch");

        assert_eq!(dispatch_output.records.len(), event_inputs.len());

        let mut expected_pairs: Vec<(u64, f64)> = event_inputs
            .iter()
            .map(|event_input| {
                (
                    event_input.event_generation_order,
                    event_input.shared_severity_mark,
                )
            })
            .collect();
        expected_pairs.sort_by_key(|pair| pair.0);

        let observed_pairs: Vec<(u64, f64)> = dispatch_output
            .records
            .iter()
            .map(|record| (record.event_generation_order, record.j_t_increment))
            .collect();
        assert_eq!(observed_pairs, expected_pairs);

        let mut running_j_t = 0.0;
        for record in &dispatch_output.records {
            running_j_t += record.shared_severity_mark;
            assert_eq!(record.j_t_increment, record.shared_severity_mark);
            assert!((record.j_t_after_event - running_j_t).abs() <= 1e-12);
        }
    }

    #[test]
    fn shared_news_event_dispatch_couples_jt_with_closed_window_gt_updates() {
        let event_inputs = vec![
            SharedNewsEventDispatchInput {
                event_timestamp_utc_nanos: 10,
                event_generation_order: 0,
                news_arrival_component: NewsArrivalComponent::Known,
                market_state: MarketState::Open,
                shared_severity_mark: 0.6,
                coupled_return_jump_mark: 1.1,
            },
            SharedNewsEventDispatchInput {
                event_timestamp_utc_nanos: 20,
                event_generation_order: 1,
                news_arrival_component: NewsArrivalComponent::Surprise,
                market_state: MarketState::Closed,
                shared_severity_mark: 0.9,
                coupled_return_jump_mark: -0.35,
            },
        ];

        let initial_state = SharedNewsAccumulatorState::new(1.2, 0.15, 0.8);
        let dispatch_output = dispatch_shared_news_events(initial_state, &event_inputs)
            .expect("valid shared dispatch inputs should dispatch");

        assert_eq!(
            dispatch_output.records[0].m_t_increment, 1.1,
            "open-window event should emit direct jump into M_t"
        );
        assert_eq!(
            dispatch_output.records[0].g_t_increment, 0.0,
            "open-window event must not emit a direct gap increment"
        );
        assert_eq!(
            dispatch_output.records[1].m_t_increment, 0.0,
            "closed-window event must not emit direct jump into M_t"
        );
        assert_eq!(
            dispatch_output.records[1].g_t_increment, -0.35,
            "closed-window event should emit the coupled gap increment"
        );

        assert!((dispatch_output.final_state.j_t - (1.2 + 0.6 + 0.9)).abs() <= 1e-12);
        assert!((dispatch_output.final_state.m_t - (0.15 + 1.1)).abs() <= 1e-12);
        assert!((dispatch_output.final_state.g_t - (0.8 - 0.35)).abs() <= 1e-12);
    }

    #[test]
    fn shared_news_event_dispatch_is_deterministic_under_seeded_tie_ordering_inputs() {
        let first_seeded_inputs = build_seeded_dispatch_inputs(8_110_222, 32);
        let mut reversed_inputs = first_seeded_inputs.clone();
        reversed_inputs.reverse();

        let first_output = dispatch_shared_news_events(
            SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
            &first_seeded_inputs,
        )
        .expect("first seeded dispatch should succeed");
        let second_output = dispatch_shared_news_events(
            SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
            &reversed_inputs,
        )
        .expect("second seeded dispatch should succeed");

        assert_eq!(first_output, second_output);
        assert_eq!(
            first_output.ordering_policy,
            SHARED_NEWS_EVENT_DISPATCH_ORDERING_POLICY
        );
        assert_eq!(
            first_output.j_t_update_policy,
            SHARED_NEWS_EVENT_DISPATCH_JT_UPDATE_POLICY
        );
        assert_eq!(
            first_output.g_t_gating_policy,
            SHARED_NEWS_EVENT_DISPATCH_GT_GATING_POLICY
        );
        assert_eq!(
            first_output.m_t_gating_policy,
            SHARED_NEWS_EVENT_DISPATCH_MT_GATING_POLICY
        );
        assert_eq!(
            first_output.market_state_dependency,
            SHARED_NEWS_EVENT_MARKET_STATE_DEPENDENCY
        );
        assert_eq!(
            first_output.market_state_boundary_policy,
            SHARED_NEWS_EVENT_BOUNDARY_POLICY
        );

        assert!(first_output.records.windows(2).all(|record_pair| {
            let first_record = &record_pair[0];
            let second_record = &record_pair[1];
            (
                first_record.event_timestamp_utc_nanos,
                first_record.event_generation_order,
            ) <= (
                second_record.event_timestamp_utc_nanos,
                second_record.event_generation_order,
            )
        }));
    }

    #[test]
    fn shared_news_event_market_state_resolution_uses_canonical_service_outputs() {
        let registry = build_news_dispatch_calendar_registry();
        let event_inputs = vec![
            SharedNewsEventTimestampedInput {
                event_timestamp_utc_nanos: timestamp_utc_nanos("2026-02-02T14:30:00Z"),
                event_generation_order: 0,
                news_arrival_component: NewsArrivalComponent::Known,
                shared_severity_mark: 0.5,
                coupled_return_jump_mark: 0.2,
            },
            SharedNewsEventTimestampedInput {
                event_timestamp_utc_nanos: timestamp_utc_nanos("2026-02-02T21:00:00Z"),
                event_generation_order: 1,
                news_arrival_component: NewsArrivalComponent::Surprise,
                shared_severity_mark: 0.7,
                coupled_return_jump_mark: -0.3,
            },
        ];

        let resolved_event_inputs =
            resolve_shared_news_event_market_states(&registry, "XNYS", &event_inputs)
                .expect("market states should resolve from calendar service");

        assert_eq!(resolved_event_inputs.len(), 2);
        assert_eq!(resolved_event_inputs[0].market_state, MarketState::Open);
        assert_eq!(resolved_event_inputs[1].market_state, MarketState::Closed);
    }

    #[test]
    fn shared_news_event_dispatch_with_resolved_market_states_applies_open_close_boundaries() {
        let registry = build_news_dispatch_calendar_registry();
        let event_inputs = vec![
            SharedNewsEventTimestampedInput {
                event_timestamp_utc_nanos: timestamp_utc_nanos("2026-02-02T14:30:00Z"),
                event_generation_order: 0,
                news_arrival_component: NewsArrivalComponent::Known,
                shared_severity_mark: 0.6,
                coupled_return_jump_mark: 1.1,
            },
            SharedNewsEventTimestampedInput {
                event_timestamp_utc_nanos: timestamp_utc_nanos("2026-02-02T21:00:00Z"),
                event_generation_order: 1,
                news_arrival_component: NewsArrivalComponent::Surprise,
                shared_severity_mark: 0.9,
                coupled_return_jump_mark: -0.35,
            },
        ];

        let dispatch_output = dispatch_shared_news_events_with_resolved_market_states(
            &registry,
            "XNYS",
            SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
            &event_inputs,
        )
        .expect("dispatch should resolve market states and apply canonical gating");

        assert_eq!(dispatch_output.records.len(), 2);
        assert_eq!(dispatch_output.records[0].market_state, MarketState::Open);
        assert_eq!(dispatch_output.records[0].m_t_increment, 1.1);
        assert_eq!(dispatch_output.records[0].g_t_increment, 0.0);
        assert_eq!(dispatch_output.records[1].market_state, MarketState::Closed);
        assert_eq!(dispatch_output.records[1].m_t_increment, 0.0);
        assert_eq!(dispatch_output.records[1].g_t_increment, -0.35);
        assert!((dispatch_output.final_state.j_t - 1.5).abs() <= 1e-12);
        assert!((dispatch_output.final_state.m_t - 1.1).abs() <= 1e-12);
        assert!((dispatch_output.final_state.g_t - (-0.35)).abs() <= 1e-12);
        assert_eq!(
            dispatch_output.market_state_dependency,
            SHARED_NEWS_EVENT_MARKET_STATE_DEPENDENCY
        );
        assert_eq!(
            dispatch_output.market_state_boundary_policy,
            SHARED_NEWS_EVENT_BOUNDARY_POLICY
        );
    }

    fn build_seeded_dispatch_inputs(
        seed: u64,
        event_count: usize,
    ) -> Vec<SharedNewsEventDispatchInput> {
        let gamma_configuration = GammaSeverityMarkConfiguration::try_new(1.6, 0.9)
            .expect("gamma configuration should be valid");
        let base_shock_configuration =
            DoubleExponentialBaseShockConfiguration::try_new(0.4, 1.7, 2.5)
                .expect("base-shock configuration should be valid");
        let coupled_gap_configuration = CoupledGapMarkConfiguration::new(0.35);
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(seed);

        let mut event_inputs: Vec<SharedNewsEventDispatchInput> = Vec::with_capacity(event_count);
        for event_position in 0..event_count {
            let timestamp_bucket =
                (randomness_stream.next_unit_interval_f64() * 4.0).floor() as i64;
            let event_timestamp_utc_nanos = 1_700_000_000_000_000_000_i64 + timestamp_bucket;
            let market_state = if randomness_stream.next_unit_interval_f64() < 0.5 {
                MarketState::Closed
            } else {
                MarketState::Open
            };

            let shared_severity_mark = gamma_configuration
                .sample_severity_mark(&mut randomness_stream)
                .expect("severity mark should sample from valid configuration");
            let shared_base_shock_mark = base_shock_configuration
                .sample_base_shock_mark(&mut randomness_stream)
                .expect("base-shock mark should sample from valid configuration");
            let coupled_gap_jump_mark = coupled_gap_configuration
                .construct_gap_jump_mark(shared_severity_mark, shared_base_shock_mark)
                .expect("coupled gap mark should construct from sampled marks")
                .gap_jump_mark;

            event_inputs.push(SharedNewsEventDispatchInput {
                event_timestamp_utc_nanos,
                event_generation_order: (event_count - event_position - 1) as u64,
                news_arrival_component: if event_position % 2 == 0 {
                    NewsArrivalComponent::Known
                } else {
                    NewsArrivalComponent::Surprise
                },
                market_state,
                shared_severity_mark,
                coupled_return_jump_mark: coupled_gap_jump_mark,
            });
        }

        event_inputs
    }

    fn timestamp_utc_nanos(raw_timestamp_utc: &str) -> i64 {
        parse_timestamp(raw_timestamp_utc)
            .timestamp_nanos_opt()
            .expect("timestamp fixture must be in nanosecond range")
    }

    fn parse_timestamp(raw_timestamp_utc: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw_timestamp_utc)
            .expect("fixture timestamp must be RFC3339")
            .with_timezone(&Utc)
    }

    fn build_news_dispatch_calendar_registry() -> ExchangeCalendarRegistry {
        ExchangeCalendarRegistryLoader::load_from_json(
            r#"
            {
              "schema_version": "calendar.registry.v1",
              "artifact_date": "2026-01-31",
              "exchanges": [
                {
                  "schema_version": "calendar.v1",
                  "exchange_code": "XNYS",
                  "exchange_name": "New York Stock Exchange",
                  "timezone_name": "America/New_York",
                  "rule_metadata": {
                    "holiday_calendar_code": "US_EQUITIES",
                    "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
                  },
                  "regular_sessions": [
                    {
                      "weekday": "Monday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Tuesday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Wednesday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Thursday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Friday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    }
                  ],
                  "holiday_dates": [],
                  "early_close_sessions": []
                }
              ]
            }
            "#,
        )
        .expect("calendar registry fixture should load")
    }
}
