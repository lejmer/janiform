use crate::calendar::ExchangeCalendarRegistry;
use crate::hourly_factors::{
    HourlyFactorContractError, HourlyFactorContractOutput, build_hourly_factor_contract,
    materialize_hourly_news_intensity_factors, materialize_hourly_seasonality_factors,
};
use chrono::{DateTime, Utc};

use super::constants::{
    HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    HOURLY_FACTOR_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD,
};
use super::error::HourlyFactorProviderError;
use super::request::{
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
};

/// Resolve interval-aligned hourly factors through one shared provider API.
pub fn provide_hourly_factor_contract(
    calendar_registry: &ExchangeCalendarRegistry,
    provider_request: HourlyFactorProviderRequest<'_>,
) -> Result<HourlyFactorContractOutput, HourlyFactorProviderError> {
    let HourlyFactorProviderRequest {
        exchange_code,
        range_start_utc,
        range_end_utc,
        interval_duration_nanos,
        seasonality_source,
        news_intensity_source,
    } = provider_request;

    let resolved_seasonality_source =
        seasonality_source.ok_or(HourlyFactorProviderError::MissingSeasonalitySource)?;
    let resolved_news_intensity_source =
        news_intensity_source.ok_or(HourlyFactorProviderError::MissingNewsIntensitySource)?;

    let (seasonality_hourly_factors, seasonality_materialization_method) =
        match resolved_seasonality_source {
            HourlySeasonalityFactorSource::Explicit {
                hourly_factors,
                materialization_method,
            } => (
                resample_explicit_hourly_factors_to_requested_intervals(
                    range_start_utc,
                    range_end_utc,
                    interval_duration_nanos,
                    hourly_factors,
                )
                .map_err(map_contract_validation_error)?,
                materialization_method.to_string(),
            ),
            HourlySeasonalityFactorSource::Materialized {
                source_timestamp_utc_nanos,
                source_seasonality_factors,
                interpolation_method,
                aggregation_method,
                sparse_fallback_policy,
                sparse_hour_minimum_observation_count,
            } => {
                let seasonality_materialization_output = materialize_hourly_seasonality_factors(
                    calendar_registry,
                    exchange_code,
                    range_start_utc,
                    range_end_utc,
                    interval_duration_nanos,
                    source_timestamp_utc_nanos,
                    source_seasonality_factors,
                    interpolation_method,
                    aggregation_method,
                    sparse_fallback_policy,
                    sparse_hour_minimum_observation_count,
                )
                .map_err(map_seasonality_source_error)?;
                (
                    seasonality_materialization_output.s_h,
                    HOURLY_FACTOR_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD.to_string(),
                )
            }
        };

    let (
        news_intensity_hourly_factors,
        known_component_nu_h,
        surprise_component_nu_h,
        news_intensity_materialization_method,
    ) =
        match resolved_news_intensity_source {
            HourlyNewsIntensityFactorSource::Explicit {
                hourly_factors,
                materialization_method,
            } => {
                let resampled_hourly_factors = resample_explicit_hourly_factors_to_requested_intervals(
                    range_start_utc,
                    range_end_utc,
                    interval_duration_nanos,
                    hourly_factors,
                )
                .map_err(map_contract_validation_error)?;
                (
                    resampled_hourly_factors.clone(),
                    vec![0.0; resampled_hourly_factors.len()],
                    resampled_hourly_factors,
                    materialization_method.to_string(),
                )
            }
            HourlyNewsIntensityFactorSource::Materialized {
                known_scheduled_event_timestamps_utc_nanos,
                surprise_baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index,
            } => {
                let news_materialization_output = materialize_hourly_news_intensity_factors(
                    calendar_registry,
                    exchange_code,
                    range_start_utc,
                    range_end_utc,
                    interval_duration_nanos,
                    known_scheduled_event_timestamps_utc_nanos,
                    surprise_baseline_intensity,
                    surprise_deterministic_time_multiplier_by_interval_index,
                )
                .map_err(map_news_source_error)?;
                (
                    news_materialization_output.nu_h,
                    news_materialization_output.known_component_nu_h,
                    news_materialization_output.surprise_component_nu_h,
                    HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD.to_string(),
                )
            }
        };

    let mut hourly_factor_contract = build_hourly_factor_contract(
        calendar_registry,
        exchange_code,
        range_start_utc,
        range_end_utc,
        interval_duration_nanos,
        &seasonality_hourly_factors,
        &news_intensity_hourly_factors,
        &seasonality_materialization_method,
        &news_intensity_materialization_method,
    )
    .map_err(map_contract_validation_error)?;
    hourly_factor_contract.known_component_nu_h = known_component_nu_h;
    hourly_factor_contract.surprise_component_nu_h = surprise_component_nu_h;

    Ok(hourly_factor_contract)
}

fn map_seasonality_source_error(source: HourlyFactorContractError) -> HourlyFactorProviderError {
    if is_unsupported_range_error(&source) {
        return HourlyFactorProviderError::UnsupportedRange { source };
    }

    HourlyFactorProviderError::SeasonalitySourceError { source }
}

fn map_news_source_error(source: HourlyFactorContractError) -> HourlyFactorProviderError {
    if is_unsupported_range_error(&source) {
        return HourlyFactorProviderError::UnsupportedRange { source };
    }

    HourlyFactorProviderError::NewsIntensitySourceError { source }
}

fn map_contract_validation_error(source: HourlyFactorContractError) -> HourlyFactorProviderError {
    if is_unsupported_range_error(&source) {
        return HourlyFactorProviderError::UnsupportedRange { source };
    }

    HourlyFactorProviderError::ContractValidationError { source }
}

fn is_unsupported_range_error(error: &HourlyFactorContractError) -> bool {
    matches!(
        error,
        HourlyFactorContractError::TimestampOutOfRange { .. }
            | HourlyFactorContractError::NonIncreasingRange { .. }
            | HourlyFactorContractError::InvalidIntervalDurationNanos { .. }
            | HourlyFactorContractError::RangeDurationNotDivisibleByInterval { .. }
    )
}

fn resample_explicit_hourly_factors_to_requested_intervals(
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
    hourly_factors: &[f64],
) -> Result<Vec<f64>, HourlyFactorContractError> {
    if interval_duration_nanos <= 0 {
        return Err(HourlyFactorContractError::InvalidIntervalDurationNanos {
            value: interval_duration_nanos,
        });
    }

    let range_start_utc_nanos = range_start_utc.timestamp_nanos_opt().ok_or(
        HourlyFactorContractError::TimestampOutOfRange {
            field: "range_start_utc",
        },
    )?;
    let range_end_utc_nanos = range_end_utc.timestamp_nanos_opt().ok_or(
        HourlyFactorContractError::TimestampOutOfRange {
            field: "range_end_utc",
        },
    )?;
    if range_start_utc_nanos >= range_end_utc_nanos {
        return Err(HourlyFactorContractError::NonIncreasingRange {
            range_start_utc_nanos,
            range_end_utc_nanos,
        });
    }

    let request_duration_nanos = range_end_utc_nanos - range_start_utc_nanos;
    let request_remainder_nanos = request_duration_nanos.rem_euclid(interval_duration_nanos);
    if request_remainder_nanos != 0 {
        return Err(
            HourlyFactorContractError::RangeDurationNotDivisibleByInterval {
                range_start_utc_nanos,
                range_end_utc_nanos,
                interval_duration_nanos,
                remainder_nanos: request_remainder_nanos,
            },
        );
    }

    const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;

    let canonical_hour_start_utc_nanos =
        range_start_utc_nanos.div_euclid(ONE_HOUR_NANOS) * ONE_HOUR_NANOS;
    let canonical_hour_end_utc_nanos = if range_end_utc_nanos % ONE_HOUR_NANOS == 0 {
        range_end_utc_nanos
    } else {
        range_end_utc_nanos
            .div_euclid(ONE_HOUR_NANOS)
            .saturating_add(1)
            .saturating_mul(ONE_HOUR_NANOS)
    };
    let canonical_hour_count =
        ((canonical_hour_end_utc_nanos - canonical_hour_start_utc_nanos) / ONE_HOUR_NANOS) as usize;
    if hourly_factors.len() != canonical_hour_count {
        return Err(HourlyFactorContractError::FactorLengthMismatch {
            expected_interval_count: canonical_hour_count,
            seasonality_factor_count: hourly_factors.len(),
            news_intensity_factor_count: hourly_factors.len(),
        });
    }

    let requested_interval_count = (request_duration_nanos / interval_duration_nanos) as usize;
    let mut resampled_factors: Vec<f64> = Vec::with_capacity(requested_interval_count);

    for interval_index in 0..requested_interval_count {
        let interval_start_utc_nanos =
            range_start_utc_nanos + interval_index as i64 * interval_duration_nanos;
        let interval_end_utc_nanos = interval_start_utc_nanos + interval_duration_nanos;
        let mut weighted_factor_sum = 0.0_f64;

        let first_hour_index =
            ((interval_start_utc_nanos - canonical_hour_start_utc_nanos) / ONE_HOUR_NANOS) as usize;
        let last_hour_index = ((interval_end_utc_nanos - 1 - canonical_hour_start_utc_nanos)
            / ONE_HOUR_NANOS) as usize;

        for hour_index in first_hour_index..=last_hour_index {
            let hour_start_utc_nanos =
                canonical_hour_start_utc_nanos + hour_index as i64 * ONE_HOUR_NANOS;
            let hour_end_utc_nanos = hour_start_utc_nanos + ONE_HOUR_NANOS;
            let overlap_start_utc_nanos = interval_start_utc_nanos.max(hour_start_utc_nanos);
            let overlap_end_utc_nanos = interval_end_utc_nanos.min(hour_end_utc_nanos);
            let overlap_duration_nanos = overlap_end_utc_nanos - overlap_start_utc_nanos;
            if overlap_duration_nanos <= 0 {
                continue;
            }

            weighted_factor_sum += hourly_factors[hour_index]
                * (overlap_duration_nanos as f64 / interval_duration_nanos as f64);
        }

        resampled_factors.push(weighted_factor_sum);
    }

    Ok(resampled_factors)
}
