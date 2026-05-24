use std::error::Error;
use std::fmt::{Display, Formatter};

/// Validation failures and contract build errors for hourly factor outputs.
#[derive(Debug, Clone, PartialEq)]
pub enum HourlyFactorContractError {
    UnexpectedContractFieldValue {
        field: &'static str,
        expected: &'static str,
        observed: String,
    },
    EmptyExchangeCode,
    EmptySeasonalityMaterializationMethod,
    EmptyNewsIntensityMaterializationMethod,
    InvalidIntervalDurationNanos {
        value: i64,
    },
    TimestampOutOfRange {
        field: &'static str,
    },
    NonIncreasingRange {
        range_start_utc_nanos: i64,
        range_end_utc_nanos: i64,
    },
    RangeDurationNotDivisibleByInterval {
        range_start_utc_nanos: i64,
        range_end_utc_nanos: i64,
        interval_duration_nanos: i64,
        remainder_nanos: i64,
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
    IntervalFieldLengthMismatch {
        field: &'static str,
        expected_length: usize,
        actual_length: usize,
    },
    IntervalStartMismatch {
        field: &'static str,
        expected: i64,
        actual: i64,
    },
    IntervalIndexMismatch {
        interval_position: usize,
        expected_index: u64,
        actual_index: u64,
    },
    InvalidIntervalBounds {
        interval_position: usize,
        start_utc_nanos: i64,
        end_utc_nanos: i64,
        interval_duration_nanos: i64,
    },
    NonContiguousIntervals {
        previous_interval_end_utc_nanos: i64,
        current_interval_start_utc_nanos: i64,
        interval_position: usize,
    },
}

impl Display for HourlyFactorContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedContractFieldValue {
                field,
                expected,
                observed,
            } => write!(
                formatter,
                "hourly factor contract field `{field}` must be `{expected}`, received `{observed}`"
            ),
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
            Self::InvalidIntervalDurationNanos { value } => write!(
                formatter,
                "interval_duration_nanos must be strictly positive, received {value}"
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
            Self::RangeDurationNotDivisibleByInterval {
                range_start_utc_nanos,
                range_end_utc_nanos,
                interval_duration_nanos,
                remainder_nanos,
            } => write!(
                formatter,
                "range [{range_start_utc_nanos}, {range_end_utc_nanos}) must partition exactly into interval_duration_nanos={interval_duration_nanos}, remainder={remainder_nanos}"
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
                "s_h must contain finite non-negative values, found {value} at interval index {interval_index}"
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
                "seasonality source factors must be finite non-negative values, found {value} at source position {source_position}"
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
            Self::IntervalFieldLengthMismatch {
                field,
                expected_length,
                actual_length,
            } => write!(
                formatter,
                "{field} must have length {expected_length}, received {actual_length}"
            ),
            Self::IntervalStartMismatch {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "hourly factor contract field `{field}` must equal {expected}, received {actual}"
            ),
            Self::IntervalIndexMismatch {
                interval_position,
                expected_index,
                actual_index,
            } => write!(
                formatter,
                "hourly factor contract interval index at position {interval_position} must be {expected_index}, received {actual_index}"
            ),
            Self::InvalidIntervalBounds {
                interval_position,
                start_utc_nanos,
                end_utc_nanos,
                interval_duration_nanos,
            } => write!(
                formatter,
                "hourly factor contract interval at position {interval_position} must have start < end and width {interval_duration_nanos}, received start={start_utc_nanos}, end={end_utc_nanos}"
            ),
            Self::NonContiguousIntervals {
                previous_interval_end_utc_nanos,
                current_interval_start_utc_nanos,
                interval_position,
            } => write!(
                formatter,
                "hourly factor contract intervals must be contiguous, found previous_end={previous_interval_end_utc_nanos} and current_start={current_interval_start_utc_nanos} at position {interval_position}"
            ),
        }
    }
}

impl Error for HourlyFactorContractError {}
