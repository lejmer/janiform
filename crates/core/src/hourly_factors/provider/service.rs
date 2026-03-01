use crate::calendar::ExchangeCalendarRegistry;
use crate::hourly_factors::{
    build_hourly_factor_contract, materialize_hourly_news_intensity_factors,
    materialize_hourly_seasonality_factors, HourlyFactorContractError, HourlyFactorContractOutput,
};

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
            } => (hourly_factors.to_vec(), materialization_method.to_string()),
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

    let (news_intensity_hourly_factors, news_intensity_materialization_method) =
        match resolved_news_intensity_source {
            HourlyNewsIntensityFactorSource::Explicit {
                hourly_factors,
                materialization_method,
            } => (hourly_factors.to_vec(), materialization_method.to_string()),
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
                    known_scheduled_event_timestamps_utc_nanos,
                    surprise_baseline_intensity,
                    surprise_deterministic_time_multiplier_by_interval_index,
                )
                .map_err(map_news_source_error)?;
                (
                    news_materialization_output.nu_h,
                    HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD.to_string(),
                )
            }
        };

    build_hourly_factor_contract(
        calendar_registry,
        exchange_code,
        range_start_utc,
        range_end_utc,
        &seasonality_hourly_factors,
        &news_intensity_hourly_factors,
        &seasonality_materialization_method,
        &news_intensity_materialization_method,
    )
    .map_err(map_contract_validation_error)
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
            | HourlyFactorContractError::RangeBoundaryNotHourAligned { .. }
    )
}
