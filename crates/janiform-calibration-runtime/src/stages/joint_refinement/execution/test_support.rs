use janiform_calibration_artifacts::{
    CalibrationLossAdmissibility, CalibrationLossFamilyDiagnostic, HawkesStageFittedParameters,
    JointRefinementStageFittedParameters, JointRefinementStageObjectiveBreakdown,
    PriceDiscretizationSurface, ReturnComponentsStageFittedParameters,
    SurpriseNewsDeterministicIntensityProfile,
};
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameters, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization,
};
use janiform_model_kernels::market_pressure::MarketPressureParameters;
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;
use janiform_model_kernels::news::{
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;

use crate::stages::return_components::ContinuousLikelihoodEvaluatorCounts;

use super::state::{
    JointRefinementCandidateEvaluation, JointRefinementCandidateOutcome,
    JointRefinementRawAdmissibilityMetrics,
};

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
        market_pressure_parameters: MarketPressureParameters::no_anchor_impact(),
        price_discretization_surface: PriceDiscretizationSurface {
            tick_size: None,
            tick_size_source: "test".to_string(),
            inference_policy: "test".to_string(),
            observation_policy: "test".to_string(),
        },
        timestamp_batching_surface:
            janiform_model_kernels::timestamp_batching_surface::TimestampBatchingSurface::disabled(),
        mean_reversion_fitted_parameters: MeanReversionFittedParameters::zero_kappa(),
    };

    JointRefinementStageFittedParameters {
        hawkes_fitted_parameters,
        return_components_fitted_parameters,
        pricing_block_fitted_parameters: None,
    }
}

pub(crate) fn test_joint_objective_breakdown(
    global_loss: f64,
) -> JointRefinementStageObjectiveBreakdown {
    JointRefinementStageObjectiveBreakdown {
        global_loss,
        family_losses: vec![CalibrationLossFamilyDiagnostic {
            family_id: "test_fit".to_string(),
            coordinate_count: 1,
            rank: 1,
            whitened_coordinates: vec![global_loss.sqrt()],
            loss: global_loss,
            max_standardized_residual: global_loss,
            worst_term_id: Some("test_fit".to_string()),
            unidentified: false,
        }],
        term_diagnostics: Vec::new(),
        domain_margin_diagnostics: Vec::new(),
        admissibility: CalibrationLossAdmissibility {
            admissible: true,
            max_standardized_residual: global_loss,
            excess_loss: 0.0,
            warnings: Vec::new(),
        },
    }
}

pub(crate) fn test_joint_raw_admissibility_metrics(
    value: f64,
) -> JointRefinementRawAdmissibilityMetrics {
    JointRefinementRawAdmissibilityMetrics {
        aggregated_likelihood_fit: value,
        aggregated_return_shape_fit: value,
        return_asymmetry_fit: value,
        tick_return_asymmetry_fit: value,
        aggregated_return_scale_fit: value,
        tick_return_scale_fit: value,
        business_time_return_scale_fit: value,
        intraday_return_level_fit: value,
        activity_shape_fit: value,
        activity_level_fit: value,
        rv_activity_coupling_fit: value,
        long_variance_horizon_fit: value,
    }
}

pub(crate) fn test_joint_candidate_evaluation(
    global_loss: f64,
) -> JointRefinementCandidateEvaluation {
    JointRefinementCandidateEvaluation {
        fitted_parameters: test_joint_fitted_parameters(),
        breakdown: test_joint_objective_breakdown(global_loss),
        raw_admissibility_metrics: test_joint_raw_admissibility_metrics(global_loss),
        exact_likelihood_evaluator_counts: ContinuousLikelihoodEvaluatorCounts::default(),
        objective_terms: Vec::new(),
        invalid_reason: None,
    }
}

pub(crate) fn test_joint_candidate_outcome(
    candidate_id: &str,
    global_loss: f64,
) -> JointRefinementCandidateOutcome {
    let evaluation = test_joint_candidate_evaluation(global_loss);

    JointRefinementCandidateOutcome {
        candidate_id: candidate_id.to_string(),
        parameter_vector: vec![global_loss],
        local_evaluation: evaluation.clone(),
        final_evaluation: evaluation,
        local_evaluation_count: 1,
        stop_reason: "test".to_string(),
    }
}
