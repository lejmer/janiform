use janiform::model::factors::interval_factor_grid::IntervalFactorGrid;
use janiform::model::hawkes::HawkesExcitationState;
use janiform::model::long_variance::LongVarianceState;
use janiform::model::market_pressure::MarketPressureState;
use janiform::simulation::MeanReversionBoundaryState;

use crate::error::WorkflowError;
use crate::model::{FittedModelContext, materialize_mean_reversion_runtime_inputs};

use super::request::{
    DEFAULT_FORECAST_INITIAL_PRICE, ForecastInitialState, ForecastWorkflowRequest, validation_error,
};

const MODEL_DEFAULT_INITIAL_STATE_POLICY: &str = "model_reference_price_log_coordinate_stationary_hawkes_unit_long_variance_neutral_market_pressure_zero_deviation.v1";
const EXPLICIT_CURRENT_STATE_POLICY: &str = "explicit_current_state.v1";

pub(super) struct ResolvedForecastInitialState {
    pub(super) initial_price: f64,
    pub(super) initial_log_price: f64,
    pub(super) policy: &'static str,
    pub(super) excitation_state: HawkesExcitationState,
    pub(super) long_variance_state: LongVarianceState,
    pub(super) mean_reversion_boundary_state: MeanReversionBoundaryState,
    pub(super) market_pressure_state: MarketPressureState,
}

pub(super) fn resolve_forecast_initial_state(
    request: &ForecastWorkflowRequest,
    context: &FittedModelContext,
    interval_factor_grid: &IntervalFactorGrid,
    reference_price: f64,
) -> Result<ResolvedForecastInitialState, WorkflowError> {
    match request.state.as_ref() {
        Some(state) => resolve_explicit_initial_state(state, reference_price),
        None => resolve_model_default_initial_state(
            request,
            context,
            interval_factor_grid,
            reference_price,
        ),
    }
}

pub(super) fn validate_forecast_initial_state(
    state: &ForecastInitialState,
) -> Result<(), WorkflowError> {
    state
        .hawkes_excitation_state
        .validate()
        .map_err(|source| validation_error("state.hawkes_excitation_state", source.to_string()))?;

    state
        .long_variance_state
        .validate()
        .map_err(|source| validation_error("state.long_variance_state", source.to_string()))?;

    state
        .mean_reversion_boundary_state
        .validate()
        .map_err(|source| {
            validation_error("state.mean_reversion_boundary_state", source.to_string())
        })?;

    state
        .market_pressure_state
        .validate()
        .map_err(|source| validation_error("state.market_pressure_state", source.to_string()))?;

    Ok(())
}

fn resolve_explicit_initial_state(
    state: &ForecastInitialState,
    reference_price: f64,
) -> Result<ResolvedForecastInitialState, WorkflowError> {
    let initial_model_log_price = state.mean_reversion_boundary_state.observed_log_price;
    let initial_price = reference_price * initial_model_log_price.exp();
    let initial_log_price = reference_price.ln() + initial_model_log_price;

    if initial_price <= 0.0 || !initial_price.is_finite() || !initial_log_price.is_finite() {
        return Err(validation_error(
            "state.mean_reversion_boundary_state.observed_log_price",
            "converted state price must be finite and strictly positive",
        ));
    }

    Ok(ResolvedForecastInitialState {
        initial_price,
        initial_log_price,
        policy: EXPLICIT_CURRENT_STATE_POLICY,
        excitation_state: state.hawkes_excitation_state,
        long_variance_state: state.long_variance_state,
        mean_reversion_boundary_state: state.mean_reversion_boundary_state,
        // Forecast initial state owns the pressure-state distribution.
        market_pressure_state: state.market_pressure_state.clone(),
    })
}

fn resolve_model_default_initial_state(
    request: &ForecastWorkflowRequest,
    context: &FittedModelContext,
    interval_factor_grid: &IntervalFactorGrid,
    reference_price: f64,
) -> Result<ResolvedForecastInitialState, WorkflowError> {
    let initial_price = request
        .initial_price
        .unwrap_or(DEFAULT_FORECAST_INITIAL_PRICE);

    let initial_model_log_price = (initial_price / reference_price).ln();
    let initial_log_price = initial_price.ln();

    if !initial_model_log_price.is_finite() || !initial_log_price.is_finite() {
        return Err(validation_error(
            "initial_price",
            "converted initial log price must be finite",
        ));
    }

    let excitation_state = context
        .fitted_parameters
        .hawkes_fitted_parameters
        .kernel_parameters
        .stationary_excitation_state()
        .map_err(|source| validation_error("kernel_parameters", source.to_string()))?;

    let (_, mean_reversion_boundary_state) = materialize_mean_reversion_runtime_inputs(
        context,
        interval_factor_grid,
        excitation_state,
        initial_model_log_price,
    )?;
    let market_pressure_state = MarketPressureState::neutral_for_parameters(
        &context
            .fitted_parameters
            .return_components_fitted_parameters
            .market_pressure_parameters,
    )
    .map_err(|source| validation_error("market_pressure_state", source.to_string()))?;

    Ok(ResolvedForecastInitialState {
        initial_price,
        initial_log_price,
        policy: MODEL_DEFAULT_INITIAL_STATE_POLICY,
        excitation_state,
        long_variance_state: LongVarianceState::unit(),
        mean_reversion_boundary_state,
        market_pressure_state,
    })
}
