use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::market_data::preprocessing::{
    MarketDataTranslationCalibrationError, MarketDataTranslationCalibrationOutput,
    MarketDataTranslationCalibrationSettings,
};

use super::progress::MarketDataLoadingProgressError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataLoadingRequest {
    pub session_configuration: MarketDataSessionConfiguration,
    pub trade_events_data_reference: String,
    pub tick_data_source: MarketDataInputFileReference,
    pub tick_data_column_mapping: MarketDataTickDataColumnMapping,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coarse_data_input: Option<MarketDataCoarseDataLoadingInput>,
    pub normalization_policy: MarketDataNormalizationPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration_settings: Option<MarketDataTranslationCalibrationSettings>,
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
pub struct MarketDataCoarseDataLoadingInput {
    pub source: MarketDataInputFileReference,
    pub column_mapping: MarketDataCoarseDataColumnMapping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataCoarseDataColumnMapping {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_column: Option<String>,
    #[serde(default)]
    pub timestamp_semantics: MarketDataCoarseDataTimestampSemantics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_start_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_end_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_column: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MarketDataCoarseDataTimestampSemantics {
    #[default]
    BarOpenTime,
    BarCloseTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketDataCoarseDataKind {
    Ohlcv,
    CloseOnly,
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

/// Policy for resolving price coordinates when option strikes live in a different source
/// coordinate than the fitted continuous return series.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceCoordinatePolicy {
    #[default]
    Auto,
    Single,
    ContinuousFitRawOptionAnchor,
}

/// Default relative put-call parity warning threshold for option coordinate validation.
pub const DEFAULT_OPTION_FORWARD_PARITY_WARNING_RELATIVE_THRESHOLD: f64 = 0.0025;

/// Policy for warning when the resolved model forward disagrees with option parity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionForwardParityWarningPolicy {
    /// Relative model-vs-observed parity delta that triggers a warning.
    pub relative_threshold: f64,
}

impl Default for OptionForwardParityWarningPolicy {
    fn default() -> Self {
        Self {
            relative_threshold: DEFAULT_OPTION_FORWARD_PARITY_WARNING_RELATIVE_THRESHOLD,
        }
    }
}

impl OptionForwardParityWarningPolicy {
    pub fn validate(&self) -> Result<(), MarketDataLoadingError> {
        if self.relative_threshold.is_finite() && self.relative_threshold > 0.0 {
            return Ok(());
        }

        Err(MarketDataLoadingError::InvalidValue {
            field_path: "option_forward_parity_warning_policy.relative_threshold".to_string(),
            message: "must be finite and strictly positive".to_string(),
        })
    }
}

/// Resolved tick coordinate for one workflow role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPriceCoordinate {
    pub coordinate_role: String,
    pub coordinate_kind: String,
    pub tick_data_column_mapping: MarketDataTickDataColumnMapping,
    pub tick_price_input_policy: Option<MarketDataTickPriceInputPolicy>,
}

/// Evidence that an adjusted price column is an additive transform of a raw price column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedRollAdjustmentEvidence {
    pub adjusted_price_column: String,
    pub raw_price_column: String,
    pub adjustment_column: String,
    pub checked_row_count: usize,
    pub max_absolute_relation_error: f64,
}

/// Put-call parity comparison for the resolved option valuation coordinate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionForwardParityCheck {
    pub model_forward_price: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_forward_price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_delta: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_delta: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bid_ask_forward_lower: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_bid_ask_forward_upper: Option<f64>,
    pub relative_warning_threshold: f64,
    pub paired_strike_count: usize,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Resolved price coordinates for dynamics fitting and option valuation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPriceCoordinateSet {
    pub policy: PriceCoordinatePolicy,
    pub dynamics_fit: ResolvedPriceCoordinate,
    pub option_valuation: ResolvedPriceCoordinate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_underlier_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_underlier_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll_adjustment_evidence: Option<ResolvedRollAdjustmentEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parity_forward_check: Option<OptionForwardParityCheck>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataLoadingOutput {
    pub session_resolution: MarketDataSessionResolutionPayload,
    pub calibration_output: MarketDataTranslationCalibrationOutput,
    pub tick_data_summary: MarketDataSourceSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coarse_data_summary: Option<MarketDataCoarseDataSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coarse_data_records: Option<Vec<MarketDataNormalizedCoarseDataRecord>>,
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
pub struct MarketDataCoarseDataSummary {
    pub source_summary: MarketDataSourceSummary,
    pub inferred_kind: MarketDataCoarseDataKind,
    pub resolved_column_mapping: MarketDataCoarseDataColumnMapping,
    pub timestamp_semantics: String,
    pub interval_inference_policy: String,
    pub nominal_interval_nanos: i64,
    pub retained_return_interval_count: usize,
    pub retained_mean_reversion_observation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataNormalizedCoarseDataRecord {
    pub interval_start_utc_nanos: i64,
    pub interval_end_utc_nanos: i64,
    pub observation_timestamp_utc_nanos: i64,
    pub close_price: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low_price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_to_close_log_return: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_to_close_log_return: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_to_open_gap_log_return: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_low_log_range: Option<f64>,
    pub source_row_number: usize,
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
    Calibration(MarketDataTranslationCalibrationError),
    Progress(MarketDataLoadingProgressError),
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
            Self::Calibration(error) => Display::fmt(error, formatter),
            Self::Progress(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for MarketDataLoadingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Calibration(error) => Some(error),
            Self::Progress(error) => Some(error),
            Self::Io { .. }
            | Self::Csv { .. }
            | Self::Parquet { .. }
            | Self::MissingColumn { .. }
            | Self::InvalidValue { .. } => None,
        }
    }
}
