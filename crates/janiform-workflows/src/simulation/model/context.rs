use chrono::{DateTime, Duration as ChronoDuration, Utc};
use janiform::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact};
use janiform::hourly_factors::duration_nanos_to_hours;
use janiform_calibration_artifacts::{
    HawkesStageFittedParameters, ReturnComponentsStageFittedParameters,
};
use serde::Deserialize;
use serde_json::Value;

use crate::error::WorkflowError;

use super::ModelSimulationWorkflowRequest;
use super::support::{interval_span_nanos, validation_error};

const MODEL_SCHEMA_VERSION: &str = "calibration.high_level.model.v1";

pub(super) struct ModelRuntimeContext {
    pub(super) model: PersistedModelPayload,
    pub(super) calendar_registry: ExchangeCalendarRegistry,
    pub(super) calendar_registry_artifact: ExchangeCalendarRegistryArtifact,
    pub(super) interval_delta: f64,
    pub(super) range_end_utc: DateTime<Utc>,
    pub(super) hawkes_fitted_parameters: HawkesStageFittedParameters,
    pub(super) return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
}

#[derive(Debug, Deserialize)]
pub(super) struct PersistedModelPayload {
    pub(super) schema_version: String,
    pub(super) parameters: Value,
    pub(super) metadata: Value,
    pub(super) resolved_configuration: Value,
    pub(super) session_resolution: ModelSessionResolutionPayload,
}

#[derive(Debug, Deserialize)]
pub(super) struct ModelSessionResolutionPayload {
    pub(super) exchange_code: String,
    pub(super) resolved_session_definition_json: String,
}

pub(super) fn runtime_context(
    request: &ModelSimulationWorkflowRequest,
) -> Result<ModelRuntimeContext, WorkflowError> {
    // Intentional copy because the workflow request remains borrowed while the
    // persisted model payload is deserialized into typed runtime context.
    let model: PersistedModelPayload = serde_json::from_value(request.model_payload.clone())
        .map_err(|source| {
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
    // coarse simulation requests also own the original registry artifact.
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

    let hawkes_fitted_parameters = hawkes_fitted_parameters(&model.parameters)?;
    let return_components_fitted_parameters =
        return_components_fitted_parameters(&model.parameters)?;
    let interval_delta = duration_nanos_to_hours(request.interval_duration_nanos);

    let range_end_utc = request
        .start_time_utc
        .checked_add_signed(ChronoDuration::nanoseconds(interval_span_nanos(
            request.interval_duration_nanos,
            request.horizon_interval_count,
        )?))
        .ok_or_else(|| validation_error("range_end_utc", "simulation horizon is out of range"))?;

    Ok(ModelRuntimeContext {
        model,
        calendar_registry,
        calendar_registry_artifact,
        interval_delta,
        range_end_utc,
        hawkes_fitted_parameters,
        return_components_fitted_parameters,
    })
}

fn hawkes_fitted_parameters(
    parameters: &Value,
) -> Result<HawkesStageFittedParameters, WorkflowError> {
    let payload = parameters
        .pointer("/joint_refinement/fitted_parameters/hawkes_fitted_parameters")
        .or_else(|| parameters.pointer("/hawkes/fitted_parameters"))
        .ok_or_else(|| {
            validation_error(
                "model_payload.parameters.hawkes",
                "missing fitted parameters",
            )
        })?;

    // Intentional copy because serde_json deserializes typed contracts from
    // owned Value payloads while model parameters remain available to callers.
    serde_json::from_value(payload.clone()).map_err(|source| {
        validation_error(
            "model_payload.parameters.hawkes.fitted_parameters",
            format!("failed to deserialize Hawkes fitted parameters: {source}"),
        )
    })
}

fn return_components_fitted_parameters(
    parameters: &Value,
) -> Result<ReturnComponentsStageFittedParameters, WorkflowError> {
    let payload = parameters
        .pointer("/joint_refinement/fitted_parameters/return_components_fitted_parameters")
        .or_else(|| parameters.pointer("/return_components/fitted_parameters"))
        .ok_or_else(|| {
            validation_error(
                "model_payload.parameters.return_components",
                "missing fitted parameters",
            )
        })?;

    // Intentional copy because serde_json deserializes typed contracts from
    // owned Value payloads while model parameters remain available to callers.
    serde_json::from_value(payload.clone()).map_err(|source| {
        validation_error(
            "model_payload.parameters.return_components.fitted_parameters",
            format!("failed to deserialize return-components fitted parameters: {source}"),
        )
    })
}
