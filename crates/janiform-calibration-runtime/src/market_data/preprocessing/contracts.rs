use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_market_time::business_time_clock::FourierBusinessTimeClockAxis;

use crate::execution_bridge::{
    CalibrationHawkesDirectionalLabelDiagnostics, CalibrationHawkesStageCandidateInput,
    CalibrationMeanReversionStageObservation,
    CalibrationReturnComponentsStageCandidateGenerationInput,
    CalibrationSeasonalityStageExecutionInput, CalibrationTimestampEventSessionFilter,
};

const DEFAULT_RETURN_COMPONENTS_INTRADAY_JUMP_THRESHOLD_MULTIPLIER: f64 = 4.0;
const DEFAULT_RETURN_COMPONENTS_CLOSE_OPEN_GAP_TOLERANCE_MULTIPLIER: f64 = 3.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataTranslationCalibrationRequest {
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    pub tick_records: Vec<MarketDataTranslationTickRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_price: Option<f64>,
    #[serde(default)]
    pub known_news_records: Vec<MarketDataTranslationKnownNewsRecord>,
    #[serde(default)]
    pub option_surface_quote_values: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directional_label_diagnostics: Option<CalibrationHawkesDirectionalLabelDiagnostics>,
    pub require_hawkes: bool,
    pub require_return_components: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration_settings: Option<MarketDataTranslationCalibrationSettings>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataTranslationCalibrationSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seasonality: Option<MarketDataTranslationSeasonalityCalibrationSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_components: Option<MarketDataTranslationReturnComponentsCalibrationSettings>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataTranslationSeasonalityCalibrationSettings {
    pub session_filter: CalibrationTimestampEventSessionFilter,
    pub sparse_hour_event_threshold: usize,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub maximum_harmonic_orders: BTreeMap<FourierBusinessTimeClockAxis, usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataTranslationReturnComponentsCalibrationSettings {
    pub intraday_jump_threshold_multiplier: f64,
    pub close_open_gap_tolerance_multiplier: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_size: Option<f64>,
}

impl Default for MarketDataTranslationReturnComponentsCalibrationSettings {
    fn default() -> Self {
        Self {
            intraday_jump_threshold_multiplier:
                DEFAULT_RETURN_COMPONENTS_INTRADAY_JUMP_THRESHOLD_MULTIPLIER,
            close_open_gap_tolerance_multiplier:
                DEFAULT_RETURN_COMPONENTS_CLOSE_OPEN_GAP_TOLERANCE_MULTIPLIER,
            tick_size: None,
        }
    }
}

/// Return calibration settings with `tick_size` applied to return components when absent.
///
/// The caller owns tick-size precedence and validation. This helper only merges
/// a selected fallback into an owned calibration payload without replacing an
/// already supplied return-components tick size.
pub fn calibration_settings_with_return_components_tick_size(
    calibration_settings: Option<&MarketDataTranslationCalibrationSettings>,
    tick_size: Option<f64>,
) -> Option<MarketDataTranslationCalibrationSettings> {
    let Some(tick_size) = tick_size else {
        return calibration_settings.cloned();
    };

    let mut effective_settings =
        calibration_settings
            .cloned()
            .unwrap_or(MarketDataTranslationCalibrationSettings {
                seasonality: None,
                return_components: None,
            });

    let return_components = effective_settings
        .return_components
        .get_or_insert_with(MarketDataTranslationReturnComponentsCalibrationSettings::default);

    if return_components.tick_size.is_none() {
        return_components.tick_size = Some(tick_size);
    }

    Some(effective_settings)
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
pub struct MarketDataTranslationCalibrationOutput {
    pub seasonality_stage_input: CalibrationSeasonalityStageExecutionInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hawkes_candidate_inputs: Option<Vec<CalibrationHawkesStageCandidateInput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hawkes_event_side_codes: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_components_candidate_generation_input:
        Option<CalibrationReturnComponentsStageCandidateGenerationInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_reversion_open_session_price_observations:
        Option<Vec<CalibrationMeanReversionStageObservation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_reversion_observed_session_duration_hours: Option<Vec<f64>>,
    pub session_open_tick_count: usize,
    pub session_closed_tick_count: usize,
    pub known_news_event_count: usize,
    pub known_news_events_outside_source_window_count: usize,
    pub known_news_open_event_count: usize,
    pub known_news_closed_event_count: usize,
    pub material_close_open_gap_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarketDataTranslationCalibrationError {
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
    InvalidTickGridReferencePrice {
        reference_price: Option<f64>,
    },
    InvalidTimestampBatchingSurface {
        error_message: String,
    },
    InvalidSessionBoundary {
        exchange_code: String,
        timestamp_utc_nanos: i64,
    },
    SessionBoundaryOutOfNanosecondRange {
        exchange_code: String,
        local_date: String,
        local_time: String,
    },
    AmbiguousSessionBoundary {
        exchange_code: String,
        timezone_name: String,
        local_date: String,
        local_time: String,
    },
}

impl Display for MarketDataTranslationCalibrationError {
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
                "hawkes input calibration requires resolved trade signs for source_row_number `{source_row_number}`"
            ),
            Self::HawkesRequiresDirectionalLabelDiagnostics => {
                write!(
                    formatter,
                    "hawkes input calibration requires directional label diagnostics"
                )
            }
            Self::MissingOpenSessionPriceObservations => write!(
                formatter,
                "full calibration requires at least two open-session price observations"
            ),
            Self::InvalidTickGridReferencePrice { reference_price } => write!(
                formatter,
                "price discretization surface calibration requires a finite positive reference price, found `{reference_price:?}`"
            ),
            Self::InvalidTimestampBatchingSurface { error_message } => write!(
                formatter,
                "timestamp batching surface calibration failed: {error_message}"
            ),
            Self::InvalidSessionBoundary {
                exchange_code,
                timestamp_utc_nanos,
            } => write!(
                formatter,
                "calendar session boundary could not be resolved for exchange `{exchange_code}` at `{timestamp_utc_nanos}`"
            ),
            Self::SessionBoundaryOutOfNanosecondRange {
                exchange_code,
                local_date,
                local_time,
            } => write!(
                formatter,
                "calendar session boundary for exchange `{exchange_code}` at `{local_date} {local_time}` cannot be represented in UTC nanoseconds"
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

impl Error for MarketDataTranslationCalibrationError {}
