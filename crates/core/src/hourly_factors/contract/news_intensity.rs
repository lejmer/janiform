use chrono::{DateTime, Utc};

use crate::calendar::ExchangeCalendarRegistry;
use crate::news::{
    KnownNewsArrivalConfiguration, NewsArrivalConfiguration, NewsArrivalConfigurationError,
    NewsArrivalIntensityEvaluationInput, NewsComponentIntensityEvaluationError,
    SurpriseNewsArrivalConfiguration, NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY,
};

use super::constants::{
    DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY, DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY,
    DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD, DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY,
    HOURLY_FACTOR_FALL_BACK_POLICY, HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
    HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY, HOURLY_FACTOR_SPRING_FORWARD_POLICY,
    HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY, HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
};
use super::error::HourlyFactorContractError;
use super::interval_indexing::build_hourly_interval_indexing;
use super::types::{
    HourlyNewsIntensityMaterializationDiagnostics, HourlyNewsIntensityMaterializationMetadata,
    HourlyNewsIntensityMaterializationOutput,
};

/// Materialize hourly `nu_h` from known-news calendar anchors and surprise-news controls.
///
/// Component policy:
/// - Known component intensity is the count of scheduled known-news timestamps that fall in each
///   interval bucket `[start, end)`.
/// - Surprise component intensity is `surprise_baseline_intensity *
///   surprise_deterministic_time_multiplier`, where the multiplier defaults to `1.0`.
/// - Total interval intensity is `known_component + surprise_component`.
pub fn materialize_hourly_news_intensity_factors(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    known_scheduled_event_timestamps_utc_nanos: &[i64],
    surprise_baseline_intensity: f64,
    surprise_deterministic_time_multiplier_by_interval_index: &[(u64, f64)],
) -> Result<HourlyNewsIntensityMaterializationOutput, HourlyFactorContractError> {
    let arrival_configuration = NewsArrivalConfiguration {
        known: KnownNewsArrivalConfiguration {
            scheduled_event_timestamps_utc_nanos: known_scheduled_event_timestamps_utc_nanos
                .to_vec(),
        },
        surprise: SurpriseNewsArrivalConfiguration {
            baseline_intensity: surprise_baseline_intensity,
        },
    };
    arrival_configuration
        .validate()
        .map_err(map_news_arrival_configuration_error)?;

    let interval_indexing = build_hourly_interval_indexing(
        calendar_registry,
        exchange_code,
        range_start_utc,
        range_end_utc,
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
                HourlyFactorContractError::InvalidSurpriseDeterministicTimeMultiplier {
                    interval_index,
                    value: deterministic_time_multiplier,
                },
            );
        }

        let interval_position = interval_index as usize;
        if interval_position >= interval_indexing.interval_count {
            return Err(
                HourlyFactorContractError::SurpriseDeterministicTimeMultiplierIntervalOutOfRange {
                    interval_index,
                    interval_count: interval_indexing.interval_count,
                },
            );
        }

        surprise_deterministic_time_multipliers[interval_position] = deterministic_time_multiplier;
        surprise_multiplier_override_flags[interval_position] = true;
    }

    let mut known_component_nu_h: Vec<f64> = Vec::with_capacity(interval_indexing.interval_count);
    let mut surprise_component_nu_h: Vec<f64> =
        Vec::with_capacity(interval_indexing.interval_count);
    let mut materialized_nu_h: Vec<f64> = Vec::with_capacity(interval_indexing.interval_count);
    let mut known_component_non_zero_interval_count: usize = 0;
    let mut surprise_component_non_zero_interval_count: usize = 0;
    let mut combined_component_non_zero_interval_count: usize = 0;
    let mut known_scheduled_arrival_count: u64 = 0;
    let mut arrival_intensity_policy: &'static str = NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY;

    for interval_index in 0..interval_indexing.interval_count {
        let interval_intensity_output = arrival_configuration
            .evaluate_with_deterministic_time_multiplier(NewsArrivalIntensityEvaluationInput {
                interval_start_utc_nanos: interval_indexing.interval_start_utc_nanos
                    [interval_index],
                interval_end_utc_nanos: interval_indexing.interval_end_utc_nanos[interval_index],
                surprise_deterministic_time_multiplier: surprise_deterministic_time_multipliers
                    [interval_index],
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
                HourlyFactorContractError::KnownScheduledArrivalCountOverflow {
                    interval_index,
                    value: usize::MAX,
                },
            )?;

        let interval_known_component_nu_h = interval_intensity_output.known_component_intensity;
        if !interval_known_component_nu_h.is_finite() || interval_known_component_nu_h < 0.0 {
            return Err(
                HourlyFactorContractError::InvalidKnownComponentNewsIntensityFactor {
                    interval_index,
                    value: interval_known_component_nu_h,
                },
            );
        }

        let interval_surprise_component_nu_h =
            interval_intensity_output.surprise_component_intensity;
        if !interval_surprise_component_nu_h.is_finite() || interval_surprise_component_nu_h < 0.0 {
            return Err(
                HourlyFactorContractError::InvalidSurpriseComponentNewsIntensityFactor {
                    interval_index,
                    value: interval_surprise_component_nu_h,
                },
            );
        }

        let interval_nu_h = interval_intensity_output.total_intensity;

        if !interval_nu_h.is_finite() || interval_nu_h < 0.0 {
            return Err(HourlyFactorContractError::InvalidNewsIntensityFactor {
                interval_index,
                value: interval_nu_h,
            });
        }

        known_component_nu_h.push(interval_known_component_nu_h);
        surprise_component_nu_h.push(interval_surprise_component_nu_h);
        materialized_nu_h.push(interval_nu_h);
    }

    Ok(HourlyNewsIntensityMaterializationOutput {
        schema_version: HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
        exchange_code: interval_indexing.exchange_code.clone(),
        exchange_timezone_name: interval_indexing.exchange_timezone_name.clone(),
        request_start_utc_nanos: interval_indexing.request_start_utc_nanos,
        request_end_utc_nanos: interval_indexing.request_end_utc_nanos,
        interval_indices: interval_indexing.interval_indices,
        interval_start_utc_nanos: interval_indexing.interval_start_utc_nanos,
        interval_end_utc_nanos: interval_indexing.interval_end_utc_nanos,
        interval_start_exchange_local: interval_indexing.interval_start_exchange_local,
        interval_end_exchange_local: interval_indexing.interval_end_exchange_local,
        known_component_nu_h,
        surprise_component_nu_h,
        nu_h: materialized_nu_h,
        metadata: HourlyNewsIntensityMaterializationMetadata {
            materialization_method: DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD,
            component_aggregation_policy: DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY,
            arrival_intensity_policy,
            known_news_calendar_anchor_policy: DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY,
            surprise_news_intensity_control_policy: DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY,
            known_news_calendar_anchor_count: known_scheduled_event_timestamps_utc_nanos.len(),
            surprise_baseline_intensity,
            interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
            fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
        },
        diagnostics: HourlyNewsIntensityMaterializationDiagnostics {
            interval_count: interval_indexing.interval_count,
            known_component_non_zero_interval_count,
            surprise_component_non_zero_interval_count,
            combined_component_non_zero_interval_count,
            known_scheduled_arrival_count,
            surprise_multiplier_override_interval_count: surprise_multiplier_override_flags
                .iter()
                .filter(|is_overridden| **is_overridden)
                .count(),
            skipped_local_hour_interval_count: interval_indexing.skipped_local_hour_interval_count,
            repeated_local_hour_interval_count: interval_indexing
                .repeated_local_hour_interval_count,
        },
    })
}

fn map_news_arrival_configuration_error(
    source: NewsArrivalConfigurationError,
) -> HourlyFactorContractError {
    match source {
        NewsArrivalConfigurationError::InvalidKnownScheduledTimestampUtcNanos { value } => {
            HourlyFactorContractError::InvalidKnownScheduledTimestampUtcNanos { value }
        }
        NewsArrivalConfigurationError::KnownScheduledTimestampsNotStrictlyIncreasing {
            previous_timestamp_utc_nanos,
            current_timestamp_utc_nanos,
        } => HourlyFactorContractError::KnownScheduledTimestampsNotStrictlyIncreasing {
            previous_timestamp_utc_nanos,
            current_timestamp_utc_nanos,
        },
        NewsArrivalConfigurationError::InvalidSurpriseBaselineIntensity { value } => {
            HourlyFactorContractError::InvalidSurpriseBaselineIntensity { value }
        }
    }
}

fn map_news_component_intensity_evaluation_error(
    interval_index: usize,
    source: NewsComponentIntensityEvaluationError,
) -> HourlyFactorContractError {
    match source {
        NewsComponentIntensityEvaluationError::InvalidConfiguration { source } => {
            map_news_arrival_configuration_error(source)
        }
        NewsComponentIntensityEvaluationError::NonIncreasingInterval {
            interval_start_utc_nanos,
            interval_end_utc_nanos,
        } => HourlyFactorContractError::NonIncreasingRange {
            range_start_utc_nanos: interval_start_utc_nanos,
            range_end_utc_nanos: interval_end_utc_nanos,
        },
        NewsComponentIntensityEvaluationError::InvalidIntervalStartTimestampUtcNanos { .. } => {
            HourlyFactorContractError::TimestampOutOfRange {
                field: "range_start_utc",
            }
        }
        NewsComponentIntensityEvaluationError::InvalidIntervalEndTimestampUtcNanos { .. } => {
            HourlyFactorContractError::TimestampOutOfRange {
                field: "range_end_utc",
            }
        }
        NewsComponentIntensityEvaluationError::InvalidSurpriseDeterministicTimeMultiplier {
            value,
        } => HourlyFactorContractError::InvalidSurpriseDeterministicTimeMultiplier {
            interval_index: interval_index as u64,
            value,
        },
        NewsComponentIntensityEvaluationError::KnownScheduledArrivalCountOverflow { value } => {
            HourlyFactorContractError::KnownScheduledArrivalCountOverflow {
                interval_index,
                value,
            }
        }
        NewsComponentIntensityEvaluationError::InvalidKnownComponentIntensity { value } => {
            HourlyFactorContractError::InvalidKnownComponentNewsIntensityFactor {
                interval_index,
                value,
            }
        }
        NewsComponentIntensityEvaluationError::InvalidSurpriseComponentIntensity { value } => {
            HourlyFactorContractError::InvalidSurpriseComponentNewsIntensityFactor {
                interval_index,
                value,
            }
        }
        NewsComponentIntensityEvaluationError::InvalidTotalIntensity { value } => {
            HourlyFactorContractError::InvalidNewsIntensityFactor {
                interval_index,
                value,
            }
        }
    }
}
