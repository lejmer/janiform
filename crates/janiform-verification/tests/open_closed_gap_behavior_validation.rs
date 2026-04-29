mod support;

use std::collections::BTreeMap;

use chrono::DateTime;
use janiform_market_time::calendar::{ExchangeCalendarRegistry, MarketState};
use janiform_model_kernels::news::{
    NewsArrivalComponent, SharedNewsAccumulatorState, SharedNewsEventTimestampedInput,
    dispatch_shared_news_events_with_resolved_market_states,
};

use support::validation_fixture::{
    OpenClosedGapBehaviorEventExpectationFixture, OpenClosedGapBehaviorScenarioCaseFixture,
    load_calendar_regression_fixture_from_manifest,
    load_open_closed_gap_behavior_scenario_fixture_from_manifest,
};

const FLOAT_TOLERANCE: f64 = 1.0e-12;

#[test]
fn fixture_scenarios_route_activity_open_jump_and_closed_gap_as_expected() {
    let (calendar_registry, fixture_cases) = load_gap_behavior_test_inputs();

    for fixture_case in &fixture_cases {
        let expected_by_event_id: BTreeMap<&str, &OpenClosedGapBehaviorEventExpectationFixture> =
            fixture_case
                .expected_outcome
                .event_outcomes
                .iter()
                .map(|event_expectation| (event_expectation.event_id.as_str(), event_expectation))
                .collect();

        let dispatch_output = dispatch_fixture_case(&calendar_registry, fixture_case);
        assert_eq!(
            dispatch_output.records.len(),
            fixture_case.injected_events.len(),
            "scenario {} should emit one record per injected event",
            fixture_case.fixture_id
        );

        let mut observed_open_event_count = 0usize;
        let mut observed_closed_event_count = 0usize;
        let mut observed_total_activity_coupling_j_t_increment = 0.0;
        let mut observed_total_open_jump_m_t_increment = 0.0;
        let mut observed_total_closed_gap_g_t_increment = 0.0;

        for dispatch_record in &dispatch_output.records {
            let event_index = dispatch_record.event_generation_order as usize;
            let injected_event = &fixture_case.injected_events[event_index];
            let expected_event = expected_by_event_id
                .get(injected_event.event_id.as_str())
                .unwrap_or_else(|| {
                    panic!(
                        "scenario {} expected_outcome missing event {}",
                        fixture_case.fixture_id, injected_event.event_id
                    )
                });

            let expected_market_state =
                parse_market_state(expected_event.expected_market_state.as_str());
            assert_eq!(
                dispatch_record.market_state, expected_market_state,
                "scenario {} event {} resolved to unexpected market state",
                fixture_case.fixture_id, injected_event.event_id
            );

            assert!(
                float_close(
                    dispatch_record.j_t_increment,
                    expected_event.expected_activity_coupling_j_t_increment
                ),
                "scenario {} event {} expected activity coupling {}, observed {}",
                fixture_case.fixture_id,
                injected_event.event_id,
                expected_event.expected_activity_coupling_j_t_increment,
                dispatch_record.j_t_increment
            );
            assert!(
                float_close(
                    dispatch_record.m_t_increment,
                    expected_event.expected_open_jump_m_t_increment
                ),
                "scenario {} event {} expected open jump {}, observed {}",
                fixture_case.fixture_id,
                injected_event.event_id,
                expected_event.expected_open_jump_m_t_increment,
                dispatch_record.m_t_increment
            );
            assert!(
                float_close(
                    dispatch_record.g_t_increment,
                    expected_event.expected_closed_gap_g_t_increment
                ),
                "scenario {} event {} expected closed gap {}, observed {}",
                fixture_case.fixture_id,
                injected_event.event_id,
                expected_event.expected_closed_gap_g_t_increment,
                dispatch_record.g_t_increment
            );
            assert!(
                float_close(expected_event.expected_trading_return_increment, 0.0),
                "scenario {} event {} expected trading return increment must remain zero",
                fixture_case.fixture_id,
                injected_event.event_id
            );

            match dispatch_record.market_state {
                MarketState::Open => {
                    observed_open_event_count = observed_open_event_count.saturating_add(1);
                }
                MarketState::Closed => {
                    observed_closed_event_count = observed_closed_event_count.saturating_add(1);
                }
            }

            observed_total_activity_coupling_j_t_increment += dispatch_record.j_t_increment;
            observed_total_open_jump_m_t_increment += dispatch_record.m_t_increment;
            observed_total_closed_gap_g_t_increment += dispatch_record.g_t_increment;
        }

        let expected_summary = fixture_case.expected_outcome.summary;

        assert_eq!(
            observed_open_event_count, expected_summary.expected_open_event_count,
            "scenario {} expected open event count mismatch",
            fixture_case.fixture_id
        );
        assert_eq!(
            observed_closed_event_count, expected_summary.expected_closed_event_count,
            "scenario {} expected closed event count mismatch",
            fixture_case.fixture_id
        );
        assert!(
            float_close(
                observed_total_activity_coupling_j_t_increment,
                expected_summary.expected_total_activity_coupling_j_t_increment
            ),
            "scenario {} expected total activity coupling mismatch",
            fixture_case.fixture_id
        );
        assert!(
            float_close(
                observed_total_open_jump_m_t_increment,
                expected_summary.expected_total_open_jump_m_t_increment
            ),
            "scenario {} expected total open jump mismatch",
            fixture_case.fixture_id
        );
        assert!(
            float_close(
                observed_total_closed_gap_g_t_increment,
                expected_summary.expected_total_closed_gap_g_t_increment
            ),
            "scenario {} expected total closed gap mismatch",
            fixture_case.fixture_id
        );
        assert!(
            float_close(
                expected_summary.expected_total_trading_return_increment,
                0.0
            ),
            "scenario {} expected total trading return increment must remain zero",
            fixture_case.fixture_id
        );
    }
}

#[test]
fn mixed_news_scenarios_include_known_and_surprise_components() {
    let (_calendar_registry, fixture_cases) = load_gap_behavior_test_inputs();

    let mixed_news_cases: Vec<&OpenClosedGapBehaviorScenarioCaseFixture> = fixture_cases
        .iter()
        .filter(|fixture_case| fixture_case.scenario_class == "mixed_news")
        .collect();
    assert!(
        !mixed_news_cases.is_empty(),
        "fixture suite must include mixed_news scenario coverage"
    );

    for mixed_case in mixed_news_cases {
        let mut known_event_count = 0usize;
        let mut surprise_event_count = 0usize;
        for injected_event in &mixed_case.injected_events {
            match injected_event.news_arrival_component.as_str() {
                "known" => {
                    known_event_count = known_event_count.saturating_add(1);
                }
                "surprise" => {
                    surprise_event_count = surprise_event_count.saturating_add(1);
                }
                unsupported_component => panic!(
                    "scenario {} contains unsupported news_arrival_component {}",
                    mixed_case.fixture_id, unsupported_component
                ),
            }
        }

        assert!(
            known_event_count > 0,
            "scenario {} missing known events",
            mixed_case.fixture_id
        );
        assert!(
            surprise_event_count > 0,
            "scenario {} missing surprise events",
            mixed_case.fixture_id
        );
    }
}

#[test]
fn open_closed_gap_behavior_dispatch_outputs_are_repeatable() {
    let (calendar_registry, fixture_cases) = load_gap_behavior_test_inputs();
    let selected_case = fixture_cases
        .iter()
        .find(|fixture_case| fixture_case.scenario_class == "mixed_news")
        .expect("fixture suite should contain at least one mixed_news scenario");

    let first_output = dispatch_fixture_case(&calendar_registry, selected_case);
    let second_output = dispatch_fixture_case(&calendar_registry, selected_case);

    assert_eq!(first_output, second_output);
}

fn load_gap_behavior_test_inputs() -> (
    ExchangeCalendarRegistry,
    Vec<OpenClosedGapBehaviorScenarioCaseFixture>,
) {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from manifest");
    let calendar_registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture.fixture_document.calendar_registry,
    )
    .expect("calendar registry fixture should be valid");

    let loaded_gap_behavior_fixture =
        load_open_closed_gap_behavior_scenario_fixture_from_manifest()
            .expect("open and closed gap behavior fixture should load from manifest");

    (
        calendar_registry,
        loaded_gap_behavior_fixture.fixture_document.cases,
    )
}

fn dispatch_fixture_case(
    calendar_registry: &ExchangeCalendarRegistry,
    fixture_case: &OpenClosedGapBehaviorScenarioCaseFixture,
) -> janiform_model_kernels::news::SharedNewsEventDispatchOutput {
    let mut event_inputs: Vec<SharedNewsEventTimestampedInput> = Vec::new();
    for (event_position, injected_event) in fixture_case.injected_events.iter().enumerate() {
        let event_timestamp_utc = DateTime::parse_from_rfc3339(&injected_event.timestamp_utc)
            .unwrap_or_else(|source| {
                panic!(
                    "scenario {} event {} timestamp parse failed: {}",
                    fixture_case.fixture_id, injected_event.event_id, source
                )
            })
            .with_timezone(&chrono::Utc);

        let event_timestamp_utc_nanos =
            event_timestamp_utc
                .timestamp_nanos_opt()
                .unwrap_or_else(|| {
                    panic!(
                        "scenario {} event {} timestamp is outside nanosecond range",
                        fixture_case.fixture_id, injected_event.event_id
                    )
                });

        event_inputs.push(SharedNewsEventTimestampedInput {
            event_timestamp_utc_nanos,
            event_generation_order: event_position as u64,
            news_arrival_component: parse_news_arrival_component(
                injected_event.news_arrival_component.as_str(),
            ),
            shared_severity_mark: injected_event.shared_severity_mark,
            coupled_return_jump_mark: injected_event.coupled_return_jump_mark,
        });
    }

    dispatch_shared_news_events_with_resolved_market_states(
        calendar_registry,
        fixture_case.exchange_code.as_str(),
        SharedNewsAccumulatorState::new(
            fixture_case.initial_accumulator_state.j_t,
            fixture_case.initial_accumulator_state.m_t,
            fixture_case.initial_accumulator_state.g_t,
        ),
        &event_inputs,
    )
    .unwrap_or_else(|source| {
        panic!(
            "scenario {} failed to dispatch events: {}",
            fixture_case.fixture_id, source
        )
    })
}

fn parse_market_state(raw_market_state: &str) -> MarketState {
    match raw_market_state {
        "open" => MarketState::Open,
        "closed" => MarketState::Closed,
        _ => panic!("unsupported expected_market_state {}", raw_market_state),
    }
}

fn parse_news_arrival_component(raw_component: &str) -> NewsArrivalComponent {
    match raw_component {
        "known" => NewsArrivalComponent::Known,
        "surprise" => NewsArrivalComponent::Surprise,
        _ => panic!("unsupported news_arrival_component {}", raw_component),
    }
}

fn float_close(left: f64, right: f64) -> bool {
    (left - right).abs() <= FLOAT_TOLERANCE
}
