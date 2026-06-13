use crate::calendar::ExchangeCalendarRegistry;
use crate::interval_factor_grids::{
    IntervalFactorGrid, IntervalFactorGridBuildInput, IntervalFactorGridError,
    IntervalNewsIntensityMaterializationInput, IntervalSeasonalityMaterializationInput,
    IntervalSeasonalityMaterializationPolicy, IntervalSeasonalitySourceSignal,
    build_interval_factor_grid, materialize_interval_news_intensity_factors,
    materialize_interval_seasonality_factors,
};
use chrono::{DateTime, Utc};

use super::constants::{
    INTERVAL_FACTOR_GRID_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    INTERVAL_FACTOR_GRID_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD,
};
use super::error::IntervalFactorGridProviderError;
use super::request::{
    IntervalFactorGridProviderRequest, IntervalNewsIntensityFactorSource,
    IntervalSeasonalityFactorSource,
};

/// Resolve interval-aligned interval factor grids through one shared provider API.
pub fn provide_interval_factor_grid(
    calendar_registry: &ExchangeCalendarRegistry,
    provider_request: IntervalFactorGridProviderRequest<'_>,
) -> Result<IntervalFactorGrid, IntervalFactorGridProviderError> {
    let IntervalFactorGridProviderRequest {
        exchange_code,
        range_start_utc,
        range_end_utc,
        interval_duration_nanos,
        seasonality_source,
        news_intensity_source,
    } = provider_request;

    let resolved_seasonality_source =
        seasonality_source.ok_or(IntervalFactorGridProviderError::MissingSeasonalitySource)?;

    let resolved_news_intensity_source =
        news_intensity_source.ok_or(IntervalFactorGridProviderError::MissingNewsIntensitySource)?;

    let (seasonality_interval_factors, seasonality_materialization_method) =
        match resolved_seasonality_source {
            IntervalSeasonalityFactorSource::Explicit {
                interval_factors,
                materialization_method,
            } => (
                validate_explicit_interval_factors_for_requested_intervals(
                    range_start_utc,
                    range_end_utc,
                    interval_duration_nanos,
                    interval_factors,
                )
                .map_err(map_contract_validation_error)?,
                materialization_method.to_string(),
            ),
            IntervalSeasonalityFactorSource::Materialized {
                source_timestamp_utc_nanos,
                source_seasonality_factors,
                interpolation_method,
                aggregation_method,
                sparse_fallback_policy,
                sparse_interval_minimum_observation_count,
            } => {
                let seasonality_materialization_output = materialize_interval_seasonality_factors(
                    IntervalSeasonalityMaterializationInput {
                        calendar_registry,
                        exchange_code,
                        range_utc: range_start_utc..range_end_utc,
                        interval_duration_nanos,
                        source_signal: IntervalSeasonalitySourceSignal {
                            timestamp_utc_nanos: source_timestamp_utc_nanos,
                            seasonality_factors: source_seasonality_factors,
                        },
                        policy: IntervalSeasonalityMaterializationPolicy {
                            interpolation_method,
                            aggregation_method,
                            sparse_fallback_policy,
                            sparse_interval_minimum_observation_count,
                        },
                    },
                )
                .map_err(map_seasonality_source_error)?;

                (
                    seasonality_materialization_output.seasonality_factor,
                    INTERVAL_FACTOR_GRID_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD.to_string(),
                )
            }
        };

    let (
        news_intensity_interval_factors,
        known_news_intensity,
        surprise_news_intensity,
        news_intensity_materialization_method,
    ) = match resolved_news_intensity_source {
        IntervalNewsIntensityFactorSource::Explicit {
            interval_factors,
            materialization_method,
        } => {
            let resolved_interval_factors =
                validate_explicit_interval_factors_for_requested_intervals(
                    range_start_utc,
                    range_end_utc,
                    interval_duration_nanos,
                    interval_factors,
                )
                .map_err(map_contract_validation_error)?;

            (
                resolved_interval_factors.clone(),
                vec![0.0; resolved_interval_factors.len()],
                resolved_interval_factors,
                materialization_method.to_string(),
            )
        }
        IntervalNewsIntensityFactorSource::Materialized {
            known_scheduled_event_timestamps_utc_nanos,
            surprise_open_baseline_intensity,
            surprise_closed_baseline_intensity,
            surprise_deterministic_time_multiplier_by_interval_index,
        } => {
            let news_materialization_output = materialize_interval_news_intensity_factors(
                IntervalNewsIntensityMaterializationInput {
                    calendar_registry,
                    exchange_code,
                    range_utc: range_start_utc..range_end_utc,
                    interval_duration_nanos,
                    known_scheduled_event_timestamps_utc_nanos,
                    surprise_open_baseline_intensity,
                    surprise_closed_baseline_intensity,
                    surprise_deterministic_time_multiplier_by_interval_index,
                },
            )
            .map_err(map_news_source_error)?;

            (
                news_materialization_output.news_intensity,
                news_materialization_output.known_news_intensity,
                news_materialization_output.surprise_news_intensity,
                INTERVAL_FACTOR_GRID_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD.to_string(),
            )
        }
    };

    let mut interval_factor_grid = build_interval_factor_grid(IntervalFactorGridBuildInput {
        calendar_registry,
        exchange_code,
        range_utc: range_start_utc..range_end_utc,
        interval_duration_nanos,
        seasonality_factors: &seasonality_interval_factors,
        news_intensity_factors: &news_intensity_interval_factors,
        seasonality_materialization_method: &seasonality_materialization_method,
        news_intensity_materialization_method: &news_intensity_materialization_method,
    })
    .map_err(map_contract_validation_error)?;

    interval_factor_grid.known_news_intensity = known_news_intensity;
    interval_factor_grid.surprise_news_intensity = surprise_news_intensity;

    Ok(interval_factor_grid)
}

fn map_seasonality_source_error(
    source: IntervalFactorGridError,
) -> IntervalFactorGridProviderError {
    if is_unsupported_range_error(&source) {
        return IntervalFactorGridProviderError::UnsupportedRange { source };
    }

    IntervalFactorGridProviderError::SeasonalitySourceError { source }
}

fn map_news_source_error(source: IntervalFactorGridError) -> IntervalFactorGridProviderError {
    if is_unsupported_range_error(&source) {
        return IntervalFactorGridProviderError::UnsupportedRange { source };
    }

    IntervalFactorGridProviderError::NewsIntensitySourceError { source }
}

fn map_contract_validation_error(
    source: IntervalFactorGridError,
) -> IntervalFactorGridProviderError {
    if is_unsupported_range_error(&source) {
        return IntervalFactorGridProviderError::UnsupportedRange { source };
    }

    IntervalFactorGridProviderError::ContractValidationError { source }
}

fn is_unsupported_range_error(error: &IntervalFactorGridError) -> bool {
    matches!(
        error,
        IntervalFactorGridError::TimestampOutOfRange { .. }
            | IntervalFactorGridError::NonIncreasingRange { .. }
            | IntervalFactorGridError::InvalidIntervalDurationNanos { .. }
            | IntervalFactorGridError::RangeDurationNotDivisibleByInterval { .. }
    )
}

fn validate_explicit_interval_factors_for_requested_intervals(
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
    interval_factors: &[f64],
) -> Result<Vec<f64>, IntervalFactorGridError> {
    if interval_duration_nanos <= 0 {
        return Err(IntervalFactorGridError::InvalidIntervalDurationNanos {
            value: interval_duration_nanos,
        });
    }

    let range_start_utc_nanos = range_start_utc.timestamp_nanos_opt().ok_or(
        IntervalFactorGridError::TimestampOutOfRange {
            field: "range_start_utc",
        },
    )?;

    let range_end_utc_nanos = range_end_utc.timestamp_nanos_opt().ok_or(
        IntervalFactorGridError::TimestampOutOfRange {
            field: "range_end_utc",
        },
    )?;

    if range_start_utc_nanos >= range_end_utc_nanos {
        return Err(IntervalFactorGridError::NonIncreasingRange {
            range_start_utc_nanos,
            range_end_utc_nanos,
        });
    }

    let request_duration_nanos = range_end_utc_nanos - range_start_utc_nanos;
    let request_remainder_nanos = request_duration_nanos.rem_euclid(interval_duration_nanos);

    if request_remainder_nanos != 0 {
        return Err(
            IntervalFactorGridError::RangeDurationNotDivisibleByInterval {
                range_start_utc_nanos,
                range_end_utc_nanos,
                interval_duration_nanos,
                remainder_nanos: request_remainder_nanos,
            },
        );
    }

    let requested_interval_count = (request_duration_nanos / interval_duration_nanos) as usize;
    if interval_factors.len() != requested_interval_count {
        return Err(IntervalFactorGridError::FactorLengthMismatch {
            expected_interval_count: requested_interval_count,
            seasonality_factor_count: interval_factors.len(),
            news_intensity_factor_count: interval_factors.len(),
        });
    }

    Ok(interval_factors.to_vec())
}
