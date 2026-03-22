use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::hash::Hash;
use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use csv::StringRecord;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use serde::{Deserialize, Serialize};

use crate::calendar::ExchangeCalendarRegistry;
use crate::calendar::{
    CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarRegistryArtifact,
    ExchangeCalendarRegistryLoader, MarketState, RegularSessionArtifact, SessionWindowArtifact,
    TradingWeekday,
};
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

const SESSION_RESOLUTION_SCHEMA_VERSION: &str = "calibration.market_data.session_resolution.v1";
const LONG_GAP_THRESHOLD_HOURS: i64 = 4;
const GAP_SCAN_STEP_MINUTES: i64 = 30;
const MINIMUM_GENERIC_24_5_WEEKDAY_COUNT: usize = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataLoadingRequest {
    pub session_configuration: MarketDataSessionConfiguration,
    pub trade_events_data_reference: String,
    pub tick_data_source: MarketDataInputFileReference,
    pub tick_data_column_mapping: MarketDataTickDataColumnMapping,
    pub normalization_policy: MarketDataNormalizationPolicy,
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
pub struct MarketDataOptionSurfaceLoadingInput {
    pub source: MarketDataInputFileReference,
    pub column_mapping: MarketDataOptionSurfaceColumnMapping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataOptionSurfaceColumnMapping {
    pub quote_timestamp_column: String,
    pub expiry_timestamp_column: String,
    pub strike_column: String,
    pub option_type_column: String,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataSessionResolutionPayload {
    pub schema_version: String,
    pub resolution_kind: String,
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
struct LoadedOptionSurfaceRecord {
    quote_timestamp_utc_nanos: i64,
    expiry_timestamp_utc_nanos: i64,
    strike: f64,
    option_type: String,
    quote_value: f64,
    mid_price: Option<f64>,
    bid_price: Option<f64>,
    ask_price: Option<f64>,
    implied_volatility: Option<f64>,
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

#[derive(Debug, Clone)]
struct OptionSurfaceLoadOutput {
    option_surface_quote_values: Vec<f64>,
    summary: MarketDataSourceSummary,
}

#[derive(Debug, Clone)]
struct ResolvedMarketDataSession {
    calendar_registry: ExchangeCalendarRegistry,
    session_resolution: MarketDataSessionResolutionPayload,
}

#[derive(Debug, Clone)]
struct SessionTemplate {
    policy_id: String,
    exchange_name: String,
    timezone_name: String,
    timezone_source: String,
    holiday_calendar_code: String,
    early_close_rule_set: Option<String>,
    regular_session_windows: Vec<MarketDataSessionRegularWindowPayload>,
    continuous_weekday_template: bool,
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
) -> Result<MarketDataLoadingOutput, MarketDataLoadingError> {
    let tick_output = load_tick_records(
        &request.tick_data_source,
        &request.tick_data_column_mapping,
        &request.normalization_policy,
        request.require_hawkes,
    )?;
    let resolved_session = resolve_market_data_session_configuration_from_tick_records(
        &request.session_configuration,
        tick_output.tick_records.as_slice(),
    )?;

    let known_news_output = if let Some(known_news_input) = &request.known_news_input {
        Some(load_known_news_records(known_news_input)?)
    } else {
        None
    };

    let option_surface_output = if let Some(option_surface_input) = &request.option_surface_input {
        Some(load_option_surface_records(
            option_surface_input,
            &request.normalization_policy,
        )?)
    } else {
        None
    };

    let preprocessing_request = MarketDataTranslationPreprocessingRequest {
        exchange_code: request.session_configuration.exchange_code,
        trade_events_data_reference: request.trade_events_data_reference,
        tick_records: tick_output.tick_records,
        known_news_records: known_news_output
            .as_ref()
            .map(|output| output.known_news_records.clone())
            .unwrap_or_default(),
        option_surface_quote_values: option_surface_output
            .as_ref()
            .map(|output| output.option_surface_quote_values.clone())
            .unwrap_or_default(),
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
        preprocess_market_data_translation_request(
            preprocessing_request,
            &resolved_session.calendar_registry,
        )
        .map_err(MarketDataLoadingError::Preprocessing)?;

    Ok(MarketDataLoadingOutput {
        session_resolution: resolved_session.session_resolution,
        preprocessing_output,
        tick_data_summary: tick_output.summary,
        option_surface_summary: option_surface_output.map(|output| output.summary),
        known_news_summary: known_news_output.map(|output| output.summary),
        reference_price: tick_output.reference_price,
        fit_warning: tick_output.fit_warning,
    })
}

pub fn resolve_market_data_session_configuration_from_source_file(
    session_configuration: MarketDataSessionConfiguration,
    tick_data_source: MarketDataInputFileReference,
    tick_data_column_mapping: MarketDataTickDataColumnMapping,
    normalization_policy: MarketDataNormalizationPolicy,
) -> Result<MarketDataSessionResolutionPayload, MarketDataLoadingError> {
    let tick_output = load_tick_records(
        &tick_data_source,
        &tick_data_column_mapping,
        &normalization_policy,
        false,
    )?;
    let resolved_session = resolve_market_data_session_configuration_from_tick_records(
        &session_configuration,
        tick_output.tick_records.as_slice(),
    )?;

    Ok(resolved_session.session_resolution)
}

fn resolve_market_data_session_configuration_from_tick_records(
    session_configuration: &MarketDataSessionConfiguration,
    tick_records: &[MarketDataTranslationTickRecord],
) -> Result<ResolvedMarketDataSession, MarketDataLoadingError> {
    if let Some(session_source) = &session_configuration.session_definition_source {
        return resolve_explicit_session_definition(
            session_configuration.exchange_code.as_str(),
            session_source,
            session_configuration.session_timezone.as_deref(),
        );
    }

    let inferred_template = infer_session_template(
        session_configuration.exchange_code.as_str(),
        session_configuration.session_timezone.as_deref(),
        tick_records,
    )?;
    let registry_json = build_calendar_registry_json(
        session_configuration.exchange_code.as_str(),
        &inferred_template,
    )?;
    let calendar_registry = ExchangeCalendarRegistryLoader::load_from_json(registry_json.as_str())
        .map_err(|error| MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: format!("does not contain a valid calendar registry: {error}"),
        })?;
    let gap_alignment_diagnostics = evaluate_gap_alignment(
        &calendar_registry,
        session_configuration.exchange_code.as_str(),
        tick_records,
    )?;

    Ok(ResolvedMarketDataSession {
        calendar_registry,
        session_resolution: MarketDataSessionResolutionPayload {
            schema_version: SESSION_RESOLUTION_SCHEMA_VERSION.to_string(),
            resolution_kind: "automatic_inference".to_string(),
            selected_policy_id: inferred_template.policy_id.clone(),
            manual_override_applied: false,
            exchange_code: session_configuration.exchange_code.clone(),
            exchange_name: inferred_template.exchange_name.clone(),
            resolved_timezone_name: inferred_template.timezone_name.clone(),
            timezone_source: inferred_template.timezone_source.clone(),
            open_session_duration_hours_per_week: compute_open_session_duration_hours_per_week(
                &inferred_template,
            ),
            confidence_score: confidence_score_for_inference(
                &inferred_template,
                gap_alignment_diagnostics,
            ),
            confidence_label: confidence_label_for_gap_alignment(
                &inferred_template,
                gap_alignment_diagnostics,
            ),
            regular_session_windows: inferred_template.regular_session_windows.clone(),
            recurring_closed_windows: build_recurring_closed_windows(&inferred_template),
            reasons: automatic_resolution_reasons(
                session_configuration.exchange_code.as_str(),
                &inferred_template,
                gap_alignment_diagnostics,
            ),
            ambiguity_warnings: automatic_resolution_warnings(
                &inferred_template,
                gap_alignment_diagnostics,
            ),
            metadata_discrepancies: Vec::new(),
            resolved_session_definition_json: registry_json,
        },
    })
}

fn resolve_explicit_session_definition(
    exchange_code: &str,
    session_source: &MarketDataInputFileReference,
    configured_session_timezone: Option<&str>,
) -> Result<ResolvedMarketDataSession, MarketDataLoadingError> {
    if session_source.file_format != MarketDataInputFileFormat::Json {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration.session_definition_source.file_format".to_string(),
            message: "must be json when a session-definition source is provided".to_string(),
        });
    }

    let calendar_registry_json = std::fs::read_to_string(&session_source.path).map_err(|error| {
        let field_path = "session_configuration.session_definition_source.path".to_string();
        match error.kind() {
            std::io::ErrorKind::NotFound => MarketDataLoadingError::Io {
                field_path,
                message: "does not exist".to_string(),
            },
            _ => MarketDataLoadingError::Io {
                field_path,
                message: format!("could not be read: {error}"),
            },
        }
    })?;

    let calendar_registry =
        ExchangeCalendarRegistryLoader::load_from_json(calendar_registry_json.as_str()).map_err(
            |error| MarketDataLoadingError::InvalidValue {
                field_path: "session_configuration.session_definition_source.path".to_string(),
                message: format!("does not contain a valid calendar registry: {error}"),
            },
        )?;
    let definition = calendar_registry
        .get_required(exchange_code)
        .map_err(|_| MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration.exchange_code".to_string(),
            message: "must exist in the resolved calendar registry".to_string(),
        })?;
    let resolved_timezone_name = definition.timezone_name().to_string();
    let exchange_name = definition.exchange_metadata().exchange_name().to_string();
    let holiday_calendar_code = definition.rule_metadata().holiday_calendar_code().to_string();
    let early_close_rule_set = definition
        .rule_metadata()
        .early_close_rule_set()
        .map(ToOwned::to_owned);
    let metadata_discrepancies = match configured_session_timezone {
        Some(configured_timezone) if configured_timezone != resolved_timezone_name => {
            vec![format!(
                "configured session_timezone does not match explicit session-definition timezone for `{exchange_code}` and the session-definition timezone was used instead"
            )]
        }
        _ => Vec::new(),
    };
    let regular_session_windows = regular_session_windows_from_definition(definition);
    let session_template = SessionTemplate {
        policy_id: "session_definition_source.calendar_registry_json.v1".to_string(),
        exchange_name: exchange_name.clone(),
        timezone_name: resolved_timezone_name.clone(),
        timezone_source: "session_definition_source".to_string(),
        holiday_calendar_code,
        early_close_rule_set,
        regular_session_windows: regular_session_windows.clone(),
        continuous_weekday_template: looks_like_continuous_weekday_template(
            exchange_code,
            regular_session_windows.as_slice(),
        ),
    };

    Ok(ResolvedMarketDataSession {
        calendar_registry,
        session_resolution: MarketDataSessionResolutionPayload {
            schema_version: SESSION_RESOLUTION_SCHEMA_VERSION.to_string(),
            resolution_kind: "explicit_override".to_string(),
            selected_policy_id: "session_definition_source.calendar_registry_json.v1".to_string(),
            manual_override_applied: true,
            exchange_code: exchange_code.to_string(),
            exchange_name,
            resolved_timezone_name,
            timezone_source: "session_definition_source".to_string(),
            open_session_duration_hours_per_week: compute_open_session_duration_hours_per_week(
                &session_template,
            ),
            confidence_score: 1.0,
            confidence_label: "explicit_override".to_string(),
            regular_session_windows,
            recurring_closed_windows: build_recurring_closed_windows(&session_template),
            reasons: vec![
                "loaded session behavior from the explicit session-definition override and reused it for calibration and simulation preparation".to_string(),
            ],
            ambiguity_warnings: Vec::new(),
            metadata_discrepancies,
            resolved_session_definition_json: calendar_registry_json,
        },
    })
}

fn infer_session_template(
    exchange_code: &str,
    configured_session_timezone: Option<&str>,
    tick_records: &[MarketDataTranslationTickRecord],
) -> Result<SessionTemplate, MarketDataLoadingError> {
    match exchange_code {
        "XNYS" | "XNAS" => {
            let exchange_name = if exchange_code == "XNYS" {
                "New York Stock Exchange"
            } else {
                "Nasdaq Stock Market"
            };
            if let Some(timezone_name) = configured_session_timezone {
                if timezone_name != "America/New_York" {
                    return Err(MarketDataLoadingError::InvalidValue {
                        field_path: "session_configuration.session_timezone".to_string(),
                        message: format!(
                            "conflicts with the built-in exchange default for {exchange_code}. Provide an explicit session_definition_source to override the default."
                        ),
                    });
                }
            }
            return Ok(SessionTemplate {
                policy_id: format!(
                    "exchange_code_builtin_default.{}_regular_session.v1",
                    exchange_code.to_ascii_lowercase()
                ),
                exchange_name: exchange_name.to_string(),
                timezone_name: "America/New_York".to_string(),
                timezone_source: "exchange_code_builtin_default".to_string(),
                holiday_calendar_code: "US_EQUITIES".to_string(),
                early_close_rule_set: Some("US_EQUITIES_EARLY_CLOSE".to_string()),
                regular_session_windows: trading_weekdays()
                    .iter()
                    .map(|weekday| MarketDataSessionRegularWindowPayload {
                        weekday: weekday.to_string(),
                        label: "regular".to_string(),
                        open_time: "09:30".to_string(),
                        close_time: "16:00".to_string(),
                    })
                    .collect(),
                continuous_weekday_template: false,
            });
        }
        "FXSPOT" => {
            if let Some(timezone_name) = configured_session_timezone {
                if timezone_name != "UTC" {
                    return Err(MarketDataLoadingError::InvalidValue {
                        field_path: "session_configuration.session_timezone".to_string(),
                        message: "conflicts with the built-in exchange default for FXSPOT. Provide an explicit session_definition_source to override the default.".to_string(),
                    });
                }
            }
            return Ok(SessionTemplate {
                policy_id: "exchange_code_builtin_default.fxspot_weekend_window.v1".to_string(),
                exchange_name: "OTC FX Spot 24/5".to_string(),
                timezone_name: "UTC".to_string(),
                timezone_source: "exchange_code_builtin_default".to_string(),
                holiday_calendar_code: "FX_24_5".to_string(),
                early_close_rule_set: None,
                regular_session_windows: vec![
                    MarketDataSessionRegularWindowPayload { weekday: "Monday".to_string(), label: "spot".to_string(), open_time: "00:00".to_string(), close_time: "24:00".to_string() },
                    MarketDataSessionRegularWindowPayload { weekday: "Tuesday".to_string(), label: "spot".to_string(), open_time: "00:00".to_string(), close_time: "24:00".to_string() },
                    MarketDataSessionRegularWindowPayload { weekday: "Wednesday".to_string(), label: "spot".to_string(), open_time: "00:00".to_string(), close_time: "24:00".to_string() },
                    MarketDataSessionRegularWindowPayload { weekday: "Thursday".to_string(), label: "spot".to_string(), open_time: "00:00".to_string(), close_time: "24:00".to_string() },
                    MarketDataSessionRegularWindowPayload { weekday: "Friday".to_string(), label: "spot".to_string(), open_time: "00:00".to_string(), close_time: "19:00".to_string() },
                    MarketDataSessionRegularWindowPayload { weekday: "Sunday".to_string(), label: "spot".to_string(), open_time: "21:00".to_string(), close_time: "24:00".to_string() },
                ],
                continuous_weekday_template: false,
            });
        }
        "CRYPTO" => {
            if let Some(timezone_name) = configured_session_timezone {
                if timezone_name != "UTC" {
                    return Err(MarketDataLoadingError::InvalidValue {
                        field_path: "session_configuration.session_timezone".to_string(),
                        message: "conflicts with the built-in exchange default for CRYPTO. Provide an explicit session_definition_source to override the default.".to_string(),
                    });
                }
            }
            return Ok(SessionTemplate {
                policy_id: "exchange_code_builtin_default.crypto_continuous_session.v1".to_string(),
                exchange_name: "Crypto Spot 24/7".to_string(),
                timezone_name: "UTC".to_string(),
                timezone_source: "exchange_code_builtin_default".to_string(),
                holiday_calendar_code: "CRYPTO_24_7".to_string(),
                early_close_rule_set: None,
                regular_session_windows: weekday_names()
                    .iter()
                    .map(|weekday| MarketDataSessionRegularWindowPayload {
                        weekday: weekday.to_string(),
                        label: "spot".to_string(),
                        open_time: "00:00".to_string(),
                        close_time: "24:00".to_string(),
                    })
                    .collect(),
                continuous_weekday_template: false,
            });
        }
        _ => {}
    }

    if exchange_code.starts_with("FX_") || exchange_code.starts_with("CFD_") {
        let generic_template =
            build_generic_24_5_template(exchange_code, configured_session_timezone);
        validate_generic_24_5_inference(exchange_code, tick_records)?;
        return Ok(generic_template);
    }

    let Some(timezone_name) = configured_session_timezone else {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: "could not infer session behavior because no explicit session_definition_source or session_timezone was provided for an exchange code without built-in defaults. Provide a session-definition override.".to_string(),
        });
    };

    let generic_template = build_generic_24_5_template(exchange_code, Some(timezone_name));
    validate_generic_24_5_inference(exchange_code, tick_records)?;
    Ok(generic_template)
}

fn build_generic_24_5_template(
    exchange_code: &str,
    configured_session_timezone: Option<&str>,
) -> SessionTemplate {
    let timezone_name = configured_session_timezone.unwrap_or("UTC").to_string();
    let exchange_label = exchange_code.replace('_', " ");
    let (holiday_calendar_code, exchange_name) = if exchange_code.starts_with("CFD_") {
        ("CFD_24_5", format!("{} CFD 24/5", exchange_label))
    } else {
        ("FX_24_5", format!("{} 24/5", exchange_label))
    };

    SessionTemplate {
        policy_id: "recurring_weekly_inactivity.generic_24_5_broker_market.v1".to_string(),
        exchange_name,
        timezone_name,
        timezone_source: configured_session_timezone
            .map(|_| "session_configuration.session_timezone".to_string())
            .unwrap_or_else(|| "default_utc_for_generic_24_5_inference".to_string()),
        holiday_calendar_code: holiday_calendar_code.to_string(),
        early_close_rule_set: None,
        regular_session_windows: trading_weekdays()
            .iter()
            .map(|weekday| MarketDataSessionRegularWindowPayload {
                weekday: weekday.to_string(),
                label: "regular".to_string(),
                open_time: "00:00".to_string(),
                close_time: "24:00".to_string(),
            })
            .collect(),
        continuous_weekday_template: true,
    }
}

fn validate_generic_24_5_inference(
    exchange_code: &str,
    tick_records: &[MarketDataTranslationTickRecord],
) -> Result<(), MarketDataLoadingError> {
    let mut distinct_weekdays = HashSet::new();
    let mut long_gap_count = 0;
    let mut weekend_gap_count = 0;
    let mut unexpected_weekday_gap_count = 0;
    for tick_record in tick_records {
        let weekday = DateTime::<Utc>::from_timestamp_nanos(tick_record.timestamp_utc_nanos)
            .weekday()
            .num_days_from_monday();
        distinct_weekdays.insert(weekday);
    }

    for pair in tick_records.windows(2) {
        let gap_nanos = pair[1].timestamp_utc_nanos - pair[0].timestamp_utc_nanos;
        if gap_nanos < Duration::hours(LONG_GAP_THRESHOLD_HOURS).num_nanoseconds().unwrap_or(0) {
            continue;
        }
        long_gap_count += 1;
        let midpoint_nanos = pair[0].timestamp_utc_nanos + (gap_nanos / 2);
        let midpoint_weekday = DateTime::<Utc>::from_timestamp_nanos(midpoint_nanos)
            .weekday()
            .num_days_from_monday();
        if midpoint_weekday >= 5 {
            weekend_gap_count += 1;
        } else {
            unexpected_weekday_gap_count += 1;
        }
    }

    if distinct_weekdays.len() < MINIMUM_GENERIC_24_5_WEEKDAY_COUNT {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: format!(
                "automatic 24/5 session inference is ambiguous because the observed tick window does not span enough weekdays. Provide an explicit session-definition override. exchange_code={exchange_code}"
            ),
        });
    }
    if long_gap_count == 0 || weekend_gap_count == 0 || unexpected_weekday_gap_count > 0 {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: format!(
                "automatic 24/5 session inference is ambiguous because the observed gap structure does not clearly support a weekday-continuous broker market. Provide an explicit session-definition override. exchange_code={exchange_code}"
            ),
        });
    }

    Ok(())
}

fn load_option_surface_records(
    option_surface_input: &MarketDataOptionSurfaceLoadingInput,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Result<OptionSurfaceLoadOutput, MarketDataLoadingError> {
    let required_columns =
        required_option_surface_source_columns(&option_surface_input.column_mapping, normalization_policy);
    let available_columns = load_source_columns(&option_surface_input.source, "option_surface_source")?;
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
        MarketDataInputFileFormat::Json => Err(MarketDataLoadingError::InvalidValue {
            field_path: format!("{field_path}.file_format"),
            message: "must be one of: csv, parquet".to_string(),
        }),
    }
}

fn load_rows(
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

fn required_option_surface_source_columns(
    column_mapping: &MarketDataOptionSurfaceColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Vec<String> {
    let mut columns = vec![
        column_mapping.quote_timestamp_column.clone(),
        column_mapping.expiry_timestamp_column.clone(),
        column_mapping.strike_column.clone(),
        column_mapping.option_type_column.clone(),
    ];
    match normalization_policy.option_quote_input_policy {
        Some(MarketDataOptionQuoteInputPolicy::MidPriceColumn) => {
            if let Some(column) = &column_mapping.mid_price_column {
                columns.push(column.clone());
            }
        }
        Some(MarketDataOptionQuoteInputPolicy::BidAskMidpoint) => {
            if let Some(column) = &column_mapping.bid_column {
                columns.push(column.clone());
            }
            if let Some(column) = &column_mapping.ask_column {
                columns.push(column.clone());
            }
        }
        Some(MarketDataOptionQuoteInputPolicy::ImpliedVolatilityColumn) => {
            if let Some(column) = &column_mapping.implied_volatility_column {
                columns.push(column.clone());
            }
        }
        None => {}
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

fn option_quote_policy_string(policy: MarketDataOptionQuoteInputPolicy) -> String {
    match policy {
        MarketDataOptionQuoteInputPolicy::MidPriceColumn => "mid_price_column".to_string(),
        MarketDataOptionQuoteInputPolicy::BidAskMidpoint => "bid_ask_midpoint".to_string(),
        MarketDataOptionQuoteInputPolicy::ImpliedVolatilityColumn => {
            "implied_volatility_column".to_string()
        }
    }
}

fn file_format_string(format: MarketDataInputFileFormat) -> &'static str {
    match format {
        MarketDataInputFileFormat::Csv => "csv",
        MarketDataInputFileFormat::Parquet => "parquet",
        MarketDataInputFileFormat::Json => "json",
    }
}

fn normalize_required_numeric_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
    allow_zero: bool,
    allow_negative: bool,
) -> Result<f64, MarketDataLoadingError> {
    normalize_optional_numeric_value(
        raw_value,
        field_path,
        source_path,
        allow_zero,
        allow_negative,
    )?
    .ok_or_else(|| MarketDataLoadingError::InvalidValue {
        field_path: field_path.to_string(),
        message: format!("is required and cannot be blank. source_path={source_path}"),
    })
}

fn normalize_required_text_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
) -> Result<String, MarketDataLoadingError> {
    normalize_optional_text_value(raw_value, field_path, source_path)?.ok_or_else(|| {
        MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("is required and cannot be blank. source_path={source_path}"),
        }
    })
}

fn normalize_option_type(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
) -> Result<String, MarketDataLoadingError> {
    let text_value = normalize_required_text_value(raw_value, field_path, source_path)?
        .to_ascii_lowercase();
    match text_value.as_str() {
        "call" | "c" => Ok("call".to_string()),
        "put" | "p" => Ok("put".to_string()),
        _ => Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("must be one of: call, put, c, p. source_path={source_path}"),
        }),
    }
}

fn normalize_option_surface_row(
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
        format!("{row_field_path}.{}", column_mapping.expiry_timestamp_column).as_str(),
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

fn option_surface_record_dedup_key(
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

fn trading_weekdays() -> [&'static str; 5] {
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday"]
}

fn weekday_names() -> [&'static str; 7] {
    [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ]
}

fn regular_session_windows_from_definition(
    definition: &crate::calendar::ExchangeCalendarDefinition,
) -> Vec<MarketDataSessionRegularWindowPayload> {
    let mut result = Vec::new();
    for regular_session in definition.regular_sessions() {
        for window in regular_session.windows() {
            result.push(MarketDataSessionRegularWindowPayload {
                weekday: regular_session.weekday().to_string(),
                label: window.label().to_string(),
                open_time: window.open_time().to_string(),
                close_time: window.close_time().to_string(),
            });
        }
    }

    result
}

fn looks_like_continuous_weekday_template(
    exchange_code: &str,
    regular_session_windows: &[MarketDataSessionRegularWindowPayload],
) -> bool {
    if !(exchange_code.starts_with("FX_") || exchange_code.starts_with("CFD_")) {
        return false;
    }
    if regular_session_windows.len() != trading_weekdays().len() {
        return false;
    }

    regular_session_windows.iter().all(|window| {
        window.open_time == "00:00" && window.close_time == "24:00"
    })
}

fn build_recurring_closed_windows(
    session_template: &SessionTemplate,
) -> Vec<MarketDataSessionRecurringClosedWindowPayload> {
    if session_template.continuous_weekday_template {
        return vec![MarketDataSessionRecurringClosedWindowPayload {
            start_weekday: "Saturday".to_string(),
            start_time: "00:00".to_string(),
            end_weekday: "Monday".to_string(),
            end_time: "00:00".to_string(),
            reason: "weekend inactivity window inferred for weekday-continuous 24/5 trading"
                .to_string(),
        }];
    }

    let mut open_intervals = session_template
        .regular_session_windows
        .iter()
        .map(|window| {
            (
                weekday_start_minute(window.weekday.as_str()) + clock_minutes(window.open_time.as_str()),
                weekday_start_minute(window.weekday.as_str()) + clock_minutes(window.close_time.as_str()),
            )
        })
        .collect::<Vec<_>>();
    open_intervals.sort_unstable();

    let week_minutes = 7 * 24 * 60;
    let mut closed_intervals = Vec::new();
    let mut cursor = 0;
    for (open_start, open_end) in open_intervals {
        if cursor < open_start {
            closed_intervals.push((cursor, open_start));
        }
        cursor = cursor.max(open_end);
    }
    if cursor < week_minutes {
        closed_intervals.push((cursor, week_minutes));
    }

    closed_intervals
        .into_iter()
        .map(|(start_minute, end_minute)| MarketDataSessionRecurringClosedWindowPayload {
            start_weekday: weekday_name_for_minute(start_minute).to_string(),
            start_time: clock_time_for_minute(start_minute),
            end_weekday: weekday_name_for_minute(end_minute % week_minutes).to_string(),
            end_time: clock_time_for_minute(end_minute % week_minutes),
            reason: "recurring closure derived from the resolved weekly session pattern".to_string(),
        })
        .collect()
}

fn compute_open_session_duration_hours_per_week(session_template: &SessionTemplate) -> f64 {
    if session_template.continuous_weekday_template {
        return 120.0;
    }

    let open_minutes = session_template
        .regular_session_windows
        .iter()
        .map(|window| clock_minutes(window.close_time.as_str()) - clock_minutes(window.open_time.as_str()))
        .sum::<i32>();
    f64::from(open_minutes) / 60.0
}

fn build_calendar_registry_json(
    exchange_code: &str,
    session_template: &SessionTemplate,
) -> Result<String, MarketDataLoadingError> {
    let exchange_artifact = ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: exchange_code.to_string(),
        exchange_name: session_template.exchange_name.clone(),
        timezone_name: Some(session_template.timezone_name.clone()),
        rule_metadata: CalendarRuleMetadataArtifact {
            holiday_calendar_code: session_template.holiday_calendar_code.clone(),
            early_close_rule_set: session_template.early_close_rule_set.clone(),
        },
        regular_sessions: build_regular_session_artifacts(
            session_template.regular_session_windows.as_slice(),
            build_recurring_closed_windows(session_template).as_slice(),
        ),
        holiday_dates: Vec::new(),
        early_close_sessions: Vec::new(),
    };
    let artifact = ExchangeCalendarRegistryArtifact {
        schema_version: "calendar.registry.v1".to_string(),
        artifact_date: Utc::now().date_naive().to_string(),
        exchanges: vec![exchange_artifact],
    };

    serde_json::to_string(&artifact).map_err(|error| MarketDataLoadingError::InvalidValue {
        field_path: "session_configuration".to_string(),
        message: format!("failed to serialize resolved calendar registry: {error}"),
    })
}

fn build_regular_session_artifacts(
    regular_session_windows: &[MarketDataSessionRegularWindowPayload],
    recurring_closed_windows: &[MarketDataSessionRecurringClosedWindowPayload],
) -> Vec<RegularSessionArtifact> {
    let retained_open_intervals = subtract_closed_intervals_from_open_intervals(
        regular_session_windows
            .iter()
            .map(|window| {
                (
                    weekday_start_minute(window.weekday.as_str())
                        + clock_minutes(window.open_time.as_str()),
                    weekday_start_minute(window.weekday.as_str())
                        + clock_minutes(window.close_time.as_str()),
                    window.label.clone(),
                )
            })
            .collect(),
        merge_weekly_intervals(
            recurring_closed_windows
                .iter()
                .flat_map(weekly_intervals_for_recurring_closed_window)
                .collect(),
        ),
    );
    let mut windows_by_weekday: BTreeMap<String, Vec<SessionWindowArtifact>> = BTreeMap::new();
    for (open_start, open_end, label) in retained_open_intervals {
        let mut current_minute = open_start;
        while current_minute < open_end {
            let day_start_minute = (current_minute / (24 * 60)) * (24 * 60);
            let day_end_minute = day_start_minute + (24 * 60);
            let segment_end_minute = open_end.min(day_end_minute);
            let weekday_name = weekday_name_for_minute(current_minute).to_string();
            windows_by_weekday
                .entry(weekday_name)
                .or_default()
                .push(SessionWindowArtifact {
                    label: label.clone(),
                    open_time: clock_time_for_minute_of_day(current_minute - day_start_minute),
                    close_time: clock_time_for_minute_of_day(segment_end_minute - day_start_minute),
                });
            current_minute = segment_end_minute;
        }
    }

    weekday_names()
        .iter()
        .filter_map(|weekday| {
            windows_by_weekday.remove(*weekday).map(|mut windows| {
                windows.sort_by(|left, right| {
                    (
                        left.open_time.as_str(),
                        left.close_time.as_str(),
                        left.label.as_str(),
                    )
                        .cmp(&(
                            right.open_time.as_str(),
                            right.close_time.as_str(),
                            right.label.as_str(),
                        ))
                });
                RegularSessionArtifact {
                    weekday: trading_weekday_from_name(weekday),
                    windows,
                }
            })
        })
        .collect()
}

fn weekly_intervals_for_recurring_closed_window(
    closed_window: &MarketDataSessionRecurringClosedWindowPayload,
) -> Vec<(i32, i32)> {
    let week_minutes = 7 * 24 * 60;
    let start_minute =
        weekday_start_minute(closed_window.start_weekday.as_str()) + clock_minutes(closed_window.start_time.as_str());
    let end_minute =
        weekday_start_minute(closed_window.end_weekday.as_str()) + clock_minutes(closed_window.end_time.as_str());

    if start_minute < end_minute {
        vec![(start_minute, end_minute)]
    } else if start_minute > end_minute {
        vec![(start_minute, week_minutes), (0, end_minute)]
    } else {
        vec![(0, week_minutes)]
    }
}

fn merge_weekly_intervals(mut intervals: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    if intervals.is_empty() {
        return intervals;
    }
    intervals.sort_unstable();
    let mut merged = vec![intervals[0]];
    for (start_minute, end_minute) in intervals.into_iter().skip(1) {
        let previous = merged.last_mut().expect("merged is initialized");
        if start_minute <= previous.1 {
            previous.1 = previous.1.max(end_minute);
        } else {
            merged.push((start_minute, end_minute));
        }
    }
    merged
}

fn subtract_closed_intervals_from_open_intervals(
    open_intervals: Vec<(i32, i32, String)>,
    closed_intervals: Vec<(i32, i32)>,
) -> Vec<(i32, i32, String)> {
    let mut retained = Vec::new();
    for (open_start, open_end, label) in open_intervals {
        let mut current_segments = vec![(open_start, open_end)];
        for (closed_start, closed_end) in &closed_intervals {
            let mut next_segments = Vec::new();
            for (segment_start, segment_end) in &current_segments {
                if *closed_end <= *segment_start || *closed_start >= *segment_end {
                    next_segments.push((*segment_start, *segment_end));
                    continue;
                }
                if *closed_start > *segment_start {
                    next_segments.push((*segment_start, *closed_start));
                }
                if *closed_end < *segment_end {
                    next_segments.push((*closed_end, *segment_end));
                }
            }
            current_segments = next_segments;
            if current_segments.is_empty() {
                break;
            }
        }
        retained.extend(
            current_segments
                .into_iter()
                .filter(|(segment_start, segment_end)| segment_start < segment_end)
                .map(|(segment_start, segment_end)| (segment_start, segment_end, label.clone())),
        );
    }
    retained
}

fn evaluate_gap_alignment(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    tick_records: &[MarketDataTranslationTickRecord],
) -> Result<(usize, usize, usize), MarketDataLoadingError> {
    let mut long_gap_count = 0;
    let mut aligned_gap_count = 0;
    let mut unexpected_gap_count = 0;
    for pair in tick_records.windows(2) {
        let gap_nanos = pair[1].timestamp_utc_nanos - pair[0].timestamp_utc_nanos;
        if gap_nanos < Duration::hours(LONG_GAP_THRESHOLD_HOURS).num_nanoseconds().unwrap_or(0) {
            continue;
        }
        long_gap_count += 1;
        if gap_contains_closed_period(calendar_registry, exchange_code, pair[0].timestamp_utc_nanos, pair[1].timestamp_utc_nanos)? {
            aligned_gap_count += 1;
        } else {
            unexpected_gap_count += 1;
        }
    }

    Ok((long_gap_count, aligned_gap_count, unexpected_gap_count))
}

fn gap_contains_closed_period(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    gap_start_utc_nanos: i64,
    gap_end_utc_nanos: i64,
) -> Result<bool, MarketDataLoadingError> {
    let mut probe_timestamp = DateTime::<Utc>::from_timestamp_nanos(gap_start_utc_nanos)
        + Duration::minutes(GAP_SCAN_STEP_MINUTES);
    let gap_end = DateTime::<Utc>::from_timestamp_nanos(gap_end_utc_nanos);
    while probe_timestamp < gap_end {
        let market_state = calendar_registry
            .resolve_market_state_at_utc(exchange_code, probe_timestamp)
            .map_err(|error| MarketDataLoadingError::InvalidValue {
                field_path: "session_configuration".to_string(),
                message: format!("failed to resolve inferred market state: {error}"),
            })?;
        if market_state == MarketState::Closed {
            return Ok(true);
        }
        probe_timestamp += Duration::minutes(GAP_SCAN_STEP_MINUTES);
    }

    Ok(false)
}

fn confidence_score_for_inference(
    template: &SessionTemplate,
    gap_alignment_diagnostics: (usize, usize, usize),
) -> f64 {
    let (long_gap_count, aligned_gap_count, unexpected_gap_count) = gap_alignment_diagnostics;
    let mut confidence_score: f64 = if template.timezone_source == "exchange_code_builtin_default" {
        0.65
    } else {
        0.55
    };
    if aligned_gap_count > 0 {
        confidence_score += 0.25;
    }
    if template.continuous_weekday_template {
        confidence_score += 0.10;
    }
    if long_gap_count > 0 && unexpected_gap_count == 0 {
        confidence_score += 0.10;
    }
    if unexpected_gap_count > 0 {
        confidence_score -= 0.20;
    }

    confidence_score.clamp(0.0, 1.0)
}

fn confidence_label_for_gap_alignment(
    template: &SessionTemplate,
    gap_alignment_diagnostics: (usize, usize, usize),
) -> String {
    let confidence_score = confidence_score_for_inference(template, gap_alignment_diagnostics);
    if confidence_score >= 0.9 {
        "high".to_string()
    } else if confidence_score >= 0.75 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

fn automatic_resolution_reasons(
    exchange_code: &str,
    template: &SessionTemplate,
    gap_alignment_diagnostics: (usize, usize, usize),
) -> Vec<String> {
    let (long_gap_count, aligned_gap_count, _) = gap_alignment_diagnostics;
    let mut reasons = Vec::new();
    if template.timezone_source == "exchange_code_builtin_default" {
        reasons.push(format!(
            "used the built-in exchange default for `{exchange_code}` to resolve the trading schedule"
        ));
    } else {
        reasons.push(
            "selected the generic weekday-continuous 24/5 broker-market template based on exchange code or workflow defaults".to_string(),
        );
    }
    if aligned_gap_count > 0 {
        reasons.push(format!(
            "validated the inferred session against {aligned_gap_count} observed long gaps that overlapped inferred closed windows"
        ));
    } else if long_gap_count == 0 {
        reasons.push(
            "no long gaps were present in the observed window, so exchange metadata carried the inference"
                .to_string(),
        );
    }

    reasons
}

fn automatic_resolution_warnings(
    template: &SessionTemplate,
    gap_alignment_diagnostics: (usize, usize, usize),
) -> Vec<String> {
    let (long_gap_count, _, unexpected_gap_count) = gap_alignment_diagnostics;
    let mut warnings = Vec::new();
    if long_gap_count == 0 && template.timezone_source == "exchange_code_builtin_default" {
        warnings.push(
            "observed data did not include long inactivity gaps, so the session was inferred from exchange defaults".to_string(),
        );
    }
    if unexpected_gap_count > 0 {
        warnings.push(format!(
            "{unexpected_gap_count} observed long gaps did not clearly overlap inferred closed windows. If downstream diagnostics look inconsistent, provide an explicit session-definition override."
        ));
    }

    warnings
}

fn weekday_start_minute(weekday_name: &str) -> i32 {
    match weekday_name {
        "Monday" => 0,
        "Tuesday" => 24 * 60,
        "Wednesday" => 2 * 24 * 60,
        "Thursday" => 3 * 24 * 60,
        "Friday" => 4 * 24 * 60,
        "Saturday" => 5 * 24 * 60,
        "Sunday" => 6 * 24 * 60,
        _ => 0,
    }
}

fn clock_minutes(clock_text: &str) -> i32 {
    let mut parts = clock_text.split(':');
    let hour = parts.next().unwrap_or("0").parse::<i32>().unwrap_or(0);
    let minute = parts.next().unwrap_or("0").parse::<i32>().unwrap_or(0);
    (hour * 60) + minute
}

fn weekday_name_for_minute(week_minute: i32) -> &'static str {
    weekday_names()[((week_minute / (24 * 60)).rem_euclid(7)) as usize]
}

fn clock_time_for_minute(week_minute: i32) -> String {
    let minute_of_day = week_minute.rem_euclid(24 * 60);
    let hour = minute_of_day / 60;
    let minute = minute_of_day % 60;
    format!("{hour:02}:{minute:02}")
}

fn clock_time_for_minute_of_day(minute_of_day: i32) -> String {
    if minute_of_day == 24 * 60 {
        return "24:00".to_string();
    }
    let hour = minute_of_day / 60;
    let minute = minute_of_day % 60;
    format!("{hour:02}:{minute:02}")
}

fn trading_weekday_from_name(name: &str) -> TradingWeekday {
    match name {
        "Monday" => TradingWeekday::Monday,
        "Tuesday" => TradingWeekday::Tuesday,
        "Wednesday" => TradingWeekday::Wednesday,
        "Thursday" => TradingWeekday::Thursday,
        "Friday" => TradingWeekday::Friday,
        "Saturday" => TradingWeekday::Saturday,
        "Sunday" => TradingWeekday::Sunday,
        _ => TradingWeekday::Monday,
    }
}
