mod support;

use std::collections::BTreeMap;

use chrono::DateTime;
use janiform_core::calendar::{ExchangeCalendarRegistry, MarketState};
use janiform_core::news::{
    dispatch_shared_news_events_with_resolved_market_states, SharedNewsAccumulatorState,
    SharedNewsEventTimestampedInput,
};

use support::validation_fixture::{
    load_calendar_regression_fixture_from_manifest,
    load_open_closed_gap_behavior_scenario_fixture_from_manifest,
    OpenClosedGapBehaviorEventExpectationFixture, OpenClosedGapBehaviorScenarioCaseFixture,
};

const OPEN_WINDOW_SCENARIO_ID: &str = "xnys_intraday_open_window_gap_suppression";
const FLOAT_TOLERANCE: f64 = 1.0e-12;

#[derive(Debug, Clone, PartialEq)]
struct OpenWindowEventValidationOutput {
    scenario_id: String,
    event_id: String,
    event_timestamp_utc: String,
    observed_market_state: MarketState,
    activity_coupling_j_t_increment: f64,
    direct_gap_g_t_increment: f64,
    expected_activity_coupling_j_t_increment: f64,
    expected_direct_gap_g_t_increment: f64,
    expected_direct_gap_applied: bool,
    observed_direct_gap_applied: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct OpenWindowScenarioSummaryOutput {
    scenario_id: String,
    initial_j_t: f64,
    final_j_t: f64,
    expected_total_activity_coupling_j_t_increment: f64,
    observed_total_activity_coupling_j_t_increment: f64,
    expected_total_direct_gap_g_t_increment: f64,
    observed_total_direct_gap_g_t_increment: f64,
    expected_open_event_count: usize,
    observed_open_event_count: usize,
    expected_open_window_direct_gap_increment_count: usize,
    observed_open_window_direct_gap_increment_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct OpenWindowScenarioValidationOutput {
    scenario_id: String,
    event_outputs: Vec<OpenWindowEventValidationOutput>,
    summary: OpenWindowScenarioSummaryOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenWindowGapValidationFailure {
    check_id: &'static str,
    scenario_id: String,
    event_id: String,
    event_timestamp_utc: String,
    detail: String,
}

#[derive(Debug, Clone, PartialEq)]
struct ExpectedEventMetadata {
    event_id: String,
    event_timestamp_utc: String,
    expected_market_state: MarketState,
    expected_activity_coupling_j_t_increment: f64,
    expected_direct_gap_g_t_increment: f64,
    expected_direct_gap_applied: bool,
}

#[test]
fn open_window_mixed_mark_events_update_activity_without_direct_gap_increments() {
    let (calendar_registry, fixture_cases) = load_gap_behavior_test_inputs();

    let open_window_cases = open_window_only_cases(&fixture_cases);
    assert!(
        !open_window_cases.is_empty(),
        "fixture suite must include at least one open-window-only scenario"
    );

    for fixture_case in open_window_cases {
        let has_positive_gap_mark = fixture_case
            .injected_events
            .iter()
            .any(|event| event.coupled_gap_jump_mark > 0.0);
        let has_negative_gap_mark = fixture_case
            .injected_events
            .iter()
            .any(|event| event.coupled_gap_jump_mark < 0.0);
        assert!(
            has_positive_gap_mark && has_negative_gap_mark,
            "scenario {} must include mixed-sign coupled_gap_jump_mark inputs",
            fixture_case.fixture_id
        );

        let output = evaluate_open_window_scenario(&calendar_registry, fixture_case);
        let failures = collect_open_window_validation_failures(&output);
        assert!(
            failures.is_empty(),
            "open-window scenario validation failed:\n{}",
            render_failures(&failures)
        );

        assert!(
            output
                .summary
                .observed_total_activity_coupling_j_t_increment
                > 0.0,
            "scenario {} must produce a positive activity-coupling update path",
            fixture_case.fixture_id
        );
        assert!(
            output.summary.observed_total_direct_gap_g_t_increment.abs() <= FLOAT_TOLERANCE,
            "scenario {} must keep total direct gap contribution at zero for open-window events",
            fixture_case.fixture_id
        );
    }
}

#[test]
fn open_window_scenario_outputs_are_deterministic_and_keep_direct_gap_effect_zero() {
    let (calendar_registry, fixture_cases) = load_gap_behavior_test_inputs();

    let open_window_case = fixture_case_by_id(&fixture_cases, OPEN_WINDOW_SCENARIO_ID);

    let first_output = evaluate_open_window_scenario(&calendar_registry, open_window_case);
    let second_output = evaluate_open_window_scenario(&calendar_registry, open_window_case);

    assert_eq!(first_output, second_output);
    assert_eq!(
        first_output.summary.expected_total_direct_gap_g_t_increment,
        0.0
    );
    assert!(
        first_output
            .summary
            .observed_total_direct_gap_g_t_increment
            .abs()
            <= FLOAT_TOLERANCE
    );
    assert_eq!(
        first_output
            .summary
            .observed_open_window_direct_gap_increment_count,
        0
    );
    assert!(
        float_close(
            first_output.summary.final_j_t - first_output.summary.initial_j_t,
            first_output
                .summary
                .expected_total_activity_coupling_j_t_increment,
        ),
        "scenario {} must update only the activity path by expected J_t total",
        first_output.scenario_id
    );
}

#[test]
fn open_window_gap_leakage_regression_reports_scenario_id_and_event_timestamp() {
    let (calendar_registry, fixture_cases) = load_gap_behavior_test_inputs();

    let open_window_case = fixture_case_by_id(&fixture_cases, OPEN_WINDOW_SCENARIO_ID);
    let mut output = evaluate_open_window_scenario(&calendar_registry, open_window_case);

    let injected_leakage = 0.19;
    output.event_outputs[0].direct_gap_g_t_increment = injected_leakage;
    output.event_outputs[0].observed_direct_gap_applied = true;

    let failures = collect_open_window_validation_failures(&output);
    assert!(
        !failures.is_empty(),
        "injected leakage must trigger at least one validation failure"
    );

    let leakage_failure = failures
        .iter()
        .find(|failure| failure.check_id == "open_window_direct_gap_leakage")
        .expect("injected leakage must emit open-window direct-gap leakage failure");

    assert_eq!(leakage_failure.scenario_id, output.scenario_id);
    assert_eq!(
        leakage_failure.event_timestamp_utc,
        output.event_outputs[0].event_timestamp_utc
    );
}

fn load_gap_behavior_test_inputs() -> (
    ExchangeCalendarRegistry,
    Vec<OpenClosedGapBehaviorScenarioCaseFixture>,
) {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load");

    let calendar_registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture.fixture_document.calendar_registry,
    )
    .expect("calendar registry fixture artifact should construct");

    let loaded_gap_behavior_fixture =
        load_open_closed_gap_behavior_scenario_fixture_from_manifest()
            .expect("open and closed gap behavior fixture should load");

    (
        calendar_registry,
        loaded_gap_behavior_fixture.fixture_document.cases,
    )
}

fn open_window_only_cases<'a>(
    fixture_cases: &'a [OpenClosedGapBehaviorScenarioCaseFixture],
) -> Vec<&'a OpenClosedGapBehaviorScenarioCaseFixture> {
    fixture_cases
        .iter()
        .filter(|fixture_case| {
            fixture_case
                .expected_outcome
                .summary
                .expected_closed_event_count
                == 0
        })
        .collect()
}

fn fixture_case_by_id<'a>(
    fixture_cases: &'a [OpenClosedGapBehaviorScenarioCaseFixture],
    scenario_id: &str,
) -> &'a OpenClosedGapBehaviorScenarioCaseFixture {
    fixture_cases
        .iter()
        .find(|fixture_case| fixture_case.fixture_id == scenario_id)
        .expect("requested fixture_id must exist")
}

fn evaluate_open_window_scenario(
    calendar_registry: &ExchangeCalendarRegistry,
    fixture_case: &OpenClosedGapBehaviorScenarioCaseFixture,
) -> OpenWindowScenarioValidationOutput {
    let expected_outcome_by_event_id: BTreeMap<
        &str,
        &OpenClosedGapBehaviorEventExpectationFixture,
    > = fixture_case
        .expected_outcome
        .event_outcomes
        .iter()
        .map(|event_expectation| (event_expectation.event_id.as_str(), event_expectation))
        .collect();

    let mut expected_metadata_by_generation_order: BTreeMap<u64, ExpectedEventMetadata> =
        BTreeMap::new();
    let event_inputs: Vec<SharedNewsEventTimestampedInput> = fixture_case
        .injected_events
        .iter()
        .enumerate()
        .map(|(event_index, injected_event)| {
            let event_generation_order =
                u64::try_from(event_index).expect("event index should fit in u64");
            let expected_outcome = expected_outcome_by_event_id
                .get(injected_event.event_id.as_str())
                .expect("expected outcome must exist for injected event");

            expected_metadata_by_generation_order.insert(
                event_generation_order,
                ExpectedEventMetadata {
                    event_id: injected_event.event_id.clone(),
                    event_timestamp_utc: injected_event.timestamp_utc.clone(),
                    expected_market_state: parse_market_state(
                        &expected_outcome.expected_market_state,
                    ),
                    expected_activity_coupling_j_t_increment: expected_outcome
                        .expected_j_t_increment,
                    expected_direct_gap_g_t_increment: expected_outcome.expected_g_t_increment,
                    expected_direct_gap_applied: expected_outcome.expected_direct_gap_applied,
                },
            );

            SharedNewsEventTimestampedInput {
                event_timestamp_utc_nanos: timestamp_utc_to_nanos(&injected_event.timestamp_utc),
                event_generation_order,
                shared_severity_mark: injected_event.shared_severity_mark,
                coupled_gap_jump_mark: injected_event.coupled_gap_jump_mark,
            }
        })
        .collect();

    let dispatch_output = dispatch_shared_news_events_with_resolved_market_states(
        calendar_registry,
        &fixture_case.exchange_code,
        SharedNewsAccumulatorState::new(
            fixture_case.initial_accumulator_state.j_t,
            fixture_case.initial_accumulator_state.g_t,
        ),
        &event_inputs,
    )
    .expect("open-window scenario dispatch should succeed");

    let event_outputs: Vec<OpenWindowEventValidationOutput> = dispatch_output
        .records
        .iter()
        .map(|dispatch_record| {
            let expected_metadata = expected_metadata_by_generation_order
                .get(&dispatch_record.event_generation_order)
                .expect("every dispatch record should map back to one expected event");
            assert_eq!(
                expected_metadata.expected_market_state,
                dispatch_record.market_state,
                "fixture expected market state must match resolved market state for scenario {} event_id {}",
                fixture_case.fixture_id,
                expected_metadata.event_id
            );

            OpenWindowEventValidationOutput {
                scenario_id: fixture_case.fixture_id.clone(),
                event_id: expected_metadata.event_id.clone(),
                event_timestamp_utc: expected_metadata.event_timestamp_utc.clone(),
                observed_market_state: dispatch_record.market_state,
                activity_coupling_j_t_increment: dispatch_record.j_t_increment,
                direct_gap_g_t_increment: dispatch_record.g_t_increment,
                expected_activity_coupling_j_t_increment: expected_metadata
                    .expected_activity_coupling_j_t_increment,
                expected_direct_gap_g_t_increment: expected_metadata
                    .expected_direct_gap_g_t_increment,
                expected_direct_gap_applied: expected_metadata.expected_direct_gap_applied,
                observed_direct_gap_applied: dispatch_record.g_t_increment.abs() > FLOAT_TOLERANCE,
            }
        })
        .collect();

    let observed_open_event_count = event_outputs
        .iter()
        .filter(|event_output| event_output.observed_market_state == MarketState::Open)
        .count();
    let observed_open_window_direct_gap_increment_count = event_outputs
        .iter()
        .filter(|event_output| {
            event_output.observed_market_state == MarketState::Open
                && event_output.observed_direct_gap_applied
        })
        .count();
    let observed_total_activity_coupling_j_t_increment = event_outputs
        .iter()
        .map(|event_output| event_output.activity_coupling_j_t_increment)
        .sum();
    let observed_total_direct_gap_g_t_increment = event_outputs
        .iter()
        .map(|event_output| event_output.direct_gap_g_t_increment)
        .sum();

    OpenWindowScenarioValidationOutput {
        scenario_id: fixture_case.fixture_id.clone(),
        event_outputs,
        summary: OpenWindowScenarioSummaryOutput {
            scenario_id: fixture_case.fixture_id.clone(),
            initial_j_t: fixture_case.initial_accumulator_state.j_t,
            final_j_t: dispatch_output.final_state.j_t,
            expected_total_activity_coupling_j_t_increment: fixture_case
                .expected_outcome
                .summary
                .expected_total_j_t_increment,
            observed_total_activity_coupling_j_t_increment,
            expected_total_direct_gap_g_t_increment: fixture_case
                .expected_outcome
                .summary
                .expected_total_g_t_increment,
            observed_total_direct_gap_g_t_increment,
            expected_open_event_count: fixture_case
                .expected_outcome
                .summary
                .expected_open_event_count,
            observed_open_event_count,
            expected_open_window_direct_gap_increment_count: fixture_case
                .expected_outcome
                .summary
                .expected_open_window_direct_gap_increment_count,
            observed_open_window_direct_gap_increment_count,
        },
    }
}

fn collect_open_window_validation_failures(
    output: &OpenWindowScenarioValidationOutput,
) -> Vec<OpenWindowGapValidationFailure> {
    let mut failures: Vec<OpenWindowGapValidationFailure> = Vec::new();
    for event_output in &output.event_outputs {
        if event_output.observed_market_state != MarketState::Open {
            failures.push(OpenWindowGapValidationFailure {
                check_id: "open_window_market_state_mismatch",
                scenario_id: event_output.scenario_id.clone(),
                event_id: event_output.event_id.clone(),
                event_timestamp_utc: event_output.event_timestamp_utc.clone(),
                detail: format!(
                    "expected open market state but observed {}",
                    event_output.observed_market_state
                ),
            });
        }

        if !float_close(
            event_output.activity_coupling_j_t_increment,
            event_output.expected_activity_coupling_j_t_increment,
        ) {
            failures.push(OpenWindowGapValidationFailure {
                check_id: "open_window_activity_path_mismatch",
                scenario_id: event_output.scenario_id.clone(),
                event_id: event_output.event_id.clone(),
                event_timestamp_utc: event_output.event_timestamp_utc.clone(),
                detail: format!(
                    "activity coupling increment mismatch expected={} observed={}",
                    event_output.expected_activity_coupling_j_t_increment,
                    event_output.activity_coupling_j_t_increment
                ),
            });
        }

        if !float_close(
            event_output.direct_gap_g_t_increment,
            event_output.expected_direct_gap_g_t_increment,
        ) {
            failures.push(OpenWindowGapValidationFailure {
                check_id: "open_window_direct_gap_increment_mismatch",
                scenario_id: event_output.scenario_id.clone(),
                event_id: event_output.event_id.clone(),
                event_timestamp_utc: event_output.event_timestamp_utc.clone(),
                detail: format!(
                    "direct gap increment mismatch expected={} observed={}",
                    event_output.expected_direct_gap_g_t_increment,
                    event_output.direct_gap_g_t_increment
                ),
            });
        }

        if event_output.observed_direct_gap_applied {
            failures.push(OpenWindowGapValidationFailure {
                check_id: "open_window_direct_gap_leakage",
                scenario_id: event_output.scenario_id.clone(),
                event_id: event_output.event_id.clone(),
                event_timestamp_utc: event_output.event_timestamp_utc.clone(),
                detail: format!(
                    "open-window event leaked direct gap increment with observed g_t_increment={}",
                    event_output.direct_gap_g_t_increment
                ),
            });
        }

        if event_output.expected_direct_gap_applied {
            failures.push(OpenWindowGapValidationFailure {
                check_id: "open_window_expected_direct_gap_applied_invalid",
                scenario_id: event_output.scenario_id.clone(),
                event_id: event_output.event_id.clone(),
                event_timestamp_utc: event_output.event_timestamp_utc.clone(),
                detail: "open-window expected outcome must keep expected_direct_gap_applied=false"
                    .to_string(),
            });
        }
    }

    failures
}

fn render_failures(failures: &[OpenWindowGapValidationFailure]) -> String {
    failures
        .iter()
        .map(|failure| {
            format!(
                "check_id={} scenario_id={} event_id={} event_timestamp_utc={} detail={}",
                failure.check_id,
                failure.scenario_id,
                failure.event_id,
                failure.event_timestamp_utc,
                failure.detail
            )
        })
        .collect::<Vec<String>>()
        .join("\n")
}

fn parse_market_state(value: &str) -> MarketState {
    match value {
        "open" => MarketState::Open,
        "closed" => MarketState::Closed,
        _ => panic!("unexpected market state value in fixture: {value}"),
    }
}

fn timestamp_utc_to_nanos(timestamp_utc: &str) -> i64 {
    DateTime::parse_from_rfc3339(timestamp_utc)
        .expect("fixture timestamp must be RFC3339")
        .timestamp_nanos_opt()
        .expect("fixture timestamp must map to nanos")
}

fn float_close(left: f64, right: f64) -> bool {
    (left - right).abs() <= FLOAT_TOLERANCE
}
