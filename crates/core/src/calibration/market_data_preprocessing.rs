use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::calendar::{
    CalendarDate, ExchangeCalendarDefinition, ExchangeCalendarRegistry,
    ExchangeCalendarRegistryLookupError, MarketState, SessionTime, SessionWindowDefinition,
    TradingWeekday,
};
use crate::hawkes::{
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
    HawkesStructuredRhoSelfDominancePolicy,
};
use crate::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration,
};

use super::{
    CalibrationDayOfWeekSeasonalityEstimatorSettings,
    CalibrationDayOfYearSeasonalityEstimatorSettings, CalibrationHawkesDirectionalLabelDiagnostics,
    CalibrationHawkesObservedSourceRegime, CalibrationHawkesStageCandidateInput,
    CalibrationIntradaySeasonalityEstimatorSettings, CalibrationIntradaySmoothingKernel,
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

const ACTIVITY_LIFT_WINDOW_MINUTES: i64 = 30;
const KNOWN_NEWS_EXCLUSION_WINDOW_MINUTES: i64 = 15;
const MINIMUM_OPEN_TICK_COUNT_DURATION_HOURS: f64 = 0.25;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TradeSign {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
struct ResolvedTickRecord {
    timestamp_utc_nanos: i64,
    trade_sign: Option<TradeSign>,
    source_row_number: usize,
}

#[derive(Debug, Clone)]
struct OpenTickRecord {
    timestamp_utc_nanos: i64,
    price: f64,
    session_open_utc_nanos: i64,
    session_close_utc_nanos: i64,
}

#[derive(Debug, Clone)]
struct SessionWindowUtc {
    open_utc_nanos: i64,
    close_utc_nanos: i64,
}

#[derive(Debug, Clone)]
struct ResolvedKnownNewsRecord {
    event_timestamp_utc_nanos: i64,
    event_identifier: Option<String>,
    market_state: MarketState,
}

#[derive(Debug, Clone)]
struct TickPreprocessingSummary {
    resolved_ticks: Vec<ResolvedTickRecord>,
    open_price_ticks: Vec<OpenTickRecord>,
    timestamp_event_records: Vec<CalibrationTimestampEventRecord>,
    source_window: CalibrationReturnComponentsSignalSourceWindow,
    session_open_tick_count: usize,
    session_closed_tick_count: usize,
    open_session_buy_tick_count: usize,
    open_session_sell_tick_count: usize,
    open_signed_tick_count: usize,
}

pub fn preprocess_market_data_translation_request(
    request: MarketDataTranslationPreprocessingRequest,
    calendar_registry: &ExchangeCalendarRegistry,
) -> Result<MarketDataTranslationPreprocessingOutput, MarketDataTranslationPreprocessingError> {
    let (definition, timezone) =
        required_definition_and_timezone(calendar_registry, request.exchange_code.as_str())?;
    let tick_summary = preprocess_tick_records(
        request.tick_records.as_slice(),
        definition,
        timezone,
        request.exchange_code.as_str(),
    )?;
    let retained_known_news = resolve_known_news_records(
        request.known_news_records.as_slice(),
        definition,
        timezone,
        tick_summary.source_window.clone(),
    )?;

    let seasonality_stage_input = build_seasonality_stage_input(
        tick_summary.timestamp_event_records.clone(),
        retained_known_news.retained_records.as_slice(),
        request.trade_events_data_reference,
    );

    let hawkes_candidate_inputs = if request.require_hawkes {
        Some(build_hawkes_candidate_inputs(
            tick_summary.resolved_ticks.as_slice(),
            tick_summary.open_price_ticks.as_slice(),
            retained_known_news.retained_records.as_slice(),
            tick_summary.session_open_tick_count,
            tick_summary.session_closed_tick_count,
            tick_summary.open_session_buy_tick_count,
            tick_summary.open_session_sell_tick_count,
            request.directional_label_diagnostics,
        )?)
    } else {
        None
    };

    let (return_components_candidate_generation_input, material_close_open_gap_count) =
        if request.require_return_components {
            let output = build_return_components_candidate_generation_input(
                tick_summary.open_price_ticks.as_slice(),
                tick_summary.open_signed_tick_count,
                tick_summary.open_session_sell_tick_count,
                retained_known_news.retained_records.as_slice(),
                tick_summary.source_window.clone(),
                request.option_surface_quote_values.as_slice(),
            )?;
            (Some(output.0), output.1)
        } else {
            (None, 0)
        };

    Ok(MarketDataTranslationPreprocessingOutput {
        seasonality_stage_input,
        hawkes_candidate_inputs,
        return_components_candidate_generation_input,
        session_open_tick_count: tick_summary.session_open_tick_count,
        session_closed_tick_count: tick_summary.session_closed_tick_count,
        known_news_event_count: retained_known_news.retained_records.len(),
        known_news_events_outside_source_window_count: retained_known_news
            .outside_source_window_count,
        known_news_open_event_count: retained_known_news.open_event_count,
        known_news_closed_event_count: retained_known_news.closed_event_count,
        material_close_open_gap_count,
    })
}

struct KnownNewsResolutionSummary {
    retained_records: Vec<ResolvedKnownNewsRecord>,
    outside_source_window_count: usize,
    open_event_count: usize,
    closed_event_count: usize,
}

fn preprocess_tick_records(
    tick_records: &[MarketDataTranslationTickRecord],
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    exchange_code: &str,
) -> Result<TickPreprocessingSummary, MarketDataTranslationPreprocessingError> {
    let mut resolved_ticks: Vec<ResolvedTickRecord> = Vec::with_capacity(tick_records.len());
    let mut open_price_ticks: Vec<OpenTickRecord> = Vec::new();

    let mut timestamp_event_records: Vec<CalibrationTimestampEventRecord> =
        Vec::with_capacity(tick_records.len());

    let mut session_open_tick_count: usize = 0;
    let mut session_closed_tick_count: usize = 0;
    let mut open_session_buy_tick_count: usize = 0;
    let mut open_session_sell_tick_count: usize = 0;
    let mut open_signed_tick_count: usize = 0;

    for tick_record in tick_records {
        let timestamp_utc = datetime_from_nanos(
            tick_record.timestamp_utc_nanos,
            MarketDataTranslationPreprocessingError::TickTimestampOutOfRange {
                source_row_number: tick_record.source_row_number,
            },
        )?;

        let trade_sign = parse_trade_sign(tick_record.trade_sign.as_deref())?;

        let (market_state, session_window) = resolve_market_state_and_session_window(
            definition,
            timezone,
            exchange_code,
            timestamp_utc,
        )?;

        timestamp_event_records.push(CalibrationTimestampEventRecord {
            source_record_position: tick_record.source_row_number.saturating_sub(1) as u64,
            timestamp_utc_nanos: tick_record.timestamp_utc_nanos,
        });

        match market_state {
            MarketState::Open => {
                session_open_tick_count += 1;

                if let Some(sign) = trade_sign {
                    open_signed_tick_count += 1;
                    match sign {
                        TradeSign::Buy => open_session_buy_tick_count += 1,
                        TradeSign::Sell => open_session_sell_tick_count += 1,
                    }
                }

                if let Some(price) = tick_record.price {
                    if price <= 0.0 {
                        return Err(
                            MarketDataTranslationPreprocessingError::InvalidOpenSessionPrice {
                                source_row_number: tick_record.source_row_number,
                                price,
                            },
                        );
                    }
                    let Some(session_window) = session_window else {
                        return Err(
                            MarketDataTranslationPreprocessingError::InvalidSessionBoundary {
                                exchange_code: exchange_code.to_string(),
                                timestamp_utc_nanos: tick_record.timestamp_utc_nanos,
                            },
                        );
                    };
                    open_price_ticks.push(OpenTickRecord {
                        timestamp_utc_nanos: tick_record.timestamp_utc_nanos,
                        price,
                        session_open_utc_nanos: session_window.open_utc_nanos,
                        session_close_utc_nanos: session_window.close_utc_nanos,
                    });
                }
            }

            MarketState::Closed => {
                session_closed_tick_count += 1;
            }
        }

        resolved_ticks.push(ResolvedTickRecord {
            timestamp_utc_nanos: tick_record.timestamp_utc_nanos,
            trade_sign,
            source_row_number: tick_record.source_row_number,
        });
    }

    let source_window = build_source_window(tick_records);

    Ok(TickPreprocessingSummary {
        resolved_ticks,
        open_price_ticks,
        timestamp_event_records,
        source_window,
        session_open_tick_count,
        session_closed_tick_count,
        open_session_buy_tick_count,
        open_session_sell_tick_count,
        open_signed_tick_count,
    })
}

fn resolve_known_news_records(
    known_news_records: &[MarketDataTranslationKnownNewsRecord],
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    source_window: CalibrationReturnComponentsSignalSourceWindow,
) -> Result<KnownNewsResolutionSummary, MarketDataTranslationPreprocessingError> {
    let mut retained_records: Vec<ResolvedKnownNewsRecord> = Vec::new();
    let mut outside_source_window_count: usize = 0;
    let mut open_event_count: usize = 0;
    let mut closed_event_count: usize = 0;

    for (event_index, known_news_record) in known_news_records.iter().enumerate() {
        if known_news_record.event_timestamp_utc_nanos < source_window.window_start_utc_nanos
            || known_news_record.event_timestamp_utc_nanos >= source_window.window_end_utc_nanos
        {
            outside_source_window_count += 1;
            continue;
        }

        let timestamp_utc = datetime_from_nanos(
            known_news_record.event_timestamp_utc_nanos,
            MarketDataTranslationPreprocessingError::KnownNewsTimestampOutOfRange { event_index },
        )?;

        let (market_state, _) =
            resolve_market_state_and_session_window(definition, timezone, "", timestamp_utc)?;
        match market_state {
            MarketState::Open => open_event_count += 1,
            MarketState::Closed => closed_event_count += 1,
        }

        retained_records.push(ResolvedKnownNewsRecord {
            event_timestamp_utc_nanos: known_news_record.event_timestamp_utc_nanos,
            event_identifier: known_news_record.event_identifier.clone(),
            market_state,
        });
    }

    Ok(KnownNewsResolutionSummary {
        retained_records,
        outside_source_window_count,
        open_event_count,
        closed_event_count,
    })
}

fn build_seasonality_stage_input(
    timestamp_event_records: Vec<CalibrationTimestampEventRecord>,
    retained_known_news: &[ResolvedKnownNewsRecord],
    trade_events_data_reference: String,
) -> CalibrationSeasonalityStageExecutionInput {
    let scheduled_event_overlays = retained_known_news
        .iter()
        .enumerate()
        .map(
            |(position, known_news_record)| CalibrationScheduledEventOverlay {
                overlay_id: known_news_record
                    .event_identifier
                    .clone()
                    .unwrap_or_else(|| format!("known_news_{}", position + 1)),
                start_timestamp_utc_nanos: known_news_record.event_timestamp_utc_nanos,
                end_timestamp_utc_nanos: known_news_record.event_timestamp_utc_nanos,
                log_adjustment: 0.0,
                precedence: 0,
                enabled: true,
                source_label: "known_news_calendar".to_string(),
            },
        )
        .collect();

    CalibrationSeasonalityStageExecutionInput {
        timestamp_event_records,
        scheduled_event_overlays,
        settings: Some(CalibrationSeasonalityStageExecutionSettings {
            session_filter: CalibrationTimestampEventSessionFilter::OpenOnly,
            sparse_hour_event_threshold: 1,
            intraday_settings: CalibrationIntradaySeasonalityEstimatorSettings {
                minutes_per_bin: 1,
                smoothing_kernel: CalibrationIntradaySmoothingKernel::Gaussian,
                smoothing_bandwidth_bins: 3.0,
                minimum_bin_events: 1,
                pseudocount: 1.0e-6,
            },
            day_of_week_settings: CalibrationDayOfWeekSeasonalityEstimatorSettings {
                minimum_bucket_events: 1,
                pseudocount: 1.0e-6,
            },
            day_of_year_settings: CalibrationDayOfYearSeasonalityEstimatorSettings {
                minimum_bucket_events: 1,
                pseudocount: 1.0e-6,
            },
        }),
        dataset_id_override: Some(trade_events_data_reference),
        estimator_version_override: None,
    }
}

fn build_hawkes_candidate_inputs(
    resolved_ticks: &[ResolvedTickRecord],
    open_price_ticks: &[OpenTickRecord],
    retained_known_news: &[ResolvedKnownNewsRecord],
    session_open_tick_count: usize,
    session_closed_tick_count: usize,
    open_session_buy_tick_count: usize,
    open_session_sell_tick_count: usize,
    directional_label_diagnostics: Option<CalibrationHawkesDirectionalLabelDiagnostics>,
) -> Result<Vec<CalibrationHawkesStageCandidateInput>, MarketDataTranslationPreprocessingError> {
    if let Some(missing_sign_tick) = resolved_ticks
        .iter()
        .find(|resolved_tick| resolved_tick.trade_sign.is_none())
    {
        return Err(
            MarketDataTranslationPreprocessingError::HawkesRequiresResolvedTradeSigns {
                source_row_number: missing_sign_tick.source_row_number,
            },
        );
    }

    let directional_label_diagnostics = directional_label_diagnostics.ok_or(
        MarketDataTranslationPreprocessingError::HawkesRequiresDirectionalLabelDiagnostics,
    )?;

    let tick_count = resolved_ticks.len();

    let observation_duration_hours = observation_window_hours(
        resolved_ticks.first().map(|tick| tick.timestamp_utc_nanos),
        resolved_ticks.last().map(|tick| tick.timestamp_utc_nanos),
    );
    let (open_session_duration_hours, open_interarrival_seconds) =
        open_session_transition_metrics(open_price_ticks);

    let mean_interarrival_seconds =
        arithmetic_mean(open_interarrival_seconds.as_slice()).unwrap_or(60.0);

    let interarrival_coefficient_of_variation =
        coefficient_of_variation(open_interarrival_seconds.as_slice());

    let short_gap_threshold_seconds = (mean_interarrival_seconds * 0.5).clamp(1.0, 300.0);
    let short_gap_ratio = if open_interarrival_seconds.is_empty() {
        0.0
    } else {
        open_interarrival_seconds
            .iter()
            .filter(|duration_seconds| **duration_seconds <= short_gap_threshold_seconds)
            .count() as f64
            / open_interarrival_seconds.len() as f64
    };

    let open_known_news_timestamps: Vec<i64> = retained_known_news
        .iter()
        .filter(|record| record.market_state == MarketState::Open)
        .map(|record| record.event_timestamp_utc_nanos)
        .collect();

    let open_known_news_activity_share = open_session_known_news_activity_share(
        open_price_ticks,
        open_known_news_timestamps.as_slice(),
    );

    let source_regime = CalibrationHawkesObservedSourceRegime {
        observation_duration_hours: round_to(observation_duration_hours, 6),
        open_session_duration_hours: round_to(open_session_duration_hours, 6),
        total_tick_count: tick_count,
        open_session_tick_count: session_open_tick_count,
        closed_session_tick_count: session_closed_tick_count,
        open_session_buy_tick_count,
        open_session_sell_tick_count,
        open_session_mean_interarrival_seconds: round_to(mean_interarrival_seconds.max(1.0e-6), 6),
        open_session_interarrival_coefficient_of_variation: round_to(
            interarrival_coefficient_of_variation,
            6,
        ),
        open_session_short_gap_threshold_seconds: round_to(short_gap_threshold_seconds, 6),
        open_session_short_gap_ratio: round_to(short_gap_ratio, 6),
        known_news_open_event_count: open_known_news_timestamps.len(),
        open_session_known_news_activity_share: round_to(open_known_news_activity_share, 6),
    };

    let open_session_rate_per_hour =
        session_open_tick_count as f64 / open_session_duration_hours.max(1.0e-9);
    let open_session_rate_per_second = open_session_rate_per_hour / 3600.0;
    let poisson_short_gap_ratio =
        1.0 - f64::exp(-open_session_rate_per_second * short_gap_threshold_seconds);
    let clustering_excess = (short_gap_ratio - poisson_short_gap_ratio).max(0.0);
    let clustering_center = clamp(
        0.15 + 0.45 * clustering_excess
            + 0.20 * (interarrival_coefficient_of_variation - 1.0).max(0.0),
        0.05,
        0.78,
    );
    let beta_center = clamp(60.0 / mean_interarrival_seconds.max(1.0), 0.05, 6.0);
    let open_sell_share =
        open_session_sell_tick_count as f64 / session_open_tick_count.max(1) as f64;
    let asymmetry_fraction = clamp((open_sell_share - 0.5).abs() * 1.6, 0.0, 0.35);
    let news_rate_per_hour =
        open_known_news_timestamps.len() as f64 / open_session_duration_hours.max(1.0e-9);

    let branching_values = unique_sorted_rounded(&[
        0.08,
        0.14,
        0.20,
        0.28,
        clamp(clustering_center - 0.20, 0.05, 0.82),
        clamp(clustering_center - 0.12, 0.05, 0.82),
        clustering_center,
        clamp(clustering_center + 0.12, 0.05, 0.82),
    ]);
    let beta_values = unique_sorted_rounded(&[
        clamp(beta_center * 0.75, 0.05, 8.0),
        beta_center,
        clamp(beta_center * 1.35, 0.05, 8.0),
    ]);

    let cross_shares = [0.10, 0.22];
    let lambda_scales = [0.90, 1.00, 1.10];
    let rho_scales = [0.50, 1.00, 1.50];

    let mut candidate_inputs: Vec<CalibrationHawkesStageCandidateInput> = Vec::new();
    let mut candidate_index: usize = 0;
    for branching_ratio in branching_values {
        for beta in beta_values.iter().copied() {
            for cross_share in cross_shares {
                let structured_alpha = build_structured_alpha_candidate(
                    branching_ratio,
                    beta,
                    cross_share,
                    asymmetry_fraction,
                );
                let matched_lambda_bar = matched_lambda_bar_for_observed_open_rate(
                    open_session_rate_per_hour,
                    beta,
                    &structured_alpha,
                );

                for lambda_scale in lambda_scales {
                    let lambda_bar = (matched_lambda_bar * lambda_scale).max(1.0e-4);
                    for rho_scale in rho_scales {
                        let structured_rho = build_structured_rho_candidate(
                            beta,
                            (cross_share + 0.04).min(0.35),
                            (asymmetry_fraction + 0.05).min(0.40),
                            open_session_rate_per_hour,
                            news_rate_per_hour,
                            open_known_news_activity_share,
                            rho_scale,
                        );

                        candidate_inputs.push(CalibrationHawkesStageCandidateInput {
                            candidate_id: format!("market_data_fit_{candidate_index:03}"),
                            lambda_bar: round_to(lambda_bar, 6),
                            beta: round_to(beta, 6),
                            structured_alpha,
                            structured_rho,
                            self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy {
                                minimum_self_to_cross_ratio: 1.0,
                            }),
                            stability_numeric_tolerance: 0.0,
                            source_regime: source_regime.clone(),
                            directional_label_diagnostics: directional_label_diagnostics.clone(),
                        });

                        candidate_index += 1;
                    }
                }
            }
        }
    }

    Ok(candidate_inputs)
}

fn build_return_components_candidate_generation_input(
    open_price_ticks: &[OpenTickRecord],
    open_signed_tick_count: usize,
    open_session_sell_tick_count: usize,
    retained_known_news: &[ResolvedKnownNewsRecord],
    source_window: CalibrationReturnComponentsSignalSourceWindow,
    _option_surface_quote_values: &[f64],
) -> Result<
    (
        CalibrationReturnComponentsStageCandidateGenerationInput,
        usize,
    ),
    MarketDataTranslationPreprocessingError,
> {
    if open_price_ticks.len() < 2 {
        return Err(MarketDataTranslationPreprocessingError::MissingOpenSessionPriceObservations);
    }

    let open_price_tick_timestamps: Vec<i64> = open_price_ticks
        .iter()
        .map(|tick| tick.timestamp_utc_nanos)
        .collect();
    let same_session_returns = same_session_open_returns(open_price_ticks);
    let observed_open_session_returns = derive_observed_open_session_returns(
        same_session_returns.as_slice(),
        open_price_tick_timestamps.as_slice(),
    );
    let close_open_boundary_observations = derive_close_open_boundary_observations(
        open_price_ticks,
        open_price_tick_timestamps.as_slice(),
    );

    let known_news_return_samples = derive_known_news_return_samples(
        retained_known_news,
        open_price_ticks,
        close_open_boundary_observations.as_slice(),
        open_price_tick_timestamps.as_slice(),
    );
    let known_news_timestamps: Vec<i64> = retained_known_news
        .iter()
        .map(|record| record.event_timestamp_utc_nanos)
        .collect();

    let intraday_jump_candidates = derive_intraday_jump_candidates(
        same_session_returns.as_slice(),
        open_price_tick_timestamps.as_slice(),
        known_news_timestamps.as_slice(),
    );

    let open_tick_sell_share = if open_signed_tick_count == 0 {
        0.5
    } else {
        open_session_sell_tick_count as f64 / open_signed_tick_count as f64
    };

    let intraday_return_values: Vec<f64> = same_session_returns
        .iter()
        .map(|(_, return_value)| *return_value)
        .collect();

    let median_abs_intraday_return = median_abs(intraday_return_values.as_slice());

    let gap_tolerance = 0.0025f64.max(median_abs_intraday_return * 3.0);
    let material_close_open_gap_count = close_open_boundary_observations
        .iter()
        .filter(|observation| {
            f64::abs(f64::ln(observation.open_price / observation.close_price)) > gap_tolerance
        })
        .count();

    let candidate_generation_input = CalibrationReturnComponentsStageCandidateGenerationInput {
        known_news_calendar: CalibrationReturnComponentsKnownNewsCalendarInput {
            source_window,
            scheduled_event_timestamps_utc_nanos: known_news_timestamps,
        },
        realized_known_news_return_samples: known_news_return_samples,
        observed_open_session_returns,
        detected_intraday_jump_candidates: intraday_jump_candidates,
        observed_close_open_boundary_observations: close_open_boundary_observations.clone(),
        close_open_gap_detection_policy: CalibrationReturnComponentsCloseOpenGapDetectionPolicy {
            material_gap_absolute_return_tolerance: round_to(gap_tolerance, 6),
        },
        open_session_sell_share_proxy: round_to(open_tick_sell_share, 6),
        shared_mark_law_template: derive_shared_mark_law_template(
            intraday_return_values.as_slice(),
            close_open_boundary_observations.as_slice(),
        ),
        objective_metric_weights: CalibrationReturnComponentsObjectiveMetricWeights {
            continuous_distribution_fit_weight: 1.0,
            jump_fit_weight: 1.0,
            gap_fit_weight: 1.0,
            activity_coupling_fit_weight: 1.0,
            price_range_sanity_fit_weight: 1.0,
        },
    };

    Ok((candidate_generation_input, material_close_open_gap_count))
}

fn derive_close_open_boundary_observations(
    open_price_ticks: &[OpenTickRecord],
    open_price_tick_timestamps: &[i64],
) -> Vec<CalibrationReturnComponentsObservedCloseOpenBoundaryObservation> {
    let mut observations_by_boundary: BTreeMap<
        (i64, i64),
        CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    > = BTreeMap::new();

    for (previous_tick, current_tick) in
        open_price_ticks.iter().zip(open_price_ticks.iter().skip(1))
    {
        if previous_tick.session_close_utc_nanos == current_tick.session_close_utc_nanos
            && previous_tick.session_open_utc_nanos == current_tick.session_open_utc_nanos
        {
            continue;
        }

        let close_timestamp_utc_nanos = previous_tick.session_close_utc_nanos;
        let open_timestamp_utc_nanos = current_tick.session_open_utc_nanos;
        if open_timestamp_utc_nanos <= close_timestamp_utc_nanos {
            continue;
        }

        observations_by_boundary.insert(
            (close_timestamp_utc_nanos, open_timestamp_utc_nanos),
            CalibrationReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos,
                close_price: previous_tick.price,
                open_timestamp_utc_nanos,
                open_price: current_tick.price,
                post_shock_activity_lift: activity_lift(
                    open_timestamp_utc_nanos,
                    open_price_tick_timestamps,
                ),
            },
        );
    }

    observations_by_boundary.into_values().collect()
}

fn derive_known_news_return_samples(
    retained_known_news: &[ResolvedKnownNewsRecord],
    open_price_ticks: &[OpenTickRecord],
    close_open_boundary_observations: &[CalibrationReturnComponentsObservedCloseOpenBoundaryObservation],
    open_price_tick_timestamps: &[i64],
) -> Vec<CalibrationReturnComponentsKnownNewsReturnSample> {
    let mut return_samples: Vec<CalibrationReturnComponentsKnownNewsReturnSample> = Vec::new();

    for known_news_record in retained_known_news {
        match known_news_record.market_state {
            MarketState::Open => {
                let Some(before_index) = open_price_tick_timestamps
                    .partition_point_index(known_news_record.event_timestamp_utc_nanos, true)
                else {
                    continue;
                };

                let after_index = open_price_tick_timestamps.partition_point(|timestamp| {
                    *timestamp <= known_news_record.event_timestamp_utc_nanos
                });
                if after_index >= open_price_ticks.len() {
                    continue;
                }

                let before_tick = &open_price_ticks[before_index];
                let after_tick = &open_price_ticks[after_index];

                return_samples.push(CalibrationReturnComponentsKnownNewsReturnSample {
                    event_timestamp_utc_nanos: known_news_record.event_timestamp_utc_nanos,
                    open_window_jump_return: f64::ln(after_tick.price / before_tick.price),
                    closed_window_gap_return: 0.0,
                    post_shock_activity_lift: activity_lift(
                        known_news_record.event_timestamp_utc_nanos,
                        open_price_tick_timestamps,
                    ),
                });
            }
            MarketState::Closed => {
                if let Some(boundary_observation) = boundary_observation_for_event(
                    close_open_boundary_observations,
                    known_news_record.event_timestamp_utc_nanos,
                ) {
                    return_samples.push(CalibrationReturnComponentsKnownNewsReturnSample {
                        event_timestamp_utc_nanos: known_news_record.event_timestamp_utc_nanos,
                        open_window_jump_return: 0.0,
                        closed_window_gap_return: f64::ln(
                            boundary_observation.open_price / boundary_observation.close_price,
                        ),
                        post_shock_activity_lift: boundary_observation.post_shock_activity_lift,
                    });
                }
            }
        }
    }

    return_samples
}

fn derive_observed_open_session_returns(
    same_session_returns: &[(i64, f64)],
    open_price_tick_timestamps: &[i64],
) -> Vec<CalibrationReturnComponentsObservedOpenSessionReturn> {
    same_session_returns
        .iter()
        .map(|(return_timestamp_utc_nanos, log_return)| {
            CalibrationReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: *return_timestamp_utc_nanos,
                log_return: *log_return,
                post_shock_activity_lift: activity_lift(
                    *return_timestamp_utc_nanos,
                    open_price_tick_timestamps,
                ),
            }
        })
        .collect()
}

fn derive_intraday_jump_candidates(
    same_session_returns: &[(i64, f64)],
    open_price_tick_timestamps: &[i64],
    known_news_timestamps: &[i64],
) -> Vec<CalibrationReturnComponentsDetectedIntradayJumpCandidate> {
    let candidate_returns: Vec<(i64, f64)> = same_session_returns
        .iter()
        .copied()
        .filter(|(candidate_timestamp_utc_nanos, _)| {
            !is_within_known_news_exclusion_window(
                *candidate_timestamp_utc_nanos,
                known_news_timestamps,
            )
        })
        .collect();
    if candidate_returns.is_empty() {
        return Vec::new();
    }

    let threshold = 0.005f64.max(
        median_abs(
            candidate_returns
                .iter()
                .map(|(_, jump_return)| *jump_return)
                .collect::<Vec<_>>()
                .as_slice(),
        ) * 4.0,
    );
    let material_candidates: Vec<(i64, f64)> = candidate_returns
        .iter()
        .copied()
        .filter(|(_, jump_return)| f64::abs(*jump_return) > threshold)
        .collect();
    if material_candidates.len() < 2 {
        return Vec::new();
    }

    material_candidates
        .into_iter()
        .map(|(candidate_timestamp_utc_nanos, jump_return)| {
            CalibrationReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: candidate_timestamp_utc_nanos,
                jump_return,
                post_shock_activity_lift: activity_lift(
                    candidate_timestamp_utc_nanos,
                    open_price_tick_timestamps,
                ),
            }
        })
        .collect()
}

fn same_session_open_returns(open_price_ticks: &[OpenTickRecord]) -> Vec<(i64, f64)> {
    let mut same_session_returns: Vec<(i64, f64)> = Vec::new();
    for (previous_tick, current_tick) in
        open_price_ticks.iter().zip(open_price_ticks.iter().skip(1))
    {
        if previous_tick.session_close_utc_nanos != current_tick.session_close_utc_nanos
            || previous_tick.session_open_utc_nanos != current_tick.session_open_utc_nanos
        {
            continue;
        }

        same_session_returns.push((
            current_tick.timestamp_utc_nanos,
            f64::ln(current_tick.price / previous_tick.price),
        ));
    }

    same_session_returns
}

fn derive_shared_mark_law_template(
    intraday_return_values: &[f64],
    close_open_boundary_observations: &[CalibrationReturnComponentsObservedCloseOpenBoundaryObservation],
) -> CalibrationSharedNewsMarkLawConfiguration {
    let mean_abs_open_return = arithmetic_mean(
        intraday_return_values
            .iter()
            .map(|return_value| return_value.abs())
            .collect::<Vec<_>>()
            .as_slice(),
    )
    .unwrap_or(0.01);

    let mean_abs_gap_return = arithmetic_mean(
        close_open_boundary_observations
            .iter()
            .map(|observation| f64::abs(f64::ln(observation.open_price / observation.close_price)))
            .collect::<Vec<_>>()
            .as_slice(),
    )
    .unwrap_or(mean_abs_open_return);

    CalibrationSharedNewsMarkLawConfiguration {
        gamma_severity_marks: GammaSeverityMarkConfiguration {
            shape_k: 2.0,
            scale_theta: round_to((mean_abs_open_return * 20.0).max(0.5), 6),
        },
        double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
            probability_positive: 0.45,
            eta_plus: 8.0,
            eta_minus: 10.0,
        },
        coupled_gap_marks: CoupledGapMarkConfiguration {
            sigma_g: round_to((mean_abs_gap_return * 10.0).max(0.1), 6),
        },
    }
}

fn open_session_transition_metrics(open_price_ticks: &[OpenTickRecord]) -> (f64, Vec<f64>) {
    if open_price_ticks.len() < 2 {
        return (MINIMUM_OPEN_TICK_COUNT_DURATION_HOURS, Vec::new());
    }

    let mut cumulative_seconds = 0.0;
    let mut same_session_gaps: Vec<f64> = Vec::new();
    for (previous_tick, current_tick) in
        open_price_ticks.iter().zip(open_price_ticks.iter().skip(1))
    {
        if previous_tick.session_close_utc_nanos == current_tick.session_close_utc_nanos
            && previous_tick.session_open_utc_nanos == current_tick.session_open_utc_nanos
        {
            let gap_seconds = (current_tick.timestamp_utc_nanos - previous_tick.timestamp_utc_nanos)
                as f64
                / 1_000_000_000.0;
            let clamped_gap_seconds = gap_seconds.max(0.0);

            cumulative_seconds += clamped_gap_seconds;
            same_session_gaps.push(clamped_gap_seconds);
        } else {
            let boundary_seconds = (previous_tick.session_close_utc_nanos
                - previous_tick.timestamp_utc_nanos) as f64
                / 1_000_000_000.0;

            cumulative_seconds += boundary_seconds.max(0.0);
        }
    }

    let end_padding_seconds = arithmetic_mean(same_session_gaps.as_slice()).unwrap_or(60.0);
    cumulative_seconds += end_padding_seconds.max(60.0);
    (
        (cumulative_seconds / 3600.0).max(MINIMUM_OPEN_TICK_COUNT_DURATION_HOURS),
        same_session_gaps,
    )
}

fn open_session_known_news_activity_share(
    open_price_ticks: &[OpenTickRecord],
    open_known_news_timestamps: &[i64],
) -> f64 {
    if open_price_ticks.is_empty() || open_known_news_timestamps.is_empty() {
        return 0.0;
    }

    let window_nanos = Duration::minutes(ACTIVITY_LIFT_WINDOW_MINUTES)
        .num_nanoseconds()
        .expect("activity-lift window must fit in i64");

    let mut right_index: usize = 0;
    let mut left_index: usize = 0;
    let mut attributable_tick_count: usize = 0;

    for open_tick in open_price_ticks {
        while right_index < open_known_news_timestamps.len()
            && open_known_news_timestamps[right_index] <= open_tick.timestamp_utc_nanos
        {
            right_index += 1;
        }

        while left_index < right_index
            && open_known_news_timestamps[left_index] < open_tick.timestamp_utc_nanos - window_nanos
        {
            left_index += 1;
        }

        if right_index > left_index {
            attributable_tick_count += 1;
        }
    }

    attributable_tick_count as f64 / open_price_ticks.len() as f64
}

fn activity_lift(anchor_timestamp_utc_nanos: i64, open_price_tick_timestamps: &[i64]) -> f64 {
    let window_nanos = Duration::minutes(ACTIVITY_LIFT_WINDOW_MINUTES)
        .num_nanoseconds()
        .expect("activity-lift window must fit in i64");

    let pre_window_start = anchor_timestamp_utc_nanos - window_nanos;
    let post_window_end = anchor_timestamp_utc_nanos + window_nanos;

    let pre_event_count = count_in_half_open_interval(
        open_price_tick_timestamps,
        pre_window_start,
        anchor_timestamp_utc_nanos,
    );
    let post_event_count = count_in_half_open_interval(
        open_price_tick_timestamps,
        anchor_timestamp_utc_nanos,
        post_window_end,
    );

    if pre_event_count == 0 {
        return 1.0;
    }

    round_to(post_event_count as f64 / pre_event_count as f64, 6)
}

fn is_within_known_news_exclusion_window(
    candidate_timestamp_utc_nanos: i64,
    known_news_timestamps: &[i64],
) -> bool {
    if known_news_timestamps.is_empty() {
        return false;
    }

    let window_nanos = Duration::minutes(KNOWN_NEWS_EXCLUSION_WINDOW_MINUTES)
        .num_nanoseconds()
        .expect("known-news exclusion window must fit in i64");

    let lower_bound = candidate_timestamp_utc_nanos - window_nanos;
    let upper_bound = candidate_timestamp_utc_nanos + window_nanos;

    let lower_index = known_news_timestamps.partition_point(|timestamp| *timestamp < lower_bound);
    lower_index < known_news_timestamps.len() && known_news_timestamps[lower_index] <= upper_bound
}

fn boundary_observation_for_event(
    close_open_boundary_observations: &[CalibrationReturnComponentsObservedCloseOpenBoundaryObservation],
    event_timestamp_utc_nanos: i64,
) -> Option<&CalibrationReturnComponentsObservedCloseOpenBoundaryObservation> {
    close_open_boundary_observations
        .iter()
        .find(|boundary_observation| {
            boundary_observation.close_timestamp_utc_nanos <= event_timestamp_utc_nanos
                && event_timestamp_utc_nanos < boundary_observation.open_timestamp_utc_nanos
        })
}

fn build_structured_alpha_candidate(
    branching_ratio: f64,
    beta: f64,
    cross_share: f64,
    asymmetry_fraction: f64,
) -> HawkesStructuredAlphaParameterization {
    let row_sum_center = branching_ratio * beta;
    let asymmetry_mass = row_sum_center * asymmetry_fraction;
    let buy_row_sum = (row_sum_center - asymmetry_mass / 2.0).max(row_sum_center * 0.45);

    HawkesStructuredAlphaParameterization {
        a_self: round_to((buy_row_sum * (1.0 - cross_share)).max(1.0e-4), 6),
        a_cross: round_to((buy_row_sum * cross_share).max(0.0), 6),
        delta_self: round_to((asymmetry_mass * (1.0 - cross_share)).max(0.0), 6),
        delta_cross: round_to((asymmetry_mass * cross_share).max(0.0), 6),
    }
}

fn build_structured_rho_candidate(
    beta: f64,
    cross_share: f64,
    asymmetry_fraction: f64,
    open_session_rate_per_hour: f64,
    news_rate_per_hour: f64,
    known_news_activity_share: f64,
    rho_scale: f64,
) -> HawkesStructuredRhoParameterization {
    let base_row_sum = if news_rate_per_hour <= 0.0 || known_news_activity_share <= 0.0 {
        0.02 * rho_scale
    } else {
        let target_total_rho_mass =
            2.0 * beta * open_session_rate_per_hour * known_news_activity_share
                / news_rate_per_hour.max(1.0e-9);
        clamp(target_total_rho_mass / 2.0, 0.02, 2.50) * rho_scale
    };

    let asymmetry_mass = base_row_sum * asymmetry_fraction.min(0.5);
    let buy_row_sum = (base_row_sum - asymmetry_mass / 2.0).max(base_row_sum * 0.45);

    HawkesStructuredRhoParameterization {
        r_self: round_to((buy_row_sum * (1.0 - cross_share)).max(1.0e-4), 6),
        r_cross: round_to((buy_row_sum * cross_share).max(0.0), 6),
        delta_self_r: round_to((asymmetry_mass * (1.0 - cross_share)).max(0.0), 6),
        delta_cross_r: round_to((asymmetry_mass * cross_share).max(0.0), 6),
    }
}

fn matched_lambda_bar_for_observed_open_rate(
    open_session_rate_per_hour: f64,
    beta: f64,
    structured_alpha: &HawkesStructuredAlphaParameterization,
) -> f64 {
    let alpha_plus_plus = structured_alpha.a_self;
    let alpha_plus_minus = structured_alpha.a_cross;
    let alpha_minus_plus = structured_alpha.a_cross + structured_alpha.delta_cross;
    let alpha_minus_minus = structured_alpha.a_self + structured_alpha.delta_self;

    let scaled_plus_plus = alpha_plus_plus / beta;
    let scaled_plus_minus = alpha_plus_minus / beta;
    let scaled_minus_plus = alpha_minus_plus / beta;
    let scaled_minus_minus = alpha_minus_minus / beta;

    let determinant = (1.0 - scaled_plus_plus) * (1.0 - scaled_minus_minus)
        - scaled_plus_minus * scaled_minus_plus;
    if determinant <= 1.0e-9 {
        return (open_session_rate_per_hour / 2.0).max(1.0e-4);
    }

    let inverse_sum = ((1.0 - scaled_minus_minus)
        + scaled_plus_minus
        + scaled_minus_plus
        + (1.0 - scaled_plus_plus))
        / determinant;
    if inverse_sum <= 1.0e-9 {
        return (open_session_rate_per_hour / 2.0).max(1.0e-4);
    }

    (open_session_rate_per_hour / inverse_sum).max(1.0e-4)
}

fn required_definition_and_timezone<'a>(
    calendar_registry: &'a ExchangeCalendarRegistry,
    exchange_code: &str,
) -> Result<(&'a ExchangeCalendarDefinition, Tz), MarketDataTranslationPreprocessingError> {
    let definition =
        calendar_registry
            .get_required(exchange_code)
            .map_err(|error| match error {
                ExchangeCalendarRegistryLookupError::MissingExchangeId { exchange_code } => {
                    MarketDataTranslationPreprocessingError::MissingExchangeId { exchange_code }
                }
            })?;

    let timezone_name = definition.timezone_name();
    let timezone = Tz::from_str(timezone_name).map_err(|_| {
        MarketDataTranslationPreprocessingError::InvalidTimezoneName {
            exchange_code: exchange_code.to_string(),
            timezone_name: timezone_name.to_string(),
        }
    })?;

    Ok((definition, timezone))
}

fn resolve_market_state_and_session_window(
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    exchange_code: &str,
    timestamp_utc: DateTime<Utc>,
) -> Result<(MarketState, Option<SessionWindowUtc>), MarketDataTranslationPreprocessingError> {
    let timestamp_local = timestamp_utc.with_timezone(&timezone);
    let local_date = CalendarDate::from_local_timestamp(&timestamp_local);
    let local_weekday = TradingWeekday::from_chrono_weekday(timestamp_local.weekday());
    let local_hour = timestamp_local.hour() as u8;
    let local_minute = timestamp_local.minute() as u8;
    let local_clock = SessionTime::from_hour_minute(local_hour, local_minute);

    if definition.holiday_dates.contains(&local_date) {
        return Ok((MarketState::Closed, None));
    }

    let session_windows = if let Some(early_close_windows) =
        definition.early_close_sessions_by_date.get(&local_date)
    {
        early_close_windows.as_ref()
    } else if let Some(regular_session) = definition
        .regular_sessions
        .iter()
        .find(|regular_session| regular_session.weekday == local_weekday)
    {
        regular_session.windows()
    } else {
        &[]
    };

    let Some(session_window) = session_windows
        .iter()
        .find(|session_window| session_window.contains_local_clock(local_clock))
    else {
        return Ok((MarketState::Closed, None));
    };

    Ok((
        MarketState::Open,
        Some(build_session_window_utc(
            timezone,
            exchange_code,
            local_date,
            session_window,
        )?),
    ))
}

fn build_session_window_utc(
    timezone: Tz,
    exchange_code: &str,
    local_date: CalendarDate,
    session_window: &SessionWindowDefinition,
) -> Result<SessionWindowUtc, MarketDataTranslationPreprocessingError> {
    let open_local = local_datetime_for_session_time(
        timezone,
        exchange_code,
        local_date,
        session_window.open_time().to_string().as_str(),
        session_window.open_time().hour(),
        session_window.open_time().minute(),
    )?;
    let close_local = local_datetime_for_session_time(
        timezone,
        exchange_code,
        local_date,
        session_window.close_time().to_string().as_str(),
        session_window.close_time().hour(),
        session_window.close_time().minute(),
    )?;

    Ok(SessionWindowUtc {
        open_utc_nanos: open_local
            .with_timezone(&Utc)
            .timestamp_nanos_opt()
            .expect("session open boundary must fit in i64 nanos"),
        close_utc_nanos: close_local
            .with_timezone(&Utc)
            .timestamp_nanos_opt()
            .expect("session close boundary must fit in i64 nanos"),
    })
}

fn local_datetime_for_session_time(
    timezone: Tz,
    exchange_code: &str,
    local_date: CalendarDate,
    local_time_label: &str,
    hour: u8,
    minute: u8,
) -> Result<DateTime<Tz>, MarketDataTranslationPreprocessingError> {
    let (resolved_year, resolved_month, resolved_day, resolved_hour, resolved_minute) =
        if hour == 24 && minute == 0 {
            let next_day = NaiveDate::from_ymd_opt(
                local_date.year,
                local_date.month as u32,
                local_date.day as u32,
            )
            .and_then(|date| date.succ_opt())
            .ok_or_else(|| {
                MarketDataTranslationPreprocessingError::AmbiguousSessionBoundary {
                    exchange_code: exchange_code.to_string(),
                    timezone_name: timezone.name().to_string(),
                    local_date: local_date.to_string(),
                    local_time: local_time_label.to_string(),
                }
            })?;

            (
                next_day.year(),
                next_day.month(),
                next_day.day(),
                0_u32,
                0_u32,
            )
        } else {
            (
                local_date.year,
                local_date.month as u32,
                local_date.day as u32,
                hour as u32,
                minute as u32,
            )
        };

    match timezone.with_ymd_and_hms(
        resolved_year,
        resolved_month,
        resolved_day,
        resolved_hour,
        resolved_minute,
        0,
    ) {
        LocalResult::Single(timestamp_local) => Ok(timestamp_local),
        LocalResult::Ambiguous(_, _) | LocalResult::None => Err(
            MarketDataTranslationPreprocessingError::AmbiguousSessionBoundary {
                exchange_code: exchange_code.to_string(),
                timezone_name: timezone.name().to_string(),
                local_date: local_date.to_string(),
                local_time: local_time_label.to_string(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{
        OpenTickRecord, derive_close_open_boundary_observations,
        resolve_market_state_and_session_window,
    };
    use crate::calendar::{
        CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, MarketState,
        ExchangeCalendarDefinition,
        RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
    };

    #[test]
    fn full_day_session_window_uses_next_day_midnight_for_24_00_close() {
        let definition = ExchangeCalendarDefinition::from_artifact(ExchangeCalendarArtifact {
            schema_version: "calendar.v1".to_string(),
            exchange_code: "FXSPOT".to_string(),
            exchange_name: "OTC FX Spot".to_string(),
            timezone_name: Some("UTC".to_string()),
            rule_metadata: CalendarRuleMetadataArtifact {
                holiday_calendar_code: "FX_24_5".to_string(),
                early_close_rule_set: None,
            },
            regular_sessions: vec![RegularSessionArtifact {
                weekday: TradingWeekday::Tuesday,
                windows: vec![SessionWindowArtifact {
                    label: "regular".to_string(),
                    open_time: "00:00".to_string(),
                    close_time: "24:00".to_string(),
                }],
            }],
            holiday_dates: vec![],
            early_close_sessions: vec![],
        })
        .expect("fixture calendar should be valid");

        let timestamp_utc = chrono_tz::UTC
            .with_ymd_and_hms(2026, 2, 3, 12, 0, 0)
            .single()
            .expect("fixture timestamp should be valid")
            .with_timezone(&Utc);

        let (market_state, session_window) = resolve_market_state_and_session_window(
            &definition,
            chrono_tz::UTC,
            "FXSPOT",
            timestamp_utc,
        )
        .expect("24:00 close boundary should resolve through preprocessing");

        let session_window = session_window.expect("open timestamp should resolve a session window");
        assert_eq!(market_state, MarketState::Open);
        assert_eq!(
            session_window.close_utc_nanos,
            chrono_tz::UTC
                .with_ymd_and_hms(2026, 2, 4, 0, 0, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
        );
    }

    #[test]
    fn full_day_adjacent_sessions_do_not_emit_zero_width_close_open_boundaries() {
        let open_price_ticks = vec![
            OpenTickRecord {
                timestamp_utc_nanos: chrono_tz::UTC
                    .with_ymd_and_hms(2026, 2, 3, 23, 30, 0)
                    .single()
                    .expect("fixture timestamp should be valid")
                    .with_timezone(&Utc)
                    .timestamp_nanos_opt()
                    .expect("fixture timestamp should fit in nanos"),
                price: 1.08,
                session_open_utc_nanos: chrono_tz::UTC
                    .with_ymd_and_hms(2026, 2, 3, 0, 0, 0)
                    .single()
                    .expect("fixture timestamp should be valid")
                    .with_timezone(&Utc)
                    .timestamp_nanos_opt()
                    .expect("fixture timestamp should fit in nanos"),
                session_close_utc_nanos: chrono_tz::UTC
                    .with_ymd_and_hms(2026, 2, 4, 0, 0, 0)
                    .single()
                    .expect("fixture timestamp should be valid")
                    .with_timezone(&Utc)
                    .timestamp_nanos_opt()
                    .expect("fixture timestamp should fit in nanos"),
            },
            OpenTickRecord {
                timestamp_utc_nanos: chrono_tz::UTC
                    .with_ymd_and_hms(2026, 2, 4, 0, 30, 0)
                    .single()
                    .expect("fixture timestamp should be valid")
                    .with_timezone(&Utc)
                    .timestamp_nanos_opt()
                    .expect("fixture timestamp should fit in nanos"),
                price: 1.081,
                session_open_utc_nanos: chrono_tz::UTC
                    .with_ymd_and_hms(2026, 2, 4, 0, 0, 0)
                    .single()
                    .expect("fixture timestamp should be valid")
                    .with_timezone(&Utc)
                    .timestamp_nanos_opt()
                    .expect("fixture timestamp should fit in nanos"),
                session_close_utc_nanos: chrono_tz::UTC
                    .with_ymd_and_hms(2026, 2, 5, 0, 0, 0)
                    .single()
                    .expect("fixture timestamp should be valid")
                    .with_timezone(&Utc)
                    .timestamp_nanos_opt()
                    .expect("fixture timestamp should fit in nanos"),
            },
        ];

        let observations = derive_close_open_boundary_observations(
            open_price_ticks.as_slice(),
            &open_price_ticks
                .iter()
                .map(|tick| tick.timestamp_utc_nanos)
                .collect::<Vec<_>>(),
        );

        assert!(
            observations.is_empty(),
            "adjacent full-day sessions should not emit zero-width close-open boundaries"
        );
    }
}

fn datetime_from_nanos<E>(timestamp_utc_nanos: i64, error: E) -> Result<DateTime<Utc>, E> {
    let seconds = timestamp_utc_nanos.div_euclid(1_000_000_000);
    let nanoseconds = timestamp_utc_nanos.rem_euclid(1_000_000_000) as u32;

    DateTime::from_timestamp(seconds, nanoseconds).ok_or(error)
}

fn parse_trade_sign(
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

fn build_source_window(
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

fn observation_window_hours(
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

pub(crate) fn arithmetic_mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    Some(values.iter().sum::<f64>() / values.len() as f64)
}

pub(crate) fn coefficient_of_variation(values: &[f64]) -> f64 {
    let Some(mean_value) = arithmetic_mean(values) else {
        return 0.0;
    };
    if mean_value <= 0.0 {
        return 0.0;
    }

    let variance = values
        .iter()
        .map(|value| {
            let centered = *value - mean_value;
            centered * centered
        })
        .sum::<f64>()
        / values.len() as f64;

    variance.sqrt() / mean_value
}

fn median_abs(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let mut sorted_values: Vec<f64> = values.iter().map(|value| value.abs()).collect();
    sorted_values.sort_by(f64::total_cmp);

    let midpoint = sorted_values.len() / 2;
    if sorted_values.len() % 2 == 0 {
        (sorted_values[midpoint - 1] + sorted_values[midpoint]) / 2.0
    } else {
        sorted_values[midpoint]
    }
}

fn count_in_half_open_interval(
    sorted_timestamps: &[i64],
    start_inclusive: i64,
    end_exclusive: i64,
) -> usize {
    let start_index = sorted_timestamps.partition_point(|timestamp| *timestamp < start_inclusive);
    let end_index = sorted_timestamps.partition_point(|timestamp| *timestamp < end_exclusive);

    end_index.saturating_sub(start_index)
}

fn unique_sorted_rounded(values: &[f64]) -> Vec<f64> {
    let mut rounded_values: Vec<f64> = values.iter().map(|value| round_to(*value, 6)).collect();
    rounded_values.sort_by(f64::total_cmp);
    rounded_values.dedup_by(|left_value, right_value| *left_value == *right_value);

    rounded_values
}

fn round_to(value: f64, digits: i32) -> f64 {
    let scale = 10f64.powi(digits);
    (value * scale).round() / scale
}

fn clamp(value: f64, lower_bound: f64, upper_bound: f64) -> f64 {
    value.max(lower_bound).min(upper_bound)
}

trait PartitionPointIndex {
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
