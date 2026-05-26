use janiform::model::factors::hourly::HourlyFactorContractOutput;
use janiform::model::hawkes::HawkesExcitationState;
use janiform::simulation::MeanReversionBoundaryState;

use crate::error::WorkflowError;
use crate::model::{
    FittedModelContext, estimate_current_state_from_observation_data,
    materialize_mean_reversion_runtime_inputs, validation_error,
};

use super::FittedModelSingleOptionPricingRequest;

pub(super) struct PricingBoundaryState {
    pub(super) excitation_state: HawkesExcitationState,
    pub(super) mean_reversion_boundary_state: MeanReversionBoundaryState,
}

pub(super) fn resolve_pricing_boundary_state(
    request: &FittedModelSingleOptionPricingRequest,
    context: &FittedModelContext,
    boundary_hourly_factor_contract: &HourlyFactorContractOutput,
    observed_log_return: f64,
) -> Result<PricingBoundaryState, WorkflowError> {
    if let Some(excitation_state) = request.initial_excitation_state {
        let (_anchors, mean_reversion_boundary_state) = materialize_mean_reversion_runtime_inputs(
            context,
            boundary_hourly_factor_contract,
            excitation_state,
            observed_log_return,
            context.interval_delta,
            1,
        )?;

        return Ok(PricingBoundaryState {
            excitation_state,
            mean_reversion_boundary_state,
        });
    }

    if let Some(continuation_data) = request.continuation_data.as_ref() {
        let estimated_state = estimate_current_state_from_observation_data(
            context,
            request.valuation_time_utc,
            continuation_data,
            Some(request.spot),
        )?;

        return Ok(PricingBoundaryState {
            excitation_state: estimated_state.hawkes_excitation_state,
            mean_reversion_boundary_state: estimated_state.mean_reversion_boundary_state,
        });
    }

    let excitation_state = context
        .fitted_parameters
        .pricing_hawkes_kernel_parameters()
        .stationary_excitation_state()
        .map_err(|source| validation_error("kernel_parameters", source.to_string()))?;

    let (_anchors, mean_reversion_boundary_state) = materialize_mean_reversion_runtime_inputs(
        context,
        boundary_hourly_factor_contract,
        excitation_state,
        observed_log_return,
        context.interval_delta,
        1,
    )?;

    Ok(PricingBoundaryState {
        excitation_state,
        mean_reversion_boundary_state,
    })
}
