use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::coarse_data::LoadedCoarseRow;
use super::contracts::{MarketDataLoadingError, MarketDataNormalizedCoarseTrainingRecord};

const COARSE_TIMESTAMP_CLOSE_SEMANTICS: &str = "timestamp_is_bar_close_time.v1";
const COARSE_EXPLICIT_INTERVAL_SEMANTICS: &str = "explicit_interval_start_end.v1";
const COARSE_FIXED_INTERVAL_INFERENCE_POLICY: &str =
    "positive_adjacent_timestamp_diff_lower_median_fixed_interval.v1";
const COARSE_EXPLICIT_INTERVAL_POLICY: &str =
    "explicit_source_interval_boundaries_with_lower_median_duration_summary.v1";

#[derive(Debug, Clone, Copy)]
pub(super) struct CoarseInterval {
    pub(super) interval_start_utc_nanos: i64,
    pub(super) interval_end_utc_nanos: i64,
}

pub(super) fn resolve_row_interval(
    row: &LoadedCoarseRow,
    nominal_interval_nanos: i64,
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
) -> Result<CoarseInterval, MarketDataLoadingError> {
    match (row.interval_start_utc_nanos, row.interval_end_utc_nanos) {
        (Some(start), Some(end)) => {
            if end <= start {
                return Err(MarketDataLoadingError::InvalidValue {
                    field_path: format!(
                        "coarse_training_input.source.rows[{}]",
                        row.source_row_number
                    ),
                    message: "interval_end_column must be later than interval_start_column"
                        .to_string(),
                });
            }

            return Ok(CoarseInterval {
                interval_start_utc_nanos: start,
                interval_end_utc_nanos: end,
            });
        }
        (Some(_), None) | (None, Some(_)) => {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: format!(
                    "coarse_training_input.source.rows[{}]",
                    row.source_row_number
                ),
                message: "coarse interval columns must provide both start and end".to_string(),
            });
        }
        (None, None) => {}
    }

    let nominal_start = row
        .timestamp_utc_nanos
        .checked_sub(nominal_interval_nanos)
        .ok_or_else(|| MarketDataLoadingError::InvalidValue {
            field_path: format!(
                "coarse_training_input.source.rows[{}]",
                row.source_row_number
            ),
            message: "timestamp minus inferred coarse interval is outside i64 range".to_string(),
        })?;

    let exposure = calendar_registry
        .resolve_market_state_exposure_in_utc_interval(
            exchange_code,
            nominal_start,
            row.timestamp_utc_nanos,
        )
        .map_err(|source| MarketDataLoadingError::InvalidValue {
            field_path: format!(
                "coarse_training_input.source.rows[{}]",
                row.source_row_number
            ),
            message: format!("market-session interval exposure failed: {source}"),
        })?;

    let effective_duration_nanos =
        if exposure.open_window_nanos > 0 && exposure.open_window_nanos < nominal_interval_nanos {
            exposure.open_window_nanos
        } else {
            nominal_interval_nanos
        };

    Ok(CoarseInterval {
        interval_start_utc_nanos: row
            .timestamp_utc_nanos
            .checked_sub(effective_duration_nanos)
            .ok_or_else(|| MarketDataLoadingError::InvalidValue {
                field_path: format!(
                    "coarse_training_input.source.rows[{}]",
                    row.source_row_number
                ),
                message: "timestamp minus effective coarse interval is outside i64 range"
                    .to_string(),
            })?,
        interval_end_utc_nanos: row.timestamp_utc_nanos,
    })
}

pub(super) fn validate_coarse_record_intervals(
    records: &[MarketDataNormalizedCoarseTrainingRecord],
) -> Result<(), MarketDataLoadingError> {
    let mut previous_interval_end: Option<i64> = None;

    for record in records {
        if record.interval_end_utc_nanos <= record.interval_start_utc_nanos {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: format!(
                    "coarse_training_input.source.rows[{}]",
                    record.source_row_number
                ),
                message: "coarse interval end must be later than interval start".to_string(),
            });
        }

        if previous_interval_end.is_some_and(|end| record.interval_start_utc_nanos < end) {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: format!(
                    "coarse_training_input.source.rows[{}]",
                    record.source_row_number
                ),
                message: "coarse intervals must not overlap after timestamp normalization"
                    .to_string(),
            });
        }

        previous_interval_end = Some(record.interval_end_utc_nanos);
    }

    Ok(())
}

pub(super) fn infer_nominal_interval_nanos(
    loaded_rows: &[LoadedCoarseRow],
) -> Result<i64, MarketDataLoadingError> {
    let mut positive_deltas = loaded_rows
        .windows(2)
        .filter_map(|window| {
            let previous = &window[0];
            let current = &window[1];
            current
                .timestamp_utc_nanos
                .checked_sub(previous.timestamp_utc_nanos)
                .filter(|delta| *delta > 0)
        })
        .collect::<Vec<_>>();

    if positive_deltas.is_empty() {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "coarse_training_input.source".to_string(),
            message:
                "requires at least two strictly increasing timestamps to infer a fixed interval"
                    .to_string(),
        });
    }

    positive_deltas.sort_unstable();

    Ok(positive_deltas[(positive_deltas.len() - 1) / 2])
}

pub(super) fn infer_explicit_nominal_interval_nanos(
    loaded_rows: &[LoadedCoarseRow],
) -> Result<i64, MarketDataLoadingError> {
    let mut positive_durations = Vec::new();

    for row in loaded_rows {
        let (Some(start), Some(end)) = (row.interval_start_utc_nanos, row.interval_end_utc_nanos)
        else {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: format!(
                    "coarse_training_input.source.rows[{}]",
                    row.source_row_number
                ),
                message: "coarse interval columns must provide both start and end".to_string(),
            });
        };

        if end <= start {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: format!(
                    "coarse_training_input.source.rows[{}]",
                    row.source_row_number
                ),
                message: "interval_end_column must be later than interval_start_column".to_string(),
            });
        }

        positive_durations.push(end - start);
    }

    if positive_durations.is_empty() {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "coarse_training_input.source".to_string(),
            message: "requires at least one explicit coarse interval".to_string(),
        });
    }

    positive_durations.sort_unstable();

    Ok(positive_durations[(positive_durations.len() - 1) / 2])
}

pub(super) fn timestamp_semantics(explicit_intervals: bool) -> &'static str {
    if explicit_intervals {
        return COARSE_EXPLICIT_INTERVAL_SEMANTICS;
    }

    COARSE_TIMESTAMP_CLOSE_SEMANTICS
}

pub(super) fn interval_inference_policy(explicit_intervals: bool) -> &'static str {
    if explicit_intervals {
        return COARSE_EXPLICIT_INTERVAL_POLICY;
    }

    COARSE_FIXED_INTERVAL_INFERENCE_POLICY
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(timestamp_utc_nanos: i64) -> LoadedCoarseRow {
        LoadedCoarseRow {
            timestamp_utc_nanos,
            interval_start_utc_nanos: None,
            interval_end_utc_nanos: None,
            open_price: None,
            high_price: None,
            low_price: None,
            close_price: 100.0,
            volume: None,
            source_row_number: 1,
        }
    }

    fn record(
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
        source_row_number: usize,
    ) -> MarketDataNormalizedCoarseTrainingRecord {
        MarketDataNormalizedCoarseTrainingRecord {
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            observation_timestamp_utc_nanos: interval_end_utc_nanos,
            close_price: 100.0,
            open_price: None,
            high_price: None,
            low_price: None,
            volume: None,
            close_to_close_log_return: None,
            open_to_close_log_return: None,
            close_to_open_gap_log_return: None,
            high_low_log_range: None,
            source_row_number,
        }
    }

    #[test]
    fn inferred_nominal_interval_uses_lower_median_spacing_not_data_gaps() {
        let rows = vec![row(1_000), row(2_000), row(3_000), row(10_000)];

        let inferred = infer_nominal_interval_nanos(rows.as_slice())
            .expect("test coarse rows should infer an interval");

        assert_eq!(inferred, 1_000);
    }

    #[test]
    fn inferred_nominal_interval_floors_even_spacing_ties() {
        let rows = vec![row(1_000), row(2_000), row(4_000)];

        let inferred = infer_nominal_interval_nanos(rows.as_slice())
            .expect("test coarse rows should infer an interval");

        assert_eq!(inferred, 1_000);
    }

    #[test]
    fn explicit_nominal_interval_accepts_single_explicit_bar() {
        let mut rows = vec![row(2_000)];
        rows[0].interval_start_utc_nanos = Some(1_000);
        rows[0].interval_end_utc_nanos = Some(2_000);

        let inferred = infer_explicit_nominal_interval_nanos(rows.as_slice())
            .expect("explicit coarse interval should infer from one row");

        assert_eq!(inferred, 1_000);
    }

    #[test]
    fn normalized_coarse_intervals_reject_overlap() {
        let records = vec![record(1_000, 3_000, 1), record(2_500, 4_000, 2)];

        let error = validate_coarse_record_intervals(records.as_slice())
            .expect_err("overlapping intervals should be rejected");

        match error {
            MarketDataLoadingError::InvalidValue {
                field_path,
                message,
            } => {
                assert_eq!(field_path, "coarse_training_input.source.rows[2]");
                assert_eq!(
                    message,
                    "coarse intervals must not overlap after timestamp normalization"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
