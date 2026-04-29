//! Shared JSON display helpers.

use serde_json::Value;

/// Render a JSON value as a compact human-readable label.
pub fn compact_value_string(value: &Value) -> String {
    match value {
        Value::Null => "not recorded".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.to_string(),
        Value::Array(values) => values
            .iter()
            .map(compact_value_string)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| format!("{key} {}", compact_value_string(value)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::compact_value_string;

    #[test]
    fn compact_value_string_renders_nested_values() {
        let value = json!({
            "count": 3,
            "reason": null,
            "tags": ["a", "b"],
        });

        assert_eq!(
            compact_value_string(&value),
            "count 3, reason not recorded, tags a, b"
        );
    }
}
