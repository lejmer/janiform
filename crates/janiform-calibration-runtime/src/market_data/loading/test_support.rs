use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::Schema;
use parquet::arrow::ArrowWriter;

pub(super) fn tick(
    timestamp_utc: &str,
) -> crate::market_data::preprocessing::MarketDataTranslationTickRecord {
    use chrono::{DateTime, Utc};

    crate::market_data::preprocessing::MarketDataTranslationTickRecord {
        timestamp_utc_nanos: DateTime::parse_from_rfc3339(timestamp_utc)
            .expect("test timestamp must parse")
            .with_timezone(&Utc)
            .timestamp_nanos_opt()
            .expect("test timestamp must fit into nanoseconds"),
        price: Some(1.0),
        trade_sign: None,
        source_row_number: 1,
    }
}

pub(super) fn unique_test_path(file_name: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after UNIX_EPOCH")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "janiform-market-data-loading-{timestamp}-{file_name}"
    ))
}

pub(super) fn write_parquet_record_batch(path: &Path, schema: Arc<Schema>, columns: Vec<ArrayRef>) {
    let batch = RecordBatch::try_new(schema.clone(), columns)
        .expect("test parquet columns must match the schema");
    let file = File::create(path).expect("test parquet path must be writable");
    let mut writer =
        ArrowWriter::try_new(file, schema, None).expect("test parquet writer must initialize");

    writer
        .write(&batch)
        .expect("test parquet batch must be written");
    writer.close().expect("test parquet writer must close");
}
