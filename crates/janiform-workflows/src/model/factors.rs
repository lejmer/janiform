use chrono::{DateTime, Utc};
use janiform::calibration::staged::materialize_runtime_structural_seasonality;
use janiform::model::factors::interval_factor_grid::{
    IntervalFactorGrid, IntervalFactorGridProviderRequest, IntervalNewsIntensityFactorSource,
    IntervalSeasonalityFactorSource,
};
use janiform_calibration_runtime::{
    RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
    materialize_surprise_news_weekly_profile_multipliers_for_interval_range,
};
use janiform_foundations::time::parse_utc_datetime;
use janiform_market_time::business_time_clock::FourierBusinessTimeClockModel;
use serde_json::Value;

use crate::error::WorkflowError;

use super::context::{FittedModelContext, PersistedModelPayload, validation_error};

pub(crate) fn build_interval_factor_grid(
    context: &FittedModelContext,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
) -> Result<IntervalFactorGrid, WorkflowError> {
    let seasonality_factors = materialize_runtime_seasonality_factors(
        context,
        range_start_utc,
        range_end_utc,
        interval_duration_nanos,
    )?;

    let known_news_timestamps = known_news_timestamp_utc_nanos(&context.model)?;

    let surprise_profile = &context
        .fitted_parameters
        .return_components_fitted_parameters
        .surprise_deterministic_intensity_profile;

    let surprise_multipliers =
        materialize_surprise_news_weekly_profile_multipliers_for_interval_range(
            range_start_utc,
            range_end_utc,
            interval_duration_nanos,
            surprise_profile,
        )
        .map_err(|source| {
            validation_error(
                "surprise_deterministic_intensity_profile",
                source.to_string(),
            )
        })?;

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
            seasonality_source: Some(IntervalSeasonalityFactorSource::Explicit {
                interval_factors: seasonality_factors.as_slice(),
                materialization_method: RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
            }),
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

fn materialize_runtime_seasonality_factors(
    context: &FittedModelContext,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
) -> Result<Vec<f64>, WorkflowError> {
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

    Ok(output.seasonality_factors)
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
