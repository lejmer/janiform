use chrono::{DateTime, Datelike, Duration as ChronoDuration, Timelike, Utc};
use janiform::calibration::staged::materialize_runtime_structural_seasonality;
use janiform::model::factors::hourly::{
    DEFAULT_SEASONALITY_AGGREGATION_METHOD, DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
    DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY, DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT,
    HourlyFactorContractOutput, HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource,
    HourlySeasonalityFactorSource, NANOS_PER_HOUR,
};
use janiform_calibration_runtime::RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD;
use janiform_foundations::time::parse_utc_datetime;
use serde_json::Value;

use crate::error::WorkflowError;

use super::context::{
    FittedModelContext, PersistedModelPayload, interval_span_nanos, validation_error,
};

const SURPRISE_MULTIPLIER_HOURS_PER_WEEK: usize = 168;
const MINUTES_PER_DAY: usize = 1_440;

pub(crate) fn build_hourly_factor_contract(
    context: &FittedModelContext,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
) -> Result<HourlyFactorContractOutput, WorkflowError> {
    let seasonality_grid = materialize_runtime_seasonality_grid(
        context,
        range_start_utc,
        range_end_utc,
        interval_duration_nanos,
    )?;

    let seasonality_source = hourly_seasonality_source(
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

    let surprise_baseline_intensity = context
        .fitted_parameters
        .return_components_fitted_parameters
        .news_process_configuration
        .arrivals
        .surprise
        .baseline_intensity;

    janiform_calibration_runtime::load_hourly_factor_contract_for_calibration(
        &context.calendar_registry,
        HourlyFactorProviderRequest {
            exchange_code: context.model.session_resolution.exchange_code.as_str(),
            range_start_utc,
            range_end_utc,
            interval_duration_nanos,
            seasonality_source: Some(seasonality_source),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Materialized {
                known_scheduled_event_timestamps_utc_nanos: known_news_timestamps.as_slice(),
                surprise_baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index: surprise_multipliers
                    .as_slice(),
            }),
        },
    )
    .map_err(|source| validation_error("hourly_factor_contract", source.to_string()))
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

    let intraday_factor_by_bin = numeric_sequence(
        seasonality_parameters,
        "intraday_factor_by_bin",
        "seasonality.intraday_factor_by_bin",
    )?;

    let day_of_week_factor = numeric_sequence(
        seasonality_parameters,
        "day_of_week_factor",
        "seasonality.day_of_week_factor",
    )?;

    let day_of_year_factor = numeric_sequence(
        seasonality_parameters,
        "day_of_year_factor",
        "seasonality.day_of_year_factor",
    )?;

    let intraday_minutes_per_bin =
        intraday_minutes_per_bin(seasonality_parameters, intraday_factor_by_bin.len())?;

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
            intraday_minutes_per_bin,
            intraday_factor_by_bin,
            day_of_week_factor,
            day_of_year_factor,
        },
    )
    .map_err(|source| validation_error("seasonality", source.to_string()))?;

    Ok((output.interval_start_utc_nanos, output.seasonality_factors))
}

fn hourly_seasonality_source(
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
    seasonality_grid: &'_ (Vec<i64>, Vec<f64>),
) -> HourlySeasonalityFactorSource<'_> {
    if seasonality_grid.1.is_empty() {
        return HourlySeasonalityFactorSource::Explicit {
            hourly_factors: seasonality_grid.1.as_slice(),
            materialization_method: RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
        };
    }

    if interval_duration_nanos < NANOS_PER_HOUR
        || !runtime_interval_grid_is_hour_aligned(range_start_utc, range_end_utc)
    {
        return HourlySeasonalityFactorSource::Materialized {
            source_timestamp_utc_nanos: seasonality_grid.0.as_slice(),
            source_seasonality_factors: seasonality_grid.1.as_slice(),
            interpolation_method: DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
            aggregation_method: DEFAULT_SEASONALITY_AGGREGATION_METHOD,
            sparse_fallback_policy: DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
            sparse_hour_minimum_observation_count: DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT,
        };
    }

    HourlySeasonalityFactorSource::Explicit {
        hourly_factors: seasonality_grid.1.as_slice(),
        materialization_method: RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
    }
}

fn numeric_sequence(
    payload: &Value,
    field_name: &str,
    field_path: &'static str,
) -> Result<Vec<f64>, WorkflowError> {
    let values = payload
        .get(field_name)
        .and_then(Value::as_array)
        .ok_or_else(|| validation_error(field_path, "must be a sequence"))?;
    if values.is_empty() {
        return Err(validation_error(field_path, "must not be empty"));
    }

    values
        .iter()
        .enumerate()
        .map(|(value_index, value)| {
            value.as_f64().ok_or_else(|| {
                validation_error(
                    field_path,
                    format!("entry at position {value_index} must be numeric"),
                )
            })
        })
        .collect()
}

fn intraday_minutes_per_bin(
    seasonality_parameters: &Value,
    intraday_factor_count: usize,
) -> Result<u16, WorkflowError> {
    if let Some(raw_value) = seasonality_parameters
        .get("intraday_minutes_per_bin")
        .and_then(Value::as_u64)
    {
        return u16::try_from(raw_value).map_err(|_| {
            validation_error(
                "seasonality.intraday_minutes_per_bin",
                "must fit in an unsigned 32-bit integer",
            )
        });
    }

    let (minutes_per_bin, remainder) = (
        MINUTES_PER_DAY / intraday_factor_count,
        MINUTES_PER_DAY % intraday_factor_count,
    );
    if remainder != 0 {
        return Err(validation_error(
            "seasonality.intraday_minutes_per_bin",
            "must be present when intraday_factor_by_bin does not evenly partition a day",
        ));
    }

    u16::try_from(minutes_per_bin).map_err(|_| {
        validation_error(
            "seasonality.intraday_minutes_per_bin",
            "must fit in an unsigned 32-bit integer",
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
