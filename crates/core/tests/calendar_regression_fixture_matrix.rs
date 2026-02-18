mod support;

use std::collections::BTreeSet;

use chrono::{DateTime, Datelike, Utc};
use janiform_core::calendar::{ExchangeCalendarRegistry, MarketState};

use support::validation_fixture::{
    CalendarRegressionCaseFixture, CalendarRegressionFixtureDocument,
    load_calendar_regression_fixture_from_manifest,
};

const REQUIRED_CASE_TAGS: [&str; 9] = [
    "regular_session",
    "weekend",
    "holiday",
    "early_close",
    "boundary",
    "timezone_sensitive",
    "delayed_open",
    "session_anomaly",
    "precedence_conflict",
];

const EXPECTED_BOUNDARY_FIXTURE_IDS: [&str; 12] = [
    "xnys_regular_pre_open",
    "xnys_regular_at_open",
    "xnys_regular_at_close",
    "xnys_early_close_pre_boundary",
    "xnys_early_close_at_boundary",
    "xnys_early_close_post_boundary",
    "xnys_delayed_open_regular_open_conflict_at_regular_open",
    "xnys_delayed_open_pre_boundary",
    "xnys_delayed_open_at_boundary",
    "xnys_holiday_overrides_delayed_open_at_boundary",
    "xtks_before_utc_midnight_transition",
    "xtks_after_utc_midnight_transition",
];

const HISTORICAL_HOLIDAY_INTRADAY_TAGS: [&str; 3] = [
    "holiday_intraday_open_boundary",
    "holiday_intraday_mid_session",
    "holiday_intraday_pre_close",
];

const HISTORICAL_HOLIDAY_YEARS: [i32; 3] = [2024, 2025, 2026];

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
fn historical_holiday_fixture_coverage_spans_years_and_intraday_slots() {
    let fixture_document = load_calendar_regression_fixture_document();
    let historical_holiday_cases: Vec<&CalendarRegressionCaseFixture> = fixture_document
        .cases
        .iter()
        .filter(|fixture_case| {
            fixture_case
                .tags
                .iter()
                .any(|tag| tag == "historical_holiday")
        })
        .collect();

    assert!(!historical_holiday_cases.is_empty());

    let observed_years: BTreeSet<i32> = historical_holiday_cases
        .iter()
        .map(|fixture_case| {
            DateTime::parse_from_rfc3339(&fixture_case.timestamp_utc)
                .expect("timestamp fixture should use RFC3339 format")
                .with_timezone(&Utc)
                .year()
        })
        .collect();
    let expected_years: BTreeSet<i32> = HISTORICAL_HOLIDAY_YEARS.into_iter().collect();

    let observed_intraday_tags: BTreeSet<String> = historical_holiday_cases
        .iter()
        .flat_map(|fixture_case| fixture_case.tags.iter())
        .filter(|tag| tag.starts_with("holiday_intraday_"))
        .cloned()
        .collect();
    let expected_intraday_tags: BTreeSet<String> = HISTORICAL_HOLIDAY_INTRADAY_TAGS
        .into_iter()
        .map(ToString::to_string)
        .collect();

    assert_eq!(observed_years, expected_years);
    assert_eq!(observed_intraday_tags, expected_intraday_tags);
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

#[test]
fn calendar_regression_historical_holiday_cases_resolve_closed_market_state() {
    let fixture_document = load_calendar_regression_fixture_document();
    let registry = ExchangeCalendarRegistry::from_artifact(fixture_document.calendar_registry)
        .expect("calendar registry fixture should load");
    let historical_holiday_cases: Vec<&CalendarRegressionCaseFixture> = fixture_document
        .cases
        .iter()
        .filter(|fixture_case| {
            fixture_case
                .tags
                .iter()
                .any(|tag| tag == "historical_holiday")
        })
        .collect();

    assert!(!historical_holiday_cases.is_empty());

    for fixture_case in historical_holiday_cases {
        let timestamp = DateTime::parse_from_rfc3339(&fixture_case.timestamp_utc)
            .expect("timestamp fixture should use RFC3339 format")
            .with_timezone(&Utc);
        let resolved_market_state = registry
            .resolve_market_state_at_utc(&fixture_case.exchange_code, timestamp)
            .expect("exchange fixture should exist and include a valid timezone");

        assert_eq!(resolved_market_state, MarketState::Closed);
    }
}

#[test]
fn calendar_regression_holiday_adjacent_non_holiday_cases_resolve_open_market_state() {
    let fixture_document = load_calendar_regression_fixture_document();
    let registry = ExchangeCalendarRegistry::from_artifact(fixture_document.calendar_registry)
        .expect("calendar registry fixture should load");
    let adjacent_non_holiday_cases: Vec<&CalendarRegressionCaseFixture> = fixture_document
        .cases
        .iter()
        .filter(|fixture_case| {
            fixture_case
                .tags
                .iter()
                .any(|tag| tag == "holiday_adjacent_non_holiday")
        })
        .collect();

    assert!(!adjacent_non_holiday_cases.is_empty());

    for fixture_case in adjacent_non_holiday_cases {
        let timestamp = DateTime::parse_from_rfc3339(&fixture_case.timestamp_utc)
            .expect("timestamp fixture should use RFC3339 format")
            .with_timezone(&Utc);
        let resolved_market_state = registry
            .resolve_market_state_at_utc(&fixture_case.exchange_code, timestamp)
            .expect("exchange fixture should exist and include a valid timezone");

        assert_eq!(resolved_market_state, MarketState::Open);
    }
}

#[test]
fn calendar_regression_early_close_transition_boundaries_resolve_expected_market_state() {
    let fixture_document = load_calendar_regression_fixture_document();
    let registry =
        ExchangeCalendarRegistry::from_artifact(fixture_document.calendar_registry.clone())
            .expect("calendar registry fixture should load");
    let early_close_boundary_case_ids = [
        "xnys_early_close_pre_boundary",
        "xnys_early_close_at_boundary",
        "xnys_early_close_post_boundary",
    ];
    let early_close_boundary_cases: Vec<&CalendarRegressionCaseFixture> =
        early_close_boundary_case_ids
            .iter()
            .map(|fixture_id| find_fixture_case_by_id(&fixture_document, fixture_id))
            .collect();

    assert_eq!(
        early_close_boundary_cases.len(),
        early_close_boundary_case_ids.len()
    );

    for fixture_case in early_close_boundary_cases {
        assert_fixture_case_matches_market_state(&registry, fixture_case);
    }
}

#[test]
fn calendar_regression_delayed_open_transition_boundaries_resolve_expected_market_state() {
    let fixture_document = load_calendar_regression_fixture_document();
    let registry =
        ExchangeCalendarRegistry::from_artifact(fixture_document.calendar_registry.clone())
            .expect("calendar registry fixture should load");
    let delayed_open_transition_case_ids = [
        "xnys_delayed_open_regular_open_conflict_at_regular_open",
        "xnys_delayed_open_pre_boundary",
        "xnys_delayed_open_at_boundary",
    ];
    let delayed_open_transition_cases: Vec<&CalendarRegressionCaseFixture> =
        delayed_open_transition_case_ids
            .iter()
            .map(|fixture_id| find_fixture_case_by_id(&fixture_document, fixture_id))
            .collect();

    assert_eq!(
        delayed_open_transition_cases.len(),
        delayed_open_transition_case_ids.len()
    );

    for fixture_case in delayed_open_transition_cases {
        assert_fixture_case_matches_market_state(&registry, fixture_case);
    }
}

#[test]
fn calendar_regression_anomaly_precedence_conflict_cases_resolve_closed_market_state() {
    let fixture_document = load_calendar_regression_fixture_document();
    let registry = ExchangeCalendarRegistry::from_artifact(fixture_document.calendar_registry)
        .expect("calendar registry fixture should load");
    let precedence_conflict_cases: Vec<&CalendarRegressionCaseFixture> = fixture_document
        .cases
        .iter()
        .filter(|fixture_case| {
            fixture_case
                .tags
                .iter()
                .any(|tag| tag == "precedence_conflict")
        })
        .collect();

    assert!(!precedence_conflict_cases.is_empty());

    for fixture_case in precedence_conflict_cases {
        let timestamp = DateTime::parse_from_rfc3339(&fixture_case.timestamp_utc)
            .expect("timestamp fixture should use RFC3339 format")
            .with_timezone(&Utc);
        let resolved_market_state = registry
            .resolve_market_state_at_utc(&fixture_case.exchange_code, timestamp)
            .expect("exchange fixture should exist and include a valid timezone");

        assert_eq!(resolved_market_state, MarketState::Closed);
    }
}

#[test]
fn calendar_regression_anomaly_boundary_timestamps_are_timezone_aware_rfc3339() {
    let fixture_document = load_calendar_regression_fixture_document();
    let anomaly_boundary_cases: Vec<&CalendarRegressionCaseFixture> = fixture_document
        .cases
        .iter()
        .filter(|fixture_case| {
            fixture_case.tags.iter().any(|tag| tag == "session_anomaly")
                && fixture_case.tags.iter().any(|tag| tag == "boundary")
        })
        .collect();

    assert!(!anomaly_boundary_cases.is_empty());

    for fixture_case in anomaly_boundary_cases {
        let parsed_timestamp = DateTime::parse_from_rfc3339(&fixture_case.timestamp_utc)
            .expect("timestamp fixture should use RFC3339 format");
        assert!(fixture_case.timestamp_utc.ends_with('Z'));
        assert_eq!(parsed_timestamp.offset().local_minus_utc(), 0);
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

fn find_fixture_case_by_id<'fixture>(
    fixture_document: &'fixture CalendarRegressionFixtureDocument,
    fixture_id: &str,
) -> &'fixture CalendarRegressionCaseFixture {
    fixture_document
        .cases
        .iter()
        .find(|fixture_case| fixture_case.fixture_id == fixture_id)
        .unwrap_or_else(|| panic!("missing calendar regression fixture case: {fixture_id}"))
}
