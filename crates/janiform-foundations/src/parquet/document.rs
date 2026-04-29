//! Shared Parquet document loading helpers.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::path::{Path, PathBuf};

use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::{ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder};
use parquet::errors::ParquetError;
use parquet::file::reader::{FileReader, SerializedFileReader};

/// IO chunk size used by projected Parquet readers.
pub const PROJECTED_PARQUET_BATCH_SIZE: usize = 65_536;

/// Error raised when a Parquet document cannot be loaded from disk.
#[derive(Debug)]
pub enum ParquetDocumentLoadError {
    /// The document path could not be opened.
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The document contents could not be parsed as Parquet.
    Parse { path: PathBuf, source: ParquetError },
    /// The document row count does not fit in the platform `usize`.
    RowCountOutOfRange { path: PathBuf },
}

impl Display for ParquetDocumentLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "failed to read Parquet document `{}`: {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(
                    formatter,
                    "failed to parse Parquet document `{}`: {source}",
                    path.display()
                )
            }
            Self::RowCountOutOfRange { path } => {
                write!(
                    formatter,
                    "Parquet document `{}` row count exceeds the supported usize range",
                    path.display()
                )
            }
        }
    }
}

impl Error for ParquetDocumentLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::RowCountOutOfRange { .. } => None,
        }
    }
}

/// Load column names from a Parquet document.
pub fn load_parquet_column_names(path: &Path) -> Result<Vec<String>, ParquetDocumentLoadError> {
    let reader = open_serialized_parquet_reader(path)?;
    let schema = reader.metadata().file_metadata().schema_descr();

    Ok(schema
        .columns()
        .iter()
        .map(|column| column.name().to_string())
        .collect())
}

/// Load the row count recorded in Parquet file metadata.
pub fn load_parquet_row_count(path: &Path) -> Result<usize, ParquetDocumentLoadError> {
    let reader = open_serialized_parquet_reader(path)?;

    usize::try_from(reader.metadata().file_metadata().num_rows()).map_err(|_| {
        ParquetDocumentLoadError::RowCountOutOfRange {
            path: path.to_path_buf(),
        }
    })
}

/// Open a projected Parquet record-batch reader using the default batch size.
pub fn open_projected_parquet_record_batch_reader<I, S>(
    path: &Path,
    projected_columns: I,
) -> Result<ParquetRecordBatchReader, ParquetDocumentLoadError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    open_projected_parquet_record_batch_reader_with_batch_size(
        path,
        projected_columns,
        PROJECTED_PARQUET_BATCH_SIZE,
    )
}

/// Open a projected Parquet record-batch reader using an explicit batch size.
pub fn open_projected_parquet_record_batch_reader_with_batch_size<I, S>(
    path: &Path,
    projected_columns: I,
    batch_size: usize,
) -> Result<ParquetRecordBatchReader, ParquetDocumentLoadError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let file = File::open(path).map_err(|source| ParquetDocumentLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|source| {
        ParquetDocumentLoadError::Parse {
            path: path.to_path_buf(),
            source,
        }
    })?;

    let parquet_schema = builder.metadata().file_metadata().schema_descr_ptr();

    let projected_column_names = projected_columns
        .into_iter()
        .map(|column_name| column_name.as_ref().to_string())
        .collect::<Vec<_>>();

    let projection = ProjectionMask::columns(
        parquet_schema.as_ref(),
        projected_column_names.iter().map(String::as_str),
    );

    builder
        .with_projection(projection)
        .with_batch_size(batch_size)
        .build()
        .map_err(|source| ParquetDocumentLoadError::Parse {
            path: path.to_path_buf(),
            source,
        })
}

/// Open a serialized Parquet reader for row-wise access.
pub fn open_serialized_parquet_reader(
    path: &Path,
) -> Result<SerializedFileReader<File>, ParquetDocumentLoadError> {
    let file = File::open(path).map_err(|source| ParquetDocumentLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    SerializedFileReader::new(file).map_err(|source| ParquetDocumentLoadError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
    use arrow_schema::{DataType, Field, Schema};
    use parquet::arrow::ArrowWriter;

    use super::{
        ParquetDocumentLoadError, load_parquet_column_names, load_parquet_row_count,
        open_projected_parquet_record_batch_reader,
    };

    #[test]
    fn load_parquet_column_names_reads_schema() {
        let path = unique_test_path("columns");

        write_test_parquet(&path);

        let column_names =
            load_parquet_column_names(&path).expect("test Parquet columns should load");
        let row_count = load_parquet_row_count(&path).expect("test Parquet row count should load");

        fs::remove_file(&path).expect("test Parquet document should be removable");

        assert_eq!(
            column_names,
            vec!["timestamp".to_string(), "ignored".to_string()]
        );
        assert_eq!(row_count, 2);
    }

    #[test]
    fn open_projected_parquet_record_batch_reader_projects_columns() {
        let path = unique_test_path("projected");

        write_test_parquet(&path);

        let mut reader = open_projected_parquet_record_batch_reader(&path, ["timestamp"])
            .expect("projected Parquet reader should load");

        let batch = reader
            .next()
            .expect("test Parquet reader should return a batch")
            .expect("test Parquet batch should decode");

        fs::remove_file(&path).expect("test Parquet document should be removable");

        assert_eq!(batch.schema_ref().fields().len(), 1);
        assert_eq!(batch.schema_ref().field(0).name(), "timestamp");
    }

    #[test]
    fn load_parquet_column_names_reports_read_error() {
        let path = unique_test_path("missing");

        let error = load_parquet_column_names(&path).expect_err("missing Parquet should fail");

        match error {
            ParquetDocumentLoadError::Read {
                path: error_path, ..
            } => assert_eq!(error_path, path),
            ParquetDocumentLoadError::Parse { .. }
            | ParquetDocumentLoadError::RowCountOutOfRange { .. } => {
                panic!("expected read error")
            }
        }
    }

    fn unique_test_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "janiform-foundations-parquet-document-{label}-{}.parquet",
            std::process::id()
        ))
    }

    fn write_test_parquet(path: &std::path::Path) {
        let schema = Arc::new(Schema::new(vec![
            Field::new("timestamp", DataType::Int64, false),
            Field::new("ignored", DataType::Utf8, false),
        ]));

        let columns = vec![
            Arc::new(Int64Array::from(vec![1_i64, 2_i64])) as ArrayRef,
            Arc::new(StringArray::from(vec!["a", "b"])) as ArrayRef,
        ];

        let batch = RecordBatch::try_new(schema.clone(), columns)
            .expect("test Parquet batch should be valid");

        let file = fs::File::create(path).expect("test Parquet document should be writable");

        let mut writer =
            ArrowWriter::try_new(file, schema, None).expect("test Parquet writer should open");

        writer
            .write(&batch)
            .expect("test Parquet batch should be writable");

        writer
            .close()
            .expect("test Parquet writer should close cleanly");
    }
}
