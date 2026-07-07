use std::path::PathBuf;

use janiform::calibration::market_data::{
    MarketDataDuplicateRecordPolicy, MarketDataInputFileFormat, MarketDataTickPriceInputPolicy,
};
use janiform_foundations::json::query::nested_value;
use serde_json::Value;

use super::request::StylizedFactsValidationWorkflowRequest;
use crate::error::WorkflowError;

pub(super) fn resolve_workflow_request_inputs(
    request: &StylizedFactsValidationWorkflowRequest,
) -> Result<StylizedFactsValidationWorkflowRequest, WorkflowError> {
    if request.comparison_request.is_some() || request.empirical_data_path.is_some() {
        return Ok(request.clone());
    }

    let Some(model_path) = &request.model_path else {
        return Ok(request.clone());
    };

    let model_payload: Value = super::artifacts::load_typed_json_artifact(model_path)?;

    let Some(tick_data_source) = nested_value(
        &model_payload,
        &["metadata", "input_inference", "tick_data_source"],
    ) else {
        return Ok(request.clone());
    };

    let mut resolved = request.clone();

    resolved.empirical_data_path = Some(model_tick_data_source_path(tick_data_source)?);

    if resolved.empirical_data_format.is_none() {
        resolved.empirical_data_format = model_tick_data_source_format(tick_data_source)?;
    }

    apply_model_input_inference_defaults(&mut resolved, &model_payload)?;

    Ok(resolved)
}

fn apply_model_input_inference_defaults(
    resolved: &mut StylizedFactsValidationWorkflowRequest,
    model_payload: &Value,
) -> Result<(), WorkflowError> {
    if resolved.empirical_timestamp_column.is_none() {
        resolved.empirical_timestamp_column = optional_model_string(
            model_payload,
            &[
                "metadata",
                "input_inference",
                "tick_data_column_mapping",
                "timestamp_column",
            ],
            "model_payload.metadata.input_inference.tick_data_column_mapping.timestamp_column",
        )?;
    }

    if resolved.empirical_price_column.is_none() {
        resolved.empirical_price_column = optional_model_string(
            model_payload,
            &[
                "metadata",
                "input_inference",
                "tick_data_column_mapping",
                "price_column",
            ],
            "model_payload.metadata.input_inference.tick_data_column_mapping.price_column",
        )?;
    }

    if resolved.empirical_trade_sign_column.is_none() {
        resolved.empirical_trade_sign_column = optional_model_string(
            model_payload,
            &[
                "metadata",
                "input_inference",
                "tick_data_column_mapping",
                "trade_sign_column",
            ],
            "model_payload.metadata.input_inference.tick_data_column_mapping.trade_sign_column",
        )?;
    }

    if resolved.empirical_bid_column.is_none() {
        resolved.empirical_bid_column = optional_model_string(
            model_payload,
            &[
                "metadata",
                "input_inference",
                "tick_data_column_mapping",
                "bid_column",
            ],
            "model_payload.metadata.input_inference.tick_data_column_mapping.bid_column",
        )?;
    }

    if resolved.empirical_ask_column.is_none() {
        resolved.empirical_ask_column = optional_model_string(
            model_payload,
            &[
                "metadata",
                "input_inference",
                "tick_data_column_mapping",
                "ask_column",
            ],
            "model_payload.metadata.input_inference.tick_data_column_mapping.ask_column",
        )?;
    }

    if resolved.empirical_timestamp_timezone.is_none() {
        resolved.empirical_timestamp_timezone = optional_model_string(
            model_payload,
            &[
                "metadata",
                "input_inference",
                "normalization_policy",
                "timestamp_timezone",
            ],
            "model_payload.metadata.input_inference.normalization_policy.timestamp_timezone",
        )?;
    }

    if resolved.empirical_price_input_policy.is_none() {
        resolved.empirical_price_input_policy = optional_model_tick_price_policy(model_payload)?;
    }

    if resolved.empirical_duplicate_record_policy.is_none() {
        resolved.empirical_duplicate_record_policy =
            optional_model_duplicate_record_policy(model_payload)?;
    }

    Ok(())
}

fn model_tick_data_source_path(tick_data_source: &Value) -> Result<PathBuf, WorkflowError> {
    let Some(raw_path) = tick_data_source.get("path") else {
        return Err(WorkflowError::ValidationMissingRequired {
            field: "--empirical-data-path",
        });
    };

    let Some(path) = raw_path.as_str() else {
        return Err(WorkflowError::ValidationInputFailed {
            field: "model_payload.metadata.input_inference.tick_data_source.path",
            message: "must be a string when --empirical-data-path is omitted".to_string(),
        });
    };

    if path.is_empty() {
        return Err(WorkflowError::ValidationInputFailed {
            field: "model_payload.metadata.input_inference.tick_data_source.path",
            message: "must be non-empty when --empirical-data-path is omitted".to_string(),
        });
    }

    Ok(PathBuf::from(path))
}

fn model_tick_data_source_format(
    tick_data_source: &Value,
) -> Result<Option<MarketDataInputFileFormat>, WorkflowError> {
    let Some(raw_format) = tick_data_source.get("file_format") else {
        return Ok(None);
    };

    if raw_format.is_null() {
        return Ok(None);
    }

    let Some(format) = raw_format.as_str() else {
        return Err(WorkflowError::ValidationInputFailed {
            field: "model_payload.metadata.input_inference.tick_data_source.file_format",
            message: "must be a string when provided".to_string(),
        });
    };

    match format {
        "csv" => Ok(Some(MarketDataInputFileFormat::Csv)),
        "parquet" => Ok(Some(MarketDataInputFileFormat::Parquet)),
        "json" => Ok(Some(MarketDataInputFileFormat::Json)),
        _ => Err(WorkflowError::ValidationInputFailed {
            field: "model_payload.metadata.input_inference.tick_data_source.file_format",
            message: format!("unsupported empirical data format `{format}`"),
        }),
    }
}

fn optional_model_tick_price_policy(
    model_payload: &Value,
) -> Result<Option<MarketDataTickPriceInputPolicy>, WorkflowError> {
    let Some(policy) = optional_model_string(
        model_payload,
        &[
            "metadata",
            "input_inference",
            "normalization_policy",
            "tick_price_input_policy",
        ],
        "model_payload.metadata.input_inference.normalization_policy.tick_price_input_policy",
    )?
    else {
        return Ok(None);
    };

    match policy.as_str() {
        "explicit_price_column" => Ok(Some(MarketDataTickPriceInputPolicy::ExplicitPriceColumn)),
        "bid_ask_midpoint" => Ok(Some(MarketDataTickPriceInputPolicy::BidAskMidpoint)),
        _ => Err(WorkflowError::ValidationInputFailed {
            field: "model_payload.metadata.input_inference.normalization_policy.tick_price_input_policy",
            message: format!("unsupported tick price input policy `{policy}`"),
        }),
    }
}

fn optional_model_duplicate_record_policy(
    model_payload: &Value,
) -> Result<Option<MarketDataDuplicateRecordPolicy>, WorkflowError> {
    let Some(policy) = optional_model_string(
        model_payload,
        &[
            "metadata",
            "input_inference",
            "normalization_policy",
            "duplicate_record_policy",
        ],
        "model_payload.metadata.input_inference.normalization_policy.duplicate_record_policy",
    )?
    else {
        return Ok(None);
    };

    match policy.as_str() {
        "keep_first" => Ok(Some(MarketDataDuplicateRecordPolicy::KeepFirst)),
        "keep_last" => Ok(Some(MarketDataDuplicateRecordPolicy::KeepLast)),
        _ => Err(WorkflowError::ValidationInputFailed {
            field: "model_payload.metadata.input_inference.normalization_policy.duplicate_record_policy",
            message: format!("unsupported duplicate record policy `{policy}`"),
        }),
    }
}

fn optional_model_string(
    model_payload: &Value,
    path: &[&str],
    field: &'static str,
) -> Result<Option<String>, WorkflowError> {
    let Some(value) = nested_value(model_payload, path) else {
        return Ok(None);
    };

    if value.is_null() {
        return Ok(None);
    }

    let Some(text) = value.as_str() else {
        return Err(WorkflowError::ValidationInputFailed {
            field,
            message: "must be a string when provided".to_string(),
        });
    };

    if text.is_empty() {
        return Err(WorkflowError::ValidationInputFailed {
            field,
            message: "must be non-empty when provided".to_string(),
        });
    }

    Ok(Some(text.to_string()))
}
