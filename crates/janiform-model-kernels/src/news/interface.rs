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

use super::configuration::GammaNormalNewsMarkConstruction;
use super::configuration::NewsComponentMarkScaleConfiguration;

pub const NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION: &str = "news.process.configuration.v1";
pub const NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY: &str = "known_schedule_count_plus_market_state_split_surprise_baseline_times_deterministic_multiplier.v1";
pub const GAMMA_SEVERITY_MARK_SAMPLING_POLICY: &str =
    "unit_mean_gamma_shape_with_derived_scale_marsaglia_tsang_shape_boost.v1";
pub const STANDARD_NORMAL_NEWS_RETURN_SHOCK_SAMPLING_POLICY: &str =
    "box_muller_standard_normal_news_return_shock.v1";
pub const GAMMA_NORMAL_NEWS_MARK_CONSTRUCTION_POLICY: &str =
    "component_activity_scale_times_gamma_severity_and_gamma_normal_return_jump.v1";
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
pub const COMPOSABLE_NEWS_MARK_INTERFACE_POLICY: &str = "component_arrival_intensity_then_gamma_severity_and_normal_mark_sampling_then_coupled_gamma_normal_construction.v1";

/// Explicit component stages in the composable news-mark generation lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsMarkInterfaceStage {
    ArrivalIntensityGeneration,
    SeverityMarkSampling,
    ReturnShockMarkSampling,
    GammaNormalMarkConstruction,
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
    InvalidSampledReturnShockMark {
        value: f64,
    },
    InvalidConstructedActivityShockMark {
        value: f64,
    },
    InvalidConstructedReturnJumpMark {
        value: f64,
    },
    ConstructedGammaNormalSeverityMarkMismatch {
        expected: f64,
        observed: f64,
    },
    ConstructedGammaNormalReturnShockMarkMismatch {
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
            Self::InvalidSampledReturnShockMark { value } => write!(
                formatter,
                "composable news return-shock sample must be finite, found {value}"
            ),
            Self::InvalidConstructedActivityShockMark { value } => write!(
                formatter,
                "composable news activity-shock mark must be finite and non-negative, found {value}"
            ),
            Self::InvalidConstructedReturnJumpMark { value } => write!(
                formatter,
                "composable news return-jump mark must be finite, found {value}"
            ),
            Self::ConstructedGammaNormalSeverityMarkMismatch { expected, observed } => write!(
                formatter,
                "composable gamma-normal mark constructor must preserve the shared severity mark, expected {expected} and observed {observed}"
            ),
            Self::ConstructedGammaNormalReturnShockMarkMismatch { expected, observed } => write!(
                formatter,
                "composable gamma-normal mark constructor must preserve the shared return-shock mark, expected {expected} and observed {observed}"
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
    pub surprise_open_window_share: f64,
    pub surprise_closed_window_share: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewsArrivalIntensityEvaluationOutput {
    pub known_scheduled_arrival_count: u64,
    pub known_component_intensity: f64,
    pub surprise_open_component_intensity: f64,
    pub surprise_closed_component_intensity: f64,
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

/// Interface boundary for shared standard-normal return-shock sampling.
pub trait NewsReturnShockMarkSampler {
    fn sample_shared_return_shock_mark(
        &self,
        randomness_stream: &mut dyn SimulationRandomnessStream,
    ) -> Result<f64, NewsMarkInterfaceError>;
}

/// Fully composed mark payload for one exogenous news arrival.
///
/// Ownership and lifecycle:
/// - This payload owns plain values only and does not borrow from any generator component.
/// - Callers may persist it for replay or diagnostics independent from generator lifetime.
/// - `shared_severity_mark` is the same value used by Hawkes loading and return-jump construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposableNewsMarkPayload {
    pub shared_severity_mark: f64,
    pub shared_return_shock_mark: f64,
    pub gamma_normal_mark_construction: GammaNormalNewsMarkConstruction,
}

/// Minimal composable pipeline for arrival intensity and coupled news-mark generation.
///
/// This struct owns component implementations and orchestrates one deterministic flow:
/// 1. Arrival intensity generation
/// 2. Shared severity-mark sampling
/// 3. Shared standard-normal return-shock sampling
/// 4. Coupled Gamma-normal mark construction
///
/// The pipeline enforces output invariants even for alternative component implementations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposableNewsMarkPipeline<
    ArrivalIntensityGenerator,
    SeverityMarkSampler,
    ReturnShockMarkSampler,
> {
    arrival_intensity_generator: ArrivalIntensityGenerator,
    severity_mark_sampler: SeverityMarkSampler,
    return_shock_mark_sampler: ReturnShockMarkSampler,
}

impl<ArrivalIntensityGenerator, SeverityMarkSampler, ReturnShockMarkSampler>
    ComposableNewsMarkPipeline<
        ArrivalIntensityGenerator,
        SeverityMarkSampler,
        ReturnShockMarkSampler,
    >
where
    ArrivalIntensityGenerator: NewsArrivalIntensityGenerator,
    SeverityMarkSampler: NewsSeverityMarkSampler,
    ReturnShockMarkSampler: NewsReturnShockMarkSampler,
{
    pub const fn new(
        arrival_intensity_generator: ArrivalIntensityGenerator,
        severity_mark_sampler: SeverityMarkSampler,
        return_shock_mark_sampler: ReturnShockMarkSampler,
    ) -> Self {
        Self {
            arrival_intensity_generator,
            severity_mark_sampler,
            return_shock_mark_sampler,
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

    pub fn sample_gamma_normal_marks(
        &self,
        component_mark_scales: NewsComponentMarkScaleConfiguration,
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

        let shared_return_shock_mark = self
            .return_shock_mark_sampler
            .sample_shared_return_shock_mark(randomness_stream)?;
        if !shared_return_shock_mark.is_finite() {
            return Err(NewsMarkInterfaceError::InvalidSampledReturnShockMark {
                value: shared_return_shock_mark,
            });
        }

        let gamma_normal_mark_construction = component_mark_scales
            .construct_gamma_normal_mark(shared_severity_mark, shared_return_shock_mark)
            .map_err(|source| {
                NewsMarkInterfaceError::component_rejected(
                    NewsMarkInterfaceStage::GammaNormalMarkConstruction,
                    source.to_string(),
                )
            })?;

        if gamma_normal_mark_construction
            .shared_severity_mark
            .to_bits()
            != shared_severity_mark.to_bits()
        {
            return Err(
                NewsMarkInterfaceError::ConstructedGammaNormalSeverityMarkMismatch {
                    expected: shared_severity_mark,
                    observed: gamma_normal_mark_construction.shared_severity_mark,
                },
            );
        }

        if gamma_normal_mark_construction
            .shared_return_shock_mark
            .to_bits()
            != shared_return_shock_mark.to_bits()
        {
            return Err(
                NewsMarkInterfaceError::ConstructedGammaNormalReturnShockMarkMismatch {
                    expected: shared_return_shock_mark,
                    observed: gamma_normal_mark_construction.shared_return_shock_mark,
                },
            );
        }

        if !gamma_normal_mark_construction
            .activity_shock_mark
            .is_finite()
            || gamma_normal_mark_construction.activity_shock_mark < 0.0
        {
            return Err(
                NewsMarkInterfaceError::InvalidConstructedActivityShockMark {
                    value: gamma_normal_mark_construction.activity_shock_mark,
                },
            );
        }

        if !gamma_normal_mark_construction.return_jump_mark.is_finite() {
            return Err(NewsMarkInterfaceError::InvalidConstructedReturnJumpMark {
                value: gamma_normal_mark_construction.return_jump_mark,
            });
        }

        Ok(ComposableNewsMarkPayload {
            shared_severity_mark,
            shared_return_shock_mark,
            gamma_normal_mark_construction,
        })
    }
}
