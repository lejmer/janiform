use std::path::PathBuf;

use thiserror::Error;

/// Error returned while loading timestamp-event source files.
#[derive(Debug, Error)]
pub enum TimestampEventSourceLoadError {
    /// Source file could not be opened or read.
    #[error("failed to read timestamp-event source `{}`: {message}", path.display())]
    Io {
        /// Source path.
        path: PathBuf,
        /// User-readable IO message.
        message: String,
    },

    /// CSV parser failed.
    #[error("failed to parse timestamp-event CSV `{}`: {message}", path.display())]
    Csv {
        /// Source path.
        path: PathBuf,
        /// User-readable parser message.
        message: String,
    },

    /// Parquet reader failed.
    #[error("failed to parse timestamp-event parquet `{}`: {message}", path.display())]
    Parquet {
        /// Source path.
        path: PathBuf,
        /// User-readable parser message.
        message: String,
    },

    /// JSONL parser failed.
    #[error("failed to parse timestamp-event JSONL `{}`: {message}", path.display())]
    Jsonl {
        /// Source path.
        path: PathBuf,
        /// User-readable parser message.
        message: String,
    },

    /// Required timestamp column is missing.
    #[error(
        "timestamp-event source `{}` is missing timestamp column `{timestamp_column}`",
        path.display()
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
        path.display()
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
        path.display()
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
        path.display()
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
        path.display()
    )]
    UnsupportedTimestampColumnType {
        /// Source path.
        path: PathBuf,
        /// Arrow data type description.
        data_type: String,
    },
}
