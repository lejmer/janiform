//! Shared exogenous news process configuration and deterministic intensity model.
//!
//! This module defines one typed configuration payload that is reusable by
//! simulation, pricing, and calibration artifact persistence.

mod configuration;
mod dispatch;
mod errors;
mod interface;
mod process;
mod sampling;

pub use configuration::{
    GammaNormalNewsMarkConstruction, GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration, NewsComponentMarkScaleConfiguration, SharedNewsMarkLawConfiguration,
    StandardNormalNewsReturnShockSampler, SurpriseNewsArrivalConfiguration,
    expected_news_activity_loading_rate,
};
pub use dispatch::{
    SharedNewsAccumulatorState, SharedNewsEventDispatchError, SharedNewsEventDispatchInput,
    SharedNewsEventDispatchOutput, SharedNewsEventDispatchRecord,
    SharedNewsEventMarketStateResolutionError, SharedNewsEventMarketStateResolver,
    SharedNewsEventTimestampedInput, dispatch_shared_news_events,
    dispatch_shared_news_events_with_resolved_market_states,
    resolve_shared_news_event_market_states,
};
pub use errors::{
    GammaNormalNewsMarkConstructionError, GammaSeverityMarkSamplingError,
    NewsActivityLoadingRateError, NewsArrivalConfigurationError,
    NewsComponentIntensityEvaluationError, NewsComponentMarkScaleConfigurationError,
    NewsMarkConfigurationError, NewsProcessConfigurationError,
};
pub use interface::{
    COMPOSABLE_NEWS_MARK_INTERFACE_POLICY, ComposableNewsMarkPayload, ComposableNewsMarkPipeline,
    GAMMA_NORMAL_NEWS_MARK_CONSTRUCTION_POLICY, GAMMA_SEVERITY_MARK_SAMPLING_POLICY,
    NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY, NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
    NewsArrivalComponent, NewsArrivalIntensityEvaluationInput,
    NewsArrivalIntensityEvaluationOutput, NewsArrivalIntensityGenerator, NewsMarkInterfaceError,
    NewsMarkInterfaceStage, NewsReturnJumpRoutingTarget, NewsReturnShockMarkSampler,
    NewsSeverityMarkSampler, SHARED_NEWS_EVENT_BOUNDARY_POLICY,
    SHARED_NEWS_EVENT_DISPATCH_GT_GATING_POLICY, SHARED_NEWS_EVENT_DISPATCH_JT_UPDATE_POLICY,
    SHARED_NEWS_EVENT_DISPATCH_MT_GATING_POLICY, SHARED_NEWS_EVENT_DISPATCH_ORDERING_POLICY,
    SHARED_NEWS_EVENT_DISPATCH_RETURN_ROUTING_POLICY, SHARED_NEWS_EVENT_MARKET_STATE_DEPENDENCY,
    STANDARD_NORMAL_NEWS_RETURN_SHOCK_SAMPLING_POLICY,
};
pub use process::{DefaultNewsMarkPipeline, NewsProcessConfiguration};

#[cfg(test)]
mod tests;
