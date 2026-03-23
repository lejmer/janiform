use super::contracts::{
    MarketDataTranslationKnownNewsRecord, MarketDataTranslationPreprocessingError,
};
use super::session_windows::resolve_market_state_and_session_window;
use super::shared::{
    ACTIVITY_LIFT_WINDOW_MINUTES, CalibrationReturnComponentsKnownNewsReturnSample,
    CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    CalibrationReturnComponentsSignalSourceWindow, Duration, ExchangeCalendarDefinition,
    KNOWN_NEWS_EXCLUSION_WINDOW_MINUTES, KnownNewsResolutionSummary, MarketState, OpenTickRecord,
    PartitionPointIndex, ResolvedKnownNewsRecord, Tz, count_timestamps_in_half_open_interval,
    datetime_from_nanos, round_to,
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
                let Some(before_index) = open_price_tick_timestamps
                    .partition_point_index(known_news_record.event_timestamp_utc_nanos, true)
                else {
                    continue;
                };

                let after_index = open_price_tick_timestamps.partition_point(|timestamp| {
                    *timestamp <= known_news_record.event_timestamp_utc_nanos
                });
                if after_index >= open_price_ticks.len() {
                    continue;
                }

                let before_tick = &open_price_ticks[before_index];
                let after_tick = &open_price_ticks[after_index];

                return_samples.push(CalibrationReturnComponentsKnownNewsReturnSample {
                    event_timestamp_utc_nanos: known_news_record.event_timestamp_utc_nanos,
                    open_window_jump_return: f64::ln(after_tick.price / before_tick.price),
                    closed_window_gap_return: 0.0,
                    post_shock_activity_lift: activity_lift(
                        known_news_record.event_timestamp_utc_nanos,
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
    open_known_news_timestamps: &[i64],
) -> f64 {
    if open_price_ticks.is_empty() || open_known_news_timestamps.is_empty() {
        return 0.0;
    }

    let window_nanos = Duration::minutes(ACTIVITY_LIFT_WINDOW_MINUTES)
        .num_nanoseconds()
        .expect("activity-lift window must fit in i64");

    let mut right_index: usize = 0;
    let mut left_index: usize = 0;
    let mut attributable_tick_count: usize = 0;

    for open_tick in open_price_ticks {
        while right_index < open_known_news_timestamps.len()
            && open_known_news_timestamps[right_index] <= open_tick.timestamp_utc_nanos
        {
            right_index += 1;
        }

        while left_index < right_index
            && open_known_news_timestamps[left_index] < open_tick.timestamp_utc_nanos - window_nanos
        {
            left_index += 1;
        }

        if right_index > left_index {
            attributable_tick_count += 1;
        }
    }

    attributable_tick_count as f64 / open_price_ticks.len() as f64
}

pub(super) fn activity_lift(
    anchor_timestamp_utc_nanos: i64,
    open_price_tick_timestamps: &[i64],
) -> f64 {
    let window_nanos = Duration::minutes(ACTIVITY_LIFT_WINDOW_MINUTES)
        .num_nanoseconds()
        .expect("activity-lift window must fit in i64");

    let pre_window_start = anchor_timestamp_utc_nanos - window_nanos;
    let post_window_end = anchor_timestamp_utc_nanos + window_nanos;

    let pre_event_count = count_timestamps_in_half_open_interval(
        open_price_tick_timestamps,
        pre_window_start,
        anchor_timestamp_utc_nanos,
    );
    let post_event_count = count_timestamps_in_half_open_interval(
        open_price_tick_timestamps,
        anchor_timestamp_utc_nanos,
        post_window_end,
    );

    if pre_event_count == 0 {
        return 1.0;
    }

    round_to(post_event_count as f64 / pre_event_count as f64, 6)
}

pub(super) fn is_within_known_news_exclusion_window(
    candidate_timestamp_utc_nanos: i64,
    known_news_timestamps: &[i64],
) -> bool {
    if known_news_timestamps.is_empty() {
        return false;
    }

    let window_nanos = Duration::minutes(KNOWN_NEWS_EXCLUSION_WINDOW_MINUTES)
        .num_nanoseconds()
        .expect("known-news exclusion window must fit in i64");

    let lower_bound = candidate_timestamp_utc_nanos - window_nanos;
    let upper_bound = candidate_timestamp_utc_nanos + window_nanos;

    let lower_index = known_news_timestamps.partition_point(|timestamp| *timestamp < lower_bound);

    lower_index < known_news_timestamps.len() && known_news_timestamps[lower_index] <= upper_bound
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
