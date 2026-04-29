use janiform_foundations::json_payload;
use serde_json::Map;
use serde_json::Value;

use super::contracts::{PresetResolutionResult, PresetWorkflowResolutionError};

pub(super) fn replace_if_present(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    field_name: &str,
) -> PresetResolutionResult<()> {
    let Some(value) = source.get(field_name) else {
        return Ok(());
    };
    if value.is_null() {
        return Ok(());
    }

    // Override patch values are copied into the resolved owned configuration.
    target.insert(field_name.to_string(), value.clone());

    Ok(())
}

pub(super) fn clone_field(
    payload: &Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Value> {
    json_payload::clone_field(payload, field_name, field_path).map_err(payload_error_from_json)
}

pub(super) fn clone_object_field(
    payload: &Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Value> {
    json_payload::clone_object_field(payload, field_name, field_path)
        .map_err(payload_error_from_json)
}

pub(super) fn object_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<&'a Map<String, Value>> {
    json_payload::object_field(payload, field_name, field_path).map_err(payload_error_from_json)
}

pub(super) fn object_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<&'a mut Map<String, Value>> {
    json_payload::object_field_mut(payload, field_name, field_path).map_err(payload_error_from_json)
}

pub(super) fn optional_object_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Option<&'a Map<String, Value>>> {
    json_payload::optional_object_field(payload, field_name, field_path)
        .map_err(payload_error_from_json)
}

pub(super) fn optional_object_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Option<&'a mut Map<String, Value>>> {
    json_payload::optional_object_field_mut(payload, field_name, field_path)
        .map_err(payload_error_from_json)
}

pub(super) fn optional_array_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Option<&'a Vec<Value>>> {
    json_payload::optional_array_field(payload, field_name, field_path)
        .map_err(payload_error_from_json)
}

pub(super) fn string_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<&'a str> {
    json_payload::string_field(payload, field_name, field_path).map_err(payload_error_from_json)
}

pub(super) fn optional_string_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Option<&'a str>> {
    json_payload::optional_string_field(payload, field_name, field_path)
        .map_err(payload_error_from_json)
}

pub(super) fn object_value<'a>(
    value: &'a Value,
    field_path: &str,
) -> PresetResolutionResult<&'a Map<String, Value>> {
    json_payload::object_value(value, field_path).map_err(payload_error_from_json)
}

pub(super) fn object_value_mut<'a>(
    value: &'a mut Value,
    field_path: &str,
) -> PresetResolutionResult<&'a mut Map<String, Value>> {
    json_payload::object_value_mut(value, field_path).map_err(payload_error_from_json)
}

pub(super) fn array_value<'a>(
    value: &'a Value,
    field_path: &str,
) -> PresetResolutionResult<&'a Vec<Value>> {
    json_payload::array_value(value, field_path).map_err(payload_error_from_json)
}

pub(super) fn array_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<&'a mut Vec<Value>> {
    json_payload::array_field_mut(payload, field_name, field_path).map_err(payload_error_from_json)
}

pub(super) fn optional_array_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Option<&'a mut Vec<Value>>> {
    json_payload::optional_array_field_mut(payload, field_name, field_path)
        .map_err(payload_error_from_json)
}

pub(super) fn array_value_mut<'a>(
    value: &'a mut Value,
    field_path: &str,
) -> PresetResolutionResult<&'a mut Vec<Value>> {
    json_payload::array_value_mut(value, field_path).map_err(payload_error_from_json)
}

pub(super) fn string_value<'a>(
    value: &'a Value,
    field_path: &str,
) -> PresetResolutionResult<&'a str> {
    json_payload::string_value(value, field_path).map_err(payload_error_from_json)
}

pub(super) fn payload_error(field_path: &str, message: &str) -> PresetWorkflowResolutionError {
    PresetWorkflowResolutionError::InvalidPayload {
        field_path: field_path.to_string(),
        message: message.to_string(),
    }
}

fn payload_error_from_json(
    source: json_payload::JsonPayloadError,
) -> PresetWorkflowResolutionError {
    payload_error(source.field_path(), source.message())
}
