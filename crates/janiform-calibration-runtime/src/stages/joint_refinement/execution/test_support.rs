use janiform_calibration_artifacts::{
    HawkesStageFittedParameters, JointRefinementStageFittedParameters,
    ReturnComponentsStageFittedParameters, ReturnMarkHourOfWeekScaleProfile,
    SurpriseNewsDeterministicIntensityProfile, TickGridObservationModel,
};
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameters, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization,
};
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

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
                    baseline_intensity: 0.0,
                },
            },
            SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration {
                    shape_k: 1.5,
                    scale_theta: 0.8,
                },
                double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
                    probability_positive: 0.5,
                    eta_plus: 1.2,
                    eta_minus: 1.2,
                },
                coupled_gap_marks: CoupledGapMarkConfiguration { sigma_g: 0.2 },
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.0,
                return_jump_scale: 0.0,
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.0,
                return_jump_scale: 0.0,
            },
        )
        .expect("test news process configuration should validate"),
        surprise_deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile {
            weekly_hour_utc_multipliers: vec![1.0; 24 * 7],
            profile_resolution_hours: 1,
            fit_policy: "test".to_string(),
            normalization_policy: "test".to_string(),
        },
        return_mark_hour_of_week_scale_profile: ReturnMarkHourOfWeekScaleProfile {
            hour_of_week_factors: vec![1.0; 24 * 7],
            profile_resolution_hours: 1,
            fit_policy: "test".to_string(),
            normalization_policy: "test".to_string(),
        },
        tick_grid_observation_model: TickGridObservationModel {
            tick_size: None,
            tick_size_source: "test".to_string(),
            inference_policy: "test".to_string(),
            observation_policy: "test".to_string(),
        },
        mean_reversion_fitted_parameters:
            MeanReversionFittedParameters::zero_kappa_flat_anchor_mean_reversion(),
    };

    JointRefinementStageFittedParameters {
        hawkes_fitted_parameters,
        return_components_fitted_parameters,
        pricing_block_fitted_parameters: None,
    }
}
