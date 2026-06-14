use std::collections::BTreeMap;

use chrono::{Datelike, Timelike};
use chrono_tz::Tz;
use janiform_foundations::time::{NANOS_PER_HOUR, floor_to_hour_utc_nanos};

use crate::calendar::definition::resolve_market_state_for_definition_at_local;
use crate::calendar::errors::TimestampEventPreprocessingError;
use crate::calendar::types::{
    CLOSED_MARKET_STATE_CODE, ExchangeCalendarDefinition, MarketState, OPEN_MARKET_STATE_CODE,
    TimestampEventInputRecord, TimestampEventPreprocessingDiagnostics,
    TimestampEventPreprocessingOutput, TimestampEventSessionFilter,
};
use crate::calendar::{
    PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION, TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AcceptedPreprocessedTimestampEventRow {
    source_record_position: u64,
    timestamp_utc_nanos: i64,
    exchange_local_minute_of_day: u16,
    exchange_local_day_of_week: u8,
    exchange_local_day_of_year: u16,
    market_state_code: u8,
}

pub(crate) fn preprocess_timestamp_events(
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    exchange_code: &str,
    input_records: Vec<TimestampEventInputRecord>,
    session_filter: TimestampEventSessionFilter,
    sparse_hour_event_threshold: usize,
) -> Result<TimestampEventPreprocessingOutput, TimestampEventPreprocessingError> {
    if sparse_hour_event_threshold < 1 {
        return Err(
            TimestampEventPreprocessingError::InvalidSparseHourEventThreshold {
                sparse_hour_event_threshold,
            },
        );
    }

    let exchange_timezone_name = definition.timezone_name().to_string();

    let input_record_count = input_records.len();
    let validated_record_count = input_record_count;

    let mut accepted_rows: Vec<AcceptedPreprocessedTimestampEventRow> = Vec::new();

    for input_record in input_records {
        let timestamp_utc_nanos = input_record.timestamp_utc.timestamp_nanos_opt().ok_or(
            TimestampEventPreprocessingError::TimestampOutOfRange {
                source_record_position: input_record.source_record_position,
            },
        )?;
        let timestamp_local = input_record.timestamp_utc.with_timezone(&timezone);
        let market_state =
            resolve_market_state_for_definition_at_local(definition, timestamp_local);

        if !session_filter.includes_market_state(market_state) {
            continue;
        }

        accepted_rows.push(AcceptedPreprocessedTimestampEventRow {
            source_record_position: input_record.source_record_position,
            timestamp_utc_nanos,
            exchange_local_minute_of_day: (timestamp_local.hour() * 60 + timestamp_local.minute())
                as u16,
            exchange_local_day_of_week: timestamp_local.weekday().number_from_monday() as u8,
            exchange_local_day_of_year: timestamp_local.ordinal() as u16,
            market_state_code: market_state_to_code(market_state),
        });
    }

    accepted_rows.sort_by(|left_row, right_row| {
        left_row
            .timestamp_utc_nanos
            .cmp(&right_row.timestamp_utc_nanos)
            .then(
                left_row
                    .source_record_position
                    .cmp(&right_row.source_record_position),
            )
    });

    let accepted_record_count = accepted_rows.len();
    let filtered_record_count = validated_record_count.saturating_sub(accepted_record_count);

    let mut source_record_positions: Vec<u64> = Vec::with_capacity(accepted_record_count);
    let mut timestamp_utc_nanos: Vec<i64> = Vec::with_capacity(accepted_record_count);
    let mut exchange_local_minute_of_day: Vec<u16> = Vec::with_capacity(accepted_record_count);
    let mut exchange_local_day_of_week: Vec<u8> = Vec::with_capacity(accepted_record_count);
    let mut exchange_local_day_of_year: Vec<u16> = Vec::with_capacity(accepted_record_count);
    let mut market_state_codes: Vec<u8> = Vec::with_capacity(accepted_record_count);

    for accepted_row in accepted_rows {
        source_record_positions.push(accepted_row.source_record_position);
        timestamp_utc_nanos.push(accepted_row.timestamp_utc_nanos);
        exchange_local_minute_of_day.push(accepted_row.exchange_local_minute_of_day);
        exchange_local_day_of_week.push(accepted_row.exchange_local_day_of_week);
        exchange_local_day_of_year.push(accepted_row.exchange_local_day_of_year);
        market_state_codes.push(accepted_row.market_state_code);
    }

    let (
        missing_hour_bucket_utc_nanos,
        sparse_hour_bucket_utc_nanos,
        first_event_timestamp_utc_nanos,
        last_event_timestamp_utc_nanos,
        hour_bucket_count,
    ) = build_hour_bucket_diagnostics(&timestamp_utc_nanos, sparse_hour_event_threshold);

    let diagnostics = TimestampEventPreprocessingDiagnostics {
        input_record_count,
        validated_record_count,
        accepted_record_count,
        filtered_record_count,
        first_event_timestamp_utc_nanos,
        last_event_timestamp_utc_nanos,
        hour_bucket_count,
        missing_hour_bucket_count: missing_hour_bucket_utc_nanos.len(),
        sparse_hour_bucket_count: sparse_hour_bucket_utc_nanos.len(),
    };

    Ok(TimestampEventPreprocessingOutput {
        schema_version: PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION,
        input_schema_version: TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION,
        exchange_code: exchange_code.to_string(),
        exchange_timezone_name,
        source_record_positions,
        timestamp_utc_nanos,
        exchange_local_minute_of_day,
        exchange_local_day_of_week,
        exchange_local_day_of_year,
        market_state_codes,
        missing_hour_bucket_utc_nanos,
        sparse_hour_bucket_utc_nanos,
        diagnostics,
    })
}

fn market_state_to_code(market_state: MarketState) -> u8 {
    match market_state {
        MarketState::Open => OPEN_MARKET_STATE_CODE,
        MarketState::Closed => CLOSED_MARKET_STATE_CODE,
    }
}

fn build_hour_bucket_diagnostics(
    timestamp_utc_nanos: &[i64],
    sparse_hour_event_threshold: usize,
) -> (Vec<i64>, Vec<i64>, Option<i64>, Option<i64>, usize) {
    if timestamp_utc_nanos.is_empty() {
        return (Vec::new(), Vec::new(), None, None, 0);
    }

    let first_event_timestamp_utc_nanos = timestamp_utc_nanos.first().copied();
    let last_event_timestamp_utc_nanos = timestamp_utc_nanos.last().copied();

    let mut observed_bucket_counts: BTreeMap<i64, usize> = BTreeMap::new();
    for timestamp_utc_nanos in timestamp_utc_nanos {
        let hour_bucket_utc_nanos = floor_to_hour_utc_nanos(*timestamp_utc_nanos);
        *observed_bucket_counts
            .entry(hour_bucket_utc_nanos)
            .or_insert(0) += 1;
    }

    let (first_hour_bucket_utc_nanos, last_hour_bucket_utc_nanos) = match (
        first_event_timestamp_utc_nanos,
        last_event_timestamp_utc_nanos,
    ) {
        (Some(first_timestamp_utc_nanos), Some(last_timestamp_utc_nanos)) => (
            floor_to_hour_utc_nanos(first_timestamp_utc_nanos),
            floor_to_hour_utc_nanos(last_timestamp_utc_nanos),
        ),
        _ => {
            return (Vec::new(), Vec::new(), None, None, 0);
        }
    };

    let mut missing_hour_bucket_utc_nanos: Vec<i64> = Vec::new();
    let mut sparse_hour_bucket_utc_nanos: Vec<i64> = Vec::new();
    let mut hour_bucket_count: usize = 0;
    let mut current_bucket_utc_nanos = first_hour_bucket_utc_nanos;

    while current_bucket_utc_nanos <= last_hour_bucket_utc_nanos {
        hour_bucket_count += 1;

        let observed_count = observed_bucket_counts
            .get(&current_bucket_utc_nanos)
            .copied()
            .unwrap_or(0);

        if observed_count == 0 {
            missing_hour_bucket_utc_nanos.push(current_bucket_utc_nanos);
        } else if observed_count < sparse_hour_event_threshold {
            sparse_hour_bucket_utc_nanos.push(current_bucket_utc_nanos);
        }

        current_bucket_utc_nanos += NANOS_PER_HOUR;
    }

    (
        missing_hour_bucket_utc_nanos,
        sparse_hour_bucket_utc_nanos,
        first_event_timestamp_utc_nanos,
        last_event_timestamp_utc_nanos,
        hour_bucket_count,
    )
}
