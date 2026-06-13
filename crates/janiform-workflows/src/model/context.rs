use chrono::{DateTime, Duration as ChronoDuration, Utc};
use janiform::calibration::current_state::{
    CurrentStateEstimationInput, estimate_current_state_from_observed_data,
};
use janiform::model::factors::interval_factor_grid::duration_nanos_to_hours;
use janiform::model::hawkes::HawkesEventSide;
use janiform::time::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact};
use janiform_calibration_artifacts::JointRefinementStageFittedParameters;
use serde::Deserialize;
use serde_json::Value;

use crate::error::WorkflowError;

const MODEL_SCHEMA_VERSION: &str = "calibration.high_level.model.v1";

pub(crate) struct FittedModelContext {
    pub(crate) model: PersistedModelPayload,
    pub(crate) calendar_registry: ExchangeCalendarRegistry,
    pub(crate) calendar_registry_artifact: ExchangeCalendarRegistryArtifact,
    pub(crate) interval_delta: f64,
    pub(crate) range_end_utc: DateTime<Utc>,
    pub(crate) fitted_parameters: JointRefinementStageFittedParameters,
}

/// Trade observations used to estimate the state at a model workflow boundary.
#[derive(Debug, Deserialize)]
pub struct ModelStateObservationData {
    /// Observation timestamps in UTC nanoseconds.
    pub observation_timestamp_utc_nanos: Vec<i64>,
    /// Observed trade prices aligned with `observation_timestamp_utc_nanos`.
    pub observation_trade_prices: Vec<f64>,
    /// Trade sides aligned with `observation_timestamp_utc_nanos`.
    pub observation_trade_sides: Vec<String>,
    /// Optional observed price at the state boundary.
    pub observed_price: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PersistedModelPayload {
    pub(crate) schema_version: String,
    pub(crate) run_id: String,
    pub(crate) parameters: Value,
    pub(crate) metadata: Value,
    pub(crate) resolved_configuration: Value,
    pub(crate) session_resolution: ModelSessionResolutionPayload,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ModelSessionResolutionPayload {
    pub(crate) exchange_code: String,
    pub(crate) resolved_session_definition_json: String,
}

pub(crate) fn fitted_model_context(
    model_payload: &Value,
    range_start_utc: DateTime<Utc>,
    interval_duration_nanos: i64,
    horizon_interval_count: usize,
) -> Result<FittedModelContext, WorkflowError> {
    // Intentional copy because the workflow request remains borrowed while the
    // persisted model payload is deserialized into typed runtime context.
    let model: PersistedModelPayload =
        serde_json::from_value(model_payload.clone()).map_err(|source| {
            validation_error(
                "model_payload",
                format!("failed to deserialize model: {source}"),
            )
        })?;
    if model.schema_version != MODEL_SCHEMA_VERSION {
        return Err(validation_error(
            "model_payload.schema_version",
            format!("must be `{MODEL_SCHEMA_VERSION}`"),
        ));
    }

    let calendar_registry_artifact: ExchangeCalendarRegistryArtifact = serde_json::from_str(
        &model.session_resolution.resolved_session_definition_json,
    )
    .map_err(|source| {
        validation_error(
            "model_payload.session_resolution.resolved_session_definition_json",
            format!("failed to deserialize calendar registry: {source}"),
        )
    })?;

    // Intentional copy because the registry loader consumes the artifact and
    // downstream workflow requests may also need to own the original artifact.
    let calendar_registry = ExchangeCalendarRegistry::from_artifact(
        calendar_registry_artifact.clone(),
    )
    .map_err(|source| WorkflowError::CalendarRegistryLoadFailed {
        message: source.to_string(),
    })?;

    calendar_registry
        .get_required(model.session_resolution.exchange_code.as_str())
        .map_err(|source| {
            validation_error(
                "model_payload.session_resolution.exchange_code",
                source.to_string(),
            )
        })?;

    let fitted_parameters = fitted_parameters(&model.parameters)?;

    let interval_delta = duration_nanos_to_hours(interval_duration_nanos);

    let range_end_utc = range_start_utc
        .checked_add_signed(ChronoDuration::nanoseconds(interval_span_nanos(
            interval_duration_nanos,
            horizon_interval_count,
        )?))
        .ok_or_else(|| validation_error("range_end_utc", "runtime horizon is out of range"))?;

    Ok(FittedModelContext {
        model,
        calendar_registry,
        calendar_registry_artifact,
        interval_delta,
        range_end_utc,
        fitted_parameters,
    })
}

pub(crate) fn validate_persisted_model_payload(model_payload: &Value) -> Result<(), WorkflowError> {
    let model: PersistedModelPayload =
        serde_json::from_value(model_payload.clone()).map_err(|source| {
            validation_error(
                "model_payload",
                format!("failed to deserialize model: {source}"),
            )
        })?;

    if model.schema_version != MODEL_SCHEMA_VERSION {
        return Err(validation_error(
            "model_payload.schema_version",
            format!("must be `{MODEL_SCHEMA_VERSION}`"),
        ));
    }

    if model.run_id.trim().is_empty() {
        return Err(validation_error(
            "model_payload.run_id",
            "must be non-empty",
        ));
    }

    Ok(())
}

pub(crate) fn persisted_model_run_id(model_payload: &Value) -> Result<String, WorkflowError> {
    validate_persisted_model_payload(model_payload)?;

    let run_id = model_payload
        .get("run_id")
        .and_then(Value::as_str)
        .ok_or_else(|| validation_error("model_payload.run_id", "must be a string"))?;

    Ok(run_id.to_string())
}

pub(crate) fn model_reference_price(model: &PersistedModelPayload) -> Result<f64, WorkflowError> {
    let reference_price = model
        .metadata
        .pointer("/simulation_output_preferences/reference_price")
        .and_then(Value::as_f64)
        .ok_or_else(|| {
            validation_error(
                "model_payload.metadata.simulation_output_preferences.reference_price",
                "must be present so runtime state can be derived from spot",
            )
        })?;

    if !reference_price.is_finite() || reference_price <= 0.0 {
        return Err(validation_error(
            "model_payload.metadata.simulation_output_preferences.reference_price",
            "must be finite and strictly positive",
        ));
    }

    Ok(reference_price)
}

pub(crate) fn estimate_current_state_from_observation_data(
    context: &FittedModelContext,
    state_time_utc: DateTime<Utc>,
    observation_data: &ModelStateObservationData,
    observed_price_override: Option<f64>,
) -> Result<janiform_calibration_runtime::CurrentStateEstimationOutput, WorkflowError> {
    let observed_price = observed_price_override
        .or(observation_data.observed_price)
        .or_else(|| observation_data.observation_trade_prices.last().copied())
        .ok_or_else(|| {
            validation_error(
                "observation_data.observation_trade_prices",
                "must contain at least one price observation",
            )
        })?;

    let reference_price = model_reference_price(&context.model)?;
    let observed_log_return = observed_price.ln() - reference_price.ln();

    let observation_trade_sides = observation_data
        .observation_trade_sides
        .iter()
        .map(|side| HawkesEventSide::try_from(side.as_str()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| {
            validation_error(
                "observation_data.observation_trade_sides",
                source.to_string(),
            )
        })?;

    let state_time_utc_nanos = state_time_utc.timestamp_nanos_opt().ok_or_else(|| {
        validation_error(
            "state_time_utc",
            "timestamp is outside the supported nanosecond range",
        )
    })?;

    estimate_current_state_from_observed_data(CurrentStateEstimationInput {
        kernel_parameters: context
            .fitted_parameters
            .hawkes_fitted_parameters
            .kernel_parameters,
        long_variance_parameters: context
            .fitted_parameters
            .return_components_fitted_parameters
            .long_variance_parameters,
        // Intentional copy because current-state estimation owns the fitted
        // payload while the runtime context remains available to request builders.
        mean_reversion_fitted_parameters: context
            .fitted_parameters
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters
            .clone(),
        state_time_utc_nanos,
        observed_log_return,
        // Intentional copies because current-state estimation owns observation
        // columns while the workflow request stays available for error context.
        observation_timestamp_utc_nanos: observation_data.observation_timestamp_utc_nanos.clone(),
        observation_trade_prices: observation_data.observation_trade_prices.clone(),
        observation_trade_sides,
    })
    .map_err(|source| validation_error("observation_data", source.to_string()))
}

pub(crate) fn validation_error(field: &'static str, message: impl Into<String>) -> WorkflowError {
    WorkflowError::ModelValidationFailed {
        field,
        message: message.into(),
    }
}

pub(crate) fn interval_span_nanos(
    interval_duration_nanos: i64,
    interval_count: usize,
) -> Result<i64, WorkflowError> {
    let interval_count = i64::try_from(interval_count).map_err(|_| {
        validation_error(
            "interval_duration_nanos",
            "interval count does not fit in signed 64-bit arithmetic",
        )
    })?;

    interval_duration_nanos
        .checked_mul(interval_count)
        .ok_or_else(|| {
            validation_error(
                "interval_duration_nanos",
                "interval span overflowed signed 64-bit nanosecond arithmetic",
            )
        })
}

fn fitted_parameters(
    parameters: &Value,
) -> Result<JointRefinementStageFittedParameters, WorkflowError> {
    let payload = parameters
        .pointer("/finalization/fitted_parameters")
        .ok_or_else(|| {
            validation_error(
                "model_payload.parameters.finalization.fitted_parameters",
                "missing fitted parameters",
            )
        })?;

    // Intentional copy because serde_json deserializes typed contracts from
    // owned Value payloads while model parameters remain available to callers.
    serde_json::from_value(payload.clone()).map_err(|source| {
        validation_error(
            "model_payload.parameters.finalization.fitted_parameters",
            format!("failed to deserialize final fitted parameters: {source}"),
        )
    })
}
