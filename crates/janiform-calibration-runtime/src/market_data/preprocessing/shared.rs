pub(super) use std::collections::BTreeMap;
pub(super) use std::str::FromStr;

pub(super) use chrono::{DateTime, Datelike, LocalResult, NaiveDate, TimeZone, Timelike, Utc};
pub(super) use chrono_tz::Tz;

pub(super) use crate::execution_bridge::{
    CalibrationDayOfWeekSeasonalityEstimatorSettings,
    CalibrationDayOfYearSeasonalityEstimatorSettings, CalibrationHawkesDirectionalLabelDiagnostics,
    CalibrationHawkesStageCandidateInput, CalibrationIntradaySeasonalityEstimatorSettings,
    CalibrationIntradaySmoothingKernel, CalibrationMeanReversionStageObservation,
    CalibrationReturnComponentsCloseOpenGapDetectionPolicy,
    CalibrationReturnComponentsDetectedIntradayJumpCandidate,
    CalibrationReturnComponentsKnownNewsCalendarInput,
    CalibrationReturnComponentsKnownNewsReturnSample,
    CalibrationReturnComponentsObjectiveMetricWeights,
    CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    CalibrationReturnComponentsObservedOpenSessionReturn,
    CalibrationReturnComponentsSignalSourceWindow,
    CalibrationReturnComponentsStageCandidateGenerationInput, CalibrationScheduledEventOverlay,
    CalibrationSeasonalityStageExecutionInput, CalibrationSeasonalityStageExecutionSettings,
    CalibrationSharedNewsMarkLawConfiguration, CalibrationTimestampEventRecord,
    CalibrationTimestampEventSessionFilter,
};
pub(super) use crate::shock_mark_fitting::fit_shared_news_mark_law_from_shock_samples;
pub(super) use crate::stages::hawkes::HawkesStageObservedSourceRegime;
pub(super) use crate::temporal::datetime_from_nanos;
pub(super) use janiform_foundations::numeric::{
    arithmetic_mean, clamp_to_open_unit_interval, clamp_to_unit_interval, coefficient_of_variation,
    logistic, median_abs, median_abs_non_zero, normalized_pseudocount, round_positive_to_domain,
    round_to, smallest_positive_value, strictly_positive_machine_value, unique_sorted_values,
};
pub(super) use janiform_market_time::calendar::{
    CalendarDate, ExchangeCalendarDefinition, ExchangeCalendarRegistry,
    ExchangeCalendarRegistryLookupError, MarketState, SessionTime, SessionWindowDefinition,
    TradingWeekday,
};
pub(super) use janiform_model_kernels::hawkes::{
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
    HawkesStructuredRhoSelfDominancePolicy,
};
pub(super) use janiform_model_kernels::news::CoupledGapMarkConfiguration;

use super::contracts::{MarketDataTranslationPreprocessingError, MarketDataTranslationTickRecord};
pub(super) use super::contracts::{
    MarketDataTranslationReturnComponentsPreprocessingSettings,
    MarketDataTranslationSeasonalityPreprocessingSettings,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TradeSign {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedTickRecord {
    pub(super) timestamp_utc_nanos: i64,
    pub(super) trade_sign: Option<TradeSign>,
    pub(super) source_row_number: usize,
}

#[derive(Debug, Clone)]
pub(super) struct OpenTickRecord {
    pub(super) timestamp_utc_nanos: i64,
    pub(super) price: f64,
    pub(super) trade_sign: Option<TradeSign>,
    pub(super) session_open_utc_nanos: i64,
    pub(super) session_close_utc_nanos: i64,
}

#[derive(Debug, Clone)]
pub(super) struct SessionWindowUtc {
    pub(super) open_utc_nanos: i64,
    pub(super) close_utc_nanos: i64,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedKnownNewsRecord {
    pub(super) event_timestamp_utc_nanos: i64,
    pub(super) event_identifier: Option<String>,
    pub(super) market_state: MarketState,
}

#[derive(Debug, Clone)]
pub(super) struct TickPreprocessingSummary {
    pub(super) resolved_ticks: Vec<ResolvedTickRecord>,
    pub(super) open_price_ticks: Vec<OpenTickRecord>,
    pub(super) timestamp_event_records: Vec<CalibrationTimestampEventRecord>,
    pub(super) source_window: CalibrationReturnComponentsSignalSourceWindow,
    pub(super) session_open_tick_count: usize,
    pub(super) session_closed_tick_count: usize,
    pub(super) open_session_buy_tick_count: usize,
    pub(super) open_session_sell_tick_count: usize,
    pub(super) open_signed_tick_count: usize,
}

pub(super) struct KnownNewsResolutionSummary {
    pub(super) retained_records: Vec<ResolvedKnownNewsRecord>,
    pub(super) outside_source_window_count: usize,
    pub(super) open_event_count: usize,
    pub(super) closed_event_count: usize,
}

pub(super) fn parse_trade_sign(
    raw_trade_sign: Option<&str>,
) -> Result<Option<TradeSign>, MarketDataTranslationPreprocessingError> {
    match raw_trade_sign {
        Some("buy") => Ok(Some(TradeSign::Buy)),
        Some("sell") => Ok(Some(TradeSign::Sell)),
        Some(raw_trade_sign) => Err(MarketDataTranslationPreprocessingError::InvalidTradeSign {
            raw_value: raw_trade_sign.to_string(),
        }),
        None => Ok(None),
    }
}

pub(super) fn build_source_window(
    tick_records: &[MarketDataTranslationTickRecord],
) -> CalibrationReturnComponentsSignalSourceWindow {
    let window_start_utc_nanos = tick_records
        .first()
        .map(|record| record.timestamp_utc_nanos)
        .unwrap_or_default();
    let window_end_utc_nanos = tick_records
        .last()
        .map(|record| record.timestamp_utc_nanos.saturating_add(1))
        .unwrap_or(1);

    CalibrationReturnComponentsSignalSourceWindow {
        window_start_utc_nanos,
        window_end_utc_nanos,
    }
}

pub(super) fn observation_window_hours(
    first_timestamp_utc_nanos: Option<i64>,
    last_timestamp_utc_nanos: Option<i64>,
) -> f64 {
    match (first_timestamp_utc_nanos, last_timestamp_utc_nanos) {
        (Some(first_timestamp), Some(last_timestamp)) if last_timestamp > first_timestamp => {
            ((last_timestamp - first_timestamp) as f64 / 1_000_000_000.0).max(60.0) / 3600.0
        }
        _ => 1.0 / 60.0,
    }
}

pub(super) trait PartitionPointIndex {
    fn partition_point_index(&self, timestamp_utc_nanos: i64, include_equal: bool)
    -> Option<usize>;
}

impl PartitionPointIndex for [i64] {
    fn partition_point_index(
        &self,
        timestamp_utc_nanos: i64,
        include_equal: bool,
    ) -> Option<usize> {
        let partition_index = if include_equal {
            self.partition_point(|timestamp| *timestamp <= timestamp_utc_nanos)
        } else {
            self.partition_point(|timestamp| *timestamp < timestamp_utc_nanos)
        };

        partition_index.checked_sub(1)
    }
}
