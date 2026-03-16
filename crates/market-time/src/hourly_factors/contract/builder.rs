use chrono::{DateTime, Utc};

use crate::calendar::ExchangeCalendarRegistry;

use super::constants::{
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION, HOURLY_FACTOR_FALL_BACK_POLICY,
    HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY, HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
    HOURLY_FACTOR_SPRING_FORWARD_POLICY, HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
};
use super::error::HourlyFactorContractError;
use super::interval_indexing::build_fixed_interval_indexing;
use super::types::{
    HourlyFactorContractDiagnostics, HourlyFactorContractOutput,
    HourlyFactorMaterializationMetadata,
};

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
pub fn build_hourly_factor_contract(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
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

    let interval_indexing = build_fixed_interval_indexing(
        calendar_registry,
        exchange_code,
        range_start_utc,
        range_end_utc,
        interval_duration_nanos,
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
        interval_duration_nanos: interval_indexing.interval_duration_nanos,
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
            interval_duration_nanos: interval_indexing.interval_duration_nanos,
            first_interval_start_utc_nanos: interval_indexing.request_start_utc_nanos,
            last_interval_end_utc_nanos: interval_indexing.request_end_utc_nanos,
            skipped_local_hour_interval_count: interval_indexing.skipped_local_hour_interval_count,
            repeated_local_hour_interval_count: interval_indexing
                .repeated_local_hour_interval_count,
        },
    })
}
