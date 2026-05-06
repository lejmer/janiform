use std::path::Path;

use janiform::time::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader};
use janiform_foundations::json::document::{
    JsonDocumentLoadError, load_json_document, load_json_value,
};
use serde_json::Value;

use crate::error::CliError;

pub(crate) fn load_json_artifact(
    path: &Path,
    artifact_name: &'static str,
) -> Result<Value, CliError> {
    load_json_value(path).map_err(|source| json_document_load_error(source, artifact_name))
}

pub(crate) fn load_typed_json_artifact<T>(
    path: &Path,
    artifact_name: &'static str,
) -> Result<T, CliError>
where
    T: serde::de::DeserializeOwned,
{
    load_json_document(path).map_err(|source| json_document_load_error(source, artifact_name))
}

pub(crate) fn load_calendar_registry_artifact(
    path: &Path,
) -> Result<ExchangeCalendarRegistry, CliError> {
    let document = load_json_artifact(path, "calendar registry")?;

    ExchangeCalendarRegistryLoader::load_from_json_value(document).map_err(|source| {
        CliError::ArtifactMalformed {
            artifact_name: "calendar registry",
            path: path.to_path_buf(),
            message: source.to_string(),
        }
    })
}

pub(crate) fn require_json_object<'a>(
    value: &'a Value,
    artifact_name: &'static str,
    path: &Path,
) -> Result<&'a serde_json::Map<String, Value>, CliError> {
    value
        .as_object()
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name,
            path: path.to_path_buf(),
            message: "artifact root must be a JSON object".to_string(),
        })
}

fn json_document_load_error(
    source: JsonDocumentLoadError,
    artifact_name: &'static str,
) -> CliError {
    match source {
        JsonDocumentLoadError::Read { path, source } => CliError::ArtifactRead {
            artifact_name,
            path,
            source,
        },
        JsonDocumentLoadError::Parse { path, source } => CliError::ArtifactParse {
            artifact_name,
            path,
            source: Box::new(source),
        },
    }
}
