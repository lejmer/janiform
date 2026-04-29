use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use crate::error::CliError;

pub(super) fn root_object<'a>(
    document: &'a Value,
    artifact_name: &'static str,
    path: &Path,
) -> Result<&'a Map<String, Value>, CliError> {
    document.as_object().ok_or_else(|| {
        malformed_artifact(
            artifact_name,
            path,
            "artifact root must be a JSON object".to_string(),
        )
    })
}

pub(super) fn object_field<'a>(
    object: &'a Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<&'a Map<String, Value>, CliError> {
    required_field(object, field_name, context, artifact_name, path)?
        .as_object()
        .ok_or_else(|| {
            malformed_artifact(
                artifact_name,
                path,
                format!("{context}.{field_name} must be a JSON object"),
            )
        })
}

pub(super) fn array_field<'a>(
    object: &'a Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<&'a Vec<Value>, CliError> {
    required_field(object, field_name, context, artifact_name, path)?
        .as_array()
        .ok_or_else(|| {
            malformed_artifact(
                artifact_name,
                path,
                format!("{context} must include list field `{field_name}`"),
            )
        })
}

pub(super) fn required_field<'a>(
    object: &'a Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<&'a Value, CliError> {
    object.get(field_name).ok_or_else(|| {
        malformed_artifact(
            artifact_name,
            path,
            format!("{context} is missing required field `{field_name}`"),
        )
    })
}

pub(super) fn non_empty_string_field(
    object: &Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<String, CliError> {
    let value = required_field(object, field_name, context, artifact_name, path)?;
    let Some(value) = value.as_str() else {
        return Err(malformed_artifact(
            artifact_name,
            path,
            format!("{context}.{field_name} must be a string"),
        ));
    };
    let value = value.trim();

    if value.is_empty() {
        return Err(malformed_artifact(
            artifact_name,
            path,
            format!("{context}.{field_name} must be non-empty"),
        ));
    }

    Ok(value.to_string())
}

pub(super) fn string_array_field(
    object: &Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<Vec<String>, CliError> {
    let values = array_field(object, field_name, context, artifact_name, path)?;
    let mut parsed_values = Vec::with_capacity(values.len());

    for (value_position, value) in values.iter().enumerate() {
        let Some(value) = value.as_str() else {
            return Err(malformed_artifact(
                artifact_name,
                path,
                format!("{context}.{field_name}[{value_position}] must be a string"),
            ));
        };
        let value = value.trim();

        if value.is_empty() {
            return Err(malformed_artifact(
                artifact_name,
                path,
                format!("{context}.{field_name}[{value_position}] must be non-empty"),
            ));
        }

        parsed_values.push(value.to_string());
    }

    Ok(parsed_values)
}

pub(super) fn finite_f64_array_field(
    object: &Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<Vec<f64>, CliError> {
    let values = array_field(object, field_name, context, artifact_name, path)?;

    if values.is_empty() {
        return Err(malformed_artifact(
            artifact_name,
            path,
            format!("{context}.{field_name} must be non-empty"),
        ));
    }

    values
        .iter()
        .enumerate()
        .map(|(value_position, value)| {
            finite_f64_value(
                value,
                &format!("{context}.{field_name}[{value_position}]"),
                artifact_name,
                path,
            )
        })
        .collect()
}

pub(super) fn timestamp_nanos_array_field(
    object: &Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<Vec<i64>, CliError> {
    let values = string_array_field(object, field_name, context, artifact_name, path)?;
    let mut timestamps = Vec::with_capacity(values.len());

    for (value_position, value) in values.iter().enumerate() {
        let timestamp = parse_timestamp_utc(
            value,
            &format!("{context}.{field_name}[{value_position}]"),
            artifact_name,
            path,
        )?;

        timestamps.push(timestamp_nanos(
            timestamp,
            &format!("{context}.{field_name}[{value_position}]"),
            artifact_name,
            path,
        )?);
    }

    Ok(timestamps)
}

pub(super) fn multiplier_entries(
    object: &Map<String, Value>,
    context: &str,
    path: &Path,
) -> Result<Vec<(u64, f64)>, CliError> {
    let mut entries = BTreeMap::new();

    for (raw_interval_index, raw_multiplier) in object {
        let interval_index = raw_interval_index.parse::<u64>().map_err(|_| {
            malformed_artifact(
                "seasonality cases",
                path,
                format!("{context} key `{raw_interval_index}` must be a non-negative integer"),
            )
        })?;

        entries.insert(
            interval_index,
            finite_f64_value(
                raw_multiplier,
                &format!("{context}[`{raw_interval_index}`]"),
                "seasonality cases",
                path,
            )?,
        );
    }

    Ok(entries.into_iter().collect())
}

pub(super) fn timestamp_field(
    object: &Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<DateTime<Utc>, CliError> {
    let raw_timestamp = non_empty_string_field(object, field_name, context, artifact_name, path)?;

    parse_timestamp_utc(
        &raw_timestamp,
        &format!("{context}.{field_name}"),
        artifact_name,
        path,
    )
}

pub(super) fn finite_f64_field(
    object: &Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<f64, CliError> {
    finite_f64_value(
        required_field(object, field_name, context, artifact_name, path)?,
        &format!("{context}.{field_name}"),
        artifact_name,
        path,
    )
}

pub(super) fn finite_non_negative_f64_field(
    object: &Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<f64, CliError> {
    let value = finite_f64_field(object, field_name, context, artifact_name, path)?;

    if value >= 0.0 {
        return Ok(value);
    }

    Err(malformed_artifact(
        artifact_name,
        path,
        format!("{context}.{field_name} must be non-negative"),
    ))
}

pub(super) fn optional_finite_f64_field(
    object: &Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<Option<f64>, CliError> {
    let Some(value) = object.get(field_name) else {
        return Ok(None);
    };

    if value.is_null() {
        return Ok(None);
    }

    finite_f64_value(
        value,
        &format!("{context}.{field_name}"),
        artifact_name,
        path,
    )
    .map(Some)
}

pub(super) fn usize_field(
    object: &Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<usize, CliError> {
    let value = u64_field(object, field_name, context, artifact_name, path)?;

    usize::try_from(value).map_err(|_| {
        malformed_artifact(
            artifact_name,
            path,
            format!("{context}.{field_name} is too large for this platform"),
        )
    })
}

pub(super) fn u64_field(
    object: &Map<String, Value>,
    field_name: &'static str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<u64, CliError> {
    required_field(object, field_name, context, artifact_name, path)?
        .as_u64()
        .ok_or_else(|| {
            malformed_artifact(
                artifact_name,
                path,
                format!("{context}.{field_name} must be a non-negative integer"),
            )
        })
}

pub(super) fn malformed_artifact(
    artifact_name: &'static str,
    path: &Path,
    message: String,
) -> CliError {
    CliError::ArtifactMalformed {
        artifact_name,
        path: path.to_path_buf(),
        message,
    }
}

fn parse_timestamp_utc(
    raw_timestamp: &str,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<DateTime<Utc>, CliError> {
    DateTime::parse_from_rfc3339(raw_timestamp)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| {
            malformed_artifact(
                artifact_name,
                path,
                format!("{context} must be an RFC3339 timestamp with timezone offset"),
            )
        })
}

fn timestamp_nanos(
    timestamp: DateTime<Utc>,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<i64, CliError> {
    timestamp.timestamp_nanos_opt().ok_or_else(|| {
        malformed_artifact(
            artifact_name,
            path,
            format!("{context} is outside the supported nanosecond timestamp range"),
        )
    })
}

fn finite_f64_value(
    value: &Value,
    context: &str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<f64, CliError> {
    value
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| {
            malformed_artifact(
                artifact_name,
                path,
                format!("{context} must be a finite number"),
            )
        })
}
