use csv::StringRecord;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use std::collections::{BTreeMap, HashSet};
use std::fs::File;

use super::contracts::{
    MarketDataInputFileFormat, MarketDataInputFileReference, MarketDataLoadingError,
};
use super::shared::{RawRow, RawValue};

pub(super) fn load_source_columns(
    file_reference: &MarketDataInputFileReference,
    field_path: &str,
) -> Result<Vec<String>, MarketDataLoadingError> {
    match file_reference.file_format {
        MarketDataInputFileFormat::Csv => {
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(true)
                .from_path(&file_reference.path)
                .map_err(|error| MarketDataLoadingError::Csv {
                    field_path: format!("{field_path}.path"),
                    message: error.to_string(),
                })?;

            let headers = reader
                .headers()
                .map_err(|error| MarketDataLoadingError::Csv {
                    field_path: format!("{field_path}.path"),
                    message: error.to_string(),
                })?;

            Ok(headers.iter().map(ToOwned::to_owned).collect())
        }
        MarketDataInputFileFormat::Parquet => {
            let file =
                File::open(&file_reference.path).map_err(|error| MarketDataLoadingError::Io {
                    field_path: format!("{field_path}.path"),
                    message: error.to_string(),
                })?;

            let reader = SerializedFileReader::new(file).map_err(|error| {
                MarketDataLoadingError::Parquet {
                    field_path: format!("{field_path}.path"),
                    message: error.to_string(),
                }
            })?;

            let schema = reader.metadata().file_metadata().schema_descr();

            Ok(schema
                .columns()
                .iter()
                .map(|column| column.name().to_string())
                .collect())
        }
        MarketDataInputFileFormat::Json => Err(MarketDataLoadingError::InvalidValue {
            field_path: format!("{field_path}.file_format"),
            message: "must be one of: csv, parquet".to_string(),
        }),
    }
}

pub(super) fn probe_source_row_count_estimate(
    file_reference: &MarketDataInputFileReference,
    field_path: &str,
) -> Result<Option<usize>, MarketDataLoadingError> {
    match file_reference.file_format {
        MarketDataInputFileFormat::Csv => Ok(None),
        MarketDataInputFileFormat::Parquet => {
            let file =
                File::open(&file_reference.path).map_err(|error| MarketDataLoadingError::Io {
                    field_path: format!("{field_path}.path"),
                    message: error.to_string(),
                })?;

            let reader = SerializedFileReader::new(file).map_err(|error| {
                MarketDataLoadingError::Parquet {
                    field_path: format!("{field_path}.path"),
                    message: error.to_string(),
                }
            })?;

            let row_count = usize::try_from(reader.metadata().file_metadata().num_rows()).map_err(
                |_| MarketDataLoadingError::Parquet {
                    field_path: format!("{field_path}.path"),
                    message: "row-count metadata exceeds the supported usize range".to_string(),
                },
            )?;

            Ok(Some(row_count))
        }
        MarketDataInputFileFormat::Json => Err(MarketDataLoadingError::InvalidValue {
            field_path: format!("{field_path}.file_format"),
            message: "must be one of: csv, parquet".to_string(),
        }),
    }
}

pub(super) fn load_rows(
    file_reference: &MarketDataInputFileReference,
    field_path: &str,
) -> Result<Vec<RawRow>, MarketDataLoadingError> {
    match file_reference.file_format {
        MarketDataInputFileFormat::Csv => load_csv_rows(file_reference, field_path),
        MarketDataInputFileFormat::Parquet => load_parquet_rows(file_reference, field_path),
        MarketDataInputFileFormat::Json => Err(MarketDataLoadingError::InvalidValue {
            field_path: format!("{field_path}.file_format"),
            message: "must be one of: csv, parquet".to_string(),
        }),
    }
}

pub(super) fn load_csv_rows(
    file_reference: &MarketDataInputFileReference,
    field_path: &str,
) -> Result<Vec<RawRow>, MarketDataLoadingError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(&file_reference.path)
        .map_err(|error| MarketDataLoadingError::Csv {
            field_path: format!("{field_path}.path"),
            message: error.to_string(),
        })?;

    let headers = reader
        .headers()
        .map_err(|error| MarketDataLoadingError::Csv {
            field_path: format!("{field_path}.path"),
            message: error.to_string(),
        })?
        .clone();

    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|error| MarketDataLoadingError::Csv {
            field_path: format!("{field_path}.path"),
            message: error.to_string(),
        })?;

        rows.push(raw_row_from_csv_record(&headers, &record));
    }

    Ok(rows)
}

pub(super) fn raw_row_from_csv_record(headers: &StringRecord, record: &StringRecord) -> RawRow {
    let mut row = BTreeMap::new();
    for (header, value) in headers.iter().zip(record.iter()) {
        row.insert(header.to_string(), RawValue::Text(value.to_string()));
    }

    row
}

pub(super) fn load_parquet_rows(
    file_reference: &MarketDataInputFileReference,
    field_path: &str,
) -> Result<Vec<RawRow>, MarketDataLoadingError> {
    let file = File::open(&file_reference.path).map_err(|error| MarketDataLoadingError::Io {
        field_path: format!("{field_path}.path"),
        message: error.to_string(),
    })?;

    let reader =
        SerializedFileReader::new(file).map_err(|error| MarketDataLoadingError::Parquet {
            field_path: format!("{field_path}.path"),
            message: error.to_string(),
        })?;

    let iter = reader
        .get_row_iter(None)
        .map_err(|error| MarketDataLoadingError::Parquet {
            field_path: format!("{field_path}.path"),
            message: error.to_string(),
        })?;

    let mut rows = Vec::new();
    for row_result in iter {
        let row = row_result.map_err(|error| MarketDataLoadingError::Parquet {
            field_path: format!("{field_path}.path"),
            message: error.to_string(),
        })?;

        let mut raw_row = BTreeMap::new();

        for (column_name, field) in row.get_column_iter() {
            raw_row.insert(column_name.to_string(), raw_value_from_parquet_field(field));
        }

        rows.push(raw_row);
    }

    Ok(rows)
}

pub(super) fn raw_value_from_parquet_field(field: &Field) -> RawValue {
    match field {
        Field::Null => RawValue::Null,
        Field::Bool(_) => RawValue::Boolean,
        Field::Byte(value) => RawValue::Integer(i64::from(*value)),
        Field::Short(value) => RawValue::Integer(i64::from(*value)),
        Field::Int(value) => RawValue::Integer(i64::from(*value)),
        Field::Long(value) => RawValue::Integer(*value),
        Field::UByte(value) => RawValue::Integer(i64::from(*value)),
        Field::UShort(value) => RawValue::Integer(i64::from(*value)),
        Field::UInt(value) => RawValue::Integer(i64::from(*value)),
        Field::ULong(value) => RawValue::Float(*value as f64),
        Field::Float(value) => RawValue::Float(f64::from(*value)),
        Field::Double(value) => RawValue::Float(*value),
        Field::Str(value) => RawValue::Text(value.clone()),
        Field::Bytes(value) => RawValue::Text(String::from_utf8_lossy(value.data()).into_owned()),
        _ => RawValue::Text(field.to_string()),
    }
}

pub(super) fn validate_columns_exist(
    available_columns: &[String],
    required_columns: &[String],
    field_path: &str,
    source_path: &str,
) -> Result<(), MarketDataLoadingError> {
    let available = available_columns.iter().cloned().collect::<HashSet<_>>();

    let missing = required_columns
        .iter()
        .filter(|column| !available.contains(*column))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }

    Err(MarketDataLoadingError::MissingColumn {
        field_path: field_path.to_string(),
        message: format!(
            "references source columns that are not present in the file. Missing columns: {}. Available columns: {}. Update the explicit column mapping so Janiform does not have to guess the schema. source_path={source_path}",
            missing.join(", "),
            available_columns.join(", ")
        ),
    })
}
