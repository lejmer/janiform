use std::ops::Range;

use chrono::{DateTime, Utc};

use crate::calendar::ExchangeCalendarRegistry;

use super::constants::{
    INTERVAL_FACTOR_GRID_FALL_BACK_POLICY, INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
    INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY, INTERVAL_FACTOR_GRID_SCHEMA_VERSION,
    INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY, INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
};
use super::error::IntervalFactorGridError;
use super::interval_indexing::build_fixed_interval_indexing;
use super::types::{IntervalFactorGrid, IntervalFactorGridDiagnostics, IntervalFactorGridMetadata};

/// Build canonical factor output for an exchange and fixed-width UTC interval grid.
///
/// Interval policy:
/// - The request range is interpreted as `[range_start_utc, range_end_utc)`.
/// - `interval_duration_nanos` defines a consecutive fixed-width UTC grid anchored at
///   `range_start_utc`.
/// - Buckets are emitted as consecutive non-overlapping intervals `[start, end)`.
/// - `interval_indices` are zero-based and strictly increasing by one.
/// - Exchange-local interval labels are projections of UTC boundaries and include numeric offsets.
/// - Spring-forward transitions skip nonexistent local-hour labels.
/// - Fall-back transitions preserve repeated local-hour labels and disambiguate them by offset.
#[derive(Debug, Clone)]
pub struct IntervalFactorGridBuildInput<'a> {
    pub calendar_registry: &'a ExchangeCalendarRegistry,
    pub exchange_code: &'a str,
    pub range_utc: Range<DateTime<Utc>>,
    pub interval_duration_nanos: i64,
    pub seasonality_factors: &'a [f64],
    pub news_intensity_factors: &'a [f64],
    pub seasonality_materialization_method: &'a str,
    pub news_intensity_materialization_method: &'a str,
}

pub fn build_interval_factor_grid(
    input: IntervalFactorGridBuildInput<'_>,
) -> Result<IntervalFactorGrid, IntervalFactorGridError> {
    let IntervalFactorGridBuildInput {
        calendar_registry,
        exchange_code,
        range_utc,
        interval_duration_nanos,
        seasonality_factors,
        news_intensity_factors,
        seasonality_materialization_method,
        news_intensity_materialization_method,
    } = input;

    if seasonality_materialization_method.trim().is_empty() {
        return Err(IntervalFactorGridError::EmptySeasonalityMaterializationMethod);
    }
    if news_intensity_materialization_method.trim().is_empty() {
        return Err(IntervalFactorGridError::EmptyNewsIntensityMaterializationMethod);
    }

    let interval_indexing = build_fixed_interval_indexing(
        calendar_registry,
        exchange_code,
        range_utc.start,
        range_utc.end,
        interval_duration_nanos,
    )?;

    let interval_count = interval_indexing.interval_count;
    if seasonality_factors.len() != interval_count || news_intensity_factors.len() != interval_count
    {
        return Err(IntervalFactorGridError::FactorLengthMismatch {
            expected_interval_count: interval_count,
            seasonality_factor_count: seasonality_factors.len(),
            news_intensity_factor_count: news_intensity_factors.len(),
        });
    }

    for (interval_index, seasonality_factor) in seasonality_factors.iter().copied().enumerate() {
        if !seasonality_factor.is_finite() || seasonality_factor < 0.0 {
            return Err(IntervalFactorGridError::InvalidSeasonalityFactor {
                interval_index,
                value: seasonality_factor,
            });
        }
    }

    for (interval_index, news_intensity_factor) in
        news_intensity_factors.iter().copied().enumerate()
    {
        if !news_intensity_factor.is_finite() || news_intensity_factor < 0.0 {
            return Err(IntervalFactorGridError::InvalidNewsIntensityFactor {
                interval_index,
                value: news_intensity_factor,
            });
        }
    }

    Ok(IntervalFactorGrid {
        schema_version: INTERVAL_FACTOR_GRID_SCHEMA_VERSION,
        exchange_code: interval_indexing.exchange_code.clone(),
        exchange_timezone_name: interval_indexing.exchange_timezone_name.clone(),
        request_start_utc_nanos: interval_indexing.request_start_utc_nanos,
        request_end_utc_nanos: interval_indexing.request_end_utc_nanos,
        interval_duration_nanos: interval_indexing.interval_duration_nanos,
        interval_indices: interval_indexing.interval_indices.clone(),
        interval_start_utc_nanos: interval_indexing.interval_start_utc_nanos.clone(),
        interval_end_utc_nanos: interval_indexing.interval_end_utc_nanos.clone(),
        interval_start_exchange_local: interval_indexing.interval_start_exchange_local.clone(),
        interval_end_exchange_local: interval_indexing.interval_end_exchange_local.clone(),
        seasonality_factor: seasonality_factors.to_vec(),
        known_news_intensity: vec![0.0; interval_count],
        surprise_news_intensity: news_intensity_factors.to_vec(),
        news_intensity: news_intensity_factors.to_vec(),
        metadata: IntervalFactorGridMetadata {
            seasonality_materialization_method: seasonality_materialization_method.to_string(),
            news_intensity_materialization_method: news_intensity_materialization_method
                .to_string(),
            interval_boundary_policy: INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
            fall_back_policy: INTERVAL_FACTOR_GRID_FALL_BACK_POLICY,
        },
        diagnostics: IntervalFactorGridDiagnostics {
            interval_count,
            interval_duration_nanos: interval_indexing.interval_duration_nanos,
            first_interval_start_utc_nanos: interval_indexing.request_start_utc_nanos,
            last_interval_end_utc_nanos: interval_indexing.request_end_utc_nanos,
            skipped_exchange_local_interval_count: interval_indexing
                .skipped_exchange_local_interval_count,
            repeated_exchange_local_interval_count: interval_indexing
                .repeated_exchange_local_interval_count,
        },
    })
}
