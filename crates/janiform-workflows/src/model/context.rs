use chrono::{DateTime, Duration as ChronoDuration, Utc};
use janiform::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact};
use janiform::hourly_factors::duration_nanos_to_hours;
use janiform::nig::NigSharedPlusShiftConfiguration;
use janiform_calibration_artifacts::{
    HawkesStageFittedParameters, ReturnComponentsStageFittedParameters,
};
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
    pub(crate) hawkes_fitted_parameters: HawkesStageFittedParameters,
    pub(crate) return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
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

    let hawkes_fitted_parameters = hawkes_fitted_parameters(&model.parameters)?;

    let return_components_fitted_parameters =
        return_components_fitted_parameters(&model.parameters)?;

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
        hawkes_fitted_parameters,
        return_components_fitted_parameters,
    })
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

pub(crate) fn pricing_nig_shared_plus_shift_configuration(
    context: &FittedModelContext,
) -> Result<NigSharedPlusShiftConfiguration, WorkflowError> {
    let Some(payload) = context
        .model
        .parameters
        .pointer("/joint_refinement/fitted_parameters/pricing_block_fitted_parameters/nig_shared_plus_shift_configuration")
    else {
        return Ok(context
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration);
    };

    // Intentional copy because serde_json deserializes the optional joint
    // pricing block from an owned payload while the model remains available.
    serde_json::from_value(payload.clone()).map_err(|source| {
        validation_error(
            "model_payload.parameters.joint_refinement.fitted_parameters.pricing_block_fitted_parameters.nig_shared_plus_shift_configuration",
            format!("failed to deserialize pricing block NIG parameters: {source}"),
        )
    })
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
