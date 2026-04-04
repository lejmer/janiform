use std::collections::HashSet;

use super::contracts::{
    MarketDataLoadingError, MarketDataNormalizationPolicy, MarketDataNormalizedOptionSurfaceRecord,
    MarketDataOptionQuoteInputPolicy, MarketDataOptionSurfaceColumnMapping,
    MarketDataOptionSurfaceLoadingInput, MarketDataSourceSummary,
};
use super::normalization::{
    normalize_option_type, normalize_optional_numeric_value, normalize_required_numeric_value,
    normalize_timestamp_value,
};
use super::rows::{load_rows, load_source_columns, validate_columns_exist};
use super::shared::{
    LoadedOptionSurfaceRecord, OptionSurfaceLoadOutput, RawRow, file_format_string,
    normalized_float_key, option_quote_policy_string, required_option_surface_source_columns,
};

pub(super) fn load_option_surface_records(
    option_surface_input: &MarketDataOptionSurfaceLoadingInput,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Result<OptionSurfaceLoadOutput, MarketDataLoadingError> {
    let required_columns = required_option_surface_source_columns(
        &option_surface_input.column_mapping,
        normalization_policy,
    );
    let available_columns =
        load_source_columns(&option_surface_input.source, "option_surface_source")?;
    validate_columns_exist(
        &available_columns,
        &required_columns,
        "column_mapping.option_surface",
        &option_surface_input.source.path,
    )?;

    let raw_rows = load_rows(&option_surface_input.source, "option_surface_source")?;

    let source_row_count = raw_rows.len();

    let mut normalized_rows = Vec::with_capacity(source_row_count);
    let mut seen_keys = HashSet::new();

    for (row_index, raw_row) in raw_rows.iter().enumerate() {
        let normalized = normalize_option_surface_row(
            raw_row,
            row_index + 1,
            &option_surface_input.column_mapping,
            normalization_policy,
            &option_surface_input.source.path,
        )?;

        let key = option_surface_record_dedup_key(&normalized);

        if !seen_keys.insert(key) {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: "option_surface_source".to_string(),
                message: "contains duplicate normalized rows".to_string(),
            });
        }

        normalized_rows.push(normalized);
    }

    if normalization_policy.sort_records_by_timestamp {
        normalized_rows.sort_by(|left, right| {
            (
                left.quote_timestamp_utc_nanos,
                left.expiry_timestamp_utc_nanos,
                normalized_float_key(Some(left.strike)),
                left.option_type.as_str(),
                left.source_row_number,
            )
                .cmp(&(
                    right.quote_timestamp_utc_nanos,
                    right.expiry_timestamp_utc_nanos,
                    normalized_float_key(Some(right.strike)),
                    right.option_type.as_str(),
                    right.source_row_number,
                ))
        });
    }

    Ok(OptionSurfaceLoadOutput {
        option_surface_quote_values: normalized_rows.iter().map(|row| row.quote_value).collect(),
        option_surface_records: normalized_rows.clone(),
        summary: MarketDataSourceSummary {
            path: option_surface_input.source.path.clone(),
            file_format: file_format_string(option_surface_input.source.file_format).to_string(),
            provenance_label: option_surface_input.source.provenance_label.clone(),
            source_row_count,
            retained_row_count: normalized_rows.len(),
            dropped_duplicate_row_count: 0,
            sort_records_by_timestamp: normalization_policy.sort_records_by_timestamp,
            applied_timestamp_timezone: Some(normalization_policy.timestamp_timezone.clone()),
            canonical_value_field: Some("quote_value".to_string()),
            value_mapping_policy: normalization_policy
                .option_quote_input_policy
                .map(option_quote_policy_string),
        },
    })
}

pub fn materialize_option_surface_records_for_payload(
    records: &[LoadedOptionSurfaceRecord],
) -> Vec<MarketDataNormalizedOptionSurfaceRecord> {
    records
        .iter()
        .map(|record| MarketDataNormalizedOptionSurfaceRecord {
            quote_timestamp_utc_nanos: record.quote_timestamp_utc_nanos,
            expiry_timestamp_utc_nanos: record.expiry_timestamp_utc_nanos,
            strike: record.strike,
            option_type: record.option_type.clone(),
            quote_value: record.quote_value,
            mid_price: record.mid_price,
            bid_price: record.bid_price,
            ask_price: record.ask_price,
            implied_volatility: record.implied_volatility,
            source_row_number: record.source_row_number,
        })
        .collect()
}

pub(super) fn normalize_option_surface_row(
    raw_row: &RawRow,
    source_row_number: usize,
    column_mapping: &MarketDataOptionSurfaceColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    source_path: &str,
) -> Result<LoadedOptionSurfaceRecord, MarketDataLoadingError> {
    let row_field_path = format!("option_surface_source.rows[{source_row_number}]");

    let quote_timestamp = normalize_timestamp_value(
        raw_row.get(&column_mapping.quote_timestamp_column),
        format!("{row_field_path}.{}", column_mapping.quote_timestamp_column).as_str(),
        Some(normalization_policy.timestamp_timezone.as_str()),
        source_path,
    )?;
    let expiry_timestamp = normalize_timestamp_value(
        raw_row.get(&column_mapping.expiry_timestamp_column),
        format!(
            "{row_field_path}.{}",
            column_mapping.expiry_timestamp_column
        )
        .as_str(),
        Some(normalization_policy.timestamp_timezone.as_str()),
        source_path,
    )?;

    let strike = normalize_required_numeric_value(
        raw_row.get(&column_mapping.strike_column),
        format!("{row_field_path}.{}", column_mapping.strike_column).as_str(),
        source_path,
        false,
        false,
    )?;

    let option_type = normalize_option_type(
        raw_row.get(&column_mapping.option_type_column),
        format!("{row_field_path}.{}", column_mapping.option_type_column).as_str(),
        source_path,
    )?;

    let mid_price = match &column_mapping.mid_price_column {
        Some(column_name) => normalize_optional_numeric_value(
            raw_row.get(column_name),
            format!("{row_field_path}.{column_name}").as_str(),
            source_path,
            true,
            false,
        )?,
        None => None,
    };
    let bid_price = match &column_mapping.bid_column {
        Some(column_name) => normalize_optional_numeric_value(
            raw_row.get(column_name),
            format!("{row_field_path}.{column_name}").as_str(),
            source_path,
            true,
            false,
        )?,
        None => None,
    };
    let ask_price = match &column_mapping.ask_column {
        Some(column_name) => normalize_optional_numeric_value(
            raw_row.get(column_name),
            format!("{row_field_path}.{column_name}").as_str(),
            source_path,
            true,
            false,
        )?,
        None => None,
    };

    let implied_volatility = match &column_mapping.implied_volatility_column {
        Some(column_name) => normalize_optional_numeric_value(
            raw_row.get(column_name),
            format!("{row_field_path}.{column_name}").as_str(),
            source_path,
            true,
            false,
        )?,
        None => None,
    };

    let quote_value = match normalization_policy.option_quote_input_policy {
        Some(MarketDataOptionQuoteInputPolicy::MidPriceColumn) => mid_price.ok_or_else(|| {
            MarketDataLoadingError::InvalidValue {
                field_path: format!(
                    "{row_field_path}.{}",
                    column_mapping.mid_price_column.as_deref().unwrap_or("mid_price")
                ),
                message: "is required for mid_price_column option normalization and cannot be blank".to_string(),
            }
        })?,
        Some(MarketDataOptionQuoteInputPolicy::BidAskMidpoint) => match (bid_price, ask_price) {
            (Some(bid), Some(ask)) if ask >= bid => (bid + ask) / 2.0,
            (Some(_), Some(_)) => {
                return Err(MarketDataLoadingError::InvalidValue {
                    field_path: row_field_path,
                    message: "ask price must be greater than or equal to bid price for option bid_ask_midpoint derivation".to_string(),
                })
            }
            _ => {
                return Err(MarketDataLoadingError::InvalidValue {
                    field_path: row_field_path,
                    message: "bid_column and ask_column must both contain values to derive option bid_ask_midpoint".to_string(),
                })
            }
        },
        Some(MarketDataOptionQuoteInputPolicy::ImpliedVolatilityColumn) => {
            implied_volatility.ok_or_else(|| MarketDataLoadingError::InvalidValue {
                field_path: format!(
                    "{row_field_path}.{}",
                    column_mapping
                        .implied_volatility_column
                        .as_deref()
                        .unwrap_or("implied_volatility")
                ),
                message: "is required for implied_volatility_column option normalization and cannot be blank".to_string(),
            })?
        }
        None => {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: "normalization_policy.option_quote_input_policy".to_string(),
                message: "is required to normalize option_surface_source".to_string(),
            })
        }
    };

    Ok(LoadedOptionSurfaceRecord {
        quote_timestamp_utc_nanos: quote_timestamp,
        expiry_timestamp_utc_nanos: expiry_timestamp,
        strike,
        option_type,
        quote_value,
        mid_price,
        bid_price,
        ask_price,
        implied_volatility,
        source_row_number,
    })
}

pub(super) fn option_surface_record_dedup_key(
    row: &LoadedOptionSurfaceRecord,
) -> (
    i64,
    i64,
    u64,
    String,
    u64,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
) {
    (
        row.quote_timestamp_utc_nanos,
        row.expiry_timestamp_utc_nanos,
        row.strike.to_bits(),
        row.option_type.clone(),
        row.quote_value.to_bits(),
        normalized_float_key(row.mid_price),
        normalized_float_key(row.bid_price),
        normalized_float_key(row.ask_price),
        normalized_float_key(row.implied_volatility),
    )
}
