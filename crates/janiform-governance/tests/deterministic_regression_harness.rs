mod support;

use chrono::{DateTime, Utc};
use janiform_governance::{
    CalendarStateRegressionCase, RegressionCaseFailure, RegressionHarnessToleranceConfig,
    VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION, VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY,
    VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY, VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY,
    run_deterministic_regression_harness,
};
use janiform_market_time::calendar::{ExchangeCalendarRegistry, MarketState};
use janiform_verification::RegressionCaseStatus;

use support::validation_fixture::{
    CalendarRegressionCaseFixture, load_calendar_regression_fixture_from_manifest,
};

#[test]
fn deterministic_regression_harness_matches_fixture_sets() {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar fixture should load from shared manifest");
    let registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture.fixture_document.calendar_registry,
    )
    .expect("calendar registry fixture should load");

    let calendar_cases =
        build_calendar_case_inputs(&loaded_calendar_fixture.fixture_document.cases);

    let report = run_deterministic_regression_harness(
        &registry,
        &calendar_cases,
        &[],
        RegressionHarnessToleranceConfig::default(),
    )
    .expect("harness run should succeed");

    assert_eq!(report.summary.total_case_count, calendar_cases.len());
    assert_eq!(report.summary.failed_case_count, 0);
    assert_eq!(
        report.summary.passed_case_count,
        report.summary.total_case_count
    );
    assert_eq!(
        report.fallback_policy.schema_version,
        VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION
    );
    assert_eq!(
        report.fallback_policy.unknown_date_policy,
        VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY
    );
    assert_eq!(
        report
            .fallback_policy
            .unsupported_exchange_calendar_segment_policy,
        VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY
    );
    assert_eq!(
        report.fallback_policy.incomplete_fixture_coverage_policy,
        VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY
    );
    assert!(
        report
            .case_reports
            .iter()
            .all(|case_report| case_report.status == RegressionCaseStatus::Pass)
    );
}

#[test]
fn deterministic_regression_harness_reports_fixture_mismatches_with_diagnostics() {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar fixture should load from shared manifest");
    let registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture.fixture_document.calendar_registry,
    )
    .expect("calendar registry fixture should load");

    let mut calendar_cases =
        build_calendar_case_inputs(&loaded_calendar_fixture.fixture_document.cases);

    let calendar_failure_fixture_id = calendar_cases[0].fixture_id.clone();
    calendar_cases[0].expected_state = match calendar_cases[0].expected_state {
        MarketState::Open => MarketState::Closed,
        MarketState::Closed => MarketState::Open,
    };

    let report = run_deterministic_regression_harness(
        &registry,
        &calendar_cases,
        &[],
        RegressionHarnessToleranceConfig::default(),
    )
    .expect("harness run should succeed");

    assert_eq!(report.summary.failed_case_count, 1);
    assert_eq!(
        report.summary.passed_case_count + report.summary.failed_case_count,
        report.summary.total_case_count
    );

    let calendar_failure_report = report
        .case_reports
        .iter()
        .find(|case_report| case_report.fixture_id == calendar_failure_fixture_id)
        .expect("calendar mismatch fixture should be present in report");

    assert_eq!(calendar_failure_report.status, RegressionCaseStatus::Fail);
    assert!(matches!(
        calendar_failure_report.failure,
        Some(RegressionCaseFailure::MarketStateMismatch { .. })
    ));
}

#[test]
fn deterministic_regression_harness_report_is_repeatable() {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar fixture should load from shared manifest");
    let registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture.fixture_document.calendar_registry,
    )
    .expect("calendar registry fixture should load");

    let calendar_cases =
        build_calendar_case_inputs(&loaded_calendar_fixture.fixture_document.cases);

    let first_report = run_deterministic_regression_harness(
        &registry,
        &calendar_cases,
        &[],
        RegressionHarnessToleranceConfig::default(),
    )
    .expect("first harness run should succeed");

    let second_report = run_deterministic_regression_harness(
        &registry,
        &calendar_cases,
        &[],
        RegressionHarnessToleranceConfig::default(),
    )
    .expect("second harness run should succeed");

    let first_report_json =
        serde_json::to_string(&first_report).expect("first report should serialize as json");
    let second_report_json =
        serde_json::to_string(&second_report).expect("second report should serialize as json");

    assert_eq!(first_report, second_report);
    assert_eq!(first_report_json, second_report_json);
}

#[test]
fn deterministic_regression_harness_unknown_date_uses_regular_schedule_policy() {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar fixture should load from shared manifest");
    let registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture
            .fixture_document
            .calendar_registry
            .clone(),
    )
    .expect("calendar registry fixture should load");

    let xnys_exchange = loaded_calendar_fixture
        .fixture_document
        .calendar_registry
        .exchanges
        .iter()
        .find(|exchange| exchange.exchange_code == "XNYS")
        .expect("XNYS exchange should exist in calendar fixture");

    assert!(
        !xnys_exchange
            .holiday_dates
            .iter()
            .any(|date| date == "2027-02-02")
    );
    assert!(
        !xnys_exchange
            .early_close_sessions
            .iter()
            .any(|session| session.date == "2027-02-02")
    );

    let unknown_date_case = CalendarStateRegressionCase {
        fixture_id: "xnys_unknown_date_regular_schedule_case".to_string(),
        exchange_code: "XNYS".to_string(),
        timestamp_utc: parse_utc_timestamp("2027-02-02T14:30:00Z"),
        expected_state: MarketState::Open,
    };

    let report = run_deterministic_regression_harness(
        &registry,
        &[unknown_date_case],
        &[],
        RegressionHarnessToleranceConfig::default(),
    )
    .expect("harness run should succeed");

    assert_eq!(
        report.fallback_policy.unknown_date_policy,
        VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY
    );
    assert_eq!(report.summary.total_case_count, 1);
    assert_eq!(report.summary.failed_case_count, 0);
}

fn build_calendar_case_inputs(
    fixture_cases: &[CalendarRegressionCaseFixture],
) -> Vec<CalendarStateRegressionCase> {
    fixture_cases
        .iter()
        .map(|fixture_case| CalendarStateRegressionCase {
            fixture_id: fixture_case.fixture_id.clone(),
            exchange_code: fixture_case.exchange_code.clone(),
            timestamp_utc: parse_utc_timestamp(&fixture_case.timestamp_utc),
            expected_state: parse_market_state(&fixture_case.expected_state),
        })
        .collect()
}

fn parse_market_state(raw_state: &str) -> MarketState {
    match raw_state {
        "open" => MarketState::Open,
        "closed" => MarketState::Closed,
        _ => panic!("unsupported market state value: {raw_state}"),
    }
}

fn parse_utc_timestamp(raw_timestamp: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw_timestamp)
        .unwrap_or_else(|source| {
            panic!("timestamp `{raw_timestamp}` should parse as RFC3339: {source}")
        })
        .with_timezone(&Utc)
}
