use std::collections::BTreeMap;

use janiform_core::calendar::MarketState;
use janiform_core::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, NewsArrivalComponent, SharedNewsAccumulatorState,
    SharedNewsEventDispatchInput, SharedNewsEventDispatchOutput, dispatch_shared_news_events,
};
use janiform_core::randomness::{SimulationRandomnessStream, SplitMix64SimulationRandomnessStream};

const DISTRIBUTION_SESSION_STATE_CONTEXT: &str = "not_applicable_distribution_regression";
const COUPLING_SESSION_STATE_CONTEXT: &str = "mixed_open_closed_dispatch_sequence";
const MARK_REGRESSION_ABSOLUTE_TOLERANCE: f64 = 1.0e-12;

#[derive(Debug, Clone, PartialEq, Eq)]
struct NewsRegressionContext {
    seed: u64,
    parameter_set: String,
    session_state_context: String,
}

#[derive(Debug, Clone, PartialEq)]
struct NewsRegressionFailureDiagnostic {
    check_id: String,
    observed: f64,
    expected: f64,
    tolerance: f64,
    context: NewsRegressionContext,
}

impl NewsRegressionFailureDiagnostic {
    fn render(&self) -> String {
        format!(
            "news regression check failed: check_id={} observed={} expected={} tolerance={} seed={} parameter_set={} session_state_context={}",
            self.check_id,
            self.observed,
            self.expected,
            self.tolerance,
            self.context.seed,
            self.context.parameter_set,
            self.context.session_state_context
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SeededSharedMarkSample {
    shared_severity_mark: f64,
    shared_base_shock_mark: f64,
    gap_jump_mark: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct SeededSharedMarkDispatchCase {
    samples_by_generation_order: BTreeMap<u64, SeededSharedMarkSample>,
    dispatch_output: SharedNewsEventDispatchOutput,
}

#[test]
fn gamma_severity_distribution_regression_matches_shape_scale_moments_with_tolerance() {
    let shape_k: f64 = 2.8;
    let scale_theta: f64 = 0.55;
    let sample_count = 40_000_u64;
    let seed = 520_031_u64;

    let expected_mean = shape_k * scale_theta;
    let expected_variance = shape_k * scale_theta.powi(2);
    let expected_second_moment = expected_variance + expected_mean.powi(2);

    let configuration = GammaSeverityMarkConfiguration::try_new(shape_k, scale_theta)
        .expect("valid gamma severity configuration should construct");
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(seed);

    let mut sample_sum = 0.0;
    let mut squared_sample_sum = 0.0;
    for _sample_index in 0..sample_count {
        let sampled_mark = configuration
            .sample_severity_mark(&mut randomness_stream)
            .expect("severity sampling should succeed for valid parameters");

        sample_sum += sampled_mark;
        squared_sample_sum += sampled_mark * sampled_mark;
    }

    let sample_mean = sample_sum / sample_count as f64;
    let sample_second_moment = squared_sample_sum / sample_count as f64;

    let context = NewsRegressionContext {
        seed,
        parameter_set: format!(
            "distribution=gamma shape_k={} scale_theta={} sample_count={}",
            shape_k, scale_theta, sample_count
        ),
        session_state_context: DISTRIBUTION_SESSION_STATE_CONTEXT.to_string(),
    };

    assert_metric_within_tolerance(
        "gamma.sample_mean",
        sample_mean,
        expected_mean,
        0.03,
        &context,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
    assert_metric_within_tolerance(
        "gamma.sample_second_moment",
        sample_second_moment,
        expected_second_moment,
        0.08,
        &context,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
}

#[test]
fn double_exponential_base_shock_distribution_regression_matches_mixture_and_tail_moments() {
    let probability_positive: f64 = 0.40;
    let eta_plus: f64 = 1.35;
    let eta_minus: f64 = 2.10;
    let sample_count = 80_000_u64;
    let seed = 520_032_u64;

    let expected_positive_frequency = probability_positive;
    let expected_positive_mean = 1.0 / eta_plus;
    let expected_negative_absolute_mean = 1.0 / eta_minus;

    let configuration =
        DoubleExponentialBaseShockConfiguration::try_new(probability_positive, eta_plus, eta_minus)
            .expect("valid base-shock configuration should construct");
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(seed);

    let mut positive_count = 0_u64;
    let mut negative_count = 0_u64;
    let mut positive_sum = 0.0;
    let mut negative_absolute_sum = 0.0;

    for _sample_index in 0..sample_count {
        let sampled_mark = configuration
            .sample_base_shock_mark(&mut randomness_stream)
            .expect("base-shock sampling should succeed for valid parameters");

        if sampled_mark >= 0.0 {
            positive_count += 1;
            positive_sum += sampled_mark;
        } else {
            negative_count += 1;
            negative_absolute_sum += -sampled_mark;
        }
    }

    let observed_positive_frequency = positive_count as f64 / sample_count as f64;
    let observed_positive_mean = positive_sum / positive_count as f64;
    let observed_negative_absolute_mean = negative_absolute_sum / negative_count as f64;

    let context = NewsRegressionContext {
        seed,
        parameter_set: format!(
            "distribution=double_exponential probability_positive={} eta_plus={} eta_minus={} sample_count={}",
            probability_positive, eta_plus, eta_minus, sample_count
        ),
        session_state_context: DISTRIBUTION_SESSION_STATE_CONTEXT.to_string(),
    };

    assert_metric_within_tolerance(
        "double_exponential.positive_frequency",
        observed_positive_frequency,
        expected_positive_frequency,
        0.015,
        &context,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
    assert_metric_within_tolerance(
        "double_exponential.positive_tail_mean",
        observed_positive_mean,
        expected_positive_mean,
        0.06,
        &context,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
    assert_metric_within_tolerance(
        "double_exponential.negative_tail_absolute_mean",
        observed_negative_absolute_mean,
        expected_negative_absolute_mean,
        0.06,
        &context,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
}

#[test]
fn shared_mark_coupling_regression_is_consistent_across_jt_and_gt_for_fixed_seed() {
    let seed = 520_033_u64;
    let shape_k: f64 = 1.7;
    let scale_theta: f64 = 0.9;
    let probability_positive: f64 = 0.45;
    let eta_plus: f64 = 1.9;
    let eta_minus: f64 = 2.7;
    let sigma_g: f64 = 0.35;
    let event_count = 64;

    let first_case = run_seeded_shared_mark_dispatch_case(
        seed,
        event_count,
        shape_k,
        scale_theta,
        probability_positive,
        eta_plus,
        eta_minus,
        sigma_g,
    );
    let second_case = run_seeded_shared_mark_dispatch_case(
        seed,
        event_count,
        shape_k,
        scale_theta,
        probability_positive,
        eta_plus,
        eta_minus,
        sigma_g,
    );

    assert_eq!(first_case, second_case);

    let context = NewsRegressionContext {
        seed,
        parameter_set: format!(
            "distribution=gamma_plus_double_exponential shape_k={} scale_theta={} probability_positive={} eta_plus={} eta_minus={} sigma_g={} event_count={}",
            shape_k, scale_theta, probability_positive, eta_plus, eta_minus, sigma_g, event_count
        ),
        session_state_context: COUPLING_SESSION_STATE_CONTEXT.to_string(),
    };

    let mut observed_open_events = 0_u64;
    let mut observed_closed_events = 0_u64;

    for dispatch_record in &first_case.dispatch_output.records {
        let sampled_marks = first_case
            .samples_by_generation_order
            .get(&dispatch_record.event_generation_order)
            .expect("sample lookup by generation order should exist");

        assert_metric_within_tolerance(
            "coupling.shared_severity_mark_passthrough_to_jt",
            dispatch_record.shared_severity_mark,
            sampled_marks.shared_severity_mark,
            MARK_REGRESSION_ABSOLUTE_TOLERANCE,
            &context,
        )
        .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));

        let expected_gap_increment = match dispatch_record.market_state {
            MarketState::Open => {
                observed_open_events += 1;
                0.0
            }
            MarketState::Closed => {
                observed_closed_events += 1;
                sampled_marks.gap_jump_mark
            }
        };

        assert_metric_within_tolerance(
            "coupling.gt_gating_by_market_state",
            dispatch_record.g_t_increment,
            expected_gap_increment,
            MARK_REGRESSION_ABSOLUTE_TOLERANCE,
            &context,
        )
        .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));

        let reconstructed_gap_from_shared_marks = sigma_g
            * dispatch_record.shared_severity_mark.sqrt()
            * sampled_marks.shared_base_shock_mark;

        assert_metric_within_tolerance(
            "coupling.gap_reconstruction_uses_same_shared_mark_pair",
            dispatch_record.coupled_return_jump_mark,
            reconstructed_gap_from_shared_marks,
            MARK_REGRESSION_ABSOLUTE_TOLERANCE,
            &context,
        )
        .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
    }

    assert!(
        observed_open_events > 0,
        "coupling regression requires at least one open event for gating validation"
    );
    assert!(
        observed_closed_events > 0,
        "coupling regression requires at least one closed event for gating validation"
    );
}

#[test]
fn failure_diagnostics_emit_seed_parameter_set_and_session_state_metadata() {
    let context = NewsRegressionContext {
        seed: 11_337,
        parameter_set: "distribution=gamma shape_k=2.0 scale_theta=0.5".to_string(),
        session_state_context: "closed_only_session".to_string(),
    };

    let diagnostic =
        assert_metric_within_tolerance("diagnostics.metadata_presence", 1.0, 1.3, 0.05, &context)
            .expect_err("check must fail to emit a failure diagnostic");
    let rendered = diagnostic.render();

    assert!(rendered.contains("seed=11337"));
    assert!(rendered.contains("parameter_set=distribution=gamma shape_k=2.0 scale_theta=0.5"));
    assert!(rendered.contains("session_state_context=closed_only_session"));
    assert!(rendered.contains("check_id=diagnostics.metadata_presence"));
}

fn assert_metric_within_tolerance(
    check_id: &str,
    observed: f64,
    expected: f64,
    tolerance: f64,
    context: &NewsRegressionContext,
) -> Result<(), NewsRegressionFailureDiagnostic> {
    let baseline_magnitude = expected.abs().max(1.0);
    let normalized_delta = (observed - expected).abs() / baseline_magnitude;
    if normalized_delta <= tolerance {
        return Ok(());
    }

    Err(NewsRegressionFailureDiagnostic {
        check_id: check_id.to_string(),
        observed,
        expected,
        tolerance,
        context: context.clone(),
    })
}

#[allow(clippy::too_many_arguments)]
fn run_seeded_shared_mark_dispatch_case(
    seed: u64,
    event_count: usize,
    shape_k: f64,
    scale_theta: f64,
    probability_positive: f64,
    eta_plus: f64,
    eta_minus: f64,
    sigma_g: f64,
) -> SeededSharedMarkDispatchCase {
    let severity_configuration = GammaSeverityMarkConfiguration::try_new(shape_k, scale_theta)
        .expect("gamma configuration should be valid");
    let base_shock_configuration =
        DoubleExponentialBaseShockConfiguration::try_new(probability_positive, eta_plus, eta_minus)
            .expect("base-shock configuration should be valid");
    let coupled_configuration = CoupledGapMarkConfiguration::new(sigma_g);
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(seed);

    let mut event_inputs: Vec<SharedNewsEventDispatchInput> = Vec::with_capacity(event_count);
    let mut samples_by_generation_order: BTreeMap<u64, SeededSharedMarkSample> = BTreeMap::new();

    for event_position in 0..event_count {
        let event_generation_order = (event_count - event_position - 1) as u64;
        let timestamp_bucket = (randomness_stream.next_unit_interval_f64() * 5.0).floor() as i64;
        let event_timestamp_utc_nanos = 1_710_000_000_000_000_000_i64 + timestamp_bucket;

        let market_state = match event_position {
            0 => MarketState::Open,
            1 => MarketState::Closed,
            _ => {
                if randomness_stream.next_unit_interval_f64() < 0.5 {
                    MarketState::Closed
                } else {
                    MarketState::Open
                }
            }
        };

        let shared_severity_mark = severity_configuration
            .sample_severity_mark(&mut randomness_stream)
            .expect("severity mark should sample from valid configuration");
        let shared_base_shock_mark = base_shock_configuration
            .sample_base_shock_mark(&mut randomness_stream)
            .expect("base-shock mark should sample from valid configuration");
        let gap_jump_mark = coupled_configuration
            .construct_gap_jump_mark(shared_severity_mark, shared_base_shock_mark)
            .expect("coupled mark construction should succeed")
            .gap_jump_mark;

        event_inputs.push(SharedNewsEventDispatchInput {
            event_timestamp_utc_nanos,
            event_generation_order,
            news_arrival_component: if event_position % 2 == 0 {
                NewsArrivalComponent::Known
            } else {
                NewsArrivalComponent::Surprise
            },
            market_state,
            shared_severity_mark,
            coupled_return_jump_mark: gap_jump_mark,
        });
        samples_by_generation_order.insert(
            event_generation_order,
            SeededSharedMarkSample {
                shared_severity_mark,
                shared_base_shock_mark,
                gap_jump_mark,
            },
        );
    }

    let dispatch_output = dispatch_shared_news_events(
        SharedNewsAccumulatorState::new(0.6, 0.0, -0.25),
        &event_inputs,
    )
    .expect("seeded shared dispatch should succeed");

    SeededSharedMarkDispatchCase {
        samples_by_generation_order,
        dispatch_output,
    }
}
