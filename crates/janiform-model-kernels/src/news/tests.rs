use super::{
    COMPOSABLE_NEWS_MARK_INTERFACE_POLICY, GAMMA_NORMAL_NEWS_MARK_CONSTRUCTION_POLICY,
    GAMMA_SEVERITY_MARK_SAMPLING_POLICY, GammaNormalNewsMarkConstruction,
    GammaNormalNewsMarkConstructionError, GammaSeverityMarkConfiguration, NewsArrivalConfiguration,
    NewsArrivalIntensityEvaluationInput, NewsArrivalIntensityEvaluationOutput,
    NewsArrivalIntensityGenerator, NewsComponentMarkScaleConfiguration,
    NewsComponentMarkScaleConfigurationError, NewsMarkConfigurationError, NewsMarkInterfaceError,
    NewsProcessConfiguration, NewsReturnShockMarkSampler, NewsSeverityMarkSampler,
    SharedNewsMarkLawConfiguration, StandardNormalNewsReturnShockSampler,
    SurpriseNewsArrivalConfiguration,
};
use janiform_foundations::randomness::{
    SimulationRandomnessStream, SplitMix64SimulationRandomnessStream,
};

#[derive(Debug, Clone, Copy)]
struct MockSeverityMarkSampler {
    sampled_severity_mark: f64,
}

impl NewsSeverityMarkSampler for MockSeverityMarkSampler {
    fn sample_shared_severity_mark(
        &self,
        _randomness_stream: &mut dyn SimulationRandomnessStream,
    ) -> Result<f64, NewsMarkInterfaceError> {
        Ok(self.sampled_severity_mark)
    }
}

#[derive(Debug, Clone, Copy)]
struct MockReturnShockMarkSampler {
    sampled_return_shock_mark: f64,
}

impl NewsReturnShockMarkSampler for MockReturnShockMarkSampler {
    fn sample_shared_return_shock_mark(
        &self,
        _randomness_stream: &mut dyn SimulationRandomnessStream,
    ) -> Result<f64, NewsMarkInterfaceError> {
        Ok(self.sampled_return_shock_mark)
    }
}

#[derive(Debug, Clone, Copy)]
struct MockArrivalIntensityGenerator {
    generated_arrival_intensity: NewsArrivalIntensityEvaluationOutput,
}

impl NewsArrivalIntensityGenerator for MockArrivalIntensityGenerator {
    fn generate_arrival_intensity(
        &self,
        _intensity_input: NewsArrivalIntensityEvaluationInput,
    ) -> Result<NewsArrivalIntensityEvaluationOutput, NewsMarkInterfaceError> {
        Ok(self.generated_arrival_intensity)
    }
}

#[test]
fn gamma_severity_contract_uses_unit_mean_scale() {
    let configuration =
        GammaSeverityMarkConfiguration::try_new(2.5).expect("positive shape should construct");

    assert_eq!(configuration.expected_severity_mark(), Ok(1.0));
    assert_eq!(configuration.derived_unit_mean_scale(), 0.4);
}

#[test]
fn gamma_severity_contract_rejects_invalid_shape() {
    let error =
        GammaSeverityMarkConfiguration::try_new(0.0).expect_err("zero shape must be rejected");

    assert_eq!(
        error,
        NewsMarkConfigurationError::InvalidGammaShapeK { value: 0.0 }
    );
}

#[test]
fn gamma_normal_component_scale_accepts_signed_drift_and_rejects_negative_volatility() {
    let signed_drift_configuration = NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 0.4,
        return_jump_drift: -0.2,
        return_jump_volatility: 0.6,
    };

    assert_eq!(signed_drift_configuration.validate(), Ok(()));

    let negative_volatility_configuration = NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 0.4,
        return_jump_drift: 0.2,
        return_jump_volatility: -0.1,
    };
    let error = negative_volatility_configuration
        .validate()
        .expect_err("negative return-jump volatility must be rejected");

    assert_eq!(
        error,
        NewsComponentMarkScaleConfigurationError::InvalidReturnJumpVolatility { value: -0.1 }
    );

    let uncoupled_return_jump_configuration = NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 0.0,
        return_jump_drift: 0.1,
        return_jump_volatility: 0.0,
    };
    let uncoupled_error = uncoupled_return_jump_configuration
        .validate()
        .expect_err("return jumps must have an activity channel");

    assert_eq!(
        uncoupled_error,
        NewsComponentMarkScaleConfigurationError::ReturnJumpWithoutActivityCoupling {
            return_jump_drift: 0.1,
            return_jump_volatility: 0.0,
        }
    );
}

#[test]
fn gamma_normal_mark_constructor_couples_activity_and_return_to_shared_severity() {
    let component_scales = NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 0.25,
        return_jump_drift: -0.1,
        return_jump_volatility: 0.8,
    };
    let construction = component_scales
        .construct_gamma_normal_mark(4.0, -0.5)
        .expect("valid shared marks should construct");

    assert_eq!(
        construction,
        GammaNormalNewsMarkConstruction {
            shared_severity_mark: 4.0,
            shared_return_shock_mark: -0.5,
            activity_coupling_scale: 0.25,
            return_jump_drift: -0.1,
            return_jump_volatility: 0.8,
            activity_shock_mark: 1.0,
            return_jump_mark: -1.2000000000000002,
            construction_policy: GAMMA_NORMAL_NEWS_MARK_CONSTRUCTION_POLICY,
        }
    );
}

#[test]
fn gamma_normal_mark_constructor_rejects_invalid_shared_marks() {
    let component_scales = NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 0.25,
        return_jump_drift: 0.1,
        return_jump_volatility: 0.8,
    };

    let severity_error = component_scales
        .construct_gamma_normal_mark(-1.0, 0.0)
        .expect_err("negative severity must be rejected");
    assert_eq!(
        severity_error,
        GammaNormalNewsMarkConstructionError::InvalidSharedSeverityMark { value: -1.0 }
    );

    let normal_error = component_scales
        .construct_gamma_normal_mark(1.0, f64::INFINITY)
        .expect_err("non-finite standard-normal mark must be rejected");
    assert_eq!(
        normal_error,
        GammaNormalNewsMarkConstructionError::InvalidStandardNormalMark {
            value: f64::INFINITY
        }
    );
}

#[test]
fn composable_pipeline_wires_gamma_normal_components() {
    let generated_arrival_intensity = NewsArrivalIntensityEvaluationOutput {
        known_scheduled_arrival_count: 1,
        known_component_intensity: 0.4,
        surprise_open_component_intensity: 0.2,
        surprise_closed_component_intensity: 0.3,
        surprise_component_intensity: 0.5,
        total_intensity: 0.9,
        intensity_policy: "test.news.intensity.policy.v1",
    };
    let pipeline = super::ComposableNewsMarkPipeline::new(
        MockArrivalIntensityGenerator {
            generated_arrival_intensity,
        },
        MockSeverityMarkSampler {
            sampled_severity_mark: 4.0,
        },
        MockReturnShockMarkSampler {
            sampled_return_shock_mark: -0.5,
        },
    );
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(10);

    let arrival_output = pipeline
        .generate_arrival_intensity(NewsArrivalIntensityEvaluationInput {
            interval_start_utc_nanos: 0,
            interval_end_utc_nanos: 1,
            surprise_deterministic_time_multiplier: 1.0,
            surprise_open_window_share: 1.0,
            surprise_closed_window_share: 0.0,
        })
        .expect("mock arrival generator should produce valid intensity");
    let mark_payload = pipeline
        .sample_gamma_normal_marks(
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.25,
                return_jump_drift: -0.1,
                return_jump_volatility: 0.8,
            },
            &mut randomness_stream,
        )
        .expect("mock mark pipeline should construct valid payload");

    assert_eq!(arrival_output, generated_arrival_intensity);
    assert_eq!(mark_payload.shared_severity_mark, 4.0);
    assert_eq!(mark_payload.shared_return_shock_mark, -0.5);
    assert_eq!(
        mark_payload
            .gamma_normal_mark_construction
            .activity_shock_mark,
        1.0
    );
    assert_eq!(
        mark_payload.gamma_normal_mark_construction.return_jump_mark,
        -1.2000000000000002
    );
}

#[test]
fn composable_pipeline_rejects_invalid_return_shock_sampler_output() {
    let generated_arrival_intensity = NewsArrivalIntensityEvaluationOutput {
        known_scheduled_arrival_count: 0,
        known_component_intensity: 0.0,
        surprise_open_component_intensity: 0.0,
        surprise_closed_component_intensity: 0.0,
        surprise_component_intensity: 0.0,
        total_intensity: 0.0,
        intensity_policy: "test.news.intensity.policy.v1",
    };
    let pipeline = super::ComposableNewsMarkPipeline::new(
        MockArrivalIntensityGenerator {
            generated_arrival_intensity,
        },
        MockSeverityMarkSampler {
            sampled_severity_mark: 1.0,
        },
        MockReturnShockMarkSampler {
            sampled_return_shock_mark: f64::INFINITY,
        },
    );
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(20);
    let error = pipeline
        .sample_gamma_normal_marks(
            NewsComponentMarkScaleConfiguration::neutral(),
            &mut randomness_stream,
        )
        .expect_err("non-finite return-shock sample must be rejected");

    assert_eq!(
        error,
        NewsMarkInterfaceError::InvalidSampledReturnShockMark {
            value: f64::INFINITY
        }
    );
}

#[test]
fn news_arrival_configuration_materializes_open_and_closed_surprise_intensity() {
    let arrivals = NewsArrivalConfiguration {
        known: super::KnownNewsArrivalConfiguration {
            scheduled_event_timestamps_utc_nanos: vec![1_800_000_000_000],
        },
        surprise: SurpriseNewsArrivalConfiguration {
            open_baseline_intensity: 0.6,
            closed_baseline_intensity: 0.2,
        },
    };

    let output = arrivals
        .evaluate_with_deterministic_time_multiplier(NewsArrivalIntensityEvaluationInput {
            interval_start_utc_nanos: 10,
            interval_end_utc_nanos: 3_600_000_000_010,
            surprise_deterministic_time_multiplier: 2.0,
            surprise_open_window_share: 0.25,
            surprise_closed_window_share: 0.75,
        })
        .expect("valid arrivals should materialize interval intensity");

    assert_eq!(output.known_scheduled_arrival_count, 1);
    assert_eq!(output.known_component_intensity, 1.0);
    assert_eq!(output.surprise_open_component_intensity, 0.6 * 0.25 * 2.0);
    assert_eq!(output.surprise_closed_component_intensity, 0.2 * 0.75 * 2.0);
    assert_eq!(
        output.surprise_component_intensity,
        0.6 * 0.25 * 2.0 + 0.2 * 0.75 * 2.0
    );
    assert_eq!(
        output.total_intensity,
        1.0 + 0.6 * 0.25 * 2.0 + 0.2 * 0.75 * 2.0
    );
}

#[test]
fn news_process_configuration_builds_default_gamma_normal_pipeline() {
    let configuration = NewsProcessConfiguration::try_new(
        NewsArrivalConfiguration {
            known: super::KnownNewsArrivalConfiguration {
                scheduled_event_timestamps_utc_nanos: Vec::new(),
            },
            surprise: SurpriseNewsArrivalConfiguration {
                open_baseline_intensity: 0.0,
                closed_baseline_intensity: 0.0,
            },
        },
        SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.5)
                .expect("positive shape should construct"),
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 0.3,
            return_jump_drift: 0.1,
            return_jump_volatility: 0.2,
        },
        NewsComponentMarkScaleConfiguration::neutral(),
    )
    .expect("valid news process should construct");
    let pipeline = configuration.default_mark_pipeline();
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(30);

    let payload = pipeline
        .sample_gamma_normal_marks(
            configuration.known_component_mark_scales,
            &mut randomness_stream,
        )
        .expect("default pipeline should sample valid Gamma-normal marks");

    assert!(payload.shared_severity_mark.is_finite());
    assert!(payload.shared_severity_mark >= 0.0);
    assert!(payload.shared_return_shock_mark.is_finite());
    assert!(
        payload
            .gamma_normal_mark_construction
            .return_jump_mark
            .is_finite()
    );
}

#[test]
fn exported_news_policies_name_the_gamma_normal_contract() {
    assert_eq!(
        GAMMA_SEVERITY_MARK_SAMPLING_POLICY,
        "unit_mean_gamma_shape_with_derived_scale_marsaglia_tsang_shape_boost.v1"
    );
    assert_eq!(
        COMPOSABLE_NEWS_MARK_INTERFACE_POLICY,
        "component_arrival_intensity_then_gamma_severity_and_normal_mark_sampling_then_coupled_gamma_normal_construction.v1"
    );
}

#[test]
fn standard_normal_sampler_is_seed_reproducible() {
    let sampler = StandardNormalNewsReturnShockSampler;
    let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(77);
    let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(77);
    let first_draw = sampler
        .sample_shared_return_shock_mark(&mut first_stream)
        .expect("first draw should be finite");
    let second_draw = sampler
        .sample_shared_return_shock_mark(&mut second_stream)
        .expect("second draw should be finite");

    assert_eq!(first_draw.to_bits(), second_draw.to_bits());
}
