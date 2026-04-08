use std::collections::HashSet;

use arrow_array::RecordBatch;

use super::column_resolution::{
    resolve_option_surface_column_mapping, resolve_option_surface_quote_input_policy,
};
use super::contracts::{
    MarketDataInputFileFormat, MarketDataLoadingError, MarketDataNormalizationPolicy,
    MarketDataNormalizedOptionSurfaceRecord, MarketDataOptionQuoteInputPolicy,
    MarketDataOptionSurfaceColumnMapping, MarketDataOptionSurfaceLoadingInput,
    MarketDataSourceSummary,
};
use super::normalization::{
    normalize_option_type, normalize_optional_numeric_value, normalize_required_numeric_value,
    normalize_timestamp_value,
};
use super::rows::{
    column_index, load_projected_parquet_record_batch_reader, load_rows, load_source_columns,
    probe_source_row_count_estimate, raw_value_from_arrow_array, validate_columns_exist,
};
use super::shared::{
    LoadedOptionSurfaceRecord, OptionSurfaceLoadOutput, RawRow, RawValue, file_format_string,
    normalized_float_key, option_quote_policy_string, required_option_surface_source_columns,
};

pub(super) fn load_option_surface_records(
    option_surface_input: &MarketDataOptionSurfaceLoadingInput,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Result<OptionSurfaceLoadOutput, MarketDataLoadingError> {
    let available_columns =
        load_source_columns(&option_surface_input.source, "option_surface_source")?;
    let resolved_option_quote_input_policy = resolve_option_surface_quote_input_policy(
        available_columns.as_slice(),
        &option_surface_input.column_mapping,
        normalization_policy,
        &option_surface_input.source.path,
    )?;
    let effective_normalization_policy = MarketDataNormalizationPolicy {
        option_quote_input_policy: Some(resolved_option_quote_input_policy),
        ..normalization_policy.clone()
    };
    let resolved_column_mapping = resolve_option_surface_column_mapping(
        available_columns.as_slice(),
        &option_surface_input.column_mapping,
        &effective_normalization_policy,
        &option_surface_input.source.path,
    )?;
    let required_columns = required_option_surface_source_columns(
        &resolved_column_mapping,
        &effective_normalization_policy,
    );
    validate_columns_exist(
        &available_columns,
        &required_columns,
        "column_mapping.option_surface",
        &option_surface_input.source.path,
    )?;

    let (mut normalized_rows, source_row_count) = match option_surface_input.source.file_format {
        MarketDataInputFileFormat::Csv => load_csv_option_surface_rows(
            &MarketDataOptionSurfaceLoadingInput {
                source: option_surface_input.source.clone(),
                column_mapping: resolved_column_mapping.clone(),
            },
            &effective_normalization_policy,
        )?,
        MarketDataInputFileFormat::Parquet => load_parquet_option_surface_rows(
            &MarketDataOptionSurfaceLoadingInput {
                source: option_surface_input.source.clone(),
                column_mapping: resolved_column_mapping.clone(),
            },
            &effective_normalization_policy,
        )?,
        MarketDataInputFileFormat::Json => {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: "option_surface_source.file_format".to_string(),
                message: "must be one of: csv, parquet".to_string(),
            });
        }
    };

    let mut seen_keys = HashSet::new();
    for normalized in &normalized_rows {
        if !seen_keys.insert(option_surface_record_dedup_key(normalized)) {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: "option_surface_source".to_string(),
                message: "contains duplicate normalized rows".to_string(),
            });
        }
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

    let retained_row_count = normalized_rows.len();
    let option_surface_quote_values = normalized_rows.iter().map(|row| row.quote_value).collect();

    Ok(OptionSurfaceLoadOutput {
        option_surface_quote_values,
        option_surface_records: normalized_rows,
        summary: MarketDataSourceSummary {
            path: option_surface_input.source.path.clone(),
            file_format: file_format_string(option_surface_input.source.file_format).to_string(),
            provenance_label: option_surface_input.source.provenance_label.clone(),
            source_row_count,
            retained_row_count,
            dropped_duplicate_row_count: 0,
            sort_records_by_timestamp: effective_normalization_policy.sort_records_by_timestamp,
            applied_timestamp_timezone: Some(
                effective_normalization_policy.timestamp_timezone.clone(),
            ),
            canonical_value_field: Some("quote_value".to_string()),
            value_mapping_policy: effective_normalization_policy
                .option_quote_input_policy
                .map(option_quote_policy_string),
        },
    })
}

fn load_csv_option_surface_rows(
    option_surface_input: &MarketDataOptionSurfaceLoadingInput,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Result<(Vec<LoadedOptionSurfaceRecord>, usize), MarketDataLoadingError> {
    let raw_rows = load_rows(&option_surface_input.source, "option_surface_source")?;
    let source_row_count = raw_rows.len();

    let mut normalized_rows = Vec::with_capacity(source_row_count);

    for (row_index, raw_row) in raw_rows.iter().enumerate() {
        normalized_rows.push(normalize_option_surface_row(
            raw_row,
            row_index + 1,
            &option_surface_input.column_mapping,
            normalization_policy,
            &option_surface_input.source.path,
        )?);
    }

    Ok((normalized_rows, source_row_count))
}

fn load_parquet_option_surface_rows(
    option_surface_input: &MarketDataOptionSurfaceLoadingInput,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Result<(Vec<LoadedOptionSurfaceRecord>, usize), MarketDataLoadingError> {
    let projected_columns = required_option_surface_source_columns(
        &option_surface_input.column_mapping,
        normalization_policy,
    );

    let source_row_count =
        probe_source_row_count_estimate(&option_surface_input.source, "option_surface_source")?
            .ok_or_else(|| MarketDataLoadingError::Parquet {
                field_path: "option_surface_source.path".to_string(),
                message: "parquet source row-count metadata is unavailable".to_string(),
            })?;

    let mut reader = load_projected_parquet_record_batch_reader(
        &option_surface_input.source,
        "option_surface_source",
        projected_columns.as_slice(),
    )?;

    let mut normalized_rows = Vec::with_capacity(source_row_count);
    let mut source_row_number: usize = 0;

    while let Some(batch_result) = reader.next() {
        let batch = batch_result.map_err(|error| MarketDataLoadingError::Parquet {
            field_path: "option_surface_source.path".to_string(),
            message: error.to_string(),
        })?;

        let projected_columns = ProjectedOptionSurfaceColumns::new(
            &batch,
            &option_surface_input.column_mapping,
            &option_surface_input.source.path,
        )?;

        for row_index in 0..batch.num_rows() {
            source_row_number = source_row_number.saturating_add(1);

            let quote_timestamp_raw_value = projected_columns.required_value(
                &batch,
                projected_columns.quote_timestamp_index,
                row_index,
                "column_mapping.option_surface.quote_timestamp_column",
                &option_surface_input.source.path,
            )?;
            let expiry_timestamp_raw_value = projected_columns.required_value(
                &batch,
                projected_columns.expiry_timestamp_index,
                row_index,
                "column_mapping.option_surface.expiry_timestamp_column",
                &option_surface_input.source.path,
            )?;
            let strike_raw_value = projected_columns.required_value(
                &batch,
                projected_columns.strike_index,
                row_index,
                "column_mapping.option_surface.strike_column",
                &option_surface_input.source.path,
            )?;
            let option_type_raw_value = projected_columns.required_value(
                &batch,
                projected_columns.option_type_index,
                row_index,
                "column_mapping.option_surface.option_type_column",
                &option_surface_input.source.path,
            )?;
            let mid_price_raw_value = projected_columns.optional_value(
                &batch,
                projected_columns.mid_price_index,
                row_index,
                "column_mapping.option_surface.mid_price_column",
                &option_surface_input.source.path,
            )?;
            let bid_price_raw_value = projected_columns.optional_value(
                &batch,
                projected_columns.bid_price_index,
                row_index,
                "column_mapping.option_surface.bid_column",
                &option_surface_input.source.path,
            )?;
            let ask_price_raw_value = projected_columns.optional_value(
                &batch,
                projected_columns.ask_price_index,
                row_index,
                "column_mapping.option_surface.ask_column",
                &option_surface_input.source.path,
            )?;
            let implied_volatility_raw_value = projected_columns.optional_value(
                &batch,
                projected_columns.implied_volatility_index,
                row_index,
                "column_mapping.option_surface.implied_volatility_column",
                &option_surface_input.source.path,
            )?;

            normalized_rows.push(normalize_option_surface_values(
                Some(&quote_timestamp_raw_value),
                Some(&expiry_timestamp_raw_value),
                Some(&strike_raw_value),
                Some(&option_type_raw_value),
                mid_price_raw_value.as_ref(),
                bid_price_raw_value.as_ref(),
                ask_price_raw_value.as_ref(),
                implied_volatility_raw_value.as_ref(),
                source_row_number,
                &option_surface_input.column_mapping,
                normalization_policy,
                &option_surface_input.source.path,
            )?);
        }
    }

    Ok((normalized_rows, source_row_count))
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
    normalize_option_surface_values(
        column_mapping
            .quote_timestamp_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        column_mapping
            .expiry_timestamp_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        column_mapping
            .strike_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        column_mapping
            .option_type_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        column_mapping
            .mid_price_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        column_mapping
            .bid_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        column_mapping
            .ask_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        column_mapping
            .implied_volatility_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        source_row_number,
        column_mapping,
        normalization_policy,
        source_path,
    )
}

pub(super) fn normalize_option_surface_values(
    quote_timestamp_raw_value: Option<&super::shared::RawValue>,
    expiry_timestamp_raw_value: Option<&super::shared::RawValue>,
    strike_raw_value: Option<&super::shared::RawValue>,
    option_type_raw_value: Option<&super::shared::RawValue>,
    mid_price_raw_value: Option<&super::shared::RawValue>,
    bid_price_raw_value: Option<&super::shared::RawValue>,
    ask_price_raw_value: Option<&super::shared::RawValue>,
    implied_volatility_raw_value: Option<&super::shared::RawValue>,
    source_row_number: usize,
    column_mapping: &MarketDataOptionSurfaceColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    source_path: &str,
) -> Result<LoadedOptionSurfaceRecord, MarketDataLoadingError> {
    let row_field_path = format!("option_surface_source.rows[{source_row_number}]");
    let quote_timestamp_column = column_mapping
        .quote_timestamp_column
        .as_deref()
        .ok_or_else(|| MarketDataLoadingError::InvalidValue {
            field_path: "column_mapping.option_surface.quote_timestamp_column".to_string(),
            message: "could not resolve an option quote timestamp column".to_string(),
        })?;
    let expiry_timestamp_column = column_mapping
        .expiry_timestamp_column
        .as_deref()
        .ok_or_else(|| MarketDataLoadingError::InvalidValue {
            field_path: "column_mapping.option_surface.expiry_timestamp_column".to_string(),
            message: "could not resolve an option expiry timestamp column".to_string(),
        })?;
    let strike_column = column_mapping.strike_column.as_deref().ok_or_else(|| {
        MarketDataLoadingError::InvalidValue {
            field_path: "column_mapping.option_surface.strike_column".to_string(),
            message: "could not resolve an option strike column".to_string(),
        }
    })?;
    let option_type_column = column_mapping
        .option_type_column
        .as_deref()
        .ok_or_else(|| MarketDataLoadingError::InvalidValue {
            field_path: "column_mapping.option_surface.option_type_column".to_string(),
            message: "could not resolve an option type column".to_string(),
        })?;

    let quote_timestamp = normalize_timestamp_value(
        quote_timestamp_raw_value,
        format!("{row_field_path}.{quote_timestamp_column}").as_str(),
        Some(normalization_policy.timestamp_timezone.as_str()),
        source_path,
    )?;

    let expiry_timestamp = normalize_timestamp_value(
        expiry_timestamp_raw_value,
        format!("{row_field_path}.{expiry_timestamp_column}").as_str(),
        Some(normalization_policy.timestamp_timezone.as_str()),
        source_path,
    )?;

    let strike = normalize_required_numeric_value(
        strike_raw_value,
        format!("{row_field_path}.{strike_column}").as_str(),
        source_path,
        false,
        false,
    )?;

    let option_type = normalize_option_type(
        option_type_raw_value,
        format!("{row_field_path}.{option_type_column}").as_str(),
        source_path,
    )?;

    let mid_price = match &column_mapping.mid_price_column {
        Some(column_name) => normalize_optional_numeric_value(
            mid_price_raw_value,
            format!("{row_field_path}.{column_name}").as_str(),
            source_path,
            true,
            false,
        )?,
        None => None,
    };
    let bid_price = match &column_mapping.bid_column {
        Some(column_name) => normalize_optional_numeric_value(
            bid_price_raw_value,
            format!("{row_field_path}.{column_name}").as_str(),
            source_path,
            true,
            false,
        )?,
        None => None,
    };
    let ask_price = match &column_mapping.ask_column {
        Some(column_name) => normalize_optional_numeric_value(
            ask_price_raw_value,
            format!("{row_field_path}.{column_name}").as_str(),
            source_path,
            true,
            false,
        )?,
        None => None,
    };

    let implied_volatility = match &column_mapping.implied_volatility_column {
        Some(column_name) => normalize_optional_numeric_value(
            implied_volatility_raw_value,
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

struct ProjectedOptionSurfaceColumns {
    quote_timestamp_index: usize,
    expiry_timestamp_index: usize,
    strike_index: usize,
    option_type_index: usize,
    mid_price_index: Option<usize>,
    bid_price_index: Option<usize>,
    ask_price_index: Option<usize>,
    implied_volatility_index: Option<usize>,
}

impl ProjectedOptionSurfaceColumns {
    fn new(
        batch: &RecordBatch,
        column_mapping: &MarketDataOptionSurfaceColumnMapping,
        source_path: &str,
    ) -> Result<Self, MarketDataLoadingError> {
        let quote_timestamp_column = column_mapping
            .quote_timestamp_column
            .as_deref()
            .ok_or_else(|| MarketDataLoadingError::InvalidValue {
                field_path: "column_mapping.option_surface.quote_timestamp_column".to_string(),
                message: "could not resolve an option quote timestamp column".to_string(),
            })?;
        let expiry_timestamp_column = column_mapping
            .expiry_timestamp_column
            .as_deref()
            .ok_or_else(|| MarketDataLoadingError::InvalidValue {
                field_path: "column_mapping.option_surface.expiry_timestamp_column".to_string(),
                message: "could not resolve an option expiry timestamp column".to_string(),
            })?;
        let strike_column = column_mapping.strike_column.as_deref().ok_or_else(|| {
            MarketDataLoadingError::InvalidValue {
                field_path: "column_mapping.option_surface.strike_column".to_string(),
                message: "could not resolve an option strike column".to_string(),
            }
        })?;
        let option_type_column = column_mapping
            .option_type_column
            .as_deref()
            .ok_or_else(|| MarketDataLoadingError::InvalidValue {
                field_path: "column_mapping.option_surface.option_type_column".to_string(),
                message: "could not resolve an option type column".to_string(),
            })?;

        Ok(Self {
            quote_timestamp_index: column_index(
                batch,
                quote_timestamp_column,
                "column_mapping.option_surface.quote_timestamp_column",
                source_path,
            )?,
            expiry_timestamp_index: column_index(
                batch,
                expiry_timestamp_column,
                "column_mapping.option_surface.expiry_timestamp_column",
                source_path,
            )?,
            strike_index: column_index(
                batch,
                strike_column,
                "column_mapping.option_surface.strike_column",
                source_path,
            )?,
            option_type_index: column_index(
                batch,
                option_type_column,
                "column_mapping.option_surface.option_type_column",
                source_path,
            )?,
            mid_price_index: column_mapping
                .mid_price_column
                .as_ref()
                .map(|column_name| {
                    column_index(
                        batch,
                        column_name,
                        "column_mapping.option_surface.mid_price_column",
                        source_path,
                    )
                })
                .transpose()?,
            bid_price_index: column_mapping
                .bid_column
                .as_ref()
                .map(|column_name| {
                    column_index(
                        batch,
                        column_name,
                        "column_mapping.option_surface.bid_column",
                        source_path,
                    )
                })
                .transpose()?,
            ask_price_index: column_mapping
                .ask_column
                .as_ref()
                .map(|column_name| {
                    column_index(
                        batch,
                        column_name,
                        "column_mapping.option_surface.ask_column",
                        source_path,
                    )
                })
                .transpose()?,
            implied_volatility_index: column_mapping
                .implied_volatility_column
                .as_ref()
                .map(|column_name| {
                    column_index(
                        batch,
                        column_name,
                        "column_mapping.option_surface.implied_volatility_column",
                        source_path,
                    )
                })
                .transpose()?,
        })
    }

    fn required_value(
        &self,
        batch: &RecordBatch,
        column_index: usize,
        row_index: usize,
        field_path: &str,
        source_path: &str,
    ) -> Result<RawValue, MarketDataLoadingError> {
        raw_value_from_arrow_array(
            batch.column(column_index).as_ref(),
            row_index,
            field_path,
            source_path,
        )
    }

    fn optional_value(
        &self,
        batch: &RecordBatch,
        column_index: Option<usize>,
        row_index: usize,
        field_path: &str,
        source_path: &str,
    ) -> Result<Option<RawValue>, MarketDataLoadingError> {
        column_index
            .map(|index| {
                raw_value_from_arrow_array(
                    batch.column(index).as_ref(),
                    row_index,
                    field_path,
                    source_path,
                )
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Float64Array, StringArray, TimestampNanosecondArray};
    use arrow_schema::{DataType, Field, Schema, TimeUnit};

    use super::{
        MarketDataInputFileFormat, MarketDataNormalizationPolicy,
        MarketDataOptionSurfaceColumnMapping, MarketDataOptionSurfaceLoadingInput,
        load_option_surface_records,
    };
    use crate::MarketDataInputFileReference;
    use crate::market_data::loading::test_support::{unique_test_path, write_parquet_record_batch};

    #[test]
    fn load_option_surface_records_reads_projected_parquet_option_columns() {
        let parquet_path = unique_test_path("option-surface.parquet");
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "quote_time",
                DataType::Timestamp(TimeUnit::Nanosecond, None),
                false,
            ),
            Field::new(
                "expiry_time",
                DataType::Timestamp(TimeUnit::Nanosecond, None),
                false,
            ),
            Field::new("strike_price", DataType::Float64, false),
            Field::new("option_kind", DataType::Utf8, false),
            Field::new("best_bid", DataType::Float64, false),
            Field::new("best_ask", DataType::Float64, false),
            Field::new("ignored_label", DataType::Utf8, false),
        ]));

        write_parquet_record_batch(
            parquet_path.as_path(),
            schema,
            vec![
                Arc::new(TimestampNanosecondArray::from(vec![
                    1_711_957_660_000_000_000_i64,
                    1_711_957_600_000_000_000_i64,
                ])) as ArrayRef,
                Arc::new(TimestampNanosecondArray::from(vec![
                    1_712_563_200_000_000_000_i64,
                    1_712_476_800_000_000_000_i64,
                ])) as ArrayRef,
                Arc::new(Float64Array::from(vec![4300.0_f64, 4200.0_f64])) as ArrayRef,
                Arc::new(StringArray::from(vec!["call", "put"])) as ArrayRef,
                Arc::new(Float64Array::from(vec![12.0_f64, 8.0_f64])) as ArrayRef,
                Arc::new(Float64Array::from(vec![13.0_f64, 9.0_f64])) as ArrayRef,
                Arc::new(StringArray::from(vec!["later", "earlier"])) as ArrayRef,
            ],
        );

        let output = load_option_surface_records(
            &MarketDataOptionSurfaceLoadingInput {
                source: MarketDataInputFileReference {
                    path: parquet_path.to_string_lossy().into_owned(),
                    file_format: MarketDataInputFileFormat::Parquet,
                    provenance_label: "options".to_string(),
                },
                column_mapping: MarketDataOptionSurfaceColumnMapping {
                    quote_timestamp_column: Some("quote_time".to_string()),
                    expiry_timestamp_column: Some("expiry_time".to_string()),
                    strike_column: Some("strike_price".to_string()),
                    option_type_column: Some("option_kind".to_string()),
                    mid_price_column: None,
                    bid_column: Some("best_bid".to_string()),
                    ask_column: Some("best_ask".to_string()),
                    implied_volatility_column: None,
                },
            },
            &MarketDataNormalizationPolicy {
                timestamp_timezone: "UTC".to_string(),
                sort_records_by_timestamp: true,
                duplicate_record_policy: None,
                tick_price_input_policy: None,
                option_quote_input_policy: None,
            },
        )
        .expect("projected parquet option-surface normalization should succeed");

        assert_eq!(output.summary.source_row_count, 2);
        assert_eq!(output.summary.retained_row_count, 2);
        assert_eq!(output.option_surface_records.len(), 2);
        assert_eq!(output.option_surface_records[0].strike, 4200.0);
        assert_eq!(output.option_surface_records[0].option_type, "put");
        assert_eq!(output.option_surface_records[0].quote_value, 8.5);
        assert_eq!(output.option_surface_records[1].quote_value, 12.5);
        assert_eq!(output.option_surface_quote_values, vec![8.5, 12.5]);
        assert_eq!(
            output.summary.value_mapping_policy.as_deref(),
            Some("bid_ask_midpoint")
        );
    }
}
