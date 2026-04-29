use janiform_foundations::json_payload;
use serde_json::Value;

use crate::error::CliError;

pub(super) fn required_object<'a>(
    value: &'a Value,
    field_name: &str,
    command_path: &'static str,
) -> Result<&'a serde_json::Map<String, Value>, CliError> {
    required_value_object(
        required_value(value, field_name, command_path)?,
        field_name,
        command_path,
    )
}

pub(super) fn required_value_object<'a>(
    value: &'a Value,
    field_path: &str,
    command_path: &'static str,
) -> Result<&'a serde_json::Map<String, Value>, CliError> {
    json_payload::object_value(value, field_path)
        .map_err(|source| malformed_payload_from_json(command_path, source))
}

pub(super) fn required_value_array<'a>(
    value: &'a Value,
    field_path: &str,
    command_path: &'static str,
) -> Result<&'a Vec<Value>, CliError> {
    json_payload::array_value(value, field_path)
        .map_err(|source| malformed_payload_from_json(command_path, source))
}

pub(super) fn required_value_string<'a>(
    value: &'a Value,
    field_path: &str,
    command_path: &'static str,
) -> Result<&'a str, CliError> {
    json_payload::string_value(value, field_path)
        .map_err(|source| malformed_payload_from_json(command_path, source))
}

pub(super) fn required_array<'a>(
    value: &'a Value,
    field_name: &str,
    command_path: &'static str,
) -> Result<&'a Vec<Value>, CliError> {
    required_value_array(
        required_value(value, field_name, command_path)?,
        field_name,
        command_path,
    )
}

pub(super) fn required_value<'a>(
    value: &'a Value,
    field_name: &str,
    command_path: &'static str,
) -> Result<&'a Value, CliError> {
    value
        .get(field_name)
        .ok_or_else(|| malformed_payload(command_path, &format!("{field_name} is required")))
}

pub(super) fn required_string<'a>(
    value: &'a Value,
    field_name: &str,
    command_path: &'static str,
) -> Result<&'a str, CliError> {
    required_value_string(
        required_value(value, field_name, command_path)?,
        field_name,
        command_path,
    )
}

pub(super) fn required_object_string<'a>(
    value: &'a serde_json::Map<String, Value>,
    field_name: &str,
    command_path: &'static str,
) -> Result<&'a str, CliError> {
    json_payload::string_field(value, field_name, field_name)
        .map_err(|source| malformed_payload_from_json(command_path, source))
}

pub(super) fn required_usize(
    value: &Value,
    field_name: &str,
    command_path: &'static str,
) -> Result<usize, CliError> {
    let count = value
        .get(field_name)
        .and_then(Value::as_u64)
        .ok_or_else(|| malformed_payload(command_path, &format!("{field_name} must be a count")))?;

    usize::try_from(count).map_err(|_| {
        malformed_payload(
            command_path,
            &format!("{field_name} exceeds the supported count range"),
        )
    })
}

pub(super) fn optional_string<'a>(value: &'a Value, field_name: &str) -> Option<&'a str> {
    value.get(field_name).and_then(Value::as_str)
}

pub(super) fn optional_object_string<'a>(
    value: &'a serde_json::Map<String, Value>,
    field_name: &str,
) -> Option<&'a str> {
    value.get(field_name).and_then(Value::as_str)
}

pub(super) fn malformed_payload(command_path: &'static str, message: &str) -> CliError {
    CliError::PresetRuntimePayloadMalformed {
        command_path,
        message: message.to_string(),
    }
}

fn malformed_payload_from_json(
    command_path: &'static str,
    source: json_payload::JsonPayloadError,
) -> CliError {
    malformed_payload(
        command_path,
        &format!("{} {}", source.field_path(), source.message()),
    )
}
