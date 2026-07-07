//! Shared JSON payload shape helpers.

use std::error::Error;
use std::fmt;

use serde_json::Map;
use serde_json::Value;

use crate::numeric::validate_positive_number;

/// Error raised when a JSON payload does not match an expected value shape.
#[derive(Debug, Eq, PartialEq)]
pub struct JsonPayloadError {
    field_path: String,
    message: &'static str,
}

impl JsonPayloadError {
    /// Build a payload validation error for a JSON field path.
    pub fn new(field_path: &str, message: &'static str) -> Self {
        Self {
            field_path: field_path.to_string(),
            message,
        }
    }

    /// Return the JSON field path that failed validation.
    pub fn field_path(&self) -> &str {
        self.field_path.as_str()
    }

    /// Return the validation failure message.
    pub fn message(&self) -> &'static str {
        self.message
    }
}

impl fmt::Display for JsonPayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.field_path, self.message)
    }
}

impl Error for JsonPayloadError {}

/// Clone a required JSON object field after validating that it exists.
pub fn clone_field(
    payload: &Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<Value, JsonPayloadError> {
    payload
        .get(field_name)
        .cloned()
        .ok_or_else(|| JsonPayloadError::new(field_path, "is required"))
}

/// Clone a required JSON object field after validating that it contains an object.
pub fn clone_object_field(
    payload: &Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<Value, JsonPayloadError> {
    let value = clone_field(payload, field_name, field_path)?;

    object_value(&value, field_path)?;

    Ok(value)
}

/// Return a required JSON object field as an object.
pub fn object_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<&'a Map<String, Value>, JsonPayloadError> {
    let value = payload
        .get(field_name)
        .ok_or_else(|| JsonPayloadError::new(field_path, "is required"))?;

    object_value(value, field_path)
}

/// Return a required JSON object field as a mutable object.
pub fn object_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<&'a mut Map<String, Value>, JsonPayloadError> {
    let value = payload
        .get_mut(field_name)
        .ok_or_else(|| JsonPayloadError::new(field_path, "is required"))?;

    object_value_mut(value, field_path)
}

/// Return an optional JSON object field as an object when present and non-null.
pub fn optional_object_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<Option<&'a Map<String, Value>>, JsonPayloadError> {
    let Some(value) = payload.get(field_name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    object_value(value, field_path).map(Some)
}

/// Return an optional JSON object field as a mutable object when present and non-null.
pub fn optional_object_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<Option<&'a mut Map<String, Value>>, JsonPayloadError> {
    let Some(value) = payload.get_mut(field_name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    object_value_mut(value, field_path).map(Some)
}

/// Return an optional JSON object field as an array when present and non-null.
pub fn optional_array_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<Option<&'a Vec<Value>>, JsonPayloadError> {
    let Some(value) = payload.get(field_name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    array_value(value, field_path).map(Some)
}

/// Return a required JSON object field as an array.
pub fn array_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<&'a Vec<Value>, JsonPayloadError> {
    let value = payload
        .get(field_name)
        .ok_or_else(|| JsonPayloadError::new(field_path, "is required"))?;

    array_value(value, field_path)
}

/// Return a required JSON object field as a string.
pub fn string_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<&'a str, JsonPayloadError> {
    let value = payload
        .get(field_name)
        .ok_or_else(|| JsonPayloadError::new(field_path, "is required"))?;

    string_value(value, field_path)
}

/// Return an optional JSON object field as a string when present and non-null.
pub fn optional_string_field<'a>(
    payload: &'a Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<Option<&'a str>, JsonPayloadError> {
    let Some(value) = payload.get(field_name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    string_value(value, field_path).map(Some)
}

/// Return an optional JSON object field as an `f64` when present and non-null.
pub fn optional_f64_field(
    payload: &Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<Option<f64>, JsonPayloadError> {
    let Some(value) = payload.get(field_name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    value
        .as_f64()
        .ok_or_else(|| JsonPayloadError::new(field_path, "must be a number when provided"))
        .map(Some)
}

/// Return an optional JSON object field as a positive finite `f64`.
pub fn optional_positive_f64_field(
    payload: &Map<String, Value>,
    field_name: &str,
    field_path: &str,
    validation_field_name: &'static str,
) -> Result<Option<f64>, JsonPayloadError> {
    let Some(number) = optional_f64_field(payload, field_name, field_path)? else {
        return Ok(None);
    };

    validate_positive_number(number, validation_field_name).map_err(|_| {
        JsonPayloadError::new(
            field_path,
            "must be finite and strictly positive when provided",
        )
    })?;

    Ok(Some(number))
}

/// Return a JSON value as an object.
pub fn object_value<'a>(
    value: &'a Value,
    field_path: &str,
) -> Result<&'a Map<String, Value>, JsonPayloadError> {
    value
        .as_object()
        .ok_or_else(|| JsonPayloadError::new(field_path, "must be an object"))
}

/// Return a JSON value as a mutable object.
pub fn object_value_mut<'a>(
    value: &'a mut Value,
    field_path: &str,
) -> Result<&'a mut Map<String, Value>, JsonPayloadError> {
    value
        .as_object_mut()
        .ok_or_else(|| JsonPayloadError::new(field_path, "must be an object"))
}

/// Return a JSON value as an array.
pub fn array_value<'a>(
    value: &'a Value,
    field_path: &str,
) -> Result<&'a Vec<Value>, JsonPayloadError> {
    value
        .as_array()
        .ok_or_else(|| JsonPayloadError::new(field_path, "must be an array"))
}

/// Return a required JSON object field as a mutable array.
pub fn array_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<&'a mut Vec<Value>, JsonPayloadError> {
    let value = payload
        .get_mut(field_name)
        .ok_or_else(|| JsonPayloadError::new(field_path, "is required"))?;

    array_value_mut(value, field_path)
}

/// Return an optional JSON object field as a mutable array when present and non-null.
pub fn optional_array_field_mut<'a>(
    payload: &'a mut Map<String, Value>,
    field_name: &str,
    field_path: &str,
) -> Result<Option<&'a mut Vec<Value>>, JsonPayloadError> {
    let Some(value) = payload.get_mut(field_name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    array_value_mut(value, field_path).map(Some)
}

/// Return a JSON value as a mutable array.
pub fn array_value_mut<'a>(
    value: &'a mut Value,
    field_path: &str,
) -> Result<&'a mut Vec<Value>, JsonPayloadError> {
    value
        .as_array_mut()
        .ok_or_else(|| JsonPayloadError::new(field_path, "must be an array"))
}

/// Return a JSON value as a string.
pub fn string_value<'a>(value: &'a Value, field_path: &str) -> Result<&'a str, JsonPayloadError> {
    value
        .as_str()
        .ok_or_else(|| JsonPayloadError::new(field_path, "must be a string"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::JsonPayloadError;
    use super::object_field;
    use super::optional_positive_f64_field;
    use super::string_field;

    #[test]
    fn object_field_reports_missing_field_path() {
        let payload = json!({});
        let payload = payload.as_object().expect("fixture should be an object");

        assert_eq!(
            object_field(payload, "metadata", "artifact.metadata"),
            Err(JsonPayloadError::new("artifact.metadata", "is required"))
        );
    }

    #[test]
    fn string_field_reports_shape_mismatch() {
        let payload = json!({
            "version": 1,
        });
        let payload = payload.as_object().expect("fixture should be an object");

        assert_eq!(
            string_field(payload, "version", "artifact.version"),
            Err(JsonPayloadError::new(
                "artifact.version",
                "must be a string"
            ))
        );
    }

    #[test]
    fn optional_positive_f64_field_accepts_absent_null_and_positive_values() {
        let missing_payload = json!({});
        let missing_payload = missing_payload
            .as_object()
            .expect("fixture should be an object");

        let null_payload = json!({
            "tick_size": null,
        });
        let null_payload = null_payload
            .as_object()
            .expect("fixture should be an object");

        let positive_payload = json!({
            "tick_size": 0.01,
        });
        let positive_payload = positive_payload
            .as_object()
            .expect("fixture should be an object");

        assert_eq!(
            optional_positive_f64_field(
                missing_payload,
                "tick_size",
                "configuration.tick_size",
                "tick_size",
            ),
            Ok(None)
        );
        assert_eq!(
            optional_positive_f64_field(
                null_payload,
                "tick_size",
                "configuration.tick_size",
                "tick_size",
            ),
            Ok(None)
        );
        assert_eq!(
            optional_positive_f64_field(
                positive_payload,
                "tick_size",
                "configuration.tick_size",
                "tick_size",
            ),
            Ok(Some(0.01))
        );
    }

    #[test]
    fn optional_positive_f64_field_reports_shape_mismatch() {
        let payload = json!({
            "tick_size": "0.01",
        });
        let payload = payload.as_object().expect("fixture should be an object");

        assert_eq!(
            optional_positive_f64_field(
                payload,
                "tick_size",
                "configuration.tick_size",
                "tick_size",
            ),
            Err(JsonPayloadError::new(
                "configuration.tick_size",
                "must be a number when provided",
            ))
        );
    }

    #[test]
    fn optional_positive_f64_field_reports_non_positive_values() {
        let payload = json!({
            "tick_size": 0.0,
        });
        let payload = payload.as_object().expect("fixture should be an object");

        assert_eq!(
            optional_positive_f64_field(
                payload,
                "tick_size",
                "configuration.tick_size",
                "tick_size",
            ),
            Err(JsonPayloadError::new(
                "configuration.tick_size",
                "must be finite and strictly positive when provided",
            ))
        );
    }
}
