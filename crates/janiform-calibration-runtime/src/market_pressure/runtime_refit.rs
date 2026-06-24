use std::sync::Arc;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::errors::MarketPressureFitError;
use super::fit::{
    MarketPressureCoarseReturnLaw, MarketPressureFitOutput, MarketPressureFitProgressObserver,
    MarketPressureRuntimeState, market_pressure_parameters_from_runtime_state,
};
use super::hidden_state::fit_hidden_side_balance_model_from_cached_fit;

pub(crate) struct MarketPressureRuntimeNigFitInput<'a> {
    pub(crate) runtime_state: &'a MarketPressureRuntimeState,
    pub(crate) nig_configuration: NigSharedPlusShiftConfiguration,
    pub(crate) solver_settings: &'a CalibrationOptimizerSettings,
    pub(crate) progress_observer: Option<MarketPressureFitProgressObserver<'a>>,
}

pub(crate) fn fit_market_pressure_from_runtime_state_and_nig_configuration(
    mut input: MarketPressureRuntimeNigFitInput<'_>,
) -> Result<MarketPressureFitOutput, MarketPressureFitError> {
    let coarse_return_law = MarketPressureCoarseReturnLaw::from_nig_configuration(
        input.nig_configuration,
        input.runtime_state.coarse_return_record_count,
    )?;

    let hidden_fit = fit_hidden_side_balance_model_from_cached_fit(
        input.runtime_state.observations.as_slice(),
        input.runtime_state.coarse_return_observations.as_slice(),
        coarse_return_law,
        input.runtime_state.open_event_rate_per_hour,
        &input.runtime_state.hidden_fit,
        input.solver_settings,
        &mut input.progress_observer,
    )?;

    // Cached refits own immutable snapshots so the runtime state can outlive the source payload.
    let runtime_state = Arc::new(MarketPressureRuntimeState {
        observations: input.runtime_state.observations.clone(),
        coarse_return_record_count: input.runtime_state.coarse_return_record_count,
        coarse_return_observations: input.runtime_state.coarse_return_observations.clone(),
        coarse_return_pressure_evidence_available: coarse_return_law.is_some(),
        open_event_rate_per_hour: input.runtime_state.open_event_rate_per_hour,
        hidden_fit,
    });

    let parameters = market_pressure_parameters_from_runtime_state(&runtime_state)?;

    // The fit output owns the diagnostic rows alongside the reusable runtime state.
    let selection_diagnostics = runtime_state.hidden_fit.selection_diagnostics.clone();

    Ok(MarketPressureFitOutput {
        parameters,
        runtime_state,
        selection_diagnostics,
    })
}
