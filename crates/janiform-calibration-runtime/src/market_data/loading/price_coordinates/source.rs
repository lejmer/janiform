use std::collections::HashMap;

use arrow_array::RecordBatch;

use crate::market_data::loading::contracts::{
    MarketDataInputFileFormat, MarketDataInputFileReference, MarketDataLoadingError,
};
use crate::market_data::loading::rows::{
    column_index, load_rows, open_projected_parquet_record_batch_reader, raw_value_from_arrow_array,
};
use crate::market_data::loading::shared::RawValue;

pub(super) fn load_projected_rows(
    source: &MarketDataInputFileReference,
    projected_columns: &[String],
    max_row_count: Option<usize>,
) -> Result<Vec<HashMap<String, RawValue>>, MarketDataLoadingError> {
    match source.file_format {
        MarketDataInputFileFormat::Csv => {
            let mut rows = Vec::new();

            for raw_row in load_rows(source, "source")? {
                // Intentional copies because projected rows own their column
                // names and raw values after the streaming reader advances.
                let row = projected_columns
                    .iter()
                    .filter_map(|column| {
                        raw_row
                            .get(column)
                            .cloned()
                            .map(|value| (column.clone(), value))
                    })
                    .collect::<HashMap<_, _>>();

                rows.push(row);

                if max_row_count.is_some_and(|maximum_row_count| rows.len() >= maximum_row_count) {
                    break;
                }
            }

            Ok(rows)
        }
        MarketDataInputFileFormat::Parquet => {
            let mut rows = Vec::new();

            let reader =
                open_projected_parquet_record_batch_reader(source, "source", projected_columns)?;

            for batch_result in reader {
                let batch = batch_result.map_err(|error| MarketDataLoadingError::Parquet {
                    field_path: "source.path".to_string(),
                    message: error.to_string(),
                })?;

                append_projected_batch_rows(
                    &batch,
                    projected_columns,
                    &mut rows,
                    source.path.as_str(),
                )?;

                if let Some(maximum_row_count) = max_row_count
                    && rows.len() >= maximum_row_count
                {
                    rows.truncate(maximum_row_count);
                    break;
                }
            }

            Ok(rows)
        }
        MarketDataInputFileFormat::Json => Err(MarketDataLoadingError::InvalidValue {
            field_path: "source.file_format".to_string(),
            message: "must be one of: csv, parquet".to_string(),
        }),
    }
}

pub(super) fn numeric_raw_value(value: Option<&RawValue>) -> Option<f64> {
    match value {
        Some(RawValue::Integer(value)) => value.to_string().parse::<f64>().ok(),
        Some(RawValue::Float(value)) if value.is_finite() => Some(*value),
        Some(RawValue::Text(value)) => value.parse::<f64>().ok().filter(|value| value.is_finite()),
        Some(RawValue::Boolean) | Some(RawValue::Float(_)) | Some(RawValue::Null) | None => None,
    }
}

pub(super) fn integer_raw_value(value: Option<&RawValue>) -> Option<i64> {
    match value {
        Some(RawValue::Integer(value)) => Some(*value),
        Some(RawValue::Text(value)) => value.parse::<i64>().ok(),
        Some(RawValue::Float(value)) if value.is_finite() && value.fract() == 0.0 => {
            value.to_string().parse::<i64>().ok()
        }
        Some(RawValue::Boolean) | Some(RawValue::Float(_)) | Some(RawValue::Null) | None => None,
    }
}

pub(super) fn text_raw_value(value: Option<&RawValue>) -> Option<String> {
    match value {
        Some(RawValue::Text(value)) if !value.trim().is_empty() => Some(value.trim().to_string()),
        Some(RawValue::Integer(value)) => Some(value.to_string()),
        Some(RawValue::Float(value)) if value.is_finite() => Some(value.to_string()),
        Some(RawValue::Boolean)
        | Some(RawValue::Float(_))
        | Some(RawValue::Null)
        | Some(RawValue::Text(_))
        | None => None,
    }
}

pub(super) fn normalized_source_columns(available_columns: &[String]) -> HashMap<String, String> {
    available_columns.iter().fold(
        HashMap::<String, String>::new(),
        |mut normalized, column_name| {
            // Intentional copy preserves the original source column spelling
            // behind the normalized lookup key.
            normalized
                .entry(normalize_identifier(column_name))
                .or_insert_with(|| column_name.clone());
            normalized
        },
    )
}

pub(super) fn first_alias_match(
    available_by_normalized: &HashMap<String, String>,
    aliases: &[&str],
) -> Option<String> {
    aliases
        .iter()
        .find_map(|alias| available_by_normalized.get(&normalize_identifier(alias)))
        .cloned()
}

fn append_projected_batch_rows(
    batch: &RecordBatch,
    projected_columns: &[String],
    rows: &mut Vec<HashMap<String, RawValue>>,
    source_path: &str,
) -> Result<(), MarketDataLoadingError> {
    let column_indices = projected_columns
        .iter()
        .map(|column| column_index(batch, column, "source", source_path))
        .collect::<Result<Vec<_>, _>>()?;

    for row_index in 0..batch.num_rows() {
        let mut row = HashMap::new();

        for (column, column_index) in projected_columns.iter().zip(column_indices.iter()) {
            let value = raw_value_from_arrow_array(
                batch.column(*column_index).as_ref(),
                row_index,
                "source",
                source_path,
            )?;

            // Intentional copy because projected row maps own their column names.
            row.insert(column.clone(), value);
        }

        rows.push(row);
    }

    Ok(())
}

fn normalize_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
