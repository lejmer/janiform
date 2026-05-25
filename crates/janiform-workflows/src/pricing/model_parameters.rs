use janiform::pricing::VanillaOptionPricingModelParameters;

use crate::error::WorkflowError;
use crate::model::FittedModelContext;

use super::validation::pricing_validation_error;

pub(in crate::pricing) fn build_vanilla_option_pricing_model_parameters(
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
        context.fitted_parameters.pricing_kernel_parameters(),
    )
    .map_err(|source| pricing_validation_error("model_parameters", source.to_string()))
}
