mod support;

use serde_json::Value;
use support::hawkes_failure_diagnostics::{
    HAWKES_FAILURE_DIAGNOSTIC_REPORT_SCHEMA_VERSION, HawkesFailureDiagnosticCategory,
    HawkesFailureDiagnosticReport, HawkesFailureDiagnosticViolation,
    build_hawkes_failure_diagnostic_report, classify_constraint_or_transition_violation_category,
    classify_statistical_violation_category,
};
use support::hawkes_invariant_catalog::HawkesInvariantCheckClass;

#[test]
fn diagnostics_payload_schema_exposes_required_machine_readable_fields() {
    let report = build_hawkes_failure_diagnostic_report(
        "hawkes_constraints_invariant_suite",
        "constraint_projection_sanity_preserves_positive_intensity_domains",
        HawkesInvariantCheckClass::DeterministicEquation,
        &["HK-CON-001"],
        vec![HawkesFailureDiagnosticViolation {
            category: HawkesFailureDiagnosticCategory::Constraint,
            scope: "violation_kind=projection scenario_id=projection_interior_positive".to_string(),
            detail: "projection mismatch for deseasonalized.lambda_plus".to_string(),
            observed_value: Some(1.35),
            reference_lower_bound: Some(1.40),
            reference_upper_bound: Some(1.40),
            delta_relative_to_reference: Some(-0.05),
        }],
    );

    let serialized_report =
        serde_json::to_value(&report).expect("diagnostics report should serialize to JSON value");

    assert_eq!(
        serialized_report["schema_version"],
        Value::String(HAWKES_FAILURE_DIAGNOSTIC_REPORT_SCHEMA_VERSION.to_string())
    );
    assert_eq!(
        serialized_report["suite_id"],
        Value::String("hawkes_constraints_invariant_suite".to_string())
    );
    assert_eq!(
        serialized_report["test_name"],
        Value::String(
            "constraint_projection_sanity_preserves_positive_intensity_domains".to_string()
        )
    );
    assert_eq!(serialized_report["invariant_ids"][0], "HK-CON-001");
    assert_eq!(serialized_report["summary"]["violation_count"], 1);
    assert_eq!(serialized_report["violations"][0]["category"], "constraint");
    assert_eq!(
        serialized_report["violations"][0]["scope"],
        "violation_kind=projection scenario_id=projection_interior_positive"
    );
}

#[test]
fn diagnostics_payload_schema_rejects_missing_required_fields() {
    let report = build_hawkes_failure_diagnostic_report(
        "hawkes_cluster_signature_statistical_regression",
        "cluster_signature_regression_reports_metric_deltas_when_bands_are_violated",
        HawkesInvariantCheckClass::StatisticalProperty,
        &["HK-SAMP-001"],
        vec![HawkesFailureDiagnosticViolation {
            category: HawkesFailureDiagnosticCategory::StatisticalDrift,
            scope: "panel=clustering horizon=n/a metric=fano_factor_gap".to_string(),
            detail: "observed=0.10 reference_band=[0.35, 4.0] delta_relative_to_band=-0.25"
                .to_string(),
            observed_value: Some(0.10),
            reference_lower_bound: Some(0.35),
            reference_upper_bound: Some(4.0),
            delta_relative_to_reference: Some(-0.25),
        }],
    );

    let mut serialized_report =
        serde_json::to_value(&report).expect("diagnostics report should serialize to JSON value");
    serialized_report
        .as_object_mut()
        .expect("serialized report should be an object")
        .remove("violations");

    let parse_result = serde_json::from_value::<HawkesFailureDiagnosticReport>(serialized_report);

    assert!(parse_result.is_err());
}

#[test]
fn failure_category_classification_is_deterministic() {
    assert_eq!(
        classify_constraint_or_transition_violation_category("parameter"),
        HawkesFailureDiagnosticCategory::Constraint
    );
    assert_eq!(
        classify_constraint_or_transition_violation_category("transition"),
        HawkesFailureDiagnosticCategory::Transition
    );
    assert_eq!(
        classify_constraint_or_transition_violation_category("ordering"),
        HawkesFailureDiagnosticCategory::Ordering
    );
    assert_eq!(
        classify_statistical_violation_category("scale_consistency", "absolute_buy_share_delta"),
        HawkesFailureDiagnosticCategory::ScaleConsistencyDrift
    );
    assert_eq!(
        classify_statistical_violation_category("clustering", "clustered_non_grid_run_ratio"),
        HawkesFailureDiagnosticCategory::EventTimeGridLeakage
    );
    assert_eq!(
        classify_statistical_violation_category("clustering", "fano_factor_gap"),
        HawkesFailureDiagnosticCategory::StatisticalDrift
    );

    let first_report = build_hawkes_failure_diagnostic_report(
        "hawkes_cluster_signature_statistical_regression",
        "cluster_signature_regression_reports_metric_deltas_when_bands_are_violated",
        HawkesInvariantCheckClass::StatisticalProperty,
        &["HK-SAMP-001"],
        vec![
            HawkesFailureDiagnosticViolation {
                category: HawkesFailureDiagnosticCategory::EventTimeGridLeakage,
                scope: "panel=clustering horizon=n/a metric=clustered_non_grid_run_ratio"
                    .to_string(),
                detail: "observed=0.8 reference_band=[0.95, 1.0] delta_relative_to_band=-0.15"
                    .to_string(),
                observed_value: Some(0.8),
                reference_lower_bound: Some(0.95),
                reference_upper_bound: Some(1.0),
                delta_relative_to_reference: Some(-0.15),
            },
            HawkesFailureDiagnosticViolation {
                category: HawkesFailureDiagnosticCategory::StatisticalDrift,
                scope: "panel=clustering horizon=n/a metric=fano_factor_gap".to_string(),
                detail: "observed=0.10 reference_band=[0.35, 4.0] delta_relative_to_band=-0.25"
                    .to_string(),
                observed_value: Some(0.10),
                reference_lower_bound: Some(0.35),
                reference_upper_bound: Some(4.0),
                delta_relative_to_reference: Some(-0.25),
            },
        ],
    );

    let second_report = build_hawkes_failure_diagnostic_report(
        "hawkes_cluster_signature_statistical_regression",
        "cluster_signature_regression_reports_metric_deltas_when_bands_are_violated",
        HawkesInvariantCheckClass::StatisticalProperty,
        &["HK-SAMP-001"],
        vec![
            HawkesFailureDiagnosticViolation {
                category: HawkesFailureDiagnosticCategory::StatisticalDrift,
                scope: "panel=clustering horizon=n/a metric=fano_factor_gap".to_string(),
                detail: "observed=0.10 reference_band=[0.35, 4.0] delta_relative_to_band=-0.25"
                    .to_string(),
                observed_value: Some(0.10),
                reference_lower_bound: Some(0.35),
                reference_upper_bound: Some(4.0),
                delta_relative_to_reference: Some(-0.25),
            },
            HawkesFailureDiagnosticViolation {
                category: HawkesFailureDiagnosticCategory::EventTimeGridLeakage,
                scope: "panel=clustering horizon=n/a metric=clustered_non_grid_run_ratio"
                    .to_string(),
                detail: "observed=0.8 reference_band=[0.95, 1.0] delta_relative_to_band=-0.15"
                    .to_string(),
                observed_value: Some(0.8),
                reference_lower_bound: Some(0.95),
                reference_upper_bound: Some(1.0),
                delta_relative_to_reference: Some(-0.15),
            },
        ],
    );

    assert_eq!(first_report.summary, second_report.summary);
    assert_eq!(first_report.violations, second_report.violations);
}
