use arrow_array::{
    Array, BooleanArray, Date32Array, Date64Array, Decimal32Array, Decimal64Array, Decimal128Array,
    Float32Array, Float64Array, Int8Array, Int16Array, Int32Array, Int64Array, LargeStringArray,
    RecordBatch, StringArray, TimestampMicrosecondArray, TimestampMillisecondArray,
    TimestampNanosecondArray, TimestampSecondArray, UInt8Array, UInt16Array, UInt32Array,
    UInt64Array,
};
use janiform_foundations::csv::document::{
    CsvDocumentLoadError, StringRecord, load_csv_column_names, open_csv_reader,
};
use janiform_foundations::parquet::batch::column_index as parquet_column_index;
use janiform_foundations::parquet::document::{
    ParquetDocumentLoadError, load_parquet_column_names, load_parquet_row_count,
    open_projected_parquet_record_batch_reader as open_foundation_projected_parquet_record_batch_reader,
    open_serialized_parquet_reader,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReader;
use parquet::file::reader::FileReader;
use parquet::record::Field;
use std::collections::{BTreeMap, HashSet};

use super::contracts::{
    MarketDataInputFileFormat, MarketDataInputFileReference, MarketDataLoadingError,
};
use super::shared::{RawRow, RawValue};

const DECIMAL_RADIX_F64: f64 = 10.0;

fn csv_document_error(error: CsvDocumentLoadError, field_path: &str) -> MarketDataLoadingError {
    match error {
        CsvDocumentLoadError::Read { source, .. } => MarketDataLoadingError::Csv {
            field_path: format!("{field_path}.path"),
            message: source.to_string(),
        },
    }
}

fn parquet_document_error(
    error: ParquetDocumentLoadError,
    field_path: &str,
) -> MarketDataLoadingError {
    match error {
        ParquetDocumentLoadError::Read { source, .. } => MarketDataLoadingError::Io {
            field_path: format!("{field_path}.path"),
            message: source.to_string(),
        },
        ParquetDocumentLoadError::Parse { source, .. } => MarketDataLoadingError::Parquet {
            field_path: format!("{field_path}.path"),
            message: source.to_string(),
        },
        ParquetDocumentLoadError::RowCountOutOfRange { .. } => MarketDataLoadingError::Parquet {
            field_path: format!("{field_path}.path"),
            message: "row-count metadata exceeds the supported usize range".to_string(),
        },
    }
}

pub(super) fn load_source_columns(
    file_reference: &MarketDataInputFileReference,
    field_path: &str,
) -> Result<Vec<String>, MarketDataLoadingError> {
    match file_reference.file_format {
        MarketDataInputFileFormat::Csv => load_csv_column_names(file_reference.path.as_ref())
            .map_err(|error| csv_document_error(error, field_path)),
        MarketDataInputFileFormat::Parquet => {
            load_parquet_column_names(file_reference.path.as_ref())
                .map_err(|error| parquet_document_error(error, field_path))
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
        MarketDataInputFileFormat::Parquet => load_parquet_row_count(file_reference.path.as_ref())
            .map(Some)
            .map_err(|error| parquet_document_error(error, field_path)),
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

pub(super) fn open_projected_parquet_record_batch_reader(
    file_reference: &MarketDataInputFileReference,
    field_path: &str,
    projected_columns: &[String],
) -> Result<ParquetRecordBatchReader, MarketDataLoadingError> {
    open_foundation_projected_parquet_record_batch_reader(
        file_reference.path.as_ref(),
        projected_columns.iter().map(String::as_str),
    )
    .map_err(|error| parquet_document_error(error, field_path))
}

pub(super) fn load_csv_rows(
    file_reference: &MarketDataInputFileReference,
    field_path: &str,
) -> Result<Vec<RawRow>, MarketDataLoadingError> {
    let mut reader = open_csv_reader(file_reference.path.as_ref())
        .map_err(|error| csv_document_error(error, field_path))?;

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
    let reader = open_serialized_parquet_reader(file_reference.path.as_ref())
        .map_err(|error| parquet_document_error(error, field_path))?;

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

pub(super) fn column_index(
    batch: &RecordBatch,
    column_name: &str,
    field_path: &str,
    source_path: &str,
) -> Result<usize, MarketDataLoadingError> {
    parquet_column_index(batch, column_name).ok_or_else(|| {
        MarketDataLoadingError::MissingColumn {
            field_path: field_path.to_string(),
            message: format!(
                "references source columns that are not present in the projected parquet batch. Missing columns: {column_name}. Update the explicit column mapping so Janiform does not have to guess the schema. source_path={source_path}"
            ),
        }
    })
}

pub(super) fn raw_value_from_arrow_array(
    array: &dyn Array,
    row_index: usize,
    field_path: &str,
    source_path: &str,
) -> Result<RawValue, MarketDataLoadingError> {
    if array.is_null(row_index) {
        return Ok(RawValue::Null);
    }

    if let Some(values) = array.as_any().downcast_ref::<BooleanArray>() {
        return Ok(if values.value(row_index) {
            RawValue::Text("true".to_string())
        } else {
            RawValue::Text("false".to_string())
        });
    }

    if let Some(values) = array.as_any().downcast_ref::<Int8Array>() {
        return Ok(RawValue::Integer(i64::from(values.value(row_index))));
    }

    if let Some(values) = array.as_any().downcast_ref::<Int16Array>() {
        return Ok(RawValue::Integer(i64::from(values.value(row_index))));
    }

    if let Some(values) = array.as_any().downcast_ref::<Int32Array>() {
        return Ok(RawValue::Integer(i64::from(values.value(row_index))));
    }

    if let Some(values) = array.as_any().downcast_ref::<Int64Array>() {
        return Ok(RawValue::Integer(values.value(row_index)));
    }

    if let Some(values) = array.as_any().downcast_ref::<UInt8Array>() {
        return Ok(RawValue::Integer(i64::from(values.value(row_index))));
    }

    if let Some(values) = array.as_any().downcast_ref::<UInt16Array>() {
        return Ok(RawValue::Integer(i64::from(values.value(row_index))));
    }

    if let Some(values) = array.as_any().downcast_ref::<UInt32Array>() {
        return Ok(RawValue::Integer(i64::from(values.value(row_index))));
    }

    if let Some(values) = array.as_any().downcast_ref::<UInt64Array>() {
        return Ok(RawValue::Float(values.value(row_index) as f64));
    }

    if let Some(values) = array.as_any().downcast_ref::<Float32Array>() {
        return Ok(RawValue::Float(f64::from(values.value(row_index))));
    }

    if let Some(values) = array.as_any().downcast_ref::<Float64Array>() {
        return Ok(RawValue::Float(values.value(row_index)));
    }

    if let Some(values) = array.as_any().downcast_ref::<Decimal32Array>() {
        return Ok(RawValue::Float(decimal_i32_to_f64(
            values.value(row_index),
            values.scale(),
        )));
    }

    if let Some(values) = array.as_any().downcast_ref::<Decimal64Array>() {
        return Ok(RawValue::Float(decimal_i64_to_f64(
            values.value(row_index),
            values.scale(),
        )));
    }

    if let Some(values) = array.as_any().downcast_ref::<Decimal128Array>() {
        return Ok(RawValue::Float(decimal_i128_to_f64(
            values.value(row_index),
            values.scale(),
        )));
    }

    if let Some(values) = array.as_any().downcast_ref::<StringArray>() {
        return Ok(RawValue::Text(values.value(row_index).to_string()));
    }

    if let Some(values) = array.as_any().downcast_ref::<LargeStringArray>() {
        return Ok(RawValue::Text(values.value(row_index).to_string()));
    }

    if let Some(values) = array.as_any().downcast_ref::<TimestampSecondArray>() {
        return Ok(RawValue::Integer(values.value(row_index)));
    }

    if let Some(values) = array.as_any().downcast_ref::<TimestampMillisecondArray>() {
        return Ok(RawValue::Integer(values.value(row_index)));
    }

    if let Some(values) = array.as_any().downcast_ref::<TimestampMicrosecondArray>() {
        return Ok(RawValue::Integer(values.value(row_index)));
    }

    if let Some(values) = array.as_any().downcast_ref::<TimestampNanosecondArray>() {
        return Ok(RawValue::Integer(values.value(row_index)));
    }

    if let Some(values) = array.as_any().downcast_ref::<Date32Array>() {
        return Ok(RawValue::Integer(i64::from(values.value(row_index))));
    }

    if let Some(values) = array.as_any().downcast_ref::<Date64Array>() {
        return Ok(RawValue::Integer(values.value(row_index)));
    }

    Err(MarketDataLoadingError::Parquet {
        field_path: field_path.to_string(),
        message: format!(
            "encountered unsupported projected parquet column type {:?}. source_path={source_path}",
            array.data_type()
        ),
    })
}

fn decimal_i32_to_f64(value: i32, scale: i8) -> f64 {
    f64::from(value) / decimal_scale_factor(scale)
}

fn decimal_i64_to_f64(value: i64, scale: i8) -> f64 {
    value as f64 / decimal_scale_factor(scale)
}

fn decimal_i128_to_f64(value: i128, scale: i8) -> f64 {
    value as f64 / decimal_scale_factor(scale)
}

fn decimal_scale_factor(scale: i8) -> f64 {
    DECIMAL_RADIX_F64.powi(i32::from(scale))
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
