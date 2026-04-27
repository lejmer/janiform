use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;
use janiform_terminal_ui::KeyValueRow;
use janiform_terminal_ui::StartSummary;
use janiform_terminal_ui::TerminalRenderer;
use serde_json::Value;

use crate::error::CliError;

pub(crate) fn load_json_artifact(
    path: &Path,
    artifact_name: &'static str,
) -> Result<Value, CliError> {
    let file = fs::File::open(path).map_err(|source| CliError::ArtifactRead {
        artifact_name,
        path: path.to_path_buf(),
        source,
    })?;

    serde_json::from_reader(file).map_err(|source| CliError::ArtifactParse {
        artifact_name,
        path: path.to_path_buf(),
        source,
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

pub(crate) fn string_field<'a>(value: &'a Value, field_name: &str) -> Option<&'a str> {
    value.get(field_name).and_then(Value::as_str)
}

pub(crate) fn nested_string<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    nested_value(value, path).and_then(Value::as_str)
}

pub(crate) fn nested_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;

    for segment in path {
        current = current.get(*segment)?;
    }

    Some(current)
}

pub(crate) fn find_value_by_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(fields) => {
            if let Some(found) = fields.get(key) {
                return Some(found);
            }

            fields
                .values()
                .find_map(|value| find_value_by_key(value, key))
        }
        Value::Array(values) => values
            .iter()
            .find_map(|value| find_value_by_key(value, key)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

pub(crate) fn value_to_compact_string(value: &Value) -> String {
    match value {
        Value::Null => "not recorded".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.to_string(),
        Value::Array(values) => values
            .iter()
            .map(value_to_compact_string)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| format!("{key} {}", value_to_compact_string(value)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactListItem {
    label: String,
    value: String,
}

impl ArtifactListItem {
    pub(crate) fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }

    fn render(&self) -> String {
        format!("{}: {}", self.label, self.value)
    }
}

pub(crate) fn render_artifact_list(
    title: impl Into<String>,
    items: impl IntoIterator<Item = ArtifactListItem>,
    renderer: &TerminalRenderer,
) -> Option<String> {
    let items = unique_artifact_items(items);

    if items.is_empty() {
        return None;
    }

    let rendered_items = items
        .iter()
        .map(ArtifactListItem::render)
        .collect::<Vec<_>>()
        .join("\n");

    Some(
        StartSummary {
            title: title.into(),
            rows: vec![KeyValueRow::block("Paths", rendered_items)],
        }
        .render(renderer),
    )
}

fn unique_artifact_items(
    items: impl IntoIterator<Item = ArtifactListItem>,
) -> Vec<ArtifactListItem> {
    let mut unique_items = Vec::new();

    for item in items {
        if unique_items
            .iter()
            .any(|existing: &ArtifactListItem| existing == &item)
        {
            continue;
        }

        unique_items.push(item);
    }

    unique_items
}

pub(crate) fn stage_label(stage: &str) -> String {
    let label = match stage {
        "seasonality" => "Fit seasonality",
        "hawkes" => "Fit event activity",
        "return_components" => "Fit return distribution",
        "mean_reversion" => "Fit mean reversion",
        "joint_refinement" => "Refine dynamics",
        other => return other.replace('_', " "),
    };

    label.to_string()
}

pub(crate) fn workflow_scope_label(scope: &str) -> String {
    match scope {
        "seasonality_only" => "seasonality only".to_string(),
        "full_without_option_surface" => "full calibration without option snapshot".to_string(),
        "full_with_option_surface" => "full calibration with option snapshot".to_string(),
        "underlying_only" => "underlying calibration".to_string(),
        "joint_option_calibrated" => "joint option calibration".to_string(),
        other => other.replace('_', " "),
    }
}

pub(crate) fn display_path_relative_to(base_directory: &Path, path: &Path) -> String {
    if let Ok(relative_path) = path.strip_prefix(base_directory) {
        return relative_path.display().to_string();
    }

    path.display().to_string()
}

pub(crate) fn display_optional_path_relative_to(
    base_directory: &Path,
    path: Option<&str>,
) -> Option<String> {
    path.map(|path| display_path_relative_to(base_directory, Path::new(path)))
}

pub(crate) fn resolve_artifact_path(input_path: &Path, artifact_path: &str) -> PathBuf {
    let artifact_path = Path::new(artifact_path);

    if artifact_path.is_absolute() {
        return artifact_path.to_path_buf();
    }

    input_path.join(artifact_path)
}

pub(crate) fn elapsed_between_timestamps(
    start: Option<&str>,
    end: Option<&str>,
) -> Option<Duration> {
    let start = parse_utc_timestamp(start?)?;
    let end = parse_utc_timestamp(end?)?;
    let duration = end.signed_duration_since(start).to_std().ok()?;

    Some(duration)
}

pub(crate) fn format_file_size(byte_count: u64) -> String {
    if byte_count < 1024 {
        return format!("{byte_count} B");
    }

    let kibibytes = byte_count / 1024;

    format!("{kibibytes} KiB")
}

fn parse_utc_timestamp(timestamp: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}
