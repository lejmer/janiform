//! Regression drift attribution report construction and rendering.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_governance_bench::benchmarking::{
    ORACLE_PRICER_RISK_COMPONENT_ORACLE, ORACLE_PRICER_RISK_COMPONENT_PRICING,
    ORACLE_PRICER_RISK_COMPONENT_RISK,
};
use janiform_validation::CoarseFineConsistencyRegressionReport;
use serde::{Deserialize, Serialize};

use super::performance_baseline::OraclePricerRiskPerformanceBaselineComparisonReport;

pub const REGRESSION_DRIFT_ATTRIBUTION_REPORT_SCHEMA_VERSION: &str =
    "benchmark.regression_drift_attribution.report.v1";
pub const REGRESSION_DRIFT_ATTRIBUTION_CLASSIFICATION_POLICY: &str = "explicit_rules_numerical_failed_metric_performance_relative_degradation_and_scenario_set_delta.v1";
pub const REGRESSION_DRIFT_ATTRIBUTION_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "benchmark.regression_drift_attribution.output_schema_metadata.v1";
pub const REGRESSION_DRIFT_ATTRIBUTION_PERFORMANCE_LATENCY_THRESHOLD_ID: &str =
    "benchmark.performance.max_latency_relative_increase.v1";
pub const REGRESSION_DRIFT_ATTRIBUTION_PERFORMANCE_THROUGHPUT_THRESHOLD_ID: &str =
    "benchmark.performance.max_throughput_relative_decrease.v1";
pub const REGRESSION_DRIFT_ATTRIBUTION_SCENARIO_SET_THRESHOLD_ID: &str =
    "benchmark.scenario_set.membership_change.v1";

const REGRESSION_DRIFT_ATTRIBUTION_REQUIRED_REPORT_FIELDS: [&str; 14] = [
    "schema_version",
    "classification_policy",
    "output_schema_metadata",
    "catalog_id",
    "catalog_version",
    "benchmark_policy_id",
    "benchmark_policy_version",
    "tolerance_policy_id",
    "tolerance_policy_version",
    "comparison_context",
    "performance_thresholds",
    "status",
    "summary",
    "entries",
];
const REGRESSION_DRIFT_ATTRIBUTION_REQUIRED_ENTRY_FIELDS: [&str; 10] = [
    "scenario_id",
    "component_id",
    "drift_category",
    "threshold_id",
    "metric_id",
    "reference_value",
    "candidate_value",
    "absolute_delta",
    "relative_delta",
    "triage_details",
];
const REGRESSION_DRIFT_ATTRIBUTION_REQUIRED_SUMMARY_FIELDS: [&str; 5] = [
    "total_entry_count",
    "numerical_drift_count",
    "performance_drift_count",
    "scenario_change_count",
    "failing_scenario_count",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionDriftAttributionStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionDriftCategory {
    NumericalDrift,
    PerformanceDrift,
    ScenarioChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegressionDriftAttributionOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub required_report_fields: Vec<&'static str>,
    pub required_entry_fields: Vec<&'static str>,
    pub required_summary_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegressionDriftAttributionComparisonContext {
    pub reference_catalog_id: Option<String>,
    pub reference_catalog_version: Option<String>,
    pub reference_performance_baseline_version: Option<String>,
    pub candidate_performance_baseline_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegressionDriftAttributionPerformanceThresholds {
    pub max_latency_relative_increase: f64,
    pub max_throughput_relative_decrease: f64,
}

impl Default for RegressionDriftAttributionPerformanceThresholds {
    fn default() -> Self {
        Self {
            max_latency_relative_increase: 0.10,
            max_throughput_relative_decrease: 0.10,
        }
    }
}

impl RegressionDriftAttributionPerformanceThresholds {
    pub fn validate(self) -> Result<(), RegressionDriftAttributionPerformanceThresholdsError> {
        let fields = [
            (
                "max_latency_relative_increase",
                self.max_latency_relative_increase,
            ),
            (
                "max_throughput_relative_decrease",
                self.max_throughput_relative_decrease,
            ),
        ];
        for (field_name, value) in fields {
            if !value.is_finite() || value < 0.0 {
                return Err(
                    RegressionDriftAttributionPerformanceThresholdsError::InvalidThresholdField {
                        field_name,
                        value,
                    },
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegressionDriftAttributionPerformanceThresholdsError {
    InvalidThresholdField {
        field_name: &'static str,
        value: f64,
    },
}

impl Display for RegressionDriftAttributionPerformanceThresholdsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidThresholdField { field_name, value } => write!(
                formatter,
                "{field_name} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for RegressionDriftAttributionPerformanceThresholdsError {}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegressionDriftAttributionEntry {
    pub scenario_id: String,
    pub component_id: String,
    pub drift_category: RegressionDriftCategory,
    pub threshold_id: String,
    pub metric_id: String,
    pub reference_value: Option<f64>,
    pub candidate_value: Option<f64>,
    pub absolute_delta: Option<f64>,
    pub relative_delta: Option<f64>,
    pub triage_details: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RegressionDriftAttributionSummary {
    pub total_entry_count: usize,
    pub numerical_drift_count: usize,
    pub performance_drift_count: usize,
    pub scenario_change_count: usize,
    pub failing_scenario_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegressionDriftAttributionReport {
    pub schema_version: &'static str,
    pub classification_policy: &'static str,
    pub output_schema_metadata: RegressionDriftAttributionOutputSchemaMetadata,
    pub catalog_id: String,
    pub catalog_version: String,
    pub benchmark_policy_id: String,
    pub benchmark_policy_version: String,
    pub tolerance_policy_id: String,
    pub tolerance_policy_version: String,
    pub comparison_context: RegressionDriftAttributionComparisonContext,
    pub performance_thresholds: RegressionDriftAttributionPerformanceThresholds,
    pub status: RegressionDriftAttributionStatus,
    pub summary: RegressionDriftAttributionSummary,
    pub entries: Vec<RegressionDriftAttributionEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegressionDriftAttributionBuildError {
    InvalidPerformanceThresholds {
        source: RegressionDriftAttributionPerformanceThresholdsError,
    },
}

impl Display for RegressionDriftAttributionBuildError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPerformanceThresholds { source } => write!(
                formatter,
                "regression drift attribution performance thresholds are invalid: {source}"
            ),
        }
    }
}

impl Error for RegressionDriftAttributionBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPerformanceThresholds { source } => Some(source),
        }
    }
}

pub fn build_regression_drift_attribution_report(
    current_report: &CoarseFineConsistencyRegressionReport,
    reference_report: Option<&CoarseFineConsistencyRegressionReport>,
    performance_comparison_report: Option<&OraclePricerRiskPerformanceBaselineComparisonReport>,
    performance_thresholds: RegressionDriftAttributionPerformanceThresholds,
) -> Result<RegressionDriftAttributionReport, RegressionDriftAttributionBuildError> {
    performance_thresholds.validate().map_err(|source| {
        RegressionDriftAttributionBuildError::InvalidPerformanceThresholds { source }
    })?;

    let mut entries: Vec<RegressionDriftAttributionEntry> = Vec::new();

    for scenario_report in &current_report.scenario_reports {
        for metric_delta in &scenario_report.metric_deltas {
            if metric_delta.passed {
                continue;
            }

            let component_id = map_component_id_from_coarse_fine_metric(metric_delta.metric_id);
            let threshold_id = format!(
                "{}/{}/{}",
                current_report.tolerance_config.policy_id,
                scenario_report.applied_tolerance_band_label,
                metric_delta.metric_id
            );
            let triage_details = format!(
                "coarse-fine metric failed: absolute_delta={} relative_delta={} absolute_tolerance={} relative_tolerance={}",
                metric_delta.absolute_delta,
                metric_delta.relative_delta,
                metric_delta.absolute_tolerance,
                metric_delta.relative_tolerance
            );

            entries.push(RegressionDriftAttributionEntry {
                scenario_id: scenario_report.scenario_id.clone(),
                component_id: component_id.to_string(),
                drift_category: RegressionDriftCategory::NumericalDrift,
                threshold_id,
                metric_id: metric_delta.metric_id.to_string(),
                reference_value: Some(metric_delta.fine_reference_value),
                candidate_value: Some(metric_delta.coarse_value),
                absolute_delta: Some(metric_delta.absolute_delta),
                relative_delta: Some(metric_delta.relative_delta),
                triage_details,
            });
        }
    }

    if let Some(performance_comparison_report) = performance_comparison_report {
        for component_entry in &performance_comparison_report.component_entries {
            if component_entry.mean_latency_relative_change
                > performance_thresholds.max_latency_relative_increase
            {
                let triage_details = format!(
                    "latency regression exceeded relative increase threshold: relative_change={} threshold={}",
                    component_entry.mean_latency_relative_change,
                    performance_thresholds.max_latency_relative_increase
                );

                entries.push(RegressionDriftAttributionEntry {
                    scenario_id: component_entry.profile_id.clone(),
                    component_id: component_entry.component_id.clone(),
                    drift_category: RegressionDriftCategory::PerformanceDrift,
                    threshold_id: REGRESSION_DRIFT_ATTRIBUTION_PERFORMANCE_LATENCY_THRESHOLD_ID
                        .to_string(),
                    metric_id: "mean_latency_seconds".to_string(),
                    reference_value: Some(component_entry.reference_mean_latency_seconds),
                    candidate_value: Some(component_entry.candidate_mean_latency_seconds),
                    absolute_delta: Some(component_entry.mean_latency_seconds_delta),
                    relative_delta: Some(component_entry.mean_latency_relative_change),
                    triage_details,
                });
            }

            let throughput_relative_decrease = -component_entry.throughput_relative_change;
            if throughput_relative_decrease
                > performance_thresholds.max_throughput_relative_decrease
            {
                let triage_details = format!(
                    "throughput regression exceeded relative decrease threshold: relative_decrease={} threshold={}",
                    throughput_relative_decrease,
                    performance_thresholds.max_throughput_relative_decrease
                );

                entries.push(RegressionDriftAttributionEntry {
                    scenario_id: component_entry.profile_id.clone(),
                    component_id: component_entry.component_id.clone(),
                    drift_category: RegressionDriftCategory::PerformanceDrift,
                    threshold_id: REGRESSION_DRIFT_ATTRIBUTION_PERFORMANCE_THROUGHPUT_THRESHOLD_ID
                        .to_string(),
                    metric_id: "throughput_operations_per_second".to_string(),
                    reference_value: Some(
                        component_entry.reference_throughput_operations_per_second,
                    ),
                    candidate_value: Some(
                        component_entry.candidate_throughput_operations_per_second,
                    ),
                    absolute_delta: Some(component_entry.throughput_operations_per_second_delta),
                    relative_delta: Some(component_entry.throughput_relative_change),
                    triage_details,
                });
            }
        }
    }

    if let Some(reference_report) = reference_report {
        let current_scenario_ids: BTreeSet<&str> = current_report
            .scenario_reports
            .iter()
            .map(|scenario_report| scenario_report.scenario_id.as_str())
            .collect();
        let reference_scenario_ids: BTreeSet<&str> = reference_report
            .scenario_reports
            .iter()
            .map(|scenario_report| scenario_report.scenario_id.as_str())
            .collect();

        for removed_scenario_id in reference_scenario_ids.difference(&current_scenario_ids) {
            entries.push(RegressionDriftAttributionEntry {
                scenario_id: (*removed_scenario_id).to_string(),
                component_id: "scenario_catalog".to_string(),
                drift_category: RegressionDriftCategory::ScenarioChange,
                threshold_id: REGRESSION_DRIFT_ATTRIBUTION_SCENARIO_SET_THRESHOLD_ID.to_string(),
                metric_id: "scenario_missing_from_candidate".to_string(),
                reference_value: None,
                candidate_value: None,
                absolute_delta: None,
                relative_delta: None,
                triage_details:
                    "scenario was present in reference run but missing from candidate run"
                        .to_string(),
            });
        }

        for added_scenario_id in current_scenario_ids.difference(&reference_scenario_ids) {
            entries.push(RegressionDriftAttributionEntry {
                scenario_id: (*added_scenario_id).to_string(),
                component_id: "scenario_catalog".to_string(),
                drift_category: RegressionDriftCategory::ScenarioChange,
                threshold_id: REGRESSION_DRIFT_ATTRIBUTION_SCENARIO_SET_THRESHOLD_ID.to_string(),
                metric_id: "scenario_added_in_candidate".to_string(),
                reference_value: None,
                candidate_value: None,
                absolute_delta: None,
                relative_delta: None,
                triage_details:
                    "scenario was not present in reference run but appears in candidate run"
                        .to_string(),
            });
        }
    }

    entries.sort_by(|left, right| {
        left.scenario_id
            .cmp(&right.scenario_id)
            .then_with(|| left.component_id.cmp(&right.component_id))
            .then_with(|| {
                regression_drift_category_rank(left.drift_category)
                    .cmp(&regression_drift_category_rank(right.drift_category))
            })
            .then_with(|| left.metric_id.cmp(&right.metric_id))
            .then_with(|| left.threshold_id.cmp(&right.threshold_id))
    });

    let numerical_drift_count = entries
        .iter()
        .filter(|entry| entry.drift_category == RegressionDriftCategory::NumericalDrift)
        .count();
    let performance_drift_count = entries
        .iter()
        .filter(|entry| entry.drift_category == RegressionDriftCategory::PerformanceDrift)
        .count();
    let scenario_change_count = entries
        .iter()
        .filter(|entry| entry.drift_category == RegressionDriftCategory::ScenarioChange)
        .count();
    let failing_scenario_count = entries
        .iter()
        .map(|entry| entry.scenario_id.as_str())
        .collect::<BTreeSet<&str>>()
        .len();
    let total_entry_count = entries.len();
    let status = if total_entry_count == 0 {
        RegressionDriftAttributionStatus::Pass
    } else {
        RegressionDriftAttributionStatus::Fail
    };

    let comparison_context = RegressionDriftAttributionComparisonContext {
        reference_catalog_id: reference_report.map(|report| report.catalog_id.clone()),
        reference_catalog_version: reference_report.map(|report| report.catalog_version.clone()),
        reference_performance_baseline_version: performance_comparison_report
            .map(|comparison_report| comparison_report.reference_baseline_version.clone()),
        candidate_performance_baseline_version: performance_comparison_report
            .map(|comparison_report| comparison_report.candidate_baseline_version.clone()),
    };

    Ok(RegressionDriftAttributionReport {
        schema_version: REGRESSION_DRIFT_ATTRIBUTION_REPORT_SCHEMA_VERSION,
        classification_policy: REGRESSION_DRIFT_ATTRIBUTION_CLASSIFICATION_POLICY,
        output_schema_metadata: RegressionDriftAttributionOutputSchemaMetadata {
            schema_version: REGRESSION_DRIFT_ATTRIBUTION_OUTPUT_SCHEMA_METADATA_VERSION,
            required_report_fields: REGRESSION_DRIFT_ATTRIBUTION_REQUIRED_REPORT_FIELDS.to_vec(),
            required_entry_fields: REGRESSION_DRIFT_ATTRIBUTION_REQUIRED_ENTRY_FIELDS.to_vec(),
            required_summary_fields: REGRESSION_DRIFT_ATTRIBUTION_REQUIRED_SUMMARY_FIELDS.to_vec(),
        },
        catalog_id: current_report.catalog_id.clone(),
        catalog_version: current_report.catalog_version.clone(),
        benchmark_policy_id: current_report.benchmark_policy_id.clone(),
        benchmark_policy_version: current_report.benchmark_policy_version.clone(),
        tolerance_policy_id: current_report.tolerance_config.policy_id.clone(),
        tolerance_policy_version: current_report.tolerance_config.policy_version.clone(),
        comparison_context,
        performance_thresholds,
        status,
        summary: RegressionDriftAttributionSummary {
            total_entry_count,
            numerical_drift_count,
            performance_drift_count,
            scenario_change_count,
            failing_scenario_count,
        },
        entries,
    })
}

pub fn render_regression_drift_attribution_report(
    report: &RegressionDriftAttributionReport,
) -> String {
    let mut rendered_report = String::new();

    rendered_report.push_str("Regression Drift Attribution Report\n");
    rendered_report.push_str(&format!("schema_version: {}\n", report.schema_version));
    rendered_report.push_str(&format!("status: {:?}\n", report.status));
    rendered_report.push_str(&format!(
        "summary: total_entries={} numerical_drift={} performance_drift={} scenario_change={} failing_scenarios={}\n",
        report.summary.total_entry_count,
        report.summary.numerical_drift_count,
        report.summary.performance_drift_count,
        report.summary.scenario_change_count,
        report.summary.failing_scenario_count
    ));

    if let Some(reference_baseline_version) = &report
        .comparison_context
        .reference_performance_baseline_version
    {
        rendered_report.push_str(&format!(
            "reference_performance_baseline_version: {reference_baseline_version}\n"
        ));
    }
    if let Some(candidate_baseline_version) = &report
        .comparison_context
        .candidate_performance_baseline_version
    {
        rendered_report.push_str(&format!(
            "candidate_performance_baseline_version: {candidate_baseline_version}\n"
        ));
    }

    for entry in &report.entries {
        rendered_report.push_str(&format!(
            "- scenario_id={} component_id={} category={:?} threshold_id={} metric_id={} absolute_delta={:?} relative_delta={:?} details={}\n",
            entry.scenario_id,
            entry.component_id,
            entry.drift_category,
            entry.threshold_id,
            entry.metric_id,
            entry.absolute_delta,
            entry.relative_delta,
            entry.triage_details
        ));
    }

    rendered_report
}

fn map_component_id_from_coarse_fine_metric(metric_id: &str) -> &'static str {
    if metric_id.starts_with("oracle_") {
        return ORACLE_PRICER_RISK_COMPONENT_ORACLE;
    }
    if metric_id.starts_with("pricing_") {
        return ORACLE_PRICER_RISK_COMPONENT_PRICING;
    }
    if metric_id.starts_with("risk_") {
        return ORACLE_PRICER_RISK_COMPONENT_RISK;
    }

    "coarse_fine_consistency_harness"
}

fn regression_drift_category_rank(category: RegressionDriftCategory) -> u8 {
    match category {
        RegressionDriftCategory::NumericalDrift => 0,
        RegressionDriftCategory::PerformanceDrift => 1,
        RegressionDriftCategory::ScenarioChange => 2,
    }
}
