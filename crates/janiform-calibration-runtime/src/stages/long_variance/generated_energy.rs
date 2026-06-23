use janiform_model_kernels::hawkes::{HawkesKernelParameters, HawkesStationaryTargetSourceRates};
use janiform_model_kernels::long_variance::{
    LongVarianceParameters, stationary_return_energy_rate_per_hour_from_return_generator,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;

use super::policy::{
    LongVarianceFeedbackLoadings, long_variance_stationary_event_rates_for_hawkes_kernel,
    project_long_variance_feedback_loadings_to_stationary_budget,
};

pub(crate) fn generated_stationary_return_energy_rate_per_hour(
    hawkes_kernel_parameters: HawkesKernelParameters,
    nig_configuration: NigSharedPlusShiftConfiguration,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
) -> Result<f64, String> {
    let driver_parameters = nig_configuration
        .derive_driver_parameters()
        .map_err(|source| format!("NIG driver parameters are invalid: {source}"))?;

    stationary_return_energy_rate_per_hour_from_return_generator(
        hawkes_kernel_parameters,
        HawkesStationaryTargetSourceRates::zero(),
        return_clock_exposure_parameters,
        driver_parameters,
    )
    .ok_or_else(|| "return generator implies an invalid stationary return-energy rate".to_string())
}

pub(crate) fn project_generated_stationary_feedback_to_budget(
    long_variance_parameters: LongVarianceParameters,
    hawkes_kernel_parameters: HawkesKernelParameters,
    nig_configuration: NigSharedPlusShiftConfiguration,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
) -> Result<LongVarianceParameters, String> {
    let stationary_return_energy_rate_per_hour = generated_stationary_return_energy_rate_per_hour(
        hawkes_kernel_parameters,
        nig_configuration,
        return_clock_exposure_parameters,
    )?;

    let stationary_event_rates =
        long_variance_stationary_event_rates_for_hawkes_kernel(hawkes_kernel_parameters)
            .ok_or_else(|| {
                "long-variance generated-energy projection requires stationary event rates"
                    .to_string()
            })?;

    let projected_feedback_loadings = project_long_variance_feedback_loadings_to_stationary_budget(
        LongVarianceFeedbackLoadings {
            stationary_return_energy_rate_per_hour,
            return_energy_loading: long_variance_parameters.return_energy_loading,
            buy_event_loading: long_variance_parameters.buy_event_loading,
            sell_event_loading: long_variance_parameters.sell_event_loading,
        },
        stationary_event_rates,
        long_variance_parameters.mean_reversion_rate,
    );

    LongVarianceParameters::try_new(
        long_variance_parameters.mean_reversion_rate,
        long_variance_parameters.vol_of_vol,
        projected_feedback_loadings.stationary_return_energy_rate_per_hour,
        projected_feedback_loadings.return_energy_loading,
        projected_feedback_loadings.buy_event_loading,
        projected_feedback_loadings.sell_event_loading,
    )
    .map_err(|source| format!("long-variance parameters are invalid: {source}"))
}
