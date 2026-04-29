use janiform_foundations::json::query::nested_value;
use serde_json::Value;

use crate::error::WorkflowError;

use super::artifacts::SimulationOutputColumns;
use super::artifacts::validation_error;

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

    let timestamp_column = required_string(
        metadata,
        &[
            "input_inference",
            "tick_data_column_mapping",
            "timestamp_column",
        ],
        "model_payload.metadata.input_inference.tick_data_column_mapping.timestamp_column",
    )?;
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
        _ => Ok("parquet"),
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

    if tick_price_input_policy.as_deref() == Some("bid_ask_midpoint") {
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
    .unwrap_or_else(|| "price".to_string()))
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

    if tick_price_input_policy.as_deref() != Some("explicit_price_column") {
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

fn required_string(
    value: &Value,
    path: &[&str],
    field_name: &'static str,
) -> Result<String, WorkflowError> {
    let Some(raw_value) = nested_value(value, path) else {
        return Err(validation_error(field_name, "must be present"));
    };

    let Some(string_value) = raw_value.as_str() else {
        return Err(validation_error(field_name, "must be a string"));
    };

    if string_value.is_empty() {
        return Err(validation_error(field_name, "must be non-empty"));
    }

    Ok(string_value.to_string())
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
