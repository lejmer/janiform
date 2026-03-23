use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::execution_bridge::{
    CalibrationHawkesDirectionalLabelDiagnostics, CalibrationHawkesStageCandidateInput,
    CalibrationReturnComponentsStageCandidateGenerationInput,
    CalibrationSeasonalityStageExecutionInput,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataTranslationPreprocessingRequest {
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    pub tick_records: Vec<MarketDataTranslationTickRecord>,
    #[serde(default)]
    pub known_news_records: Vec<MarketDataTranslationKnownNewsRecord>,
    #[serde(default)]
    pub option_surface_quote_values: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directional_label_diagnostics: Option<CalibrationHawkesDirectionalLabelDiagnostics>,
    pub require_hawkes: bool,
    pub require_return_components: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataTranslationTickRecord {
    pub timestamp_utc_nanos: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trade_sign: Option<String>,
    pub source_row_number: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataTranslationKnownNewsRecord {
    pub event_timestamp_utc_nanos: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_identifier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataTranslationPreprocessingOutput {
    pub seasonality_stage_input: CalibrationSeasonalityStageExecutionInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hawkes_candidate_inputs: Option<Vec<CalibrationHawkesStageCandidateInput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_components_candidate_generation_input:
        Option<CalibrationReturnComponentsStageCandidateGenerationInput>,
    pub session_open_tick_count: usize,
    pub session_closed_tick_count: usize,
    pub known_news_event_count: usize,
    pub known_news_events_outside_source_window_count: usize,
    pub known_news_open_event_count: usize,
    pub known_news_closed_event_count: usize,
    pub material_close_open_gap_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarketDataTranslationPreprocessingError {
    MissingExchangeId {
        exchange_code: String,
    },
    InvalidTimezoneName {
        exchange_code: String,
        timezone_name: String,
    },
    TickTimestampOutOfRange {
        source_row_number: usize,
    },
    KnownNewsTimestampOutOfRange {
        event_index: usize,
    },
    InvalidTradeSign {
        raw_value: String,
    },
    InvalidOpenSessionPrice {
        source_row_number: usize,
        price: f64,
    },
    HawkesRequiresResolvedTradeSigns {
        source_row_number: usize,
    },
    HawkesRequiresDirectionalLabelDiagnostics,
    MissingOpenSessionPriceObservations,
    InvalidSessionBoundary {
        exchange_code: String,
        timestamp_utc_nanos: i64,
    },
    AmbiguousSessionBoundary {
        exchange_code: String,
        timezone_name: String,
        local_date: String,
        local_time: String,
    },
}

impl Display for MarketDataTranslationPreprocessingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingExchangeId { exchange_code } => {
                write!(
                    formatter,
                    "calendar registry does not contain exchange `{exchange_code}`"
                )
            }
            Self::InvalidTimezoneName {
                exchange_code,
                timezone_name,
            } => write!(
                formatter,
                "calendar registry exchange `{exchange_code}` has invalid timezone `{timezone_name}`"
            ),
            Self::TickTimestampOutOfRange { source_row_number } => write!(
                formatter,
                "tick record source_row_number `{source_row_number}` has an out-of-range timestamp"
            ),
            Self::KnownNewsTimestampOutOfRange { event_index } => write!(
                formatter,
                "known-news record `{event_index}` has an out-of-range timestamp"
            ),
            Self::InvalidTradeSign { raw_value } => write!(
                formatter,
                "trade sign must be either `buy` or `sell`, found `{raw_value}`"
            ),
            Self::InvalidOpenSessionPrice {
                source_row_number,
                price,
            } => write!(
                formatter,
                "open-session tick price for source_row_number `{source_row_number}` must be strictly positive, found `{price}`"
            ),
            Self::HawkesRequiresResolvedTradeSigns { source_row_number } => write!(
                formatter,
                "hawkes preprocessing requires resolved trade signs for source_row_number `{source_row_number}`"
            ),
            Self::HawkesRequiresDirectionalLabelDiagnostics => {
                write!(
                    formatter,
                    "hawkes preprocessing requires directional label diagnostics"
                )
            }
            Self::MissingOpenSessionPriceObservations => write!(
                formatter,
                "full calibration requires at least two open-session price observations"
            ),
            Self::InvalidSessionBoundary {
                exchange_code,
                timestamp_utc_nanos,
            } => write!(
                formatter,
                "calendar session boundary could not be resolved for exchange `{exchange_code}` at `{timestamp_utc_nanos}`"
            ),
            Self::AmbiguousSessionBoundary {
                exchange_code,
                timezone_name,
                local_date,
                local_time,
            } => write!(
                formatter,
                "calendar session boundary for exchange `{exchange_code}` is ambiguous in timezone `{timezone_name}` at `{local_date} {local_time}`"
            ),
        }
    }
}

impl Error for MarketDataTranslationPreprocessingError {}
