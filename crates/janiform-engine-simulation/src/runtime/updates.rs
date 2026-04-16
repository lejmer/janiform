use std::cmp::Ordering;

use janiform_model_kernels::hawkes::{HawkesEventSide, HawkesTransitionStep};
use janiform_model_kernels::news::{NewsArrivalComponent, NewsReturnJumpRoutingTarget};

use crate::artifacts::{
    SeededFinePathRecord, SeededFinePathTransitionKind, SeededTickEventDebugTraceTransitionKind,
    SeededTickEventSide, SeededTickSimulationNewsEventMarketState,
};
use crate::error::SeededTickSimulationRunError;
use janiform_market_time::calendar::MarketState;

use super::buffers::try_reserve_simulation_buffer;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum IntervalStochasticUpdate {
    EndogenousEvent {
        timestamp_utc_nanos: i64,
        side: SeededTickEventSide,
        generation_order: u64,
    },
    ExogenousNews {
        timestamp_utc_nanos: i64,
        news_arrival_component: NewsArrivalComponent,
        activity_coupling_mark: f64,
        return_jump_mark: f64,
        market_state: MarketState,
        return_jump_routing_target: NewsReturnJumpRoutingTarget,
        open_window_return_jump_increment: f64,
        closed_window_gap_increment: f64,
        generation_order: u64,
    },
}

impl IntervalStochasticUpdate {
    pub(crate) const fn timestamp_utc_nanos(&self) -> i64 {
        match *self {
            Self::EndogenousEvent {
                timestamp_utc_nanos,
                ..
            } => timestamp_utc_nanos,
            Self::ExogenousNews {
                timestamp_utc_nanos,
                ..
            } => timestamp_utc_nanos,
        }
    }

    pub(crate) const fn ordering_key(&self) -> (i64, u8, u8, u8, u64) {
        match *self {
            Self::EndogenousEvent {
                timestamp_utc_nanos,
                side,
                generation_order,
            } => (
                timestamp_utc_nanos,
                0,
                match side {
                    SeededTickEventSide::Buy => 0,
                    SeededTickEventSide::Sell => 1,
                },
                0,
                generation_order,
            ),
            Self::ExogenousNews {
                timestamp_utc_nanos,
                news_arrival_component,
                generation_order,
                ..
            } => (
                timestamp_utc_nanos,
                1,
                0,
                match news_arrival_component {
                    NewsArrivalComponent::Known => 0,
                    NewsArrivalComponent::Surprise => 1,
                },
                generation_order,
            ),
        }
    }

    pub(crate) const fn transition_step(
        &self,
        elapsed_hours_since_previous_update: f64,
    ) -> HawkesTransitionStep {
        match *self {
            Self::EndogenousEvent { side, .. } => HawkesTransitionStep::new(
                elapsed_hours_since_previous_update,
                Some(map_seeded_tick_side_to_hawkes_side(side)),
                None,
            ),
            Self::ExogenousNews {
                activity_coupling_mark,
                ..
            } => HawkesTransitionStep::new(
                elapsed_hours_since_previous_update,
                None,
                Some(activity_coupling_mark),
            ),
        }
    }

    pub(crate) const fn generation_order(&self) -> u64 {
        match *self {
            Self::EndogenousEvent {
                generation_order, ..
            } => generation_order,
            Self::ExogenousNews {
                generation_order, ..
            } => generation_order,
        }
    }

    pub(crate) const fn transition_kind(&self) -> SeededTickEventDebugTraceTransitionKind {
        match *self {
            Self::EndogenousEvent { side, .. } => match side {
                SeededTickEventSide::Buy => {
                    SeededTickEventDebugTraceTransitionKind::EndogenousBuyEvent
                }
                SeededTickEventSide::Sell => {
                    SeededTickEventDebugTraceTransitionKind::EndogenousSellEvent
                }
            },
            Self::ExogenousNews { .. } => SeededTickEventDebugTraceTransitionKind::ExogenousNews,
        }
    }

    pub(crate) const fn news_routing_update(
        &self,
    ) -> Option<SeededTickSimulationNewsRoutingUpdate> {
        match *self {
            Self::EndogenousEvent { .. } => None,
            Self::ExogenousNews {
                activity_coupling_mark,
                open_window_return_jump_increment,
                closed_window_gap_increment,
                ..
            } => Some(SeededTickSimulationNewsRoutingUpdate {
                activity_coupling_j_t_increment: activity_coupling_mark,
                open_window_return_jump_m_t_increment: open_window_return_jump_increment,
                closed_window_gap_g_t_increment: closed_window_gap_increment,
            }),
        }
    }

    pub(crate) const fn trace_news_transition_payload(
        &self,
    ) -> Option<SeededTickSimulationTraceNewsTransitionPayload> {
        match *self {
            Self::EndogenousEvent { .. } => None,
            Self::ExogenousNews {
                news_arrival_component,
                activity_coupling_mark,
                return_jump_mark,
                market_state,
                return_jump_routing_target,
                open_window_return_jump_increment,
                closed_window_gap_increment,
                ..
            } => Some(SeededTickSimulationTraceNewsTransitionPayload {
                news_arrival_component,
                activity_coupling_mark,
                return_jump_mark,
                market_state,
                return_jump_routing_target,
                open_window_return_jump_increment,
                closed_window_gap_increment,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SeededTickSimulationNewsRoutingUpdate {
    pub(crate) activity_coupling_j_t_increment: f64,
    pub(crate) open_window_return_jump_m_t_increment: f64,
    pub(crate) closed_window_gap_g_t_increment: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SeededTickSimulationTraceNewsTransitionPayload {
    pub(crate) news_arrival_component: NewsArrivalComponent,
    pub(crate) activity_coupling_mark: f64,
    pub(crate) return_jump_mark: f64,
    pub(crate) market_state: MarketState,
    pub(crate) return_jump_routing_target: NewsReturnJumpRoutingTarget,
    pub(crate) open_window_return_jump_increment: f64,
    pub(crate) closed_window_gap_increment: f64,
}

pub(crate) fn merge_ordered_interval_updates(
    endogenous_interval_updates: Vec<IntervalStochasticUpdate>,
    exogenous_interval_updates: Vec<IntervalStochasticUpdate>,
) -> Result<Vec<IntervalStochasticUpdate>, SeededTickSimulationRunError> {
    let mut endogenous_iterator = endogenous_interval_updates.into_iter();
    let mut exogenous_iterator = exogenous_interval_updates.into_iter();

    let mut next_endogenous = endogenous_iterator.next();
    let mut next_exogenous = exogenous_iterator.next();

    let merged_update_capacity = endogenous_iterator
        .len()
        .saturating_add(exogenous_iterator.len())
        .saturating_add(next_endogenous.is_some() as usize)
        .saturating_add(next_exogenous.is_some() as usize);
    let mut merged_updates: Vec<IntervalStochasticUpdate> =
        try_reserve_simulation_buffer("merged_interval_updates", merged_update_capacity)?;

    loop {
        match (&next_endogenous, &next_exogenous) {
            (Some(endogenous_update), Some(exogenous_update)) => {
                if compare_interval_updates(endogenous_update, exogenous_update)
                    != Ordering::Greater
                {
                    let Some(endogenous_update) = next_endogenous.take() else {
                        return Err(SeededTickSimulationRunError::InternalInvariantViolation {
                            message: "missing endogenous update while merging interval updates",
                        });
                    };

                    merged_updates.push(endogenous_update);
                    next_endogenous = endogenous_iterator.next();
                } else {
                    let Some(exogenous_update) = next_exogenous.take() else {
                        return Err(SeededTickSimulationRunError::InternalInvariantViolation {
                            message: "missing exogenous update while merging interval updates",
                        });
                    };

                    merged_updates.push(exogenous_update);
                    next_exogenous = exogenous_iterator.next();
                }
            }
            (Some(_), None) => {
                let Some(endogenous_update) = next_endogenous.take() else {
                    return Err(SeededTickSimulationRunError::InternalInvariantViolation {
                        message: "missing endogenous tail update while merging interval updates",
                    });
                };

                merged_updates.push(endogenous_update);
                merged_updates.extend(endogenous_iterator);

                return Ok(merged_updates);
            }
            (None, Some(_)) => {
                let Some(exogenous_update) = next_exogenous.take() else {
                    return Err(SeededTickSimulationRunError::InternalInvariantViolation {
                        message: "missing exogenous tail update while merging interval updates",
                    });
                };

                merged_updates.push(exogenous_update);
                merged_updates.extend(exogenous_iterator);

                return Ok(merged_updates);
            }
            (None, None) => return Ok(merged_updates),
        }
    }
}

pub(crate) fn map_side_to_transition_kind(
    side: SeededTickEventSide,
) -> SeededFinePathTransitionKind {
    match side {
        SeededTickEventSide::Buy => SeededFinePathTransitionKind::BuyTrade,
        SeededTickEventSide::Sell => SeededFinePathTransitionKind::SellTrade,
    }
}

pub(crate) const fn map_seeded_tick_side_to_hawkes_side(
    side: SeededTickEventSide,
) -> HawkesEventSide {
    match side {
        SeededTickEventSide::Buy => HawkesEventSide::Buy,
        SeededTickEventSide::Sell => HawkesEventSide::Sell,
    }
}

pub(crate) fn is_public_seeded_fine_path_record(record: &SeededFinePathRecord) -> bool {
    matches!(
        record.market_state,
        SeededTickSimulationNewsEventMarketState::Open
    )
}

fn compare_interval_updates(
    left: &IntervalStochasticUpdate,
    right: &IntervalStochasticUpdate,
) -> Ordering {
    left.ordering_key().cmp(&right.ordering_key())
}
