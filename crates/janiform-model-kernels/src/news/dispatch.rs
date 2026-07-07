use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use janiform_foundations::market_state::MarketState;
use janiform_foundations::time::utc_datetime_from_nanos;

use super::interface::{
    NewsArrivalComponent, NewsReturnJumpRoutingTarget, SHARED_NEWS_EVENT_BOUNDARY_POLICY,
    SHARED_NEWS_EVENT_DISPATCH_GT_GATING_POLICY, SHARED_NEWS_EVENT_DISPATCH_JT_UPDATE_POLICY,
    SHARED_NEWS_EVENT_DISPATCH_MT_GATING_POLICY, SHARED_NEWS_EVENT_DISPATCH_ORDERING_POLICY,
    SHARED_NEWS_EVENT_DISPATCH_RETURN_ROUTING_POLICY, SHARED_NEWS_EVENT_MARKET_STATE_DEPENDENCY,
};

/// Shared contract for resolving one event timestamp into a market state.
pub trait SharedNewsEventMarketStateResolver {
    fn resolve_market_state_at_utc(
        &self,
        exchange_code: &str,
        event_timestamp_utc: DateTime<Utc>,
    ) -> Result<MarketState, SharedNewsEventMarketStateResolutionError>;
}

/// Stable resolver failure contract used by low-level news dispatch helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedNewsEventMarketStateResolutionError {
    pub reason: String,
}

impl Display for SharedNewsEventMarketStateResolutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl Error for SharedNewsEventMarketStateResolutionError {}

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
    market_state_resolver: &impl SharedNewsEventMarketStateResolver,
    exchange_code: &str,
    event_inputs: &[SharedNewsEventTimestampedInput],
) -> Result<Vec<SharedNewsEventDispatchInput>, SharedNewsEventDispatchError> {
    let mut resolved_event_inputs: Vec<SharedNewsEventDispatchInput> =
        Vec::with_capacity(event_inputs.len());

    for event_input in event_inputs {
        let event_timestamp_utc = utc_datetime_from_nanos(event_input.event_timestamp_utc_nanos)
            .ok_or(
                SharedNewsEventDispatchError::InvalidEventTimestampUtcNanos {
                    event_generation_order: event_input.event_generation_order,
                    value: event_input.event_timestamp_utc_nanos,
                },
            )?;
        let market_state = market_state_resolver
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
        source: SharedNewsEventMarketStateResolutionError,
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
    market_state_resolver: &impl SharedNewsEventMarketStateResolver,
    exchange_code: &str,
    initial_state: SharedNewsAccumulatorState,
    event_inputs: &[SharedNewsEventTimestampedInput],
) -> Result<SharedNewsEventDispatchOutput, SharedNewsEventDispatchError> {
    let resolved_event_inputs = resolve_shared_news_event_market_states(
        market_state_resolver,
        exchange_code,
        event_inputs,
    )?;
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
