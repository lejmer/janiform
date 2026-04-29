use std::path::PathBuf;

use janiform::calendar::TimestampEventInputRecord;

/// Supported timestamp-event source file formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimestampEventSourceFormat {
    /// Headered CSV file.
    Csv,
    /// Columnar parquet file.
    Parquet,
    /// Newline-delimited JSON records.
    Jsonl,
}

impl TimestampEventSourceFormat {
    /// Stable format label used in diagnostics and payloads.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Parquet => "parquet",
            Self::Jsonl => "jsonl",
        }
    }
}

/// Timestamp interpretation for non-logical timestamp columns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimestampEventTimestampUnit {
    /// RFC 3339 timestamp strings.
    Rfc3339,
    /// Unix timestamp in seconds.
    Seconds,
    /// Unix timestamp in milliseconds.
    Milliseconds,
    /// Unix timestamp in microseconds.
    Microseconds,
    /// Unix timestamp in nanoseconds.
    Nanoseconds,
}

impl TimestampEventTimestampUnit {
    /// Stable unit label used in diagnostics and payloads.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rfc3339 => "rfc3339",
            Self::Seconds => "seconds",
            Self::Milliseconds => "milliseconds",
            Self::Microseconds => "microseconds",
            Self::Nanoseconds => "nanoseconds",
        }
    }
}

/// Timestamp-event source specification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimestampEventSource {
    /// Source file path.
    pub path: PathBuf,
    /// Source file format.
    pub file_format: TimestampEventSourceFormat,
    /// Source column or JSON field containing event timestamps.
    pub timestamp_column: String,
    /// Explicit unit for textual or numeric timestamp values.
    pub timestamp_unit: TimestampEventTimestampUnit,
}

/// Loaded timestamp-event records plus source diagnostics.
#[derive(Clone, Debug, PartialEq)]
pub struct TimestampEventSourceLoadOutput {
    /// Parsed event records in source order.
    pub records: Vec<TimestampEventInputRecord>,
    /// Source path.
    pub path: PathBuf,
    /// Source file format.
    pub file_format: TimestampEventSourceFormat,
    /// Timestamp column or field used for parsing.
    pub timestamp_column: String,
    /// Timestamp unit used for parsing.
    pub timestamp_unit: TimestampEventTimestampUnit,
    /// Number of source records retained by the loader.
    pub source_record_count: usize,
}
