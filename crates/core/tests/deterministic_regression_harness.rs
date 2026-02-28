mod support;

use chrono::{DateTime, Utc};
use janiform_core::calendar::{ExchangeCalendarRegistry, MarketState};
use janiform_core::verification::{
    CalendarStateRegressionCase, HourlyFactorRegressionCase, HourlyNewsIntensityRegressionSource,
    HourlySeasonalityRegressionSource, RegressionCaseFailure, RegressionCaseStatus,
    RegressionHarnessToleranceConfig, VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION,
    VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY, VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY,
    VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY, run_deterministic_regression_harness,
};

use support::validation_fixture::{
    CalendarRegressionCaseFixture, HourlyFactorCanonicalOutputCaseFixture,
    HourlyFactorRegressionFixtureDocument, HourlyNewsIntensitySourceFixture,
    HourlySeasonalitySourceFixture, STATE_CLASSIFICATION_STRICTNESS_POLICY,
    load_calendar_regression_fixture_from_manifest,
    load_hourly_factor_regression_fixture_from_manifest,
};

#[test]
fn deterministic_regression_harness_matches_fixture_sets() {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar fixture should load from shared manifest");
    let registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture.fixture_document.calendar_registry,
    )
    .expect("calendar registry fixture should load");
    let hourly_fixture_document = load_hourly_factor_fixture_document();

    let calendar_cases =
        build_calendar_case_inputs(&loaded_calendar_fixture.fixture_document.cases);
    let hourly_cases =
        build_hourly_factor_case_inputs(&hourly_fixture_document.canonical_output_cases);
    let tolerance_config = harness_tolerance_config(&hourly_fixture_document);

    let report = run_deterministic_regression_harness(
        &registry,
        &calendar_cases,
        &hourly_cases,
        tolerance_config,
    )
    .expect("harness run should succeed");

    assert_eq!(
        report.summary.total_case_count,
        calendar_cases.len() + hourly_cases.len()
    );
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
    let hourly_fixture_document = load_hourly_factor_fixture_document();

    let mut calendar_cases =
        build_calendar_case_inputs(&loaded_calendar_fixture.fixture_document.cases);
    let mut hourly_cases =
        build_hourly_factor_case_inputs(&hourly_fixture_document.canonical_output_cases);

    let calendar_failure_fixture_id = calendar_cases[0].fixture_id.clone();
    calendar_cases[0].expected_state = match calendar_cases[0].expected_state {
        MarketState::Open => MarketState::Closed,
        MarketState::Closed => MarketState::Open,
    };

    let hourly_failure_fixture_id = hourly_cases[0].fixture_id.clone();
    hourly_cases[0].expected_s_h[0] += 0.25;

    assert_eq!(
        hourly_fixture_document
            .threshold_configuration
            .state_classification_strictness,
        STATE_CLASSIFICATION_STRICTNESS_POLICY
    );

    let tolerance_config = harness_tolerance_config(&hourly_fixture_document);
    let report = run_deterministic_regression_harness(
        &registry,
        &calendar_cases,
        &hourly_cases,
        tolerance_config,
    )
    .expect("harness run should succeed");

    assert_eq!(report.summary.failed_case_count, 2);
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

    let hourly_failure_report = report
        .case_reports
        .iter()
        .find(|case_report| case_report.fixture_id == hourly_failure_fixture_id)
        .expect("hourly mismatch fixture should be present in report");
    assert_eq!(hourly_failure_report.status, RegressionCaseStatus::Fail);
    assert!(matches!(
        hourly_failure_report.failure,
        Some(RegressionCaseFailure::FactorToleranceExceeded {
            field_name: "s_h",
            ..
        })
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

    let hourly_fixture_document = load_hourly_factor_fixture_document();

    let calendar_cases =
        build_calendar_case_inputs(&loaded_calendar_fixture.fixture_document.cases);
    let hourly_cases =
        build_hourly_factor_case_inputs(&hourly_fixture_document.canonical_output_cases);
    let tolerance_config = harness_tolerance_config(&hourly_fixture_document);

    let first_report = run_deterministic_regression_harness(
        &registry,
        &calendar_cases,
        &hourly_cases,
        tolerance_config.clone(),
    )
    .expect("first harness run should succeed");

    let second_report = run_deterministic_regression_harness(
        &registry,
        &calendar_cases,
        &hourly_cases,
        tolerance_config,
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
fn deterministic_regression_harness_threshold_boundary_is_inclusive() {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar fixture should load from shared manifest");

    let registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture.fixture_document.calendar_registry,
    )
    .expect("calendar registry fixture should load");

    let hourly_fixture_document = load_hourly_factor_fixture_document();

    let calendar_cases =
        build_calendar_case_inputs(&loaded_calendar_fixture.fixture_document.cases);
    let mut hourly_cases =
        build_hourly_factor_case_inputs(&hourly_fixture_document.canonical_output_cases);

    let mut tolerance_config = harness_tolerance_config(&hourly_fixture_document);
    tolerance_config.seasonality_absolute_tolerance = 0.25;
    hourly_cases[0].expected_s_h[0] += 0.25;

    let report = run_deterministic_regression_harness(
        &registry,
        &calendar_cases,
        &hourly_cases,
        tolerance_config,
    )
    .expect("harness run should succeed");

    assert_eq!(report.summary.failed_case_count, 0);
}

#[test]
fn deterministic_regression_harness_threshold_boundary_fails_when_exceeded() {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar fixture should load from shared manifest");
    let registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture.fixture_document.calendar_registry,
    )
    .expect("calendar registry fixture should load");

    let hourly_fixture_document = load_hourly_factor_fixture_document();

    let calendar_cases =
        build_calendar_case_inputs(&loaded_calendar_fixture.fixture_document.cases);
    let mut hourly_cases =
        build_hourly_factor_case_inputs(&hourly_fixture_document.canonical_output_cases);

    let mut tolerance_config = harness_tolerance_config(&hourly_fixture_document);
    tolerance_config.seasonality_absolute_tolerance = 0.25;
    hourly_cases[0].expected_s_h[0] += 0.2501;

    let report = run_deterministic_regression_harness(
        &registry,
        &calendar_cases,
        &hourly_cases,
        tolerance_config,
    )
    .expect("harness run should succeed");

    assert_eq!(report.summary.failed_case_count, 1);
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

fn build_hourly_factor_case_inputs(
    fixture_cases: &[HourlyFactorCanonicalOutputCaseFixture],
) -> Vec<HourlyFactorRegressionCase> {
    fixture_cases
        .iter()
        .map(|fixture_case| HourlyFactorRegressionCase {
            fixture_id: fixture_case.fixture_id.clone(),
            exchange_code: fixture_case.exchange_code.clone(),
            range_start_utc: parse_utc_timestamp(&fixture_case.range_start_utc),
            range_end_utc: parse_utc_timestamp(&fixture_case.range_end_utc),
            seasonality_source: parse_hourly_seasonality_source(&fixture_case.seasonality_source),
            news_intensity_source: parse_hourly_news_intensity_source(
                &fixture_case.news_intensity_source,
            ),
            expected_s_h: fixture_case.expected_s_h.clone(),
            expected_nu_h: fixture_case.expected_nu_h.clone(),
        })
        .collect()
}

fn parse_hourly_seasonality_source(
    source_fixture: &HourlySeasonalitySourceFixture,
) -> HourlySeasonalityRegressionSource {
    match source_fixture {
        HourlySeasonalitySourceFixture::Explicit {
            hourly_factors,
            materialization_method,
        } => HourlySeasonalityRegressionSource::Explicit {
            hourly_factors: hourly_factors.clone(),
            materialization_method: materialization_method.clone(),
        },
        HourlySeasonalitySourceFixture::Materialized {
            source_timestamps_utc,
            source_seasonality_factors,
            interpolation_method,
            aggregation_method,
            sparse_fallback_policy,
            sparse_hour_minimum_observation_count,
        } => {
            let source_timestamp_utc_nanos: Vec<i64> = source_timestamps_utc
                .iter()
                .map(|timestamp| parse_utc_timestamp_to_nanos(timestamp))
                .collect();

            HourlySeasonalityRegressionSource::Materialized {
                source_timestamp_utc_nanos,
                source_seasonality_factors: source_seasonality_factors.clone(),
                interpolation_method: interpolation_method.clone(),
                aggregation_method: aggregation_method.clone(),
                sparse_fallback_policy: sparse_fallback_policy.clone(),
                sparse_hour_minimum_observation_count: *sparse_hour_minimum_observation_count,
            }
        }
    }
}

fn parse_hourly_news_intensity_source(
    source_fixture: &HourlyNewsIntensitySourceFixture,
) -> HourlyNewsIntensityRegressionSource {
    match source_fixture {
        HourlyNewsIntensitySourceFixture::Explicit {
            hourly_factors,
            materialization_method,
        } => HourlyNewsIntensityRegressionSource::Explicit {
            hourly_factors: hourly_factors.clone(),
            materialization_method: materialization_method.clone(),
        },
        HourlyNewsIntensitySourceFixture::Materialized {
            known_scheduled_event_timestamps_utc,
            surprise_baseline_intensity,
            surprise_deterministic_time_multiplier_by_interval_index,
        } => {
            let known_scheduled_event_timestamps_utc_nanos: Vec<i64> =
                known_scheduled_event_timestamps_utc
                    .iter()
                    .map(|known_timestamp| parse_utc_timestamp_to_nanos(known_timestamp))
                    .collect();

            let mut parsed_surprise_deterministic_time_multiplier_by_interval_index: Vec<(u64, f64)> =
                surprise_deterministic_time_multiplier_by_interval_index
                    .iter()
                    .map(|(interval_index, multiplier_value)| {
                        let parsed_interval_index = interval_index.parse::<u64>().unwrap_or_else(|source| {
                            panic!(
                                "surprise_deterministic_time_multiplier_by_interval_index key `{interval_index}` should parse as u64: {source}"
                            )
                        });
                        (parsed_interval_index, *multiplier_value)
                    })
                    .collect();

            parsed_surprise_deterministic_time_multiplier_by_interval_index
                .sort_by_key(|(interval_index, _)| *interval_index);

            HourlyNewsIntensityRegressionSource::Materialized {
                known_scheduled_event_timestamps_utc_nanos,
                surprise_baseline_intensity: *surprise_baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index:
                    parsed_surprise_deterministic_time_multiplier_by_interval_index,
            }
        }
    }
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

fn parse_utc_timestamp_to_nanos(raw_timestamp: &str) -> i64 {
    parse_utc_timestamp(raw_timestamp)
        .timestamp_nanos_opt()
        .unwrap_or_else(|| panic!("timestamp `{raw_timestamp}` should convert to nanoseconds"))
}

fn load_hourly_factor_fixture_document() -> HourlyFactorRegressionFixtureDocument {
    load_hourly_factor_regression_fixture_from_manifest()
        .expect("hourly factor fixture should parse as json")
        .fixture_document
}

fn harness_tolerance_config(
    fixture_document: &HourlyFactorRegressionFixtureDocument,
) -> RegressionHarnessToleranceConfig {
    RegressionHarnessToleranceConfig {
        seasonality_absolute_tolerance: fixture_document
            .threshold_configuration
            .tolerance_config
            .s_h_absolute_tolerance,
        news_intensity_absolute_tolerance: fixture_document
            .threshold_configuration
            .tolerance_config
            .nu_h_absolute_tolerance,
    }
}
