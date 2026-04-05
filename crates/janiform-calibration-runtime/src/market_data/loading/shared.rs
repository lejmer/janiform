pub(super) use chrono::{DateTime, Duration, Utc};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hash::Hash;

pub(super) use crate::calendar::{
    CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarRegistryArtifact,
    MarketState, RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
};
use crate::market_data::preprocessing::{
    MarketDataTranslationKnownNewsRecord, MarketDataTranslationTickRecord,
};

use super::contracts::{
    MarketDataInputFileFormat, MarketDataKnownNewsColumnMapping, MarketDataNormalizationPolicy,
    MarketDataOptionQuoteInputPolicy, MarketDataOptionSurfaceColumnMapping,
    MarketDataSessionRecurringClosedWindowPayload, MarketDataSessionRegularWindowPayload,
    MarketDataSessionResolutionPayload, MarketDataSourceSummary,
};

pub(super) const COMMON_TIMESTAMP_FORMATS: [&str; 37] = [
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

pub(super) const SESSION_RESOLUTION_SCHEMA_VERSION: &str =
    "calibration.market_data.session_resolution.v1";
pub(super) const LONG_GAP_THRESHOLD_HOURS: i64 = 4;
pub(super) const GAP_SCAN_STEP_MINUTES: i64 = 30;
pub(super) const MINIMUM_GENERIC_24_5_WEEKDAY_COUNT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum TradeSign {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub(super) struct LoadedTickRecord {
    pub(super) timestamp_utc_nanos: i64,
    pub(super) price: Option<f64>,
    pub(super) trade_price: Option<f64>,
    pub(super) trade_sign: Option<TradeSign>,
    pub(super) bid_price: Option<f64>,
    pub(super) ask_price: Option<f64>,
    pub(super) close_price: Option<f64>,
    pub(super) source_row_number: usize,
}

#[derive(Debug, Clone)]
pub(super) struct LoadedKnownNewsRecord {
    pub(super) event_timestamp_utc_nanos: i64,
    pub(super) event_identifier: Option<String>,
    pub(super) source_row_number: usize,
}

#[derive(Debug, Clone)]
pub(super) struct LoadedOptionSurfaceRecord {
    pub(super) quote_timestamp_utc_nanos: i64,
    pub(super) expiry_timestamp_utc_nanos: i64,
    pub(super) strike: f64,
    pub(super) option_type: String,
    pub(super) quote_value: f64,
    pub(super) mid_price: Option<f64>,
    pub(super) bid_price: Option<f64>,
    pub(super) ask_price: Option<f64>,
    pub(super) implied_volatility: Option<f64>,
    pub(super) source_row_number: usize,
}

#[derive(Debug, Clone)]
pub(super) struct TickLoadOutput {
    pub(super) tick_records: Vec<MarketDataTranslationTickRecord>,
    pub(super) directional_label_diagnostics: Option<CalibrationDirectionalLabelDiagnosticsPayload>,
    pub(super) summary: MarketDataSourceSummary,
    pub(super) reference_price: Option<f64>,
    pub(super) fit_warning: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct KnownNewsLoadOutput {
    pub(super) known_news_records: Vec<MarketDataTranslationKnownNewsRecord>,
    pub(super) summary: MarketDataSourceSummary,
}

#[derive(Debug, Clone)]
pub(super) struct OptionSurfaceLoadOutput {
    pub(super) option_surface_quote_values: Vec<f64>,
    pub(super) option_surface_records: Vec<LoadedOptionSurfaceRecord>,
    pub(super) summary: MarketDataSourceSummary,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedMarketDataSession {
    pub(super) session_resolution: MarketDataSessionResolutionPayload,
}

#[derive(Debug, Clone)]
pub(super) struct SessionTemplate {
    pub(super) policy_id: String,
    pub(super) provenance_kind: String,
    pub(super) exchange_name: String,
    pub(super) timezone_name: String,
    pub(super) timezone_source: String,
    pub(super) holiday_calendar_code: String,
    pub(super) early_close_rule_set: Option<String>,
    pub(super) regular_session_windows: Vec<MarketDataSessionRegularWindowPayload>,
    pub(super) recurring_closed_windows: Vec<MarketDataSessionRecurringClosedWindowPayload>,
    pub(super) continuous_weekday_template: bool,
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
pub(super) enum RawValue {
    Null,
    Text(String),
    Integer(i64),
    Float(f64),
    Boolean,
}

pub(super) type RawRow = BTreeMap<String, RawValue>;

pub(super) fn normalized_float_key(value: Option<f64>) -> Option<u64> {
    value.map(f64::to_bits)
}

pub(super) fn required_known_news_source_columns(
    column_mapping: &MarketDataKnownNewsColumnMapping,
) -> Vec<String> {
    let mut columns = vec![column_mapping.event_timestamp_column.clone()];
    if let Some(column) = &column_mapping.event_identifier_column {
        columns.push(column.clone());
    }

    dedup_vec(columns)
}

pub(super) fn required_option_surface_source_columns(
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

pub(super) fn dedup_vec<T>(values: Vec<T>) -> Vec<T>
where
    T: Eq + Hash + Clone,
{
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();

    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }

    deduped
}

pub(super) fn option_quote_policy_string(policy: MarketDataOptionQuoteInputPolicy) -> String {
    match policy {
        MarketDataOptionQuoteInputPolicy::MidPriceColumn => "mid_price_column".to_string(),
        MarketDataOptionQuoteInputPolicy::BidAskMidpoint => "bid_ask_midpoint".to_string(),
        MarketDataOptionQuoteInputPolicy::ImpliedVolatilityColumn => {
            "implied_volatility_column".to_string()
        }
    }
}

pub(super) fn file_format_string(format: MarketDataInputFileFormat) -> &'static str {
    match format {
        MarketDataInputFileFormat::Csv => "csv",
        MarketDataInputFileFormat::Parquet => "parquet",
        MarketDataInputFileFormat::Json => "json",
    }
}
