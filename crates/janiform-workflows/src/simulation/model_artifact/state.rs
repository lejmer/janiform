use janiform::hawkes::HawkesExcitationState;
use janiform::simulation::MeanReversionBoundaryState;

use crate::error::WorkflowError;
use crate::model::{ModelStateObservationData, estimate_current_state_from_observation_data};

use super::ModelArtifactSimulationWorkflowRequest;
use super::context::ModelRuntimeContext;
use super::support::validation_error;

pub(super) struct ResolvedInitialState {
    pub(super) excitation_state: HawkesExcitationState,
    pub(super) mean_reversion_boundary_state: Option<MeanReversionBoundaryState>,
    pub(super) start_anchor_value: f64,
}

pub(super) fn resolve_initial_state(
    request: &ModelArtifactSimulationWorkflowRequest,
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
    request: &ModelArtifactSimulationWorkflowRequest,
    context: &ModelRuntimeContext,
) -> Result<janiform_calibration_runtime::CurrentStateEstimationOutput, WorkflowError> {
    let continuation_data = request
        .continuation_data
        .as_ref()
        .ok_or_else(|| validation_error("continuation_data", "missing continuation data"))?;

    estimate_current_state_from_observation_data(
        context,
        request.start_time_utc,
        &ModelStateObservationData {
            // Intentional copies because the shared model-state estimator owns
            // observation columns while the workflow request remains borrowed.
            observation_timestamp_utc_nanos: continuation_data
                .observation_timestamp_utc_nanos
                .clone(),
            observation_trade_prices: continuation_data.observation_trade_prices.clone(),
            observation_trade_sides: continuation_data.observation_trade_sides.clone(),
            observed_price: continuation_data.observed_price,
        },
        None,
    )
    .map_err(|source| validation_error("continuation_data", source.to_string()))
}
