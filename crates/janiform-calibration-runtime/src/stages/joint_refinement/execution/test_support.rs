use janiform_calibration_artifacts::{
    HawkesStageFittedParameters, JointRefinementStageFittedParameters,
    ReturnComponentsStageFittedParameters, SurpriseNewsDeterministicIntensityProfile,
    TickGridObservationModel,
};
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameters, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization,
};
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;
use janiform_model_kernels::news::{
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;

pub(crate) fn test_joint_fitted_parameters() -> JointRefinementStageFittedParameters {
    let hawkes_fitted_parameters = HawkesStageFittedParameters {
        kernel_parameters: HawkesKernelParameters::try_new(
            1.0,
            2.0,
            HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.1,
                minus_plus: 0.1,
                minus_minus: 0.2,
            },
            HawkesKernelLoadings {
                plus_plus: 0.05,
                plus_minus: 0.02,
                minus_plus: 0.02,
                minus_minus: 0.05,
            },
        )
        .expect("test Hawkes parameters should validate"),
        structured_alpha: HawkesStructuredAlphaParameterization::new(0.2, 0.1, 0.0, 0.0),
        structured_rho: HawkesStructuredRhoParameterization::new(0.05, 0.02, 0.0, 0.0),
        self_dominance_policy: None,
        stability_numeric_tolerance: 0.0,
    };

    let return_components_fitted_parameters = ReturnComponentsStageFittedParameters {
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration::try_new(
            2.0, -0.2, 0.5, 0.0, -0.1,
        )
        .expect("test NIG configuration should validate"),
        news_process_configuration: NewsProcessConfiguration::try_new(
            NewsArrivalConfiguration {
                known: KnownNewsArrivalConfiguration {
                    scheduled_event_timestamps_utc_nanos: Vec::new(),
                },
                surprise: SurpriseNewsArrivalConfiguration {
                    open_baseline_intensity: 0.0,
                    closed_baseline_intensity: 0.0,
                },
            },
            SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.5)
                    .expect("test gamma severity should validate"),
            },
            NewsComponentMarkScaleConfiguration::neutral(),
            NewsComponentMarkScaleConfiguration::neutral(),
        )
        .expect("test news process configuration should validate"),
        surprise_deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile {
            weekly_utc_profile_multipliers: vec![1.0; 24 * 7],
            profile_resolution_hours: 1,
            fit_policy: "test".to_string(),
            normalization_policy: "test".to_string(),
        },
        return_clock_exposure_parameters: ReturnClockExposureParameters::neutral(),
        long_variance_parameters:
            janiform_model_kernels::long_variance::LongVarianceParameters::neutral(),
        tick_grid_observation_model: TickGridObservationModel {
            tick_size: None,
            tick_size_source: "test".to_string(),
            inference_policy: "test".to_string(),
            observation_policy: "test".to_string(),
        },
        timestamp_batching_observation_model:
            janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel::disabled(
            ),
        mean_reversion_fitted_parameters:
            MeanReversionFittedParameters::zero_kappa_flat_anchor_mean_reversion(),
    };

    JointRefinementStageFittedParameters {
        hawkes_fitted_parameters,
        return_components_fitted_parameters,
        pricing_block_fitted_parameters: None,
    }
}
