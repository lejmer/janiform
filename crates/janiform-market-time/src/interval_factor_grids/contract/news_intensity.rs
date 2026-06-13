use std::ops::Range;

use chrono::{DateTime, Utc};

use crate::calendar::ExchangeCalendarRegistry;
use janiform_model_kernels::news::{
    KnownNewsArrivalConfiguration, NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY,
    NewsArrivalConfiguration, NewsArrivalConfigurationError, NewsArrivalIntensityEvaluationInput,
    NewsComponentIntensityEvaluationError, SurpriseNewsArrivalConfiguration,
};

use super::constants::{
    DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY, DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY,
    DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD, DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY,
    INTERVAL_FACTOR_GRID_FALL_BACK_POLICY, INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
    INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY, INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
    INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
    INTERVAL_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
};
use super::error::IntervalFactorGridError;
use super::interval_indexing::build_fixed_interval_indexing;
use super::types::{
    IntervalNewsIntensityMaterializationDiagnostics, IntervalNewsIntensityMaterializationMetadata,
    IntervalNewsIntensityMaterializationOutput,
};

/// Materialize interval news intensity from known-news calendar anchors and surprise-news controls.
///
/// Component policy:
/// - Known component intensity is the interval-scheduled arrival count normalized to per-hour
///   intensity over each bucket `[start, end)`.
/// - Surprise component intensity is the market-state exposure weighted open/closed baseline
///   intensity times the deterministic multiplier, where the multiplier defaults to `1.0`.
/// - Total interval intensity is `known_component + surprise_component`.
#[derive(Debug, Clone)]
pub struct IntervalNewsIntensityMaterializationInput<'a> {
    pub calendar_registry: &'a ExchangeCalendarRegistry,
    pub exchange_code: &'a str,
    pub range_utc: Range<DateTime<Utc>>,
    pub interval_duration_nanos: i64,
    pub known_scheduled_event_timestamps_utc_nanos: &'a [i64],
    pub surprise_open_baseline_intensity: f64,
    pub surprise_closed_baseline_intensity: f64,
    pub surprise_deterministic_time_multiplier_by_interval_index: &'a [(u64, f64)],
}

pub fn materialize_interval_news_intensity_factors(
    input: IntervalNewsIntensityMaterializationInput<'_>,
) -> Result<IntervalNewsIntensityMaterializationOutput, IntervalFactorGridError> {
    let IntervalNewsIntensityMaterializationInput {
        calendar_registry,
        exchange_code,
        range_utc,
        interval_duration_nanos,
        known_scheduled_event_timestamps_utc_nanos,
        surprise_open_baseline_intensity,
        surprise_closed_baseline_intensity,
        surprise_deterministic_time_multiplier_by_interval_index,
    } = input;

    let arrival_configuration = NewsArrivalConfiguration {
        known: KnownNewsArrivalConfiguration {
            scheduled_event_timestamps_utc_nanos: known_scheduled_event_timestamps_utc_nanos
                .to_vec(),
        },
        surprise: SurpriseNewsArrivalConfiguration {
            open_baseline_intensity: surprise_open_baseline_intensity,
            closed_baseline_intensity: surprise_closed_baseline_intensity,
        },
    };

    arrival_configuration
        .validate()
        .map_err(map_news_arrival_configuration_error)?;

    let interval_indexing = build_fixed_interval_indexing(
        calendar_registry,
        exchange_code,
        range_utc.start,
        range_utc.end,
        interval_duration_nanos,
    )?;

    let mut surprise_deterministic_time_multipliers: Vec<f64> =
        vec![1.0; interval_indexing.interval_count];

    let mut surprise_multiplier_override_flags: Vec<bool> =
        vec![false; interval_indexing.interval_count];

    for &(interval_index, deterministic_time_multiplier) in
        surprise_deterministic_time_multiplier_by_interval_index
    {
        if !deterministic_time_multiplier.is_finite() || deterministic_time_multiplier < 0.0 {
            return Err(
                IntervalFactorGridError::InvalidSurpriseDeterministicTimeMultiplier {
                    interval_index,
                    value: deterministic_time_multiplier,
                },
            );
        }

        let interval_position = interval_index as usize;
        if interval_position >= interval_indexing.interval_count {
            return Err(
                IntervalFactorGridError::SurpriseDeterministicTimeMultiplierIntervalOutOfRange {
                    interval_index,
                    interval_count: interval_indexing.interval_count,
                },
            );
        }

        surprise_deterministic_time_multipliers[interval_position] = deterministic_time_multiplier;
        surprise_multiplier_override_flags[interval_position] = true;
    }

    let mut known_news_intensity: Vec<f64> = Vec::with_capacity(interval_indexing.interval_count);

    let mut surprise_news_intensity: Vec<f64> =
        Vec::with_capacity(interval_indexing.interval_count);

    let mut materialized_news_intensity: Vec<f64> =
        Vec::with_capacity(interval_indexing.interval_count);

    let mut known_component_non_zero_interval_count: usize = 0;
    let mut surprise_component_non_zero_interval_count: usize = 0;
    let mut combined_component_non_zero_interval_count: usize = 0;

    let mut known_scheduled_arrival_count: u64 = 0;

    let mut arrival_intensity_policy: &'static str = NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY;

    for (interval_index, surprise_deterministic_time_multiplier) in
        surprise_deterministic_time_multipliers
            .iter()
            .copied()
            .enumerate()
    {
        let interval_start_utc_nanos = interval_indexing.interval_start_utc_nanos[interval_index];
        let interval_end_utc_nanos = interval_indexing.interval_end_utc_nanos[interval_index];

        let market_state_exposure = calendar_registry
            .resolve_market_state_exposure_in_utc_interval(
                exchange_code,
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            )
            .map_err(map_calendar_resolve_error)?;

        let interval_intensity_output = arrival_configuration
            .evaluate_with_deterministic_time_multiplier(NewsArrivalIntensityEvaluationInput {
                interval_start_utc_nanos,
                interval_end_utc_nanos,
                surprise_deterministic_time_multiplier,
                surprise_open_window_share: market_state_exposure.open_window_share,
                surprise_closed_window_share: market_state_exposure.closed_window_share,
            })
            .map_err(|source| {
                map_news_component_intensity_evaluation_error(interval_index, source)
            })?;

        arrival_intensity_policy = interval_intensity_output.intensity_policy;

        if interval_intensity_output.known_component_intensity > 0.0 {
            known_component_non_zero_interval_count += 1;
        }

        if interval_intensity_output.surprise_component_intensity > 0.0 {
            surprise_component_non_zero_interval_count += 1;
        }

        if interval_intensity_output.total_intensity > 0.0 {
            combined_component_non_zero_interval_count += 1;
        }

        known_scheduled_arrival_count = known_scheduled_arrival_count
            .checked_add(interval_intensity_output.known_scheduled_arrival_count)
            .ok_or(
                IntervalFactorGridError::KnownScheduledArrivalCountOverflow {
                    interval_index,
                    value: usize::MAX,
                },
            )?;

        let interval_known_news_intensity = interval_intensity_output.known_component_intensity;
        if !interval_known_news_intensity.is_finite() || interval_known_news_intensity < 0.0 {
            return Err(
                IntervalFactorGridError::InvalidKnownComponentNewsIntensityFactor {
                    interval_index,
                    value: interval_known_news_intensity,
                },
            );
        }

        let interval_surprise_news_intensity =
            interval_intensity_output.surprise_component_intensity;
        if !interval_surprise_news_intensity.is_finite() || interval_surprise_news_intensity < 0.0 {
            return Err(
                IntervalFactorGridError::InvalidSurpriseComponentNewsIntensityFactor {
                    interval_index,
                    value: interval_surprise_news_intensity,
                },
            );
        }

        let interval_news_intensity = interval_intensity_output.total_intensity;

        if !interval_news_intensity.is_finite() || interval_news_intensity < 0.0 {
            return Err(IntervalFactorGridError::InvalidNewsIntensityFactor {
                interval_index,
                value: interval_news_intensity,
            });
        }

        known_news_intensity.push(interval_known_news_intensity);
        surprise_news_intensity.push(interval_surprise_news_intensity);
        materialized_news_intensity.push(interval_news_intensity);
    }

    Ok(IntervalNewsIntensityMaterializationOutput {
        schema_version: INTERVAL_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
        exchange_code: interval_indexing.exchange_code.clone(),
        exchange_timezone_name: interval_indexing.exchange_timezone_name.clone(),
        request_start_utc_nanos: interval_indexing.request_start_utc_nanos,
        request_end_utc_nanos: interval_indexing.request_end_utc_nanos,
        interval_duration_nanos: interval_indexing.interval_duration_nanos,
        interval_indices: interval_indexing.interval_indices,
        interval_start_utc_nanos: interval_indexing.interval_start_utc_nanos,
        interval_end_utc_nanos: interval_indexing.interval_end_utc_nanos,
        interval_start_exchange_local: interval_indexing.interval_start_exchange_local,
        interval_end_exchange_local: interval_indexing.interval_end_exchange_local,
        known_news_intensity,
        surprise_news_intensity,
        news_intensity: materialized_news_intensity,
        metadata: IntervalNewsIntensityMaterializationMetadata {
            materialization_method: DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD,
            component_aggregation_policy: DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY,
            arrival_intensity_policy,
            known_news_calendar_anchor_policy: DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY,
            surprise_news_intensity_control_policy: DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY,
            known_news_calendar_anchor_count: known_scheduled_event_timestamps_utc_nanos.len(),
            surprise_open_baseline_intensity,
            surprise_closed_baseline_intensity,
            interval_boundary_policy: INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
            fall_back_policy: INTERVAL_FACTOR_GRID_FALL_BACK_POLICY,
        },
        diagnostics: IntervalNewsIntensityMaterializationDiagnostics {
            interval_count: interval_indexing.interval_count,
            interval_duration_nanos: interval_indexing.interval_duration_nanos,
            known_component_non_zero_interval_count,
            surprise_component_non_zero_interval_count,
            combined_component_non_zero_interval_count,
            known_scheduled_arrival_count,
            surprise_multiplier_override_interval_count: surprise_multiplier_override_flags
                .iter()
                .filter(|is_overridden| **is_overridden)
                .count(),
            skipped_exchange_local_interval_count: interval_indexing
                .skipped_exchange_local_interval_count,
            repeated_exchange_local_interval_count: interval_indexing
                .repeated_exchange_local_interval_count,
        },
    })
}

fn map_news_arrival_configuration_error(
    source: NewsArrivalConfigurationError,
) -> IntervalFactorGridError {
    match source {
        NewsArrivalConfigurationError::InvalidKnownScheduledTimestampUtcNanos { value } => {
            IntervalFactorGridError::InvalidKnownScheduledTimestampUtcNanos { value }
        }
        NewsArrivalConfigurationError::KnownScheduledTimestampsNotStrictlyIncreasing {
            previous_timestamp_utc_nanos,
            current_timestamp_utc_nanos,
        } => IntervalFactorGridError::KnownScheduledTimestampsNotStrictlyIncreasing {
            previous_timestamp_utc_nanos,
            current_timestamp_utc_nanos,
        },
        NewsArrivalConfigurationError::InvalidSurpriseOpenBaselineIntensity { value } => {
            IntervalFactorGridError::InvalidSurpriseOpenBaselineIntensity { value }
        }
        NewsArrivalConfigurationError::InvalidSurpriseClosedBaselineIntensity { value } => {
            IntervalFactorGridError::InvalidSurpriseClosedBaselineIntensity { value }
        }
    }
}

fn map_calendar_resolve_error(
    source: crate::calendar::ExchangeCalendarResolveError,
) -> IntervalFactorGridError {
    match source {
        crate::calendar::ExchangeCalendarResolveError::MissingExchangeId { exchange_code } => {
            IntervalFactorGridError::MissingExchangeId { exchange_code }
        }
        crate::calendar::ExchangeCalendarResolveError::InvalidTimezoneName {
            exchange_code,
            timezone_name,
        } => IntervalFactorGridError::InvalidTimezoneName {
            exchange_code,
            timezone_name,
        },
        crate::calendar::ExchangeCalendarResolveError::NonIncreasingInterval {
            start_utc_nanos,
            end_utc_nanos,
        } => IntervalFactorGridError::NonIncreasingRange {
            range_start_utc_nanos: start_utc_nanos,
            range_end_utc_nanos: end_utc_nanos,
        },
        crate::calendar::ExchangeCalendarResolveError::TimestampOutOfRange { .. } => {
            IntervalFactorGridError::TimestampOutOfRange {
                field: "range_start_utc",
            }
        }
    }
}

fn map_news_component_intensity_evaluation_error(
    interval_index: usize,
    source: NewsComponentIntensityEvaluationError,
) -> IntervalFactorGridError {
    match source {
        NewsComponentIntensityEvaluationError::InvalidConfiguration { source } => {
            map_news_arrival_configuration_error(source)
        }
        NewsComponentIntensityEvaluationError::NonIncreasingInterval {
            interval_start_utc_nanos,
            interval_end_utc_nanos,
        } => IntervalFactorGridError::NonIncreasingRange {
            range_start_utc_nanos: interval_start_utc_nanos,
            range_end_utc_nanos: interval_end_utc_nanos,
        },
        NewsComponentIntensityEvaluationError::InvalidIntervalStartTimestampUtcNanos { .. } => {
            IntervalFactorGridError::TimestampOutOfRange {
                field: "range_start_utc",
            }
        }
        NewsComponentIntensityEvaluationError::InvalidIntervalEndTimestampUtcNanos { .. } => {
            IntervalFactorGridError::TimestampOutOfRange {
                field: "range_end_utc",
            }
        }
        NewsComponentIntensityEvaluationError::InvalidSurpriseDeterministicTimeMultiplier {
            value,
        } => IntervalFactorGridError::InvalidSurpriseDeterministicTimeMultiplier {
            interval_index: interval_index as u64,
            value,
        },
        NewsComponentIntensityEvaluationError::InvalidSurpriseOpenWindowShare { value } => {
            IntervalFactorGridError::InvalidSurpriseComponentNewsIntensityFactor {
                interval_index,
                value,
            }
        }
        NewsComponentIntensityEvaluationError::InvalidSurpriseClosedWindowShare { value } => {
            IntervalFactorGridError::InvalidSurpriseComponentNewsIntensityFactor {
                interval_index,
                value,
            }
        }
        NewsComponentIntensityEvaluationError::KnownScheduledArrivalCountOverflow { value } => {
            IntervalFactorGridError::KnownScheduledArrivalCountOverflow {
                interval_index,
                value,
            }
        }
        NewsComponentIntensityEvaluationError::InvalidKnownComponentIntensity { value } => {
            IntervalFactorGridError::InvalidKnownComponentNewsIntensityFactor {
                interval_index,
                value,
            }
        }
        NewsComponentIntensityEvaluationError::InvalidSurpriseComponentIntensity { value } => {
            IntervalFactorGridError::InvalidSurpriseComponentNewsIntensityFactor {
                interval_index,
                value,
            }
        }
        NewsComponentIntensityEvaluationError::InvalidTotalIntensity { value } => {
            IntervalFactorGridError::InvalidNewsIntensityFactor {
                interval_index,
                value,
            }
        }
    }
}
