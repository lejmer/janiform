use janiform::model::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform::model::factors::hourly::HourlyFactorContractOutput;
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
    hourly_factor_contract: HourlyFactorContractOutput,
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
        hourly_factor_contract,
        return_mark_hour_of_week_factors: context
            .fitted_parameters
            .return_components_fitted_parameters
            .return_mark_hour_of_week_scale_profile
            .hour_of_week_factors
            .clone(),
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
    )
    .map_err(|source| validation_error("equation_specification", source.to_string()))?;

    SufficientStatisticsModelInputs::try_new(
        equation_specification,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        initial_excitation_state,
        // Intentional copy because the sufficient-statistics contract owns
        // validated fitted parameters independent of the shared workflow context.
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters
            .clone(),
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    )
    .map_err(|source| validation_error("sufficient_statistics_model_inputs", source.to_string()))
}

pub(crate) fn build_coarse_simulation_model_inputs(
    context: &FittedModelContext,
    model_inputs: SufficientStatisticsModelInputs,
) -> Result<CoarseSimulationModelInputs, WorkflowError> {
    CoarseSimulationModelInputs::try_new(
        model_inputs,
        // Intentional copy because the coarse simulation contract owns model inputs
        // independent of the shared workflow context.
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration
            .clone(),
        // Intentional copy because coarse simulation owns the return-mark profile.
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .return_mark_hour_of_week_scale_profile
            .hour_of_week_factors
            .clone(),
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .tick_grid_observation_model
            .tick_size,
        // Intentional copies because coarse simulation owns serialized calendar
        // and exchange identity independent of the shared workflow context.
        context.calendar_registry_artifact.clone(),
        context.model.session_resolution.exchange_code.clone(),
    )
    .map_err(|source| validation_error("coarse_model_inputs", source.to_string()))
}

pub(crate) fn build_sufficient_statistics_interval_factors(
    context: &FittedModelContext,
    hourly_factor_contract: HourlyFactorContractOutput,
) -> Result<SufficientStatisticsIntervalFactors, WorkflowError> {
    SufficientStatisticsIntervalFactors::try_new(
        hourly_factor_contract.s_h,
        hourly_factor_contract.known_component_nu_h,
        hourly_factor_contract.surprise_component_nu_h,
        Some(context.interval_delta),
    )
    .map_err(|source| validation_error("interval_factors", source.to_string()))
}
