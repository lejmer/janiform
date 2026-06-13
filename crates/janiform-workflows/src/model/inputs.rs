use janiform::model::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform::model::factors::interval_factor_grid::IntervalFactorGrid;
use janiform::model::hawkes::HawkesExcitationState;
use janiform::model::sufficient_statistics::{
    SufficientStatisticsIntervalFactors, SufficientStatisticsModelInputs,
};
use janiform::simulation::{
    CoarseSimulationModelInputs, MeanReversionBoundaryState, MeanReversionRuntimeIntervalAnchors,
    SeededTickSimulationModelInputs,
};

use crate::error::WorkflowError;

use super::context::{FittedModelContext, validation_error};

pub(crate) fn build_seeded_tick_simulation_model_inputs(
    context: &FittedModelContext,
    initial_excitation_state: HawkesExcitationState,
    mean_reversion_runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors,
    initial_mean_reversion_boundary_state: MeanReversionBoundaryState,
    interval_factor_grid: IntervalFactorGrid,
) -> SeededTickSimulationModelInputs {
    SeededTickSimulationModelInputs {
        kernel_parameters: context
            .fitted_parameters
            .hawkes_fitted_parameters
            .kernel_parameters,
        nig_shared_plus_shift_configuration: context
            .fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        // Intentional copy because seeded simulation owns model inputs
        // independent of the shared workflow context.
        news_process_configuration: context
            .fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration
            .clone(),
        initial_excitation_state,
        // Intentional copy because seeded simulation owns a validated model
        // input independent of the shared workflow context.
        mean_reversion_fitted_parameters: context
            .fitted_parameters
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters
            .clone(),
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
        interval_factor_grid,
        return_clock_exposure_parameters: context
            .fitted_parameters
            .return_components_fitted_parameters
            .return_clock_exposure_parameters,
        long_variance_parameters: context
            .fitted_parameters
            .return_components_fitted_parameters
            .long_variance_parameters,
        initial_long_variance_state: janiform::model::long_variance::LongVarianceState::unit(),
        observed_price_tick_size: context
            .fitted_parameters
            .return_components_fitted_parameters
            .tick_grid_observation_model
            .tick_size,
        // Intentional copy because seeded simulation owns runtime inputs
        // independent of the shared workflow context.
        calendar_registry: context.calendar_registry.clone(),
    }
}

pub(crate) fn build_sufficient_statistics_model_inputs(
    context: &FittedModelContext,
    initial_excitation_state: HawkesExcitationState,
    mean_reversion_runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors,
    initial_mean_reversion_boundary_state: MeanReversionBoundaryState,
) -> Result<SufficientStatisticsModelInputs, WorkflowError> {
    let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
        context
            .fitted_parameters
            .hawkes_fitted_parameters
            .kernel_parameters,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration
            .shared_mark_law
            .gamma_severity_marks,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .long_variance_parameters,
    )
    .map_err(|source| validation_error("equation_specification", source.to_string()))?;

    SufficientStatisticsModelInputs::try_new(SufficientStatisticsModelInputs {
        equation_specification,
        nig_shared_plus_shift_configuration: context
            .fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        initial_excitation_state,
        long_variance_parameters: context
            .fitted_parameters
            .return_components_fitted_parameters
            .long_variance_parameters,
        initial_long_variance_state: janiform::model::long_variance::LongVarianceState::unit(),
        // Intentional copy because the sufficient-statistics contract owns
        // validated fitted parameters independent of the shared workflow context.
        mean_reversion_fitted_parameters: context
            .fitted_parameters
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters
            .clone(),
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    })
    .map_err(|source| validation_error("sufficient_statistics_model_inputs", source.to_string()))
}

pub(crate) fn build_coarse_simulation_model_inputs(
    context: &FittedModelContext,
    model_inputs: SufficientStatisticsModelInputs,
) -> Result<CoarseSimulationModelInputs, WorkflowError> {
    CoarseSimulationModelInputs::try_new(CoarseSimulationModelInputs {
        sufficient_statistics_model_inputs: model_inputs,
        // Intentional copy because the coarse simulation contract owns model inputs
        // independent of the shared workflow context.
        news_process_configuration: context
            .fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration
            .clone(),
        return_clock_exposure_parameters: context
            .fitted_parameters
            .return_components_fitted_parameters
            .return_clock_exposure_parameters,
        long_variance_parameters: context
            .fitted_parameters
            .return_components_fitted_parameters
            .long_variance_parameters,
        initial_long_variance_state: janiform::model::long_variance::LongVarianceState::unit(),
        observed_price_tick_size: context
            .fitted_parameters
            .return_components_fitted_parameters
            .tick_grid_observation_model
            .tick_size,
        // Intentional copies because coarse simulation owns serialized calendar
        // and exchange identity independent of the shared workflow context.
        calendar_registry_artifact: context.calendar_registry_artifact.clone(),
        exchange_code: context.model.session_resolution.exchange_code.clone(),
    })
    .map_err(|source| validation_error("coarse_model_inputs", source.to_string()))
}

pub(crate) fn build_sufficient_statistics_interval_factor_grids(
    context: &FittedModelContext,
    interval_factor_grid: IntervalFactorGrid,
) -> Result<SufficientStatisticsIntervalFactors, WorkflowError> {
    SufficientStatisticsIntervalFactors::try_new(
        interval_factor_grid.seasonality_factor,
        interval_factor_grid.known_news_intensity,
        interval_factor_grid.surprise_news_intensity,
        Some(context.interval_delta),
    )
    .map_err(|source| validation_error("interval_factor_grids", source.to_string()))
}
