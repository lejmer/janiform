use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::hash::Hash;
use chrono::{DateTime, LocalResult, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use csv::StringRecord;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use serde::{Deserialize, Serialize};

use crate::calendar::ExchangeCalendarRegistry;
use crate::execution_bridge::CalibrationHawkesDirectionalLabelDiagnostics;
use crate::market_data_preprocessing::{
    MarketDataTranslationKnownNewsRecord, MarketDataTranslationPreprocessingError,
    MarketDataTranslationPreprocessingOutput, MarketDataTranslationPreprocessingRequest,
    MarketDataTranslationTickRecord, preprocess_market_data_translation_request,
};

const COMMON_TIMESTAMP_FORMATS: [&str; 37] = [
    "%d.%m.%Y %H:%M:%S.%f%z",
    "%d.%m.%Y %H:%M:%S.%f %z",
    "%d.%m.%Y %H:%M:%S%z",
    "%d.%m.%Y %H:%M:%S %z",
    "%d.%m.%Y %H:%M%z",
    "%d.%m.%Y %H:%M %z",
    "%d.%m.%Y %H:%M:%S.%f",
    "%d.%m.%Y %H:%M:%S",
    "%d.%m.%Y %H:%M",
    "%Y/%m/%d %H:%M:%S.%f%z",
    "%Y/%m/%d %H:%M:%S.%f %z",
    "%Y/%m/%d %H:%M:%S%z",
    "%Y/%m/%d %H:%M:%S %z",
    "%Y/%m/%d %H:%M%z",
    "%Y/%m/%d %H:%M %z",
    "%Y/%m/%d %H:%M:%S.%f",
    "%Y/%m/%d %H:%M:%S",
    "%Y/%m/%d %H:%M",
    "%Y-%m-%d %H:%M:%S.%f%z",
    "%Y-%m-%d %H:%M:%S.%f %z",
    "%Y-%m-%d %H:%M:%S%z",
    "%Y-%m-%d %H:%M:%S %z",
    "%Y-%m-%d %H:%M:%S.%f",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %H:%M",
    "%Y-%m-%dT%H:%M:%S.%f",
    "%Y-%m-%dT%H:%M:%S",
    "%Y%m%d %H:%M:%S.%f",
    "%Y%m%d %H:%M:%S",
    "%Y%m%d %H%M%S.%f",
    "%Y%m%d %H%M%S",
    "%Y%m%dT%H:%M:%S.%f",
    "%Y%m%dT%H:%M:%S",
    "%Y%m%dT%H%M%S.%f",
    "%Y%m%dT%H%M%S",
    "%Y%m%d%H%M%S.%f",
    "%Y%m%d%H%M%S",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataLoadingRequest {
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    pub tick_data_source: MarketDataInputFileReference,
    pub tick_data_column_mapping: MarketDataTickDataColumnMapping,
    pub normalization_policy: MarketDataNormalizationPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_news_input: Option<MarketDataKnownNewsLoadingInput>,
    pub require_hawkes: bool,
    pub require_return_components: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataInputFileReference {
    pub path: String,
    pub file_format: MarketDataInputFileFormat,
    pub provenance_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarketDataInputFileFormat {
    Csv,
    Parquet,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataTickDataColumnMapping {
    pub timestamp_column: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trade_sign_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bid_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_column: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataKnownNewsLoadingInput {
    pub calendar_source: MarketDataInputFileReference,
    pub column_mapping: MarketDataKnownNewsColumnMapping,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar_timezone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataKnownNewsColumnMapping {
    pub event_timestamp_column: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_identifier_column: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataNormalizationPolicy {
    pub timestamp_timezone: String,
    pub sort_records_by_timestamp: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_record_policy: Option<MarketDataDuplicateRecordPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_price_input_policy: Option<MarketDataTickPriceInputPolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketDataDuplicateRecordPolicy {
    KeepFirst,
    KeepLast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketDataTickPriceInputPolicy {
    ExplicitPriceColumn,
    BidAskMidpoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataLoadingOutput {
    pub preprocessing_output: MarketDataTranslationPreprocessingOutput,
    pub tick_data_summary: MarketDataSourceSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_news_summary: Option<MarketDataSourceSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataSourceSummary {
    pub path: String,
    pub file_format: String,
    pub provenance_label: String,
    pub source_row_count: usize,
    pub retained_row_count: usize,
    pub dropped_duplicate_row_count: usize,
    pub sort_records_by_timestamp: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_timestamp_timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_value_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_mapping_policy: Option<String>,
}

#[derive(Debug)]
pub enum MarketDataLoadingError {
    Io { field_path: String, message: String },
    Csv { field_path: String, message: String },
    Parquet { field_path: String, message: String },
    MissingColumn { field_path: String, message: String },
    InvalidValue { field_path: String, message: String },
    Preprocessing(MarketDataTranslationPreprocessingError),
}

impl Display for MarketDataLoadingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io {
                field_path,
                message,
            } => write!(formatter, "{field_path}: {message}"),
            Self::Csv {
                field_path,
                message,
            } => write!(formatter, "{field_path}: {message}"),
            Self::Parquet {
                field_path,
                message,
            } => write!(formatter, "{field_path}: {message}"),
            Self::MissingColumn {
                field_path,
                message,
            } => write!(formatter, "{field_path}: {message}"),
            Self::InvalidValue {
                field_path,
                message,
            } => write!(formatter, "{field_path}: {message}"),
            Self::Preprocessing(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for MarketDataLoadingError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TradeSign {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
struct LoadedTickRecord {
    timestamp_utc_nanos: i64,
    price: Option<f64>,
    trade_price: Option<f64>,
    trade_sign: Option<TradeSign>,
    bid_price: Option<f64>,
    ask_price: Option<f64>,
    close_price: Option<f64>,
    source_row_number: usize,
}

#[derive(Debug, Clone)]
struct LoadedKnownNewsRecord {
    event_timestamp_utc_nanos: i64,
    event_identifier: Option<String>,
    source_row_number: usize,
}

#[derive(Debug, Clone)]
struct TickLoadOutput {
    tick_records: Vec<MarketDataTranslationTickRecord>,
    directional_label_diagnostics: Option<CalibrationDirectionalLabelDiagnosticsPayload>,
    summary: MarketDataSourceSummary,
    reference_price: Option<f64>,
    fit_warning: Option<String>,
}

#[derive(Debug, Clone)]
struct KnownNewsLoadOutput {
    known_news_records: Vec<MarketDataTranslationKnownNewsRecord>,
    summary: MarketDataSourceSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationDirectionalLabelDiagnosticsPayload {
    pub policy_id: String,
    pub first_inferred_trade_sign: String,
    pub provided_label_count: usize,
    pub inferred_label_count: usize,
    pub flat_move_carry_count: usize,
    pub price_up_inference_count: usize,
    pub price_down_inference_count: usize,
}

#[derive(Debug, Clone)]
enum RawValue {
    Null,
    Text(String),
    Integer(i64),
    Float(f64),
    Boolean,
}

type RawRow = BTreeMap<String, RawValue>;

pub fn load_and_preprocess_market_data_translation_request(
    request: MarketDataLoadingRequest,
    calendar_registry: &ExchangeCalendarRegistry,
) -> Result<MarketDataLoadingOutput, MarketDataLoadingError> {
    let tick_output = load_tick_records(
        &request.tick_data_source,
        &request.tick_data_column_mapping,
        &request.normalization_policy,
        request.require_hawkes,
    )?;

    let known_news_output = if let Some(known_news_input) = &request.known_news_input {
        Some(load_known_news_records(known_news_input)?)
    } else {
        None
    };

    let preprocessing_request = MarketDataTranslationPreprocessingRequest {
        exchange_code: request.exchange_code,
        trade_events_data_reference: request.trade_events_data_reference,
        tick_records: tick_output.tick_records,
        known_news_records: known_news_output
            .as_ref()
            .map(|output| output.known_news_records.clone())
            .unwrap_or_default(),
        option_surface_quote_values: Vec::new(),
        directional_label_diagnostics: tick_output
            .directional_label_diagnostics
            .map(|diagnostics| CalibrationHawkesDirectionalLabelDiagnostics {
                policy_id: diagnostics.policy_id,
                first_inferred_trade_sign: diagnostics.first_inferred_trade_sign,
                provided_label_count: diagnostics.provided_label_count,
                inferred_label_count: diagnostics.inferred_label_count,
                flat_move_carry_count: diagnostics.flat_move_carry_count,
                price_up_inference_count: diagnostics.price_up_inference_count,
                price_down_inference_count: diagnostics.price_down_inference_count,
            }),
        require_hawkes: request.require_hawkes,
        require_return_components: request.require_return_components,
    };
    let preprocessing_output =
        preprocess_market_data_translation_request(preprocessing_request, calendar_registry)
            .map_err(MarketDataLoadingError::Preprocessing)?;

    Ok(MarketDataLoadingOutput {
        preprocessing_output,
        tick_data_summary: tick_output.summary,
        known_news_summary: known_news_output.map(|output| output.summary),
        reference_price: tick_output.reference_price,
        fit_warning: tick_output.fit_warning,
    })
}

fn load_tick_records(
    file_reference: &MarketDataInputFileReference,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    require_hawkes: bool,
) -> Result<TickLoadOutput, MarketDataLoadingError> {
    let required_columns = required_tick_source_columns(column_mapping, normalization_policy);
    let available_columns = load_source_columns(file_reference, "tick_data_source")?;

    validate_columns_exist(
        &available_columns,
        &required_columns,
        "column_mapping.tick_data",
        &file_reference.path,
    )?;

    let raw_rows = load_rows(file_reference, "tick_data_source")?;
    let source_row_count = raw_rows.len();
    let mut normalized_rows = Vec::with_capacity(source_row_count);
    for (row_index, raw_row) in raw_rows.iter().enumerate() {
        normalized_rows.push(normalize_tick_row(
            raw_row,
            row_index + 1,
            column_mapping,
            normalization_policy,
            &file_reference.path,
        )?);
    }

    let (mut deduplicated_rows, dropped_duplicate_row_count) =
        deduplicate_tick_rows(normalized_rows, normalization_policy.duplicate_record_policy);
    if normalization_policy.sort_records_by_timestamp {
        deduplicated_rows.sort_by_key(|row| (row.timestamp_utc_nanos, row.source_row_number));
    }

    let fit_warning = detect_interval_data_warning(&deduplicated_rows);
    let reference_price = deduplicated_rows.last().and_then(|row| row.price);
    let (resolved_rows, directional_label_diagnostics) =
        resolve_trade_signs(deduplicated_rows, require_hawkes, &file_reference.path)?;

    let tick_records = resolved_rows
        .into_iter()
        .map(|row| MarketDataTranslationTickRecord {
            timestamp_utc_nanos: row.timestamp_utc_nanos,
            price: row.price,
            trade_sign: row.trade_sign.map(trade_sign_string),
            source_row_number: row.source_row_number,
        })
        .collect::<Vec<_>>();

    Ok(TickLoadOutput {
        tick_records,
        directional_label_diagnostics,
        summary: MarketDataSourceSummary {
            path: file_reference.path.clone(),
            file_format: file_format_string(file_reference.file_format).to_string(),
            provenance_label: file_reference.provenance_label.clone(),
            source_row_count,
            retained_row_count: source_row_count.saturating_sub(dropped_duplicate_row_count),
            dropped_duplicate_row_count,
            sort_records_by_timestamp: normalization_policy.sort_records_by_timestamp,
            applied_timestamp_timezone: Some(normalization_policy.timestamp_timezone.clone()),
            canonical_value_field: normalization_policy
                .tick_price_input_policy
                .map(|_| "price".to_string()),
            value_mapping_policy: normalization_policy
                .tick_price_input_policy
                .map(tick_price_policy_string),
        },
        reference_price,
        fit_warning,
    })
}

fn load_known_news_records(
    known_news_input: &MarketDataKnownNewsLoadingInput,
) -> Result<KnownNewsLoadOutput, MarketDataLoadingError> {
    let required_columns = required_known_news_source_columns(&known_news_input.column_mapping);
    let available_columns = load_source_columns(
        &known_news_input.calendar_source,
        "known_news_inputs.calendar_source",
    )?;

    validate_columns_exist(
        &available_columns,
        &required_columns,
        "column_mapping.known_news",
        &known_news_input.calendar_source.path,
    )?;

    let raw_rows = load_rows(
        &known_news_input.calendar_source,
        "known_news_inputs.calendar_source",
    )?;
    let source_row_count = raw_rows.len();
    let mut normalized_rows = Vec::with_capacity(source_row_count);
    let mut seen_keys: HashSet<(i64, Option<String>)> = HashSet::new();
    for (row_index, raw_row) in raw_rows.iter().enumerate() {
        let normalized = normalize_known_news_row(
            raw_row,
            row_index + 1,
            known_news_input,
            &known_news_input.calendar_source.path,
        )?;

        let key = (
            normalized.event_timestamp_utc_nanos,
            normalized.event_identifier.clone(),
        );

        if !seen_keys.insert(key) {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: "known_news_inputs.calendar_source".to_string(),
                message: "contains duplicate normalized rows".to_string(),
            });
        }

        normalized_rows.push(normalized);
    }

    normalized_rows.sort_by_key(|row| (row.event_timestamp_utc_nanos, row.source_row_number));

    Ok(KnownNewsLoadOutput {
        known_news_records: normalized_rows
            .iter()
            .map(|row| MarketDataTranslationKnownNewsRecord {
                event_timestamp_utc_nanos: row.event_timestamp_utc_nanos,
                event_identifier: row.event_identifier.clone(),
            })
            .collect(),
        summary: MarketDataSourceSummary {
            path: known_news_input.calendar_source.path.clone(),
            file_format: file_format_string(known_news_input.calendar_source.file_format)
                .to_string(),
            provenance_label: known_news_input.calendar_source.provenance_label.clone(),
            source_row_count,
            retained_row_count: normalized_rows.len(),
            dropped_duplicate_row_count: 0,
            sort_records_by_timestamp: true,
            applied_timestamp_timezone: known_news_input.calendar_timezone.clone(),
            canonical_value_field: None,
            value_mapping_policy: None,
        },
    })
}

fn load_source_columns(
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
            let file = File::open(&file_reference.path)
                .map_err(|error| MarketDataLoadingError::Io {
                    field_path: format!("{field_path}.path"),
                    message: error.to_string(),
                })?;
            let reader = SerializedFileReader::new(file)
                .map_err(|error| MarketDataLoadingError::Parquet {
                    field_path: format!("{field_path}.path"),
                    message: error.to_string(),
                })?;

            let schema = reader.metadata().file_metadata().schema_descr();

            Ok(schema
                .columns()
                .iter()
                .map(|column| column.name().to_string())
                .collect())
        }
    }
}

fn load_rows(
    file_reference: &MarketDataInputFileReference,
    field_path: &str,
) -> Result<Vec<RawRow>, MarketDataLoadingError> {
    match file_reference.file_format {
        MarketDataInputFileFormat::Csv => load_csv_rows(file_reference, field_path),
        MarketDataInputFileFormat::Parquet => load_parquet_rows(file_reference, field_path),
    }
}

fn load_csv_rows(
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

fn raw_row_from_csv_record(headers: &StringRecord, record: &StringRecord) -> RawRow {
    let mut row = BTreeMap::new();
    for (header, value) in headers.iter().zip(record.iter()) {
        row.insert(header.to_string(), RawValue::Text(value.to_string()));
    }

    row
}

fn load_parquet_rows(
    file_reference: &MarketDataInputFileReference,
    field_path: &str,
) -> Result<Vec<RawRow>, MarketDataLoadingError> {
    let file = File::open(&file_reference.path)
        .map_err(|error| MarketDataLoadingError::Io {
            field_path: format!("{field_path}.path"),
            message: error.to_string(),
        })?;
    let reader = SerializedFileReader::new(file)
        .map_err(|error| MarketDataLoadingError::Parquet {
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

fn raw_value_from_parquet_field(field: &Field) -> RawValue {
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

fn validate_columns_exist(
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

fn normalize_tick_row(
    raw_row: &RawRow,
    source_row_number: usize,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    source_path: &str,
) -> Result<LoadedTickRecord, MarketDataLoadingError> {
    let timestamp = normalize_timestamp_value(
        raw_row.get(&column_mapping.timestamp_column),
        &format!("tick_data_source.rows[{source_row_number}].{}", column_mapping.timestamp_column),
        Some(normalization_policy.timestamp_timezone.as_str()),
        source_path,
    )?;

    let explicit_price = match column_mapping.price_column.as_ref() {
        Some(column_name) => normalize_optional_numeric_value(
            raw_row.get(column_name),
            &format!("tick_data_source.rows[{source_row_number}].{column_name}"),
            source_path,
            true,
            false,
        )?,
        None => None,
    };
    let bid_price = match column_mapping.bid_column.as_ref() {
        Some(column_name) => normalize_optional_numeric_value(
            raw_row.get(column_name),
            &format!("tick_data_source.rows[{source_row_number}].{column_name}"),
            source_path,
            true,
            false,
        )?,
        None => None,
    };
    let ask_price = match column_mapping.ask_column.as_ref() {
        Some(column_name) => normalize_optional_numeric_value(
            raw_row.get(column_name),
            &format!("tick_data_source.rows[{source_row_number}].{column_name}"),
            source_path,
            true,
            false,
        )?,
        None => None,
    };
    let trade_sign = match column_mapping.trade_sign_column.as_ref() {
        Some(column_name) => normalize_optional_trade_sign_value(
            raw_row.get(column_name),
            &format!("tick_data_source.rows[{source_row_number}].{column_name}"),
            source_path,
        )?,
        None => None,
    };

    let price = match normalization_policy.tick_price_input_policy {
        Some(MarketDataTickPriceInputPolicy::ExplicitPriceColumn) => explicit_price,
        Some(MarketDataTickPriceInputPolicy::BidAskMidpoint) => match (bid_price, ask_price) {
            (Some(bid), Some(ask)) if ask >= bid => Some((bid + ask) / 2.0),
            _ => {
                return Err(MarketDataLoadingError::InvalidValue {
                    field_path: format!("tick_data_source.rows[{source_row_number}]"),
                    message: "bid_column and ask_column must both contain values to derive bid_ask_midpoint".to_string(),
                })
            }
        },
        None => None,
    };

    Ok(LoadedTickRecord {
        timestamp_utc_nanos: timestamp,
        price,
        trade_price: explicit_price,
        trade_sign,
        bid_price,
        ask_price,
        close_price: explicit_price,
        source_row_number,
    })
}

fn normalize_known_news_row(
    raw_row: &RawRow,
    source_row_number: usize,
    known_news_input: &MarketDataKnownNewsLoadingInput,
    source_path: &str,
) -> Result<LoadedKnownNewsRecord, MarketDataLoadingError> {
    let timestamp = normalize_timestamp_value(
        raw_row.get(&known_news_input.column_mapping.event_timestamp_column),
        &format!(
            "known_news_inputs.calendar_source.rows[{source_row_number}].{}",
            known_news_input.column_mapping.event_timestamp_column
        ),
        known_news_input.calendar_timezone.as_deref(),
        source_path,
    )?;
    let event_identifier = match known_news_input.column_mapping.event_identifier_column.as_ref() {
        Some(column_name) => normalize_optional_text_value(
            raw_row.get(column_name),
            &format!("known_news_inputs.calendar_source.rows[{source_row_number}].{column_name}"),
            source_path,
        )?,
        None => None,
    };

    Ok(LoadedKnownNewsRecord {
        event_timestamp_utc_nanos: timestamp,
        event_identifier,
        source_row_number,
    })
}

fn normalize_timestamp_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_timezone: Option<&str>,
    source_path: &str,
) -> Result<i64, MarketDataLoadingError> {
    let raw_value = raw_value.ok_or_else(|| MarketDataLoadingError::InvalidValue {
        field_path: field_path.to_string(),
        message: format!("is required and cannot be null. source_path={source_path}"),
    })?;

    let parsed = match raw_value {
        RawValue::Null => {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: field_path.to_string(),
                message: format!("is required and cannot be null. source_path={source_path}"),
            })
        }
        RawValue::Text(value) => parse_timestamp_text(value, field_path, source_timezone, source_path)?,
        RawValue::Integer(value) => parse_unix_epoch_value(*value as f64),
        RawValue::Float(value) => parse_unix_epoch_value(*value),
        RawValue::Boolean => {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: field_path.to_string(),
                message: format!("must be a supported timestamp string, Unix epoch value, or a parquet timestamp column. source_path={source_path}"),
            })
        }
    };

    Ok(parsed)
}

fn parse_timestamp_text(
    raw_value: &str,
    field_path: &str,
    source_timezone: Option<&str>,
    source_path: &str,
) -> Result<i64, MarketDataLoadingError> {
    let stripped = raw_value.trim();
    if stripped.is_empty() {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("is required and cannot be blank. source_path={source_path}"),
        });
    }

    if let Some(parsed_numeric) = try_parse_numeric_timestamp_text(stripped) {
        return Ok(parsed_numeric);
    }

    let normalized_value = stripped
        .replace(" UTC", "+00:00")
        .replace(" GMT", "+00:00");
    let rfc3339_candidate = if normalized_value.ends_with('Z') {
        format!("{}+00:00", &normalized_value[..normalized_value.len() - 1])
    } else {
        normalized_value.clone()
    };
    if let Ok(parsed) = DateTime::parse_from_rfc3339(&rfc3339_candidate) {
        return Ok(parsed.timestamp_nanos_opt().unwrap_or(i64::MAX));
    }

    for format in COMMON_TIMESTAMP_FORMATS {
        if let Ok(parsed) = DateTime::parse_from_str(&normalized_value, format) {
            return Ok(parsed.timestamp_nanos_opt().unwrap_or(i64::MAX));
        }
        if let Ok(parsed) = NaiveDateTime::parse_from_str(&normalized_value, format) {
            let timezone = source_timezone.ok_or_else(|| MarketDataLoadingError::InvalidValue {
                field_path: field_path.to_string(),
                message: format!("contains a naive timestamp. Provide a timezone-aware value or configure the source timezone explicitly. source_path={source_path}"),
            })?;
            return localize_naive_datetime(parsed, timezone, field_path, source_path);
        }
    }

    Err(MarketDataLoadingError::InvalidValue {
        field_path: field_path.to_string(),
        message: format!("must be a supported timestamp string, Unix epoch value, or a parquet timestamp column. source_path={source_path}"),
    })
}

fn localize_naive_datetime(
    parsed_value: NaiveDateTime,
    source_timezone: &str,
    field_path: &str,
    source_path: &str,
) -> Result<i64, MarketDataLoadingError> {
    let timezone = resolve_source_timezone(source_timezone, field_path, source_path)?;
    match timezone.from_local_datetime(&parsed_value) {
        LocalResult::Single(value) => value.timestamp_nanos_opt().ok_or_else(|| MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("timestamp value is outside the supported range. source_path={source_path}"),
        }),
        LocalResult::Ambiguous(_, _) => Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("is ambiguous in the configured source timezone due to a daylight-saving transition. Provide explicit UTC offsets in the file. source_path={source_path}"),
        }),
        LocalResult::None => Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("falls into a nonexistent local-time gap. Provide explicit UTC offsets or normalize the source file. source_path={source_path}"),
        }),
    }
}

fn resolve_source_timezone(
    source_timezone: &str,
    field_path: &str,
    source_path: &str,
) -> Result<Tz, MarketDataLoadingError> {
    let normalized = source_timezone.trim();
    if matches!(normalized.to_ascii_uppercase().as_str(), "UTC" | "Z" | "GMT" | "UT") {
        return Ok(chrono_tz::UTC);
    }

    normalized.parse::<Tz>().map_err(|_| MarketDataLoadingError::InvalidValue {
        field_path: field_path.to_string(),
        message: format!("references unknown timezone '{source_timezone}'. source_path={source_path}"),
    })
}

fn try_parse_numeric_timestamp_text(raw_value: &str) -> Option<i64> {
    let unsigned = raw_value.trim_start_matches(['+', '-']);
    if unsigned.chars().all(|character| character.is_ascii_digit()) {
        let digit_count = unsigned.len();
        if !matches!(digit_count, 9 | 10 | 12 | 13 | 15 | 16 | 18 | 19) {
            return None;
        }
    }

    if let Ok(parsed_integer) = raw_value.parse::<i64>() {
        return Some(parse_unix_epoch_value(parsed_integer as f64));
    }
    if let Ok(parsed_float) = raw_value.parse::<f64>() {
        return Some(parse_unix_epoch_value(parsed_float));
    }

    None
}

fn parse_unix_epoch_value(raw_value: f64) -> i64 {
    let absolute = raw_value.abs();
    if absolute >= 100_000_000_000_000_000.0 {
        return ((raw_value / 1_000_000_000.0) * 1_000_000_000.0).round() as i64;
    }
    if absolute >= 100_000_000_000_000.0 {
        return ((raw_value / 1_000_000.0) * 1_000_000_000.0).round() as i64;
    }
    if absolute >= 100_000_000_000.0 {
        return ((raw_value / 1_000.0) * 1_000_000_000.0).round() as i64;
    }

    (raw_value * 1_000_000_000.0).round() as i64
}

fn normalize_optional_numeric_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
    allow_zero: bool,
    allow_negative: bool,
) -> Result<Option<f64>, MarketDataLoadingError> {
    let Some(raw_value) = raw_value else {
        return Ok(None);
    };

    let numeric_value = match raw_value {
        RawValue::Null => return Ok(None),
        RawValue::Text(value) => {
            let stripped = value.trim();
            if stripped.is_empty() {
                return Ok(None);
            }
            stripped.parse::<f64>().map_err(|_| MarketDataLoadingError::InvalidValue {
                field_path: field_path.to_string(),
                message: format!("must be a finite numeric value when provided. source_path={source_path}"),
            })?
        }
        RawValue::Integer(value) => *value as f64,
        RawValue::Float(value) => *value,
        RawValue::Boolean => {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: field_path.to_string(),
                message: format!("must be a finite numeric value when provided. source_path={source_path}"),
            })
        }
    };

    if !numeric_value.is_finite() {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("must be a finite numeric value when provided. source_path={source_path}"),
        });
    }
    if !allow_negative && numeric_value < 0.0 {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("must be non-negative. source_path={source_path}"),
        });
    }
    if !allow_zero && numeric_value <= 0.0 {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("must be strictly positive. source_path={source_path}"),
        });
    }

    Ok(Some(numeric_value))
}

fn normalize_optional_text_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
) -> Result<Option<String>, MarketDataLoadingError> {
    let Some(raw_value) = raw_value else {
        return Ok(None);
    };

    match raw_value {
        RawValue::Null => Ok(None),
        RawValue::Text(value) => {
            let stripped = value.trim();
            if stripped.is_empty() {
                Ok(None)
            } else {
                Ok(Some(stripped.to_string()))
            }
        }
        _ => Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("must be a string when provided. source_path={source_path}"),
        }),
    }
}

fn normalize_optional_trade_sign_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
) -> Result<Option<TradeSign>, MarketDataLoadingError> {
    let Some(text) = normalize_optional_text_value(raw_value, field_path, source_path)? else {
        return Ok(None);
    };

    match text.to_ascii_lowercase().as_str() {
        "buy" => Ok(Some(TradeSign::Buy)),
        "sell" => Ok(Some(TradeSign::Sell)),
        _ => Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("must be either 'buy' or 'sell' when provided. source_path={source_path}"),
        }),
    }
}

fn deduplicate_tick_rows(
    rows: Vec<LoadedTickRecord>,
    duplicate_record_policy: Option<MarketDataDuplicateRecordPolicy>,
) -> (Vec<LoadedTickRecord>, usize) {
    let Some(policy) = duplicate_record_policy else {
        return (rows, 0);
    };

    match policy {
        MarketDataDuplicateRecordPolicy::KeepFirst => {
            let source_count = rows.len();
            let mut seen = HashSet::new();
            let mut retained = Vec::new();

            for row in rows {
                let key = tick_record_dedup_key(&row);
                if seen.insert(key) {
                    retained.push(row);
                }
            }

            let dropped_duplicate_row_count = source_count.saturating_sub(retained.len());

            (retained, dropped_duplicate_row_count)
        }
        MarketDataDuplicateRecordPolicy::KeepLast => {
            let source_count = rows.len();
            let mut last_index_by_key = BTreeMap::new();
            for (index, row) in rows.iter().enumerate() {
                last_index_by_key.insert(tick_record_dedup_key(row), index);
            }

            let retained = rows
                .into_iter()
                .enumerate()
                .filter_map(|(index, row)| {
                    let key = tick_record_dedup_key(&row);
                    (last_index_by_key.get(&key).copied() == Some(index)).then_some(row)
                })
                .collect::<Vec<_>>();

            let dropped_duplicate_row_count = source_count.saturating_sub(retained.len());

            (retained, dropped_duplicate_row_count)
        }
    }
}

fn tick_record_dedup_key(
    row: &LoadedTickRecord,
) -> (
    i64,
    Option<u64>,
    Option<u64>,
    Option<TradeSign>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
) {
    (
        row.timestamp_utc_nanos,
        normalized_float_key(row.price),
        normalized_float_key(row.trade_price),
        row.trade_sign,
        normalized_float_key(row.bid_price),
        normalized_float_key(row.ask_price),
        normalized_float_key(row.close_price),
    )
}

fn normalized_float_key(value: Option<f64>) -> Option<u64> {
    value.map(f64::to_bits)
}

fn resolve_trade_signs(
    rows: Vec<LoadedTickRecord>,
    require_hawkes: bool,
    source_path: &str,
) -> Result<
    (
        Vec<LoadedTickRecord>,
        Option<CalibrationDirectionalLabelDiagnosticsPayload>,
    ),
    MarketDataLoadingError,
> {
    if !require_hawkes {
        return Ok((rows, None));
    }
    if rows.is_empty() {
        return Ok((
            rows,
            Some(CalibrationDirectionalLabelDiagnosticsPayload {
                policy_id: "tick_price_change_with_flat_move_carryover.v1".to_string(),
                first_inferred_trade_sign: "buy".to_string(),
                provided_label_count: 0,
                inferred_label_count: 0,
                flat_move_carry_count: 0,
                price_up_inference_count: 0,
                price_down_inference_count: 0,
            }),
        ));
    }

    let mut previous_price: Option<f64> = None;
    let mut previous_sign = TradeSign::Buy;
    let mut provided_label_count = 0;
    let mut inferred_label_count = 0;
    let mut flat_move_carry_count = 0;
    let mut price_up_inference_count = 0;
    let mut price_down_inference_count = 0;

    let mut resolved_rows = Vec::with_capacity(rows.len());
    for mut row in rows {
        let resolved_sign = match row.trade_sign {
            Some(sign) => {
                provided_label_count += 1;
                previous_sign = sign;
                Some(sign)
            }
            None => {
                inferred_label_count += 1;

                let price = row.price.ok_or_else(|| MarketDataLoadingError::InvalidValue {
                    field_path: "column_mapping.tick_data.trade_sign_column".to_string(),
                    message: format!("trade sign inference requires a canonical tick price for every retained trade event. source_path={source_path}"),
                })?;

                let inferred_sign = match previous_price {
                    None => TradeSign::Buy,
                    Some(previous) if price > previous => {
                        price_up_inference_count += 1;
                        TradeSign::Buy
                    }
                    Some(previous) if price < previous => {
                        price_down_inference_count += 1;
                        TradeSign::Sell
                    }
                    Some(_) => {
                        flat_move_carry_count += 1;
                        previous_sign
                    }
                };

                previous_sign = inferred_sign;
                Some(inferred_sign)
            }
        };

        previous_price = row.price.or(previous_price);

        row.trade_sign = resolved_sign;
        resolved_rows.push(row);
    }

    Ok((
        resolved_rows,
        Some(CalibrationDirectionalLabelDiagnosticsPayload {
            policy_id: "tick_price_change_with_flat_move_carryover.v1".to_string(),
            first_inferred_trade_sign: "buy".to_string(),
            provided_label_count,
            inferred_label_count,
            flat_move_carry_count,
            price_up_inference_count,
            price_down_inference_count,
        }),
    ))
}

fn detect_interval_data_warning(rows: &[LoadedTickRecord]) -> Option<String> {
    if rows.len() < 4 {
        return None;
    }

    let mut deltas_in_seconds = Vec::new();
    for pair in rows.windows(2) {
        let delta_seconds = (pair[1].timestamp_utc_nanos - pair[0].timestamp_utc_nanos) / 1_000_000_000;
        if delta_seconds > 0 {
            deltas_in_seconds.push(delta_seconds);
        }
    }
    if deltas_in_seconds.len() < 3 {
        return None;
    }

    let aligned_to_minute_count = deltas_in_seconds.iter().filter(|delta| **delta % 60 == 0).count();
    let mut counts = BTreeMap::new();
    for delta in &deltas_in_seconds {
        *counts.entry(*delta).or_insert(0_usize) += 1;
    }

    let (dominant_delta_seconds, dominant_count) =
        counts.into_iter().max_by_key(|(_, count)| *count)?;
    let dominant_share = dominant_count as f64 / deltas_in_seconds.len() as f64;

    if dominant_delta_seconds < 60 {
        return None;
    }
    if aligned_to_minute_count as f64 / (deltas_in_seconds.len() as f64) < 0.8 {
        return None;
    }
    if dominant_share < 0.6 {
        return None;
    }

    Some(format!(
        "The supplied fit input appears to be interval data rather than tick data. Janiform detected a dominant spacing of {}, so fitting the model on this data will likely produce an invalid fit.",
        format_interval_warning_delta(dominant_delta_seconds)
    ))
}

fn format_interval_warning_delta(delta_seconds: i64) -> String {
    if delta_seconds % 86_400 == 0 {
        return format!("{}D", delta_seconds / 86_400);
    }
    if delta_seconds % 3_600 == 0 {
        return format!("{}h", delta_seconds / 3_600);
    }
    if delta_seconds % 60 == 0 {
        return format!("{}min", delta_seconds / 60);
    }

    format!("{delta_seconds}s")
}

fn required_tick_source_columns(
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Vec<String> {
    let mut columns = vec![column_mapping.timestamp_column.clone()];
    if let Some(column) = &column_mapping.price_column {
        columns.push(column.clone());
    }
    if let Some(column) = &column_mapping.trade_sign_column {
        columns.push(column.clone());
    }

    if matches!(
        normalization_policy.tick_price_input_policy,
        Some(MarketDataTickPriceInputPolicy::BidAskMidpoint)
    ) {
        if let Some(column) = &column_mapping.bid_column {
            columns.push(column.clone());
        }
        if let Some(column) = &column_mapping.ask_column {
            columns.push(column.clone());
        }
    }

    dedup_vec(columns)
}

fn required_known_news_source_columns(column_mapping: &MarketDataKnownNewsColumnMapping) -> Vec<String> {
    let mut columns = vec![column_mapping.event_timestamp_column.clone()];
    if let Some(column) = &column_mapping.event_identifier_column {
        columns.push(column.clone());
    }

    dedup_vec(columns)
}

fn dedup_vec<T>(values: Vec<T>) -> Vec<T>
where
    T: Eq + Hash + Clone,
{
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }

    deduped
}

fn trade_sign_string(trade_sign: TradeSign) -> String {
    match trade_sign {
        TradeSign::Buy => "buy".to_string(),
        TradeSign::Sell => "sell".to_string(),
    }
}

fn tick_price_policy_string(policy: MarketDataTickPriceInputPolicy) -> String {
    match policy {
        MarketDataTickPriceInputPolicy::ExplicitPriceColumn => "explicit_price_column".to_string(),
        MarketDataTickPriceInputPolicy::BidAskMidpoint => "bid_ask_midpoint".to_string(),
    }
}

fn file_format_string(format: MarketDataInputFileFormat) -> &'static str {
    match format {
        MarketDataInputFileFormat::Csv => "csv",
        MarketDataInputFileFormat::Parquet => "parquet",
    }
}
