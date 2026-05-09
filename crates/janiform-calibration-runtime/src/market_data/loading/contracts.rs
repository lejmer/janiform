use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::market_data::preprocessing::{
    MarketDataTranslationPreprocessingError, MarketDataTranslationPreprocessingOutput,
    MarketDataTranslationPreprocessingSettings,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataLoadingRequest {
    pub session_configuration: MarketDataSessionConfiguration,
    pub trade_events_data_reference: String,
    pub tick_data_source: MarketDataInputFileReference,
    pub tick_data_column_mapping: MarketDataTickDataColumnMapping,
    pub normalization_policy: MarketDataNormalizationPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preprocessing_settings: Option<MarketDataTranslationPreprocessingSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_surface_input: Option<MarketDataOptionSurfaceLoadingInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_news_input: Option<MarketDataKnownNewsLoadingInput>,
    pub require_hawkes: bool,
    pub require_return_components: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataSessionConfiguration {
    pub exchange_code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_definition_source: Option<MarketDataInputFileReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_timezone: Option<String>,
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
    Json,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataTickDataColumnMapping {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_column: Option<String>,
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
pub struct MarketDataOptionSurfaceLoadingInput {
    pub source: MarketDataInputFileReference,
    pub column_mapping: MarketDataOptionSurfaceColumnMapping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataOptionSurfaceColumnMapping {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_timestamp_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_timestamp_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_type_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid_price_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bid_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implied_volatility_column: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_quote_input_policy: Option<MarketDataOptionQuoteInputPolicy>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketDataOptionQuoteInputPolicy {
    MidPriceColumn,
    BidAskMidpoint,
    ImpliedVolatilityColumn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataLoadingOutput {
    pub session_resolution: MarketDataSessionResolutionPayload,
    pub preprocessing_output: MarketDataTranslationPreprocessingOutput,
    pub tick_data_summary: MarketDataSourceSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_surface_summary: Option<MarketDataSourceSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_surface_records: Option<Vec<MarketDataNormalizedOptionSurfaceRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_news_summary: Option<MarketDataSourceSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataSourceProbePayload {
    pub path: String,
    pub file_format: String,
    pub provenance_label: String,
    pub available_columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_row_count_estimate: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataNormalizedTickOutput {
    pub tick_records: Vec<crate::market_data::preprocessing::MarketDataTranslationTickRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directional_label_diagnostics:
        Option<crate::market_data::loading::CalibrationDirectionalLabelDiagnosticsPayload>,
    pub tick_data_summary: MarketDataSourceSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataNormalizedOptionSurfaceOutput {
    pub option_surface_records: Vec<MarketDataNormalizedOptionSurfaceRecord>,
    pub option_surface_summary: MarketDataSourceSummary,
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
    pub dropped_invalid_row_count: usize,
    pub sort_records_by_timestamp: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_timestamp_timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_value_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_mapping_policy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataNormalizedOptionSurfaceRecord {
    pub quote_timestamp_utc_nanos: i64,
    pub expiry_timestamp_utc_nanos: i64,
    pub strike: f64,
    pub option_type: String,
    pub quote_value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid_price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bid_price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implied_volatility: Option<f64>,
    pub source_row_number: usize,
}

impl MarketDataNormalizedOptionSurfaceRecord {
    /// Return the explicit mid price, or the bid/ask midpoint when both sides are available.
    pub fn observed_mid_price(&self) -> Option<f64> {
        self.mid_price.or(match (self.bid_price, self.ask_price) {
            (Some(bid_price), Some(ask_price)) => Some(0.5 * (bid_price + ask_price)),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataSessionResolutionPayload {
    pub schema_version: String,
    pub resolution_kind: String,
    pub provenance_kind: String,
    pub selected_policy_id: String,
    pub manual_override_applied: bool,
    pub exchange_code: String,
    pub exchange_name: String,
    pub resolved_timezone_name: String,
    pub timezone_source: String,
    pub open_session_duration_hours_per_week: f64,
    pub confidence_score: f64,
    pub confidence_label: String,
    pub regular_session_windows: Vec<MarketDataSessionRegularWindowPayload>,
    pub recurring_closed_windows: Vec<MarketDataSessionRecurringClosedWindowPayload>,
    pub reasons: Vec<String>,
    pub ambiguity_warnings: Vec<String>,
    pub metadata_discrepancies: Vec<String>,
    pub resolved_session_definition_json: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataSessionRegularWindowPayload {
    pub weekday: String,
    pub label: String,
    pub open_time: String,
    pub close_time: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataSessionRecurringClosedWindowPayload {
    pub start_weekday: String,
    pub start_time: String,
    pub end_weekday: String,
    pub end_time: String,
    pub reason: String,
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
