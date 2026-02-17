mod support;

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use janiform_core::calendar::{
    ExchangeCalendarRegistry, MarketState,
};

use support::validation_fixture::{
    load_calendar_regression_fixture_from_manifest, CalendarRegressionCaseFixture,
    CalendarRegressionFixtureDocument,
};

const REQUIRED_CASE_TAGS: [&str; 6] = [
    "regular_session",
    "weekend",
    "holiday",
    "early_close",
    "boundary",
    "timezone_sensitive",
];

const EXPECTED_BOUNDARY_FIXTURE_IDS: [&str; 7] = [
    "xnys_regular_pre_open",
    "xnys_regular_at_open",
    "xnys_regular_at_close",
    "xnys_early_close_pre_boundary",
    "xnys_early_close_at_boundary",
    "xtks_before_utc_midnight_transition",
    "xtks_after_utc_midnight_transition",
];

#[test]
fn calendar_regression_fixture_covers_required_case_tags() {
    let fixture_document = load_calendar_regression_fixture_document();
    let observed_tags: BTreeSet<String> = fixture_document
        .cases
        .iter()
        .flat_map(|fixture_case| fixture_case.tags.iter().cloned())
        .collect();
    let required_tags: BTreeSet<String> = REQUIRED_CASE_TAGS
        .into_iter()
        .map(ToString::to_string)
        .collect();

    assert!(required_tags.is_subset(&observed_tags));
}

#[test]
fn calendar_boundary_fixture_identifiers_remain_stable() {
    let fixture_document = load_calendar_regression_fixture_document();
    let observed_boundary_fixture_ids: BTreeSet<String> = fixture_document
        .cases
        .iter()
        .filter(|fixture_case| fixture_case.tags.iter().any(|tag| tag == "boundary"))
        .map(|fixture_case| fixture_case.fixture_id.clone())
        .collect();
    let expected_boundary_fixture_ids: BTreeSet<String> = EXPECTED_BOUNDARY_FIXTURE_IDS
        .into_iter()
        .map(ToString::to_string)
        .collect();

    assert_eq!(observed_boundary_fixture_ids, expected_boundary_fixture_ids);
}

#[test]
fn calendar_regression_golden_matrix_cases_resolve_expected_market_state() {
    let fixture_document = load_calendar_regression_fixture_document();
    let registry = ExchangeCalendarRegistry::from_artifact(fixture_document.calendar_registry)
        .expect("calendar registry fixture should load");

    for fixture_case in &fixture_document.cases {
        assert_fixture_case_matches_market_state(&registry, fixture_case);
    }
}

#[test]
fn calendar_regression_boundary_cases_resolve_expected_market_state() {
    let fixture_document = load_calendar_regression_fixture_document();
    let registry = ExchangeCalendarRegistry::from_artifact(fixture_document.calendar_registry)
        .expect("calendar registry fixture should load");
    let boundary_cases: Vec<&CalendarRegressionCaseFixture> = fixture_document
        .cases
        .iter()
        .filter(|fixture_case| fixture_case.tags.iter().any(|tag| tag == "boundary"))
        .collect();

    assert!(!boundary_cases.is_empty());

    for fixture_case in boundary_cases {
        assert_fixture_case_matches_market_state(&registry, fixture_case);
    }
}

#[test]
fn calendar_regression_timezone_sensitive_cases_resolve_expected_market_state() {
    let fixture_document = load_calendar_regression_fixture_document();
    let registry = ExchangeCalendarRegistry::from_artifact(fixture_document.calendar_registry)
        .expect("calendar registry fixture should load");
    let timezone_sensitive_cases: Vec<&CalendarRegressionCaseFixture> = fixture_document
        .cases
        .iter()
        .filter(|fixture_case| {
            fixture_case
                .tags
                .iter()
                .any(|tag| tag == "timezone_sensitive")
        })
        .collect();

    assert!(!timezone_sensitive_cases.is_empty());

    for fixture_case in timezone_sensitive_cases {
        assert_fixture_case_matches_market_state(&registry, fixture_case);
    }
}

fn assert_fixture_case_matches_market_state(
    registry: &ExchangeCalendarRegistry,
    fixture_case: &CalendarRegressionCaseFixture,
) {
    let timestamp = DateTime::parse_from_rfc3339(&fixture_case.timestamp_utc)
        .expect("timestamp fixture should use RFC3339 format")
        .with_timezone(&Utc);
    let expected_market_state = parse_expected_market_state(&fixture_case.expected_state);
    let resolved_market_state = registry
        .resolve_market_state_at_utc(&fixture_case.exchange_code, timestamp)
        .expect("exchange fixture should exist and include a valid timezone");

    assert_eq!(
        resolved_market_state, expected_market_state,
        "fixture {} ({}) expected {}",
        fixture_case.fixture_id, fixture_case.description, fixture_case.expected_state
    );
}

fn parse_expected_market_state(raw_market_state: &str) -> MarketState {
    match raw_market_state {
        "open" => MarketState::Open,
        "closed" => MarketState::Closed,
        _ => panic!("unexpected expected_state value in fixture: {raw_market_state}"),
    }
}

fn load_calendar_regression_fixture_document() -> CalendarRegressionFixtureDocument {
    load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from shared manifest")
        .fixture_document
}
