mod contracts;
mod error;
mod loading;

pub use contracts::{
    TimestampEventSource, TimestampEventSourceFormat, TimestampEventSourceLoadOutput,
    TimestampEventTimestampUnit,
};
pub use error::TimestampEventSourceLoadError;
pub use loading::load_timestamp_event_records;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Int64Array, RecordBatch};
    use arrow_schema::{DataType, Field, Schema};
    use parquet::arrow::ArrowWriter;

    use super::{
        TimestampEventSource, TimestampEventSourceFormat, TimestampEventTimestampUnit,
        load_timestamp_event_records,
    };

    #[test]
    fn load_csv_timestamp_events_from_rfc3339_column() {
        let path = std::env::temp_dir().join(format!(
            "janiform-timestamp-events-{}.csv",
            std::process::id()
        ));

        fs::write(
            &path,
            "timestamp,price\n2024-01-02T14:30:00Z,1.0\n2024-01-02T14:31:00Z,1.1\n",
        )
        .expect("csv fixture should write");

        let output = load_timestamp_event_records(TimestampEventSource {
            path: path.clone(),
            file_format: TimestampEventSourceFormat::Csv,
            timestamp_column: "timestamp".to_string(),
            timestamp_unit: TimestampEventTimestampUnit::Rfc3339,
        })
        .expect("csv timestamp events should load");

        assert_eq!(output.source_record_count, 2);
        assert_eq!(output.records[0].source_record_position, 0);
        assert_eq!(
            output.records[0].timestamp_utc.to_rfc3339(),
            "2024-01-02T14:30:00+00:00"
        );

        fs::remove_file(path).expect("csv fixture should remove");
    }

    #[test]
    fn load_jsonl_timestamp_events_from_numeric_nanos_field() {
        let path = std::env::temp_dir().join(format!(
            "janiform-timestamp-events-{}.jsonl",
            std::process::id()
        ));

        fs::write(
            &path,
            "{\"ts\": 1704205800000000000, \"price\": 1.0}\n{\"ts\": 1704205860000000000}\n",
        )
        .expect("jsonl fixture should write");

        let output = load_timestamp_event_records(TimestampEventSource {
            path: path.clone(),
            file_format: TimestampEventSourceFormat::Jsonl,
            timestamp_column: "ts".to_string(),
            timestamp_unit: TimestampEventTimestampUnit::Nanoseconds,
        })
        .expect("jsonl timestamp events should load");

        assert_eq!(output.source_record_count, 2);
        assert_eq!(
            output.records[1].timestamp_utc.to_rfc3339(),
            "2024-01-02T14:31:00+00:00"
        );

        fs::remove_file(path).expect("jsonl fixture should remove");
    }

    #[test]
    fn load_parquet_timestamp_events_projects_timestamp_column() {
        let path = std::env::temp_dir().join(format!(
            "janiform-timestamp-events-{}.parquet",
            std::process::id()
        ));

        let schema = Arc::new(Schema::new(vec![
            Field::new("ts", DataType::Int64, false),
            Field::new("ignored", DataType::Int64, false),
        ]));

        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(Int64Array::from(vec![
                    1_704_205_800_000_000_000_i64,
                    1_704_205_860_000_000_000_i64,
                ])) as ArrayRef,
                Arc::new(Int64Array::from(vec![1_i64, 2_i64])) as ArrayRef,
            ],
        )
        .expect("parquet fixture batch should build");

        let file = fs::File::create(&path).expect("parquet fixture should create");

        let mut writer =
            ArrowWriter::try_new(file, schema, None).expect("parquet writer should initialize");

        writer.write(&batch).expect("parquet batch should write");
        writer.close().expect("parquet writer should close");

        let output = load_timestamp_event_records(TimestampEventSource {
            path: path.clone(),
            file_format: TimestampEventSourceFormat::Parquet,
            timestamp_column: "ts".to_string(),
            timestamp_unit: TimestampEventTimestampUnit::Nanoseconds,
        })
        .expect("parquet timestamp events should load");

        assert_eq!(output.source_record_count, 2);
        assert_eq!(
            output.records[0].timestamp_utc.to_rfc3339(),
            "2024-01-02T14:30:00+00:00"
        );

        fs::remove_file(path).expect("parquet fixture should remove");
    }
}
