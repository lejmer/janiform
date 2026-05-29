use janiform_foundations::market_state::MarketState;
use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
use janiform_model_kernels::news::{
    GammaSeverityMarkConfiguration, NewsArrivalComponent, NewsComponentMarkScaleConfiguration,
    NewsReturnShockMarkSampler, SharedNewsAccumulatorState, SharedNewsEventDispatchInput,
    StandardNormalNewsReturnShockSampler, dispatch_shared_news_events,
};

const MARK_REGRESSION_ABSOLUTE_TOLERANCE: f64 = 1.0e-12;

#[derive(Debug, Clone, PartialEq)]
struct NewsRegressionFailureDiagnostic {
    check_id: &'static str,
    observed: f64,
    expected: f64,
    tolerance: f64,
}

impl NewsRegressionFailureDiagnostic {
    fn render(&self) -> String {
        format!(
            "news regression check failed: check_id={} observed={} expected={} tolerance={}",
            self.check_id, self.observed, self.expected, self.tolerance
        )
    }
}

#[test]
fn gamma_severity_distribution_regression_matches_unit_mean_moments() {
    let shape_k: f64 = 2.8;
    let sample_count = 40_000_u64;
    let seed = 520_031_u64;

    let configuration = GammaSeverityMarkConfiguration::try_new(shape_k)
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
    let expected_second_moment = 1.0 + 1.0 / shape_k;

    assert_metric_within_tolerance("gamma.sample_mean", sample_mean, 1.0, 0.03)
        .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
    assert_metric_within_tolerance(
        "gamma.sample_second_moment",
        sample_second_moment,
        expected_second_moment,
        0.08,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
}

#[test]
fn standard_normal_return_shock_regression_matches_first_two_moments() {
    let sample_count = 40_000_u64;
    let seed = 208_771_u64;
    let sampler = StandardNormalNewsReturnShockSampler;
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(seed);

    let mut sample_sum = 0.0;
    let mut squared_sample_sum = 0.0;
    for _sample_index in 0..sample_count {
        let sampled_mark = sampler
            .sample_shared_return_shock_mark(&mut randomness_stream)
            .expect("standard normal return-shock sampling should succeed");

        sample_sum += sampled_mark;
        squared_sample_sum += sampled_mark * sampled_mark;
    }

    let sample_mean = sample_sum / sample_count as f64;
    let sample_second_moment = squared_sample_sum / sample_count as f64;

    assert_metric_within_tolerance("standard_normal.sample_mean", sample_mean, 0.0, 0.03)
        .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
    assert_metric_within_tolerance(
        "standard_normal.sample_second_moment",
        sample_second_moment,
        1.0,
        0.08,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
}

#[test]
fn gamma_normal_news_mark_couples_activity_and_return_to_shared_severity() {
    let component_scales = NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 0.7,
        return_jump_drift: -0.2,
        return_jump_volatility: 0.4,
    };
    let shared_severity_mark: f64 = 1.6;
    let shared_return_shock_mark: f64 = -0.5;

    let construction = component_scales
        .construct_gamma_normal_mark(shared_severity_mark, shared_return_shock_mark)
        .expect("valid Gamma-normal mark should construct");

    let expected_activity_shock = component_scales.activity_coupling_scale * shared_severity_mark;
    let expected_return_jump = component_scales.return_jump_drift * shared_severity_mark
        + component_scales.return_jump_volatility
            * shared_severity_mark.sqrt()
            * shared_return_shock_mark;

    assert_metric_within_tolerance(
        "gamma_normal.activity_shock",
        construction.activity_shock_mark,
        expected_activity_shock,
        MARK_REGRESSION_ABSOLUTE_TOLERANCE,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
    assert_metric_within_tolerance(
        "gamma_normal.return_jump",
        construction.return_jump_mark,
        expected_return_jump,
        MARK_REGRESSION_ABSOLUTE_TOLERANCE,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
}

#[test]
fn news_dispatch_routes_same_coupled_return_jump_by_market_state() {
    let initial_state = SharedNewsAccumulatorState::new(0.1, -0.2, 0.3);
    let event_inputs = vec![
        SharedNewsEventDispatchInput {
            event_timestamp_utc_nanos: 2_000,
            event_generation_order: 2,
            news_arrival_component: NewsArrivalComponent::Surprise,
            market_state: MarketState::Closed,
            shared_severity_mark: 1.2,
            coupled_return_jump_mark: -0.8,
        },
        SharedNewsEventDispatchInput {
            event_timestamp_utc_nanos: 1_000,
            event_generation_order: 1,
            news_arrival_component: NewsArrivalComponent::Known,
            market_state: MarketState::Open,
            shared_severity_mark: 0.6,
            coupled_return_jump_mark: 0.5,
        },
    ];

    let dispatch_output = dispatch_shared_news_events(initial_state, &event_inputs)
        .expect("valid shared news events should dispatch");

    assert_metric_within_tolerance(
        "dispatch.final_j_t",
        dispatch_output.final_state.j_t,
        initial_state.j_t + 0.6 + 1.2,
        MARK_REGRESSION_ABSOLUTE_TOLERANCE,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
    assert_metric_within_tolerance(
        "dispatch.final_m_t",
        dispatch_output.final_state.m_t,
        initial_state.m_t + 0.5,
        MARK_REGRESSION_ABSOLUTE_TOLERANCE,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
    assert_metric_within_tolerance(
        "dispatch.final_g_t",
        dispatch_output.final_state.g_t,
        initial_state.g_t - 0.8,
        MARK_REGRESSION_ABSOLUTE_TOLERANCE,
    )
    .unwrap_or_else(|diagnostic| panic!("{}", diagnostic.render()));
}

fn assert_metric_within_tolerance(
    check_id: &'static str,
    observed: f64,
    expected: f64,
    tolerance: f64,
) -> Result<(), NewsRegressionFailureDiagnostic> {
    if (observed - expected).abs() <= tolerance {
        return Ok(());
    }

    Err(NewsRegressionFailureDiagnostic {
        check_id,
        observed,
        expected,
        tolerance,
    })
}
