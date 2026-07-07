#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

use crate::hawkes_invariant_catalog::HawkesInvariantCheckClass;

pub const HAWKES_FAILURE_DIAGNOSTIC_REPORT_SCHEMA_VERSION: &str =
    "hawkes.failure_diagnostic_report.v1";
pub const HAWKES_FAILURE_DIAGNOSTIC_DETERMINISM_POLICY: &str =
    "stable_category_classification_and_sorted_metadata.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HawkesFailureDiagnosticCategory {
    Constraint,
    Transition,
    Ordering,
    StatisticalDrift,
    ScaleConsistencyDrift,
    EventTimeGridLeakage,
    TimestampSnappingRegression,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesFailureDiagnosticSummary {
    pub violation_count: usize,
    pub category_counts: Vec<HawkesFailureDiagnosticCategoryCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesFailureDiagnosticCategoryCount {
    pub category: HawkesFailureDiagnosticCategory,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesFailureDiagnosticViolation {
    pub category: HawkesFailureDiagnosticCategory,
    pub scope: String,
    pub detail: String,
    pub observed_value: Option<f64>,
    pub reference_lower_bound: Option<f64>,
    pub reference_upper_bound: Option<f64>,
    pub delta_relative_to_reference: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesFailureDiagnosticReport {
    pub schema_version: String,
    pub determinism_policy: String,
    pub suite_id: String,
    pub test_name: String,
    pub check_class: HawkesInvariantCheckClass,
    pub invariant_ids: Vec<String>,
    pub summary: HawkesFailureDiagnosticSummary,
    pub violations: Vec<HawkesFailureDiagnosticViolation>,
}

pub fn build_hawkes_failure_diagnostic_report(
    suite_id: &str,
    test_name: &str,
    check_class: HawkesInvariantCheckClass,
    invariant_ids: &[&str],
    mut violations: Vec<HawkesFailureDiagnosticViolation>,
) -> HawkesFailureDiagnosticReport {
    let mut normalized_invariant_ids: Vec<String> = invariant_ids
        .iter()
        .map(|invariant_id| (*invariant_id).to_string())
        .collect();
    normalized_invariant_ids.sort();

    violations.sort_by(|left_violation, right_violation| {
        left_violation
            .category
            .cmp(&right_violation.category)
            .then_with(|| left_violation.scope.cmp(&right_violation.scope))
            .then_with(|| left_violation.detail.cmp(&right_violation.detail))
    });

    let mut category_count_lookup: BTreeMap<HawkesFailureDiagnosticCategory, usize> =
        BTreeMap::new();

    for violation in &violations {
        let current_count = category_count_lookup.entry(violation.category).or_insert(0);
        *current_count = current_count.saturating_add(1);
    }

    let category_counts: Vec<HawkesFailureDiagnosticCategoryCount> = category_count_lookup
        .into_iter()
        .map(|(category, count)| HawkesFailureDiagnosticCategoryCount { category, count })
        .collect();

    HawkesFailureDiagnosticReport {
        schema_version: HAWKES_FAILURE_DIAGNOSTIC_REPORT_SCHEMA_VERSION.to_string(),
        determinism_policy: HAWKES_FAILURE_DIAGNOSTIC_DETERMINISM_POLICY.to_string(),
        suite_id: suite_id.to_string(),
        test_name: test_name.to_string(),
        check_class,
        invariant_ids: normalized_invariant_ids,
        summary: HawkesFailureDiagnosticSummary {
            violation_count: violations.len(),
            category_counts,
        },
        violations,
    }
}

pub fn render_hawkes_failure_diagnostic_report(report: &HawkesFailureDiagnosticReport) -> String {
    serde_json::to_string_pretty(report)
        .expect("hawkes failure diagnostic report serialization should succeed")
}

pub fn classify_constraint_or_transition_violation_category(
    violation_kind: &str,
) -> HawkesFailureDiagnosticCategory {
    match violation_kind {
        "projection" | "parameter" | "constraint" => HawkesFailureDiagnosticCategory::Constraint,
        "transition" => HawkesFailureDiagnosticCategory::Transition,
        "ordering" => HawkesFailureDiagnosticCategory::Ordering,
        "timestamp_snapping" => HawkesFailureDiagnosticCategory::TimestampSnappingRegression,
        _ => HawkesFailureDiagnosticCategory::Transition,
    }
}

pub fn classify_statistical_violation_category(
    panel_id: &str,
    metric_id: &str,
) -> HawkesFailureDiagnosticCategory {
    match panel_id {
        "scale_consistency" => HawkesFailureDiagnosticCategory::ScaleConsistencyDrift,
        "clustering" => match metric_id {
            "clustered_non_grid_run_ratio" => HawkesFailureDiagnosticCategory::EventTimeGridLeakage,
            _ => HawkesFailureDiagnosticCategory::StatisticalDrift,
        },
        _ => HawkesFailureDiagnosticCategory::StatisticalDrift,
    }
}
