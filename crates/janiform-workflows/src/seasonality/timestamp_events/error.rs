use std::path::PathBuf;

use janiform_foundations::paths::format_path;
use thiserror::Error;

/// Error returned while loading timestamp-event source files.
#[derive(Debug, Error)]
pub enum TimestampEventSourceLoadError {
    /// Source file could not be opened or read.
    #[error(
        "failed to read timestamp-event source `{}`: {message}",
        format_path(path)
    )]
    Io {
        /// Source path.
        path: PathBuf,
        /// User-readable IO message.
        message: String,
    },

    /// CSV parser failed.
    #[error(
        "failed to parse timestamp-event CSV `{}`: {message}",
        format_path(path)
    )]
    Csv {
        /// Source path.
        path: PathBuf,
        /// User-readable parser message.
        message: String,
    },

    /// Parquet reader failed.
    #[error(
        "failed to parse timestamp-event parquet `{}`: {message}",
        format_path(path)
    )]
    Parquet {
        /// Source path.
        path: PathBuf,
        /// User-readable parser message.
        message: String,
    },

    /// JSONL parser failed.
    #[error(
        "failed to parse timestamp-event JSONL `{}`: {message}",
        format_path(path)
    )]
    Jsonl {
        /// Source path.
        path: PathBuf,
        /// User-readable parser message.
        message: String,
    },

    /// Required timestamp column is missing.
    #[error(
        "timestamp-event source `{}` is missing timestamp column `{timestamp_column}`",
        format_path(path)
    )]
    MissingTimestampColumn {
        /// Source path.
        path: PathBuf,
        /// Missing timestamp column.
        timestamp_column: String,
    },

    /// Timestamp value is absent.
    #[error(
        "timestamp-event source `{}` has an empty timestamp at source record {source_record_position}",
        format_path(path)
    )]
    MissingTimestampValue {
        /// Source path.
        path: PathBuf,
        /// Zero-based source record position.
        source_record_position: usize,
    },

    /// Timestamp value could not be parsed.
    #[error(
        "timestamp-event source `{}` has an invalid timestamp at source record {source_record_position}: {message}",
        format_path(path)
    )]
    InvalidTimestampValue {
        /// Source path.
        path: PathBuf,
        /// Zero-based source record position.
        source_record_position: usize,
        /// User-readable parser message.
        message: String,
    },

    /// Timestamp value cannot be represented as a Rust `DateTime<Utc>`.
    #[error(
        "timestamp-event source `{}` has a timestamp outside the supported range at source record {source_record_position}",
        format_path(path)
    )]
    TimestampOutOfRange {
        /// Source path.
        path: PathBuf,
        /// Zero-based source record position.
        source_record_position: usize,
    },

    /// Timestamp column type is unsupported.
    #[error(
        "timestamp-event source `{}` has unsupported timestamp column type `{data_type}`",
        format_path(path)
    )]
    UnsupportedTimestampColumnType {
        /// Source path.
        path: PathBuf,
        /// Arrow data type description.
        data_type: String,
    },
}
