//! Concise scale-consistency report generation and drift diagnostics rendering.

use std::cmp::Ordering;

use serde::Serialize;

use super::super::contracts::RegressionCaseStatus;

use super::{
    SCALE_CONSISTENCY_DECISION_ORDERING_POLICY, SCALE_CONSISTENCY_DECISION_REPORT_SCHEMA_VERSION,
    ScaleConsistencyDecisionBatchReport, ScaleConsistencyDecisionReasonCode,
    ScaleConsistencyDecisionScenarioReport, ScaleConsistencyMetricCategory,
    ScaleConsistencyMetricComputationStatus, ScaleConsistencyMetricDecision,
};

pub const SCALE_CONSISTENCY_REPORT_SCHEMA_VERSION: &str = "validation.scale_consistency.report.v1";
pub const SCALE_CONSISTENCY_REPORT_ORDERING_POLICY: &str =
    "preserve_decision_report_scenario_order_metric_order_and_canonical_drift_category_order.v1";
pub const SCALE_CONSISTENCY_REPORT_CLASSIFICATION_POLICY: &str = "group_failed_metric_mismatches_by_returns_volatility_and_event_derived_categories_and_rank_primary_sources_by_failed_metric_count_then_max_absolute_delta_then_category.v1";
pub const SCALE_CONSISTENCY_REPORT_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "validation.scale_consistency.report.output_schema_metadata.v1";
pub const SCALE_CONSISTENCY_REPORT_ENVIRONMENT_METADATA_POLICY: &str =
    "exclude_environment_specific_fields_from_scale_consistency_report_payloads.v1";

const SCALE_CONSISTENCY_REPORT_REQUIRED_REPORT_FIELDS: [&str; 13] = [
    "schema_version",
    "ordering_policy",
    "classification_policy",
    "output_schema_metadata",
    "decision_policy",
    "governance_scope_policy",
    "benchmark_input_policy",
    "threshold_policy_id",
    "threshold_policy_version",
    "engine_benchmark_input",
    "status",
    "summary",
    "scenario_reports",
];
const SCALE_CONSISTENCY_REPORT_REQUIRED_SUMMARY_FIELDS: [&str; 9] = [
    "total_scenario_count",
    "passed_scenario_count",
    "failed_scenario_count",
    "total_metric_count",
    "failed_metric_count",
    "returns_primary_drift_scenario_count",
    "volatility_primary_drift_scenario_count",
    "event_derived_primary_drift_scenario_count",
    "uncategorized_failed_scenario_count",
];
const SCALE_CONSISTENCY_REPORT_REQUIRED_SCENARIO_FIELDS: [&str; 12] = [
    "scenario_id",
    "parameter_set_id",
    "horizon_label",
    "coarse_interval_delta",
    "fine_interval_delta",
    "applied_interval_threshold_band_label",
    "status",
    "failed_metric_count",
    "primary_drift_category",
    "non_drift_failure_reason_codes",
    "metric_decisions",
    "drift_diagnostics",
];
const SCALE_CONSISTENCY_REPORT_REQUIRED_DRIFT_DIAGNOSTIC_FIELDS: [&str; 5] = [
    "drift_category",
    "failed_metric_count",
    "missing_metric_count",
    "max_absolute_delta",
    "metric_references",
];
const SCALE_CONSISTENCY_REPORT_REQUIRED_DRIFT_METRIC_REFERENCE_FIELDS: [&str; 3] =
    ["metric_id", "metric_variant", "threshold_id"];
const SCALE_CONSISTENCY_REPORT_REQUIRED_METRIC_DECISION_FIELDS: [&str; 7] = [
    "metric_id",
    "metric_category",
    "metric_computation_status",
    "delta_value",
    "absolute_delta",
    "status",
    "threshold_context",
];
const SCALE_CONSISTENCY_REPORT_REQUIRED_THRESHOLD_CONTEXT_FIELDS: [&str; 7] = [
    "policy_id",
    "policy_version",
    "interval_threshold_band_label",
    "threshold_id",
    "threshold_rationale",
    "max_absolute_delta",
    "absolute_delta_exceeds_threshold",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScaleConsistencyReportOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub environment_metadata_policy: &'static str,
    pub required_report_fields: Vec<&'static str>,
    pub required_summary_fields: Vec<&'static str>,
    pub required_scenario_fields: Vec<&'static str>,
    pub required_drift_diagnostic_fields: Vec<&'static str>,
    pub required_drift_metric_reference_fields: Vec<&'static str>,
    pub required_metric_decision_fields: Vec<&'static str>,
    pub required_threshold_context_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyDriftCategory {
    Returns,
    Volatility,
    EventDerived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScaleConsistencyDriftMetricReference {
    pub metric_id: String,
    pub metric_variant: Option<String>,
    pub threshold_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScaleConsistencyDriftDiagnostic {
    pub drift_category: ScaleConsistencyDriftCategory,
    pub failed_metric_count: usize,
    pub missing_metric_count: usize,
    pub max_absolute_delta: Option<f64>,
    pub metric_references: Vec<ScaleConsistencyDriftMetricReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScaleConsistencyReportScenario {
    pub scenario_id: String,
    pub parameter_set_id: String,
    pub horizon_label: String,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub applied_interval_threshold_band_label: String,
    pub status: RegressionCaseStatus,
    pub failed_metric_count: usize,
    pub primary_drift_category: Option<ScaleConsistencyDriftCategory>,
    pub non_drift_failure_reason_codes: Vec<ScaleConsistencyDecisionReasonCode>,
    pub metric_decisions: Vec<ScaleConsistencyMetricDecision>,
    pub drift_diagnostics: Vec<ScaleConsistencyDriftDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ScaleConsistencyReportSummary {
    pub total_scenario_count: usize,
    pub passed_scenario_count: usize,
    pub failed_scenario_count: usize,
    pub total_metric_count: usize,
    pub failed_metric_count: usize,
    pub returns_primary_drift_scenario_count: usize,
    pub volatility_primary_drift_scenario_count: usize,
    pub event_derived_primary_drift_scenario_count: usize,
    pub uncategorized_failed_scenario_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScaleConsistencyReport {
    pub schema_version: &'static str,
    pub ordering_policy: &'static str,
    pub classification_policy: &'static str,
    pub output_schema_metadata: ScaleConsistencyReportOutputSchemaMetadata,
    pub decision_policy: &'static str,
    pub governance_scope_policy: &'static str,
    pub benchmark_input_policy: &'static str,
    pub threshold_policy_id: String,
    pub threshold_policy_version: String,
    pub engine_benchmark_input: super::ScaleConsistencyEngineBenchmarkInput,
    pub status: RegressionCaseStatus,
    pub summary: ScaleConsistencyReportSummary,
    pub scenario_reports: Vec<ScaleConsistencyReportScenario>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScaleConsistencyReportBuildError {
    InvalidDecisionReportSchemaVersion {
        expected: &'static str,
        actual: &'static str,
    },
    InvalidDecisionReportOrderingPolicy {
        expected: &'static str,
        actual: &'static str,
    },
}

impl std::fmt::Display for ScaleConsistencyReportBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDecisionReportSchemaVersion { expected, actual } => write!(
                formatter,
                "decision_report.schema_version must be `{expected}`, found `{actual}`"
            ),
            Self::InvalidDecisionReportOrderingPolicy { expected, actual } => write!(
                formatter,
                "decision_report.ordering_policy must be `{expected}`, found `{actual}`"
            ),
        }
    }
}

impl std::error::Error for ScaleConsistencyReportBuildError {}

pub fn build_scale_consistency_report(
    decision_report: &ScaleConsistencyDecisionBatchReport,
) -> Result<ScaleConsistencyReport, ScaleConsistencyReportBuildError> {
    if decision_report.schema_version != SCALE_CONSISTENCY_DECISION_REPORT_SCHEMA_VERSION {
        return Err(
            ScaleConsistencyReportBuildError::InvalidDecisionReportSchemaVersion {
                expected: SCALE_CONSISTENCY_DECISION_REPORT_SCHEMA_VERSION,
                actual: decision_report.schema_version,
            },
        );
    }

    if decision_report.ordering_policy != SCALE_CONSISTENCY_DECISION_ORDERING_POLICY {
        return Err(
            ScaleConsistencyReportBuildError::InvalidDecisionReportOrderingPolicy {
                expected: SCALE_CONSISTENCY_DECISION_ORDERING_POLICY,
                actual: decision_report.ordering_policy,
            },
        );
    }

    let scenario_reports = decision_report
        .scenario_reports
        .iter()
        .map(build_report_scenario)
        .collect::<Vec<_>>();

    let total_scenario_count = scenario_reports.len();
    let passed_scenario_count = scenario_reports
        .iter()
        .filter(|scenario_report| scenario_report.status == RegressionCaseStatus::Pass)
        .count();
    let failed_scenario_count = total_scenario_count - passed_scenario_count;
    let total_metric_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.metric_decisions.len())
        .sum();
    let failed_metric_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.failed_metric_count)
        .sum();
    let returns_primary_drift_scenario_count = scenario_reports
        .iter()
        .filter(|scenario_report| {
            scenario_report.primary_drift_category == Some(ScaleConsistencyDriftCategory::Returns)
        })
        .count();
    let volatility_primary_drift_scenario_count = scenario_reports
        .iter()
        .filter(|scenario_report| {
            scenario_report.primary_drift_category
                == Some(ScaleConsistencyDriftCategory::Volatility)
        })
        .count();
    let event_derived_primary_drift_scenario_count = scenario_reports
        .iter()
        .filter(|scenario_report| {
            scenario_report.primary_drift_category
                == Some(ScaleConsistencyDriftCategory::EventDerived)
        })
        .count();
    let uncategorized_failed_scenario_count = scenario_reports
        .iter()
        .filter(|scenario_report| {
            scenario_report.status == RegressionCaseStatus::Fail
                && scenario_report.primary_drift_category.is_none()
        })
        .count();
    let status = if failed_scenario_count == 0 {
        RegressionCaseStatus::Pass
    } else {
        RegressionCaseStatus::Fail
    };

    Ok(ScaleConsistencyReport {
        schema_version: SCALE_CONSISTENCY_REPORT_SCHEMA_VERSION,
        ordering_policy: SCALE_CONSISTENCY_REPORT_ORDERING_POLICY,
        classification_policy: SCALE_CONSISTENCY_REPORT_CLASSIFICATION_POLICY,
        output_schema_metadata: ScaleConsistencyReportOutputSchemaMetadata {
            schema_version: SCALE_CONSISTENCY_REPORT_OUTPUT_SCHEMA_METADATA_VERSION,
            environment_metadata_policy: SCALE_CONSISTENCY_REPORT_ENVIRONMENT_METADATA_POLICY,
            required_report_fields: SCALE_CONSISTENCY_REPORT_REQUIRED_REPORT_FIELDS.to_vec(),
            required_summary_fields: SCALE_CONSISTENCY_REPORT_REQUIRED_SUMMARY_FIELDS.to_vec(),
            required_scenario_fields: SCALE_CONSISTENCY_REPORT_REQUIRED_SCENARIO_FIELDS.to_vec(),
            required_drift_diagnostic_fields:
                SCALE_CONSISTENCY_REPORT_REQUIRED_DRIFT_DIAGNOSTIC_FIELDS.to_vec(),
            required_drift_metric_reference_fields:
                SCALE_CONSISTENCY_REPORT_REQUIRED_DRIFT_METRIC_REFERENCE_FIELDS.to_vec(),
            required_metric_decision_fields:
                SCALE_CONSISTENCY_REPORT_REQUIRED_METRIC_DECISION_FIELDS.to_vec(),
            required_threshold_context_fields:
                SCALE_CONSISTENCY_REPORT_REQUIRED_THRESHOLD_CONTEXT_FIELDS.to_vec(),
        },
        decision_policy: decision_report.decision_policy,
        governance_scope_policy: decision_report.governance_scope_policy,
        benchmark_input_policy: decision_report.benchmark_input_policy,
        threshold_policy_id: decision_report.threshold_policy_id.clone(),
        threshold_policy_version: decision_report.threshold_policy_version.clone(),
        engine_benchmark_input: decision_report.engine_benchmark_input.clone(),
        status,
        summary: ScaleConsistencyReportSummary {
            total_scenario_count,
            passed_scenario_count,
            failed_scenario_count,
            total_metric_count,
            failed_metric_count,
            returns_primary_drift_scenario_count,
            volatility_primary_drift_scenario_count,
            event_derived_primary_drift_scenario_count,
            uncategorized_failed_scenario_count,
        },
        scenario_reports,
    })
}

pub fn render_scale_consistency_report_summary(report: &ScaleConsistencyReport) -> String {
    let mut rendered_report = String::new();

    rendered_report.push_str("Scale Consistency Report\n");
    rendered_report.push_str(&format!("schema_version: {}\n", report.schema_version));
    rendered_report.push_str(&format!(
        "status: {}\n",
        regression_case_status_label(report.status)
    ));
    rendered_report.push_str(&format!(
        "summary: total_scenarios={} passed={} failed={} total_metrics={} failed_metrics={} returns_primary_drift={} volatility_primary_drift={} event_derived_primary_drift={} uncategorized_failed={}\n",
        report.summary.total_scenario_count,
        report.summary.passed_scenario_count,
        report.summary.failed_scenario_count,
        report.summary.total_metric_count,
        report.summary.failed_metric_count,
        report.summary.returns_primary_drift_scenario_count,
        report.summary.volatility_primary_drift_scenario_count,
        report.summary.event_derived_primary_drift_scenario_count,
        report.summary.uncategorized_failed_scenario_count,
    ));

    for scenario_report in &report.scenario_reports {
        rendered_report.push_str(&format!(
            "- scenario_id={} status={} failed_metrics={} primary_drift_category={} interval_band={}\n",
            scenario_report.scenario_id,
            regression_case_status_label(scenario_report.status),
            scenario_report.failed_metric_count,
            scenario_report
                .primary_drift_category
                .map(scale_consistency_drift_category_label)
                .unwrap_or("none"),
            scenario_report.applied_interval_threshold_band_label,
        ));

        if !scenario_report.non_drift_failure_reason_codes.is_empty() {
            rendered_report.push_str(&format!(
                "  non_drift_failure_reason_codes={}\n",
                format_decision_reason_codes(&scenario_report.non_drift_failure_reason_codes),
            ));
        }

        if scenario_report.drift_diagnostics.is_empty() {
            rendered_report.push_str("  drift_diagnostics=none\n");
            continue;
        }

        for drift_diagnostic in &scenario_report.drift_diagnostics {
            rendered_report.push_str(&format!(
                "  drift_category={} failed_metrics={} missing_metrics={} max_absolute_delta={:?} metrics={}\n",
                scale_consistency_drift_category_label(drift_diagnostic.drift_category),
                drift_diagnostic.failed_metric_count,
                drift_diagnostic.missing_metric_count,
                drift_diagnostic.max_absolute_delta,
                format_drift_metric_references(&drift_diagnostic.metric_references),
            ));
        }
    }

    rendered_report
}

fn build_report_scenario(
    scenario_report: &ScaleConsistencyDecisionScenarioReport,
) -> ScaleConsistencyReportScenario {
    let drift_diagnostics = build_drift_diagnostics(&scenario_report.metric_decisions);
    let primary_drift_category = select_primary_drift_category(&drift_diagnostics);
    let non_drift_failure_reason_codes =
        collect_non_drift_failure_reason_codes(&scenario_report.metric_decisions);

    ScaleConsistencyReportScenario {
        scenario_id: scenario_report.scenario_id.clone(),
        parameter_set_id: scenario_report.parameter_set_id.clone(),
        horizon_label: scenario_report.horizon_label.clone(),
        coarse_interval_delta: scenario_report.coarse_interval_delta,
        fine_interval_delta: scenario_report.fine_interval_delta,
        applied_interval_threshold_band_label: scenario_report
            .applied_interval_threshold_band_label
            .clone(),
        status: scenario_report.status,
        failed_metric_count: scenario_report.failed_metric_count,
        primary_drift_category,
        non_drift_failure_reason_codes,
        metric_decisions: scenario_report.metric_decisions.clone(),
        drift_diagnostics,
    }
}

fn build_drift_diagnostics(
    metric_decisions: &[ScaleConsistencyMetricDecision],
) -> Vec<ScaleConsistencyDriftDiagnostic> {
    [
        ScaleConsistencyDriftCategory::Returns,
        ScaleConsistencyDriftCategory::Volatility,
        ScaleConsistencyDriftCategory::EventDerived,
    ]
    .into_iter()
    .filter_map(|drift_category| build_drift_diagnostic(metric_decisions, drift_category))
    .collect()
}

fn build_drift_diagnostic(
    metric_decisions: &[ScaleConsistencyMetricDecision],
    drift_category: ScaleConsistencyDriftCategory,
) -> Option<ScaleConsistencyDriftDiagnostic> {
    let relevant_metric_decisions = metric_decisions
        .iter()
        .filter(|metric_decision| {
            metric_decision.status == RegressionCaseStatus::Fail
                && map_metric_category_to_drift_category(metric_decision.metric_category)
                    == drift_category
                && metric_decision
                    .decision_reason_codes
                    .iter()
                    .copied()
                    .any(is_drift_reason_code)
        })
        .collect::<Vec<_>>();

    if relevant_metric_decisions.is_empty() {
        return None;
    }

    let failed_metric_count = relevant_metric_decisions.len();
    let missing_metric_count = relevant_metric_decisions
        .iter()
        .filter(|metric_decision| {
            metric_decision.metric_computation_status
                == ScaleConsistencyMetricComputationStatus::Missing
                || metric_decision
                    .decision_reason_codes
                    .contains(&ScaleConsistencyDecisionReasonCode::MetricComputationMissing)
        })
        .count();
    let max_absolute_delta = relevant_metric_decisions
        .iter()
        .filter_map(|metric_decision| metric_decision.absolute_delta)
        .max_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let metric_references = relevant_metric_decisions
        .iter()
        .map(|metric_decision| ScaleConsistencyDriftMetricReference {
            metric_id: metric_decision.metric_id.clone(),
            metric_variant: metric_decision.metric_variant.clone(),
            threshold_id: metric_decision.threshold_context.threshold_id.clone(),
        })
        .collect();

    Some(ScaleConsistencyDriftDiagnostic {
        drift_category,
        failed_metric_count,
        missing_metric_count,
        max_absolute_delta,
        metric_references,
    })
}

fn select_primary_drift_category(
    drift_diagnostics: &[ScaleConsistencyDriftDiagnostic],
) -> Option<ScaleConsistencyDriftCategory> {
    drift_diagnostics
        .iter()
        .max_by(|left, right| compare_drift_diagnostics(left, right))
        .map(|drift_diagnostic| drift_diagnostic.drift_category)
}

fn compare_drift_diagnostics(
    left: &ScaleConsistencyDriftDiagnostic,
    right: &ScaleConsistencyDriftDiagnostic,
) -> Ordering {
    left.failed_metric_count
        .cmp(&right.failed_metric_count)
        .then_with(|| compare_optional_f64(left.max_absolute_delta, right.max_absolute_delta))
        .then_with(|| {
            drift_category_rank(right.drift_category).cmp(&drift_category_rank(left.drift_category))
        })
}

fn compare_optional_f64(left: Option<f64>, right: Option<f64>) -> Ordering {
    match (left, right) {
        (Some(left_value), Some(right_value)) => left_value
            .partial_cmp(&right_value)
            .unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

fn collect_non_drift_failure_reason_codes(
    metric_decisions: &[ScaleConsistencyMetricDecision],
) -> Vec<ScaleConsistencyDecisionReasonCode> {
    let mut non_drift_failure_reason_codes: Vec<ScaleConsistencyDecisionReasonCode> = Vec::new();

    for metric_decision in metric_decisions {
        if metric_decision.status != RegressionCaseStatus::Fail {
            continue;
        }

        for decision_reason_code in &metric_decision.decision_reason_codes {
            if is_drift_reason_code(*decision_reason_code) {
                continue;
            }
            if !non_drift_failure_reason_codes.contains(decision_reason_code) {
                non_drift_failure_reason_codes.push(*decision_reason_code);
            }
        }
    }

    non_drift_failure_reason_codes
}

const fn is_drift_reason_code(decision_reason_code: ScaleConsistencyDecisionReasonCode) -> bool {
    matches!(
        decision_reason_code,
        ScaleConsistencyDecisionReasonCode::AbsoluteDeltaThresholdExceeded
            | ScaleConsistencyDecisionReasonCode::MetricComputationMissing
    )
}

const fn map_metric_category_to_drift_category(
    metric_category: ScaleConsistencyMetricCategory,
) -> ScaleConsistencyDriftCategory {
    match metric_category {
        ScaleConsistencyMetricCategory::Returns => ScaleConsistencyDriftCategory::Returns,
        ScaleConsistencyMetricCategory::Volatility => ScaleConsistencyDriftCategory::Volatility,
        ScaleConsistencyMetricCategory::Events => ScaleConsistencyDriftCategory::EventDerived,
    }
}

const fn drift_category_rank(drift_category: ScaleConsistencyDriftCategory) -> u8 {
    match drift_category {
        ScaleConsistencyDriftCategory::Returns => 0,
        ScaleConsistencyDriftCategory::Volatility => 1,
        ScaleConsistencyDriftCategory::EventDerived => 2,
    }
}

const fn regression_case_status_label(status: RegressionCaseStatus) -> &'static str {
    match status {
        RegressionCaseStatus::Pass => "pass",
        RegressionCaseStatus::Fail => "fail",
    }
}

const fn scale_consistency_drift_category_label(
    drift_category: ScaleConsistencyDriftCategory,
) -> &'static str {
    match drift_category {
        ScaleConsistencyDriftCategory::Returns => "returns",
        ScaleConsistencyDriftCategory::Volatility => "volatility",
        ScaleConsistencyDriftCategory::EventDerived => "event_derived",
    }
}

fn format_decision_reason_codes(
    decision_reason_codes: &[ScaleConsistencyDecisionReasonCode],
) -> String {
    decision_reason_codes
        .iter()
        .map(|decision_reason_code| match decision_reason_code {
            ScaleConsistencyDecisionReasonCode::AbsoluteDeltaThresholdExceeded => {
                "absolute_delta_threshold_exceeded"
            }
            ScaleConsistencyDecisionReasonCode::MetricComputationMissing => {
                "metric_computation_missing"
            }
            ScaleConsistencyDecisionReasonCode::EngineBenchmarkPrerequisiteFailed => {
                "engine_benchmark_prerequisite_failed"
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn format_drift_metric_references(
    metric_references: &[ScaleConsistencyDriftMetricReference],
) -> String {
    metric_references
        .iter()
        .map(|metric_reference| {
            if let Some(metric_variant) = &metric_reference.metric_variant {
                format!(
                    "{}[{}]@{}",
                    metric_reference.metric_id, metric_variant, metric_reference.threshold_id
                )
            } else {
                format!(
                    "{}@{}",
                    metric_reference.metric_id, metric_reference.threshold_id
                )
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}
