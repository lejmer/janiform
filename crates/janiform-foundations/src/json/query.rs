//! Shared JSON value lookup helpers.

use serde_json::Value;

/// Return an object field as a string when it exists and contains a string.
pub fn string_field<'a>(value: &'a Value, field_name: &str) -> Option<&'a str> {
    value.get(field_name).and_then(Value::as_str)
}

/// Return a nested JSON value by walking object fields in order.
pub fn nested_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;

    for segment in path {
        current = current.get(*segment)?;
    }

    Some(current)
}

/// Return a nested JSON value as a string when it exists and contains a string.
pub fn nested_string<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    nested_value(value, path).and_then(Value::as_str)
}

/// Return the first JSON value found under an object key during depth-first traversal.
pub fn find_value_by_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{find_value_by_key, nested_string, nested_value, string_field};

    #[test]
    fn nested_value_walks_object_path() {
        let value = json!({
            "metadata": {
                "run_id": "run-1",
            },
        });

        assert_eq!(
            nested_value(&value, &["metadata", "run_id"]),
            Some(&json!("run-1"))
        );
        assert_eq!(nested_value(&value, &["metadata", "missing"]), None);
    }

    #[test]
    fn string_helpers_return_only_string_values() {
        let value = json!({
            "run_id": "run-1",
            "count": 3,
            "metadata": {
                "scope": "seasonality_only",
            },
        });

        assert_eq!(string_field(&value, "run_id"), Some("run-1"));
        assert_eq!(string_field(&value, "count"), None);
        assert_eq!(
            nested_string(&value, &["metadata", "scope"]),
            Some("seasonality_only")
        );
    }

    #[test]
    fn find_value_by_key_searches_nested_values_depth_first() {
        let value = json!({
            "outer": [
                {"candidate": "first"},
                {"candidate": "second"},
            ],
        });

        assert_eq!(
            find_value_by_key(&value, "candidate"),
            Some(&json!("first"))
        );
        assert_eq!(find_value_by_key(&value, "missing"), None);
    }
}
