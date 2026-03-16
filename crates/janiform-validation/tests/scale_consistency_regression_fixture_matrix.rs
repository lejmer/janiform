mod support;

use std::collections::BTreeMap;
use std::fs;

use janiform_validation::{
    RegressionCaseStatus, build_scale_consistency_report,
    load_scale_consistency_threshold_policy_from_json, render_scale_consistency_report_summary,
    run_scale_consistency_threshold_decision_engine,
};

use support::scale_consistency_regression_fixture::{
    SCALE_CONSISTENCY_REGRESSION_FIXTURE_SET_ID, ScaleConsistencyRegressionCase,
    ScaleConsistencyRegressionExpectedMetricOutcome,
    load_scale_consistency_regression_fixture_from_manifest,
};
use support::validation_fixture::fixtures_root_path;

#[test]
fn scale_consistency_regression_fixture_matches_reference_outputs_and_is_stable() {
    let loaded_fixture = load_scale_consistency_regression_fixture_from_manifest()
        .expect("scale consistency regression fixture should load from manifest");
    assert_eq!(
        loaded_fixture.manifest_entry.fixture_id,
        SCALE_CONSISTENCY_REGRESSION_FIXTURE_SET_ID
    );

    let threshold_policy_payload = fs::read_to_string(
        fixtures_root_path()
            .join("validation")
            .join("scale_consistency_threshold_policy.json"),
    )
    .expect("scale consistency threshold policy fixture should be readable");
    let threshold_policy =
        load_scale_consistency_threshold_policy_from_json(threshold_policy_payload.as_str())
            .expect("scale consistency threshold policy fixture should parse");

    assert_eq!(
        loaded_fixture
            .fixture_document
            .threshold_policy_reference
            .policy_id,
        threshold_policy.policy_id
    );
    assert_eq!(
        loaded_fixture
            .fixture_document
            .threshold_policy_reference
            .policy_version,
        threshold_policy.policy_version
    );

    for fixture_case in &loaded_fixture.fixture_document.cases {
        assert_case_matches_reference(fixture_case, &threshold_policy);
    }
}

#[test]
fn scale_consistency_regression_fixture_manifest_metadata_is_complete() {
    let loaded_fixture = load_scale_consistency_regression_fixture_from_manifest()
        .expect("scale consistency regression fixture should load from manifest");

    let metadata = &loaded_fixture
        .fixture_document
        .canonical_regression_metadata;
    assert!(
        metadata
            .required_tags
            .contains(&"stable_reference".to_string())
    );
    assert!(
        metadata
            .required_interval_grid_labels
            .contains(&"subhour_grid".to_string())
    );
    assert!(
        metadata
            .required_interval_grid_labels
            .contains(&"hourly_grid".to_string())
    );
    assert!(
        metadata
            .required_expected_scenario_statuses
            .contains(&RegressionCaseStatus::Pass)
    );
    assert!(
        metadata
            .required_expected_scenario_statuses
            .contains(&RegressionCaseStatus::Fail)
    );
    assert_eq!(metadata.stable_fixture_ids.len(), 4);
}

fn assert_case_matches_reference(
    fixture_case: &ScaleConsistencyRegressionCase,
    threshold_policy: &janiform_validation::ScaleConsistencyThresholdPolicy,
) {
    let metric_delta_report = fixture_case.to_metric_delta_batch_report();
    let engine_benchmark_input = fixture_case.to_engine_benchmark_input();

    let first_decision_report = run_scale_consistency_threshold_decision_engine(
        &metric_delta_report,
        threshold_policy,
        &engine_benchmark_input,
    )
    .expect("first decision run should succeed");
    let second_decision_report = run_scale_consistency_threshold_decision_engine(
        &metric_delta_report,
        threshold_policy,
        &engine_benchmark_input,
    )
    .expect("second decision run should succeed");
    assert_eq!(
        first_decision_report, second_decision_report,
        "fixture_id={} decision report changed across repeated runs",
        fixture_case.fixture_id
    );

    let first_report = build_scale_consistency_report(&first_decision_report)
        .expect("first report build should succeed");
    let second_report = build_scale_consistency_report(&second_decision_report)
        .expect("second report build should succeed");
    assert_eq!(
        first_report, second_report,
        "fixture_id={} report changed across repeated runs",
        fixture_case.fixture_id
    );

    let first_rendered_summary = render_scale_consistency_report_summary(&first_report);
    let second_rendered_summary = render_scale_consistency_report_summary(&second_report);
    assert_eq!(
        first_rendered_summary, second_rendered_summary,
        "fixture_id={} rendered summary changed across repeated runs",
        fixture_case.fixture_id
    );

    assert_eq!(
        first_decision_report.status, fixture_case.expected_outcome.expected_batch_status,
        "fixture_id={} unexpected batch status",
        fixture_case.fixture_id
    );
    assert_eq!(
        first_report.status, fixture_case.expected_outcome.expected_batch_status,
        "fixture_id={} unexpected report status",
        fixture_case.fixture_id
    );
    assert_eq!(
        first_decision_report.scenario_reports.len(),
        1,
        "fixture_id={} should produce exactly one scenario report",
        fixture_case.fixture_id
    );

    let scenario_report = &first_decision_report.scenario_reports[0];
    assert_eq!(
        scenario_report.status, fixture_case.expected_outcome.expected_scenario_status,
        "fixture_id={} unexpected scenario status",
        fixture_case.fixture_id
    );
    assert_eq!(
        scenario_report.applied_interval_threshold_band_label,
        fixture_case.expected_applied_interval_threshold_band_label,
        "fixture_id={} unexpected interval threshold band",
        fixture_case.fixture_id
    );
    assert_eq!(
        scenario_report.failed_metric_count,
        fixture_case.expected_outcome.expected_failed_metric_count,
        "fixture_id={} unexpected failed metric count",
        fixture_case.fixture_id
    );

    let expected_metric_outcomes: BTreeMap<
        (&str, Option<&str>),
        &ScaleConsistencyRegressionExpectedMetricOutcome,
    > = fixture_case
        .expected_outcome
        .expected_metric_outcomes
        .iter()
        .map(|expected_metric_outcome| {
            (
                (
                    expected_metric_outcome.metric_id.as_str(),
                    expected_metric_outcome.metric_variant.as_deref(),
                ),
                expected_metric_outcome,
            )
        })
        .collect();

    assert_eq!(
        scenario_report.metric_decisions.len(),
        expected_metric_outcomes.len(),
        "fixture_id={} unexpected metric decision count",
        fixture_case.fixture_id
    );

    for metric_decision in &scenario_report.metric_decisions {
        let expected_metric_outcome = expected_metric_outcomes
            .get(&(metric_decision.metric_id.as_str(), metric_decision.metric_variant.as_deref()))
            .unwrap_or_else(|| {
                panic!(
                    "fixture_id={} missing expected metric outcome for metric_id={} metric_variant={:?}",
                    fixture_case.fixture_id, metric_decision.metric_id, metric_decision.metric_variant
                )
            });

        assert_eq!(
            metric_decision.status, expected_metric_outcome.expected_status,
            "fixture_id={} metric_id={} metric_variant={:?} unexpected metric status",
            fixture_case.fixture_id, metric_decision.metric_id, metric_decision.metric_variant
        );
        assert_eq!(
            metric_decision.threshold_context.threshold_id,
            expected_metric_outcome.expected_threshold_id,
            "fixture_id={} metric_id={} metric_variant={:?} unexpected threshold id",
            fixture_case.fixture_id,
            metric_decision.metric_id,
            metric_decision.metric_variant
        );

        let observed_reason_codes = metric_decision
            .decision_reason_codes
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let expected_reason_codes = expected_metric_outcome
            .expected_decision_reason_codes
            .iter()
            .cloned()
            .map(|reason_code| reason_code.into_runtime())
            .collect::<Vec<_>>();
        assert_eq!(
            observed_reason_codes, expected_reason_codes,
            "fixture_id={} metric_id={} metric_variant={:?} unexpected decision reason codes",
            fixture_case.fixture_id, metric_decision.metric_id, metric_decision.metric_variant
        );
    }

    let report_scenario = &first_report.scenario_reports[0];
    assert_eq!(
        report_scenario.primary_drift_category,
        fixture_case
            .expected_outcome
            .expected_primary_drift_category
            .clone()
            .map(|drift_category| drift_category.into_runtime()),
        "fixture_id={} unexpected primary drift category",
        fixture_case.fixture_id
    );
    assert_eq!(
        report_scenario.non_drift_failure_reason_codes,
        fixture_case
            .expected_outcome
            .expected_non_drift_failure_reason_codes
            .iter()
            .cloned()
            .map(|reason_code| reason_code.into_runtime())
            .collect::<Vec<_>>(),
        "fixture_id={} unexpected non-drift failure reason codes",
        fixture_case.fixture_id
    );
    assert_eq!(
        first_rendered_summary, fixture_case.expected_outcome.expected_rendered_summary,
        "fixture_id={} rendered summary mismatch",
        fixture_case.fixture_id
    );
}
