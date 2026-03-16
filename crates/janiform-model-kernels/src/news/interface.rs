//! Shared exogenous news process configuration and deterministic intensity model.
//!
//! This module defines one typed configuration payload that is reusable by
//! simulation, pricing, and calibration artifact persistence.

use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::market_state::{
    MARKET_STATE_RESOLUTION_POLICY, SESSION_WINDOW_BOUNDARY_POLICY,
};
use janiform_foundations::randomness::SimulationRandomnessStream;
use serde::Deserialize;
use serde::Serialize;

use super::configuration::CoupledGapMarkConstruction;

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

pub(crate) const ONE_SECOND_NANOS: i64 = 1_000_000_000;

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
    pub(crate) fn component_rejected(
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
