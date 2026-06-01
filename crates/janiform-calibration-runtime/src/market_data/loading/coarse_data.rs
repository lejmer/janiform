use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::coarse_intervals::{
    infer_explicit_nominal_interval_nanos, infer_nominal_interval_nanos, interval_inference_policy,
    resolve_row_interval, timestamp_semantics, validate_coarse_record_intervals,
};
use super::column_resolution::{resolve_coarse_data_column_mapping, resolved_coarse_data_kind};
use super::contracts::{
    MarketDataCoarseDataColumnMapping, MarketDataCoarseDataKind, MarketDataCoarseDataLoadingInput,
    MarketDataCoarseDataSummary, MarketDataCoarseDataTimestampSemantics, MarketDataInputFileFormat,
    MarketDataLoadingError, MarketDataNormalizedCoarseDataRecord, MarketDataSourceSummary,
};
use super::normalization::{normalize_optional_numeric_value, normalize_timestamp_value};
use super::progress::{
    MarketDataLoadingProgressContext, MarketDataLoadingProgressEvent,
    MarketDataLoadingProgressPhase, MarketDataLoadingSource,
};
use super::rows::{
    load_rows_with_progress, load_source_columns, probe_source_row_count_estimate,
    validate_columns_exist,
};
use super::shared::{RawRow, dedup_vec, file_format_string};

#[derive(Debug, Clone)]
pub(super) struct CoarseDataLoadOutput {
    pub(super) records: Vec<MarketDataNormalizedCoarseDataRecord>,
    pub(super) summary: MarketDataCoarseDataSummary,
}

#[derive(Debug, Clone)]
pub(super) struct LoadedCoarseRow {
    pub(super) timestamp_utc_nanos: i64,
    pub(super) interval_start_utc_nanos: Option<i64>,
    pub(super) interval_end_utc_nanos: Option<i64>,
    pub(super) open_price: Option<f64>,
    pub(super) high_price: Option<f64>,
    pub(super) low_price: Option<f64>,
    pub(super) close_price: f64,
    pub(super) volume: Option<f64>,
    pub(super) source_row_number: usize,
}

pub(super) fn load_coarse_data_records(
    input: &MarketDataCoarseDataLoadingInput,
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    progress: &mut MarketDataLoadingProgressContext<'_>,
) -> Result<CoarseDataLoadOutput, MarketDataLoadingError> {
    let available_columns = load_source_columns(&input.source, "coarse_data_input.source")?;

    let resolved_column_mapping = resolve_coarse_data_column_mapping(
        available_columns.as_slice(),
        &input.column_mapping,
        &input.source.path,
    )?;

    let required_columns = required_coarse_data_source_columns(&resolved_column_mapping);

    validate_columns_exist(
        &available_columns,
        &required_columns,
        "column_mapping.coarse_data",
        &input.source.path,
    )?;

    let raw_rows = load_rows_with_progress(
        &input.source,
        "coarse_data_input.source",
        progress,
        MarketDataLoadingSource::CoarseData,
    )?;

    let source_row_count = source_row_count(&input.source, raw_rows.len())?;

    let mut loaded_rows = normalize_coarse_rows(
        raw_rows.as_slice(),
        &resolved_column_mapping,
        &input.source.path,
        progress,
    )?;

    loaded_rows.sort_by_key(|row| (row.timestamp_utc_nanos, row.source_row_number));

    let inferred_kind = resolved_coarse_data_kind(&resolved_column_mapping);

    let explicit_intervals = resolved_column_mapping.interval_start_column.is_some()
        && resolved_column_mapping.interval_end_column.is_some();

    let nominal_interval_nanos = if explicit_intervals {
        infer_explicit_nominal_interval_nanos(loaded_rows.as_slice())?
    } else {
        infer_nominal_interval_nanos(loaded_rows.as_slice())?
    };

    let records = materialize_coarse_records(
        loaded_rows.as_slice(),
        inferred_kind,
        nominal_interval_nanos,
        resolved_column_mapping.timestamp_semantics,
        calendar_registry,
        exchange_code,
    )?;

    validate_coarse_record_intervals(records.as_slice())?;

    let summary_timestamp_semantics = timestamp_semantics(
        explicit_intervals,
        resolved_column_mapping.timestamp_semantics,
    )
    .to_string();

    let summary = MarketDataCoarseDataSummary {
        source_summary: MarketDataSourceSummary {
            path: input.source.path.clone(),
            file_format: file_format_string(input.source.file_format).to_string(),
            provenance_label: input.source.provenance_label.clone(),
            source_row_count,
            retained_row_count: records.len(),
            dropped_duplicate_row_count: 0,
            dropped_invalid_row_count: 0,
            sort_records_by_timestamp: true,
            applied_timestamp_timezone: None,
            canonical_value_field: Some("close_price".to_string()),
            value_mapping_policy: Some(coarse_kind_label(inferred_kind).to_string()),
        },
        inferred_kind,
        resolved_column_mapping,
        timestamp_semantics: summary_timestamp_semantics,
        interval_inference_policy: interval_inference_policy(explicit_intervals).to_string(),
        nominal_interval_nanos,
        retained_return_interval_count: records
            .iter()
            .filter(|record| record.close_to_close_log_return.is_some())
            .count(),
        retained_mean_reversion_observation_count: records.len(),
    };

    let output = CoarseDataLoadOutput { records, summary };

    progress.emit(MarketDataLoadingProgressEvent::new(
        MarketDataLoadingSource::CoarseData,
        MarketDataLoadingProgressPhase::Completed,
        source_row_count,
        Some(source_row_count),
    ))?;

    Ok(output)
}

fn source_row_count(
    source: &super::contracts::MarketDataInputFileReference,
    loaded_row_count: usize,
) -> Result<usize, MarketDataLoadingError> {
    match source.file_format {
        MarketDataInputFileFormat::Csv => Ok(loaded_row_count),
        MarketDataInputFileFormat::Parquet => Ok(probe_source_row_count_estimate(
            source,
            "coarse_data_input.source",
        )?
        .unwrap_or(loaded_row_count)),
        MarketDataInputFileFormat::Json => Err(MarketDataLoadingError::InvalidValue {
            field_path: "coarse_data_input.source.file_format".to_string(),
            message: "must be one of: csv, parquet".to_string(),
        }),
    }
}

fn normalize_coarse_rows(
    raw_rows: &[RawRow],
    column_mapping: &MarketDataCoarseDataColumnMapping,
    source_path: &str,
    progress: &mut MarketDataLoadingProgressContext<'_>,
) -> Result<Vec<LoadedCoarseRow>, MarketDataLoadingError> {
    let mut rows = Vec::with_capacity(raw_rows.len());

    progress.emit(MarketDataLoadingProgressEvent::new(
        MarketDataLoadingSource::CoarseData,
        MarketDataLoadingProgressPhase::NormalizingRecords,
        0,
        Some(raw_rows.len()),
    ))?;

    for (row_position, raw_row) in raw_rows.iter().enumerate() {
        let source_row_number = row_position + 1;

        rows.push(normalize_coarse_row(
            raw_row,
            source_row_number,
            column_mapping,
            source_path,
        )?);

        progress.emit(MarketDataLoadingProgressEvent::new(
            MarketDataLoadingSource::CoarseData,
            MarketDataLoadingProgressPhase::NormalizingRecords,
            source_row_number,
            Some(raw_rows.len()),
        ))?;
    }

    Ok(rows)
}

fn normalize_coarse_row(
    raw_row: &RawRow,
    source_row_number: usize,
    column_mapping: &MarketDataCoarseDataColumnMapping,
    source_path: &str,
) -> Result<LoadedCoarseRow, MarketDataLoadingError> {
    let timestamp_column = column_mapping.timestamp_column.as_deref().ok_or_else(|| {
        MarketDataLoadingError::InvalidValue {
            field_path: "column_mapping.coarse_data.timestamp_column".to_string(),
            message: "could not resolve a coarse timestamp column".to_string(),
        }
    })?;

    let close_column = column_mapping.close_column.as_deref().ok_or_else(|| {
        MarketDataLoadingError::InvalidValue {
            field_path: "column_mapping.coarse_data.close_column".to_string(),
            message: "could not resolve a coarse close column".to_string(),
        }
    })?;

    Ok(LoadedCoarseRow {
        timestamp_utc_nanos: normalize_timestamp_value(
            raw_row.get(timestamp_column),
            &format!("coarse_data_input.source.rows[{source_row_number}].{timestamp_column}"),
            None,
            source_path,
        )?,
        interval_start_utc_nanos: optional_timestamp(
            raw_row,
            column_mapping.interval_start_column.as_deref(),
            source_row_number,
            source_path,
        )?,
        interval_end_utc_nanos: optional_timestamp(
            raw_row,
            column_mapping.interval_end_column.as_deref(),
            source_row_number,
            source_path,
        )?,
        open_price: optional_price(
            raw_row,
            column_mapping.open_column.as_deref(),
            source_row_number,
            source_path,
        )?,
        high_price: optional_price(
            raw_row,
            column_mapping.high_column.as_deref(),
            source_row_number,
            source_path,
        )?,
        low_price: optional_price(
            raw_row,
            column_mapping.low_column.as_deref(),
            source_row_number,
            source_path,
        )?,
        close_price: required_price(raw_row, close_column, source_row_number, source_path)?,
        volume: optional_volume(
            raw_row,
            column_mapping.volume_column.as_deref(),
            source_row_number,
            source_path,
        )?,
        source_row_number,
    })
}

fn optional_timestamp(
    raw_row: &RawRow,
    column_name: Option<&str>,
    source_row_number: usize,
    source_path: &str,
) -> Result<Option<i64>, MarketDataLoadingError> {
    let Some(column_name) = column_name else {
        return Ok(None);
    };

    normalize_timestamp_value(
        raw_row.get(column_name),
        &format!("coarse_data_input.source.rows[{source_row_number}].{column_name}"),
        None,
        source_path,
    )
    .map(Some)
}

fn required_price(
    raw_row: &RawRow,
    column_name: &str,
    source_row_number: usize,
    source_path: &str,
) -> Result<f64, MarketDataLoadingError> {
    normalize_optional_numeric_value(
        raw_row.get(column_name),
        &format!("coarse_data_input.source.rows[{source_row_number}].{column_name}"),
        source_path,
        false,
        false,
    )?
    .ok_or_else(|| MarketDataLoadingError::InvalidValue {
        field_path: format!("coarse_data_input.source.rows[{source_row_number}].{column_name}"),
        message: format!("must contain a strictly positive value. source_path={source_path}"),
    })
}

fn optional_price(
    raw_row: &RawRow,
    column_name: Option<&str>,
    source_row_number: usize,
    source_path: &str,
) -> Result<Option<f64>, MarketDataLoadingError> {
    let Some(column_name) = column_name else {
        return Ok(None);
    };

    normalize_optional_numeric_value(
        raw_row.get(column_name),
        &format!("coarse_data_input.source.rows[{source_row_number}].{column_name}"),
        source_path,
        false,
        false,
    )
}

fn optional_volume(
    raw_row: &RawRow,
    column_name: Option<&str>,
    source_row_number: usize,
    source_path: &str,
) -> Result<Option<f64>, MarketDataLoadingError> {
    let Some(column_name) = column_name else {
        return Ok(None);
    };

    normalize_optional_numeric_value(
        raw_row.get(column_name),
        &format!("coarse_data_input.source.rows[{source_row_number}].{column_name}"),
        source_path,
        true,
        false,
    )
}

fn materialize_coarse_records(
    loaded_rows: &[LoadedCoarseRow],
    inferred_kind: MarketDataCoarseDataKind,
    nominal_interval_nanos: i64,
    timestamp_semantics: MarketDataCoarseDataTimestampSemantics,
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
) -> Result<Vec<MarketDataNormalizedCoarseDataRecord>, MarketDataLoadingError> {
    let mut previous_close_price: Option<f64> = None;
    let mut records = Vec::with_capacity(loaded_rows.len());

    for row in loaded_rows {
        let interval = resolve_row_interval(
            row,
            nominal_interval_nanos,
            timestamp_semantics,
            calendar_registry,
            exchange_code,
        )?;

        validate_ohlc_shape(row, inferred_kind)?;

        let close_to_close_log_return = previous_close_price
            .map(|previous_close_price| (row.close_price / previous_close_price).ln());

        let open_to_close_log_return = row
            .open_price
            .map(|open_price| (row.close_price / open_price).ln());

        let close_to_open_gap_log_return = match (previous_close_price, row.open_price) {
            (Some(previous_close_price), Some(open_price)) => {
                Some((open_price / previous_close_price).ln())
            }
            _ => None,
        };

        let high_low_log_range = match (row.high_price, row.low_price) {
            (Some(high_price), Some(low_price)) => Some((high_price / low_price).ln()),
            _ => None,
        };

        records.push(MarketDataNormalizedCoarseDataRecord {
            interval_start_utc_nanos: interval.interval_start_utc_nanos,
            interval_end_utc_nanos: interval.interval_end_utc_nanos,
            observation_timestamp_utc_nanos: row.timestamp_utc_nanos,
            close_price: row.close_price,
            open_price: row.open_price,
            high_price: row.high_price,
            low_price: row.low_price,
            volume: row.volume,
            close_to_close_log_return,
            open_to_close_log_return,
            close_to_open_gap_log_return,
            high_low_log_range,
            source_row_number: row.source_row_number,
        });

        previous_close_price = Some(row.close_price);
    }

    Ok(records)
}

fn validate_ohlc_shape(
    row: &LoadedCoarseRow,
    inferred_kind: MarketDataCoarseDataKind,
) -> Result<(), MarketDataLoadingError> {
    if inferred_kind != MarketDataCoarseDataKind::Ohlcv {
        return Ok(());
    }

    let (Some(open_price), Some(high_price), Some(low_price)) =
        (row.open_price, row.high_price, row.low_price)
    else {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: format!("coarse_data_input.source.rows[{}]", row.source_row_number),
            message: "OHLCV coarse data requires open, high, low, and close values".to_string(),
        });
    };

    if low_price > open_price
        || low_price > row.close_price
        || high_price < open_price
        || high_price < row.close_price
        || high_price < low_price
    {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: format!("coarse_data_input.source.rows[{}]", row.source_row_number),
            message: "OHLC prices must satisfy low <= open, close <= high".to_string(),
        });
    }

    Ok(())
}

fn coarse_kind_label(kind: MarketDataCoarseDataKind) -> &'static str {
    match kind {
        MarketDataCoarseDataKind::Ohlcv => "ohlcv",
        MarketDataCoarseDataKind::CloseOnly => "close_only",
    }
}

fn required_coarse_data_source_columns(
    column_mapping: &MarketDataCoarseDataColumnMapping,
) -> Vec<String> {
    let mut columns = Vec::new();

    if let Some(column) = &column_mapping.timestamp_column {
        columns.push(column.clone());
    }

    if let Some(column) = &column_mapping.interval_start_column {
        columns.push(column.clone());
    }

    if let Some(column) = &column_mapping.interval_end_column {
        columns.push(column.clone());
    }

    if let Some(column) = &column_mapping.open_column {
        columns.push(column.clone());
    }

    if let Some(column) = &column_mapping.high_column {
        columns.push(column.clone());
    }

    if let Some(column) = &column_mapping.low_column {
        columns.push(column.clone());
    }

    if let Some(column) = &column_mapping.close_column {
        columns.push(column.clone());
    }

    if let Some(column) = &column_mapping.volume_column {
        columns.push(column.clone());
    }

    dedup_vec(columns)
}
