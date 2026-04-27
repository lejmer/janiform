use janiform::calibration::{
    CurrentStateEstimationInput, estimate_current_state_from_observed_data,
};
use janiform::hawkes::{HawkesEventSide, HawkesExcitationState};
use janiform::simulation::MeanReversionBoundaryState;
use serde_json::Value;

use crate::error::WorkflowError;

use super::ModelSimulationWorkflowRequest;
use super::context::{ModelRuntimeContext, PersistedModelPayload};
use super::support::validation_error;

pub(super) struct ResolvedInitialState {
    pub(super) excitation_state: HawkesExcitationState,
    pub(super) mean_reversion_boundary_state: Option<MeanReversionBoundaryState>,
    pub(super) start_anchor_value: f64,
}

pub(super) fn resolve_initial_state(
    request: &ModelSimulationWorkflowRequest,
    context: &ModelRuntimeContext,
) -> Result<ResolvedInitialState, WorkflowError> {
    let estimated_state = if request.continuation_data.is_some()
        && (request.initial_excitation_state.is_none()
            || request.initial_mean_reversion_boundary_state.is_none())
    {
        Some(estimate_current_state(request, context)?)
    } else {
        None
    };

    let excitation_state = match (request.initial_excitation_state, estimated_state.as_ref()) {
        (Some(excitation_state), _) => excitation_state,
        (None, Some(estimated_state)) => estimated_state.hawkes_excitation_state,
        (None, None) => context
            .hawkes_fitted_parameters
            .kernel_parameters
            .stationary_excitation_state()
            .map_err(|source| validation_error("kernel_parameters", source.to_string()))?,
    };

    // Intentional copy because the resolved state is stored and also inspected
    // for the runtime anchor start value.
    let mean_reversion_boundary_state = request
        .initial_mean_reversion_boundary_state
        .clone()
        .or_else(|| estimated_state.map(|state| state.mean_reversion_boundary_state));

    let start_anchor_value = mean_reversion_boundary_state
        .as_ref()
        .map(|boundary_state| boundary_state.anchor_value)
        .unwrap_or(0.0);

    Ok(ResolvedInitialState {
        excitation_state,
        mean_reversion_boundary_state,
        start_anchor_value,
    })
}

fn estimate_current_state(
    request: &ModelSimulationWorkflowRequest,
    context: &ModelRuntimeContext,
) -> Result<janiform_calibration_runtime::CurrentStateEstimationOutput, WorkflowError> {
    let continuation_data = request
        .continuation_data
        .as_ref()
        .ok_or_else(|| validation_error("continuation_data", "missing continuation data"))?;

    let observed_price = continuation_data.observed_price.unwrap_or_else(|| {
        continuation_data
            .observation_trade_prices
            .last()
            .copied()
            .unwrap_or(0.0)
    });

    let reference_price = model_reference_price(&context.model)?;

    let observed_log_price = (observed_price / reference_price).ln();

    let observation_trade_sides = continuation_data
        .observation_trade_sides
        .iter()
        .map(|side| HawkesEventSide::try_from(side.as_str()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| {
            validation_error(
                "continuation_data.observation_trade_sides",
                source.to_string(),
            )
        })?;

    let state_time_utc_nanos = request
        .start_time_utc
        .timestamp_nanos_opt()
        .ok_or_else(|| {
            validation_error(
                "start_time_utc",
                "timestamp is outside the supported nanosecond range",
            )
        })?;

    estimate_current_state_from_observed_data(CurrentStateEstimationInput {
        kernel_parameters: context.hawkes_fitted_parameters.kernel_parameters,
        // Intentional copy because current-state estimation owns the fitted
        // payload while the runtime context remains available to request builders.
        mean_reversion_fitted_parameters: context
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters
            .clone(),
        state_time_utc_nanos,
        observed_log_price,
        // Intentional copies because current-state estimation owns observation
        // columns while the workflow request stays available for error context.
        observation_timestamp_utc_nanos: continuation_data.observation_timestamp_utc_nanos.clone(),
        observation_trade_prices: continuation_data.observation_trade_prices.clone(),
        observation_trade_sides,
    })
    .map_err(|source| validation_error("continuation_data", source.to_string()))
}

fn model_reference_price(model: &PersistedModelPayload) -> Result<f64, WorkflowError> {
    let reference_price = model
        .metadata
        .pointer("/simulation_output_preferences/reference_price")
        .and_then(Value::as_f64)
        .ok_or_else(|| {
            validation_error(
                "model_payload.metadata.simulation_output_preferences.reference_price",
                "must be present so continuation state can be derived from spot",
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
