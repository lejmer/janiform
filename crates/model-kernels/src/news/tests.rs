use super::{
    COMPOSABLE_NEWS_MARK_INTERFACE_POLICY, COUPLED_GAP_MARK_CONSTRUCTION_POLICY,
    ComposableNewsMarkPipeline, CoupledGapMarkConfiguration, CoupledGapMarkConstruction,
    CoupledGapMarkConstructionError, DOUBLE_EXPONENTIAL_BASE_SHOCK_SAMPLING_POLICY,
    DefaultNewsMarkPipeline, DoubleExponentialBaseShockConfiguration,
    DoubleExponentialBaseShockSamplingError, GAMMA_SEVERITY_MARK_SAMPLING_POLICY,
    GammaSeverityMarkConfiguration, GammaSeverityMarkSamplingError,
    NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY, NEWS_INTENSITY_BASELINE_POLICY,
    NEWS_INTENSITY_DETERMINISTIC_INPUT_POLICY, NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
    NewsArrivalComponent, NewsArrivalConfiguration, NewsArrivalIntensityEvaluationInput,
    NewsArrivalIntensityEvaluationOutput, NewsArrivalIntensityGenerator, NewsBaseShockMarkSampler,
    NewsComponentMarkScaleConfiguration, NewsCoupledMarkConstructor, NewsIntensityConfiguration,
    NewsMarkConfigurationError, NewsMarkInterfaceError, NewsMarkInterfaceStage,
    NewsProcessConfiguration, NewsSeverityMarkSampler, SHARED_NEWS_EVENT_BOUNDARY_POLICY,
    SHARED_NEWS_EVENT_DISPATCH_GT_GATING_POLICY, SHARED_NEWS_EVENT_DISPATCH_JT_UPDATE_POLICY,
    SHARED_NEWS_EVENT_DISPATCH_MT_GATING_POLICY, SHARED_NEWS_EVENT_DISPATCH_ORDERING_POLICY,
    SHARED_NEWS_EVENT_MARKET_STATE_DEPENDENCY, SharedNewsAccumulatorState,
    SharedNewsEventDispatchInput, SharedNewsEventMarketStateResolutionError,
    SharedNewsEventMarketStateResolver, SharedNewsEventTimestampedInput,
    SharedNewsMarkLawConfiguration, dispatch_shared_news_events,
    dispatch_shared_news_events_with_resolved_market_states,
    resolve_shared_news_event_market_states,
};
use chrono::{DateTime, Utc};
use janiform_foundations::market_time::MarketState;
use janiform_foundations::randomness::{
    SimulationRandomnessStream, SplitMix64SimulationRandomnessStream,
};
use serde_json::json;

const MOCK_COUPLED_MARK_POLICY: &str = "test.mock.coupled.mark.policy.v1";

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

#[derive(Debug, Clone)]
struct RejectingSeverityMarkSampler {
    rejection_reason: String,
}

impl NewsSeverityMarkSampler for RejectingSeverityMarkSampler {
    fn sample_shared_severity_mark(
        &self,
        _randomness_stream: &mut dyn SimulationRandomnessStream,
    ) -> Result<f64, NewsMarkInterfaceError> {
        Err(NewsMarkInterfaceError::component_rejected(
            NewsMarkInterfaceStage::SeverityMarkSampling,
            self.rejection_reason.clone(),
        ))
    }
}

#[derive(Debug, Clone, Copy)]
struct MockBaseShockMarkSampler {
    sampled_base_shock_mark: f64,
}

impl NewsBaseShockMarkSampler for MockBaseShockMarkSampler {
    fn sample_shared_base_shock_mark(
        &self,
        _randomness_stream: &mut dyn SimulationRandomnessStream,
    ) -> Result<f64, NewsMarkInterfaceError> {
        Ok(self.sampled_base_shock_mark)
    }
}

#[derive(Debug, Clone, Copy)]
struct MockCoupledMarkConstructor {
    severity_mark_override: Option<f64>,
    base_shock_mark_override: Option<f64>,
    gap_jump_mark_override: Option<f64>,
}

impl NewsCoupledMarkConstructor for MockCoupledMarkConstructor {
    fn construct_coupled_mark(
        &self,
        shared_severity_mark: f64,
        shared_base_shock_mark: f64,
    ) -> Result<CoupledGapMarkConstruction, NewsMarkInterfaceError> {
        let constructed_severity_mark = self.severity_mark_override.unwrap_or(shared_severity_mark);
        let constructed_base_shock_mark = self
            .base_shock_mark_override
            .unwrap_or(shared_base_shock_mark);
        let gap_jump_mark = self
            .gap_jump_mark_override
            .unwrap_or(constructed_severity_mark.sqrt() * constructed_base_shock_mark);

        Ok(CoupledGapMarkConstruction {
            shared_severity_mark: constructed_severity_mark,
            shared_base_shock_mark: constructed_base_shock_mark,
            sigma_g: 1.0,
            gap_jump_mark,
            construction_policy: MOCK_COUPLED_MARK_POLICY,
        })
    }
}

fn sample_arrival_configuration() -> NewsArrivalConfiguration {
    NewsArrivalConfiguration {
        known: super::KnownNewsArrivalConfiguration {
            scheduled_event_timestamps_utc_nanos: vec![
                1_710_000_000_000_000_000,
                1_710_000_000_000_000_500,
                1_710_000_000_000_001_000,
            ],
        },
        surprise: super::SurpriseNewsArrivalConfiguration {
            baseline_intensity: 0.75,
        },
    }
}

fn sample_shared_mark_law_configuration() -> SharedNewsMarkLawConfiguration {
    SharedNewsMarkLawConfiguration {
        gamma_severity_marks: GammaSeverityMarkConfiguration::new(1.8, 0.9),
        double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::new(0.35, 2.1, 3.0),
        coupled_gap_marks: CoupledGapMarkConfiguration::new(0.6),
    }
}

#[derive(Debug, Clone, Copy)]
struct MockNewsEventMarketStateResolver;

impl SharedNewsEventMarketStateResolver for MockNewsEventMarketStateResolver {
    fn resolve_market_state_at_utc(
        &self,
        exchange_code: &str,
        event_timestamp_utc: DateTime<Utc>,
    ) -> Result<MarketState, SharedNewsEventMarketStateResolutionError> {
        if exchange_code != "XNYS" {
            return Err(SharedNewsEventMarketStateResolutionError {
                reason: format!("unsupported exchange `{exchange_code}`"),
            });
        }

        let market_state = if event_timestamp_utc < parse_timestamp("2026-02-02T20:00:00Z") {
            MarketState::Open
        } else {
            MarketState::Closed
        };

        Ok(market_state)
    }
}

fn sample_process_configuration() -> NewsProcessConfiguration {
    NewsProcessConfiguration::try_new(
        sample_arrival_configuration(),
        sample_shared_mark_law_configuration(),
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 1.0,
            return_jump_scale: 1.0,
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 1.0,
            return_jump_scale: 1.0,
        },
    )
    .expect("valid process configuration should construct")
}

fn sample_arrival_intensity_input() -> NewsArrivalIntensityEvaluationInput {
    NewsArrivalIntensityEvaluationInput {
        interval_start_utc_nanos: 1_710_000_000_000_000_000,
        interval_end_utc_nanos: 1_710_000_000_000_001_000,
        surprise_deterministic_time_multiplier: 1.25,
    }
}

fn mock_arrival_intensity_output(
    known_component_intensity: f64,
    surprise_component_intensity: f64,
) -> NewsArrivalIntensityEvaluationOutput {
    let known_scheduled_arrival_count =
        if known_component_intensity.is_finite() && known_component_intensity >= 0.0 {
            known_component_intensity.floor() as u64
        } else {
            0
        };
    NewsArrivalIntensityEvaluationOutput {
        known_scheduled_arrival_count,
        known_component_intensity,
        surprise_component_intensity,
        total_intensity: known_component_intensity + surprise_component_intensity,
        intensity_policy: NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY,
    }
}

#[test]
fn news_process_configuration_round_trip_preserves_payload() {
    let configuration = sample_process_configuration();

    let serialized =
        serde_json::to_string(&configuration).expect("configuration payload should serialize");
    let parsed: NewsProcessConfiguration =
        serde_json::from_str(&serialized).expect("serialized payload should deserialize");

    assert_eq!(parsed, configuration);
    assert_eq!(
        parsed.schema_version,
        NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION
    );
}

#[test]
fn news_process_configuration_deserialization_reports_missing_required_field() {
    let payload = json!({
        "schema_version": NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
        "shared_mark_law": {
            "gamma_severity_marks": {
                "shape_k": 1.8,
                "scale_theta": 0.9
            },
            "double_exponential_base_shock": {
                "probability_positive": 0.35,
                "eta_plus": 2.1,
                "eta_minus": 3.0
            },
            "coupled_gap_marks": {
                "sigma_g": 0.6
            }
        },
        "known_component_mark_scales": {
            "activity_coupling_scale": 1.0,
            "return_jump_scale": 1.0
        },
        "surprise_component_mark_scales": {
            "activity_coupling_scale": 1.0,
            "return_jump_scale": 1.0
        }
    });

    let parse_result = serde_json::from_value::<NewsProcessConfiguration>(payload);
    let error_text = parse_result
        .expect_err("missing arrivals field must fail")
        .to_string();

    assert!(
        error_text.contains("missing field `arrivals`"),
        "unexpected error text: {error_text}"
    );
}

#[test]
fn news_process_configuration_deserialization_reports_invalid_field_value() {
    let payload = json!({
        "schema_version": NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION,
        "arrivals": {
            "known": {
                "scheduled_event_timestamps_utc_nanos": [1710000000000000000_i64]
            },
            "surprise": {
                "baseline_intensity": -0.1
            }
        },
        "shared_mark_law": {
            "gamma_severity_marks": {
                "shape_k": 1.8,
                "scale_theta": 0.9
            },
            "double_exponential_base_shock": {
                "probability_positive": 0.35,
                "eta_plus": 2.1,
                "eta_minus": 3.0
            },
            "coupled_gap_marks": {
                "sigma_g": 0.6
            }
        },
        "known_component_mark_scales": {
            "activity_coupling_scale": 1.0,
            "return_jump_scale": 1.0
        },
        "surprise_component_mark_scales": {
            "activity_coupling_scale": 1.0,
            "return_jump_scale": 1.0
        }
    });

    let parse_result = serde_json::from_value::<NewsProcessConfiguration>(payload);
    let error_text = parse_result
        .expect_err("invalid baseline intensity must fail")
        .to_string();

    assert!(
        error_text.contains("surprise.baseline_intensity must be a finite non-negative value"),
        "unexpected error text: {error_text}"
    );
}

#[test]
fn deterministic_intensity_model_matches_closed_window_policy_equation() {
    let intensity_configuration = NewsIntensityConfiguration::try_new(0.8)
        .expect("valid baseline intensity should construct");

    let interval_intensity = intensity_configuration
        .evaluate_with_deterministic_time_multiplier(0.75, 1.5)
        .expect("valid deterministic intensity inputs should evaluate");

    assert!((interval_intensity - 0.9).abs() <= 1e-12);
    assert_eq!(
        NEWS_INTENSITY_BASELINE_POLICY,
        "closed_window_share_times_baseline_intensity_times_deterministic_multiplier.v1"
    );
    assert_eq!(
        NEWS_INTENSITY_DETERMINISTIC_INPUT_POLICY,
        "interval_indexed_non_negative_multiplier_with_default_one.v1"
    );
}

#[test]
fn deterministic_intensity_model_rejects_out_of_domain_inputs() {
    let intensity_configuration = NewsIntensityConfiguration::new(0.8);

    let closed_share_error = intensity_configuration
        .evaluate_with_deterministic_time_multiplier(1.1, 1.0)
        .expect_err("closed_window_share above 1 must fail");
    assert_eq!(
        closed_share_error.to_string(),
        "closed_window_share must be finite and in [0, 1], found 1.1"
    );

    let multiplier_error = intensity_configuration
        .evaluate_with_deterministic_time_multiplier(0.5, -0.2)
        .expect_err("negative deterministic_time_multiplier must fail");
    assert_eq!(
        multiplier_error.to_string(),
        "deterministic_time_multiplier must be finite and non-negative, found -0.2"
    );
}

#[test]
fn default_composable_pipeline_wires_gamma_and_double_exponential_components() {
    let configuration = sample_process_configuration();
    let pipeline: DefaultNewsMarkPipeline = configuration.default_mark_pipeline();
    let intensity_input = sample_arrival_intensity_input();

    let generated_arrival_intensity = pipeline
        .generate_arrival_intensity(intensity_input)
        .expect("default pipeline should evaluate arrival intensity");
    let expected_arrival_intensity = configuration
        .arrivals
        .evaluate_with_deterministic_time_multiplier(intensity_input)
        .expect("deterministic intensity inputs should evaluate");

    assert_eq!(generated_arrival_intensity, expected_arrival_intensity);
    assert_eq!(
        generated_arrival_intensity.intensity_policy,
        NEWS_ARRIVAL_COMPONENT_INTENSITY_POLICY
    );
    assert_eq!(
        COMPOSABLE_NEWS_MARK_INTERFACE_POLICY,
        "component_arrival_intensity_then_shared_mark_sampling_then_coupled_mark_construction.v1"
    );

    let mut pipeline_stream = SplitMix64SimulationRandomnessStream::from_seed(90_011);
    let sampled_payload = pipeline
        .sample_coupled_marks(&mut pipeline_stream)
        .expect("default pipeline should sample coupled marks");

    let mut expected_stream = SplitMix64SimulationRandomnessStream::from_seed(90_011);
    let expected_severity_mark = configuration
        .shared_mark_law
        .gamma_severity_marks
        .sample_severity_mark(&mut expected_stream)
        .expect("gamma severity sampler should succeed");
    let expected_base_shock_mark = configuration
        .shared_mark_law
        .double_exponential_base_shock
        .sample_base_shock_mark(&mut expected_stream)
        .expect("double-exponential base-shock sampler should succeed");
    let expected_construction = configuration
        .shared_mark_law
        .coupled_gap_marks
        .construct_gap_jump_mark(expected_severity_mark, expected_base_shock_mark)
        .expect("coupled gap-mark constructor should succeed");

    assert_eq!(sampled_payload.shared_severity_mark, expected_severity_mark);
    assert_eq!(
        sampled_payload.shared_base_shock_mark,
        expected_base_shock_mark
    );
    assert_eq!(
        sampled_payload.coupled_gap_mark_construction,
        expected_construction
    );
}

#[test]
fn composable_pipeline_supports_pluggable_component_implementations() {
    let pipeline = ComposableNewsMarkPipeline::new(
        MockArrivalIntensityGenerator {
            generated_arrival_intensity: mock_arrival_intensity_output(1.0, 0.4),
        },
        MockSeverityMarkSampler {
            sampled_severity_mark: 2.25,
        },
        MockBaseShockMarkSampler {
            sampled_base_shock_mark: -0.8,
        },
        MockCoupledMarkConstructor {
            severity_mark_override: None,
            base_shock_mark_override: None,
            gap_jump_mark_override: Some(-1.2),
        },
    );
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(10);

    let generated_arrival_intensity = pipeline
        .generate_arrival_intensity(sample_arrival_intensity_input())
        .expect("mock arrival generator should produce intensity");
    let sampled_payload = pipeline
        .sample_coupled_marks(&mut randomness_stream)
        .expect("mock component wiring should generate coupled marks");

    assert!((generated_arrival_intensity.total_intensity - 1.4).abs() <= 1e-12);
    assert_eq!(sampled_payload.shared_severity_mark, 2.25);
    assert_eq!(sampled_payload.shared_base_shock_mark, -0.8);
    assert_eq!(
        sampled_payload.coupled_gap_mark_construction.gap_jump_mark,
        -1.2
    );
    assert_eq!(
        sampled_payload
            .coupled_gap_mark_construction
            .construction_policy,
        MOCK_COUPLED_MARK_POLICY
    );
}

#[test]
fn composable_pipeline_contract_rejects_invalid_arrival_intensity_output() {
    let pipeline = ComposableNewsMarkPipeline::new(
        MockArrivalIntensityGenerator {
            generated_arrival_intensity: mock_arrival_intensity_output(f64::NAN, 0.1),
        },
        MockSeverityMarkSampler {
            sampled_severity_mark: 1.0,
        },
        MockBaseShockMarkSampler {
            sampled_base_shock_mark: 1.0,
        },
        MockCoupledMarkConstructor {
            severity_mark_override: None,
            base_shock_mark_override: None,
            gap_jump_mark_override: None,
        },
    );

    let error = pipeline
        .generate_arrival_intensity(sample_arrival_intensity_input())
        .expect_err("non-finite arrival intensity must fail contract validation");
    assert!(matches!(
        error,
        NewsMarkInterfaceError::InvalidGeneratedKnownArrivalIntensity { value }
        if value.is_nan()
    ));
}

#[test]
fn composable_pipeline_contract_rejects_invalid_severity_and_base_shock_outputs() {
    let severity_violation_pipeline = ComposableNewsMarkPipeline::new(
        MockArrivalIntensityGenerator {
            generated_arrival_intensity: mock_arrival_intensity_output(0.7, 0.3),
        },
        MockSeverityMarkSampler {
            sampled_severity_mark: -0.01,
        },
        MockBaseShockMarkSampler {
            sampled_base_shock_mark: 1.0,
        },
        MockCoupledMarkConstructor {
            severity_mark_override: None,
            base_shock_mark_override: None,
            gap_jump_mark_override: None,
        },
    );

    let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(11);
    let severity_error = severity_violation_pipeline
        .sample_coupled_marks(&mut first_stream)
        .expect_err("negative severity mark must fail contract validation");

    assert!(matches!(
        severity_error,
        NewsMarkInterfaceError::InvalidSampledSeverityMark { value } if value == -0.01
    ));

    let base_shock_violation_pipeline = ComposableNewsMarkPipeline::new(
        MockArrivalIntensityGenerator {
            generated_arrival_intensity: mock_arrival_intensity_output(0.7, 0.3),
        },
        MockSeverityMarkSampler {
            sampled_severity_mark: 1.0,
        },
        MockBaseShockMarkSampler {
            sampled_base_shock_mark: f64::INFINITY,
        },
        MockCoupledMarkConstructor {
            severity_mark_override: None,
            base_shock_mark_override: None,
            gap_jump_mark_override: None,
        },
    );

    let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(12);
    let base_shock_error = base_shock_violation_pipeline
        .sample_coupled_marks(&mut second_stream)
        .expect_err("non-finite base-shock mark must fail contract validation");

    assert!(matches!(
        base_shock_error,
        NewsMarkInterfaceError::InvalidSampledBaseShockMark { value }
        if value.is_infinite()
    ));
}

#[test]
fn composable_pipeline_contract_rejects_coupled_mark_output_violations() {
    let severity_mismatch_pipeline = ComposableNewsMarkPipeline::new(
        MockArrivalIntensityGenerator {
            generated_arrival_intensity: mock_arrival_intensity_output(0.7, 0.3),
        },
        MockSeverityMarkSampler {
            sampled_severity_mark: 2.0,
        },
        MockBaseShockMarkSampler {
            sampled_base_shock_mark: -0.5,
        },
        MockCoupledMarkConstructor {
            severity_mark_override: Some(3.0),
            base_shock_mark_override: None,
            gap_jump_mark_override: Some(-0.75),
        },
    );

    let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(13);
    let severity_mismatch_error = severity_mismatch_pipeline
        .sample_coupled_marks(&mut first_stream)
        .expect_err("severity mismatch must fail contract validation");

    assert!(matches!(
        severity_mismatch_error,
        NewsMarkInterfaceError::ConstructedSeverityMarkMismatch { expected, observed }
        if expected == 2.0 && observed == 3.0
    ));

    let non_finite_gap_pipeline = ComposableNewsMarkPipeline::new(
        MockArrivalIntensityGenerator {
            generated_arrival_intensity: mock_arrival_intensity_output(0.7, 0.3),
        },
        MockSeverityMarkSampler {
            sampled_severity_mark: 2.0,
        },
        MockBaseShockMarkSampler {
            sampled_base_shock_mark: -0.5,
        },
        MockCoupledMarkConstructor {
            severity_mark_override: None,
            base_shock_mark_override: None,
            gap_jump_mark_override: Some(f64::NAN),
        },
    );

    let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(14);
    let non_finite_gap_error = non_finite_gap_pipeline
        .sample_coupled_marks(&mut second_stream)
        .expect_err("non-finite coupled gap mark must fail contract validation");

    assert!(matches!(
        non_finite_gap_error,
        NewsMarkInterfaceError::InvalidConstructedGapMark { value } if value.is_nan()
    ));
}

#[test]
fn composable_pipeline_propagates_component_rejections_from_pluggable_components() {
    let pipeline = ComposableNewsMarkPipeline::new(
        MockArrivalIntensityGenerator {
            generated_arrival_intensity: mock_arrival_intensity_output(0.7, 0.3),
        },
        RejectingSeverityMarkSampler {
            rejection_reason: "mock rejection for severity stage".to_string(),
        },
        MockBaseShockMarkSampler {
            sampled_base_shock_mark: 1.0,
        },
        MockCoupledMarkConstructor {
            severity_mark_override: None,
            base_shock_mark_override: None,
            gap_jump_mark_override: None,
        },
    );

    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(15);

    let error = pipeline
        .sample_coupled_marks(&mut randomness_stream)
        .expect_err("component rejection should propagate");

    assert!(matches!(
        error,
        NewsMarkInterfaceError::ComponentRejected {
            stage: NewsMarkInterfaceStage::SeverityMarkSampling,
            reason,
        } if reason == "mock rejection for severity stage"
    ));
}

#[test]
fn gamma_severity_configuration_try_new_rejects_invalid_parameters() {
    let invalid_shape_error = GammaSeverityMarkConfiguration::try_new(0.0, 1.0)
        .expect_err("shape_k must be strictly positive");
    assert!(matches!(
        invalid_shape_error,
        NewsMarkConfigurationError::InvalidGammaShapeK { value } if value == 0.0
    ));

    let invalid_scale_error = GammaSeverityMarkConfiguration::try_new(1.0, -0.5)
        .expect_err("scale_theta must be strictly positive");
    assert!(matches!(
        invalid_scale_error,
        NewsMarkConfigurationError::InvalidGammaScaleTheta { value } if value == -0.5
    ));
}

#[test]
fn gamma_severity_sampler_rejects_invalid_configuration_at_sampling_time() {
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(44_101);
    let invalid_configuration = GammaSeverityMarkConfiguration::new(-1.0, 0.8);

    let sampling_error = invalid_configuration
        .sample_severity_mark(&mut randomness_stream)
        .expect_err("invalid shape parameter must fail fast before sampling");

    assert!(matches!(
        sampling_error,
        GammaSeverityMarkSamplingError::InvalidConfiguration {
            source: NewsMarkConfigurationError::InvalidGammaShapeK { value },
        } if value == -1.0
    ));
}

#[test]
fn gamma_severity_sampler_outputs_finite_non_negative_marks() {
    let configuration = GammaSeverityMarkConfiguration::try_new(2.25, 0.6)
        .expect("valid gamma severity configuration should construct");
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(55_101);

    let mut sampled_marks: Vec<f64> = Vec::with_capacity(2_048);
    for _sample_index in 0..2_048 {
        let sampled_mark = configuration
            .sample_severity_mark(&mut randomness_stream)
            .expect("sampling with valid parameters should succeed");
        sampled_marks.push(sampled_mark);
    }

    assert!(sampled_marks.iter().all(|sample| sample.is_finite()));
    assert!(sampled_marks.iter().all(|sample| *sample >= 0.0));
}

#[test]
fn gamma_severity_sampler_is_seed_reproducible() {
    let configuration = GammaSeverityMarkConfiguration::try_new(1.7, 1.2)
        .expect("valid gamma severity configuration should construct");
    let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(77_201);
    let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(77_201);
    let mut different_seed_stream = SplitMix64SimulationRandomnessStream::from_seed(77_202);

    let first_draws: Vec<f64> = (0..64)
        .map(|_| {
            configuration
                .sample_severity_mark(&mut first_stream)
                .expect("first seeded stream should sample")
        })
        .collect();
    let second_draws: Vec<f64> = (0..64)
        .map(|_| {
            configuration
                .sample_severity_mark(&mut second_stream)
                .expect("second seeded stream should sample")
        })
        .collect();
    let different_seed_draws: Vec<f64> = (0..64)
        .map(|_| {
            configuration
                .sample_severity_mark(&mut different_seed_stream)
                .expect("different seeded stream should sample")
        })
        .collect();

    assert_eq!(first_draws, second_draws);
    assert_ne!(first_draws, different_seed_draws);
    assert_eq!(
        GAMMA_SEVERITY_MARK_SAMPLING_POLICY,
        "marsaglia_tsang_shape_scale_with_shape_less_than_one_boost.v1"
    );
}

#[test]
fn gamma_severity_sampler_mean_tracks_shape_scale_expectation() {
    let shape_k = 2.4;
    let scale_theta = 0.75;
    let expected_mean = shape_k * scale_theta;
    let sample_count = 40_000;

    let configuration = GammaSeverityMarkConfiguration::try_new(shape_k, scale_theta)
        .expect("valid gamma severity configuration should construct");
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(91_502);

    let mut sample_sum = 0.0;
    for _sample_index in 0..sample_count {
        sample_sum += configuration
            .sample_severity_mark(&mut randomness_stream)
            .expect("sampling with valid parameters should succeed");
    }
    let sample_mean = sample_sum / sample_count as f64;

    let relative_error = (sample_mean - expected_mean).abs() / expected_mean;
    assert!(
        relative_error <= 0.03,
        "sample mean should stay close to expectation, expected_mean={expected_mean}, sample_mean={sample_mean}, relative_error={relative_error}"
    );
}

#[test]
fn double_exponential_base_shock_try_new_rejects_invalid_parameters() {
    let invalid_probability_low = DoubleExponentialBaseShockConfiguration::try_new(0.0, 2.1, 3.0)
        .expect_err("probability_positive must be strictly between 0 and 1");
    assert!(matches!(
        invalid_probability_low,
        NewsMarkConfigurationError::InvalidPositiveJumpProbability { value } if value == 0.0
    ));

    let invalid_probability_high = DoubleExponentialBaseShockConfiguration::try_new(1.0, 2.1, 3.0)
        .expect_err("probability_positive must be strictly between 0 and 1");
    assert!(matches!(
        invalid_probability_high,
        NewsMarkConfigurationError::InvalidPositiveJumpProbability { value } if value == 1.0
    ));

    let invalid_eta_plus = DoubleExponentialBaseShockConfiguration::try_new(0.35, -0.2, 3.0)
        .expect_err("eta_plus must be strictly positive");
    assert!(matches!(
        invalid_eta_plus,
        NewsMarkConfigurationError::InvalidEtaPlus { value } if value == -0.2
    ));

    let invalid_eta_minus = DoubleExponentialBaseShockConfiguration::try_new(0.35, 2.1, 0.0)
        .expect_err("eta_minus must be strictly positive");
    assert!(matches!(
        invalid_eta_minus,
        NewsMarkConfigurationError::InvalidEtaMinus { value } if value == 0.0
    ));
}

#[test]
fn double_exponential_base_shock_sampler_rejects_invalid_configuration_at_sampling_time() {
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(44_102);
    let invalid_configuration = DoubleExponentialBaseShockConfiguration::new(1.2, 2.1, 3.0);

    let sampling_error = invalid_configuration
        .sample_base_shock_mark(&mut randomness_stream)
        .expect_err("invalid p parameter must fail fast before sampling");

    assert!(matches!(
        sampling_error,
        DoubleExponentialBaseShockSamplingError::InvalidConfiguration {
            source: NewsMarkConfigurationError::InvalidPositiveJumpProbability { value },
        } if value == 1.2
    ));
}

#[test]
fn double_exponential_base_shock_sampler_outputs_two_sided_tail_shape() {
    let probability_positive = 0.35;
    let eta_plus = 1.2;
    let eta_minus = 2.4;
    let sample_count = 80_000;
    let expected_positive_mean = 1.0 / eta_plus;
    let expected_negative_absolute_mean = 1.0 / eta_minus;

    let configuration =
        DoubleExponentialBaseShockConfiguration::try_new(probability_positive, eta_plus, eta_minus)
            .expect("valid base-shock configuration should construct");
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(98_113);

    let mut positive_count = 0_usize;
    let mut negative_count = 0_usize;
    let mut positive_sum = 0.0;
    let mut negative_absolute_sum = 0.0;

    for _sample_index in 0..sample_count {
        let sampled_mark = configuration
            .sample_base_shock_mark(&mut randomness_stream)
            .expect("sampling with valid base-shock parameters should succeed");
        assert!(sampled_mark.is_finite());

        if sampled_mark >= 0.0 {
            positive_count += 1;
            positive_sum += sampled_mark;
        } else {
            negative_count += 1;
            negative_absolute_sum += -sampled_mark;
        }
    }

    assert!(positive_count > 0);
    assert!(negative_count > 0);

    let positive_frequency = positive_count as f64 / sample_count as f64;
    let positive_mean = positive_sum / positive_count as f64;
    let negative_absolute_mean = negative_absolute_sum / negative_count as f64;

    assert!(
        (positive_frequency - probability_positive).abs() <= 0.015,
        "positive-frequency sanity check failed, expected_probability={probability_positive}, observed_probability={positive_frequency}"
    );
    assert!(
        (positive_mean - expected_positive_mean).abs() / expected_positive_mean <= 0.06,
        "positive-tail mean sanity check failed, expected_mean={expected_positive_mean}, observed_mean={positive_mean}"
    );
    assert!(
        (negative_absolute_mean - expected_negative_absolute_mean).abs()
            / expected_negative_absolute_mean
            <= 0.06,
        "negative-tail mean sanity check failed, expected_mean={expected_negative_absolute_mean}, observed_mean={negative_absolute_mean}"
    );
    assert!(
        positive_mean > negative_absolute_mean,
        "tail-shape sanity check failed, expected positive tail to be heavier for eta_plus={eta_plus} and eta_minus={eta_minus}, observed positive_mean={positive_mean}, negative_absolute_mean={negative_absolute_mean}"
    );
    assert_eq!(
        DOUBLE_EXPONENTIAL_BASE_SHOCK_SAMPLING_POLICY,
        "kou_two_sided_exponential_with_constant_mixture_probability.v1"
    );
}

#[test]
fn double_exponential_base_shock_sampler_is_seed_reproducible() {
    let configuration = DoubleExponentialBaseShockConfiguration::try_new(0.35, 1.8, 2.7)
        .expect("valid base-shock configuration should construct");

    let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(66_777);
    let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(66_777);
    let mut different_seed_stream = SplitMix64SimulationRandomnessStream::from_seed(66_778);

    let first_draws: Vec<f64> = (0..64)
        .map(|_| {
            configuration
                .sample_base_shock_mark(&mut first_stream)
                .expect("first seeded stream should sample")
        })
        .collect();
    let second_draws: Vec<f64> = (0..64)
        .map(|_| {
            configuration
                .sample_base_shock_mark(&mut second_stream)
                .expect("second seeded stream should sample")
        })
        .collect();
    let different_seed_draws: Vec<f64> = (0..64)
        .map(|_| {
            configuration
                .sample_base_shock_mark(&mut different_seed_stream)
                .expect("different seeded stream should sample")
        })
        .collect();

    assert_eq!(first_draws, second_draws);
    assert_ne!(first_draws, different_seed_draws);
}

#[test]
fn coupled_gap_mark_constructor_rejects_invalid_sigma_configuration() {
    let construction_error = CoupledGapMarkConfiguration::new(-0.01)
        .construct_gap_jump_mark(1.25, -0.5)
        .expect_err("negative sigma_g must fail gap-mark construction");
    assert!(matches!(
        construction_error,
        CoupledGapMarkConstructionError::InvalidConfiguration {
            source: NewsMarkConfigurationError::InvalidSigmaG { value }
        } if value == -0.01
    ));

    let non_finite_configuration_error = CoupledGapMarkConfiguration::new(f64::INFINITY)
        .construct_gap_jump_mark(1.25, -0.5)
        .expect_err("non-finite sigma_g must fail gap-mark construction");
    assert!(matches!(
        non_finite_configuration_error,
        CoupledGapMarkConstructionError::InvalidConfiguration {
            source: NewsMarkConfigurationError::InvalidSigmaG { value }
        } if value.is_infinite()
    ));
}

#[test]
fn coupled_gap_mark_constructor_deterministically_scales_shared_marks() {
    let construction = CoupledGapMarkConfiguration::new(0.3)
        .construct_gap_jump_mark(4.0, -1.25)
        .expect("valid coupled-mark payload must construct");

    assert!((construction.gap_jump_mark - (-0.75)).abs() <= 1e-12);
    assert_eq!(construction.shared_severity_mark, 4.0);
    assert_eq!(construction.shared_base_shock_mark, -1.25);
    assert_eq!(construction.sigma_g, 0.3);
    assert_eq!(
        construction.construction_policy,
        COUPLED_GAP_MARK_CONSTRUCTION_POLICY
    );
}

#[test]
fn coupled_gap_mark_constructor_preserves_shared_severity_for_hawkes_and_gap_consumers() {
    let shared_severity_mark = 2.25;
    let shared_base_shock_mark = 0.8;
    let sigma_g = 0.4;

    let construction = CoupledGapMarkConfiguration::new(sigma_g)
        .construct_gap_jump_mark(shared_severity_mark, shared_base_shock_mark)
        .expect("valid shared marks must construct");

    let hawkes_consumer_severity_mark = construction.shared_severity_mark;
    let replayed_gap_jump_mark = construction.sigma_g
        * hawkes_consumer_severity_mark.sqrt()
        * construction.shared_base_shock_mark;

    assert_eq!(hawkes_consumer_severity_mark, shared_severity_mark);
    assert!(
        (construction.gap_jump_mark - replayed_gap_jump_mark).abs() <= 1e-12,
        "gap consumer must use the same shared severity mark used by Hawkes loading"
    );
}

#[test]
fn shared_news_event_dispatch_maps_each_news_event_to_one_jt_increment() {
    let event_inputs = vec![
        SharedNewsEventDispatchInput {
            event_timestamp_utc_nanos: 300,
            event_generation_order: 2,
            news_arrival_component: NewsArrivalComponent::Surprise,
            market_state: MarketState::Open,
            shared_severity_mark: 1.25,
            coupled_return_jump_mark: -0.3,
        },
        SharedNewsEventDispatchInput {
            event_timestamp_utc_nanos: 100,
            event_generation_order: 0,
            news_arrival_component: NewsArrivalComponent::Known,
            market_state: MarketState::Closed,
            shared_severity_mark: 0.4,
            coupled_return_jump_mark: -0.2,
        },
        SharedNewsEventDispatchInput {
            event_timestamp_utc_nanos: 200,
            event_generation_order: 1,
            news_arrival_component: NewsArrivalComponent::Surprise,
            market_state: MarketState::Closed,
            shared_severity_mark: 0.8,
            coupled_return_jump_mark: 0.1,
        },
    ];

    let dispatch_output = dispatch_shared_news_events(
        SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
        &event_inputs,
    )
    .expect("valid shared dispatch inputs should dispatch");

    assert_eq!(dispatch_output.records.len(), event_inputs.len());

    let mut expected_pairs: Vec<(u64, f64)> = event_inputs
        .iter()
        .map(|event_input| {
            (
                event_input.event_generation_order,
                event_input.shared_severity_mark,
            )
        })
        .collect();
    expected_pairs.sort_by_key(|pair| pair.0);

    let observed_pairs: Vec<(u64, f64)> = dispatch_output
        .records
        .iter()
        .map(|record| (record.event_generation_order, record.j_t_increment))
        .collect();
    assert_eq!(observed_pairs, expected_pairs);

    let mut running_j_t = 0.0;
    for record in &dispatch_output.records {
        running_j_t += record.shared_severity_mark;
        assert_eq!(record.j_t_increment, record.shared_severity_mark);
        assert!((record.j_t_after_event - running_j_t).abs() <= 1e-12);
    }
}

#[test]
fn shared_news_event_dispatch_couples_jt_with_closed_window_gt_updates() {
    let event_inputs = vec![
        SharedNewsEventDispatchInput {
            event_timestamp_utc_nanos: 10,
            event_generation_order: 0,
            news_arrival_component: NewsArrivalComponent::Known,
            market_state: MarketState::Open,
            shared_severity_mark: 0.6,
            coupled_return_jump_mark: 1.1,
        },
        SharedNewsEventDispatchInput {
            event_timestamp_utc_nanos: 20,
            event_generation_order: 1,
            news_arrival_component: NewsArrivalComponent::Surprise,
            market_state: MarketState::Closed,
            shared_severity_mark: 0.9,
            coupled_return_jump_mark: -0.35,
        },
    ];

    let initial_state = SharedNewsAccumulatorState::new(1.2, 0.15, 0.8);
    let dispatch_output = dispatch_shared_news_events(initial_state, &event_inputs)
        .expect("valid shared dispatch inputs should dispatch");

    assert_eq!(
        dispatch_output.records[0].m_t_increment, 1.1,
        "open-window event should emit direct jump into M_t"
    );
    assert_eq!(
        dispatch_output.records[0].g_t_increment, 0.0,
        "open-window event must not emit a direct gap increment"
    );
    assert_eq!(
        dispatch_output.records[1].m_t_increment, 0.0,
        "closed-window event must not emit direct jump into M_t"
    );
    assert_eq!(
        dispatch_output.records[1].g_t_increment, -0.35,
        "closed-window event should emit the coupled gap increment"
    );

    assert!((dispatch_output.final_state.j_t - (1.2 + 0.6 + 0.9)).abs() <= 1e-12);
    assert!((dispatch_output.final_state.m_t - (0.15 + 1.1)).abs() <= 1e-12);
    assert!((dispatch_output.final_state.g_t - (0.8 - 0.35)).abs() <= 1e-12);
}

#[test]
fn shared_news_event_dispatch_is_deterministic_under_seeded_tie_ordering_inputs() {
    let first_seeded_inputs = build_seeded_dispatch_inputs(8_110_222, 32);
    let mut reversed_inputs = first_seeded_inputs.clone();
    reversed_inputs.reverse();

    let first_output = dispatch_shared_news_events(
        SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
        &first_seeded_inputs,
    )
    .expect("first seeded dispatch should succeed");
    let second_output = dispatch_shared_news_events(
        SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
        &reversed_inputs,
    )
    .expect("second seeded dispatch should succeed");

    assert_eq!(first_output, second_output);
    assert_eq!(
        first_output.ordering_policy,
        SHARED_NEWS_EVENT_DISPATCH_ORDERING_POLICY
    );
    assert_eq!(
        first_output.j_t_update_policy,
        SHARED_NEWS_EVENT_DISPATCH_JT_UPDATE_POLICY
    );
    assert_eq!(
        first_output.g_t_gating_policy,
        SHARED_NEWS_EVENT_DISPATCH_GT_GATING_POLICY
    );
    assert_eq!(
        first_output.m_t_gating_policy,
        SHARED_NEWS_EVENT_DISPATCH_MT_GATING_POLICY
    );
    assert_eq!(
        first_output.market_state_dependency,
        SHARED_NEWS_EVENT_MARKET_STATE_DEPENDENCY
    );
    assert_eq!(
        first_output.market_state_boundary_policy,
        SHARED_NEWS_EVENT_BOUNDARY_POLICY
    );

    assert!(first_output.records.windows(2).all(|record_pair| {
        let first_record = &record_pair[0];
        let second_record = &record_pair[1];
        (
            first_record.event_timestamp_utc_nanos,
            first_record.event_generation_order,
        ) <= (
            second_record.event_timestamp_utc_nanos,
            second_record.event_generation_order,
        )
    }));
}

#[test]
fn shared_news_event_market_state_resolution_uses_canonical_service_outputs() {
    let market_state_resolver = MockNewsEventMarketStateResolver;
    let event_inputs = vec![
        SharedNewsEventTimestampedInput {
            event_timestamp_utc_nanos: timestamp_utc_nanos("2026-02-02T14:30:00Z"),
            event_generation_order: 0,
            news_arrival_component: NewsArrivalComponent::Known,
            shared_severity_mark: 0.5,
            coupled_return_jump_mark: 0.2,
        },
        SharedNewsEventTimestampedInput {
            event_timestamp_utc_nanos: timestamp_utc_nanos("2026-02-02T21:00:00Z"),
            event_generation_order: 1,
            news_arrival_component: NewsArrivalComponent::Surprise,
            shared_severity_mark: 0.7,
            coupled_return_jump_mark: -0.3,
        },
    ];

    let resolved_event_inputs =
        resolve_shared_news_event_market_states(&market_state_resolver, "XNYS", &event_inputs)
            .expect("market states should resolve from calendar service");

    assert_eq!(resolved_event_inputs.len(), 2);
    assert_eq!(resolved_event_inputs[0].market_state, MarketState::Open);
    assert_eq!(resolved_event_inputs[1].market_state, MarketState::Closed);
}

#[test]
fn shared_news_event_dispatch_with_resolved_market_states_applies_open_close_boundaries() {
    let market_state_resolver = MockNewsEventMarketStateResolver;
    let event_inputs = vec![
        SharedNewsEventTimestampedInput {
            event_timestamp_utc_nanos: timestamp_utc_nanos("2026-02-02T14:30:00Z"),
            event_generation_order: 0,
            news_arrival_component: NewsArrivalComponent::Known,
            shared_severity_mark: 0.6,
            coupled_return_jump_mark: 1.1,
        },
        SharedNewsEventTimestampedInput {
            event_timestamp_utc_nanos: timestamp_utc_nanos("2026-02-02T21:00:00Z"),
            event_generation_order: 1,
            news_arrival_component: NewsArrivalComponent::Surprise,
            shared_severity_mark: 0.9,
            coupled_return_jump_mark: -0.35,
        },
    ];

    let dispatch_output = dispatch_shared_news_events_with_resolved_market_states(
        &market_state_resolver,
        "XNYS",
        SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
        &event_inputs,
    )
    .expect("dispatch should resolve market states and apply canonical gating");

    assert_eq!(dispatch_output.records.len(), 2);
    assert_eq!(dispatch_output.records[0].market_state, MarketState::Open);
    assert_eq!(dispatch_output.records[0].m_t_increment, 1.1);
    assert_eq!(dispatch_output.records[0].g_t_increment, 0.0);
    assert_eq!(dispatch_output.records[1].market_state, MarketState::Closed);
    assert_eq!(dispatch_output.records[1].m_t_increment, 0.0);
    assert_eq!(dispatch_output.records[1].g_t_increment, -0.35);
    assert!((dispatch_output.final_state.j_t - 1.5).abs() <= 1e-12);
    assert!((dispatch_output.final_state.m_t - 1.1).abs() <= 1e-12);
    assert!((dispatch_output.final_state.g_t - (-0.35)).abs() <= 1e-12);
    assert_eq!(
        dispatch_output.market_state_dependency,
        SHARED_NEWS_EVENT_MARKET_STATE_DEPENDENCY
    );
    assert_eq!(
        dispatch_output.market_state_boundary_policy,
        SHARED_NEWS_EVENT_BOUNDARY_POLICY
    );
}

fn build_seeded_dispatch_inputs(
    seed: u64,
    event_count: usize,
) -> Vec<SharedNewsEventDispatchInput> {
    let gamma_configuration = GammaSeverityMarkConfiguration::try_new(1.6, 0.9)
        .expect("gamma configuration should be valid");
    let base_shock_configuration = DoubleExponentialBaseShockConfiguration::try_new(0.4, 1.7, 2.5)
        .expect("base-shock configuration should be valid");
    let coupled_gap_configuration = CoupledGapMarkConfiguration::new(0.35);
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(seed);

    let mut event_inputs: Vec<SharedNewsEventDispatchInput> = Vec::with_capacity(event_count);
    for event_position in 0..event_count {
        let timestamp_bucket = (randomness_stream.next_unit_interval_f64() * 4.0).floor() as i64;
        let event_timestamp_utc_nanos = 1_700_000_000_000_000_000_i64 + timestamp_bucket;
        let market_state = if randomness_stream.next_unit_interval_f64() < 0.5 {
            MarketState::Closed
        } else {
            MarketState::Open
        };

        let shared_severity_mark = gamma_configuration
            .sample_severity_mark(&mut randomness_stream)
            .expect("severity mark should sample from valid configuration");
        let shared_base_shock_mark = base_shock_configuration
            .sample_base_shock_mark(&mut randomness_stream)
            .expect("base-shock mark should sample from valid configuration");
        let coupled_gap_jump_mark = coupled_gap_configuration
            .construct_gap_jump_mark(shared_severity_mark, shared_base_shock_mark)
            .expect("coupled gap mark should construct from sampled marks")
            .gap_jump_mark;

        event_inputs.push(SharedNewsEventDispatchInput {
            event_timestamp_utc_nanos,
            event_generation_order: (event_count - event_position - 1) as u64,
            news_arrival_component: if event_position % 2 == 0 {
                NewsArrivalComponent::Known
            } else {
                NewsArrivalComponent::Surprise
            },
            market_state,
            shared_severity_mark,
            coupled_return_jump_mark: coupled_gap_jump_mark,
        });
    }

    event_inputs
}

fn timestamp_utc_nanos(raw_timestamp_utc: &str) -> i64 {
    parse_timestamp(raw_timestamp_utc)
        .timestamp_nanos_opt()
        .expect("timestamp fixture must be in nanosecond range")
}

fn parse_timestamp(raw_timestamp_utc: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw_timestamp_utc)
        .expect("fixture timestamp must be RFC3339")
        .with_timezone(&Utc)
}
