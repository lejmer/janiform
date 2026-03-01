mod support;

use chrono::{DateTime, Utc};
use janiform_core::calendar::ExchangeCalendarRegistry;
use janiform_core::news::NewsArrivalComponent;
use janiform_core::verification::{
    OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY,
    OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION,
    OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION,
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY,
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
    OpenClosedGapDeterministicReplayMetadata, OpenClosedGapIssueTraceabilityMetadata,
    OpenClosedGapLeakageRegressionCase, OpenClosedGapLeakageRegressionEventInput,
    OpenClosedGapLeakageRegressionFailure, OpenClosedGapLeakageRegressionHarnessError,
    OpenClosedGapLeakageRegressionHarnessToleranceConfig, RegressionCaseStatus,
    VerificationSemanticBucket, run_open_closed_gap_leakage_regression_harness,
};

use support::validation_fixture::{
    OpenClosedGapBehaviorScenarioCaseFixture, load_calendar_regression_fixture_from_manifest,
    load_open_closed_gap_behavior_scenario_fixture_from_manifest,
};

const OPEN_WINDOW_SCENARIO_ID: &str = "xnys_intraday_open_window_gap_suppression";

#[test]
fn open_closed_gap_leakage_regression_harness_matches_shared_fixture_suite() {
    let (registry, gap_behavior_cases) = load_gap_behavior_harness_inputs();

    let report = run_open_closed_gap_leakage_regression_harness(
        &registry,
        &gap_behavior_cases,
        OpenClosedGapLeakageRegressionHarnessToleranceConfig::default(),
    )
    .expect("gap leakage regression harness should succeed");

    assert_eq!(
        report.schema_version,
        OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION
    );
    assert_eq!(
        report.determinism_policy,
        OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY
    );
    assert_eq!(report.summary.total_case_count, gap_behavior_cases.len());
    assert_eq!(report.summary.failed_case_count, 0);
    assert_eq!(
        report.summary.passed_case_count,
        report.summary.total_case_count
    );
    assert_eq!(report.summary.total_semantic_mismatch_failure_count, 0);
    assert!(
        report.summary.scenario_class_coverage.known_only_case_count >= 1
            && report.summary.scenario_class_coverage.surprise_only_case_count >= 1
            && report.summary.scenario_class_coverage.mixed_news_case_count >= 1
    );
    assert_eq!(
        report.output_schema_metadata.schema_version,
        OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION
    );
    assert_eq!(
        report.summary_artifact.schema_version,
        OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION
    );
    assert_eq!(
        report.behavior_diff_report.schema_version,
        OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION
    );
    assert_eq!(
        report.behavior_diff_report.baseline_expected_output_policy,
        OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY
    );
    assert_eq!(
        report.summary_artifact.total_case_count,
        report.summary.total_case_count
    );
    assert_eq!(
        report.behavior_diff_report.summary.total_case_count,
        report.summary.total_case_count
    );
    assert_eq!(report.behavior_diff_report.summary.regression_case_count, 0);
    assert_eq!(
        report
            .behavior_diff_report
            .summary
            .trading_return_regression_metric_count,
        0
    );
    assert_eq!(
        report
            .behavior_diff_report
            .summary
            .open_jump_regression_metric_count,
        0
    );
    assert_eq!(
        report
            .behavior_diff_report
            .summary
            .closed_gap_regression_metric_count,
        0
    );
    assert_eq!(
        report
            .behavior_diff_report
            .summary
            .activity_coupling_regression_metric_count,
        0
    );
    assert!(
        report
            .case_reports
            .iter()
            .all(|case_report| case_report.status == RegressionCaseStatus::Pass)
    );
    assert!(report.case_reports.iter().all(|case_report| {
        let contribution_summary = &case_report.contribution_summary;
        (contribution_summary.expected_activity_coupling_total
            - contribution_summary.observed_activity_coupling_total)
            .abs()
            <= 1e-12
            && (contribution_summary.expected_trading_return_total
                - contribution_summary.observed_trading_return_total)
                .abs()
                <= 1e-12
            && (contribution_summary.expected_open_jump_total
                - contribution_summary.observed_open_jump_total)
                .abs()
                <= 1e-12
            && (contribution_summary.expected_closed_gap_total
                - contribution_summary.observed_closed_gap_total)
                .abs()
                <= 1e-12
    }));
}

#[test]
fn open_closed_gap_leakage_regression_harness_enforces_scenario_class_coverage() {
    let (registry, gap_behavior_cases) = load_gap_behavior_harness_inputs();
    let open_only_case = fixture_case_by_id(&gap_behavior_cases, OPEN_WINDOW_SCENARIO_ID).clone();

    let error = run_open_closed_gap_leakage_regression_harness(
        &registry,
        &[open_only_case],
        OpenClosedGapLeakageRegressionHarnessToleranceConfig::default(),
    )
    .expect_err("single-case suite should fail scenario class coverage requirements");

    match error {
        OpenClosedGapLeakageRegressionHarnessError::MissingScenarioClassCoverage {
            missing_scenario_classes,
        } => {
            assert!(
                !missing_scenario_classes.is_empty(),
                "error should report missing scenario classes",
            );
        }
        OpenClosedGapLeakageRegressionHarnessError::InvalidComponentAbsoluteTolerance { .. } => {
            panic!("expected missing scenario class coverage error");
        }
    }
}

#[test]
fn open_closed_gap_leakage_regression_harness_reports_semantic_bucket_for_failures() {
    let (registry, mut gap_behavior_cases) = load_gap_behavior_harness_inputs();

    let open_window_case_position = gap_behavior_cases
        .iter()
        .position(|fixture_case| fixture_case.fixture_id == OPEN_WINDOW_SCENARIO_ID)
        .expect("fixture suite should include the open-window scenario");
    let expected_event_id = gap_behavior_cases[open_window_case_position].injected_events[0]
        .event_id
        .clone();

    gap_behavior_cases[open_window_case_position].injected_events[0]
        .observed_closed_gap_g_t_increment_override = Some(0.19);

    let report = run_open_closed_gap_leakage_regression_harness(
        &registry,
        &gap_behavior_cases,
        OpenClosedGapLeakageRegressionHarnessToleranceConfig::default(),
    )
    .expect("gap leakage regression harness should succeed with deterministic failure report");

    assert_eq!(report.summary.total_case_count, gap_behavior_cases.len());
    assert_eq!(report.summary.failed_case_count, 1);
    assert_eq!(report.summary.total_semantic_mismatch_failure_count, 1);

    let failure_case = report
        .case_reports
        .iter()
        .find(|case_report| case_report.fixture_id == OPEN_WINDOW_SCENARIO_ID)
        .expect("harness should include one failing open-window case report");

    assert_eq!(failure_case.status, RegressionCaseStatus::Fail);
    assert_eq!(failure_case.semantic_mismatch_failure_count, 1);
    assert!(
        (failure_case.max_absolute_mismatch - 0.19).abs() <= 1e-12,
        "expected max mismatch magnitude to be exactly injected override",
    );

    let semantic_failure = failure_case
        .failures
        .iter()
        .find_map(|failure| match failure {
            OpenClosedGapLeakageRegressionFailure::SemanticMismatch {
                semantic_bucket,
                mismatch_kind,
                event_id,
                absolute_difference,
                ..
            } => Some((semantic_bucket, mismatch_kind, event_id, absolute_difference)),
            OpenClosedGapLeakageRegressionFailure::DispatchError { .. } => None,
        })
        .expect("failure report should include a semantic mismatch entry");

    assert_eq!(semantic_failure.0, &VerificationSemanticBucket::ClosedGap);
    assert_eq!(*semantic_failure.1, "open_window_closed_gap_leakage");
    assert_eq!(semantic_failure.2, &expected_event_id);
    assert!((*semantic_failure.3 - 0.19).abs() <= 1e-12);

    let closed_gap_diff_metric = failure_case
        .behavior_diff
        .metrics
        .iter()
        .find(|metric| metric.metric_id == "closed_gap_g_t_increment_total")
        .expect("case behavior diff must include closed-gap comparison metric");

    assert_eq!(
        closed_gap_diff_metric.semantic_bucket,
        VerificationSemanticBucket::ClosedGap
    );
    assert!(closed_gap_diff_metric.regression_detected);
    assert!((closed_gap_diff_metric.observed_value - 0.19).abs() <= 1e-12);
    assert_eq!(report.behavior_diff_report.summary.regression_case_count, 1);
    assert_eq!(
        report
            .behavior_diff_report
            .summary
            .closed_gap_regression_metric_count,
        1
    );
}

#[test]
fn open_closed_gap_leakage_regression_harness_report_is_repeatable() {
    let (registry, gap_behavior_cases) = load_gap_behavior_harness_inputs();
    let tolerance_config = OpenClosedGapLeakageRegressionHarnessToleranceConfig::default();

    let first_report = run_open_closed_gap_leakage_regression_harness(
        &registry,
        &gap_behavior_cases,
        tolerance_config.clone(),
    )
    .expect("first harness run should succeed");
    let second_report = run_open_closed_gap_leakage_regression_harness(
        &registry,
        &gap_behavior_cases,
        tolerance_config,
    )
    .expect("second harness run should succeed");

    let first_report_json =
        serde_json::to_string(&first_report).expect("first report should serialize to json");
    let second_report_json =
        serde_json::to_string(&second_report).expect("second report should serialize to json");

    assert_eq!(first_report, second_report);
    assert_eq!(first_report_json, second_report_json);
}

#[test]
fn open_closed_gap_leakage_regression_harness_report_schema_has_required_artifact_fields() {
    let (registry, gap_behavior_cases) = load_gap_behavior_harness_inputs();
    let report = run_open_closed_gap_leakage_regression_harness(
        &registry,
        &gap_behavior_cases,
        OpenClosedGapLeakageRegressionHarnessToleranceConfig::default(),
    )
    .expect("gap leakage regression harness should succeed");

    let serialized_report =
        serde_json::to_value(&report).expect("open closed gap leakage report should serialize");
    let output_schema_metadata = serialized_report
        .get("output_schema_metadata")
        .and_then(serde_json::Value::as_object)
        .expect("report output_schema_metadata should be present as an object");
    let required_summary_artifact_fields = output_schema_metadata
        .get("required_summary_artifact_fields")
        .and_then(serde_json::Value::as_array)
        .expect("required_summary_artifact_fields should be present");
    let required_behavior_diff_report_fields = output_schema_metadata
        .get("required_behavior_diff_report_fields")
        .and_then(serde_json::Value::as_array)
        .expect("required_behavior_diff_report_fields should be present");
    let required_behavior_diff_metric_fields = output_schema_metadata
        .get("required_behavior_diff_metric_fields")
        .and_then(serde_json::Value::as_array)
        .expect("required_behavior_diff_metric_fields should be present");

    let canonical_component_vocabulary = output_schema_metadata
        .get("canonical_component_vocabulary")
        .and_then(serde_json::Value::as_array)
        .expect("canonical_component_vocabulary should be present");
    assert_eq!(canonical_component_vocabulary.len(), 4);

    let summary_artifact = serialized_report
        .get("summary_artifact")
        .and_then(serde_json::Value::as_object)
        .expect("summary_artifact should be present");
    for required_field in required_summary_artifact_fields {
        let required_field = required_field
            .as_str()
            .expect("required summary artifact field name should be a string");
        assert!(
            summary_artifact.contains_key(required_field),
            "summary artifact is missing required field `{required_field}`",
        );
    }

    let behavior_diff_report = serialized_report
        .get("behavior_diff_report")
        .and_then(serde_json::Value::as_object)
        .expect("behavior_diff_report should be present");
    for required_field in required_behavior_diff_report_fields {
        let required_field = required_field
            .as_str()
            .expect("required behavior diff report field name should be a string");
        assert!(
            behavior_diff_report.contains_key(required_field),
            "behavior_diff_report is missing required field `{required_field}`",
        );
    }

    let first_metric = serialized_report["behavior_diff_report"]["scenario_diffs"][0]["metrics"][0]
        .as_object()
        .expect("first behavior diff metric should be an object");
    for required_field in required_behavior_diff_metric_fields {
        let required_field = required_field
            .as_str()
            .expect("required behavior diff metric field name should be a string");
        assert!(
            first_metric.contains_key(required_field),
            "behavior diff metric is missing required field `{required_field}`",
        );
    }

    assert!(
        first_metric.contains_key("semantic_bucket"),
        "behavior diff metric should include semantic_bucket",
    );
}

fn load_gap_behavior_harness_inputs() -> (
    ExchangeCalendarRegistry,
    Vec<OpenClosedGapLeakageRegressionCase>,
) {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from manifest");
    let registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture.fixture_document.calendar_registry,
    )
    .expect("calendar registry fixture should load");

    let loaded_gap_behavior_fixture =
        load_open_closed_gap_behavior_scenario_fixture_from_manifest()
            .expect("open and closed gap behavior fixture should load from manifest");
    let gap_behavior_cases =
        build_gap_behavior_case_inputs(&loaded_gap_behavior_fixture.fixture_document.cases);

    (registry, gap_behavior_cases)
}

fn build_gap_behavior_case_inputs(
    fixture_cases: &[OpenClosedGapBehaviorScenarioCaseFixture],
) -> Vec<OpenClosedGapLeakageRegressionCase> {
    fixture_cases
        .iter()
        .map(|fixture_case| OpenClosedGapLeakageRegressionCase {
            fixture_id: fixture_case.fixture_id.clone(),
            exchange_code: fixture_case.exchange_code.clone(),
            scenario_class: parse_scenario_class(fixture_case.scenario_class.as_str()),
            deterministic_replay_metadata: OpenClosedGapDeterministicReplayMetadata {
                replay_baseline_id: fixture_case
                    .deterministic_replay_metadata
                    .replay_baseline_id
                    .clone(),
                event_calendar_binding_id: fixture_case
                    .deterministic_replay_metadata
                    .event_calendar_binding_id
                    .clone(),
                jump_observation_binding_id: fixture_case
                    .deterministic_replay_metadata
                    .jump_observation_binding_id
                    .clone(),
                gap_observation_binding_id: fixture_case
                    .deterministic_replay_metadata
                    .gap_observation_binding_id
                    .clone(),
                candidate_selection_policy_id: fixture_case
                    .deterministic_replay_metadata
                    .candidate_selection_policy_id
                    .clone(),
                candidate_selection_rank: fixture_case
                    .deterministic_replay_metadata
                    .candidate_selection_rank as u64,
            },
            issue_traceability: OpenClosedGapIssueTraceabilityMetadata {
                issue_id: fixture_case.issue_traceability.issue_id.clone(),
                semantic_focus_tags: fixture_case.issue_traceability.semantic_focus_tags.clone(),
            },
            initial_j_t: fixture_case.initial_accumulator_state.j_t,
            initial_m_t: fixture_case.initial_accumulator_state.m_t,
            initial_g_t: fixture_case.initial_accumulator_state.g_t,
            injected_events: fixture_case
                .injected_events
                .iter()
                .map(|injected_event| OpenClosedGapLeakageRegressionEventInput {
                    event_id: injected_event.event_id.clone(),
                    event_timestamp_utc: parse_utc_timestamp(&injected_event.timestamp_utc),
                    news_arrival_component: parse_news_arrival_component(
                        injected_event.news_arrival_component.as_str(),
                    ),
                    shared_severity_mark: injected_event.shared_severity_mark,
                    coupled_return_jump_mark: injected_event.coupled_return_jump_mark,
                    observed_activity_coupling_j_t_increment_override: None,
                    observed_open_jump_m_t_increment_override: None,
                    observed_closed_gap_g_t_increment_override: None,
                    observed_trading_return_increment_override: None,
                })
                .collect(),
        })
        .collect()
}

fn fixture_case_by_id<'a>(
    fixture_cases: &'a [OpenClosedGapLeakageRegressionCase],
    fixture_id: &str,
) -> &'a OpenClosedGapLeakageRegressionCase {
    fixture_cases
        .iter()
        .find(|fixture_case| fixture_case.fixture_id == fixture_id)
        .expect("fixture id should exist")
}

fn parse_utc_timestamp(raw_timestamp: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw_timestamp)
        .unwrap_or_else(|source| {
            panic!("timestamp `{raw_timestamp}` should parse as RFC3339: {source}")
        })
        .with_timezone(&Utc)
}

fn parse_news_arrival_component(raw_component: &str) -> NewsArrivalComponent {
    match raw_component {
        "known" => NewsArrivalComponent::Known,
        "surprise" => NewsArrivalComponent::Surprise,
        _ => panic!("unsupported news_arrival_component `{raw_component}` in fixture"),
    }
}

fn parse_scenario_class(
    raw_class: &str,
) -> janiform_core::verification::NewsSemanticsScenarioClass {
    match raw_class {
        "known_only" => janiform_core::verification::NewsSemanticsScenarioClass::KnownOnly,
        "surprise_only" => janiform_core::verification::NewsSemanticsScenarioClass::SurpriseOnly,
        "mixed_news" => janiform_core::verification::NewsSemanticsScenarioClass::MixedNews,
        _ => panic!("unsupported scenario_class `{raw_class}` in fixture"),
    }
}
