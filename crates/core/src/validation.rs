//! Deterministic fixture-driven regression harness for calendar state and hourly factors.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::calendar::{ExchangeCalendarRegistry, MarketState};
use crate::hawkes::HawkesExcitationState;
use crate::hourly_factor_provider::{
    HourlyFactorProviderError, HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource,
    HourlySeasonalityFactorSource, provide_hourly_factor_contract,
};
use crate::news::{
    SharedNewsAccumulatorState, SharedNewsEventTimestampedInput,
    dispatch_shared_news_events_with_resolved_market_states,
};
use crate::risk::{
    SufficientStatisticsBoundaryStateUpdateError, SufficientStatisticsBoundaryStateUpdateSnapshot,
    SufficientStatisticsIntervalStatistics, SufficientStatisticsRiskRunRequest,
    SufficientStatisticsRiskRunRequestError, SufficientStatisticsTransformSliceContext,
    SufficientStatisticsTransformSliceSamplingError, SufficientStatisticsTransformSliceSettings,
    SufficientStatisticsTransformSliceSolveError, SufficientStatisticsTransformSlices,
    derive_sufficient_statistics_batch_path_seed, sample_interval_statistics_from_transform_slices,
    solve_sufficient_statistics_transform_slices, update_boundary_state_from_interval_statistics,
};

pub const DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION: &str =
    "validation.deterministic_regression_harness.report.v1";
pub const DETERMINISTIC_REGRESSION_HARNESS_DETERMINISM_POLICY: &str =
    "stable_input_order_and_first_mismatch_reporting.v1";
pub const VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION: &str = "validation.fallback_policy.v1";
pub const VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY: &str =
    "resolve_with_regular_sessions_when_no_date_override.v1";
pub const VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY: &str =
    "fail_case_with_explicit_calendar_resolution_error.v1";
pub const VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY: &str =
    "fail_fast_on_fixture_contract_validation.v1";
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
pub const SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_REPORT_SCHEMA_VERSION: &str =
    "validation.sufficient_statistics.multi_interval.report.v1";
pub const SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_DETERMINISM_POLICY: &str =
    "stable_input_case_order_and_seed_replay_validation.v1";
pub const SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_COVERAGE_POLICY: &str =
    "require_short_medium_and_long_horizon_scenarios.v1";
pub const SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "validation.sufficient_statistics.multi_interval.output_schema_metadata.v1";
pub const SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLDS_SCHEMA_VERSION: &str =
    "validation.sufficient_statistics.multi_interval.thresholds.v1";
pub const SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLD_POLICY: &str =
    "deterministic_invariants_strict_and_interval_mean_tolerance_guard.v1";

const SUFFICIENT_STATISTICS_MULTI_INTERVAL_REQUIRED_CASE_REPORT_FIELDS: [&str; 8] = [
    "scenario_id",
    "horizon_category",
    "status",
    "interval_count",
    "deterministic_failed_check_count",
    "statistical_failed_check_count",
    "deterministic_checks",
    "statistical_checks",
];
const SUFFICIENT_STATISTICS_MULTI_INTERVAL_REQUIRED_DETERMINISTIC_CHECK_FIELDS: [&str; 3] =
    ["check_id", "passed", "detail"];
const SUFFICIENT_STATISTICS_MULTI_INTERVAL_REQUIRED_STATISTICAL_CHECK_FIELDS: [&str; 7] = [
    "check_id",
    "interval_index",
    "expected_mean",
    "observed_mean",
    "absolute_difference",
    "absolute_tolerance",
    "passed",
];
const SUFFICIENT_STATISTICS_MULTI_INTERVAL_EXPECTATION_REAL_PART_TOLERANCE: f64 = 1.0e-8;
const SUFFICIENT_STATISTICS_MULTI_INTERVAL_EXPECTATION_NEGATIVE_TOLERANCE: f64 = 1.0e-10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SufficientStatisticsValidationHorizonCategory {
    Short,
    Medium,
    Long,
}

impl Display for SufficientStatisticsValidationHorizonCategory {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Short => write!(formatter, "short"),
            Self::Medium => write!(formatter, "medium"),
            Self::Long => write!(formatter, "long"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SufficientStatisticsMultiIntervalValidationScenario {
    pub scenario_id: String,
    pub horizon_category: SufficientStatisticsValidationHorizonCategory,
    pub run_request: SufficientStatisticsRiskRunRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsMultiIntervalValidationThresholds {
    pub schema_version: &'static str,
    pub threshold_policy: &'static str,
    pub statistical_sample_count: usize,
    pub integrated_intensity_mean_absolute_floor: f64,
    pub integrated_intensity_mean_relative_tolerance: f64,
    pub event_count_mean_absolute_floor: f64,
    pub event_count_mean_relative_tolerance: f64,
}

impl Default for SufficientStatisticsMultiIntervalValidationThresholds {
    fn default() -> Self {
        Self {
            schema_version:
                SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLDS_SCHEMA_VERSION,
            threshold_policy: SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLD_POLICY,
            statistical_sample_count: 256,
            integrated_intensity_mean_absolute_floor: 0.35,
            integrated_intensity_mean_relative_tolerance: 0.55,
            event_count_mean_absolute_floor: 0.45,
            event_count_mean_relative_tolerance: 0.55,
        }
    }
}

impl SufficientStatisticsMultiIntervalValidationThresholds {
    pub fn validate(
        &self,
    ) -> Result<(), SufficientStatisticsMultiIntervalValidationThresholdsError> {
        if self.schema_version
            != SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLDS_SCHEMA_VERSION
        {
            return Err(
                SufficientStatisticsMultiIntervalValidationThresholdsError::InvalidSchemaVersion {
                    expected:
                        SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLDS_SCHEMA_VERSION,
                    actual: self.schema_version.to_string(),
                },
            );
        }

        if self.threshold_policy != SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLD_POLICY
        {
            return Err(
                SufficientStatisticsMultiIntervalValidationThresholdsError::InvalidThresholdPolicy {
                    expected: SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLD_POLICY,
                    actual: self.threshold_policy.to_string(),
                },
            );
        }

        if self.statistical_sample_count == 0 {
            return Err(
                SufficientStatisticsMultiIntervalValidationThresholdsError::InvalidStatisticalSampleCount {
                    value: self.statistical_sample_count,
                },
            );
        }

        validate_non_negative_threshold(
            "integrated_intensity_mean_absolute_floor",
            self.integrated_intensity_mean_absolute_floor,
        )?;
        validate_non_negative_threshold(
            "integrated_intensity_mean_relative_tolerance",
            self.integrated_intensity_mean_relative_tolerance,
        )?;
        validate_non_negative_threshold(
            "event_count_mean_absolute_floor",
            self.event_count_mean_absolute_floor,
        )?;
        validate_non_negative_threshold(
            "event_count_mean_relative_tolerance",
            self.event_count_mean_relative_tolerance,
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsMultiIntervalValidationThresholdsError {
    InvalidSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    InvalidThresholdPolicy {
        expected: &'static str,
        actual: String,
    },
    InvalidStatisticalSampleCount {
        value: usize,
    },
    InvalidThresholdValue {
        field_name: &'static str,
        value: f64,
    },
}

impl Display for SufficientStatisticsMultiIntervalValidationThresholdsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion { expected, actual } => write!(
                formatter,
                "thresholds.schema_version must be `{expected}`, found `{actual}`"
            ),
            Self::InvalidThresholdPolicy { expected, actual } => write!(
                formatter,
                "thresholds.threshold_policy must be `{expected}`, found `{actual}`"
            ),
            Self::InvalidStatisticalSampleCount { value } => write!(
                formatter,
                "thresholds.statistical_sample_count must be greater than 0, found {value}"
            ),
            Self::InvalidThresholdValue { field_name, value } => write!(
                formatter,
                "thresholds.{field_name} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsMultiIntervalValidationThresholdsError {}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsMultiIntervalValidationOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub thresholds_schema_version: &'static str,
    pub required_case_report_fields: Vec<&'static str>,
    pub required_deterministic_check_fields: Vec<&'static str>,
    pub required_statistical_check_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsDeterministicInvariantCheck {
    pub check_id: &'static str,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsDistributionalCheck {
    pub check_id: &'static str,
    pub interval_index: usize,
    pub expected_mean: f64,
    pub observed_mean: f64,
    pub absolute_difference: f64,
    pub absolute_tolerance: f64,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsMultiIntervalValidationCaseReport {
    pub scenario_id: String,
    pub horizon_category: SufficientStatisticsValidationHorizonCategory,
    pub status: RegressionCaseStatus,
    pub interval_count: usize,
    pub deterministic_failed_check_count: usize,
    pub statistical_failed_check_count: usize,
    pub deterministic_checks: Vec<SufficientStatisticsDeterministicInvariantCheck>,
    pub statistical_checks: Vec<SufficientStatisticsDistributionalCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SufficientStatisticsMultiIntervalValidationSummary {
    pub total_case_count: usize,
    pub passed_case_count: usize,
    pub failed_case_count: usize,
    pub deterministic_failed_check_count: usize,
    pub statistical_failed_check_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsMultiIntervalValidationReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub coverage_policy: &'static str,
    pub output_schema_metadata: SufficientStatisticsMultiIntervalValidationOutputSchemaMetadata,
    pub thresholds: SufficientStatisticsMultiIntervalValidationThresholds,
    pub summary: SufficientStatisticsMultiIntervalValidationSummary,
    pub case_reports: Vec<SufficientStatisticsMultiIntervalValidationCaseReport>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsMultiIntervalValidationError {
    EmptyScenarios,
    EmptyScenarioId,
    DuplicateScenarioId {
        scenario_id: String,
    },
    MissingHorizonCoverage {
        missing_horizon_categories: Vec<SufficientStatisticsValidationHorizonCategory>,
    },
    InvalidThresholds {
        source: SufficientStatisticsMultiIntervalValidationThresholdsError,
    },
    InvalidRunRequest {
        scenario_id: String,
        source: SufficientStatisticsRiskRunRequestError,
    },
    TransformSliceSolveFailure {
        scenario_id: String,
        interval_index: usize,
        source: SufficientStatisticsTransformSliceSolveError,
    },
    TransformSliceSamplingFailure {
        scenario_id: String,
        interval_index: usize,
        source: SufficientStatisticsTransformSliceSamplingError,
    },
    BoundaryStateUpdateFailure {
        scenario_id: String,
        interval_index: usize,
        source: SufficientStatisticsBoundaryStateUpdateError,
    },
    InvalidExpectedStatistic {
        scenario_id: String,
        interval_index: usize,
        check_id: &'static str,
        value: f64,
    },
}

impl Display for SufficientStatisticsMultiIntervalValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyScenarios => {
                write!(formatter, "validation suite requires at least one scenario")
            }
            Self::EmptyScenarioId => write!(
                formatter,
                "validation scenarios must use non-empty scenario_id values"
            ),
            Self::DuplicateScenarioId { scenario_id } => write!(
                formatter,
                "validation scenarios must use unique scenario_id values, found duplicate `{scenario_id}`"
            ),
            Self::MissingHorizonCoverage {
                missing_horizon_categories,
            } => {
                let missing_categories = missing_horizon_categories
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<String>>()
                    .join(", ");
                write!(
                    formatter,
                    "validation scenarios must cover short, medium, and long horizon categories, missing [{missing_categories}]"
                )
            }
            Self::InvalidThresholds { source } => {
                write!(formatter, "threshold configuration is invalid: {source}")
            }
            Self::InvalidRunRequest {
                scenario_id,
                source,
            } => write!(
                formatter,
                "scenario `{scenario_id}` has invalid run_request payload: {source}"
            ),
            Self::TransformSliceSolveFailure {
                scenario_id,
                interval_index,
                source,
            } => write!(
                formatter,
                "scenario `{scenario_id}` interval {interval_index} failed transform-slice solve: {source}"
            ),
            Self::TransformSliceSamplingFailure {
                scenario_id,
                interval_index,
                source,
            } => write!(
                formatter,
                "scenario `{scenario_id}` interval {interval_index} failed transform-slice sampling: {source}"
            ),
            Self::BoundaryStateUpdateFailure {
                scenario_id,
                interval_index,
                source,
            } => write!(
                formatter,
                "scenario `{scenario_id}` interval {interval_index} failed boundary-state update: {source}"
            ),
            Self::InvalidExpectedStatistic {
                scenario_id,
                interval_index,
                check_id,
                value,
            } => write!(
                formatter,
                "scenario `{scenario_id}` interval {interval_index} produced non-finite expected statistic for `{check_id}`: {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsMultiIntervalValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidThresholds { source } => Some(source),
            Self::InvalidRunRequest { source, .. } => Some(source),
            Self::TransformSliceSolveFailure { source, .. } => Some(source),
            Self::TransformSliceSamplingFailure { source, .. } => Some(source),
            Self::BoundaryStateUpdateFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegressionHarnessToleranceConfig {
    pub seasonality_absolute_tolerance: f64,
    pub news_intensity_absolute_tolerance: f64,
}

impl Default for RegressionHarnessToleranceConfig {
    fn default() -> Self {
        Self {
            seasonality_absolute_tolerance: 1e-12,
            news_intensity_absolute_tolerance: 1e-12,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarStateRegressionCase {
    pub fixture_id: String,
    pub exchange_code: String,
    pub timestamp_utc: DateTime<Utc>,
    pub expected_state: MarketState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HourlyFactorRegressionCase {
    pub fixture_id: String,
    pub exchange_code: String,
    pub range_start_utc: DateTime<Utc>,
    pub range_end_utc: DateTime<Utc>,
    pub seasonality_source: HourlySeasonalityRegressionSource,
    pub news_intensity_source: HourlyNewsIntensityRegressionSource,
    pub expected_s_h: Vec<f64>,
    pub expected_nu_h: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HourlySeasonalityRegressionSource {
    Explicit {
        hourly_factors: Vec<f64>,
        materialization_method: String,
    },
    Materialized {
        source_timestamp_utc_nanos: Vec<i64>,
        source_seasonality_factors: Vec<f64>,
        interpolation_method: String,
        aggregation_method: String,
        sparse_fallback_policy: String,
        sparse_hour_minimum_observation_count: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum HourlyNewsIntensityRegressionSource {
    Explicit {
        hourly_factors: Vec<f64>,
        materialization_method: String,
    },
    Materialized {
        closed_window_baseline_intensity: f64,
        open_window_override_by_interval_index: Vec<(u64, f64)>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionCaseKind {
    CalendarMarketState,
    HourlyFactorOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionCaseStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum RegressionCaseFailure {
    CalendarResolutionError {
        exchange_code: String,
        timestamp_utc: String,
        error_message: String,
    },
    MarketStateMismatch {
        exchange_code: String,
        timestamp_utc: String,
        expected_state: String,
        observed_state: String,
    },
    HourlyFactorResolutionError {
        error_message: String,
    },
    FactorLengthMismatch {
        field_name: &'static str,
        expected_length: usize,
        observed_length: usize,
    },
    FactorToleranceExceeded {
        field_name: &'static str,
        value_index: usize,
        expected_value: f64,
        observed_value: f64,
        absolute_difference: f64,
        absolute_tolerance: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegressionCaseReport {
    pub fixture_id: String,
    pub case_kind: RegressionCaseKind,
    pub status: RegressionCaseStatus,
    pub summary: String,
    pub failure: Option<RegressionCaseFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegressionHarnessSummary {
    pub total_case_count: usize,
    pub passed_case_count: usize,
    pub failed_case_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegressionHarnessFallbackPolicy {
    pub schema_version: &'static str,
    pub unknown_date_policy: &'static str,
    pub unsupported_exchange_calendar_segment_policy: &'static str,
    pub incomplete_fixture_coverage_policy: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegressionHarnessReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub fallback_policy: RegressionHarnessFallbackPolicy,
    pub tolerance_config: RegressionHarnessToleranceConfig,
    pub summary: RegressionHarnessSummary,
    pub case_reports: Vec<RegressionCaseReport>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegressionHarnessError {
    InvalidSeasonalityAbsoluteTolerance { value: f64 },
    InvalidNewsIntensityAbsoluteTolerance { value: f64 },
}

impl Display for RegressionHarnessError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSeasonalityAbsoluteTolerance { value } => write!(
                formatter,
                "seasonality_absolute_tolerance must be a finite non-negative value, found {value}"
            ),
            Self::InvalidNewsIntensityAbsoluteTolerance { value } => write!(
                formatter,
                "news_intensity_absolute_tolerance must be a finite non-negative value, found {value}"
            ),
        }
    }
}

impl Error for RegressionHarnessError {}

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

pub fn run_deterministic_regression_harness(
    calendar_registry: &ExchangeCalendarRegistry,
    calendar_cases: &[CalendarStateRegressionCase],
    hourly_factor_cases: &[HourlyFactorRegressionCase],
    tolerance_config: RegressionHarnessToleranceConfig,
) -> Result<RegressionHarnessReport, RegressionHarnessError> {
    validate_tolerance_config(&tolerance_config)?;

    let mut case_reports: Vec<RegressionCaseReport> =
        Vec::with_capacity(calendar_cases.len() + hourly_factor_cases.len());

    for calendar_case in calendar_cases {
        case_reports.push(evaluate_calendar_case(calendar_registry, calendar_case));
    }

    for hourly_factor_case in hourly_factor_cases {
        case_reports.push(evaluate_hourly_factor_case(
            calendar_registry,
            hourly_factor_case,
            &tolerance_config,
        ));
    }

    let passed_case_count = case_reports
        .iter()
        .filter(|case_report| case_report.status == RegressionCaseStatus::Pass)
        .count();
    let total_case_count = case_reports.len();
    let failed_case_count = total_case_count - passed_case_count;

    Ok(RegressionHarnessReport {
        schema_version: DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
        determinism_policy: DETERMINISTIC_REGRESSION_HARNESS_DETERMINISM_POLICY,
        fallback_policy: RegressionHarnessFallbackPolicy {
            schema_version: VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION,
            unknown_date_policy: VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY,
            unsupported_exchange_calendar_segment_policy:
                VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY,
            incomplete_fixture_coverage_policy: VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY,
        },
        tolerance_config,
        summary: RegressionHarnessSummary {
            total_case_count,
            passed_case_count,
            failed_case_count,
        },
        case_reports,
    })
}

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

fn validate_tolerance_config(
    tolerance_config: &RegressionHarnessToleranceConfig,
) -> Result<(), RegressionHarnessError> {
    if !tolerance_config.seasonality_absolute_tolerance.is_finite()
        || tolerance_config.seasonality_absolute_tolerance < 0.0
    {
        return Err(
            RegressionHarnessError::InvalidSeasonalityAbsoluteTolerance {
                value: tolerance_config.seasonality_absolute_tolerance,
            },
        );
    }

    if !tolerance_config
        .news_intensity_absolute_tolerance
        .is_finite()
        || tolerance_config.news_intensity_absolute_tolerance < 0.0
    {
        return Err(
            RegressionHarnessError::InvalidNewsIntensityAbsoluteTolerance {
                value: tolerance_config.news_intensity_absolute_tolerance,
            },
        );
    }

    Ok(())
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

fn evaluate_calendar_case(
    calendar_registry: &ExchangeCalendarRegistry,
    calendar_case: &CalendarStateRegressionCase,
) -> RegressionCaseReport {
    let timestamp_utc = calendar_case.timestamp_utc.to_rfc3339();

    match calendar_registry
        .resolve_market_state_at_utc(&calendar_case.exchange_code, calendar_case.timestamp_utc)
    {
        Ok(observed_state) => {
            if observed_state == calendar_case.expected_state {
                return RegressionCaseReport {
                    fixture_id: calendar_case.fixture_id.clone(),
                    case_kind: RegressionCaseKind::CalendarMarketState,
                    status: RegressionCaseStatus::Pass,
                    summary: format!(
                        "resolved expected market state `{}`",
                        calendar_case.expected_state
                    ),
                    failure: None,
                };
            }

            RegressionCaseReport {
                fixture_id: calendar_case.fixture_id.clone(),
                case_kind: RegressionCaseKind::CalendarMarketState,
                status: RegressionCaseStatus::Fail,
                summary: format!(
                    "expected market state `{}` but observed `{}`",
                    calendar_case.expected_state, observed_state
                ),
                failure: Some(RegressionCaseFailure::MarketStateMismatch {
                    exchange_code: calendar_case.exchange_code.clone(),
                    timestamp_utc,
                    expected_state: calendar_case.expected_state.to_string(),
                    observed_state: observed_state.to_string(),
                }),
            }
        }
        Err(source) => RegressionCaseReport {
            fixture_id: calendar_case.fixture_id.clone(),
            case_kind: RegressionCaseKind::CalendarMarketState,
            status: RegressionCaseStatus::Fail,
            summary: format!("failed to resolve market state: {source}"),
            failure: Some(RegressionCaseFailure::CalendarResolutionError {
                exchange_code: calendar_case.exchange_code.clone(),
                timestamp_utc,
                error_message: source.to_string(),
            }),
        },
    }
}

fn evaluate_hourly_factor_case(
    calendar_registry: &ExchangeCalendarRegistry,
    hourly_factor_case: &HourlyFactorRegressionCase,
    tolerance_config: &RegressionHarnessToleranceConfig,
) -> RegressionCaseReport {
    let contract_output = resolve_hourly_factor_contract(calendar_registry, hourly_factor_case);
    let contract_output = match contract_output {
        Ok(output) => output,
        Err(error) => {
            return RegressionCaseReport {
                fixture_id: hourly_factor_case.fixture_id.clone(),
                case_kind: RegressionCaseKind::HourlyFactorOutput,
                status: RegressionCaseStatus::Fail,
                summary: format!("failed to resolve hourly factor output: {error}"),
                failure: Some(RegressionCaseFailure::HourlyFactorResolutionError {
                    error_message: error.to_string(),
                }),
            };
        }
    };

    if let Some(failure) = compare_factor_vectors(
        "s_h",
        &hourly_factor_case.expected_s_h,
        &contract_output.s_h,
        tolerance_config.seasonality_absolute_tolerance,
    ) {
        return RegressionCaseReport {
            fixture_id: hourly_factor_case.fixture_id.clone(),
            case_kind: RegressionCaseKind::HourlyFactorOutput,
            status: RegressionCaseStatus::Fail,
            summary: "seasonality factor output mismatch".to_string(),
            failure: Some(failure),
        };
    }

    if let Some(failure) = compare_factor_vectors(
        "nu_h",
        &hourly_factor_case.expected_nu_h,
        &contract_output.nu_h,
        tolerance_config.news_intensity_absolute_tolerance,
    ) {
        return RegressionCaseReport {
            fixture_id: hourly_factor_case.fixture_id.clone(),
            case_kind: RegressionCaseKind::HourlyFactorOutput,
            status: RegressionCaseStatus::Fail,
            summary: "news intensity factor output mismatch".to_string(),
            failure: Some(failure),
        };
    }

    RegressionCaseReport {
        fixture_id: hourly_factor_case.fixture_id.clone(),
        case_kind: RegressionCaseKind::HourlyFactorOutput,
        status: RegressionCaseStatus::Pass,
        summary: "hourly factor outputs matched expected vectors".to_string(),
        failure: None,
    }
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

fn resolve_hourly_factor_contract(
    calendar_registry: &ExchangeCalendarRegistry,
    hourly_factor_case: &HourlyFactorRegressionCase,
) -> Result<crate::hourly_factors::HourlyFactorContractOutput, HourlyFactorProviderError> {
    let seasonality_source = match &hourly_factor_case.seasonality_source {
        HourlySeasonalityRegressionSource::Explicit {
            hourly_factors,
            materialization_method,
        } => HourlySeasonalityFactorSource::Explicit {
            hourly_factors,
            materialization_method,
        },
        HourlySeasonalityRegressionSource::Materialized {
            source_timestamp_utc_nanos,
            source_seasonality_factors,
            interpolation_method,
            aggregation_method,
            sparse_fallback_policy,
            sparse_hour_minimum_observation_count,
        } => HourlySeasonalityFactorSource::Materialized {
            source_timestamp_utc_nanos,
            source_seasonality_factors,
            interpolation_method,
            aggregation_method,
            sparse_fallback_policy,
            sparse_hour_minimum_observation_count: *sparse_hour_minimum_observation_count,
        },
    };

    let news_intensity_source = match &hourly_factor_case.news_intensity_source {
        HourlyNewsIntensityRegressionSource::Explicit {
            hourly_factors,
            materialization_method,
        } => HourlyNewsIntensityFactorSource::Explicit {
            hourly_factors,
            materialization_method,
        },
        HourlyNewsIntensityRegressionSource::Materialized {
            closed_window_baseline_intensity,
            open_window_override_by_interval_index,
        } => HourlyNewsIntensityFactorSource::Materialized {
            closed_window_baseline_intensity: *closed_window_baseline_intensity,
            open_window_override_by_interval_index,
        },
    };

    provide_hourly_factor_contract(
        calendar_registry,
        HourlyFactorProviderRequest {
            exchange_code: &hourly_factor_case.exchange_code,
            range_start_utc: hourly_factor_case.range_start_utc,
            range_end_utc: hourly_factor_case.range_end_utc,
            seasonality_source: Some(seasonality_source),
            news_intensity_source: Some(news_intensity_source),
        },
    )
}

fn compare_factor_vectors(
    field_name: &'static str,
    expected_values: &[f64],
    observed_values: &[f64],
    absolute_tolerance: f64,
) -> Option<RegressionCaseFailure> {
    if expected_values.len() != observed_values.len() {
        return Some(RegressionCaseFailure::FactorLengthMismatch {
            field_name,
            expected_length: expected_values.len(),
            observed_length: observed_values.len(),
        });
    }

    for value_index in 0..expected_values.len() {
        let expected_value = expected_values[value_index];
        let observed_value = observed_values[value_index];
        let absolute_difference = (expected_value - observed_value).abs();
        if !absolute_difference.is_finite() || absolute_difference > absolute_tolerance {
            return Some(RegressionCaseFailure::FactorToleranceExceeded {
                field_name,
                value_index,
                expected_value,
                observed_value,
                absolute_difference,
                absolute_tolerance,
            });
        }
    }

    None
}

pub fn run_sufficient_statistics_multi_interval_validation_suite(
    scenarios: &[SufficientStatisticsMultiIntervalValidationScenario],
    thresholds: SufficientStatisticsMultiIntervalValidationThresholds,
) -> Result<
    SufficientStatisticsMultiIntervalValidationReport,
    SufficientStatisticsMultiIntervalValidationError,
> {
    if scenarios.is_empty() {
        return Err(SufficientStatisticsMultiIntervalValidationError::EmptyScenarios);
    }

    thresholds.validate().map_err(|source| {
        SufficientStatisticsMultiIntervalValidationError::InvalidThresholds { source }
    })?;
    validate_multi_interval_scenario_coverage(scenarios)?;

    let mut case_reports: Vec<SufficientStatisticsMultiIntervalValidationCaseReport> =
        Vec::with_capacity(scenarios.len());

    for validation_scenario in scenarios {
        case_reports.push(run_multi_interval_validation_case(
            validation_scenario,
            &thresholds,
        )?);
    }

    let passed_case_count = case_reports
        .iter()
        .filter(|case_report| case_report.status == RegressionCaseStatus::Pass)
        .count();
    let total_case_count = case_reports.len();
    let failed_case_count = total_case_count - passed_case_count;
    let deterministic_failed_check_count = case_reports
        .iter()
        .map(|case_report| case_report.deterministic_failed_check_count)
        .sum();
    let statistical_failed_check_count = case_reports
        .iter()
        .map(|case_report| case_report.statistical_failed_check_count)
        .sum();

    Ok(SufficientStatisticsMultiIntervalValidationReport {
        schema_version: SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_REPORT_SCHEMA_VERSION,
        determinism_policy: SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_DETERMINISM_POLICY,
        coverage_policy: SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_COVERAGE_POLICY,
        output_schema_metadata: SufficientStatisticsMultiIntervalValidationOutputSchemaMetadata {
            schema_version:
                SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_OUTPUT_SCHEMA_METADATA_VERSION,
            thresholds_schema_version:
                SUFFICIENT_STATISTICS_MULTI_INTERVAL_VALIDATION_THRESHOLDS_SCHEMA_VERSION,
            required_case_report_fields:
                SUFFICIENT_STATISTICS_MULTI_INTERVAL_REQUIRED_CASE_REPORT_FIELDS.to_vec(),
            required_deterministic_check_fields:
                SUFFICIENT_STATISTICS_MULTI_INTERVAL_REQUIRED_DETERMINISTIC_CHECK_FIELDS.to_vec(),
            required_statistical_check_fields:
                SUFFICIENT_STATISTICS_MULTI_INTERVAL_REQUIRED_STATISTICAL_CHECK_FIELDS.to_vec(),
        },
        thresholds,
        summary: SufficientStatisticsMultiIntervalValidationSummary {
            total_case_count,
            passed_case_count,
            failed_case_count,
            deterministic_failed_check_count,
            statistical_failed_check_count,
        },
        case_reports,
    })
}

fn run_multi_interval_validation_case(
    validation_scenario: &SufficientStatisticsMultiIntervalValidationScenario,
    thresholds: &SufficientStatisticsMultiIntervalValidationThresholds,
) -> Result<
    SufficientStatisticsMultiIntervalValidationCaseReport,
    SufficientStatisticsMultiIntervalValidationError,
> {
    validation_scenario
        .run_request
        .validate()
        .map_err(
            |source| SufficientStatisticsMultiIntervalValidationError::InvalidRunRequest {
                scenario_id: validation_scenario.scenario_id.clone(),
                source,
            },
        )?;

    let first_path = execute_sufficient_statistics_validation_path(validation_scenario)?;
    let second_path = execute_sufficient_statistics_validation_path(validation_scenario)?;

    let deterministic_checks =
        build_deterministic_invariant_checks(validation_scenario, &first_path, &second_path);
    let statistical_checks =
        build_distributional_checks(validation_scenario, thresholds, &first_path)?;

    let deterministic_failed_check_count = deterministic_checks
        .iter()
        .filter(|check| !check.passed)
        .count();
    let statistical_failed_check_count = statistical_checks
        .iter()
        .filter(|check| !check.passed)
        .count();
    let status = if deterministic_failed_check_count == 0 && statistical_failed_check_count == 0 {
        RegressionCaseStatus::Pass
    } else {
        RegressionCaseStatus::Fail
    };

    Ok(SufficientStatisticsMultiIntervalValidationCaseReport {
        scenario_id: validation_scenario.scenario_id.clone(),
        horizon_category: validation_scenario.horizon_category,
        status,
        interval_count: validation_scenario.run_request.horizon_interval_count,
        deterministic_failed_check_count,
        statistical_failed_check_count,
        deterministic_checks,
        statistical_checks,
    })
}

fn validate_multi_interval_scenario_coverage(
    scenarios: &[SufficientStatisticsMultiIntervalValidationScenario],
) -> Result<(), SufficientStatisticsMultiIntervalValidationError> {
    let mut scenario_ids: BTreeSet<String> = BTreeSet::new();
    let mut covered_horizon_categories: BTreeSet<SufficientStatisticsValidationHorizonCategory> =
        BTreeSet::new();

    for validation_scenario in scenarios {
        let scenario_id = validation_scenario.scenario_id.trim();
        if scenario_id.is_empty() {
            return Err(SufficientStatisticsMultiIntervalValidationError::EmptyScenarioId);
        }

        if !scenario_ids.insert(scenario_id.to_string()) {
            return Err(
                SufficientStatisticsMultiIntervalValidationError::DuplicateScenarioId {
                    scenario_id: scenario_id.to_string(),
                },
            );
        }

        covered_horizon_categories.insert(validation_scenario.horizon_category);
    }

    let required_horizon_categories = [
        SufficientStatisticsValidationHorizonCategory::Short,
        SufficientStatisticsValidationHorizonCategory::Medium,
        SufficientStatisticsValidationHorizonCategory::Long,
    ];
    let missing_horizon_categories: Vec<SufficientStatisticsValidationHorizonCategory> =
        required_horizon_categories
            .into_iter()
            .filter(|horizon_category| !covered_horizon_categories.contains(horizon_category))
            .collect();

    if !missing_horizon_categories.is_empty() {
        return Err(
            SufficientStatisticsMultiIntervalValidationError::MissingHorizonCoverage {
                missing_horizon_categories,
            },
        );
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
struct SufficientStatisticsValidationPathArtifacts {
    sampled_statistics: Vec<SufficientStatisticsIntervalStatistics>,
    state_after_interval: Vec<HawkesExcitationState>,
    solved_slices: Vec<SufficientStatisticsTransformSlices>,
}

fn execute_sufficient_statistics_validation_path(
    validation_scenario: &SufficientStatisticsMultiIntervalValidationScenario,
) -> Result<
    SufficientStatisticsValidationPathArtifacts,
    SufficientStatisticsMultiIntervalValidationError,
> {
    let run_request = &validation_scenario.run_request;
    let mut current_state = run_request.model_inputs.initial_excitation_state;
    let equation_specification = run_request.model_inputs.equation_specification;
    let kernel_parameters = equation_specification.hawkes_kernel_parameters;
    let slice_settings = SufficientStatisticsTransformSliceSettings::default();

    let mut sampled_statistics: Vec<SufficientStatisticsIntervalStatistics> =
        Vec::with_capacity(run_request.horizon_interval_count);
    let mut state_after_interval: Vec<HawkesExcitationState> =
        Vec::with_capacity(run_request.horizon_interval_count);
    let mut solved_slices: Vec<SufficientStatisticsTransformSlices> =
        Vec::with_capacity(run_request.horizon_interval_count);

    for interval_index in 0..run_request.horizon_interval_count {
        let slice_context = SufficientStatisticsTransformSliceContext::try_new(
            interval_index,
            run_request.interval_factors.s_h[interval_index],
            run_request.interval_factors.nu_h[interval_index],
            run_request.interval_delta,
            current_state,
        )
        .map_err(|source| {
            SufficientStatisticsMultiIntervalValidationError::TransformSliceSolveFailure {
                scenario_id: validation_scenario.scenario_id.clone(),
                interval_index,
                source: SufficientStatisticsTransformSliceSolveError::InvalidContext { source },
            }
        })?;

        let slices = solve_sufficient_statistics_transform_slices(
            equation_specification,
            slice_context,
            slice_settings,
        )
        .map_err(|source| {
            SufficientStatisticsMultiIntervalValidationError::TransformSliceSolveFailure {
                scenario_id: validation_scenario.scenario_id.clone(),
                interval_index,
                source,
            }
        })?;

        let sampled_interval_statistics = sample_interval_statistics_from_transform_slices(
            run_request.seed,
            slices,
        )
        .map_err(|source| {
            SufficientStatisticsMultiIntervalValidationError::TransformSliceSamplingFailure {
                scenario_id: validation_scenario.scenario_id.clone(),
                interval_index,
                source,
            }
        })?;

        let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
            interval_index,
            run_request.interval_delta,
            current_state,
            sampled_interval_statistics,
        );

        let updated_state =
            update_boundary_state_from_interval_statistics(kernel_parameters, update_snapshot)
                .map_err(|source| {
                    SufficientStatisticsMultiIntervalValidationError::BoundaryStateUpdateFailure {
                        scenario_id: validation_scenario.scenario_id.clone(),
                        interval_index,
                        source,
                    }
                })?;

        sampled_statistics.push(sampled_interval_statistics);
        state_after_interval.push(updated_state);
        solved_slices.push(slices);
        current_state = updated_state;
    }

    Ok(SufficientStatisticsValidationPathArtifacts {
        sampled_statistics,
        state_after_interval,
        solved_slices,
    })
}

fn build_deterministic_invariant_checks(
    validation_scenario: &SufficientStatisticsMultiIntervalValidationScenario,
    first_path: &SufficientStatisticsValidationPathArtifacts,
    second_path: &SufficientStatisticsValidationPathArtifacts,
) -> Vec<SufficientStatisticsDeterministicInvariantCheck> {
    let replay_matches = first_path.sampled_statistics == second_path.sampled_statistics
        && first_path.state_after_interval == second_path.state_after_interval
        && first_path.solved_slices == second_path.solved_slices;

    let mut replay_state = validation_scenario
        .run_request
        .model_inputs
        .initial_excitation_state;
    let kernel_parameters = validation_scenario
        .run_request
        .model_inputs
        .equation_specification
        .hawkes_kernel_parameters;
    let mut state_transition_replay_matches = true;

    for (interval_index, sampled_statistics) in
        first_path.sampled_statistics.iter().copied().enumerate()
    {
        let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
            interval_index,
            validation_scenario.run_request.interval_delta,
            replay_state,
            sampled_statistics,
        );

        let replayed_state = match update_boundary_state_from_interval_statistics(
            kernel_parameters,
            update_snapshot,
        ) {
            Ok(value) => value,
            Err(_source) => {
                state_transition_replay_matches = false;
                break;
            }
        };

        if replayed_state != first_path.state_after_interval[interval_index] {
            state_transition_replay_matches = false;
            break;
        }

        replay_state = replayed_state;
    }

    vec![
        SufficientStatisticsDeterministicInvariantCheck {
            check_id: "seed_replay_interval_statistics_and_state_path",
            passed: replay_matches,
            detail: "re-running identical scenario with fixed seed must produce identical slices, sampled statistics, and boundary-state path"
                .to_string(),
        },
        SufficientStatisticsDeterministicInvariantCheck {
            check_id: "state_transition_replay_consistency",
            passed: state_transition_replay_matches,
            detail: "replaying boundary-state updates from sampled statistics must recover identical per-interval states"
                .to_string(),
        },
    ]
}

#[derive(Debug, Clone, Copy)]
struct SufficientStatisticsExpectedMeans {
    a_plus_mean: f64,
    a_minus_mean: f64,
    n_plus_mean: f64,
    n_minus_mean: f64,
}

fn build_distributional_checks(
    validation_scenario: &SufficientStatisticsMultiIntervalValidationScenario,
    thresholds: &SufficientStatisticsMultiIntervalValidationThresholds,
    path_artifacts: &SufficientStatisticsValidationPathArtifacts,
) -> Result<
    Vec<SufficientStatisticsDistributionalCheck>,
    SufficientStatisticsMultiIntervalValidationError,
> {
    let sample_count_inverse = 1.0 / thresholds.statistical_sample_count as f64;
    let mut statistical_checks: Vec<SufficientStatisticsDistributionalCheck> =
        Vec::with_capacity(path_artifacts.solved_slices.len() * 4);

    for (interval_index, slices) in path_artifacts.solved_slices.iter().copied().enumerate() {
        let expected_means =
            derive_expected_means_from_slices(validation_scenario, interval_index, slices)?;
        let mut cumulative_a_plus = 0.0;
        let mut cumulative_a_minus = 0.0;
        let mut cumulative_n_plus = 0.0;
        let mut cumulative_n_minus = 0.0;

        for sample_position in 0..thresholds.statistical_sample_count {
            let sample_seed = derive_sufficient_statistics_batch_path_seed(
                validation_scenario.run_request.seed,
                sample_position as u64,
            );
            let sampled_statistics = sample_interval_statistics_from_transform_slices(
                sample_seed,
                slices,
            )
            .map_err(|source| {
                SufficientStatisticsMultiIntervalValidationError::TransformSliceSamplingFailure {
                    scenario_id: validation_scenario.scenario_id.clone(),
                    interval_index,
                    source,
                }
            })?;

            cumulative_a_plus += sampled_statistics.a_plus;
            cumulative_a_minus += sampled_statistics.a_minus;
            cumulative_n_plus += sampled_statistics.n_plus as f64;
            cumulative_n_minus += sampled_statistics.n_minus as f64;
        }

        let observed_a_plus_mean = cumulative_a_plus * sample_count_inverse;
        let observed_a_minus_mean = cumulative_a_minus * sample_count_inverse;
        let observed_n_plus_mean = cumulative_n_plus * sample_count_inverse;
        let observed_n_minus_mean = cumulative_n_minus * sample_count_inverse;

        statistical_checks.push(build_distributional_check(
            "a_plus_mean",
            interval_index,
            expected_means.a_plus_mean,
            observed_a_plus_mean,
            thresholds.integrated_intensity_mean_absolute_floor,
            thresholds.integrated_intensity_mean_relative_tolerance,
        ));
        statistical_checks.push(build_distributional_check(
            "a_minus_mean",
            interval_index,
            expected_means.a_minus_mean,
            observed_a_minus_mean,
            thresholds.integrated_intensity_mean_absolute_floor,
            thresholds.integrated_intensity_mean_relative_tolerance,
        ));
        statistical_checks.push(build_distributional_check(
            "n_plus_mean",
            interval_index,
            expected_means.n_plus_mean,
            observed_n_plus_mean,
            thresholds.event_count_mean_absolute_floor,
            thresholds.event_count_mean_relative_tolerance,
        ));
        statistical_checks.push(build_distributional_check(
            "n_minus_mean",
            interval_index,
            expected_means.n_minus_mean,
            observed_n_minus_mean,
            thresholds.event_count_mean_absolute_floor,
            thresholds.event_count_mean_relative_tolerance,
        ));
    }

    Ok(statistical_checks)
}

fn derive_expected_means_from_slices(
    validation_scenario: &SufficientStatisticsMultiIntervalValidationScenario,
    interval_index: usize,
    slices: SufficientStatisticsTransformSlices,
) -> Result<SufficientStatisticsExpectedMeans, SufficientStatisticsMultiIntervalValidationError> {
    let baseline_transform = extract_real_transform_component(
        validation_scenario,
        interval_index,
        "baseline_transform",
        slices.baseline_transform,
    )?;

    let a_plus_transform = extract_real_transform_component(
        validation_scenario,
        interval_index,
        "a_plus_transform",
        slices.a_plus_transform,
    )?;
    let a_minus_transform = extract_real_transform_component(
        validation_scenario,
        interval_index,
        "a_minus_transform",
        slices.a_minus_transform,
    )?;
    let n_plus_transform = extract_real_transform_component(
        validation_scenario,
        interval_index,
        "n_plus_transform",
        slices.n_plus_transform,
    )?;
    let n_minus_transform = extract_real_transform_component(
        validation_scenario,
        interval_index,
        "n_minus_transform",
        slices.n_minus_transform,
    )?;

    let expected_a_plus = normalize_expected_statistic(
        validation_scenario,
        interval_index,
        "a_plus_mean",
        (baseline_transform - a_plus_transform) / slices.settings.a_plus_u,
    )?;
    let expected_a_minus = normalize_expected_statistic(
        validation_scenario,
        interval_index,
        "a_minus_mean",
        (baseline_transform - a_minus_transform) / slices.settings.a_minus_u,
    )?;
    let expected_n_plus = normalize_expected_statistic(
        validation_scenario,
        interval_index,
        "n_plus_mean",
        (n_plus_transform - baseline_transform) / (slices.settings.n_plus_z - 1.0),
    )?;
    let expected_n_minus = normalize_expected_statistic(
        validation_scenario,
        interval_index,
        "n_minus_mean",
        (n_minus_transform - baseline_transform) / (slices.settings.n_minus_z - 1.0),
    )?;

    Ok(SufficientStatisticsExpectedMeans {
        a_plus_mean: expected_a_plus,
        a_minus_mean: expected_a_minus,
        n_plus_mean: expected_n_plus,
        n_minus_mean: expected_n_minus,
    })
}

fn extract_real_transform_component(
    validation_scenario: &SufficientStatisticsMultiIntervalValidationScenario,
    interval_index: usize,
    check_id: &'static str,
    value: crate::nig::ComplexScalar,
) -> Result<f64, SufficientStatisticsMultiIntervalValidationError> {
    if !value.is_finite()
        || value.imaginary.abs()
            > SUFFICIENT_STATISTICS_MULTI_INTERVAL_EXPECTATION_REAL_PART_TOLERANCE
    {
        return Err(
            SufficientStatisticsMultiIntervalValidationError::InvalidExpectedStatistic {
                scenario_id: validation_scenario.scenario_id.clone(),
                interval_index,
                check_id,
                value: value.real,
            },
        );
    }

    Ok(value.real)
}

fn normalize_expected_statistic(
    validation_scenario: &SufficientStatisticsMultiIntervalValidationScenario,
    interval_index: usize,
    check_id: &'static str,
    value: f64,
) -> Result<f64, SufficientStatisticsMultiIntervalValidationError> {
    if !value.is_finite() {
        return Err(
            SufficientStatisticsMultiIntervalValidationError::InvalidExpectedStatistic {
                scenario_id: validation_scenario.scenario_id.clone(),
                interval_index,
                check_id,
                value,
            },
        );
    }

    if value < -SUFFICIENT_STATISTICS_MULTI_INTERVAL_EXPECTATION_NEGATIVE_TOLERANCE {
        return Err(
            SufficientStatisticsMultiIntervalValidationError::InvalidExpectedStatistic {
                scenario_id: validation_scenario.scenario_id.clone(),
                interval_index,
                check_id,
                value,
            },
        );
    }

    Ok(value.max(0.0))
}

fn build_distributional_check(
    check_id: &'static str,
    interval_index: usize,
    expected_mean: f64,
    observed_mean: f64,
    absolute_floor: f64,
    relative_tolerance: f64,
) -> SufficientStatisticsDistributionalCheck {
    let absolute_difference = (expected_mean - observed_mean).abs();
    let absolute_tolerance = absolute_floor + relative_tolerance * expected_mean.abs();
    let passed = absolute_difference.is_finite() && absolute_difference <= absolute_tolerance;

    SufficientStatisticsDistributionalCheck {
        check_id,
        interval_index,
        expected_mean,
        observed_mean,
        absolute_difference,
        absolute_tolerance,
        passed,
    }
}

fn validate_non_negative_threshold(
    field_name: &'static str,
    value: f64,
) -> Result<(), SufficientStatisticsMultiIntervalValidationThresholdsError> {
    if !value.is_finite() || value < 0.0 {
        return Err(
            SufficientStatisticsMultiIntervalValidationThresholdsError::InvalidThresholdValue {
                field_name,
                value,
            },
        );
    }

    Ok(())
}
