use chrono::{DateTime, Datelike, Duration as ChronoDuration, Timelike, Utc};
use janiform::calibration::staged::materialize_runtime_structural_seasonality;
use janiform::model::factors::interval_factor_grid::{
    DEFAULT_SEASONALITY_AGGREGATION_METHOD, DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
    DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY, DEFAULT_SPARSE_INTERVAL_MINIMUM_OBSERVATION_COUNT,
    IntervalFactorGrid, IntervalFactorGridProviderRequest, IntervalNewsIntensityFactorSource,
    IntervalSeasonalityFactorSource, NANOS_PER_HOUR,
};
use janiform_calibration_runtime::RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD;
use janiform_foundations::time::parse_utc_datetime;
use janiform_market_time::business_time_clock::FourierBusinessTimeClockModel;
use serde_json::Value;

use crate::error::WorkflowError;

use super::context::{
    FittedModelContext, PersistedModelPayload, interval_span_nanos, validation_error,
};

const SURPRISE_MULTIPLIER_HOURS_PER_WEEK: usize = 168;

pub(crate) fn build_interval_factor_grid(
    context: &FittedModelContext,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
) -> Result<IntervalFactorGrid, WorkflowError> {
    let seasonality_grid = materialize_runtime_seasonality_grid(
        context,
        range_start_utc,
        range_end_utc,
        interval_duration_nanos,
    )?;

    let seasonality_source = interval_seasonality_source(
        range_start_utc,
        range_end_utc,
        interval_duration_nanos,
        &seasonality_grid,
    );

    let known_news_timestamps = known_news_timestamp_utc_nanos(&context.model)?;

    let surprise_multipliers = surprise_deterministic_time_multiplier_by_interval_index(
        range_start_utc,
        range_end_utc,
        interval_duration_nanos,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .surprise_deterministic_intensity_profile
            .weekly_hour_utc_multipliers
            .as_slice(),
    )?;

    let surprise_arrival_configuration = context
        .fitted_parameters
        .return_components_fitted_parameters
        .news_process_configuration
        .arrivals
        .surprise;

    janiform_calibration_runtime::load_interval_factor_grid_for_calibration(
        &context.calendar_registry,
        IntervalFactorGridProviderRequest {
            exchange_code: context.model.session_resolution.exchange_code.as_str(),
            range_start_utc,
            range_end_utc,
            interval_duration_nanos,
            seasonality_source: Some(seasonality_source),
            news_intensity_source: Some(IntervalNewsIntensityFactorSource::Materialized {
                known_scheduled_event_timestamps_utc_nanos: known_news_timestamps.as_slice(),
                surprise_open_baseline_intensity: surprise_arrival_configuration
                    .open_baseline_intensity,
                surprise_closed_baseline_intensity: surprise_arrival_configuration
                    .closed_baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index: surprise_multipliers
                    .as_slice(),
            }),
        },
    )
    .map_err(|source| validation_error("interval_factor_grid", source.to_string()))
}

fn materialize_runtime_seasonality_grid(
    context: &FittedModelContext,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
) -> Result<(Vec<i64>, Vec<f64>), WorkflowError> {
    let seasonality_parameters = context.model.parameters.get("seasonality").ok_or_else(|| {
        validation_error(
            "model_payload.parameters.seasonality",
            "missing compact seasonality parameters",
        )
    })?;

    let clock_model = seasonality_clock_model(seasonality_parameters)?;

    let output = materialize_runtime_structural_seasonality(
        &context.calendar_registry,
        janiform_calibration_runtime::RuntimeStructuralSeasonalityMaterializationInput {
            // Intentional copy because the materialization input owns its exchange identifier.
            exchange_code: context.model.session_resolution.exchange_code.clone(),
            range_start_utc_nanos: range_start_utc.timestamp_nanos_opt().ok_or_else(|| {
                validation_error(
                    "range_start_utc",
                    "timestamp is outside the supported nanosecond range",
                )
            })?,
            range_end_utc_nanos: range_end_utc.timestamp_nanos_opt().ok_or_else(|| {
                validation_error(
                    "range_end_utc",
                    "timestamp is outside the supported nanosecond range",
                )
            })?,
            interval_duration_nanos,
            clock_model,
        },
    )
    .map_err(|source| validation_error("seasonality", source.to_string()))?;

    Ok((output.interval_start_utc_nanos, output.seasonality_factors))
}

fn interval_seasonality_source(
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
    seasonality_grid: &'_ (Vec<i64>, Vec<f64>),
) -> IntervalSeasonalityFactorSource<'_> {
    if seasonality_grid.1.is_empty() {
        return IntervalSeasonalityFactorSource::Explicit {
            interval_factors: seasonality_grid.1.as_slice(),
            materialization_method: RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
        };
    }

    if interval_duration_nanos != NANOS_PER_HOUR
        || !runtime_interval_grid_is_hour_aligned(range_start_utc, range_end_utc)
    {
        return IntervalSeasonalityFactorSource::Materialized {
            source_timestamp_utc_nanos: seasonality_grid.0.as_slice(),
            source_seasonality_factors: seasonality_grid.1.as_slice(),
            interpolation_method: DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
            aggregation_method: DEFAULT_SEASONALITY_AGGREGATION_METHOD,
            sparse_fallback_policy: DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
            sparse_interval_minimum_observation_count:
                DEFAULT_SPARSE_INTERVAL_MINIMUM_OBSERVATION_COUNT,
        };
    }

    IntervalSeasonalityFactorSource::Explicit {
        interval_factors: seasonality_grid.1.as_slice(),
        materialization_method: RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
    }
}

fn seasonality_clock_model(
    seasonality_parameters: &Value,
) -> Result<FourierBusinessTimeClockModel, WorkflowError> {
    let clock_model_value = seasonality_parameters
        .get("clock_model")
        .ok_or_else(|| validation_error("seasonality.clock_model", "must be present"))?;

    // Intentional copy because serde_json deserialization owns its input value.
    serde_json::from_value(clock_model_value.clone()).map_err(|source| {
        validation_error(
            "seasonality.clock_model",
            format!("failed to deserialize clock model: {source}"),
        )
    })
}

fn known_news_timestamp_utc_nanos(
    model: &PersistedModelPayload,
) -> Result<Vec<i64>, WorkflowError> {
    let Some(events) = model
        .resolved_configuration
        .pointer("/known_news/configuration/events")
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };

    events
        .iter()
        .enumerate()
        .map(|(event_index, event)| {
            let scheduled_at = event
                .get("scheduled_at")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    validation_error(
                        "model_payload.resolved_configuration.known_news.configuration.events",
                        format!("event at position {event_index} is missing scheduled_at"),
                    )
                })?;

            let timestamp = parse_utc_datetime(scheduled_at).map_err(|source| {
                validation_error(
                    "model_payload.resolved_configuration.known_news.configuration.events.scheduled_at",
                    source.to_string(),
                )
            })?;

            timestamp.timestamp_nanos_opt().ok_or_else(|| {
                validation_error(
                    "model_payload.resolved_configuration.known_news.configuration.events.scheduled_at",
                    "timestamp is outside the supported nanosecond range",
                )
            })
        })
        .collect()
}

fn surprise_deterministic_time_multiplier_by_interval_index(
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
    weekly_hour_utc_multipliers: &[f64],
) -> Result<Vec<(u64, f64)>, WorkflowError> {
    if weekly_hour_utc_multipliers.len() != SURPRISE_MULTIPLIER_HOURS_PER_WEEK {
        return Err(validation_error(
            "surprise_deterministic_intensity_profile.weekly_hour_utc_multipliers",
            "must contain exactly 168 hourly multipliers",
        ));
    }

    if range_end_utc <= range_start_utc {
        return Ok(Vec::new());
    }

    let interval_count_nanos = range_end_utc.timestamp_nanos_opt().ok_or_else(|| {
        validation_error(
            "range_end_utc",
            "timestamp is outside the supported nanosecond range",
        )
    })? - range_start_utc.timestamp_nanos_opt().ok_or_else(|| {
        validation_error(
            "range_start_utc",
            "timestamp is outside the supported nanosecond range",
        )
    })?;

    let interval_count =
        usize::try_from(interval_count_nanos / interval_duration_nanos).map_err(|_| {
            validation_error(
                "interval_duration_nanos",
                "runtime interval count does not fit in memory on this platform",
            )
        })?;

    let mut multiplier_by_interval = Vec::new();

    for interval_index in 0..interval_count {
        let interval_start_utc = range_start_utc
            .checked_add_signed(ChronoDuration::nanoseconds(interval_span_nanos(
                interval_duration_nanos,
                interval_index,
            )?))
            .ok_or_else(|| {
                validation_error(
                    "interval_duration_nanos",
                    "interval timestamp is out of range",
                )
            })?;

        let interval_end_utc = interval_start_utc
            .checked_add_signed(ChronoDuration::nanoseconds(interval_duration_nanos))
            .map(|timestamp| timestamp.min(range_end_utc))
            .ok_or_else(|| {
                validation_error(
                    "interval_duration_nanos",
                    "interval timestamp is out of range",
                )
            })?;

        let multiplier = average_weekly_hour_multiplier_over_interval(
            interval_start_utc,
            interval_end_utc,
            weekly_hour_utc_multipliers,
        )?;

        let interval_index = u64::try_from(interval_index).map_err(|_| {
            validation_error(
                "interval_duration_nanos",
                "runtime interval index does not fit in u64",
            )
        })?;

        multiplier_by_interval.push((interval_index, multiplier));
    }

    Ok(multiplier_by_interval)
}

fn average_weekly_hour_multiplier_over_interval(
    interval_start_utc: DateTime<Utc>,
    interval_end_utc: DateTime<Utc>,
    weekly_hour_utc_multipliers: &[f64],
) -> Result<f64, WorkflowError> {
    if interval_end_utc <= interval_start_utc {
        return Ok(1.0);
    }

    let mut weighted_multiplier_sum = 0.0;
    let mut weighted_duration_seconds = 0.0;
    let mut current = interval_start_utc;

    while current < interval_end_utc {
        let hour_start_utc = current
            .with_minute(0)
            .and_then(|timestamp| timestamp.with_second(0))
            .and_then(|timestamp| timestamp.with_nanosecond(0))
            .ok_or_else(|| {
                validation_error(
                    "surprise_multiplier_interval",
                    "failed to derive UTC hour boundary",
                )
            })?;

        let next_hour_boundary_utc = if current == hour_start_utc {
            current + ChronoDuration::hours(1)
        } else {
            hour_start_utc + ChronoDuration::hours(1)
        };

        let segment_end_utc = next_hour_boundary_utc.min(interval_end_utc);

        let segment_duration_seconds = (segment_end_utc - current)
            .to_std()
            .map_err(|source| validation_error("surprise_multiplier_interval", source.to_string()))?
            .as_secs_f64();

        weighted_multiplier_sum += weekly_hour_utc_multipliers[weekly_hour_bucket_index(current)?]
            * segment_duration_seconds;
        weighted_duration_seconds += segment_duration_seconds;

        current = segment_end_utc;
    }

    if weighted_duration_seconds <= 0.0 {
        return Ok(1.0);
    }

    Ok(weighted_multiplier_sum / weighted_duration_seconds)
}

fn weekly_hour_bucket_index(timestamp_utc: DateTime<Utc>) -> Result<usize, WorkflowError> {
    let weekday_index =
        usize::try_from(timestamp_utc.weekday().num_days_from_monday()).map_err(|_| {
            validation_error(
                "surprise_multiplier_interval",
                "weekday index does not fit in usize",
            )
        })?;

    let hour_index = usize::try_from(timestamp_utc.hour()).map_err(|_| {
        validation_error(
            "surprise_multiplier_interval",
            "hour index does not fit in usize",
        )
    })?;

    Ok(weekday_index * 24 + hour_index)
}

fn runtime_interval_grid_is_hour_aligned(
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
) -> bool {
    let Some(range_start_utc_nanos) = range_start_utc.timestamp_nanos_opt() else {
        return false;
    };

    let Some(range_end_utc_nanos) = range_end_utc.timestamp_nanos_opt() else {
        return false;
    };

    range_start_utc_nanos % NANOS_PER_HOUR == 0 && range_end_utc_nanos % NANOS_PER_HOUR == 0
}

#[cfg(test)]
mod tests {
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};
    use janiform::model::factors::interval_factor_grid::NANOS_PER_HOUR;
    use janiform_foundations::time::NANOS_PER_DAY;

    use super::interval_seasonality_source;

    #[test]
    fn runtime_seasonality_source_materializes_non_hourly_intervals() {
        let range_start_utc = Utc
            .with_ymd_and_hms(2026, 3, 2, 0, 0, 0)
            .single()
            .expect("test timestamp should be valid");

        let range_end_utc = range_start_utc + ChronoDuration::days(2);

        let seasonality_grid = (
            vec![
                range_start_utc
                    .timestamp_nanos_opt()
                    .expect("test timestamp should fit in nanos"),
                (range_start_utc + ChronoDuration::days(1))
                    .timestamp_nanos_opt()
                    .expect("test timestamp should fit in nanos"),
            ],
            vec![1.0, 1.1],
        );

        let source = interval_seasonality_source(
            range_start_utc,
            range_end_utc,
            NANOS_PER_DAY,
            &seasonality_grid,
        );

        assert!(matches!(
            source,
            janiform::model::factors::interval_factor_grid::IntervalSeasonalityFactorSource::Materialized { .. }
        ));
    }

    #[test]
    fn runtime_seasonality_source_uses_explicit_source_for_hourly_aligned_grid() {
        let range_start_utc = Utc
            .with_ymd_and_hms(2026, 3, 2, 0, 0, 0)
            .single()
            .expect("test timestamp should be valid");

        let range_end_utc = range_start_utc + ChronoDuration::hours(2);

        let seasonality_grid = (
            vec![
                range_start_utc
                    .timestamp_nanos_opt()
                    .expect("test timestamp should fit in nanos"),
                (range_start_utc + ChronoDuration::hours(1))
                    .timestamp_nanos_opt()
                    .expect("test timestamp should fit in nanos"),
            ],
            vec![1.0, 1.1],
        );

        let source = interval_seasonality_source(
            range_start_utc,
            range_end_utc,
            NANOS_PER_HOUR,
            &seasonality_grid,
        );

        assert!(matches!(
            source,
            janiform::model::factors::interval_factor_grid::IntervalSeasonalityFactorSource::Explicit { .. }
        ));
    }
}
