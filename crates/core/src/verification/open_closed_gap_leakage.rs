//! Open/closed gap leakage regression harness and diff-report artifacts.

use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::calendar::{ExchangeCalendarRegistry, MarketState};
use crate::news::{
    SharedNewsAccumulatorState, SharedNewsEventTimestampedInput,
    dispatch_shared_news_events_with_resolved_market_states,
};

use super::contracts::RegressionCaseStatus;

pub const OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION: &str =
    "validation.open_closed_gap_leakage_regression_harness.report.v1";
pub const OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY: &str =
    "stable_input_order_and_event_timestamp_then_generation_order.v1";
pub const OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "validation.open_closed_gap_leakage_regression_harness.output_schema_metadata.v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION: &str =
    "validation.open_closed_gap_behavior.summary_artifact.v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION: &str =
    "validation.open_closed_gap_behavior.diff_report.v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY: &str =
    "market_state_gated_expected_contribution_baseline.v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_DETERMINISM_POLICY: &str =
    "preserve_input_case_order.v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_DIFF_DETERMINISM_POLICY: &str =
    "preserve_input_case_order_and_metric_order.v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_ENVIRONMENT_METADATA_POLICY: &str =
    "exclude_environment_specific_metadata.v1";

#[derive(Debug, Clone, PartialEq)]
pub struct OpenClosedGapLeakageRegressionEventInput {
    pub event_id: String,
    pub event_timestamp_utc: DateTime<Utc>,
    pub shared_severity_mark: f64,
    pub coupled_gap_jump_mark: f64,
    pub observed_direct_gap_increment_override: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenClosedGapLeakageRegressionCase {
    pub fixture_id: String,
    pub exchange_code: String,
    pub initial_j_t: f64,
    pub initial_g_t: f64,
    pub injected_events: Vec<OpenClosedGapLeakageRegressionEventInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapLeakageRegressionHarnessToleranceConfig {
    pub direct_gap_absolute_tolerance: f64,
}

impl Default for OpenClosedGapLeakageRegressionHarnessToleranceConfig {
    fn default() -> Self {
        Self {
            direct_gap_absolute_tolerance: 1e-12,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum OpenClosedGapLeakageRegressionFailure {
    DispatchError {
        error_message: String,
    },
    OpenWindowDirectGapLeakage {
        event_id: String,
        event_timestamp_utc: String,
        observed_direct_gap_increment: f64,
        absolute_leakage_magnitude: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapLeakageRegressionCaseReport {
    pub fixture_id: String,
    pub exchange_code: String,
    pub status: RegressionCaseStatus,
    pub summary: String,
    pub open_window_event_count: usize,
    pub leakage_failure_count: usize,
    pub max_absolute_leakage_magnitude: f64,
    pub contribution_summary: OpenClosedGapBehaviorContributionSummary,
    pub behavior_diff: OpenClosedGapBehaviorScenarioDiff,
    pub failures: Vec<OpenClosedGapLeakageRegressionFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenClosedGapLeakageRegressionHarnessSummary {
    pub total_case_count: usize,
    pub passed_case_count: usize,
    pub failed_case_count: usize,
    pub total_open_window_event_count: usize,
    pub total_leakage_failure_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapLeakageRegressionOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub summary_artifact_schema_version: &'static str,
    pub behavior_diff_report_schema_version: &'static str,
    pub baseline_expected_output_policy: &'static str,
    pub environment_metadata_policy: &'static str,
    pub required_case_report_fields: Vec<&'static str>,
    pub required_summary_artifact_fields: Vec<&'static str>,
    pub required_behavior_diff_report_fields: Vec<&'static str>,
    pub required_behavior_diff_metric_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapBehaviorContributionSummary {
    pub open_window_event_count: usize,
    pub closed_window_event_count: usize,
    pub expected_activity_coupling_j_t_increment_total: f64,
    pub observed_activity_coupling_j_t_increment_total: f64,
    pub expected_open_window_direct_gap_g_t_increment_total: f64,
    pub observed_open_window_direct_gap_g_t_increment_total: f64,
    pub expected_closed_window_direct_gap_g_t_increment_total: f64,
    pub observed_closed_window_direct_gap_g_t_increment_total: f64,
    pub expected_total_direct_gap_g_t_increment: f64,
    pub observed_total_direct_gap_g_t_increment: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapBehaviorDiffMetric {
    pub metric_id: &'static str,
    pub expected_value: f64,
    pub observed_value: f64,
    pub absolute_difference: f64,
    pub absolute_tolerance: f64,
    pub regression_detected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapBehaviorScenarioDiff {
    pub fixture_id: String,
    pub exchange_code: String,
    pub regression_metric_count: usize,
    pub max_absolute_difference: f64,
    pub metrics: Vec<OpenClosedGapBehaviorDiffMetric>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapBehaviorScenarioSummaryArtifact {
    pub fixture_id: String,
    pub exchange_code: String,
    pub contributions: OpenClosedGapBehaviorContributionSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapBehaviorSummaryArtifact {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub total_case_count: usize,
    pub total_open_window_event_count: usize,
    pub total_closed_window_event_count: usize,
    pub scenarios: Vec<OpenClosedGapBehaviorScenarioSummaryArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenClosedGapBehaviorDiffReportSummary {
    pub total_case_count: usize,
    pub regression_case_count: usize,
    pub total_regression_metric_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapBehaviorDiffReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub baseline_expected_output_policy: &'static str,
    pub summary: OpenClosedGapBehaviorDiffReportSummary,
    pub scenario_diffs: Vec<OpenClosedGapBehaviorScenarioDiff>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapLeakageRegressionHarnessReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub output_schema_metadata: OpenClosedGapLeakageRegressionOutputSchemaMetadata,
    pub tolerance_config: OpenClosedGapLeakageRegressionHarnessToleranceConfig,
    pub summary: OpenClosedGapLeakageRegressionHarnessSummary,
    pub summary_artifact: OpenClosedGapBehaviorSummaryArtifact,
    pub behavior_diff_report: OpenClosedGapBehaviorDiffReport,
    pub case_reports: Vec<OpenClosedGapLeakageRegressionCaseReport>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpenClosedGapLeakageRegressionHarnessError {
    InvalidDirectGapAbsoluteTolerance { value: f64 },
}

impl Display for OpenClosedGapLeakageRegressionHarnessError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDirectGapAbsoluteTolerance { value } => write!(
                formatter,
                "direct_gap_absolute_tolerance must be a finite non-negative value, found {value}"
            ),
        }
    }
}

impl Error for OpenClosedGapLeakageRegressionHarnessError {}

pub fn run_open_closed_gap_leakage_regression_harness(
    calendar_registry: &ExchangeCalendarRegistry,
    gap_behavior_cases: &[OpenClosedGapLeakageRegressionCase],
    tolerance_config: OpenClosedGapLeakageRegressionHarnessToleranceConfig,
) -> Result<OpenClosedGapLeakageRegressionHarnessReport, OpenClosedGapLeakageRegressionHarnessError>
{
    validate_gap_leakage_tolerance_config(&tolerance_config)?;

    let case_reports: Vec<OpenClosedGapLeakageRegressionCaseReport> = gap_behavior_cases
        .iter()
        .map(|gap_behavior_case| {
            evaluate_open_closed_gap_leakage_case(
                calendar_registry,
                gap_behavior_case,
                tolerance_config.direct_gap_absolute_tolerance,
            )
        })
        .collect();

    let passed_case_count = case_reports
        .iter()
        .filter(|case_report| case_report.status == RegressionCaseStatus::Pass)
        .count();
    let total_case_count = case_reports.len();
    let failed_case_count = total_case_count - passed_case_count;
    let total_open_window_event_count = case_reports
        .iter()
        .map(|case_report| case_report.open_window_event_count)
        .sum();
    let total_closed_window_event_count = case_reports
        .iter()
        .map(|case_report| case_report.contribution_summary.closed_window_event_count)
        .sum();
    let total_leakage_failure_count = case_reports
        .iter()
        .map(|case_report| case_report.leakage_failure_count)
        .sum();
    let summary_artifact_scenarios: Vec<OpenClosedGapBehaviorScenarioSummaryArtifact> =
        case_reports
            .iter()
            .map(|case_report| OpenClosedGapBehaviorScenarioSummaryArtifact {
                fixture_id: case_report.fixture_id.clone(),
                exchange_code: case_report.exchange_code.clone(),
                contributions: case_report.contribution_summary.clone(),
            })
            .collect();
    let scenario_diffs: Vec<OpenClosedGapBehaviorScenarioDiff> = case_reports
        .iter()
        .map(|case_report| case_report.behavior_diff.clone())
        .collect();
    let regression_case_count = scenario_diffs
        .iter()
        .filter(|scenario_diff| scenario_diff.regression_metric_count > 0)
        .count();
    let total_regression_metric_count = scenario_diffs
        .iter()
        .map(|scenario_diff| scenario_diff.regression_metric_count)
        .sum();

    Ok(OpenClosedGapLeakageRegressionHarnessReport {
        schema_version: OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
        determinism_policy: OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY,
        output_schema_metadata: OpenClosedGapLeakageRegressionOutputSchemaMetadata {
            schema_version: OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
            summary_artifact_schema_version:
                OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION,
            behavior_diff_report_schema_version:
                OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION,
            baseline_expected_output_policy:
                OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY,
            environment_metadata_policy: OPEN_CLOSED_GAP_BEHAVIOR_ENVIRONMENT_METADATA_POLICY,
            required_case_report_fields: vec![
                "fixture_id",
                "exchange_code",
                "status",
                "summary",
                "open_window_event_count",
                "leakage_failure_count",
                "max_absolute_leakage_magnitude",
                "contribution_summary",
                "behavior_diff",
                "failures",
            ],
            required_summary_artifact_fields: vec![
                "schema_version",
                "determinism_policy",
                "total_case_count",
                "total_open_window_event_count",
                "total_closed_window_event_count",
                "scenarios",
            ],
            required_behavior_diff_report_fields: vec![
                "schema_version",
                "determinism_policy",
                "baseline_expected_output_policy",
                "summary",
                "scenario_diffs",
            ],
            required_behavior_diff_metric_fields: vec![
                "metric_id",
                "expected_value",
                "observed_value",
                "absolute_difference",
                "absolute_tolerance",
                "regression_detected",
            ],
        },
        tolerance_config,
        summary: OpenClosedGapLeakageRegressionHarnessSummary {
            total_case_count,
            passed_case_count,
            failed_case_count,
            total_open_window_event_count,
            total_leakage_failure_count,
        },
        summary_artifact: OpenClosedGapBehaviorSummaryArtifact {
            schema_version: OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION,
            determinism_policy: OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_DETERMINISM_POLICY,
            total_case_count,
            total_open_window_event_count,
            total_closed_window_event_count,
            scenarios: summary_artifact_scenarios,
        },
        behavior_diff_report: OpenClosedGapBehaviorDiffReport {
            schema_version: OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION,
            determinism_policy: OPEN_CLOSED_GAP_BEHAVIOR_DIFF_DETERMINISM_POLICY,
            baseline_expected_output_policy:
                OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY,
            summary: OpenClosedGapBehaviorDiffReportSummary {
                total_case_count,
                regression_case_count,
                total_regression_metric_count,
            },
            scenario_diffs,
        },
        case_reports,
    })
}
fn validate_gap_leakage_tolerance_config(
    tolerance_config: &OpenClosedGapLeakageRegressionHarnessToleranceConfig,
) -> Result<(), OpenClosedGapLeakageRegressionHarnessError> {
    if !tolerance_config.direct_gap_absolute_tolerance.is_finite()
        || tolerance_config.direct_gap_absolute_tolerance < 0.0
    {
        return Err(
            OpenClosedGapLeakageRegressionHarnessError::InvalidDirectGapAbsoluteTolerance {
                value: tolerance_config.direct_gap_absolute_tolerance,
            },
        );
    }

    Ok(())
}
#[derive(Debug, Clone, PartialEq)]
struct OpenClosedGapLeakageEventMetadata {
    event_id: String,
    event_timestamp_utc: String,
    observed_direct_gap_increment_override: Option<f64>,
}

fn evaluate_open_closed_gap_leakage_case(
    calendar_registry: &ExchangeCalendarRegistry,
    gap_behavior_case: &OpenClosedGapLeakageRegressionCase,
    direct_gap_absolute_tolerance: f64,
) -> OpenClosedGapLeakageRegressionCaseReport {
    let mut event_inputs: Vec<SharedNewsEventTimestampedInput> =
        Vec::with_capacity(gap_behavior_case.injected_events.len());
    let mut event_metadata_by_generation_order: Vec<OpenClosedGapLeakageEventMetadata> =
        Vec::with_capacity(gap_behavior_case.injected_events.len());

    for (event_position, event_input) in gap_behavior_case.injected_events.iter().enumerate() {
        let event_generation_order =
            u64::try_from(event_position).expect("event position should fit in u64");

        if let Some(observed_direct_gap_increment_override) =
            event_input.observed_direct_gap_increment_override
        {
            if !observed_direct_gap_increment_override.is_finite() {
                return dispatch_error_case_report(
                    gap_behavior_case,
                    direct_gap_absolute_tolerance,
                    format!(
                        "event `{}` observed_direct_gap_increment_override must be finite when set, found {}",
                        event_input.event_id, observed_direct_gap_increment_override
                    ),
                );
            }
        }

        let event_timestamp_utc_nanos = match event_input.event_timestamp_utc.timestamp_nanos_opt()
        {
            Some(event_timestamp_utc_nanos) => event_timestamp_utc_nanos,
            None => {
                return dispatch_error_case_report(
                    gap_behavior_case,
                    direct_gap_absolute_tolerance,
                    format!(
                        "event `{}` timestamp `{}` is outside nanosecond range",
                        event_input.event_id,
                        event_input.event_timestamp_utc.to_rfc3339()
                    ),
                );
            }
        };

        event_inputs.push(SharedNewsEventTimestampedInput {
            event_timestamp_utc_nanos,
            event_generation_order,
            shared_severity_mark: event_input.shared_severity_mark,
            coupled_gap_jump_mark: event_input.coupled_gap_jump_mark,
        });
        event_metadata_by_generation_order.push(OpenClosedGapLeakageEventMetadata {
            event_id: event_input.event_id.clone(),
            event_timestamp_utc: event_input.event_timestamp_utc.to_rfc3339(),
            observed_direct_gap_increment_override: event_input
                .observed_direct_gap_increment_override,
        });
    }

    let dispatch_output = dispatch_shared_news_events_with_resolved_market_states(
        calendar_registry,
        &gap_behavior_case.exchange_code,
        SharedNewsAccumulatorState::new(
            gap_behavior_case.initial_j_t,
            gap_behavior_case.initial_g_t,
        ),
        &event_inputs,
    );
    let dispatch_output = match dispatch_output {
        Ok(dispatch_output) => dispatch_output,
        Err(source) => {
            return dispatch_error_case_report(
                gap_behavior_case,
                direct_gap_absolute_tolerance,
                source.to_string(),
            );
        }
    };

    let mut open_window_event_count = 0usize;
    let mut closed_window_event_count = 0usize;
    let mut max_absolute_leakage_magnitude = 0.0;
    let mut failures: Vec<OpenClosedGapLeakageRegressionFailure> = Vec::new();
    let mut expected_activity_coupling_j_t_increment_total = 0.0;
    let mut observed_activity_coupling_j_t_increment_total = 0.0;
    let mut expected_open_window_direct_gap_g_t_increment_total = 0.0;
    let mut observed_open_window_direct_gap_g_t_increment_total = 0.0;
    let mut expected_closed_window_direct_gap_g_t_increment_total = 0.0;
    let mut observed_closed_window_direct_gap_g_t_increment_total = 0.0;

    for dispatch_record in &dispatch_output.records {
        let event_metadata = usize::try_from(dispatch_record.event_generation_order)
            .ok()
            .and_then(|event_position| event_metadata_by_generation_order.get(event_position));
        let observed_direct_gap_increment = event_metadata
            .and_then(|event_metadata| event_metadata.observed_direct_gap_increment_override)
            .unwrap_or(dispatch_record.g_t_increment);
        let expected_direct_gap_increment = match dispatch_record.market_state {
            MarketState::Open => 0.0,
            MarketState::Closed => dispatch_record.coupled_gap_jump_mark,
        };

        expected_activity_coupling_j_t_increment_total += dispatch_record.shared_severity_mark;
        observed_activity_coupling_j_t_increment_total += dispatch_record.j_t_increment;

        match dispatch_record.market_state {
            MarketState::Open => {
                open_window_event_count += 1;
                expected_open_window_direct_gap_g_t_increment_total +=
                    expected_direct_gap_increment;
                observed_open_window_direct_gap_g_t_increment_total +=
                    observed_direct_gap_increment;
            }
            MarketState::Closed => {
                closed_window_event_count += 1;
                expected_closed_window_direct_gap_g_t_increment_total +=
                    expected_direct_gap_increment;
                observed_closed_window_direct_gap_g_t_increment_total +=
                    observed_direct_gap_increment;
                continue;
            }
        }

        let absolute_leakage_magnitude = observed_direct_gap_increment.abs();
        if absolute_leakage_magnitude <= direct_gap_absolute_tolerance {
            continue;
        }
        if absolute_leakage_magnitude > max_absolute_leakage_magnitude {
            max_absolute_leakage_magnitude = absolute_leakage_magnitude;
        }

        let (event_id, event_timestamp_utc) = match event_metadata {
            Some(event_metadata) => (
                event_metadata.event_id.clone(),
                event_metadata.event_timestamp_utc.clone(),
            ),
            None => (
                format!(
                    "event_generation_order_{}",
                    dispatch_record.event_generation_order
                ),
                String::new(),
            ),
        };

        failures.push(
            OpenClosedGapLeakageRegressionFailure::OpenWindowDirectGapLeakage {
                event_id,
                event_timestamp_utc,
                observed_direct_gap_increment,
                absolute_leakage_magnitude,
            },
        );
    }

    let contribution_summary = OpenClosedGapBehaviorContributionSummary {
        open_window_event_count,
        closed_window_event_count,
        expected_activity_coupling_j_t_increment_total,
        observed_activity_coupling_j_t_increment_total,
        expected_open_window_direct_gap_g_t_increment_total,
        observed_open_window_direct_gap_g_t_increment_total,
        expected_closed_window_direct_gap_g_t_increment_total,
        observed_closed_window_direct_gap_g_t_increment_total,
        expected_total_direct_gap_g_t_increment: expected_open_window_direct_gap_g_t_increment_total
            + expected_closed_window_direct_gap_g_t_increment_total,
        observed_total_direct_gap_g_t_increment: observed_open_window_direct_gap_g_t_increment_total
            + observed_closed_window_direct_gap_g_t_increment_total,
    };
    let behavior_diff = build_open_closed_behavior_scenario_diff(
        &gap_behavior_case.fixture_id,
        &gap_behavior_case.exchange_code,
        &contribution_summary,
        direct_gap_absolute_tolerance,
    );

    if failures.is_empty() {
        return OpenClosedGapLeakageRegressionCaseReport {
            fixture_id: gap_behavior_case.fixture_id.clone(),
            exchange_code: gap_behavior_case.exchange_code.clone(),
            status: RegressionCaseStatus::Pass,
            summary: format!(
                "no open-window direct-gap leakage detected across {open_window_event_count} open-window event(s)"
            ),
            open_window_event_count,
            leakage_failure_count: 0,
            max_absolute_leakage_magnitude: 0.0,
            contribution_summary,
            behavior_diff,
            failures,
        };
    }
    let leakage_failure_count = failures.len();

    OpenClosedGapLeakageRegressionCaseReport {
        fixture_id: gap_behavior_case.fixture_id.clone(),
        exchange_code: gap_behavior_case.exchange_code.clone(),
        status: RegressionCaseStatus::Fail,
        summary: format!(
            "detected {} open-window direct-gap leakage event(s), max absolute leakage magnitude={}",
            leakage_failure_count, max_absolute_leakage_magnitude
        ),
        open_window_event_count,
        leakage_failure_count,
        max_absolute_leakage_magnitude,
        contribution_summary,
        behavior_diff,
        failures,
    }
}

fn dispatch_error_case_report(
    gap_behavior_case: &OpenClosedGapLeakageRegressionCase,
    direct_gap_absolute_tolerance: f64,
    error_message: String,
) -> OpenClosedGapLeakageRegressionCaseReport {
    let contribution_summary = OpenClosedGapBehaviorContributionSummary {
        open_window_event_count: 0,
        closed_window_event_count: 0,
        expected_activity_coupling_j_t_increment_total: 0.0,
        observed_activity_coupling_j_t_increment_total: 0.0,
        expected_open_window_direct_gap_g_t_increment_total: 0.0,
        observed_open_window_direct_gap_g_t_increment_total: 0.0,
        expected_closed_window_direct_gap_g_t_increment_total: 0.0,
        observed_closed_window_direct_gap_g_t_increment_total: 0.0,
        expected_total_direct_gap_g_t_increment: 0.0,
        observed_total_direct_gap_g_t_increment: 0.0,
    };
    let behavior_diff = build_open_closed_behavior_scenario_diff(
        &gap_behavior_case.fixture_id,
        &gap_behavior_case.exchange_code,
        &contribution_summary,
        direct_gap_absolute_tolerance,
    );

    OpenClosedGapLeakageRegressionCaseReport {
        fixture_id: gap_behavior_case.fixture_id.clone(),
        exchange_code: gap_behavior_case.exchange_code.clone(),
        status: RegressionCaseStatus::Fail,
        summary: format!("failed to dispatch shared news events: {error_message}"),
        open_window_event_count: 0,
        leakage_failure_count: 0,
        max_absolute_leakage_magnitude: 0.0,
        contribution_summary,
        behavior_diff,
        failures: vec![OpenClosedGapLeakageRegressionFailure::DispatchError { error_message }],
    }
}

fn build_open_closed_behavior_scenario_diff(
    fixture_id: &str,
    exchange_code: &str,
    contribution_summary: &OpenClosedGapBehaviorContributionSummary,
    absolute_tolerance: f64,
) -> OpenClosedGapBehaviorScenarioDiff {
    let metrics = vec![
        build_open_closed_behavior_diff_metric(
            "activity_coupling_j_t_increment_total",
            contribution_summary.expected_activity_coupling_j_t_increment_total,
            contribution_summary.observed_activity_coupling_j_t_increment_total,
            absolute_tolerance,
        ),
        build_open_closed_behavior_diff_metric(
            "open_window_direct_gap_g_t_increment_total",
            contribution_summary.expected_open_window_direct_gap_g_t_increment_total,
            contribution_summary.observed_open_window_direct_gap_g_t_increment_total,
            absolute_tolerance,
        ),
        build_open_closed_behavior_diff_metric(
            "closed_window_direct_gap_g_t_increment_total",
            contribution_summary.expected_closed_window_direct_gap_g_t_increment_total,
            contribution_summary.observed_closed_window_direct_gap_g_t_increment_total,
            absolute_tolerance,
        ),
        build_open_closed_behavior_diff_metric(
            "total_direct_gap_g_t_increment",
            contribution_summary.expected_total_direct_gap_g_t_increment,
            contribution_summary.observed_total_direct_gap_g_t_increment,
            absolute_tolerance,
        ),
    ];

    let regression_metric_count = metrics
        .iter()
        .filter(|metric| metric.regression_detected)
        .count();
    let max_absolute_difference = metrics
        .iter()
        .map(|metric| metric.absolute_difference)
        .fold(0.0_f64, |current_max, absolute_difference| {
            if absolute_difference.is_finite() {
                current_max.max(absolute_difference)
            } else {
                f64::INFINITY
            }
        });

    OpenClosedGapBehaviorScenarioDiff {
        fixture_id: fixture_id.to_string(),
        exchange_code: exchange_code.to_string(),
        regression_metric_count,
        max_absolute_difference,
        metrics,
    }
}

fn build_open_closed_behavior_diff_metric(
    metric_id: &'static str,
    expected_value: f64,
    observed_value: f64,
    absolute_tolerance: f64,
) -> OpenClosedGapBehaviorDiffMetric {
    let absolute_difference = (expected_value - observed_value).abs();
    let regression_detected =
        !absolute_difference.is_finite() || absolute_difference > absolute_tolerance;

    OpenClosedGapBehaviorDiffMetric {
        metric_id,
        expected_value,
        observed_value,
        absolute_difference,
        absolute_tolerance,
        regression_detected,
    }
}
