//! Shared JSON document loading helpers.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

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

impl Error for JsonDocumentLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
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
