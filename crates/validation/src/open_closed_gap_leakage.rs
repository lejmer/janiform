//! Open/closed news-routing regression harness and diff-report artifacts.

use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use janiform_analytics_stylized_facts::{
    NewsSemanticsScenarioClass, OpenClosedGapDeterministicReplayMetadata,
    OpenClosedGapIssueTraceabilityMetadata,
};
use janiform_market_time::calendar::{ExchangeCalendarRegistry, MarketState};
use janiform_model_kernels::news::{
    NewsArrivalComponent, SharedNewsAccumulatorState, SharedNewsEventTimestampedInput,
    dispatch_shared_news_events_with_resolved_market_states,
};
use serde::Serialize;

use super::contracts::{
    RegressionCaseStatus, VERIFICATION_CANONICAL_COMPONENT_VOCABULARY, VerificationSemanticBucket,
};

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
    pub news_arrival_component: NewsArrivalComponent,
    pub shared_severity_mark: f64,
    pub coupled_return_jump_mark: f64,
    pub observed_activity_coupling_j_t_increment_override: Option<f64>,
    pub observed_open_jump_m_t_increment_override: Option<f64>,
    pub observed_closed_gap_g_t_increment_override: Option<f64>,
    pub observed_trading_return_increment_override: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenClosedGapLeakageRegressionCase {
    pub fixture_id: String,
    pub exchange_code: String,
    pub scenario_class: NewsSemanticsScenarioClass,
    pub deterministic_replay_metadata: OpenClosedGapDeterministicReplayMetadata,
    pub issue_traceability: OpenClosedGapIssueTraceabilityMetadata,
    pub initial_j_t: f64,
    pub initial_m_t: f64,
    pub initial_g_t: f64,
    pub injected_events: Vec<OpenClosedGapLeakageRegressionEventInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapLeakageRegressionHarnessToleranceConfig {
    pub component_absolute_tolerance: f64,
}

impl Default for OpenClosedGapLeakageRegressionHarnessToleranceConfig {
    fn default() -> Self {
        Self {
            component_absolute_tolerance: 1e-12,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum OpenClosedGapLeakageRegressionFailure {
    DispatchError {
        error_message: String,
    },
    SemanticMismatch {
        semantic_bucket: VerificationSemanticBucket,
        mismatch_kind: &'static str,
        event_id: String,
        event_timestamp_utc: String,
        expected_value: f64,
        observed_value: f64,
        absolute_difference: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OpenClosedGapScenarioClassCoverage {
    pub known_only_case_count: usize,
    pub surprise_only_case_count: usize,
    pub mixed_news_case_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapLeakageRegressionCaseReport {
    pub fixture_id: String,
    pub exchange_code: String,
    pub scenario_class: NewsSemanticsScenarioClass,
    pub deterministic_replay_metadata: OpenClosedGapDeterministicReplayMetadata,
    pub issue_traceability: OpenClosedGapIssueTraceabilityMetadata,
    pub status: RegressionCaseStatus,
    pub summary: String,
    pub semantic_mismatch_failure_count: usize,
    pub max_absolute_mismatch: f64,
    pub contribution_summary: OpenClosedGapBehaviorContributionSummary,
    pub behavior_diff: OpenClosedGapBehaviorScenarioDiff,
    pub failures: Vec<OpenClosedGapLeakageRegressionFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenClosedGapLeakageRegressionHarnessSummary {
    pub total_case_count: usize,
    pub passed_case_count: usize,
    pub failed_case_count: usize,
    pub total_semantic_mismatch_failure_count: usize,
    pub scenario_class_coverage: OpenClosedGapScenarioClassCoverage,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapLeakageRegressionOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub summary_artifact_schema_version: &'static str,
    pub behavior_diff_report_schema_version: &'static str,
    pub baseline_expected_output_policy: &'static str,
    pub environment_metadata_policy: &'static str,
    pub canonical_component_vocabulary: Vec<&'static str>,
    pub canonical_semantic_bucket_vocabulary: Vec<&'static str>,
    pub required_case_report_fields: Vec<&'static str>,
    pub required_summary_artifact_fields: Vec<&'static str>,
    pub required_behavior_diff_report_fields: Vec<&'static str>,
    pub required_behavior_diff_metric_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapBehaviorContributionSummary {
    pub open_window_event_count: usize,
    pub closed_window_event_count: usize,
    pub expected_activity_coupling_total: f64,
    pub observed_activity_coupling_total: f64,
    pub expected_trading_return_total: f64,
    pub observed_trading_return_total: f64,
    pub expected_open_jump_total: f64,
    pub observed_open_jump_total: f64,
    pub expected_closed_gap_total: f64,
    pub observed_closed_gap_total: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapBehaviorDiffMetric {
    pub metric_id: &'static str,
    pub semantic_bucket: VerificationSemanticBucket,
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
    pub scenario_class: NewsSemanticsScenarioClass,
    pub deterministic_replay_metadata: OpenClosedGapDeterministicReplayMetadata,
    pub issue_traceability: OpenClosedGapIssueTraceabilityMetadata,
    pub contributions: OpenClosedGapBehaviorContributionSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenClosedGapBehaviorSummaryArtifact {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub total_case_count: usize,
    pub total_open_window_event_count: usize,
    pub total_closed_window_event_count: usize,
    pub scenario_class_coverage: OpenClosedGapScenarioClassCoverage,
    pub scenarios: Vec<OpenClosedGapBehaviorScenarioSummaryArtifact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OpenClosedGapBehaviorDiffReportSummary {
    pub total_case_count: usize,
    pub regression_case_count: usize,
    pub total_regression_metric_count: usize,
    pub trading_return_regression_metric_count: usize,
    pub open_jump_regression_metric_count: usize,
    pub closed_gap_regression_metric_count: usize,
    pub activity_coupling_regression_metric_count: usize,
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
    InvalidComponentAbsoluteTolerance {
        value: f64,
    },
    MissingScenarioClassCoverage {
        missing_scenario_classes: Vec<NewsSemanticsScenarioClass>,
    },
}

impl Display for OpenClosedGapLeakageRegressionHarnessError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidComponentAbsoluteTolerance { value } => write!(
                formatter,
                "component_absolute_tolerance must be a finite non-negative value, found {value}"
            ),
            Self::MissingScenarioClassCoverage {
                missing_scenario_classes,
            } => write!(
                formatter,
                "regression harness requires known_only, surprise_only, and mixed_news scenario coverage, missing {:?}",
                missing_scenario_classes
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
    validate_tolerance_config(&tolerance_config)?;

    let scenario_class_coverage = compute_scenario_class_coverage(gap_behavior_cases);
    validate_scenario_class_coverage(scenario_class_coverage)?;

    let case_reports: Vec<OpenClosedGapLeakageRegressionCaseReport> = gap_behavior_cases
        .iter()
        .map(|gap_behavior_case| {
            evaluate_open_closed_gap_leakage_case(
                calendar_registry,
                gap_behavior_case,
                tolerance_config.component_absolute_tolerance,
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
        .map(|case_report| case_report.contribution_summary.open_window_event_count)
        .sum();
    let total_closed_window_event_count = case_reports
        .iter()
        .map(|case_report| case_report.contribution_summary.closed_window_event_count)
        .sum();
    let total_semantic_mismatch_failure_count = case_reports
        .iter()
        .map(|case_report| case_report.semantic_mismatch_failure_count)
        .sum();
    let summary_artifact_scenarios: Vec<OpenClosedGapBehaviorScenarioSummaryArtifact> =
        case_reports
            .iter()
            .map(|case_report| OpenClosedGapBehaviorScenarioSummaryArtifact {
                fixture_id: case_report.fixture_id.clone(),
                exchange_code: case_report.exchange_code.clone(),
                scenario_class: case_report.scenario_class,
                deterministic_replay_metadata: case_report.deterministic_replay_metadata.clone(),
                issue_traceability: case_report.issue_traceability.clone(),
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
    let trading_return_regression_metric_count = count_regression_metrics_by_bucket(
        &scenario_diffs,
        VerificationSemanticBucket::TradingReturn,
    );
    let open_jump_regression_metric_count =
        count_regression_metrics_by_bucket(&scenario_diffs, VerificationSemanticBucket::OpenJump);
    let closed_gap_regression_metric_count =
        count_regression_metrics_by_bucket(&scenario_diffs, VerificationSemanticBucket::ClosedGap);
    let activity_coupling_regression_metric_count = count_regression_metrics_by_bucket(
        &scenario_diffs,
        VerificationSemanticBucket::ActivityCoupling,
    );

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
            canonical_component_vocabulary: VERIFICATION_CANONICAL_COMPONENT_VOCABULARY.to_vec(),
            canonical_semantic_bucket_vocabulary: VERIFICATION_CANONICAL_COMPONENT_VOCABULARY
                .to_vec(),
            required_case_report_fields: vec![
                "fixture_id",
                "exchange_code",
                "scenario_class",
                "deterministic_replay_metadata",
                "issue_traceability",
                "status",
                "summary",
                "semantic_mismatch_failure_count",
                "max_absolute_mismatch",
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
                "scenario_class_coverage",
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
                "semantic_bucket",
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
            total_semantic_mismatch_failure_count,
            scenario_class_coverage,
        },
        summary_artifact: OpenClosedGapBehaviorSummaryArtifact {
            schema_version: OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION,
            determinism_policy: OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_DETERMINISM_POLICY,
            total_case_count,
            total_open_window_event_count,
            total_closed_window_event_count,
            scenario_class_coverage,
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
                trading_return_regression_metric_count,
                open_jump_regression_metric_count,
                closed_gap_regression_metric_count,
                activity_coupling_regression_metric_count,
            },
            scenario_diffs,
        },
        case_reports,
    })
}

fn validate_tolerance_config(
    tolerance_config: &OpenClosedGapLeakageRegressionHarnessToleranceConfig,
) -> Result<(), OpenClosedGapLeakageRegressionHarnessError> {
    if !tolerance_config.component_absolute_tolerance.is_finite()
        || tolerance_config.component_absolute_tolerance < 0.0
    {
        return Err(
            OpenClosedGapLeakageRegressionHarnessError::InvalidComponentAbsoluteTolerance {
                value: tolerance_config.component_absolute_tolerance,
            },
        );
    }

    Ok(())
}

fn validate_scenario_class_coverage(
    scenario_class_coverage: OpenClosedGapScenarioClassCoverage,
) -> Result<(), OpenClosedGapLeakageRegressionHarnessError> {
    let mut missing_scenario_classes: Vec<NewsSemanticsScenarioClass> = Vec::new();

    if scenario_class_coverage.known_only_case_count == 0 {
        missing_scenario_classes.push(NewsSemanticsScenarioClass::KnownOnly);
    }
    if scenario_class_coverage.surprise_only_case_count == 0 {
        missing_scenario_classes.push(NewsSemanticsScenarioClass::SurpriseOnly);
    }
    if scenario_class_coverage.mixed_news_case_count == 0 {
        missing_scenario_classes.push(NewsSemanticsScenarioClass::MixedNews);
    }

    if missing_scenario_classes.is_empty() {
        return Ok(());
    }

    Err(
        OpenClosedGapLeakageRegressionHarnessError::MissingScenarioClassCoverage {
            missing_scenario_classes,
        },
    )
}

fn compute_scenario_class_coverage(
    gap_behavior_cases: &[OpenClosedGapLeakageRegressionCase],
) -> OpenClosedGapScenarioClassCoverage {
    let mut known_only_case_count = 0usize;
    let mut surprise_only_case_count = 0usize;
    let mut mixed_news_case_count = 0usize;

    for gap_behavior_case in gap_behavior_cases {
        match gap_behavior_case.scenario_class {
            NewsSemanticsScenarioClass::KnownOnly => {
                known_only_case_count = known_only_case_count.saturating_add(1);
            }
            NewsSemanticsScenarioClass::SurpriseOnly => {
                surprise_only_case_count = surprise_only_case_count.saturating_add(1);
            }
            NewsSemanticsScenarioClass::MixedNews => {
                mixed_news_case_count = mixed_news_case_count.saturating_add(1);
            }
        }
    }

    OpenClosedGapScenarioClassCoverage {
        known_only_case_count,
        surprise_only_case_count,
        mixed_news_case_count,
    }
}

#[derive(Debug, Clone, PartialEq)]
struct OpenClosedGapLeakageEventMetadata {
    event_id: String,
    event_timestamp_utc: String,
    observed_activity_coupling_j_t_increment_override: Option<f64>,
    observed_open_jump_m_t_increment_override: Option<f64>,
    observed_closed_gap_g_t_increment_override: Option<f64>,
    observed_trading_return_increment_override: Option<f64>,
}

fn evaluate_open_closed_gap_leakage_case(
    calendar_registry: &ExchangeCalendarRegistry,
    gap_behavior_case: &OpenClosedGapLeakageRegressionCase,
    component_absolute_tolerance: f64,
) -> OpenClosedGapLeakageRegressionCaseReport {
    let mut event_inputs: Vec<SharedNewsEventTimestampedInput> =
        Vec::with_capacity(gap_behavior_case.injected_events.len());
    let mut event_metadata_by_generation_order: Vec<OpenClosedGapLeakageEventMetadata> =
        Vec::with_capacity(gap_behavior_case.injected_events.len());

    for (event_position, event_input) in gap_behavior_case.injected_events.iter().enumerate() {
        let event_generation_order = match u64::try_from(event_position) {
            Ok(event_generation_order) => event_generation_order,
            Err(_) => {
                return dispatch_error_case_report(
                    gap_behavior_case,
                    component_absolute_tolerance,
                    format!("event position {event_position} does not fit in u64 generation order"),
                );
            }
        };

        if let Some(error_message) = validate_override_finite(
            "observed_activity_coupling_j_t_increment_override",
            event_input.observed_activity_coupling_j_t_increment_override,
            &event_input.event_id,
        ) {
            return dispatch_error_case_report(
                gap_behavior_case,
                component_absolute_tolerance,
                error_message,
            );
        }

        if let Some(error_message) = validate_override_finite(
            "observed_open_jump_m_t_increment_override",
            event_input.observed_open_jump_m_t_increment_override,
            &event_input.event_id,
        ) {
            return dispatch_error_case_report(
                gap_behavior_case,
                component_absolute_tolerance,
                error_message,
            );
        }

        if let Some(error_message) = validate_override_finite(
            "observed_closed_gap_g_t_increment_override",
            event_input.observed_closed_gap_g_t_increment_override,
            &event_input.event_id,
        ) {
            return dispatch_error_case_report(
                gap_behavior_case,
                component_absolute_tolerance,
                error_message,
            );
        }

        if let Some(error_message) = validate_override_finite(
            "observed_trading_return_increment_override",
            event_input.observed_trading_return_increment_override,
            &event_input.event_id,
        ) {
            return dispatch_error_case_report(
                gap_behavior_case,
                component_absolute_tolerance,
                error_message,
            );
        }

        let event_timestamp_utc_nanos = match event_input.event_timestamp_utc.timestamp_nanos_opt()
        {
            Some(event_timestamp_utc_nanos) => event_timestamp_utc_nanos,
            None => {
                return dispatch_error_case_report(
                    gap_behavior_case,
                    component_absolute_tolerance,
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
            news_arrival_component: event_input.news_arrival_component,
            shared_severity_mark: event_input.shared_severity_mark,
            coupled_return_jump_mark: event_input.coupled_return_jump_mark,
        });
        event_metadata_by_generation_order.push(OpenClosedGapLeakageEventMetadata {
            event_id: event_input.event_id.clone(),
            event_timestamp_utc: event_input.event_timestamp_utc.to_rfc3339(),
            observed_activity_coupling_j_t_increment_override: event_input
                .observed_activity_coupling_j_t_increment_override,
            observed_open_jump_m_t_increment_override: event_input
                .observed_open_jump_m_t_increment_override,
            observed_closed_gap_g_t_increment_override: event_input
                .observed_closed_gap_g_t_increment_override,
            observed_trading_return_increment_override: event_input
                .observed_trading_return_increment_override,
        });
    }

    let dispatch_output = dispatch_shared_news_events_with_resolved_market_states(
        calendar_registry,
        &gap_behavior_case.exchange_code,
        SharedNewsAccumulatorState::new(
            gap_behavior_case.initial_j_t,
            gap_behavior_case.initial_m_t,
            gap_behavior_case.initial_g_t,
        ),
        &event_inputs,
    );

    let dispatch_output = match dispatch_output {
        Ok(dispatch_output) => dispatch_output,
        Err(source) => {
            return dispatch_error_case_report(
                gap_behavior_case,
                component_absolute_tolerance,
                source.to_string(),
            );
        }
    };

    let mut open_window_event_count = 0usize;
    let mut closed_window_event_count = 0usize;
    let mut max_absolute_mismatch = 0.0;
    let mut failures: Vec<OpenClosedGapLeakageRegressionFailure> = Vec::new();
    let mut expected_activity_coupling_total = 0.0;
    let mut observed_activity_coupling_total = 0.0;
    let mut expected_trading_return_total = 0.0;
    let mut observed_trading_return_total = 0.0;
    let mut expected_open_jump_total = 0.0;
    let mut observed_open_jump_total = 0.0;
    let mut expected_closed_gap_total = 0.0;
    let mut observed_closed_gap_total = 0.0;

    for dispatch_record in &dispatch_output.records {
        let event_metadata = usize::try_from(dispatch_record.event_generation_order)
            .ok()
            .and_then(|event_position| event_metadata_by_generation_order.get(event_position));
        let (event_id, event_timestamp_utc) = event_metadata
            .map(|event_metadata| {
                (
                    event_metadata.event_id.clone(),
                    event_metadata.event_timestamp_utc.clone(),
                )
            })
            .unwrap_or_else(|| {
                (
                    format!(
                        "event_generation_order_{}",
                        dispatch_record.event_generation_order
                    ),
                    String::new(),
                )
            });

        let observed_activity_coupling = event_metadata
            .and_then(|event_metadata| {
                event_metadata.observed_activity_coupling_j_t_increment_override
            })
            .unwrap_or(dispatch_record.j_t_increment);
        let observed_open_jump = event_metadata
            .and_then(|event_metadata| event_metadata.observed_open_jump_m_t_increment_override)
            .unwrap_or(dispatch_record.m_t_increment);
        let observed_closed_gap = event_metadata
            .and_then(|event_metadata| event_metadata.observed_closed_gap_g_t_increment_override)
            .unwrap_or(dispatch_record.g_t_increment);
        let observed_trading_return = event_metadata
            .and_then(|event_metadata| event_metadata.observed_trading_return_increment_override)
            .unwrap_or(0.0);

        let expected_activity_coupling = dispatch_record.shared_severity_mark;
        let expected_trading_return = 0.0;
        let (expected_open_jump, expected_closed_gap) = match dispatch_record.market_state {
            MarketState::Open => {
                open_window_event_count = open_window_event_count.saturating_add(1);
                (dispatch_record.coupled_return_jump_mark, 0.0)
            }
            MarketState::Closed => {
                closed_window_event_count = closed_window_event_count.saturating_add(1);
                (0.0, dispatch_record.coupled_return_jump_mark)
            }
        };

        expected_activity_coupling_total += expected_activity_coupling;
        observed_activity_coupling_total += observed_activity_coupling;
        expected_trading_return_total += expected_trading_return;
        observed_trading_return_total += observed_trading_return;
        expected_open_jump_total += expected_open_jump;
        observed_open_jump_total += observed_open_jump;
        expected_closed_gap_total += expected_closed_gap;
        observed_closed_gap_total += observed_closed_gap;

        collect_semantic_mismatch(
            &mut failures,
            component_absolute_tolerance,
            &event_id,
            &event_timestamp_utc,
            VerificationSemanticBucket::ActivityCoupling,
            "activity_coupling_component_mismatch",
            expected_activity_coupling,
            observed_activity_coupling,
            &mut max_absolute_mismatch,
        );
        collect_semantic_mismatch(
            &mut failures,
            component_absolute_tolerance,
            &event_id,
            &event_timestamp_utc,
            VerificationSemanticBucket::TradingReturn,
            "trading_return_component_mismatch",
            expected_trading_return,
            observed_trading_return,
            &mut max_absolute_mismatch,
        );

        let open_jump_mismatch_kind = match dispatch_record.market_state {
            MarketState::Open => "open_window_open_jump_mismatch",
            MarketState::Closed => "closed_window_open_jump_leakage",
        };
        collect_semantic_mismatch(
            &mut failures,
            component_absolute_tolerance,
            &event_id,
            &event_timestamp_utc,
            VerificationSemanticBucket::OpenJump,
            open_jump_mismatch_kind,
            expected_open_jump,
            observed_open_jump,
            &mut max_absolute_mismatch,
        );

        let closed_gap_mismatch_kind = match dispatch_record.market_state {
            MarketState::Open => "open_window_closed_gap_leakage",
            MarketState::Closed => "closed_window_closed_gap_mismatch",
        };
        collect_semantic_mismatch(
            &mut failures,
            component_absolute_tolerance,
            &event_id,
            &event_timestamp_utc,
            VerificationSemanticBucket::ClosedGap,
            closed_gap_mismatch_kind,
            expected_closed_gap,
            observed_closed_gap,
            &mut max_absolute_mismatch,
        );
    }

    let contribution_summary = OpenClosedGapBehaviorContributionSummary {
        open_window_event_count,
        closed_window_event_count,
        expected_activity_coupling_total,
        observed_activity_coupling_total,
        expected_trading_return_total,
        observed_trading_return_total,
        expected_open_jump_total,
        observed_open_jump_total,
        expected_closed_gap_total,
        observed_closed_gap_total,
    };
    let behavior_diff = build_open_closed_behavior_scenario_diff(
        &gap_behavior_case.fixture_id,
        &gap_behavior_case.exchange_code,
        &contribution_summary,
        component_absolute_tolerance,
    );

    let semantic_mismatch_failure_count = failures.len();
    if failures.is_empty() {
        return OpenClosedGapLeakageRegressionCaseReport {
            fixture_id: gap_behavior_case.fixture_id.clone(),
            exchange_code: gap_behavior_case.exchange_code.clone(),
            scenario_class: gap_behavior_case.scenario_class,
            deterministic_replay_metadata: gap_behavior_case.deterministic_replay_metadata.clone(),
            issue_traceability: gap_behavior_case.issue_traceability.clone(),
            status: RegressionCaseStatus::Pass,
            summary: format!(
                "component routing matched expected activity/open-jump/closed-gap semantics for {} event(s)",
                dispatch_output.records.len()
            ),
            semantic_mismatch_failure_count: 0,
            max_absolute_mismatch: 0.0,
            contribution_summary,
            behavior_diff,
            failures,
        };
    }

    OpenClosedGapLeakageRegressionCaseReport {
        fixture_id: gap_behavior_case.fixture_id.clone(),
        exchange_code: gap_behavior_case.exchange_code.clone(),
        scenario_class: gap_behavior_case.scenario_class,
        deterministic_replay_metadata: gap_behavior_case.deterministic_replay_metadata.clone(),
        issue_traceability: gap_behavior_case.issue_traceability.clone(),
        status: RegressionCaseStatus::Fail,
        summary: format!(
            "detected {} semantic mismatch(es), max absolute mismatch={}",
            semantic_mismatch_failure_count, max_absolute_mismatch
        ),
        semantic_mismatch_failure_count,
        max_absolute_mismatch,
        contribution_summary,
        behavior_diff,
        failures,
    }
}

fn validate_override_finite(
    field_name: &'static str,
    override_value: Option<f64>,
    event_id: &str,
) -> Option<String> {
    let override_value = override_value?;
    if override_value.is_finite() {
        return None;
    }
    Some(format!(
        "event `{event_id}` {field_name} must be finite when set, found {override_value}"
    ))
}

fn collect_semantic_mismatch(
    failures: &mut Vec<OpenClosedGapLeakageRegressionFailure>,
    absolute_tolerance: f64,
    event_id: &str,
    event_timestamp_utc: &str,
    semantic_bucket: VerificationSemanticBucket,
    mismatch_kind: &'static str,
    expected_value: f64,
    observed_value: f64,
    max_absolute_mismatch: &mut f64,
) {
    let absolute_difference = (expected_value - observed_value).abs();
    if absolute_difference <= absolute_tolerance && absolute_difference.is_finite() {
        return;
    }

    if absolute_difference > *max_absolute_mismatch {
        *max_absolute_mismatch = absolute_difference;
    }
    failures.push(OpenClosedGapLeakageRegressionFailure::SemanticMismatch {
        semantic_bucket,
        mismatch_kind,
        event_id: event_id.to_string(),
        event_timestamp_utc: event_timestamp_utc.to_string(),
        expected_value,
        observed_value,
        absolute_difference,
    });
}

fn dispatch_error_case_report(
    gap_behavior_case: &OpenClosedGapLeakageRegressionCase,
    component_absolute_tolerance: f64,
    error_message: String,
) -> OpenClosedGapLeakageRegressionCaseReport {
    let contribution_summary = OpenClosedGapBehaviorContributionSummary {
        open_window_event_count: 0,
        closed_window_event_count: 0,
        expected_activity_coupling_total: 0.0,
        observed_activity_coupling_total: 0.0,
        expected_trading_return_total: 0.0,
        observed_trading_return_total: 0.0,
        expected_open_jump_total: 0.0,
        observed_open_jump_total: 0.0,
        expected_closed_gap_total: 0.0,
        observed_closed_gap_total: 0.0,
    };
    let behavior_diff = build_open_closed_behavior_scenario_diff(
        &gap_behavior_case.fixture_id,
        &gap_behavior_case.exchange_code,
        &contribution_summary,
        component_absolute_tolerance,
    );

    OpenClosedGapLeakageRegressionCaseReport {
        fixture_id: gap_behavior_case.fixture_id.clone(),
        exchange_code: gap_behavior_case.exchange_code.clone(),
        scenario_class: gap_behavior_case.scenario_class,
        deterministic_replay_metadata: gap_behavior_case.deterministic_replay_metadata.clone(),
        issue_traceability: gap_behavior_case.issue_traceability.clone(),
        status: RegressionCaseStatus::Fail,
        summary: format!("failed to dispatch shared news events: {error_message}"),
        semantic_mismatch_failure_count: 0,
        max_absolute_mismatch: 0.0,
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
            "trading_return_increment_total",
            VerificationSemanticBucket::TradingReturn,
            contribution_summary.expected_trading_return_total,
            contribution_summary.observed_trading_return_total,
            absolute_tolerance,
        ),
        build_open_closed_behavior_diff_metric(
            "activity_coupling_j_t_increment_total",
            VerificationSemanticBucket::ActivityCoupling,
            contribution_summary.expected_activity_coupling_total,
            contribution_summary.observed_activity_coupling_total,
            absolute_tolerance,
        ),
        build_open_closed_behavior_diff_metric(
            "open_jump_m_t_increment_total",
            VerificationSemanticBucket::OpenJump,
            contribution_summary.expected_open_jump_total,
            contribution_summary.observed_open_jump_total,
            absolute_tolerance,
        ),
        build_open_closed_behavior_diff_metric(
            "closed_gap_g_t_increment_total",
            VerificationSemanticBucket::ClosedGap,
            contribution_summary.expected_closed_gap_total,
            contribution_summary.observed_closed_gap_total,
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
    semantic_bucket: VerificationSemanticBucket,
    expected_value: f64,
    observed_value: f64,
    absolute_tolerance: f64,
) -> OpenClosedGapBehaviorDiffMetric {
    let absolute_difference = (expected_value - observed_value).abs();
    let regression_detected =
        !absolute_difference.is_finite() || absolute_difference > absolute_tolerance;

    OpenClosedGapBehaviorDiffMetric {
        metric_id,
        semantic_bucket,
        expected_value,
        observed_value,
        absolute_difference,
        absolute_tolerance,
        regression_detected,
    }
}

fn count_regression_metrics_by_bucket(
    scenario_diffs: &[OpenClosedGapBehaviorScenarioDiff],
    semantic_bucket: VerificationSemanticBucket,
) -> usize {
    scenario_diffs
        .iter()
        .flat_map(|scenario_diff| scenario_diff.metrics.iter())
        .filter(|metric| metric.semantic_bucket == semantic_bucket && metric.regression_detected)
        .count()
}
