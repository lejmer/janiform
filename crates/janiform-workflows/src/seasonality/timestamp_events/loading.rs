use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use arrow_array::{
    Array, Int32Array, Int64Array, LargeStringArray, RecordBatch, StringArray,
    TimestampMicrosecondArray, TimestampMillisecondArray, TimestampNanosecondArray,
    TimestampSecondArray, UInt32Array, UInt64Array,
};
use chrono::{DateTime, Utc};
use csv::StringRecord;
use janiform::calendar::TimestampEventInputRecord;
use janiform_foundations::time::{
    NANOS_PER_MICROSECOND, NANOS_PER_MILLISECOND, NANOS_PER_SECOND, parse_utc_datetime,
};
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::{ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder};
use serde_json::Value;

use super::contracts::{
    TimestampEventSource, TimestampEventSourceFormat, TimestampEventSourceLoadOutput,
    TimestampEventTimestampUnit,
};
use super::error::TimestampEventSourceLoadError;

// IO chunk size only. This does not affect numerical behavior or output contracts.
const PARQUET_BATCH_SIZE: usize = 65_536;

/// Load timestamp-event records from a CSV, parquet, or JSONL source.
pub fn load_timestamp_event_records(
    source: TimestampEventSource,
) -> Result<TimestampEventSourceLoadOutput, TimestampEventSourceLoadError> {
    let records = match source.file_format {
        TimestampEventSourceFormat::Csv => load_csv_timestamp_events(&source)?,
        TimestampEventSourceFormat::Parquet => load_parquet_timestamp_events(&source)?,
        TimestampEventSourceFormat::Jsonl => load_jsonl_timestamp_events(&source)?,
    };

    Ok(TimestampEventSourceLoadOutput {
        source_record_count: records.len(),
        records,
        path: source.path,
        file_format: source.file_format,
        timestamp_column: source.timestamp_column,
        timestamp_unit: source.timestamp_unit,
    })
}

fn load_csv_timestamp_events(
    source: &TimestampEventSource,
) -> Result<Vec<TimestampEventInputRecord>, TimestampEventSourceLoadError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(&source.path)
        .map_err(|source_error| TimestampEventSourceLoadError::Csv {
            path: source.path.clone(),
            message: source_error.to_string(),
        })?;

    let headers = reader
        .headers()
        .map_err(|source_error| TimestampEventSourceLoadError::Csv {
            path: source.path.clone(),
            message: source_error.to_string(),
        })?
        .clone();

    let timestamp_column_position =
        timestamp_column_position(&headers, &source.path, &source.timestamp_column)?;

    let mut records = Vec::new();

    for (source_record_position, record_result) in reader.records().enumerate() {
        let record = record_result.map_err(|source_error| TimestampEventSourceLoadError::Csv {
            path: source.path.clone(),
            message: source_error.to_string(),
        })?;

        let timestamp_value = record.get(timestamp_column_position).unwrap_or("");

        let timestamp_utc = parse_timestamp_text(
            timestamp_value,
            source.timestamp_unit,
            &source.path,
            source_record_position,
        )?;

        records.push(TimestampEventInputRecord {
            source_record_position: source_record_position as u64,
            timestamp_utc,
        });
    }

    Ok(records)
}

fn timestamp_column_position(
    headers: &StringRecord,
    path: &Path,
    timestamp_column: &str,
) -> Result<usize, TimestampEventSourceLoadError> {
    headers
        .iter()
        .position(|header| header == timestamp_column)
        .ok_or_else(|| TimestampEventSourceLoadError::MissingTimestampColumn {
            path: path.to_path_buf(),
            timestamp_column: timestamp_column.to_string(),
        })
}

fn load_jsonl_timestamp_events(
    source: &TimestampEventSource,
) -> Result<Vec<TimestampEventInputRecord>, TimestampEventSourceLoadError> {
    let file =
        File::open(&source.path).map_err(|source_error| TimestampEventSourceLoadError::Io {
            path: source.path.clone(),
            message: source_error.to_string(),
        })?;

    let reader = BufReader::new(file);

    let mut records = Vec::new();

    for (source_record_position, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|source_error| TimestampEventSourceLoadError::Io {
            path: source.path.clone(),
            message: source_error.to_string(),
        })?;
        if line.trim().is_empty() {
            continue;
        }

        let value: Value = serde_json::from_str(&line).map_err(|source_error| {
            TimestampEventSourceLoadError::Jsonl {
                path: source.path.clone(),
                message: format!(
                    "record {source_record_position} must be a JSON object: {source_error}"
                ),
            }
        })?;

        let object = value
            .as_object()
            .ok_or_else(|| TimestampEventSourceLoadError::Jsonl {
                path: source.path.clone(),
                message: format!("record {source_record_position} must be a JSON object"),
            })?;

        let timestamp_value = object.get(&source.timestamp_column).ok_or_else(|| {
            TimestampEventSourceLoadError::MissingTimestampColumn {
                path: source.path.clone(),
                timestamp_column: source.timestamp_column.clone(),
            }
        })?;

        let timestamp_utc = parse_timestamp_json_value(
            timestamp_value,
            source.timestamp_unit,
            &source.path,
            source_record_position,
        )?;

        records.push(TimestampEventInputRecord {
            source_record_position: source_record_position as u64,
            timestamp_utc,
        });
    }

    Ok(records)
}

fn load_parquet_timestamp_events(
    source: &TimestampEventSource,
) -> Result<Vec<TimestampEventInputRecord>, TimestampEventSourceLoadError> {
    let mut reader = projected_parquet_reader(source)?;
    let mut records = Vec::new();
    let mut source_record_position = 0_usize;

    for batch_result in &mut reader {
        let batch =
            batch_result.map_err(|source_error| TimestampEventSourceLoadError::Parquet {
                path: source.path.clone(),
                message: source_error.to_string(),
            })?;

        let column = projected_timestamp_column(&batch, source)?;

        for batch_row_position in 0..batch.num_rows() {
            let timestamp_utc = parse_timestamp_arrow_value(
                column,
                batch_row_position,
                source.timestamp_unit,
                &source.path,
                source_record_position,
            )?;

            records.push(TimestampEventInputRecord {
                source_record_position: source_record_position as u64,
                timestamp_utc,
            });

            source_record_position += 1;
        }
    }

    Ok(records)
}

fn projected_parquet_reader(
    source: &TimestampEventSource,
) -> Result<ParquetRecordBatchReader, TimestampEventSourceLoadError> {
    let file =
        File::open(&source.path).map_err(|source_error| TimestampEventSourceLoadError::Io {
            path: source.path.clone(),
            message: source_error.to_string(),
        })?;

    let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|source_error| {
        TimestampEventSourceLoadError::Parquet {
            path: source.path.clone(),
            message: source_error.to_string(),
        }
    })?;

    let parquet_schema = builder.metadata().file_metadata().schema_descr_ptr();

    let projection =
        ProjectionMask::columns(parquet_schema.as_ref(), [source.timestamp_column.as_str()]);

    builder
        .with_projection(projection)
        .with_batch_size(PARQUET_BATCH_SIZE)
        .build()
        .map_err(|source_error| TimestampEventSourceLoadError::Parquet {
            path: source.path.clone(),
            message: source_error.to_string(),
        })
}

fn projected_timestamp_column<'a>(
    batch: &'a RecordBatch,
    source: &TimestampEventSource,
) -> Result<&'a dyn Array, TimestampEventSourceLoadError> {
    let column_position = batch
        .schema_ref()
        .index_of(&source.timestamp_column)
        .map_err(|_| TimestampEventSourceLoadError::MissingTimestampColumn {
            path: source.path.clone(),
            timestamp_column: source.timestamp_column.clone(),
        })?;

    Ok(batch.column(column_position).as_ref())
}

fn parse_timestamp_json_value(
    value: &Value,
    timestamp_unit: TimestampEventTimestampUnit,
    path: &Path,
    source_record_position: usize,
) -> Result<DateTime<Utc>, TimestampEventSourceLoadError> {
    match value {
        Value::String(text) => {
            parse_timestamp_text(text, timestamp_unit, path, source_record_position)
        }
        Value::Number(number) => {
            let raw_value = number.as_i64().ok_or_else(|| {
                TimestampEventSourceLoadError::InvalidTimestampValue {
                    path: path.to_path_buf(),
                    source_record_position,
                    message: "numeric timestamp must fit in signed 64-bit integer range"
                        .to_string(),
                }
            })?;

            datetime_from_timestamp_integer(raw_value, timestamp_unit, path, source_record_position)
        }
        _ => Err(TimestampEventSourceLoadError::InvalidTimestampValue {
            path: path.to_path_buf(),
            source_record_position,
            message: "timestamp value must be a string or integer".to_string(),
        }),
    }
}

fn parse_timestamp_arrow_value(
    array: &dyn Array,
    row_position: usize,
    timestamp_unit: TimestampEventTimestampUnit,
    path: &Path,
    source_record_position: usize,
) -> Result<DateTime<Utc>, TimestampEventSourceLoadError> {
    if array.is_null(row_position) {
        return Err(TimestampEventSourceLoadError::MissingTimestampValue {
            path: path.to_path_buf(),
            source_record_position,
        });
    }

    if let Some(values) = array.as_any().downcast_ref::<StringArray>() {
        return parse_timestamp_text(
            values.value(row_position),
            timestamp_unit,
            path,
            source_record_position,
        );
    }

    if let Some(values) = array.as_any().downcast_ref::<LargeStringArray>() {
        return parse_timestamp_text(
            values.value(row_position),
            timestamp_unit,
            path,
            source_record_position,
        );
    }

    if let Some(values) = array.as_any().downcast_ref::<TimestampSecondArray>() {
        return datetime_from_timestamp_integer(
            values.value(row_position),
            TimestampEventTimestampUnit::Seconds,
            path,
            source_record_position,
        );
    }

    if let Some(values) = array.as_any().downcast_ref::<TimestampMillisecondArray>() {
        return datetime_from_timestamp_integer(
            values.value(row_position),
            TimestampEventTimestampUnit::Milliseconds,
            path,
            source_record_position,
        );
    }

    if let Some(values) = array.as_any().downcast_ref::<TimestampMicrosecondArray>() {
        return datetime_from_timestamp_integer(
            values.value(row_position),
            TimestampEventTimestampUnit::Microseconds,
            path,
            source_record_position,
        );
    }

    if let Some(values) = array.as_any().downcast_ref::<TimestampNanosecondArray>() {
        return datetime_from_timestamp_integer(
            values.value(row_position),
            TimestampEventTimestampUnit::Nanoseconds,
            path,
            source_record_position,
        );
    }

    if let Some(values) = array.as_any().downcast_ref::<Int32Array>() {
        return datetime_from_timestamp_integer(
            i64::from(values.value(row_position)),
            timestamp_unit,
            path,
            source_record_position,
        );
    }

    if let Some(values) = array.as_any().downcast_ref::<Int64Array>() {
        return datetime_from_timestamp_integer(
            values.value(row_position),
            timestamp_unit,
            path,
            source_record_position,
        );
    }

    if let Some(values) = array.as_any().downcast_ref::<UInt32Array>() {
        return datetime_from_timestamp_integer(
            i64::from(values.value(row_position)),
            timestamp_unit,
            path,
            source_record_position,
        );
    }

    if let Some(values) = array.as_any().downcast_ref::<UInt64Array>() {
        let raw_value = i64::try_from(values.value(row_position)).map_err(|_| {
            TimestampEventSourceLoadError::InvalidTimestampValue {
                path: path.to_path_buf(),
                source_record_position,
                message: "numeric timestamp must fit in signed 64-bit integer range".to_string(),
            }
        })?;

        return datetime_from_timestamp_integer(
            raw_value,
            timestamp_unit,
            path,
            source_record_position,
        );
    }

    Err(
        TimestampEventSourceLoadError::UnsupportedTimestampColumnType {
            path: path.to_path_buf(),
            data_type: array.data_type().to_string(),
        },
    )
}

fn parse_timestamp_text(
    value: &str,
    timestamp_unit: TimestampEventTimestampUnit,
    path: &Path,
    source_record_position: usize,
) -> Result<DateTime<Utc>, TimestampEventSourceLoadError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(TimestampEventSourceLoadError::MissingTimestampValue {
            path: path.to_path_buf(),
            source_record_position,
        });
    }

    match timestamp_unit {
        TimestampEventTimestampUnit::Rfc3339 => parse_utc_datetime(trimmed).map_err(|source| {
            TimestampEventSourceLoadError::InvalidTimestampValue {
                path: path.to_path_buf(),
                source_record_position,
                message: format!("must be a valid RFC 3339 timestamp: {source}"),
            }
        }),
        TimestampEventTimestampUnit::Seconds
        | TimestampEventTimestampUnit::Milliseconds
        | TimestampEventTimestampUnit::Microseconds
        | TimestampEventTimestampUnit::Nanoseconds => {
            let raw_value = trimmed.parse::<i64>().map_err(|source| {
                TimestampEventSourceLoadError::InvalidTimestampValue {
                    path: path.to_path_buf(),
                    source_record_position,
                    message: format!("must be an integer timestamp: {source}"),
                }
            })?;

            datetime_from_timestamp_integer(raw_value, timestamp_unit, path, source_record_position)
        }
    }
}

fn datetime_from_timestamp_integer(
    raw_value: i64,
    timestamp_unit: TimestampEventTimestampUnit,
    path: &Path,
    source_record_position: usize,
) -> Result<DateTime<Utc>, TimestampEventSourceLoadError> {
    let timestamp_nanos =
        timestamp_nanos_from_integer(raw_value, timestamp_unit).ok_or_else(|| {
            TimestampEventSourceLoadError::TimestampOutOfRange {
                path: path.to_path_buf(),
                source_record_position,
            }
        })?;

    let seconds = timestamp_nanos.div_euclid(NANOS_PER_SECOND);
    let nanos = timestamp_nanos.rem_euclid(NANOS_PER_SECOND) as u32;

    DateTime::from_timestamp(seconds, nanos).ok_or_else(|| {
        TimestampEventSourceLoadError::TimestampOutOfRange {
            path: path.to_path_buf(),
            source_record_position,
        }
    })
}

fn timestamp_nanos_from_integer(
    raw_value: i64,
    timestamp_unit: TimestampEventTimestampUnit,
) -> Option<i64> {
    match timestamp_unit {
        TimestampEventTimestampUnit::Rfc3339 => None,
        TimestampEventTimestampUnit::Seconds => raw_value.checked_mul(NANOS_PER_SECOND),
        TimestampEventTimestampUnit::Milliseconds => raw_value.checked_mul(NANOS_PER_MILLISECOND),
        TimestampEventTimestampUnit::Microseconds => raw_value.checked_mul(NANOS_PER_MICROSECOND),
        TimestampEventTimestampUnit::Nanoseconds => Some(raw_value),
    }
}
