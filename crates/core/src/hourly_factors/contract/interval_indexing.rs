use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use crate::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLookupError};

use super::constants::ONE_SECOND_NANOS;
use super::error::HourlyFactorContractError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FixedIntervalIndexing {
    pub(super) exchange_code: String,
    pub(super) exchange_timezone_name: String,
    pub(super) request_start_utc_nanos: i64,
    pub(super) request_end_utc_nanos: i64,
    pub(super) interval_duration_nanos: i64,
    pub(super) interval_count: usize,
    pub(super) interval_indices: Vec<u64>,
    pub(super) interval_start_utc_nanos: Vec<i64>,
    pub(super) interval_end_utc_nanos: Vec<i64>,
    pub(super) interval_start_exchange_local: Vec<String>,
    pub(super) interval_end_exchange_local: Vec<String>,
    pub(super) skipped_local_hour_interval_count: usize,
    pub(super) repeated_local_hour_interval_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExchangeLocalIntervalTransitionKind {
    Standard,
    Skipped,
    Repeated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExchangeLocalIntervalBoundaryProjection {
    interval_start_exchange_local: String,
    interval_end_exchange_local: String,
    transition_kind: ExchangeLocalIntervalTransitionKind,
}

pub(super) fn build_fixed_interval_indexing(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
) -> Result<FixedIntervalIndexing, HourlyFactorContractError> {
    if exchange_code.trim().is_empty() {
        return Err(HourlyFactorContractError::EmptyExchangeCode);
    }
    if interval_duration_nanos <= 0 {
        return Err(HourlyFactorContractError::InvalidIntervalDurationNanos {
            value: interval_duration_nanos,
        });
    }

    let range_start_utc_nanos = timestamp_to_utc_nanos(range_start_utc, "range_start_utc")?;
    let range_end_utc_nanos = timestamp_to_utc_nanos(range_end_utc, "range_end_utc")?;

    if range_start_utc_nanos >= range_end_utc_nanos {
        return Err(HourlyFactorContractError::NonIncreasingRange {
            range_start_utc_nanos,
            range_end_utc_nanos,
        });
    }

    let range_duration_nanos = range_end_utc_nanos - range_start_utc_nanos;
    let remainder_nanos = range_duration_nanos.rem_euclid(interval_duration_nanos);
    if remainder_nanos != 0 {
        return Err(
            HourlyFactorContractError::RangeDurationNotDivisibleByInterval {
                range_start_utc_nanos,
                range_end_utc_nanos,
                interval_duration_nanos,
                remainder_nanos,
            },
        );
    }

    let definition = calendar_registry
        .get_required(exchange_code)
        .map_err(map_registry_lookup_error)?;
    let exchange_timezone_name = definition.timezone_name().to_string();
    let timezone = Tz::from_str(definition.timezone_name()).map_err(|_| {
        HourlyFactorContractError::InvalidTimezoneName {
            exchange_code: exchange_code.to_string(),
            timezone_name: definition.timezone_name().to_string(),
        }
    })?;

    let interval_count = (range_duration_nanos / interval_duration_nanos) as usize;
    let mut interval_indices: Vec<u64> = Vec::with_capacity(interval_count);
    let mut interval_start_utc_nanos: Vec<i64> = Vec::with_capacity(interval_count);
    let mut interval_end_utc_nanos: Vec<i64> = Vec::with_capacity(interval_count);
    let mut interval_start_exchange_local: Vec<String> = Vec::with_capacity(interval_count);
    let mut interval_end_exchange_local: Vec<String> = Vec::with_capacity(interval_count);
    let mut skipped_local_hour_interval_count: usize = 0;
    let mut repeated_local_hour_interval_count: usize = 0;

    for interval_index in 0..interval_count {
        let interval_offset_nanos = interval_index as i64 * interval_duration_nanos;
        let interval_start_nanos = range_start_utc_nanos + interval_offset_nanos;
        let interval_end_nanos = interval_start_nanos + interval_duration_nanos;

        let start_utc = utc_timestamp_from_nanos(interval_start_nanos, "range_start_utc")?;
        let end_utc = utc_timestamp_from_nanos(interval_end_nanos, "range_end_utc")?;
        let local_boundary_projection = project_interval_boundaries_to_exchange_local(
            start_utc,
            end_utc,
            &timezone,
            interval_duration_nanos,
        )?;

        match local_boundary_projection.transition_kind {
            ExchangeLocalIntervalTransitionKind::Standard => {}
            ExchangeLocalIntervalTransitionKind::Skipped => skipped_local_hour_interval_count += 1,
            ExchangeLocalIntervalTransitionKind::Repeated => {
                repeated_local_hour_interval_count += 1;
            }
        }

        interval_indices.push(interval_index as u64);
        interval_start_utc_nanos.push(interval_start_nanos);
        interval_end_utc_nanos.push(interval_end_nanos);
        interval_start_exchange_local.push(local_boundary_projection.interval_start_exchange_local);
        interval_end_exchange_local.push(local_boundary_projection.interval_end_exchange_local);
    }

    Ok(FixedIntervalIndexing {
        exchange_code: exchange_code.to_string(),
        exchange_timezone_name,
        request_start_utc_nanos: range_start_utc_nanos,
        request_end_utc_nanos: range_end_utc_nanos,
        interval_duration_nanos,
        interval_count,
        interval_indices,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        interval_start_exchange_local,
        interval_end_exchange_local,
        skipped_local_hour_interval_count,
        repeated_local_hour_interval_count,
    })
}

fn project_interval_boundaries_to_exchange_local(
    interval_start_utc: DateTime<Utc>,
    interval_end_utc: DateTime<Utc>,
    timezone: &Tz,
    interval_duration_nanos: i64,
) -> Result<ExchangeLocalIntervalBoundaryProjection, HourlyFactorContractError> {
    let interval_start_exchange_local = interval_start_utc.with_timezone(timezone);
    let interval_end_exchange_local = interval_end_utc.with_timezone(timezone);

    let wall_clock_nanos_between_boundaries = interval_end_exchange_local
        .naive_local()
        .signed_duration_since(interval_start_exchange_local.naive_local())
        .num_nanoseconds()
        .ok_or(HourlyFactorContractError::TimestampOutOfRange {
            field: "interval_end_exchange_local",
        })?;

    let transition_kind = if wall_clock_nanos_between_boundaries > interval_duration_nanos {
        ExchangeLocalIntervalTransitionKind::Skipped
    } else if wall_clock_nanos_between_boundaries < interval_duration_nanos {
        ExchangeLocalIntervalTransitionKind::Repeated
    } else {
        ExchangeLocalIntervalTransitionKind::Standard
    };

    Ok(ExchangeLocalIntervalBoundaryProjection {
        interval_start_exchange_local: interval_start_exchange_local.to_rfc3339(),
        interval_end_exchange_local: interval_end_exchange_local.to_rfc3339(),
        transition_kind,
    })
}

fn map_registry_lookup_error(
    source: ExchangeCalendarRegistryLookupError,
) -> HourlyFactorContractError {
    match source {
        ExchangeCalendarRegistryLookupError::MissingExchangeId { exchange_code } => {
            HourlyFactorContractError::MissingExchangeId { exchange_code }
        }
    }
}

fn timestamp_to_utc_nanos(
    timestamp_utc: DateTime<Utc>,
    field: &'static str,
) -> Result<i64, HourlyFactorContractError> {
    timestamp_utc
        .timestamp_nanos_opt()
        .ok_or(HourlyFactorContractError::TimestampOutOfRange { field })
}

fn utc_timestamp_from_nanos(
    timestamp_utc_nanos: i64,
    field: &'static str,
) -> Result<DateTime<Utc>, HourlyFactorContractError> {
    let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
    let subsecond_nanos = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;

    DateTime::from_timestamp(seconds, subsecond_nanos)
        .ok_or(HourlyFactorContractError::TimestampOutOfRange { field })
}
