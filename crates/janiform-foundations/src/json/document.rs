//! Shared JSON document loading helpers.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Error raised when a JSON document cannot be loaded from disk.
#[derive(Debug)]
pub enum JsonDocumentLoadError {
    /// The document path could not be read.
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The document contents could not be parsed as JSON.
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
}

/// Error raised when a JSON document cannot be written to disk.
#[derive(Debug)]
pub enum JsonDocumentWriteError {
    /// The document parent directory could not be created.
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The document file could not be created.
    Create {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The document payload could not be serialized.
    Serialize { source: serde_json::Error },
    /// The document payload could not be written.
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl Display for JsonDocumentLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "failed to read JSON document `{}`: {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(
                    formatter,
                    "failed to parse JSON document `{}`: {source}",
                    path.display()
                )
            }
        }
    }
}

impl Display for JsonDocumentWriteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirectory { path, source } => {
                write!(
                    formatter,
                    "failed to create JSON document directory `{}`: {source}",
                    path.display()
                )
            }
            Self::Create { path, source } => {
                write!(
                    formatter,
                    "failed to create JSON document `{}`: {source}",
                    path.display()
                )
            }
            Self::Serialize { source } => {
                write!(formatter, "failed to serialize JSON document: {source}")
            }
            Self::Write { path, source } => {
                write!(
                    formatter,
                    "failed to write JSON document `{}`: {source}",
                    path.display()
                )
            }
        }
    }
}

impl Error for JsonDocumentLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

impl Error for JsonDocumentWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateDirectory { source, .. }
            | Self::Create { source, .. }
            | Self::Write { source, .. } => Some(source),
            Self::Serialize { source } => Some(source),
        }
    }
}

/// Load a JSON document from a filesystem path as a dynamic JSON value.
pub fn load_json_value(path: &Path) -> Result<Value, JsonDocumentLoadError> {
    load_json_document(path)
}

/// Load and deserialize a JSON document from a filesystem path.
pub fn load_json_document<T>(path: &Path) -> Result<T, JsonDocumentLoadError>
where
    T: DeserializeOwned,
{
    let contents = fs::read_to_string(path).map_err(|source| JsonDocumentLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    serde_json::from_str(&contents).map_err(|source| JsonDocumentLoadError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Write a pretty-printed JSON document to a filesystem path.
pub fn write_pretty_json_document<T>(path: &Path, payload: &T) -> Result<(), JsonDocumentWriteError>
where
    T: Serialize + ?Sized,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| JsonDocumentWriteError::CreateDirectory {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let mut file = fs::File::create(path).map_err(|source| JsonDocumentWriteError::Create {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::to_writer_pretty(&mut file, payload)
        .map_err(|source| JsonDocumentWriteError::Serialize { source })?;

    writeln!(file).map_err(|source| JsonDocumentWriteError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::{JsonDocumentLoadError, load_json_value};

    #[test]
    fn load_json_value_reads_document() {
        let path = std::env::temp_dir().join(format!(
            "janiform-foundations-json-document-{}.json",
            std::process::id()
        ));

        fs::write(&path, r#"{"schema_version":"test.v1"}"#)
            .expect("test JSON document should be writable");

        let loaded = load_json_value(&path).expect("test JSON document should load");

        fs::remove_file(&path).expect("test JSON document should be removable");

        assert_eq!(loaded, json!({"schema_version": "test.v1"}));
    }

    #[test]
    fn load_json_value_reports_parse_error() {
        let path = std::env::temp_dir().join(format!(
            "janiform-foundations-json-document-invalid-{}.json",
            std::process::id()
        ));

        fs::write(&path, "{").expect("test JSON document should be writable");

        let error = load_json_value(&path).expect_err("invalid JSON should fail");

        fs::remove_file(&path).expect("test JSON document should be removable");

        match error {
            JsonDocumentLoadError::Parse {
                path: error_path, ..
            } => assert_eq!(error_path, path),
            JsonDocumentLoadError::Read { .. } => panic!("expected parse error"),
        }
    }
}
