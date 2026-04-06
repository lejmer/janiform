use super::contracts::{
    MarketDataTranslationKnownNewsRecord, MarketDataTranslationPreprocessingError,
};
use super::session_windows::resolve_market_state_and_session_window;
use super::shared::{
    CalibrationReturnComponentsKnownNewsReturnSample,
    CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    CalibrationReturnComponentsSignalSourceWindow, ExchangeCalendarDefinition,
    KnownNewsResolutionSummary, MarketState, OpenTickRecord, PartitionPointIndex,
    ResolvedKnownNewsRecord, Tz, datetime_from_nanos, round_to,
};

pub(super) fn resolve_known_news_records(
    known_news_records: &[MarketDataTranslationKnownNewsRecord],
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    source_window: CalibrationReturnComponentsSignalSourceWindow,
) -> Result<KnownNewsResolutionSummary, MarketDataTranslationPreprocessingError> {
    let mut retained_records: Vec<ResolvedKnownNewsRecord> = Vec::new();
    let mut outside_source_window_count: usize = 0;
    let mut open_event_count: usize = 0;
    let mut closed_event_count: usize = 0;

    for (event_index, known_news_record) in known_news_records.iter().enumerate() {
        if known_news_record.event_timestamp_utc_nanos < source_window.window_start_utc_nanos
            || known_news_record.event_timestamp_utc_nanos >= source_window.window_end_utc_nanos
        {
            outside_source_window_count += 1;
            continue;
        }

        let timestamp_utc = datetime_from_nanos(
            known_news_record.event_timestamp_utc_nanos,
            MarketDataTranslationPreprocessingError::KnownNewsTimestampOutOfRange { event_index },
        )?;

        let (market_state, _) =
            resolve_market_state_and_session_window(definition, timezone, "", timestamp_utc)?;
        match market_state {
            MarketState::Open => open_event_count += 1,
            MarketState::Closed => closed_event_count += 1,
        }

        retained_records.push(ResolvedKnownNewsRecord {
            event_timestamp_utc_nanos: known_news_record.event_timestamp_utc_nanos,
            event_identifier: known_news_record.event_identifier.clone(),
            market_state,
        });
    }

    Ok(KnownNewsResolutionSummary {
        retained_records,
        outside_source_window_count,
        open_event_count,
        closed_event_count,
    })
}

pub(super) fn derive_known_news_return_samples(
    retained_known_news: &[ResolvedKnownNewsRecord],
    open_price_ticks: &[OpenTickRecord],
    close_open_boundary_observations: &[CalibrationReturnComponentsObservedCloseOpenBoundaryObservation],
    open_price_tick_timestamps: &[i64],
) -> Vec<CalibrationReturnComponentsKnownNewsReturnSample> {
    let mut return_samples: Vec<CalibrationReturnComponentsKnownNewsReturnSample> = Vec::new();

    for known_news_record in retained_known_news {
        match known_news_record.market_state {
            MarketState::Open => {
                let Some((before_index, after_index)) = attributed_open_return_interval(
                    known_news_record.event_timestamp_utc_nanos,
                    open_price_ticks,
                    open_price_tick_timestamps,
                ) else {
                    continue;
                };

                let before_tick = &open_price_ticks[before_index];
                let after_tick = &open_price_ticks[after_index];

                return_samples.push(CalibrationReturnComponentsKnownNewsReturnSample {
                    event_timestamp_utc_nanos: known_news_record.event_timestamp_utc_nanos,
                    attributed_open_return_timestamp_utc_nanos: Some(
                        after_tick.timestamp_utc_nanos,
                    ),
                    open_window_jump_return: f64::ln(after_tick.price / before_tick.price),
                    closed_window_gap_return: 0.0,
                    post_shock_activity_lift: activity_lift(
                        known_news_record.event_timestamp_utc_nanos,
                        open_price_ticks,
                        open_price_tick_timestamps,
                    ),
                });
            }
            MarketState::Closed => {
                if let Some(boundary_observation) = boundary_observation_for_event(
                    close_open_boundary_observations,
                    known_news_record.event_timestamp_utc_nanos,
                ) {
                    return_samples.push(CalibrationReturnComponentsKnownNewsReturnSample {
                        event_timestamp_utc_nanos: known_news_record.event_timestamp_utc_nanos,
                        attributed_open_return_timestamp_utc_nanos: None,
                        open_window_jump_return: 0.0,
                        closed_window_gap_return: f64::ln(
                            boundary_observation.open_price / boundary_observation.close_price,
                        ),
                        post_shock_activity_lift: boundary_observation.post_shock_activity_lift,
                    });
                }
            }
        }
    }

    return_samples
}

pub(super) fn open_session_known_news_activity_share(
    open_price_ticks: &[OpenTickRecord],
    attributed_open_return_timestamps: &[i64],
) -> f64 {
    if open_price_ticks.is_empty() || attributed_open_return_timestamps.is_empty() {
        return 0.0;
    }

    let mut attributable_tick_count: usize = 0;

    for open_tick in open_price_ticks {
        if attributed_open_return_timestamps
            .binary_search(&open_tick.timestamp_utc_nanos)
            .is_ok()
        {
            attributable_tick_count += 1;
        }
    }

    attributable_tick_count as f64 / open_price_ticks.len() as f64
}

pub(super) fn activity_lift(
    anchor_timestamp_utc_nanos: i64,
    open_price_ticks: &[OpenTickRecord],
    open_price_tick_timestamps: &[i64],
) -> f64 {
    let Some((left_gap_nanos, right_gap_nanos)) = local_activity_comparison_window(
        anchor_timestamp_utc_nanos,
        open_price_ticks,
        open_price_tick_timestamps,
    ) else {
        return 1.0;
    };

    if left_gap_nanos <= 0 || right_gap_nanos <= 0 {
        return 1.0;
    }

    round_to(left_gap_nanos as f64 / right_gap_nanos as f64, 6)
}

pub(super) fn is_attributed_to_known_news_open_return(
    candidate_timestamp_utc_nanos: i64,
    attributed_open_return_timestamps: &[i64],
) -> bool {
    if attributed_open_return_timestamps.is_empty() {
        return false;
    }

    attributed_open_return_timestamps
        .binary_search(&candidate_timestamp_utc_nanos)
        .is_ok()
}

pub(super) fn boundary_observation_for_event(
    close_open_boundary_observations: &[CalibrationReturnComponentsObservedCloseOpenBoundaryObservation],
    event_timestamp_utc_nanos: i64,
) -> Option<&CalibrationReturnComponentsObservedCloseOpenBoundaryObservation> {
    close_open_boundary_observations
        .iter()
        .find(|boundary_observation| {
            boundary_observation.close_timestamp_utc_nanos <= event_timestamp_utc_nanos
                && event_timestamp_utc_nanos < boundary_observation.open_timestamp_utc_nanos
        })
}

pub(super) fn attributed_open_return_timestamps(
    retained_known_news: &[ResolvedKnownNewsRecord],
    open_price_ticks: &[OpenTickRecord],
    open_price_tick_timestamps: &[i64],
) -> Vec<i64> {
    let mut timestamps = retained_known_news
        .iter()
        .filter(|record| record.market_state == MarketState::Open)
        .filter_map(|record| {
            attributed_open_return_interval(
                record.event_timestamp_utc_nanos,
                open_price_ticks,
                open_price_tick_timestamps,
            )
            .map(|(_, after_index)| open_price_ticks[after_index].timestamp_utc_nanos)
        })
        .collect::<Vec<_>>();
    timestamps.sort_unstable();
    timestamps.dedup();
    timestamps
}

fn attributed_open_return_interval(
    event_timestamp_utc_nanos: i64,
    open_price_ticks: &[OpenTickRecord],
    open_price_tick_timestamps: &[i64],
) -> Option<(usize, usize)> {
    let before_index =
        open_price_tick_timestamps.partition_point_index(event_timestamp_utc_nanos, true)?;
    let after_index = open_price_tick_timestamps
        .partition_point(|timestamp| *timestamp <= event_timestamp_utc_nanos);

    if after_index >= open_price_ticks.len() {
        return None;
    }

    Some((before_index, after_index))
}

fn local_activity_comparison_window(
    anchor_timestamp_utc_nanos: i64,
    open_price_ticks: &[OpenTickRecord],
    open_price_tick_timestamps: &[i64],
) -> Option<(i64, i64)> {
    if open_price_ticks.is_empty() {
        return None;
    }

    let containing_session =
        containing_session_window(anchor_timestamp_utc_nanos, open_price_ticks)?;

    let previous_tick_index =
        open_price_tick_timestamps.partition_point_index(anchor_timestamp_utc_nanos, false);
    let next_tick_index = open_price_tick_timestamps
        .partition_point(|timestamp| *timestamp <= anchor_timestamp_utc_nanos);

    let left_span_nanos = previous_tick_index
        .and_then(|index| open_price_ticks.get(index))
        .filter(|tick| {
            tick.session_open_utc_nanos == containing_session.0
                && tick.session_close_utc_nanos == containing_session.1
        })
        .map(|tick| anchor_timestamp_utc_nanos - tick.timestamp_utc_nanos)
        .unwrap_or(anchor_timestamp_utc_nanos - containing_session.0);

    let right_span_nanos = open_price_ticks
        .get(next_tick_index)
        .filter(|tick| {
            tick.session_open_utc_nanos == containing_session.0
                && tick.session_close_utc_nanos == containing_session.1
        })
        .map(|tick| tick.timestamp_utc_nanos - anchor_timestamp_utc_nanos)
        .unwrap_or(containing_session.1 - anchor_timestamp_utc_nanos);

    if left_span_nanos <= 0 || right_span_nanos <= 0 {
        return None;
    }

    Some((left_span_nanos, right_span_nanos))
}

fn containing_session_window(
    anchor_timestamp_utc_nanos: i64,
    open_price_ticks: &[OpenTickRecord],
) -> Option<(i64, i64)> {
    let current_tick_index = open_price_ticks
        .partition_point(|tick| tick.timestamp_utc_nanos <= anchor_timestamp_utc_nanos);

    let previous_tick = current_tick_index
        .checked_sub(1)
        .and_then(|index| open_price_ticks.get(index));
    if let Some(tick) = previous_tick.filter(|tick| {
        tick.session_open_utc_nanos <= anchor_timestamp_utc_nanos
            && anchor_timestamp_utc_nanos < tick.session_close_utc_nanos
    }) {
        return Some((tick.session_open_utc_nanos, tick.session_close_utc_nanos));
    }

    open_price_ticks
        .get(current_tick_index)
        .filter(|tick| {
            tick.session_open_utc_nanos <= anchor_timestamp_utc_nanos
                && anchor_timestamp_utc_nanos < tick.session_close_utc_nanos
        })
        .map(|tick| (tick.session_open_utc_nanos, tick.session_close_utc_nanos))
}
