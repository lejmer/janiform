use janiform::pricing::VanillaOptionPricingModelParameters;
use janiform_engine_pricing::VanillaOptionPricingKernelParameters;

use crate::error::WorkflowError;

use super::{FittedModelContext, validation_error};

pub(crate) fn build_physical_vanilla_option_model_parameters(
    context: &FittedModelContext,
) -> Result<VanillaOptionPricingModelParameters, WorkflowError> {
    VanillaOptionPricingModelParameters::try_new(
        context
            .fitted_parameters
            .hawkes_fitted_parameters
            .kernel_parameters,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration
            // Intentional copy because forecast model parameters own their
            // native contract after workflow preparation completes.
            .clone(),
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .return_clock_exposure_parameters,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .long_variance_parameters,
        VanillaOptionPricingKernelParameters::identity(),
    )
    .map_err(|source| validation_error("model_parameters", source.to_string()))
}

pub(crate) fn build_risk_neutral_vanilla_option_model_parameters(
    context: &FittedModelContext,
) -> Result<VanillaOptionPricingModelParameters, WorkflowError> {
    VanillaOptionPricingModelParameters::try_new(
        context.fitted_parameters.pricing_hawkes_kernel_parameters(),
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration
            // Intentional copy because pricing model parameters own their
            // native contract after workflow preparation completes.
            .clone(),
        context
            .fitted_parameters
            .pricing_nig_shared_plus_shift_configuration(),
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .return_clock_exposure_parameters,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .long_variance_parameters,
        context.fitted_parameters.pricing_kernel_parameters(),
    )
    .map_err(|source| validation_error("model_parameters", source.to_string()))
}
