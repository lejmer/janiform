use std::ops::Range;
use std::str::FromStr;

use chrono::{DateTime, Utc};

use crate::calendar::ExchangeCalendarRegistry;

use super::constants::{
    INTERVAL_FACTOR_GRID_FALL_BACK_POLICY, INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
    INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY, INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
    INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
    INTERVAL_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
};
use super::error::IntervalFactorGridError;
use super::interval_indexing::build_fixed_interval_indexing;
use super::types::{
    IntervalSeasonalityMaterializationDiagnostics, IntervalSeasonalityMaterializationMetadata,
    IntervalSeasonalityMaterializationOutput,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntervalSeasonalityInterpolationMethod {
    PreviousObservationCarryForward,
    LinearBetweenObservations,
}

impl IntervalSeasonalityInterpolationMethod {
    fn as_policy_identifier(self) -> &'static str {
        match self {
            Self::PreviousObservationCarryForward => "previous_observation_carry_forward.v1",
            Self::LinearBetweenObservations => "linear_between_observations.v1",
        }
    }
}

impl FromStr for IntervalSeasonalityInterpolationMethod {
    type Err = IntervalFactorGridError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        match source {
            "previous_observation_carry_forward.v1" => Ok(Self::PreviousObservationCarryForward),
            "linear_between_observations.v1" => Ok(Self::LinearBetweenObservations),
            _ => Err(
                IntervalFactorGridError::InvalidSeasonalityInterpolationMethod {
                    method: source.to_string(),
                },
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntervalSeasonalityAggregationMethod {
    MeanOfObservations,
    LastObservation,
}

impl IntervalSeasonalityAggregationMethod {
    fn as_policy_identifier(self) -> &'static str {
        match self {
            Self::MeanOfObservations => "mean_of_observations.v1",
            Self::LastObservation => "last_observation.v1",
        }
    }
}

impl FromStr for IntervalSeasonalityAggregationMethod {
    type Err = IntervalFactorGridError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        match source {
            "mean_of_observations.v1" => Ok(Self::MeanOfObservations),
            "last_observation.v1" => Ok(Self::LastObservation),
            _ => Err(
                IntervalFactorGridError::InvalidSeasonalityAggregationMethod {
                    method: source.to_string(),
                },
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntervalSeasonalitySparseFallbackPolicy {
    GlobalMean,
    PreviousMaterializedThenGlobalMean,
}

impl IntervalSeasonalitySparseFallbackPolicy {
    fn as_policy_identifier(self) -> &'static str {
        match self {
            Self::GlobalMean => "global_mean.v1",
            Self::PreviousMaterializedThenGlobalMean => "previous_materialized_then_global_mean.v1",
        }
    }
}

impl FromStr for IntervalSeasonalitySparseFallbackPolicy {
    type Err = IntervalFactorGridError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        match source {
            "global_mean.v1" => Ok(Self::GlobalMean),
            "previous_materialized_then_global_mean.v1" => {
                Ok(Self::PreviousMaterializedThenGlobalMean)
            }
            _ => Err(
                IntervalFactorGridError::InvalidSeasonalitySparseFallbackPolicy {
                    policy: source.to_string(),
                },
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IntervalSeasonalityMaterializationSettings {
    interpolation_method: IntervalSeasonalityInterpolationMethod,
    aggregation_method: IntervalSeasonalityAggregationMethod,
    sparse_fallback_policy: IntervalSeasonalitySparseFallbackPolicy,
    sparse_interval_minimum_observation_count: usize,
}

impl IntervalSeasonalityMaterializationSettings {
    fn from_policy_identifiers(
        interpolation_method: &str,
        aggregation_method: &str,
        sparse_fallback_policy: &str,
        sparse_interval_minimum_observation_count: usize,
    ) -> Result<Self, IntervalFactorGridError> {
        if sparse_interval_minimum_observation_count < 1 {
            return Err(
                IntervalFactorGridError::InvalidSparseIntervalMinimumObservationCount {
                    sparse_interval_minimum_observation_count,
                },
            );
        }

        Ok(Self {
            interpolation_method: IntervalSeasonalityInterpolationMethod::from_str(
                interpolation_method,
            )?,
            aggregation_method: IntervalSeasonalityAggregationMethod::from_str(aggregation_method)?,
            sparse_fallback_policy: IntervalSeasonalitySparseFallbackPolicy::from_str(
                sparse_fallback_policy,
            )?,
            sparse_interval_minimum_observation_count,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SourceSeasonalityPoint {
    timestamp_utc_nanos: i64,
    seasonality_factor: f64,
    source_position: usize,
}

/// Materialize a continuous or high-resolution seasonality signal into interval seasonality.
///
/// Sparse or missing intervals use a deterministic fallback sequence:
/// - Attempt interpolation using the configured interpolation method.
/// - If interpolation is not possible, apply the configured sparse fallback policy.
/// - UTC bucket boundaries are authoritative during timezone transitions.
#[derive(Debug, Clone, Copy)]
pub struct IntervalSeasonalitySourceSignal<'a> {
    pub timestamp_utc_nanos: &'a [i64],
    pub seasonality_factors: &'a [f64],
}

#[derive(Debug, Clone, Copy)]
pub struct IntervalSeasonalityMaterializationPolicy<'a> {
    pub interpolation_method: &'a str,
    pub aggregation_method: &'a str,
    pub sparse_fallback_policy: &'a str,
    pub sparse_interval_minimum_observation_count: usize,
}

#[derive(Debug, Clone)]
pub struct IntervalSeasonalityMaterializationInput<'a> {
    pub calendar_registry: &'a ExchangeCalendarRegistry,
    pub exchange_code: &'a str,
    pub range_utc: Range<DateTime<Utc>>,
    pub interval_duration_nanos: i64,
    pub source_signal: IntervalSeasonalitySourceSignal<'a>,
    pub policy: IntervalSeasonalityMaterializationPolicy<'a>,
}

pub fn materialize_interval_seasonality_factors(
    input: IntervalSeasonalityMaterializationInput<'_>,
) -> Result<IntervalSeasonalityMaterializationOutput, IntervalFactorGridError> {
    let IntervalSeasonalityMaterializationInput {
        calendar_registry,
        exchange_code,
        range_utc,
        interval_duration_nanos,
        source_signal,
        policy,
    } = input;

    let settings = IntervalSeasonalityMaterializationSettings::from_policy_identifiers(
        policy.interpolation_method,
        policy.aggregation_method,
        policy.sparse_fallback_policy,
        policy.sparse_interval_minimum_observation_count,
    )?;

    if source_signal.timestamp_utc_nanos.len() != source_signal.seasonality_factors.len() {
        return Err(IntervalFactorGridError::SourceLengthMismatch {
            source_timestamp_count: source_signal.timestamp_utc_nanos.len(),
            source_factor_count: source_signal.seasonality_factors.len(),
        });
    }

    if source_signal.timestamp_utc_nanos.is_empty() {
        return Err(IntervalFactorGridError::EmptySeasonalitySourceSignal);
    }

    let mut source_points: Vec<SourceSeasonalityPoint> =
        Vec::with_capacity(source_signal.timestamp_utc_nanos.len());

    for (source_position, (timestamp_utc_nanos, seasonality_factor)) in source_signal
        .timestamp_utc_nanos
        .iter()
        .copied()
        .zip(source_signal.seasonality_factors.iter().copied())
        .enumerate()
    {
        if !seasonality_factor.is_finite() || seasonality_factor < 0.0 {
            return Err(IntervalFactorGridError::InvalidSeasonalitySourceFactor {
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

    let interval_indexing = build_fixed_interval_indexing(
        calendar_registry,
        exchange_code,
        range_utc.start,
        range_utc.end,
        interval_duration_nanos,
    )?;

    let mut materialized_seasonality_factor: Vec<f64> =
        Vec::with_capacity(interval_indexing.interval_count);

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
        let interval_factor = if source_points_in_interval.len()
            >= settings.sparse_interval_minimum_observation_count
        {
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
                    &materialized_seasonality_factor,
                    global_mean_factor,
                )
            }
        };

        if !interval_factor.is_finite() || interval_factor < 0.0 {
            return Err(IntervalFactorGridError::InvalidSeasonalityFactor {
                interval_index,
                value: interval_factor,
            });
        }

        materialized_seasonality_factor.push(interval_factor);
    }

    Ok(IntervalSeasonalityMaterializationOutput {
        schema_version: INTERVAL_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
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
        seasonality_factor: materialized_seasonality_factor,
        metadata: IntervalSeasonalityMaterializationMetadata {
            interpolation_method: settings.interpolation_method.as_policy_identifier(),
            aggregation_method: settings.aggregation_method.as_policy_identifier(),
            sparse_fallback_policy: settings.sparse_fallback_policy.as_policy_identifier(),
            sparse_interval_minimum_observation_count: settings
                .sparse_interval_minimum_observation_count,
            interval_boundary_policy: INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
            fall_back_policy: INTERVAL_FACTOR_GRID_FALL_BACK_POLICY,
        },
        diagnostics: IntervalSeasonalityMaterializationDiagnostics {
            source_point_count: source_points.len(),
            interval_count: interval_indexing.interval_count,
            interval_duration_nanos: interval_indexing.interval_duration_nanos,
            sparse_interval_count,
            interpolated_interval_count,
            fallback_interval_count,
            skipped_exchange_local_interval_count: interval_indexing
                .skipped_exchange_local_interval_count,
            repeated_exchange_local_interval_count: interval_indexing
                .repeated_exchange_local_interval_count,
        },
    })
}
fn aggregate_source_points_in_interval(
    source_points_in_interval: &[SourceSeasonalityPoint],
    aggregation_method: IntervalSeasonalityAggregationMethod,
) -> f64 {
    match aggregation_method {
        IntervalSeasonalityAggregationMethod::MeanOfObservations => {
            let summed_factor: f64 = source_points_in_interval
                .iter()
                .map(|source_point| source_point.seasonality_factor)
                .sum();
            summed_factor / source_points_in_interval.len() as f64
        }
        IntervalSeasonalityAggregationMethod::LastObservation => source_points_in_interval
            .last()
            .map(|source_point| source_point.seasonality_factor)
            .unwrap_or(1.0),
    }
}

fn interpolate_sparse_interval(
    sorted_source_points: &[SourceSeasonalityPoint],
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    interpolation_method: IntervalSeasonalityInterpolationMethod,
) -> Option<f64> {
    match interpolation_method {
        IntervalSeasonalityInterpolationMethod::PreviousObservationCarryForward => {
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
        IntervalSeasonalityInterpolationMethod::LinearBetweenObservations => {
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
    sparse_fallback_policy: IntervalSeasonalitySparseFallbackPolicy,
    materialized_seasonality_factor: &[f64],
    global_mean_factor: f64,
) -> f64 {
    match sparse_fallback_policy {
        IntervalSeasonalitySparseFallbackPolicy::GlobalMean => global_mean_factor,
        IntervalSeasonalitySparseFallbackPolicy::PreviousMaterializedThenGlobalMean => {
            materialized_seasonality_factor
                .last()
                .copied()
                .unwrap_or(global_mean_factor)
        }
    }
}
