//! Sufficient statistics multi-interval validation suite.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_engine_risk::{
    SufficientStatisticsBoundaryStateUpdateError, SufficientStatisticsBoundaryStateUpdateSnapshot,
    SufficientStatisticsIntervalStatistics, SufficientStatisticsRiskRunRequest,
    SufficientStatisticsRiskRunRequestError, SufficientStatisticsTransformSliceContext,
    SufficientStatisticsTransformSliceSamplingError, SufficientStatisticsTransformSliceSettings,
    SufficientStatisticsTransformSliceSolveError, SufficientStatisticsTransformSlices,
    derive_sufficient_statistics_batch_path_seed, sample_interval_statistics_from_transform_slices,
    solve_sufficient_statistics_transform_slices, update_boundary_state_from_interval_statistics,
};
use janiform_foundations::numeric::validate_non_negative_number;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use serde::Serialize;

use super::contracts::RegressionCaseStatus;

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
            run_request
                .interval_factors
                .news_intensity_input_at(interval_index),
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
    value: janiform_foundations::complex_math::ComplexScalar,
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
    validate_non_negative_number(value, field_name).map_err(|source| {
        SufficientStatisticsMultiIntervalValidationThresholdsError::InvalidThresholdValue {
            field_name: source.field,
            value: source.value,
        }
    })
}
