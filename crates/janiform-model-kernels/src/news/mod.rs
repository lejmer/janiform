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
    CoupledGapMarkConfiguration, CoupledGapMarkConstruction,
    DoubleExponentialBaseShockConfiguration, GammaSeverityMarkConfiguration,
    KnownNewsArrivalConfiguration, NewsArrivalConfiguration, NewsComponentMarkScaleConfiguration,
    NewsIntensityConfiguration, SharedNewsMarkLawConfiguration, SurpriseNewsArrivalConfiguration,
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
    CoupledGapMarkConstructionError, DoubleExponentialBaseShockSamplingError,
    GammaSeverityMarkSamplingError, NewsArrivalConfigurationError,
    NewsComponentIntensityEvaluationError, NewsComponentMarkScaleConfigurationError,
    NewsDeterministicIntensityEvaluationError, NewsIntensityError, NewsMarkConfigurationError,
    NewsProcessConfigurationError,
};
pub use interface::{
    COMPOSABLE_NEWS_MARK_INTERFACE_POLICY, COUPLED_GAP_MARK_CONSTRUCTION_POLICY,
    ComposableNewsMarkPayload, ComposableNewsMarkPipeline,
    DOUBLE_EXPONENTIAL_BASE_SHOCK_SAMPLING_POLICY, GAMMA_SEVERITY_MARK_SAMPLING_POLICY,
    NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY, NEWS_INTENSITY_BASELINE_POLICY,
    NEWS_INTENSITY_DETERMINISTIC_INPUT_POLICY, NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
    NewsArrivalComponent, NewsArrivalIntensityEvaluationInput,
    NewsArrivalIntensityEvaluationOutput, NewsArrivalIntensityGenerator, NewsBaseShockMarkSampler,
    NewsCoupledMarkConstructor, NewsMarkInterfaceError, NewsMarkInterfaceStage,
    NewsReturnJumpRoutingTarget, NewsSeverityMarkSampler, SHARED_NEWS_EVENT_BOUNDARY_POLICY,
    SHARED_NEWS_EVENT_DISPATCH_GT_GATING_POLICY, SHARED_NEWS_EVENT_DISPATCH_JT_UPDATE_POLICY,
    SHARED_NEWS_EVENT_DISPATCH_MT_GATING_POLICY, SHARED_NEWS_EVENT_DISPATCH_ORDERING_POLICY,
    SHARED_NEWS_EVENT_DISPATCH_RETURN_ROUTING_POLICY, SHARED_NEWS_EVENT_MARKET_STATE_DEPENDENCY,
};
pub use process::{DefaultNewsMarkPipeline, NewsProcessConfiguration};

#[cfg(test)]
mod tests;
