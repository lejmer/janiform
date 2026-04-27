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
    payload
        .get(field_name)
        .cloned()
        .ok_or_else(|| payload_error(field_path, "is required"))
}

pub(super) fn clone_object_field(
    payload: &Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Value> {
    let value = clone_field(payload, field_name, field_path)?;
    object_value(&value, field_path)?;

    Ok(value)
}

pub(super) fn object_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<&'a Map<String, Value>> {
    let value = payload
        .get(field_name)
        .ok_or_else(|| payload_error(field_path, "is required"))?;

    object_value(value, field_path)
}

pub(super) fn object_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<&'a mut Map<String, Value>> {
    let value = payload
        .get_mut(field_name)
        .ok_or_else(|| payload_error(field_path, "is required"))?;

    object_value_mut(value, field_path)
}

pub(super) fn optional_object_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Option<&'a Map<String, Value>>> {
    let Some(value) = payload.get(field_name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    object_value(value, field_path).map(Some)
}

pub(super) fn optional_object_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Option<&'a mut Map<String, Value>>> {
    let Some(value) = payload.get_mut(field_name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    object_value_mut(value, field_path).map(Some)
}

pub(super) fn optional_array_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Option<&'a Vec<Value>>> {
    let Some(value) = payload.get(field_name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    array_value(value, field_path).map(Some)
}

pub(super) fn string_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<&'a str> {
    let value = payload
        .get(field_name)
        .ok_or_else(|| payload_error(field_path, "is required"))?;

    string_value(value, field_path)
}

pub(super) fn optional_string_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Option<&'a str>> {
    let Some(value) = payload.get(field_name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    string_value(value, field_path).map(Some)
}

pub(super) fn object_value<'a>(
    value: &'a Value,
    field_path: &str,
) -> PresetResolutionResult<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| payload_error(field_path, "must be an object"))
}

pub(super) fn object_value_mut<'a>(
    value: &'a mut Value,
    field_path: &str,
) -> PresetResolutionResult<&'a mut Map<String, Value>> {
    value
        .as_object_mut()
        .ok_or_else(|| payload_error(field_path, "must be an object"))
}

pub(super) fn array_value<'a>(
    value: &'a Value,
    field_path: &str,
) -> PresetResolutionResult<&'a Vec<Value>> {
    value
        .as_array()
        .ok_or_else(|| payload_error(field_path, "must be an array"))
}

pub(super) fn array_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<&'a mut Vec<Value>> {
    let value = payload
        .get_mut(field_name)
        .ok_or_else(|| payload_error(field_path, "is required"))?;

    array_value_mut(value, field_path)
}

pub(super) fn optional_array_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> PresetResolutionResult<Option<&'a mut Vec<Value>>> {
    let Some(value) = payload.get_mut(field_name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    array_value_mut(value, field_path).map(Some)
}

pub(super) fn array_value_mut<'a>(
    value: &'a mut Value,
    field_path: &str,
) -> PresetResolutionResult<&'a mut Vec<Value>> {
    value
        .as_array_mut()
        .ok_or_else(|| payload_error(field_path, "must be an array"))
}

pub(super) fn string_value<'a>(
    value: &'a Value,
    field_path: &str,
) -> PresetResolutionResult<&'a str> {
    value
        .as_str()
        .ok_or_else(|| payload_error(field_path, "must be a string"))
}

pub(super) fn payload_error(field_path: &str, message: &str) -> PresetWorkflowResolutionError {
    PresetWorkflowResolutionError::InvalidPayload {
        field_path: field_path.to_string(),
        message: message.to_string(),
    }
}
