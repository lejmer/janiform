//! Canonical hourly factor contract and deterministic interval indexing.
//!
//! The contract is the shared boundary object consumed by downstream engines.
//! It is intentionally independent from pricing and simulation internals.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::Serialize;

use crate::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLookupError};
use crate::news::{
    KnownNewsArrivalConfiguration, NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY,
    NewsArrivalConfiguration, NewsArrivalConfigurationError, NewsArrivalIntensityEvaluationInput,
    NewsComponentIntensityEvaluationError, SurpriseNewsArrivalConfiguration,
};

pub const HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION: &str = "seasonality.hourly_factor.contract.v1";
pub const HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY: &str = "start_inclusive_end_exclusive.v1";
pub const HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY: &str = "utc_hour_aligned.v1";
pub const HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY: &str =
    "utc_hour_bucket_projected_to_exchange_local.v1";
pub const HOURLY_FACTOR_SPRING_FORWARD_POLICY: &str = "skip_nonexistent_local_hour_labels.v1";
pub const HOURLY_FACTOR_FALL_BACK_POLICY: &str =
    "preserve_repeated_local_hour_labels_with_numeric_utc_offset.v1";
pub const DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD: &str = "explicit_hourly_input.v1";
pub const HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION: &str =
    "seasonality.hourly_s_h.materialization.v1";
pub const HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION: &str =
    "seasonality.hourly_nu_h.materialization.v2";
pub const DEFAULT_SEASONALITY_INTERPOLATION_METHOD: &str = "previous_observation_carry_forward.v1";
pub const DEFAULT_SEASONALITY_AGGREGATION_METHOD: &str = "mean_of_observations.v1";
pub const DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY: &str = "global_mean.v1";
pub const DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT: usize = 1;
pub const DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD: &str =
    "known_calendar_anchor_plus_surprise_background_controls.v1";
pub const DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY: &str =
    "known_component_plus_surprise_component.v1";
pub const DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY: &str =
    "known_scheduled_timestamp_utc_nanos_start_inclusive_end_exclusive_count.v1";
pub const DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY: &str =
    "interval_indexed_surprise_multiplier_with_default_one.v1";

const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;
const ONE_SECOND_NANOS: i64 = 1_000_000_000;

/// Audit metadata emitted with hourly factor materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlyFactorMaterializationMetadata {
    pub seasonality_materialization_method: String,
    pub news_intensity_materialization_method: String,
    pub interval_boundary_policy: &'static str,
    pub interval_alignment_policy: &'static str,
    pub timezone_transition_policy: &'static str,
    pub spring_forward_policy: &'static str,
    pub fall_back_policy: &'static str,
}

/// Deterministic diagnostics for hourly factor interval coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlyFactorContractDiagnostics {
    pub interval_count: usize,
    pub first_interval_start_utc_nanos: i64,
    pub last_interval_end_utc_nanos: i64,
    pub skipped_local_hour_interval_count: usize,
    pub repeated_local_hour_interval_count: usize,
}

/// Canonical hourly factor output contract for downstream engines.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlyFactorContractOutput {
    pub schema_version: &'static str,
    pub exchange_code: String,
    pub exchange_timezone_name: String,
    pub request_start_utc_nanos: i64,
    pub request_end_utc_nanos: i64,
    pub interval_indices: Vec<u64>,
    pub interval_start_utc_nanos: Vec<i64>,
    pub interval_end_utc_nanos: Vec<i64>,
    pub interval_start_exchange_local: Vec<String>,
    pub interval_end_exchange_local: Vec<String>,
    pub s_h: Vec<f64>,
    pub nu_h: Vec<f64>,
    pub metadata: HourlyFactorMaterializationMetadata,
    pub diagnostics: HourlyFactorContractDiagnostics,
}

/// Audit metadata emitted with hourly seasonality materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlySeasonalityMaterializationMetadata {
    pub interpolation_method: &'static str,
    pub aggregation_method: &'static str,
    pub sparse_fallback_policy: &'static str,
    pub sparse_hour_minimum_observation_count: usize,
    pub interval_boundary_policy: &'static str,
    pub interval_alignment_policy: &'static str,
    pub timezone_transition_policy: &'static str,
    pub spring_forward_policy: &'static str,
    pub fall_back_policy: &'static str,
}

/// Deterministic diagnostics for hourly seasonality materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlySeasonalityMaterializationDiagnostics {
    pub source_point_count: usize,
    pub interval_count: usize,
    pub sparse_interval_count: usize,
    pub interpolated_interval_count: usize,
    pub fallback_interval_count: usize,
    pub skipped_local_hour_interval_count: usize,
    pub repeated_local_hour_interval_count: usize,
}

/// Canonical hourly seasonality output for downstream engines.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlySeasonalityMaterializationOutput {
    pub schema_version: &'static str,
    pub exchange_code: String,
    pub exchange_timezone_name: String,
    pub request_start_utc_nanos: i64,
    pub request_end_utc_nanos: i64,
    pub interval_indices: Vec<u64>,
    pub interval_start_utc_nanos: Vec<i64>,
    pub interval_end_utc_nanos: Vec<i64>,
    pub interval_start_exchange_local: Vec<String>,
    pub interval_end_exchange_local: Vec<String>,
    pub s_h: Vec<f64>,
    pub metadata: HourlySeasonalityMaterializationMetadata,
    pub diagnostics: HourlySeasonalityMaterializationDiagnostics,
}

/// Audit metadata emitted with hourly news-intensity materialization outputs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlyNewsIntensityMaterializationMetadata {
    pub materialization_method: &'static str,
    pub component_aggregation_policy: &'static str,
    pub arrival_intensity_policy: &'static str,
    pub known_news_calendar_anchor_policy: &'static str,
    pub surprise_news_intensity_control_policy: &'static str,
    pub known_news_calendar_anchor_count: usize,
    pub surprise_baseline_intensity: f64,
    pub interval_boundary_policy: &'static str,
    pub interval_alignment_policy: &'static str,
    pub timezone_transition_policy: &'static str,
    pub spring_forward_policy: &'static str,
    pub fall_back_policy: &'static str,
}

/// Deterministic diagnostics for hourly news-intensity materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlyNewsIntensityMaterializationDiagnostics {
    pub interval_count: usize,
    pub known_component_non_zero_interval_count: usize,
    pub surprise_component_non_zero_interval_count: usize,
    pub combined_component_non_zero_interval_count: usize,
    pub known_scheduled_arrival_count: u64,
    pub surprise_multiplier_override_interval_count: usize,
    pub skipped_local_hour_interval_count: usize,
    pub repeated_local_hour_interval_count: usize,
}

/// Canonical hourly news-intensity output for downstream engines.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlyNewsIntensityMaterializationOutput {
    pub schema_version: &'static str,
    pub exchange_code: String,
    pub exchange_timezone_name: String,
    pub request_start_utc_nanos: i64,
    pub request_end_utc_nanos: i64,
    pub interval_indices: Vec<u64>,
    pub interval_start_utc_nanos: Vec<i64>,
    pub interval_end_utc_nanos: Vec<i64>,
    pub interval_start_exchange_local: Vec<String>,
    pub interval_end_exchange_local: Vec<String>,
    pub known_component_nu_h: Vec<f64>,
    pub surprise_component_nu_h: Vec<f64>,
    pub nu_h: Vec<f64>,
    pub metadata: HourlyNewsIntensityMaterializationMetadata,
    pub diagnostics: HourlyNewsIntensityMaterializationDiagnostics,
}

/// Validation failures and contract build errors for hourly factor outputs.
#[derive(Debug, Clone, PartialEq)]
pub enum HourlyFactorContractError {
    EmptyExchangeCode,
    EmptySeasonalityMaterializationMethod,
    EmptyNewsIntensityMaterializationMethod,
    TimestampOutOfRange {
        field: &'static str,
    },
    NonIncreasingRange {
        range_start_utc_nanos: i64,
        range_end_utc_nanos: i64,
    },
    RangeBoundaryNotHourAligned {
        field: &'static str,
        timestamp_utc_nanos: i64,
    },
    MissingExchangeId {
        exchange_code: String,
    },
    InvalidTimezoneName {
        exchange_code: String,
        timezone_name: String,
    },
    FactorLengthMismatch {
        expected_interval_count: usize,
        seasonality_factor_count: usize,
        news_intensity_factor_count: usize,
    },
    InvalidSeasonalityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidKnownScheduledTimestampUtcNanos {
        value: i64,
    },
    KnownScheduledTimestampsNotStrictlyIncreasing {
        previous_timestamp_utc_nanos: i64,
        current_timestamp_utc_nanos: i64,
    },
    InvalidSurpriseBaselineIntensity {
        value: f64,
    },
    SurpriseDeterministicTimeMultiplierIntervalOutOfRange {
        interval_index: u64,
        interval_count: usize,
    },
    InvalidSurpriseDeterministicTimeMultiplier {
        interval_index: u64,
        value: f64,
    },
    KnownScheduledArrivalCountOverflow {
        interval_index: usize,
        value: usize,
    },
    InvalidKnownComponentNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidSurpriseComponentNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    SourceLengthMismatch {
        source_timestamp_count: usize,
        source_factor_count: usize,
    },
    EmptySeasonalitySourceSignal,
    InvalidSeasonalitySourceFactor {
        source_position: usize,
        value: f64,
    },
    InvalidSeasonalityInterpolationMethod {
        method: String,
    },
    InvalidSeasonalityAggregationMethod {
        method: String,
    },
    InvalidSeasonalitySparseFallbackPolicy {
        policy: String,
    },
    InvalidSparseHourMinimumObservationCount {
        sparse_hour_minimum_observation_count: usize,
    },
}

impl Display for HourlyFactorContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyExchangeCode => {
                write!(formatter, "exchange_code must be a non-empty string")
            }
            Self::EmptySeasonalityMaterializationMethod => write!(
                formatter,
                "seasonality_materialization_method must be a non-empty string"
            ),
            Self::EmptyNewsIntensityMaterializationMethod => write!(
                formatter,
                "news_intensity_materialization_method must be a non-empty string"
            ),
            Self::TimestampOutOfRange { field } => {
                write!(
                    formatter,
                    "{field} must be within the supported nanosecond timestamp range"
                )
            }
            Self::NonIncreasingRange {
                range_start_utc_nanos,
                range_end_utc_nanos,
            } => write!(
                formatter,
                "range_start_utc must be earlier than range_end_utc, received start={range_start_utc_nanos}, end={range_end_utc_nanos}"
            ),
            Self::RangeBoundaryNotHourAligned {
                field,
                timestamp_utc_nanos,
            } => write!(
                formatter,
                "{field} must be aligned to an exact UTC hour boundary, received {timestamp_utc_nanos}"
            ),
            Self::MissingExchangeId { exchange_code } => {
                write!(
                    formatter,
                    "missing exchange id `{exchange_code}` in registry"
                )
            }
            Self::InvalidTimezoneName {
                exchange_code,
                timezone_name,
            } => write!(
                formatter,
                "invalid timezone `{timezone_name}` configured for exchange `{exchange_code}`"
            ),
            Self::FactorLengthMismatch {
                expected_interval_count,
                seasonality_factor_count,
                news_intensity_factor_count,
            } => write!(
                formatter,
                "hourly factor length mismatch, expected {expected_interval_count} intervals and received s_h={seasonality_factor_count}, nu_h={news_intensity_factor_count}"
            ),
            Self::InvalidSeasonalityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "s_h must contain finite positive values, found {value} at interval index {interval_index}"
            ),
            Self::InvalidNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "nu_h must contain finite non-negative values, found {value} at interval index {interval_index}"
            ),
            Self::InvalidKnownScheduledTimestampUtcNanos { value } => write!(
                formatter,
                "known_scheduled_event_timestamps_utc_nanos contains an invalid UTC timestamp nanos value {value}"
            ),
            Self::KnownScheduledTimestampsNotStrictlyIncreasing {
                previous_timestamp_utc_nanos,
                current_timestamp_utc_nanos,
            } => write!(
                formatter,
                "known_scheduled_event_timestamps_utc_nanos must be strictly increasing, found previous={previous_timestamp_utc_nanos} and current={current_timestamp_utc_nanos}"
            ),
            Self::InvalidSurpriseBaselineIntensity { value } => write!(
                formatter,
                "surprise_baseline_intensity must be a finite non-negative value, found {value}"
            ),
            Self::SurpriseDeterministicTimeMultiplierIntervalOutOfRange {
                interval_index,
                interval_count,
            } => write!(
                formatter,
                "surprise_deterministic_time_multiplier_by_interval_index contains interval index {interval_index}, expected a value in [0, {interval_count})"
            ),
            Self::InvalidSurpriseDeterministicTimeMultiplier {
                interval_index,
                value,
            } => write!(
                formatter,
                "surprise_deterministic_time_multiplier_by_interval_index value at interval index {interval_index} must be finite and non-negative, found {value}"
            ),
            Self::KnownScheduledArrivalCountOverflow {
                interval_index,
                value,
            } => write!(
                formatter,
                "known scheduled arrival count overflowed at interval index {interval_index}, found {value}"
            ),
            Self::InvalidKnownComponentNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "known_component_nu_h must contain finite non-negative values, found {value} at interval index {interval_index}"
            ),
            Self::InvalidSurpriseComponentNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "surprise_component_nu_h must contain finite non-negative values, found {value} at interval index {interval_index}"
            ),
            Self::SourceLengthMismatch {
                source_timestamp_count,
                source_factor_count,
            } => write!(
                formatter,
                "seasonality source length mismatch, received timestamp_count={source_timestamp_count}, factor_count={source_factor_count}"
            ),
            Self::EmptySeasonalitySourceSignal => write!(
                formatter,
                "seasonality source must contain at least one timestamp and factor sample"
            ),
            Self::InvalidSeasonalitySourceFactor {
                source_position,
                value,
            } => write!(
                formatter,
                "seasonality source factors must be finite positive values, found {value} at source position {source_position}"
            ),
            Self::InvalidSeasonalityInterpolationMethod { method } => write!(
                formatter,
                "invalid seasonality interpolation_method `{method}`, supported values are `previous_observation_carry_forward.v1` and `linear_between_observations.v1`"
            ),
            Self::InvalidSeasonalityAggregationMethod { method } => write!(
                formatter,
                "invalid seasonality aggregation_method `{method}`, supported values are `mean_of_observations.v1` and `last_observation.v1`"
            ),
            Self::InvalidSeasonalitySparseFallbackPolicy { policy } => write!(
                formatter,
                "invalid seasonality sparse_fallback_policy `{policy}`, supported values are `global_mean.v1` and `previous_materialized_then_global_mean.v1`"
            ),
            Self::InvalidSparseHourMinimumObservationCount {
                sparse_hour_minimum_observation_count,
            } => write!(
                formatter,
                "sparse_hour_minimum_observation_count must be at least 1, received {sparse_hour_minimum_observation_count}"
            ),
        }
    }
}

impl Error for HourlyFactorContractError {}

/// Build canonical hourly factor output for an exchange and UTC date range.
///
/// Interval policy:
/// - The request range is interpreted as `[range_start_utc, range_end_utc)`.
/// - Both boundaries must be exact UTC hour boundaries.
/// - Buckets are emitted as consecutive non-overlapping intervals `[start, end)`.
/// - `interval_indices` are zero-based and strictly increasing by one.
/// - Exchange-local interval labels are projections of UTC boundaries and include numeric offsets.
/// - Spring-forward transitions skip nonexistent local-hour labels.
/// - Fall-back transitions preserve repeated local-hour labels and disambiguate them by offset.
pub fn build_hourly_factor_contract(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    seasonality_factors: &[f64],
    news_intensity_factors: &[f64],
    seasonality_materialization_method: &str,
    news_intensity_materialization_method: &str,
) -> Result<HourlyFactorContractOutput, HourlyFactorContractError> {
    if seasonality_materialization_method.trim().is_empty() {
        return Err(HourlyFactorContractError::EmptySeasonalityMaterializationMethod);
    }
    if news_intensity_materialization_method.trim().is_empty() {
        return Err(HourlyFactorContractError::EmptyNewsIntensityMaterializationMethod);
    }

    let interval_indexing = build_hourly_interval_indexing(
        calendar_registry,
        exchange_code,
        range_start_utc,
        range_end_utc,
    )?;

    let interval_count = interval_indexing.interval_count;
    if seasonality_factors.len() != interval_count || news_intensity_factors.len() != interval_count
    {
        return Err(HourlyFactorContractError::FactorLengthMismatch {
            expected_interval_count: interval_count,
            seasonality_factor_count: seasonality_factors.len(),
            news_intensity_factor_count: news_intensity_factors.len(),
        });
    }

    for (interval_index, seasonality_factor) in seasonality_factors.iter().copied().enumerate() {
        if !seasonality_factor.is_finite() || seasonality_factor <= 0.0 {
            return Err(HourlyFactorContractError::InvalidSeasonalityFactor {
                interval_index,
                value: seasonality_factor,
            });
        }
    }

    for (interval_index, news_intensity_factor) in
        news_intensity_factors.iter().copied().enumerate()
    {
        if !news_intensity_factor.is_finite() || news_intensity_factor < 0.0 {
            return Err(HourlyFactorContractError::InvalidNewsIntensityFactor {
                interval_index,
                value: news_intensity_factor,
            });
        }
    }

    Ok(HourlyFactorContractOutput {
        schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        exchange_code: interval_indexing.exchange_code.clone(),
        exchange_timezone_name: interval_indexing.exchange_timezone_name.clone(),
        request_start_utc_nanos: interval_indexing.request_start_utc_nanos,
        request_end_utc_nanos: interval_indexing.request_end_utc_nanos,
        interval_indices: interval_indexing.interval_indices.clone(),
        interval_start_utc_nanos: interval_indexing.interval_start_utc_nanos.clone(),
        interval_end_utc_nanos: interval_indexing.interval_end_utc_nanos.clone(),
        interval_start_exchange_local: interval_indexing.interval_start_exchange_local.clone(),
        interval_end_exchange_local: interval_indexing.interval_end_exchange_local.clone(),
        s_h: seasonality_factors.to_vec(),
        nu_h: news_intensity_factors.to_vec(),
        metadata: HourlyFactorMaterializationMetadata {
            seasonality_materialization_method: seasonality_materialization_method.to_string(),
            news_intensity_materialization_method: news_intensity_materialization_method
                .to_string(),
            interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
            fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
        },
        diagnostics: HourlyFactorContractDiagnostics {
            interval_count,
            first_interval_start_utc_nanos: interval_indexing.request_start_utc_nanos,
            last_interval_end_utc_nanos: interval_indexing.request_end_utc_nanos,
            skipped_local_hour_interval_count: interval_indexing.skipped_local_hour_interval_count,
            repeated_local_hour_interval_count: interval_indexing
                .repeated_local_hour_interval_count,
        },
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HourlySeasonalityInterpolationMethod {
    PreviousObservationCarryForward,
    LinearBetweenObservations,
}

impl HourlySeasonalityInterpolationMethod {
    fn as_policy_identifier(self) -> &'static str {
        match self {
            Self::PreviousObservationCarryForward => "previous_observation_carry_forward.v1",
            Self::LinearBetweenObservations => "linear_between_observations.v1",
        }
    }
}

impl FromStr for HourlySeasonalityInterpolationMethod {
    type Err = HourlyFactorContractError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        match source {
            "previous_observation_carry_forward.v1" => Ok(Self::PreviousObservationCarryForward),
            "linear_between_observations.v1" => Ok(Self::LinearBetweenObservations),
            _ => Err(
                HourlyFactorContractError::InvalidSeasonalityInterpolationMethod {
                    method: source.to_string(),
                },
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HourlySeasonalityAggregationMethod {
    MeanOfObservations,
    LastObservation,
}

impl HourlySeasonalityAggregationMethod {
    fn as_policy_identifier(self) -> &'static str {
        match self {
            Self::MeanOfObservations => "mean_of_observations.v1",
            Self::LastObservation => "last_observation.v1",
        }
    }
}

impl FromStr for HourlySeasonalityAggregationMethod {
    type Err = HourlyFactorContractError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        match source {
            "mean_of_observations.v1" => Ok(Self::MeanOfObservations),
            "last_observation.v1" => Ok(Self::LastObservation),
            _ => Err(
                HourlyFactorContractError::InvalidSeasonalityAggregationMethod {
                    method: source.to_string(),
                },
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HourlySeasonalitySparseFallbackPolicy {
    GlobalMean,
    PreviousMaterializedThenGlobalMean,
}

impl HourlySeasonalitySparseFallbackPolicy {
    fn as_policy_identifier(self) -> &'static str {
        match self {
            Self::GlobalMean => "global_mean.v1",
            Self::PreviousMaterializedThenGlobalMean => "previous_materialized_then_global_mean.v1",
        }
    }
}

impl FromStr for HourlySeasonalitySparseFallbackPolicy {
    type Err = HourlyFactorContractError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        match source {
            "global_mean.v1" => Ok(Self::GlobalMean),
            "previous_materialized_then_global_mean.v1" => {
                Ok(Self::PreviousMaterializedThenGlobalMean)
            }
            _ => Err(
                HourlyFactorContractError::InvalidSeasonalitySparseFallbackPolicy {
                    policy: source.to_string(),
                },
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HourlySeasonalityMaterializationSettings {
    interpolation_method: HourlySeasonalityInterpolationMethod,
    aggregation_method: HourlySeasonalityAggregationMethod,
    sparse_fallback_policy: HourlySeasonalitySparseFallbackPolicy,
    sparse_hour_minimum_observation_count: usize,
}

impl HourlySeasonalityMaterializationSettings {
    fn from_policy_identifiers(
        interpolation_method: &str,
        aggregation_method: &str,
        sparse_fallback_policy: &str,
        sparse_hour_minimum_observation_count: usize,
    ) -> Result<Self, HourlyFactorContractError> {
        if sparse_hour_minimum_observation_count < 1 {
            return Err(
                HourlyFactorContractError::InvalidSparseHourMinimumObservationCount {
                    sparse_hour_minimum_observation_count,
                },
            );
        }

        Ok(Self {
            interpolation_method: HourlySeasonalityInterpolationMethod::from_str(
                interpolation_method,
            )?,
            aggregation_method: HourlySeasonalityAggregationMethod::from_str(aggregation_method)?,
            sparse_fallback_policy: HourlySeasonalitySparseFallbackPolicy::from_str(
                sparse_fallback_policy,
            )?,
            sparse_hour_minimum_observation_count,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SourceSeasonalityPoint {
    timestamp_utc_nanos: i64,
    seasonality_factor: f64,
    source_position: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HourlyIntervalIndexing {
    exchange_code: String,
    exchange_timezone_name: String,
    request_start_utc_nanos: i64,
    request_end_utc_nanos: i64,
    interval_count: usize,
    interval_indices: Vec<u64>,
    interval_start_utc_nanos: Vec<i64>,
    interval_end_utc_nanos: Vec<i64>,
    interval_start_exchange_local: Vec<String>,
    interval_end_exchange_local: Vec<String>,
    skipped_local_hour_interval_count: usize,
    repeated_local_hour_interval_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExchangeLocalHourTransitionKind {
    Standard,
    Skipped,
    Repeated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExchangeLocalIntervalBoundaryProjection {
    interval_start_exchange_local: String,
    interval_end_exchange_local: String,
    transition_kind: ExchangeLocalHourTransitionKind,
}

/// Materialize a continuous or high-resolution seasonality signal into hourly `s_h`.
///
/// Sparse or missing intervals use a deterministic fallback sequence:
/// - Attempt interpolation using the configured interpolation method.
/// - If interpolation is not possible, apply the configured sparse fallback policy.
/// - UTC bucket boundaries are authoritative during timezone transitions.
pub fn materialize_hourly_seasonality_factors(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    source_timestamp_utc_nanos: &[i64],
    source_seasonality_factors: &[f64],
    interpolation_method: &str,
    aggregation_method: &str,
    sparse_fallback_policy: &str,
    sparse_hour_minimum_observation_count: usize,
) -> Result<HourlySeasonalityMaterializationOutput, HourlyFactorContractError> {
    let settings = HourlySeasonalityMaterializationSettings::from_policy_identifiers(
        interpolation_method,
        aggregation_method,
        sparse_fallback_policy,
        sparse_hour_minimum_observation_count,
    )?;

    if source_timestamp_utc_nanos.len() != source_seasonality_factors.len() {
        return Err(HourlyFactorContractError::SourceLengthMismatch {
            source_timestamp_count: source_timestamp_utc_nanos.len(),
            source_factor_count: source_seasonality_factors.len(),
        });
    }
    if source_timestamp_utc_nanos.is_empty() {
        return Err(HourlyFactorContractError::EmptySeasonalitySourceSignal);
    }

    let mut source_points: Vec<SourceSeasonalityPoint> =
        Vec::with_capacity(source_timestamp_utc_nanos.len());

    for (source_position, (timestamp_utc_nanos, seasonality_factor)) in source_timestamp_utc_nanos
        .iter()
        .copied()
        .zip(source_seasonality_factors.iter().copied())
        .enumerate()
    {
        if !seasonality_factor.is_finite() || seasonality_factor <= 0.0 {
            return Err(HourlyFactorContractError::InvalidSeasonalitySourceFactor {
                source_position,
                value: seasonality_factor,
            });
        }

        source_points.push(SourceSeasonalityPoint {
            timestamp_utc_nanos,
            seasonality_factor,
            source_position,
        });
    }

    source_points.sort_by_key(|source_point| {
        (
            source_point.timestamp_utc_nanos,
            source_point.source_position,
        )
    });

    let source_factor_sum: f64 = source_points
        .iter()
        .map(|source_point| source_point.seasonality_factor)
        .sum();
    let global_mean_factor = source_factor_sum / source_points.len() as f64;

    let interval_indexing = build_hourly_interval_indexing(
        calendar_registry,
        exchange_code,
        range_start_utc,
        range_end_utc,
    )?;
    let mut materialized_s_h: Vec<f64> = Vec::with_capacity(interval_indexing.interval_count);
    let mut sparse_interval_count: usize = 0;
    let mut interpolated_interval_count: usize = 0;
    let mut fallback_interval_count: usize = 0;

    let mut source_window_start: usize = 0;
    let mut source_window_end: usize = 0;

    for interval_index in 0..interval_indexing.interval_count {
        let interval_start_utc_nanos = interval_indexing.interval_start_utc_nanos[interval_index];
        let interval_end_utc_nanos = interval_indexing.interval_end_utc_nanos[interval_index];

        while source_window_start < source_points.len()
            && source_points[source_window_start].timestamp_utc_nanos < interval_start_utc_nanos
        {
            source_window_start += 1;
        }

        if source_window_end < source_window_start {
            source_window_end = source_window_start;
        }

        while source_window_end < source_points.len()
            && source_points[source_window_end].timestamp_utc_nanos < interval_end_utc_nanos
        {
            source_window_end += 1;
        }

        let source_points_in_interval = &source_points[source_window_start..source_window_end];
        let interval_factor =
            if source_points_in_interval.len() >= settings.sparse_hour_minimum_observation_count {
                aggregate_source_points_in_interval(
                    source_points_in_interval,
                    settings.aggregation_method,
                )
            } else {
                sparse_interval_count += 1;

                if let Some(interpolated_factor) = interpolate_sparse_interval(
                    &source_points,
                    interval_start_utc_nanos,
                    interval_end_utc_nanos,
                    settings.interpolation_method,
                ) {
                    interpolated_interval_count += 1;
                    interpolated_factor
                } else {
                    fallback_interval_count += 1;
                    fallback_factor_for_sparse_interval(
                        settings.sparse_fallback_policy,
                        &materialized_s_h,
                        global_mean_factor,
                    )
                }
            };

        if !interval_factor.is_finite() || interval_factor <= 0.0 {
            return Err(HourlyFactorContractError::InvalidSeasonalityFactor {
                interval_index,
                value: interval_factor,
            });
        }

        materialized_s_h.push(interval_factor);
    }

    Ok(HourlySeasonalityMaterializationOutput {
        schema_version: HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
        exchange_code: interval_indexing.exchange_code.clone(),
        exchange_timezone_name: interval_indexing.exchange_timezone_name.clone(),
        request_start_utc_nanos: interval_indexing.request_start_utc_nanos,
        request_end_utc_nanos: interval_indexing.request_end_utc_nanos,
        interval_indices: interval_indexing.interval_indices,
        interval_start_utc_nanos: interval_indexing.interval_start_utc_nanos,
        interval_end_utc_nanos: interval_indexing.interval_end_utc_nanos,
        interval_start_exchange_local: interval_indexing.interval_start_exchange_local,
        interval_end_exchange_local: interval_indexing.interval_end_exchange_local,
        s_h: materialized_s_h,
        metadata: HourlySeasonalityMaterializationMetadata {
            interpolation_method: settings.interpolation_method.as_policy_identifier(),
            aggregation_method: settings.aggregation_method.as_policy_identifier(),
            sparse_fallback_policy: settings.sparse_fallback_policy.as_policy_identifier(),
            sparse_hour_minimum_observation_count: settings.sparse_hour_minimum_observation_count,
            interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
            fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
        },
        diagnostics: HourlySeasonalityMaterializationDiagnostics {
            source_point_count: source_points.len(),
            interval_count: interval_indexing.interval_count,
            sparse_interval_count,
            interpolated_interval_count,
            fallback_interval_count,
            skipped_local_hour_interval_count: interval_indexing.skipped_local_hour_interval_count,
            repeated_local_hour_interval_count: interval_indexing
                .repeated_local_hour_interval_count,
        },
    })
}

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

fn build_hourly_interval_indexing(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
) -> Result<HourlyIntervalIndexing, HourlyFactorContractError> {
    if exchange_code.trim().is_empty() {
        return Err(HourlyFactorContractError::EmptyExchangeCode);
    }

    let range_start_utc_nanos = timestamp_to_utc_nanos(range_start_utc, "range_start_utc")?;
    let range_end_utc_nanos = timestamp_to_utc_nanos(range_end_utc, "range_end_utc")?;

    if range_start_utc_nanos >= range_end_utc_nanos {
        return Err(HourlyFactorContractError::NonIncreasingRange {
            range_start_utc_nanos,
            range_end_utc_nanos,
        });
    }
    if range_start_utc_nanos % ONE_HOUR_NANOS != 0 {
        return Err(HourlyFactorContractError::RangeBoundaryNotHourAligned {
            field: "range_start_utc",
            timestamp_utc_nanos: range_start_utc_nanos,
        });
    }
    if range_end_utc_nanos % ONE_HOUR_NANOS != 0 {
        return Err(HourlyFactorContractError::RangeBoundaryNotHourAligned {
            field: "range_end_utc",
            timestamp_utc_nanos: range_end_utc_nanos,
        });
    }

    let definition = calendar_registry
        .get_required(exchange_code)
        .map_err(map_registry_lookup_error)?;
    let exchange_timezone_name = definition.timezone_name().to_string();
    let timezone = Tz::from_str(definition.timezone_name()).map_err(|_| {
        HourlyFactorContractError::InvalidTimezoneName {
            exchange_code: exchange_code.to_string(),
            timezone_name: definition.timezone_name().to_string(),
        }
    })?;

    let interval_count = ((range_end_utc_nanos - range_start_utc_nanos) / ONE_HOUR_NANOS) as usize;

    let mut interval_indices: Vec<u64> = Vec::with_capacity(interval_count);
    let mut interval_start_utc_nanos: Vec<i64> = Vec::with_capacity(interval_count);
    let mut interval_end_utc_nanos: Vec<i64> = Vec::with_capacity(interval_count);
    let mut interval_start_exchange_local: Vec<String> = Vec::with_capacity(interval_count);
    let mut interval_end_exchange_local: Vec<String> = Vec::with_capacity(interval_count);
    let mut skipped_local_hour_interval_count: usize = 0;
    let mut repeated_local_hour_interval_count: usize = 0;

    for interval_index in 0..interval_count {
        let interval_offset_nanos = interval_index as i64 * ONE_HOUR_NANOS;
        let interval_start_nanos = range_start_utc_nanos + interval_offset_nanos;
        let interval_end_nanos = interval_start_nanos + ONE_HOUR_NANOS;

        let start_utc = utc_timestamp_from_nanos(interval_start_nanos, "range_start_utc")?;
        let end_utc = utc_timestamp_from_nanos(interval_end_nanos, "range_end_utc")?;
        let local_boundary_projection =
            project_interval_boundaries_to_exchange_local(start_utc, end_utc, &timezone);

        match local_boundary_projection.transition_kind {
            ExchangeLocalHourTransitionKind::Standard => {}
            ExchangeLocalHourTransitionKind::Skipped => skipped_local_hour_interval_count += 1,
            ExchangeLocalHourTransitionKind::Repeated => repeated_local_hour_interval_count += 1,
        }

        interval_indices.push(interval_index as u64);
        interval_start_utc_nanos.push(interval_start_nanos);
        interval_end_utc_nanos.push(interval_end_nanos);
        interval_start_exchange_local.push(local_boundary_projection.interval_start_exchange_local);
        interval_end_exchange_local.push(local_boundary_projection.interval_end_exchange_local);
    }

    Ok(HourlyIntervalIndexing {
        exchange_code: exchange_code.to_string(),
        exchange_timezone_name,
        request_start_utc_nanos: range_start_utc_nanos,
        request_end_utc_nanos: range_end_utc_nanos,
        interval_count,
        interval_indices,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        interval_start_exchange_local,
        interval_end_exchange_local,
        skipped_local_hour_interval_count,
        repeated_local_hour_interval_count,
    })
}

fn project_interval_boundaries_to_exchange_local(
    interval_start_utc: DateTime<Utc>,
    interval_end_utc: DateTime<Utc>,
    timezone: &Tz,
) -> ExchangeLocalIntervalBoundaryProjection {
    let interval_start_exchange_local = interval_start_utc.with_timezone(timezone);
    let interval_end_exchange_local = interval_end_utc.with_timezone(timezone);
    let wall_clock_seconds_between_boundaries = interval_end_exchange_local
        .naive_local()
        .signed_duration_since(interval_start_exchange_local.naive_local())
        .num_seconds();
    let transition_kind =
        if wall_clock_seconds_between_boundaries > ONE_HOUR_NANOS / ONE_SECOND_NANOS {
            ExchangeLocalHourTransitionKind::Skipped
        } else if wall_clock_seconds_between_boundaries < ONE_HOUR_NANOS / ONE_SECOND_NANOS {
            ExchangeLocalHourTransitionKind::Repeated
        } else {
            ExchangeLocalHourTransitionKind::Standard
        };

    ExchangeLocalIntervalBoundaryProjection {
        interval_start_exchange_local: interval_start_exchange_local.to_rfc3339(),
        interval_end_exchange_local: interval_end_exchange_local.to_rfc3339(),
        transition_kind,
    }
}

fn aggregate_source_points_in_interval(
    source_points_in_interval: &[SourceSeasonalityPoint],
    aggregation_method: HourlySeasonalityAggregationMethod,
) -> f64 {
    match aggregation_method {
        HourlySeasonalityAggregationMethod::MeanOfObservations => {
            let summed_factor: f64 = source_points_in_interval
                .iter()
                .map(|source_point| source_point.seasonality_factor)
                .sum();
            summed_factor / source_points_in_interval.len() as f64
        }
        HourlySeasonalityAggregationMethod::LastObservation => source_points_in_interval
            .last()
            .map(|source_point| source_point.seasonality_factor)
            .unwrap_or(1.0),
    }
}

fn interpolate_sparse_interval(
    sorted_source_points: &[SourceSeasonalityPoint],
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    interpolation_method: HourlySeasonalityInterpolationMethod,
) -> Option<f64> {
    match interpolation_method {
        HourlySeasonalityInterpolationMethod::PreviousObservationCarryForward => {
            let prior_source_count = sorted_source_points.partition_point(|source_point| {
                source_point.timestamp_utc_nanos < interval_end_utc_nanos
            });

            if prior_source_count == 0 {
                None
            } else {
                sorted_source_points
                    .get(prior_source_count - 1)
                    .map(|source_point| source_point.seasonality_factor)
            }
        }
        HourlySeasonalityInterpolationMethod::LinearBetweenObservations => {
            let interval_midpoint_utc_nanos =
                interval_start_utc_nanos + (interval_end_utc_nanos - interval_start_utc_nanos) / 2;
            let right_source_index = sorted_source_points.partition_point(|source_point| {
                source_point.timestamp_utc_nanos < interval_midpoint_utc_nanos
            });

            if right_source_index < sorted_source_points.len()
                && sorted_source_points[right_source_index].timestamp_utc_nanos
                    == interval_midpoint_utc_nanos
            {
                return Some(sorted_source_points[right_source_index].seasonality_factor);
            }

            if right_source_index == 0 || right_source_index >= sorted_source_points.len() {
                return None;
            }

            let left_source_point = sorted_source_points[right_source_index - 1];
            let right_source_point = sorted_source_points[right_source_index];

            let linear_denominator = (right_source_point.timestamp_utc_nanos
                - left_source_point.timestamp_utc_nanos)
                as f64;
            if linear_denominator <= 0.0 {
                return Some(
                    (left_source_point.seasonality_factor + right_source_point.seasonality_factor)
                        / 2.0,
                );
            }

            let midpoint_weight = (interval_midpoint_utc_nanos
                - left_source_point.timestamp_utc_nanos) as f64
                / linear_denominator;

            Some(
                left_source_point.seasonality_factor
                    + (right_source_point.seasonality_factor
                        - left_source_point.seasonality_factor)
                        * midpoint_weight,
            )
        }
    }
}

fn fallback_factor_for_sparse_interval(
    sparse_fallback_policy: HourlySeasonalitySparseFallbackPolicy,
    materialized_s_h: &[f64],
    global_mean_factor: f64,
) -> f64 {
    match sparse_fallback_policy {
        HourlySeasonalitySparseFallbackPolicy::GlobalMean => global_mean_factor,
        HourlySeasonalitySparseFallbackPolicy::PreviousMaterializedThenGlobalMean => {
            materialized_s_h
                .last()
                .copied()
                .unwrap_or(global_mean_factor)
        }
    }
}

fn map_registry_lookup_error(
    source: ExchangeCalendarRegistryLookupError,
) -> HourlyFactorContractError {
    match source {
        ExchangeCalendarRegistryLookupError::MissingExchangeId { exchange_code } => {
            HourlyFactorContractError::MissingExchangeId { exchange_code }
        }
    }
}

fn timestamp_to_utc_nanos(
    timestamp_utc: DateTime<Utc>,
    field: &'static str,
) -> Result<i64, HourlyFactorContractError> {
    timestamp_utc
        .timestamp_nanos_opt()
        .ok_or(HourlyFactorContractError::TimestampOutOfRange { field })
}

fn utc_timestamp_from_nanos(
    timestamp_utc_nanos: i64,
    field: &'static str,
) -> Result<DateTime<Utc>, HourlyFactorContractError> {
    let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
    let subsecond_nanos = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;

    DateTime::from_timestamp(seconds, subsecond_nanos)
        .ok_or(HourlyFactorContractError::TimestampOutOfRange { field })
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use crate::calendar::ExchangeCalendarRegistryLoader;
    use crate::hourly_factors::{
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY,
        DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY,
        DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD,
        DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY, HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        HOURLY_FACTOR_FALL_BACK_POLICY, HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
        HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY, HOURLY_FACTOR_SPRING_FORWARD_POLICY,
        HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
        HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
        HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION, HourlyFactorContractError,
        build_hourly_factor_contract, materialize_hourly_news_intensity_factors,
        materialize_hourly_seasonality_factors,
    };

    #[test]
    fn hourly_factor_contract_builds_expected_schema_and_metadata() {
        let registry = build_calendar_registry();

        let output = build_hourly_factor_contract(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T17:00:00Z"),
            &[1.0, 1.1, 0.9],
            &[0.4, 0.0, 0.2],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            "calendar_closed_window_baseline.v1",
        )
        .expect("hourly factor contract build should succeed");

        assert_eq!(output.schema_version, HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION);
        assert_eq!(output.exchange_code, "XNYS");
        assert_eq!(output.exchange_timezone_name, "America/New_York");
        assert_eq!(
            output.metadata.interval_boundary_policy,
            HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY
        );
        assert_eq!(
            output.metadata.interval_alignment_policy,
            HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY
        );
        assert_eq!(
            output.metadata.timezone_transition_policy,
            HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY
        );
        assert_eq!(
            output.metadata.spring_forward_policy,
            HOURLY_FACTOR_SPRING_FORWARD_POLICY
        );
        assert_eq!(
            output.metadata.fall_back_policy,
            HOURLY_FACTOR_FALL_BACK_POLICY
        );
        assert_eq!(
            output.metadata.seasonality_materialization_method,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD
        );
        assert_eq!(
            output.metadata.news_intensity_materialization_method,
            "calendar_closed_window_baseline.v1"
        );
        assert_eq!(output.diagnostics.interval_count, 3);
        assert_eq!(output.diagnostics.skipped_local_hour_interval_count, 0);
        assert_eq!(output.diagnostics.repeated_local_hour_interval_count, 0);
    }

    #[test]
    fn hourly_factor_contract_intervals_are_consecutive_and_non_overlapping() {
        let registry = build_calendar_registry();

        let output = build_hourly_factor_contract(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T18:00:00Z"),
            &[1.0, 1.0, 1.0, 1.0],
            &[0.0, 0.0, 0.0, 0.0],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        )
        .expect("hourly factor contract build should succeed");

        assert_eq!(output.interval_indices, vec![0, 1, 2, 3]);

        for interval_position in 1..output.interval_start_utc_nanos.len() {
            assert_eq!(
                output.interval_start_utc_nanos[interval_position],
                output.interval_end_utc_nanos[interval_position - 1]
            );
        }

        for interval_position in 0..output.interval_start_utc_nanos.len() {
            let interval_start = output.interval_start_utc_nanos[interval_position];
            let interval_end = output.interval_end_utc_nanos[interval_position];
            assert!(interval_start < interval_end);
            assert_eq!(interval_end - interval_start, 3_600_000_000_000);
        }
    }

    #[test]
    fn hourly_factor_contract_spring_forward_policy_skips_nonexistent_local_hour_labels() {
        let registry = build_calendar_registry();

        let output = build_hourly_factor_contract(
            &registry,
            "XNYS",
            parse_timestamp("2026-03-08T06:00:00Z"),
            parse_timestamp("2026-03-08T09:00:00Z"),
            &[1.0, 1.0, 1.0],
            &[0.0, 0.0, 0.0],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        )
        .expect("hourly factor contract build should succeed");

        assert_eq!(
            output.interval_start_exchange_local,
            vec![
                "2026-03-08T01:00:00-05:00",
                "2026-03-08T03:00:00-04:00",
                "2026-03-08T04:00:00-04:00",
            ]
        );
        assert_eq!(
            output.interval_end_exchange_local,
            vec![
                "2026-03-08T03:00:00-04:00",
                "2026-03-08T04:00:00-04:00",
                "2026-03-08T05:00:00-04:00",
            ]
        );
        assert_eq!(
            output.metadata.spring_forward_policy,
            HOURLY_FACTOR_SPRING_FORWARD_POLICY
        );
        assert_eq!(output.diagnostics.skipped_local_hour_interval_count, 1);
        assert_eq!(output.diagnostics.repeated_local_hour_interval_count, 0);
    }

    #[test]
    fn hourly_factor_contract_fall_back_policy_preserves_repeated_local_hour_labels() {
        let registry = build_calendar_registry();

        let output = build_hourly_factor_contract(
            &registry,
            "XNYS",
            parse_timestamp("2026-11-01T05:00:00Z"),
            parse_timestamp("2026-11-01T08:00:00Z"),
            &[1.0, 1.0, 1.0],
            &[0.0, 0.0, 0.0],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        )
        .expect("hourly factor contract build should succeed");

        assert_eq!(
            output.interval_start_exchange_local,
            vec![
                "2026-11-01T01:00:00-04:00",
                "2026-11-01T01:00:00-05:00",
                "2026-11-01T02:00:00-05:00",
            ]
        );
        assert_eq!(
            output.interval_end_exchange_local,
            vec![
                "2026-11-01T01:00:00-05:00",
                "2026-11-01T02:00:00-05:00",
                "2026-11-01T03:00:00-05:00",
            ]
        );
        assert_eq!(
            output.metadata.fall_back_policy,
            HOURLY_FACTOR_FALL_BACK_POLICY
        );
        assert_eq!(output.diagnostics.skipped_local_hour_interval_count, 0);
        assert_eq!(output.diagnostics.repeated_local_hour_interval_count, 1);
    }

    #[test]
    fn hourly_factor_contract_rejects_invalid_ranges_and_unknown_exchange_ids() {
        let registry = build_calendar_registry();

        let non_increasing_range_error = build_hourly_factor_contract(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T14:00:00Z"),
            &[],
            &[],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        )
        .expect_err("non-increasing range must fail");

        assert!(matches!(
            non_increasing_range_error,
            HourlyFactorContractError::NonIncreasingRange { .. }
        ));

        let unaligned_range_error = build_hourly_factor_contract(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:30:00Z"),
            parse_timestamp("2026-02-02T16:00:00Z"),
            &[1.0],
            &[0.0],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        )
        .expect_err("hour-unaligned range must fail");

        assert!(matches!(
            unaligned_range_error,
            HourlyFactorContractError::RangeBoundaryNotHourAligned { .. }
        ));

        let missing_exchange_error = build_hourly_factor_contract(
            &registry,
            "MISSING",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T16:00:00Z"),
            &[1.0, 1.0],
            &[0.0, 0.0],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        )
        .expect_err("missing exchange id must fail");

        assert!(matches!(
            missing_exchange_error,
            HourlyFactorContractError::MissingExchangeId { .. }
        ));
    }

    #[test]
    fn seasonality_materializer_handles_sparse_and_missing_intervals_with_fallback_policy() {
        let registry = build_calendar_registry();

        let output = materialize_hourly_seasonality_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T13:00:00Z"),
            parse_timestamp("2026-02-02T18:00:00Z"),
            &[
                parse_timestamp("2026-02-02T14:05:00Z")
                    .timestamp_nanos_opt()
                    .unwrap(),
                parse_timestamp("2026-02-02T14:20:00Z")
                    .timestamp_nanos_opt()
                    .unwrap(),
                parse_timestamp("2026-02-02T15:10:00Z")
                    .timestamp_nanos_opt()
                    .unwrap(),
                parse_timestamp("2026-02-02T17:15:00Z")
                    .timestamp_nanos_opt()
                    .unwrap(),
            ],
            &[1.2, 1.0, 0.8, 1.4],
            "previous_observation_carry_forward.v1",
            "mean_of_observations.v1",
            "previous_materialized_then_global_mean.v1",
            2,
        )
        .expect("seasonality materialization should succeed");

        assert_eq!(
            output.schema_version,
            HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION
        );
        assert_eq!(output.s_h, vec![1.1, 1.1, 0.8, 0.8, 1.4]);
        assert_eq!(output.diagnostics.source_point_count, 4);
        assert_eq!(output.diagnostics.interval_count, 5);
        assert_eq!(output.diagnostics.sparse_interval_count, 4);
        assert_eq!(output.diagnostics.interpolated_interval_count, 3);
        assert_eq!(output.diagnostics.fallback_interval_count, 1);
        assert_eq!(
            output.metadata.sparse_fallback_policy,
            "previous_materialized_then_global_mean.v1"
        );
        assert_eq!(
            output.metadata.timezone_transition_policy,
            HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY
        );
        assert_eq!(output.diagnostics.skipped_local_hour_interval_count, 0);
        assert_eq!(output.diagnostics.repeated_local_hour_interval_count, 0);
    }

    #[test]
    fn seasonality_materializer_is_deterministic_for_identical_inputs() {
        let registry = build_calendar_registry();

        let source_timestamp_utc_nanos = vec![
            parse_timestamp("2026-02-02T15:05:00Z")
                .timestamp_nanos_opt()
                .unwrap(),
            parse_timestamp("2026-02-02T14:20:00Z")
                .timestamp_nanos_opt()
                .unwrap(),
            parse_timestamp("2026-02-02T16:10:00Z")
                .timestamp_nanos_opt()
                .unwrap(),
            parse_timestamp("2026-02-02T15:30:00Z")
                .timestamp_nanos_opt()
                .unwrap(),
            parse_timestamp("2026-02-02T14:05:00Z")
                .timestamp_nanos_opt()
                .unwrap(),
            parse_timestamp("2026-02-02T16:50:00Z")
                .timestamp_nanos_opt()
                .unwrap(),
        ];
        let source_seasonality_factors = vec![0.9, 1.2, 1.4, 1.1, 1.0, 1.6];

        let first_output = materialize_hourly_seasonality_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T17:00:00Z"),
            &source_timestamp_utc_nanos,
            &source_seasonality_factors,
            "linear_between_observations.v1",
            "last_observation.v1",
            "global_mean.v1",
            2,
        )
        .expect("seasonality materialization should succeed");

        let second_output = materialize_hourly_seasonality_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T17:00:00Z"),
            &source_timestamp_utc_nanos,
            &source_seasonality_factors,
            "linear_between_observations.v1",
            "last_observation.v1",
            "global_mean.v1",
            2,
        )
        .expect("seasonality materialization should succeed");

        assert_eq!(first_output.s_h, vec![1.2, 1.1, 1.6]);
        assert_eq!(first_output, second_output);
    }

    #[test]
    fn seasonality_materializer_assigns_transition_boundary_observations_deterministically() {
        let registry = build_calendar_registry();

        let spring_forward_output = materialize_hourly_seasonality_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-03-08T06:00:00Z"),
            parse_timestamp("2026-03-08T09:00:00Z"),
            &[
                parse_timestamp("2026-03-08T06:30:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
                parse_timestamp("2026-03-08T07:00:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
                parse_timestamp("2026-03-08T08:00:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
            ],
            &[1.0, 2.0, 3.0],
            "previous_observation_carry_forward.v1",
            "last_observation.v1",
            "global_mean.v1",
            1,
        )
        .expect("seasonality materialization should succeed");

        assert_eq!(spring_forward_output.s_h, vec![1.0, 2.0, 3.0]);
        assert_eq!(
            spring_forward_output
                .diagnostics
                .skipped_local_hour_interval_count,
            1
        );
        assert_eq!(
            spring_forward_output
                .diagnostics
                .repeated_local_hour_interval_count,
            0
        );

        let fall_back_output = materialize_hourly_seasonality_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-11-01T05:00:00Z"),
            parse_timestamp("2026-11-01T08:00:00Z"),
            &[
                parse_timestamp("2026-11-01T05:00:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
                parse_timestamp("2026-11-01T06:00:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
                parse_timestamp("2026-11-01T07:00:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
            ],
            &[1.5, 2.5, 3.5],
            "previous_observation_carry_forward.v1",
            "last_observation.v1",
            "global_mean.v1",
            1,
        )
        .expect("seasonality materialization should succeed");

        assert_eq!(fall_back_output.s_h, vec![1.5, 2.5, 3.5]);
        assert_eq!(
            fall_back_output
                .diagnostics
                .skipped_local_hour_interval_count,
            0
        );
        assert_eq!(
            fall_back_output
                .diagnostics
                .repeated_local_hour_interval_count,
            1
        );
    }

    #[test]
    fn news_intensity_materializer_supports_known_only_component() {
        let registry = build_calendar_registry();

        let output = materialize_hourly_news_intensity_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T17:00:00Z"),
            &[
                parse_timestamp("2026-02-02T14:05:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
                parse_timestamp("2026-02-02T15:10:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
            ],
            0.0,
            &[],
        )
        .expect("news intensity materialization should succeed");

        assert_eq!(
            output.schema_version,
            HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION
        );
        assert_eq!(output.known_component_nu_h, vec![1.0, 1.0, 0.0]);
        assert_eq!(output.surprise_component_nu_h, vec![0.0, 0.0, 0.0]);
        assert_eq!(output.nu_h, vec![1.0, 1.0, 0.0]);
        assert_eq!(
            output.metadata.materialization_method,
            DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD
        );
        assert_eq!(
            output.metadata.component_aggregation_policy,
            DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY
        );
        assert_eq!(
            output.metadata.known_news_calendar_anchor_policy,
            DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY
        );
        assert_eq!(
            output.metadata.surprise_news_intensity_control_policy,
            DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY
        );
        assert_eq!(output.metadata.known_news_calendar_anchor_count, 2);
        assert_eq!(output.metadata.surprise_baseline_intensity, 0.0);
        assert_eq!(output.diagnostics.interval_count, 3);
        assert_eq!(
            output.diagnostics.known_component_non_zero_interval_count,
            2
        );
        assert_eq!(
            output
                .diagnostics
                .surprise_component_non_zero_interval_count,
            0
        );
        assert_eq!(
            output
                .diagnostics
                .combined_component_non_zero_interval_count,
            2
        );
        assert_eq!(output.diagnostics.known_scheduled_arrival_count, 2);
        assert_eq!(
            output
                .diagnostics
                .surprise_multiplier_override_interval_count,
            0
        );
        assert_eq!(
            output.metadata.timezone_transition_policy,
            HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY
        );
        assert_eq!(output.diagnostics.skipped_local_hour_interval_count, 0);
        assert_eq!(output.diagnostics.repeated_local_hour_interval_count, 0);
    }

    #[test]
    fn news_intensity_materializer_supports_surprise_only_component() {
        let registry = build_calendar_registry();

        let output = materialize_hourly_news_intensity_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-07T00:00:00Z"),
            parse_timestamp("2026-02-07T03:00:00Z"),
            &[],
            0.4,
            &[(0, 1.0), (1, 0.5), (2, 2.0)],
        )
        .expect("surprise-only news intensity materialization should succeed");

        assert_eq!(output.known_component_nu_h, vec![0.0, 0.0, 0.0]);
        assert_eq!(output.surprise_component_nu_h, vec![0.4, 0.2, 0.8]);
        assert_eq!(output.nu_h, vec![0.4, 0.2, 0.8]);
        assert_eq!(
            output.diagnostics.known_component_non_zero_interval_count,
            0
        );
        assert_eq!(
            output
                .diagnostics
                .surprise_component_non_zero_interval_count,
            3
        );
        assert_eq!(
            output
                .diagnostics
                .combined_component_non_zero_interval_count,
            3
        );
        assert_eq!(
            output
                .diagnostics
                .surprise_multiplier_override_interval_count,
            3
        );
    }

    #[test]
    fn news_intensity_materializer_supports_combined_known_and_surprise_components() {
        let registry = build_calendar_registry();

        let output = materialize_hourly_news_intensity_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T17:00:00Z"),
            &[
                parse_timestamp("2026-02-02T14:05:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
                parse_timestamp("2026-02-02T15:10:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
            ],
            0.1,
            &[(0, 1.0), (1, 2.0), (2, 1.0)],
        )
        .expect("combined known and surprise materialization should succeed");

        assert_eq!(output.known_component_nu_h, vec![1.0, 1.0, 0.0]);
        assert_eq!(output.surprise_component_nu_h, vec![0.1, 0.2, 0.1]);
        assert_eq!(output.nu_h, vec![1.1, 1.2, 0.1]);
        assert_eq!(
            output.diagnostics.known_component_non_zero_interval_count,
            2
        );
        assert_eq!(
            output
                .diagnostics
                .surprise_component_non_zero_interval_count,
            3
        );
        assert_eq!(
            output
                .diagnostics
                .combined_component_non_zero_interval_count,
            3
        );
    }

    #[test]
    fn news_intensity_materializer_rejects_non_increasing_known_calendar_anchor_input() {
        let registry = build_calendar_registry();

        let non_increasing_error = materialize_hourly_news_intensity_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T16:00:00Z"),
            &[
                parse_timestamp("2026-02-02T15:10:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
                parse_timestamp("2026-02-02T14:05:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
            ],
            0.4,
            &[],
        )
        .expect_err("non-increasing known timestamps must fail");

        assert!(matches!(
            non_increasing_error,
            HourlyFactorContractError::KnownScheduledTimestampsNotStrictlyIncreasing { .. }
        ));
    }

    #[test]
    fn news_intensity_materializer_rejects_invalid_surprise_intensity_controls() {
        let registry = build_calendar_registry();

        let baseline_error = materialize_hourly_news_intensity_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T16:00:00Z"),
            &[],
            -0.1,
            &[],
        )
        .expect_err("negative surprise baseline must fail");

        assert_eq!(
            baseline_error,
            HourlyFactorContractError::InvalidSurpriseBaselineIntensity { value: -0.1 }
        );

        let multiplier_error = materialize_hourly_news_intensity_factors(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T16:00:00Z"),
            &[],
            0.5,
            &[(0, -0.2)],
        )
        .expect_err("negative surprise multiplier must fail");

        assert_eq!(
            multiplier_error,
            HourlyFactorContractError::InvalidSurpriseDeterministicTimeMultiplier {
                interval_index: 0,
                value: -0.2,
            }
        );
        assert!(matches!(
            materialize_hourly_news_intensity_factors(
                &registry,
                "XNYS",
                parse_timestamp("2026-02-02T14:00:00Z"),
                parse_timestamp("2026-02-02T16:00:00Z"),
                &[],
                0.5,
                &[(2, 1.0)],
            )
            .expect_err("out-of-range surprise multiplier interval index must fail"),
            HourlyFactorContractError::SurpriseDeterministicTimeMultiplierIntervalOutOfRange {
                interval_index: 2,
                interval_count: 2
            }
        ));
    }

    fn build_calendar_registry() -> crate::calendar::ExchangeCalendarRegistry {
        ExchangeCalendarRegistryLoader::load_from_json(
            r#"
            {
              "schema_version": "calendar.registry.v1",
              "artifact_date": "2026-01-31",
              "exchanges": [
                {
                  "schema_version": "calendar.v1",
                  "exchange_code": "XNYS",
                  "exchange_name": "New York Stock Exchange",
                  "timezone_name": "America/New_York",
                  "rule_metadata": {
                    "holiday_calendar_code": "US_EQUITIES",
                    "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
                  },
                  "regular_sessions": [
                    {
                      "weekday": "Monday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Tuesday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Wednesday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Thursday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Friday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    }
                  ],
                  "holiday_dates": [],
                  "early_close_sessions": []
                }
              ]
            }
            "#,
        )
        .expect("fixture calendar registry should load")
    }

    fn parse_timestamp(raw_timestamp: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw_timestamp)
            .expect("timestamp fixture must parse")
            .with_timezone(&Utc)
    }
}
