use janiform_foundations::json::query::nested_value;
use serde_json::Value;

use crate::error::WorkflowError;

use super::artifacts::SimulationOutputColumns;
use super::artifacts::validation_error;

const DEFAULT_SIMULATION_TIMESTAMP_COLUMN: &str = "timestamp_utc";
const DEFAULT_SIMULATION_PRICE_COLUMN: &str = "price";
const DEFAULT_SIMULATION_FILE_FORMAT: &str = "parquet";
const BID_ASK_MIDPOINT_PRICE_INPUT_POLICY: &str = "bid_ask_midpoint";
const EXPLICIT_PRICE_COLUMN_INPUT_POLICY: &str = "explicit_price_column";

/// Output preferences derived from a persisted fitted-model payload.
#[derive(Clone, Debug)]
pub struct SimulationOutputPreferences {
    /// Output column contract.
    pub output_columns: SimulationOutputColumns,
    /// Reference price used to convert simulated log prices.
    pub reference_price: f64,
    /// Default artifact file format for simulation exports.
    pub default_file_format: &'static str,
}

/// Derive simulation output settings from a persisted fitted-model payload.
pub fn simulation_output_preferences_from_model_payload(
    model_payload: &Value,
) -> Result<SimulationOutputPreferences, WorkflowError> {
    let metadata = model_payload
        .get("metadata")
        .ok_or_else(|| validation_error("model_payload.metadata", "must be present"))?;

    let timestamp_column = optional_string(
        metadata,
        &[
            "input_inference",
            "tick_data_column_mapping",
            "timestamp_column",
        ],
        "model_payload.metadata.input_inference.tick_data_column_mapping.timestamp_column",
    )?
    .unwrap_or_else(|| DEFAULT_SIMULATION_TIMESTAMP_COLUMN.to_string());

    let price_column = simulation_output_price_column(metadata)?;
    let side_column = simulation_output_side_column(metadata)?;
    let reference_price = simulation_output_reference_price(metadata)?;
    let default_file_format = default_simulation_output_file_format(metadata)?;

    Ok(SimulationOutputPreferences {
        output_columns: SimulationOutputColumns {
            timestamp_column,
            price_column,
            side_column,
        },
        reference_price,
        default_file_format,
    })
}

fn default_simulation_output_file_format(metadata: &Value) -> Result<&'static str, WorkflowError> {
    let raw_file_format = optional_string(
        metadata,
        &["input_inference", "tick_data_source", "file_format"],
        "model_payload.metadata.input_inference.tick_data_source.file_format",
    )?;

    match raw_file_format.as_deref() {
        Some("csv") => Ok("csv"),
        Some("parquet") => Ok("parquet"),
        _ => Ok(DEFAULT_SIMULATION_FILE_FORMAT),
    }
}

fn simulation_output_price_column(metadata: &Value) -> Result<String, WorkflowError> {
    let tick_price_input_policy = optional_string(
        metadata,
        &[
            "input_inference",
            "normalization_policy",
            "tick_price_input_policy",
        ],
        "model_payload.metadata.input_inference.normalization_policy.tick_price_input_policy",
    )?;

    if tick_price_input_policy.as_deref() == Some(BID_ASK_MIDPOINT_PRICE_INPUT_POLICY) {
        return Ok("mid_price".to_string());
    }

    Ok(optional_string(
        metadata,
        &[
            "input_inference",
            "tick_data_column_mapping",
            "price_column",
        ],
        "model_payload.metadata.input_inference.tick_data_column_mapping.price_column",
    )?
    .unwrap_or_else(|| DEFAULT_SIMULATION_PRICE_COLUMN.to_string()))
}

fn simulation_output_side_column(metadata: &Value) -> Result<Option<String>, WorkflowError> {
    let tick_price_input_policy = optional_string(
        metadata,
        &[
            "input_inference",
            "normalization_policy",
            "tick_price_input_policy",
        ],
        "model_payload.metadata.input_inference.normalization_policy.tick_price_input_policy",
    )?;

    if tick_price_input_policy.as_deref() != Some(EXPLICIT_PRICE_COLUMN_INPUT_POLICY) {
        return Ok(None);
    }

    optional_string(
        metadata,
        &[
            "input_inference",
            "tick_data_column_mapping",
            "trade_sign_column",
        ],
        "model_payload.metadata.input_inference.tick_data_column_mapping.trade_sign_column",
    )
}

fn simulation_output_reference_price(metadata: &Value) -> Result<f64, WorkflowError> {
    let Some(raw_reference_price) = nested_value(
        metadata,
        &["simulation_output_preferences", "reference_price"],
    ) else {
        return Ok(1.0);
    };

    if raw_reference_price.is_null() {
        return Ok(1.0);
    }

    let Some(reference_price) = raw_reference_price.as_f64() else {
        return Err(validation_error(
            "model_payload.metadata.simulation_output_preferences.reference_price",
            "must be numeric when provided",
        ));
    };

    if !reference_price.is_finite() || reference_price <= 0.0 {
        return Err(validation_error(
            "model_payload.metadata.simulation_output_preferences.reference_price",
            "must be finite and strictly positive",
        ));
    }

    Ok(reference_price)
}

fn optional_string(
    value: &Value,
    path: &[&str],
    field_name: &'static str,
) -> Result<Option<String>, WorkflowError> {
    let Some(raw_value) = nested_value(value, path) else {
        return Ok(None);
    };

    if raw_value.is_null() {
        return Ok(None);
    }

    let Some(string_value) = raw_value.as_str() else {
        return Err(validation_error(
            field_name,
            "must be a string when provided",
        ));
    };

    if string_value.is_empty() {
        return Err(validation_error(
            field_name,
            "must be non-empty when provided",
        ));
    }

    Ok(Some(string_value.to_string()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::simulation_output_preferences_from_model_payload;

    #[test]
    fn output_preferences_use_canonical_columns_without_input_inference() {
        let preferences = simulation_output_preferences_from_model_payload(&json!({
            "metadata": {
                "input_inference": null,
                "simulation_output_preferences": {
                    "reference_price": 6563.25,
                },
            },
        }))
        .expect("missing input inference should use synthetic output defaults");

        assert_eq!(preferences.output_columns.timestamp_column, "timestamp_utc");
        assert_eq!(preferences.output_columns.price_column, "price");
        assert_eq!(preferences.output_columns.side_column, None);
        assert_eq!(preferences.reference_price, 6563.25);
        assert_eq!(preferences.default_file_format, "parquet");
    }

    #[test]
    fn output_preferences_preserve_explicit_trade_side_column() {
        let preferences = simulation_output_preferences_from_model_payload(&json!({
            "metadata": {
                "input_inference": {
                    "tick_data_source": {
                        "file_format": "csv",
                    },
                    "tick_data_column_mapping": {
                        "timestamp_column": "time",
                        "price_column": "trade_price",
                        "trade_sign_column": "side",
                    },
                    "normalization_policy": {
                        "tick_price_input_policy": "explicit_price_column",
                    },
                },
            },
        }))
        .expect("explicit input inference should shape simulation columns");

        assert_eq!(preferences.output_columns.timestamp_column, "time");
        assert_eq!(preferences.output_columns.price_column, "trade_price");
        assert_eq!(
            preferences.output_columns.side_column,
            Some("side".to_string())
        );
        assert_eq!(preferences.reference_price, 1.0);
        assert_eq!(preferences.default_file_format, "csv");
    }
}
