//! Shared CSV document loading helpers.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::path::{Path, PathBuf};

pub use ::csv::StringRecord;
use ::csv::{Reader, ReaderBuilder};

use crate::paths::format_path;

/// Error raised when a CSV document cannot be loaded from disk.
#[derive(Debug)]
pub enum CsvDocumentLoadError {
    /// The document could not be opened or parsed as CSV.
    Read { path: PathBuf, source: ::csv::Error },
}

impl Display for CsvDocumentLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "failed to read CSV document `{}`: {source}",
                    format_path(path)
                )
            }
        }
    }
}

impl Error for CsvDocumentLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
        }
    }
}

/// Open a CSV reader configured for header-bearing documents.
pub fn open_csv_reader(path: &Path) -> Result<Reader<File>, CsvDocumentLoadError> {
    ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .map_err(|source| CsvDocumentLoadError::Read {
            path: path.to_path_buf(),
            source,
        })
}

/// Load the header row from a CSV document.
pub fn load_csv_headers(path: &Path) -> Result<StringRecord, CsvDocumentLoadError> {
    let mut reader = open_csv_reader(path)?;

    reader
        .headers()
        .cloned()
        .map_err(|source| CsvDocumentLoadError::Read {
            path: path.to_path_buf(),
            source,
        })
}

/// Load column names from a CSV document.
pub fn load_csv_column_names(path: &Path) -> Result<Vec<String>, CsvDocumentLoadError> {
    Ok(load_csv_headers(path)?
        .iter()
        .map(ToOwned::to_owned)
        .collect())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{CsvDocumentLoadError, load_csv_column_names};

    #[test]
    fn load_csv_column_names_reads_header_row() {
        let path = std::env::temp_dir().join(format!(
            "janiform-foundations-csv-document-{}.csv",
            std::process::id()
        ));

        fs::write(&path, "timestamp,price\n2026-01-01T00:00:00Z,1.0\n")
            .expect("test CSV document should be writable");

        let column_names = load_csv_column_names(&path).expect("test CSV document should load");

        fs::remove_file(&path).expect("test CSV document should be removable");

        assert_eq!(
            column_names,
            vec!["timestamp".to_string(), "price".to_string()]
        );
    }

    #[test]
    fn load_csv_column_names_reports_read_error() {
        let path = std::env::temp_dir().join(format!(
            "janiform-foundations-csv-document-missing-{}.csv",
            std::process::id()
        ));

        let error = load_csv_column_names(&path).expect_err("missing CSV should fail");

        match error {
            CsvDocumentLoadError::Read {
                path: error_path, ..
            } => assert_eq!(error_path, path),
        }
    }
}
