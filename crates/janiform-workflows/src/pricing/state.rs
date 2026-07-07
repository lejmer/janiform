use janiform::model::factors::interval_factor_grid::IntervalFactorGrid;
use janiform::model::hawkes::HawkesExcitationState;
use janiform::model::long_variance::LongVarianceState;
use janiform::model::market_pressure::MarketPressureState;
use janiform::simulation::MeanReversionBoundaryState;

use crate::error::WorkflowError;
use crate::model::{
    FittedModelContext, estimate_current_state_from_observation_data,
    materialize_mean_reversion_runtime_inputs, validation_error,
};

use super::FittedModelSingleOptionPricingRequest;

pub(super) struct PricingBoundaryState {
    pub(super) excitation_state: HawkesExcitationState,
    pub(super) long_variance_state: LongVarianceState,
    pub(super) mean_reversion_boundary_state: MeanReversionBoundaryState,
    pub(super) market_pressure_state: MarketPressureState,
}

pub(super) fn resolve_pricing_boundary_state(
    request: &FittedModelSingleOptionPricingRequest,
    context: &FittedModelContext,
    boundary_interval_factor_grid: &IntervalFactorGrid,
    observed_log_return: f64,
) -> Result<PricingBoundaryState, WorkflowError> {
    if let Some(excitation_state) = request.initial_excitation_state {
        let (_anchors, mean_reversion_boundary_state) = materialize_mean_reversion_runtime_inputs(
            context,
            boundary_interval_factor_grid,
            excitation_state,
            observed_log_return,
        )?;
        let market_pressure_state = neutral_market_pressure_state_for_context(context)?;

        return Ok(PricingBoundaryState {
            excitation_state,
            long_variance_state: LongVarianceState::unit(),
            mean_reversion_boundary_state,
            market_pressure_state,
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
            long_variance_state: estimated_state.long_variance_state,
            mean_reversion_boundary_state: estimated_state.mean_reversion_boundary_state,
            market_pressure_state: estimated_state.market_pressure_state,
        });
    }

    let excitation_state = context
        .fitted_parameters
        .pricing_hawkes_kernel_parameters()
        .stationary_excitation_state()
        .map_err(|source| validation_error("kernel_parameters", source.to_string()))?;

    let (_anchors, mean_reversion_boundary_state) = materialize_mean_reversion_runtime_inputs(
        context,
        boundary_interval_factor_grid,
        excitation_state,
        observed_log_return,
    )?;
    let market_pressure_state = neutral_market_pressure_state_for_context(context)?;

    Ok(PricingBoundaryState {
        excitation_state,
        long_variance_state: LongVarianceState::unit(),
        mean_reversion_boundary_state,
        market_pressure_state,
    })
}

fn neutral_market_pressure_state_for_context(
    context: &FittedModelContext,
) -> Result<MarketPressureState, WorkflowError> {
    MarketPressureState::neutral_for_parameters(
        &context
            .fitted_parameters
            .return_components_fitted_parameters
            .market_pressure_parameters,
    )
    .map_err(|source| validation_error("market_pressure_state", source.to_string()))
}
