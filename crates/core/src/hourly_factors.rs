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

pub const HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION: &str = "seasonality.hourly_factor.contract.v1";
pub const HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY: &str = "start_inclusive_end_exclusive.v1";
pub const HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY: &str = "utc_hour_aligned.v1";
pub const DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD: &str = "explicit_hourly_input.v1";
pub const HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION: &str =
    "seasonality.hourly_s_h.materialization.v1";
pub const DEFAULT_SEASONALITY_INTERPOLATION_METHOD: &str = "previous_observation_carry_forward.v1";
pub const DEFAULT_SEASONALITY_AGGREGATION_METHOD: &str = "mean_of_observations.v1";
pub const DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY: &str = "global_mean.v1";
pub const DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT: usize = 1;

const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;
const ONE_SECOND_NANOS: i64 = 1_000_000_000;

/// Audit metadata emitted with hourly factor materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlyFactorMaterializationMetadata {
    pub seasonality_materialization_method: String,
    pub news_intensity_materialization_method: String,
    pub interval_boundary_policy: &'static str,
    pub interval_alignment_policy: &'static str,
}

/// Deterministic diagnostics for hourly factor interval coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlyFactorContractDiagnostics {
    pub interval_count: usize,
    pub first_interval_start_utc_nanos: i64,
    pub last_interval_end_utc_nanos: i64,
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
}

/// Deterministic diagnostics for hourly seasonality materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlySeasonalityMaterializationDiagnostics {
    pub source_point_count: usize,
    pub interval_count: usize,
    pub sparse_interval_count: usize,
    pub interpolated_interval_count: usize,
    pub fallback_interval_count: usize,
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
        },
        diagnostics: HourlyFactorContractDiagnostics {
            interval_count,
            first_interval_start_utc_nanos: interval_indexing.request_start_utc_nanos,
            last_interval_end_utc_nanos: interval_indexing.request_end_utc_nanos,
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
}

/// Materialize a continuous or high-resolution seasonality signal into hourly `s_h`.
///
/// Sparse or missing intervals use a deterministic fallback sequence:
/// - Attempt interpolation using the configured interpolation method.
/// - If interpolation is not possible, apply the configured sparse fallback policy.
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
        },
        diagnostics: HourlySeasonalityMaterializationDiagnostics {
            source_point_count: source_points.len(),
            interval_count: interval_indexing.interval_count,
            sparse_interval_count,
            interpolated_interval_count,
            fallback_interval_count,
        },
    })
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
    for interval_index in 0..interval_count {
        let interval_offset_nanos = interval_index as i64 * ONE_HOUR_NANOS;
        let interval_start_nanos = range_start_utc_nanos + interval_offset_nanos;
        let interval_end_nanos = interval_start_nanos + ONE_HOUR_NANOS;

        let start_utc = utc_timestamp_from_nanos(interval_start_nanos, "range_start_utc")?;
        let end_utc = utc_timestamp_from_nanos(interval_end_nanos, "range_end_utc")?;
        interval_indices.push(interval_index as u64);
        interval_start_utc_nanos.push(interval_start_nanos);
        interval_end_utc_nanos.push(interval_end_nanos);
        interval_start_exchange_local.push(start_utc.with_timezone(&timezone).to_rfc3339());
        interval_end_exchange_local.push(end_utc.with_timezone(&timezone).to_rfc3339());
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
    })
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
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY, HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
        HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION, HourlyFactorContractError,
        build_hourly_factor_contract, materialize_hourly_seasonality_factors,
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
            output.metadata.seasonality_materialization_method,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD
        );
        assert_eq!(
            output.metadata.news_intensity_materialization_method,
            "calendar_closed_window_baseline.v1"
        );
        assert_eq!(output.diagnostics.interval_count, 3);
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
