//! Cross-scale regression helpers for coarse simulation versus aggregated seeded-tick paths.

use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_engine_simulation::{
    CoarseSimulationPath, SeededTickEventDebugTraceRecord, SeededTickEventDebugTraceTransitionKind,
    SeededTickSimulationRunConfig, SeededTickSimulationRunResult,
};
use janiform_foundations::numeric::arithmetic_mean_or_zero;
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use serde::Serialize;

use super::contracts::RegressionCaseStatus;

const INTERVAL_RATIO_TOLERANCE: f64 = 1.0e-9;
const FLOAT_ZERO_TOLERANCE: f64 = 1.0e-12;

pub const SEEDED_TICK_COARSE_COMPARABLE_PATH_SCHEMA_VERSION: &str =
    "validation.seeded_tick.coarse_comparable_path.v1";
pub const SEEDED_TICK_COARSE_COMPARABLE_DIAGNOSTICS_SCHEMA_VERSION: &str =
    "validation.seeded_tick.coarse_comparable_path.diagnostics.v1";
pub const SEEDED_TICK_COARSE_COMPARABLE_AGGREGATION_POLICY: &str =
    "full_debug_trace_reconstruction_with_exact_partitioned_interval_grouping.v1";
pub const COARSE_SEEDED_TICK_CONSISTENCY_REPORT_SCHEMA_VERSION: &str =
    "validation.coarse_seeded_tick_consistency.report.v1";
pub const COARSE_SEEDED_TICK_CONSISTENCY_DETERMINISM_POLICY: &str =
    "preserve_input_scenario_order_and_metric_order_with_full_trace_reconstruction.v1";
pub const COARSE_SEEDED_TICK_CONSISTENCY_EXPECTATION_POLICY: &str = "exact_alignment_for_interval_structure_and_routing_presence_plus_approximate_and_distributional_numeric_checks.v1";
pub const COARSE_SEEDED_TICK_CONSISTENCY_DRIFT_DIAGNOSTIC_POLICY: &str = "bucket_mismatches_into_structural_alignment_sufficient_statistics_sampling_direct_jump_generation_and_state_carryover.v1";
pub const COARSE_SEEDED_TICK_CONSISTENCY_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "validation.coarse_seeded_tick_consistency.output_schema_metadata.v1";
pub const COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLDS_SCHEMA_VERSION: &str =
    "validation.coarse_seeded_tick_consistency.thresholds.v1";
pub const COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLD_POLICY: &str =
    "exact_gate_checks_plus_absolute_mean_and_wasserstein_distance_tolerances.v1";

const COARSE_SEEDED_TICK_CONSISTENCY_REQUIRED_REPORT_FIELDS: [&str; 8] = [
    "schema_version",
    "determinism_policy",
    "expectation_policy",
    "drift_diagnostic_policy",
    "output_schema_metadata",
    "thresholds",
    "summary",
    "scenario_reports",
];
const COARSE_SEEDED_TICK_CONSISTENCY_REQUIRED_SCENARIO_FIELDS: [&str; 10] = [
    "scenario_id",
    "interval_grid_label",
    "status",
    "paired_path_count",
    "coarse_interval_delta",
    "fine_interval_delta",
    "deterministic_failed_check_count",
    "statistical_failed_check_count",
    "deterministic_checks",
    "statistical_checks",
];
const COARSE_SEEDED_TICK_CONSISTENCY_REQUIRED_DETERMINISTIC_CHECK_FIELDS: [&str; 5] = [
    "check_id",
    "expectation_class",
    "diagnostic_bucket",
    "passed",
    "detail",
];
const COARSE_SEEDED_TICK_CONSISTENCY_REQUIRED_STATISTICAL_CHECK_FIELDS: [&str; 8] = [
    "check_id",
    "expectation_class",
    "diagnostic_bucket",
    "coarse_reference_value",
    "fine_reference_value",
    "observed_distance",
    "tolerance",
    "passed",
];

/// One aggregated fine-path interval reconstructed from seeded-tick debug traces.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededTickCoarseComparableIntervalRecord {
    pub interval_index: usize,
    pub interval_start_utc_nanos: i64,
    pub interval_end_utc_nanos: i64,
    pub a_plus: f64,
    pub a_minus: f64,
    pub n_plus: u64,
    pub n_minus: u64,
    pub buy_side_return: f64,
    pub sell_side_return: f64,
    pub trading_time_return: f64,
    pub open_jump_return: f64,
    pub closed_gap_return: f64,
    pub core_total_return: f64,
    pub observed_interval_return: f64,
    pub observed_log_price_end: f64,
    pub anchor_value_end: f64,
    pub deviation_from_anchor_end: f64,
    pub terminal_excitation_state: HawkesExcitationState,
}

/// Deterministic diagnostics for one aggregated seeded-tick path.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededTickCoarseComparablePathDiagnostics {
    pub schema_version: &'static str,
    pub cumulative_a_plus: f64,
    pub cumulative_a_minus: f64,
    pub cumulative_n_plus: u64,
    pub cumulative_n_minus: u64,
    pub cumulative_buy_side_return: f64,
    pub cumulative_sell_side_return: f64,
    pub cumulative_trading_time_return: f64,
    pub cumulative_open_jump_return: f64,
    pub cumulative_closed_gap_return: f64,
    pub cumulative_core_total_return: f64,
    pub cumulative_observed_return: f64,
    pub max_abs_deviation_from_anchor: f64,
    pub terminal_observed_log_price: f64,
    pub terminal_deviation_from_anchor: f64,
}

/// Coarse-comparable path reconstructed from a seeded-tick run on a finer grid.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeededTickCoarseComparablePath {
    pub schema_version: &'static str,
    pub aggregation_policy: &'static str,
    pub seed: u64,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub fine_subinterval_count_per_coarse_interval: usize,
    pub simulated_interval_count: usize,
    pub start_time_utc_nanos: i64,
    pub end_time_utc_nanos: i64,
    pub initial_excitation_state: HawkesExcitationState,
    pub terminal_excitation_state: HawkesExcitationState,
    pub diagnostics: SeededTickCoarseComparablePathDiagnostics,
    pub records: Vec<SeededTickCoarseComparableIntervalRecord>,
}

/// Reconstruct a coarse-comparable path from a seeded-tick run with full debug traces enabled.
pub fn aggregate_seeded_tick_run_to_coarse_comparable_path(
    run_config: &SeededTickSimulationRunConfig,
    run_result: &SeededTickSimulationRunResult,
    coarse_interval_delta: f64,
) -> Result<SeededTickCoarseComparablePath, SeededTickCoarseAggregationError> {
    let debug_trace = run_result
        .debug_trace_output
        .as_ref()
        .ok_or(SeededTickCoarseAggregationError::MissingDebugTrace)?;

    if debug_trace.metadata.sample_every_nth_transition != 1 {
        return Err(
            SeededTickCoarseAggregationError::UnsupportedDebugTraceSamplingStride {
                value: debug_trace.metadata.sample_every_nth_transition,
            },
        );
    }

    if run_result.simulated_interval_count != run_config.horizon_interval_count {
        return Err(
            SeededTickCoarseAggregationError::RunConfigIntervalCountMismatch {
                configured_interval_count: run_config.horizon_interval_count,
                observed_interval_count: run_result.simulated_interval_count,
            },
        );
    }

    let horizon_interval_count = run_config.horizon_interval_count;
    let fine_interval_delta =
        derive_uniform_interval_delta_hours(run_config, horizon_interval_count)?;
    let fine_subinterval_count_per_coarse_interval =
        resolve_interval_ratio(coarse_interval_delta, fine_interval_delta)?;

    if horizon_interval_count % fine_subinterval_count_per_coarse_interval != 0 {
        return Err(
            SeededTickCoarseAggregationError::FineIntervalCountDoesNotPartitionHorizon {
                fine_interval_count: horizon_interval_count,
                fine_subinterval_count_per_coarse_interval,
            },
        );
    }

    let flat_trace_records = flatten_trace_records(debug_trace);
    let fine_records =
        reconstruct_fine_interval_records(run_config, run_result, &flat_trace_records)?;
    let observed_fine_terminal_state = fine_records
        .last()
        .map(|record| record.terminal_excitation_state)
        .ok_or(SeededTickCoarseAggregationError::MissingFineIntervalRecords)?;
    if observed_fine_terminal_state != run_result.final_excitation_state {
        return Err(SeededTickCoarseAggregationError::FinalStateMismatch);
    }
    let coarse_records = aggregate_fine_records_to_coarse_records(
        &fine_records,
        fine_subinterval_count_per_coarse_interval,
    );
    let diagnostics = derive_seeded_tick_comparable_diagnostics(&coarse_records);
    let start_time_utc_nanos = coarse_records
        .first()
        .map(|record| record.interval_start_utc_nanos)
        .ok_or(SeededTickCoarseAggregationError::MissingFineIntervalRecords)?;
    let end_time_utc_nanos = coarse_records
        .last()
        .map(|record| record.interval_end_utc_nanos)
        .ok_or(SeededTickCoarseAggregationError::MissingFineIntervalRecords)?;
    let terminal_excitation_state = coarse_records
        .last()
        .map(|record| record.terminal_excitation_state)
        .ok_or(SeededTickCoarseAggregationError::MissingFineIntervalRecords)?;

    Ok(SeededTickCoarseComparablePath {
        schema_version: SEEDED_TICK_COARSE_COMPARABLE_PATH_SCHEMA_VERSION,
        aggregation_policy: SEEDED_TICK_COARSE_COMPARABLE_AGGREGATION_POLICY,
        seed: run_result.seed,
        coarse_interval_delta,
        fine_interval_delta,
        fine_subinterval_count_per_coarse_interval,
        simulated_interval_count: coarse_records.len(),
        start_time_utc_nanos,
        end_time_utc_nanos,
        initial_excitation_state: run_config.model_inputs.initial_excitation_state,
        terminal_excitation_state,
        diagnostics,
        records: coarse_records,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum SeededTickCoarseAggregationError {
    MissingDebugTrace,
    UnsupportedDebugTraceSamplingStride {
        value: u64,
    },
    RunConfigIntervalCountMismatch {
        configured_interval_count: usize,
        observed_interval_count: usize,
    },
    InvalidCoarseIntervalDelta {
        value: f64,
    },
    InvalidFineIntervalDelta {
        value: f64,
    },
    FineIntervalCountDoesNotPartitionHorizon {
        fine_interval_count: usize,
        fine_subinterval_count_per_coarse_interval: usize,
    },
    CoarseFineIntervalRatioMismatch {
        coarse_interval_delta: f64,
        fine_interval_delta: f64,
        implied_ratio: f64,
    },
    MissingFineIntervalRecords,
    TraceRecordIntervalOutOfRange {
        interval_index: usize,
        fine_interval_count: usize,
    },
    MissingBoundaryDriftRecord {
        interval_index: usize,
    },
    BoundaryTimestampMismatch {
        interval_index: usize,
        expected_interval_end_utc_nanos: i64,
        observed_timestamp_utc_nanos: i64,
    },
    MissingFinePathTransitionRecord {
        transition_index: u64,
    },
    FinalStateMismatch,
}

impl Display for SeededTickCoarseAggregationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingDebugTrace => write!(
                formatter,
                "seeded-tick run must include debug_trace_output with full transition capture"
            ),
            Self::UnsupportedDebugTraceSamplingStride { value } => write!(
                formatter,
                "debug_trace_output.metadata.sample_every_nth_transition must be 1 for exact aggregation, found {value}"
            ),
            Self::RunConfigIntervalCountMismatch {
                configured_interval_count,
                observed_interval_count,
            } => write!(
                formatter,
                "run_config.horizon_interval_count ({configured_interval_count}) must match run_result.simulated_interval_count ({observed_interval_count})"
            ),
            Self::InvalidCoarseIntervalDelta { value } => write!(
                formatter,
                "coarse_interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidFineIntervalDelta { value } => write!(
                formatter,
                "fine interval delta must be finite and strictly positive, found {value}"
            ),
            Self::FineIntervalCountDoesNotPartitionHorizon {
                fine_interval_count,
                fine_subinterval_count_per_coarse_interval,
            } => write!(
                formatter,
                "fine interval count {fine_interval_count} must be divisible by fine_subinterval_count_per_coarse_interval {fine_subinterval_count_per_coarse_interval}"
            ),
            Self::CoarseFineIntervalRatioMismatch {
                coarse_interval_delta,
                fine_interval_delta,
                implied_ratio,
            } => write!(
                formatter,
                "coarse_interval_delta ({coarse_interval_delta}) must be an exact integer multiple of fine_interval_delta ({fine_interval_delta}), found implied_ratio={implied_ratio}"
            ),
            Self::MissingFineIntervalRecords => {
                write!(
                    formatter,
                    "seeded-tick debug trace produced no fine interval records"
                )
            }
            Self::TraceRecordIntervalOutOfRange {
                interval_index,
                fine_interval_count,
            } => write!(
                formatter,
                "debug trace record interval_index {interval_index} is out of range for fine interval count {fine_interval_count}"
            ),
            Self::MissingBoundaryDriftRecord { interval_index } => write!(
                formatter,
                "fine interval {interval_index} must end with an interval-boundary drift record"
            ),
            Self::BoundaryTimestampMismatch {
                interval_index,
                expected_interval_end_utc_nanos,
                observed_timestamp_utc_nanos,
            } => write!(
                formatter,
                "fine interval {interval_index} boundary timestamp mismatch, expected {expected_interval_end_utc_nanos}, found {observed_timestamp_utc_nanos}"
            ),
            Self::MissingFinePathTransitionRecord { transition_index } => write!(
                formatter,
                "seeded fine_path is missing transition record {transition_index} required for return aggregation"
            ),
            Self::FinalStateMismatch => write!(
                formatter,
                "aggregated fine path terminal state must match run_result.final_excitation_state"
            ),
        }
    }
}

impl Error for SeededTickCoarseAggregationError {}

/// Exact or approximate expectation class attached to one regression comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoarseSeededTickExpectationClass {
    Exact,
    Approximate,
    Distributional,
}

/// Diagnostic bucket for triaging coarse-versus-seeded-tick drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoarseSeededTickDiagnosticBucket {
    StructuralAlignment,
    SufficientStatisticsSampling,
    DirectJumpGeneration,
    StateCarryover,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseSeededTickConsistencyOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub thresholds_schema_version: &'static str,
    pub required_report_fields: Vec<&'static str>,
    pub required_scenario_fields: Vec<&'static str>,
    pub required_deterministic_check_fields: Vec<&'static str>,
    pub required_statistical_check_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseSeededTickDeterministicCheck {
    pub check_id: &'static str,
    pub expectation_class: CoarseSeededTickExpectationClass,
    pub diagnostic_bucket: CoarseSeededTickDiagnosticBucket,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseSeededTickStatisticalCheck {
    pub check_id: &'static str,
    pub expectation_class: CoarseSeededTickExpectationClass,
    pub diagnostic_bucket: CoarseSeededTickDiagnosticBucket,
    pub coarse_reference_value: f64,
    pub fine_reference_value: f64,
    pub observed_distance: f64,
    pub tolerance: f64,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseSeededTickConsistencyScenarioReport {
    pub scenario_id: String,
    pub interval_grid_label: String,
    pub status: RegressionCaseStatus,
    pub paired_path_count: usize,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub deterministic_failed_check_count: usize,
    pub statistical_failed_check_count: usize,
    pub deterministic_checks: Vec<CoarseSeededTickDeterministicCheck>,
    pub statistical_checks: Vec<CoarseSeededTickStatisticalCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoarseSeededTickConsistencySummary {
    pub total_scenario_count: usize,
    pub passed_scenario_count: usize,
    pub failed_scenario_count: usize,
    pub deterministic_failed_check_count: usize,
    pub statistical_failed_check_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseSeededTickConsistencyReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub expectation_policy: &'static str,
    pub drift_diagnostic_policy: &'static str,
    pub output_schema_metadata: CoarseSeededTickConsistencyOutputSchemaMetadata,
    pub thresholds: CoarseSeededTickConsistencyThresholds,
    pub summary: CoarseSeededTickConsistencySummary,
    pub scenario_reports: Vec<CoarseSeededTickConsistencyScenarioReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CoarseSeededTickConsistencyThresholds {
    pub schema_version: &'static str,
    pub threshold_policy: &'static str,
    pub mean_cumulative_a_plus_absolute_tolerance: f64,
    pub mean_cumulative_a_minus_absolute_tolerance: f64,
    pub mean_cumulative_n_plus_absolute_tolerance: f64,
    pub mean_cumulative_n_minus_absolute_tolerance: f64,
    pub mean_cumulative_trading_time_return_absolute_tolerance: f64,
    pub open_jump_wasserstein_distance_tolerance: f64,
    pub closed_gap_wasserstein_distance_tolerance: f64,
    pub total_return_wasserstein_distance_tolerance: f64,
    pub terminal_state_l1_mean_tolerance: f64,
}

impl Default for CoarseSeededTickConsistencyThresholds {
    fn default() -> Self {
        Self {
            schema_version: COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLDS_SCHEMA_VERSION,
            threshold_policy: COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLD_POLICY,
            mean_cumulative_a_plus_absolute_tolerance: 3.0,
            mean_cumulative_a_minus_absolute_tolerance: 3.0,
            mean_cumulative_n_plus_absolute_tolerance: 3.0,
            mean_cumulative_n_minus_absolute_tolerance: 3.0,
            mean_cumulative_trading_time_return_absolute_tolerance: 2.0,
            open_jump_wasserstein_distance_tolerance: 2.0,
            closed_gap_wasserstein_distance_tolerance: 2.0,
            total_return_wasserstein_distance_tolerance: 3.0,
            terminal_state_l1_mean_tolerance: 2.5,
        }
    }
}

impl CoarseSeededTickConsistencyThresholds {
    pub fn validate(self) -> Result<(), CoarseSeededTickConsistencyThresholdsError> {
        if self.schema_version != COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLDS_SCHEMA_VERSION {
            return Err(
                CoarseSeededTickConsistencyThresholdsError::InvalidSchemaVersion {
                    expected: COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLDS_SCHEMA_VERSION,
                    actual: self.schema_version.to_string(),
                },
            );
        }
        if self.threshold_policy != COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLD_POLICY {
            return Err(
                CoarseSeededTickConsistencyThresholdsError::InvalidThresholdPolicy {
                    expected: COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLD_POLICY,
                    actual: self.threshold_policy.to_string(),
                },
            );
        }

        for (field_name, value) in [
            (
                "mean_cumulative_a_plus_absolute_tolerance",
                self.mean_cumulative_a_plus_absolute_tolerance,
            ),
            (
                "mean_cumulative_a_minus_absolute_tolerance",
                self.mean_cumulative_a_minus_absolute_tolerance,
            ),
            (
                "mean_cumulative_n_plus_absolute_tolerance",
                self.mean_cumulative_n_plus_absolute_tolerance,
            ),
            (
                "mean_cumulative_n_minus_absolute_tolerance",
                self.mean_cumulative_n_minus_absolute_tolerance,
            ),
            (
                "mean_cumulative_trading_time_return_absolute_tolerance",
                self.mean_cumulative_trading_time_return_absolute_tolerance,
            ),
            (
                "open_jump_wasserstein_distance_tolerance",
                self.open_jump_wasserstein_distance_tolerance,
            ),
            (
                "closed_gap_wasserstein_distance_tolerance",
                self.closed_gap_wasserstein_distance_tolerance,
            ),
            (
                "total_return_wasserstein_distance_tolerance",
                self.total_return_wasserstein_distance_tolerance,
            ),
            (
                "terminal_state_l1_mean_tolerance",
                self.terminal_state_l1_mean_tolerance,
            ),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(
                    CoarseSeededTickConsistencyThresholdsError::InvalidThresholdValue {
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
pub enum CoarseSeededTickConsistencyThresholdsError {
    InvalidSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    InvalidThresholdPolicy {
        expected: &'static str,
        actual: String,
    },
    InvalidThresholdValue {
        field_name: &'static str,
        value: f64,
    },
}

impl Display for CoarseSeededTickConsistencyThresholdsError {
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
            Self::InvalidThresholdValue { field_name, value } => write!(
                formatter,
                "thresholds.{field_name} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for CoarseSeededTickConsistencyThresholdsError {}

#[derive(Debug, Clone, PartialEq)]
pub struct CoarseSeededTickPairedPathInput {
    pub coarse_path: CoarseSimulationPath,
    pub aggregated_fine_path: SeededTickCoarseComparablePath,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoarseSeededTickConsistencyScenarioInput {
    pub scenario_id: String,
    pub interval_grid_label: String,
    pub paired_paths: Vec<CoarseSeededTickPairedPathInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSeededTickConsistencyReportError {
    InvalidThresholds {
        source: CoarseSeededTickConsistencyThresholdsError,
    },
    MissingScenarios,
    MissingScenarioId,
    MissingIntervalGridLabel {
        scenario_id: String,
    },
    MissingPairedPaths {
        scenario_id: String,
    },
    IntervalDeltaMismatch {
        scenario_id: String,
    },
}

impl Display for CoarseSeededTickConsistencyReportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidThresholds { source } => {
                write!(formatter, "thresholds are invalid: {source}")
            }
            Self::MissingScenarios => write!(formatter, "scenarios must not be empty"),
            Self::MissingScenarioId => write!(formatter, "scenario_id must be a non-empty string"),
            Self::MissingIntervalGridLabel { scenario_id } => write!(
                formatter,
                "scenario `{scenario_id}` must provide interval_grid_label"
            ),
            Self::MissingPairedPaths { scenario_id } => write!(
                formatter,
                "scenario `{scenario_id}` must include at least one paired path"
            ),
            Self::IntervalDeltaMismatch { scenario_id } => write!(
                formatter,
                "scenario `{scenario_id}` must use one consistent coarse_interval_delta and fine_interval_delta across all paired paths"
            ),
        }
    }
}

impl Error for CoarseSeededTickConsistencyReportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidThresholds { source } => Some(source),
            _ => None,
        }
    }
}

/// Build a deterministic regression report for coarse simulation versus aggregated seeded ticks.
pub fn build_coarse_seeded_tick_consistency_report(
    scenarios: &[CoarseSeededTickConsistencyScenarioInput],
    thresholds: CoarseSeededTickConsistencyThresholds,
) -> Result<CoarseSeededTickConsistencyReport, CoarseSeededTickConsistencyReportError> {
    thresholds
        .validate()
        .map_err(|source| CoarseSeededTickConsistencyReportError::InvalidThresholds { source })?;

    if scenarios.is_empty() {
        return Err(CoarseSeededTickConsistencyReportError::MissingScenarios);
    }

    let mut scenario_reports = Vec::with_capacity(scenarios.len());
    for scenario in scenarios {
        scenario_reports.push(build_scenario_report(scenario, thresholds)?);
    }

    let passed_scenario_count = scenario_reports
        .iter()
        .filter(|scenario_report| scenario_report.status == RegressionCaseStatus::Pass)
        .count();
    let deterministic_failed_check_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.deterministic_failed_check_count)
        .sum();
    let statistical_failed_check_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.statistical_failed_check_count)
        .sum();

    Ok(CoarseSeededTickConsistencyReport {
        schema_version: COARSE_SEEDED_TICK_CONSISTENCY_REPORT_SCHEMA_VERSION,
        determinism_policy: COARSE_SEEDED_TICK_CONSISTENCY_DETERMINISM_POLICY,
        expectation_policy: COARSE_SEEDED_TICK_CONSISTENCY_EXPECTATION_POLICY,
        drift_diagnostic_policy: COARSE_SEEDED_TICK_CONSISTENCY_DRIFT_DIAGNOSTIC_POLICY,
        output_schema_metadata: CoarseSeededTickConsistencyOutputSchemaMetadata {
            schema_version: COARSE_SEEDED_TICK_CONSISTENCY_OUTPUT_SCHEMA_METADATA_VERSION,
            thresholds_schema_version: COARSE_SEEDED_TICK_CONSISTENCY_THRESHOLDS_SCHEMA_VERSION,
            required_report_fields: COARSE_SEEDED_TICK_CONSISTENCY_REQUIRED_REPORT_FIELDS.to_vec(),
            required_scenario_fields: COARSE_SEEDED_TICK_CONSISTENCY_REQUIRED_SCENARIO_FIELDS
                .to_vec(),
            required_deterministic_check_fields:
                COARSE_SEEDED_TICK_CONSISTENCY_REQUIRED_DETERMINISTIC_CHECK_FIELDS.to_vec(),
            required_statistical_check_fields:
                COARSE_SEEDED_TICK_CONSISTENCY_REQUIRED_STATISTICAL_CHECK_FIELDS.to_vec(),
        },
        thresholds,
        summary: CoarseSeededTickConsistencySummary {
            total_scenario_count: scenario_reports.len(),
            passed_scenario_count,
            failed_scenario_count: scenario_reports.len() - passed_scenario_count,
            deterministic_failed_check_count,
            statistical_failed_check_count,
        },
        scenario_reports,
    })
}

fn build_scenario_report(
    scenario: &CoarseSeededTickConsistencyScenarioInput,
    thresholds: CoarseSeededTickConsistencyThresholds,
) -> Result<CoarseSeededTickConsistencyScenarioReport, CoarseSeededTickConsistencyReportError> {
    if scenario.scenario_id.trim().is_empty() {
        return Err(CoarseSeededTickConsistencyReportError::MissingScenarioId);
    }
    if scenario.interval_grid_label.trim().is_empty() {
        return Err(
            CoarseSeededTickConsistencyReportError::MissingIntervalGridLabel {
                scenario_id: scenario.scenario_id.clone(),
            },
        );
    }
    if scenario.paired_paths.is_empty() {
        return Err(CoarseSeededTickConsistencyReportError::MissingPairedPaths {
            scenario_id: scenario.scenario_id.clone(),
        });
    }

    let coarse_interval_delta = scenario.paired_paths[0].coarse_path.interval_delta;
    let fine_interval_delta = scenario.paired_paths[0]
        .aggregated_fine_path
        .fine_interval_delta;
    if scenario.paired_paths.iter().any(|paired_path| {
        paired_path.coarse_path.interval_delta != coarse_interval_delta
            || paired_path.aggregated_fine_path.fine_interval_delta != fine_interval_delta
            || paired_path.aggregated_fine_path.coarse_interval_delta != coarse_interval_delta
    }) {
        return Err(
            CoarseSeededTickConsistencyReportError::IntervalDeltaMismatch {
                scenario_id: scenario.scenario_id.clone(),
            },
        );
    }

    let deterministic_checks = build_deterministic_checks(scenario);
    let statistical_checks = build_statistical_checks(scenario, thresholds);
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

    Ok(CoarseSeededTickConsistencyScenarioReport {
        scenario_id: scenario.scenario_id.clone(),
        interval_grid_label: scenario.interval_grid_label.clone(),
        status,
        paired_path_count: scenario.paired_paths.len(),
        coarse_interval_delta,
        fine_interval_delta,
        deterministic_failed_check_count,
        statistical_failed_check_count,
        deterministic_checks,
        statistical_checks,
    })
}

fn build_deterministic_checks(
    scenario: &CoarseSeededTickConsistencyScenarioInput,
) -> Vec<CoarseSeededTickDeterministicCheck> {
    let mut interval_count_detail =
        "all paired paths share the same aggregated interval count".to_string();
    let interval_count_alignment_passed = scenario.paired_paths.iter().all(|paired_path| {
        let passed = paired_path.coarse_path.simulated_interval_count
            == paired_path.aggregated_fine_path.simulated_interval_count;
        if !passed {
            interval_count_detail = format!(
                "coarse interval count {} does not match aggregated fine interval count {}",
                paired_path.coarse_path.simulated_interval_count,
                paired_path.aggregated_fine_path.simulated_interval_count
            );
        }
        passed
    });

    let mut boundary_detail =
        "all paired paths preserve identical coarse interval start and end boundaries".to_string();
    let mut boundary_alignment_passed = true;
    for paired_path in &scenario.paired_paths {
        if paired_path.coarse_path.records.len() != paired_path.aggregated_fine_path.records.len() {
            boundary_alignment_passed = false;
            boundary_detail =
                "coarse and aggregated fine interval counts differ before boundary comparison"
                    .to_string();
            break;
        }

        for (coarse_record, fine_record) in paired_path
            .coarse_path
            .records
            .iter()
            .zip(paired_path.aggregated_fine_path.records.iter())
        {
            if coarse_record.interval_start_utc_nanos != fine_record.interval_start_utc_nanos
                || coarse_record.interval_end_utc_nanos != fine_record.interval_end_utc_nanos
            {
                boundary_alignment_passed = false;
                boundary_detail = format!(
                    "interval {} boundary mismatch coarse=[{}, {}) fine=[{}, {})",
                    coarse_record.interval_index,
                    coarse_record.interval_start_utc_nanos,
                    coarse_record.interval_end_utc_nanos,
                    fine_record.interval_start_utc_nanos,
                    fine_record.interval_end_utc_nanos
                );
                break;
            }
        }

        if !boundary_alignment_passed {
            break;
        }
    }

    let mut routing_detail =
        "all paired paths agree on open-jump and closed-gap component presence per interval"
            .to_string();
    let mut routing_presence_passed = true;
    for paired_path in &scenario.paired_paths {
        if paired_path.coarse_path.records.len() != paired_path.aggregated_fine_path.records.len() {
            routing_presence_passed = false;
            routing_detail =
                "coarse and aggregated fine interval counts differ before routing comparison"
                    .to_string();
            break;
        }

        for (coarse_record, fine_record) in paired_path
            .coarse_path
            .records
            .iter()
            .zip(paired_path.aggregated_fine_path.records.iter())
        {
            let coarse_open_present =
                !is_effectively_zero(coarse_record.return_components.open_jump_return);
            let fine_open_present = !is_effectively_zero(fine_record.open_jump_return);
            let coarse_gap_present =
                !is_effectively_zero(coarse_record.return_components.closed_gap_return);
            let fine_gap_present = !is_effectively_zero(fine_record.closed_gap_return);
            if coarse_open_present != fine_open_present || coarse_gap_present != fine_gap_present {
                routing_presence_passed = false;
                routing_detail = format!(
                    "interval {} routing presence mismatch coarse_open={} fine_open={} coarse_gap={} fine_gap={}",
                    coarse_record.interval_index,
                    coarse_open_present,
                    fine_open_present,
                    coarse_gap_present,
                    fine_gap_present
                );
                break;
            }
        }

        if !routing_presence_passed {
            break;
        }
    }

    vec![
        CoarseSeededTickDeterministicCheck {
            check_id: "interval_count_alignment",
            expectation_class: CoarseSeededTickExpectationClass::Exact,
            diagnostic_bucket: CoarseSeededTickDiagnosticBucket::StructuralAlignment,
            passed: interval_count_alignment_passed,
            detail: interval_count_detail,
        },
        CoarseSeededTickDeterministicCheck {
            check_id: "boundary_alignment",
            expectation_class: CoarseSeededTickExpectationClass::Exact,
            diagnostic_bucket: CoarseSeededTickDiagnosticBucket::StructuralAlignment,
            passed: boundary_alignment_passed,
            detail: boundary_detail,
        },
        CoarseSeededTickDeterministicCheck {
            check_id: "component_routing_presence_alignment",
            expectation_class: CoarseSeededTickExpectationClass::Exact,
            diagnostic_bucket: CoarseSeededTickDiagnosticBucket::DirectJumpGeneration,
            passed: routing_presence_passed,
            detail: routing_detail,
        },
    ]
}

fn build_statistical_checks(
    scenario: &CoarseSeededTickConsistencyScenarioInput,
    thresholds: CoarseSeededTickConsistencyThresholds,
) -> Vec<CoarseSeededTickStatisticalCheck> {
    let coarse_cumulative_a_plus = scenario
        .paired_paths
        .iter()
        .map(|paired_path| paired_path.coarse_path.diagnostics.cumulative_a_plus)
        .collect::<Vec<_>>();

    let fine_cumulative_a_plus = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .diagnostics
                .cumulative_a_plus
        })
        .collect::<Vec<_>>();

    let coarse_cumulative_a_minus = scenario
        .paired_paths
        .iter()
        .map(|paired_path| paired_path.coarse_path.diagnostics.cumulative_a_minus)
        .collect::<Vec<_>>();

    let fine_cumulative_a_minus = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .diagnostics
                .cumulative_a_minus
        })
        .collect::<Vec<_>>();

    let coarse_cumulative_n_plus = scenario
        .paired_paths
        .iter()
        .map(|paired_path| paired_path.coarse_path.diagnostics.cumulative_n_plus as f64)
        .collect::<Vec<_>>();

    let fine_cumulative_n_plus = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .diagnostics
                .cumulative_n_plus as f64
        })
        .collect::<Vec<_>>();

    let coarse_cumulative_n_minus = scenario
        .paired_paths
        .iter()
        .map(|paired_path| paired_path.coarse_path.diagnostics.cumulative_n_minus as f64)
        .collect::<Vec<_>>();

    let fine_cumulative_n_minus = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .diagnostics
                .cumulative_n_minus as f64
        })
        .collect::<Vec<_>>();

    let coarse_open_jump = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .coarse_path
                .diagnostics
                .cumulative_open_jump_return
        })
        .collect::<Vec<_>>();

    let fine_open_jump = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .diagnostics
                .cumulative_open_jump_return
        })
        .collect::<Vec<_>>();

    let coarse_closed_gap = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .coarse_path
                .diagnostics
                .cumulative_closed_gap_return
        })
        .collect::<Vec<_>>();

    let fine_closed_gap = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .diagnostics
                .cumulative_closed_gap_return
        })
        .collect::<Vec<_>>();

    let coarse_trading_time = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .coarse_path
                .diagnostics
                .cumulative_trading_time_return
        })
        .collect::<Vec<_>>();

    let fine_trading_time = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .diagnostics
                .cumulative_trading_time_return
        })
        .collect::<Vec<_>>();

    let coarse_core_total_return = scenario
        .paired_paths
        .iter()
        .map(|paired_path| paired_path.coarse_path.diagnostics.cumulative_total_return)
        .collect::<Vec<_>>();

    let fine_core_total_return = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .diagnostics
                .cumulative_core_total_return
        })
        .collect::<Vec<_>>();

    let coarse_observed_return = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .coarse_path
                .diagnostics
                .cumulative_observed_return
        })
        .collect::<Vec<_>>();

    let fine_observed_return = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .diagnostics
                .cumulative_observed_return
        })
        .collect::<Vec<_>>();

    let coarse_terminal_observed_log_price = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .coarse_path
                .diagnostics
                .terminal_observed_log_price
        })
        .collect::<Vec<_>>();

    let fine_terminal_observed_log_price = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .diagnostics
                .terminal_observed_log_price
        })
        .collect::<Vec<_>>();

    let coarse_terminal_deviation_from_anchor = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .coarse_path
                .diagnostics
                .terminal_deviation_from_anchor
        })
        .collect::<Vec<_>>();

    let fine_terminal_deviation_from_anchor = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            paired_path
                .aggregated_fine_path
                .diagnostics
                .terminal_deviation_from_anchor
        })
        .collect::<Vec<_>>();

    let terminal_state_l1_distances = scenario
        .paired_paths
        .iter()
        .map(|paired_path| {
            hawkes_state_l1_distance(
                paired_path.coarse_path.terminal_excitation_state,
                paired_path.aggregated_fine_path.terminal_excitation_state,
            )
        })
        .collect::<Vec<_>>();

    vec![
        build_mean_delta_check(
            "mean_cumulative_a_plus",
            CoarseSeededTickDiagnosticBucket::SufficientStatisticsSampling,
            &coarse_cumulative_a_plus,
            &fine_cumulative_a_plus,
            thresholds.mean_cumulative_a_plus_absolute_tolerance,
        ),
        build_mean_delta_check(
            "mean_cumulative_a_minus",
            CoarseSeededTickDiagnosticBucket::SufficientStatisticsSampling,
            &coarse_cumulative_a_minus,
            &fine_cumulative_a_minus,
            thresholds.mean_cumulative_a_minus_absolute_tolerance,
        ),
        build_mean_delta_check(
            "mean_cumulative_n_plus",
            CoarseSeededTickDiagnosticBucket::SufficientStatisticsSampling,
            &coarse_cumulative_n_plus,
            &fine_cumulative_n_plus,
            thresholds.mean_cumulative_n_plus_absolute_tolerance,
        ),
        build_mean_delta_check(
            "mean_cumulative_n_minus",
            CoarseSeededTickDiagnosticBucket::SufficientStatisticsSampling,
            &coarse_cumulative_n_minus,
            &fine_cumulative_n_minus,
            thresholds.mean_cumulative_n_minus_absolute_tolerance,
        ),
        build_mean_delta_check(
            "mean_cumulative_trading_time_return",
            CoarseSeededTickDiagnosticBucket::SufficientStatisticsSampling,
            &coarse_trading_time,
            &fine_trading_time,
            thresholds.mean_cumulative_trading_time_return_absolute_tolerance,
        ),
        build_wasserstein_check(
            "open_jump_cumulative_wasserstein_distance",
            CoarseSeededTickDiagnosticBucket::DirectJumpGeneration,
            &coarse_open_jump,
            &fine_open_jump,
            thresholds.open_jump_wasserstein_distance_tolerance,
        ),
        build_wasserstein_check(
            "closed_gap_cumulative_wasserstein_distance",
            CoarseSeededTickDiagnosticBucket::DirectJumpGeneration,
            &coarse_closed_gap,
            &fine_closed_gap,
            thresholds.closed_gap_wasserstein_distance_tolerance,
        ),
        build_wasserstein_check(
            "core_total_return_cumulative_wasserstein_distance",
            CoarseSeededTickDiagnosticBucket::SufficientStatisticsSampling,
            &coarse_core_total_return,
            &fine_core_total_return,
            thresholds.total_return_wasserstein_distance_tolerance,
        ),
        build_wasserstein_check(
            "observed_return_cumulative_wasserstein_distance",
            CoarseSeededTickDiagnosticBucket::StateCarryover,
            &coarse_observed_return,
            &fine_observed_return,
            thresholds.total_return_wasserstein_distance_tolerance,
        ),
        build_mean_delta_check(
            "terminal_observed_log_price_mean_distance",
            CoarseSeededTickDiagnosticBucket::StateCarryover,
            &coarse_terminal_observed_log_price,
            &fine_terminal_observed_log_price,
            thresholds.terminal_state_l1_mean_tolerance,
        ),
        build_mean_delta_check(
            "terminal_deviation_from_anchor_mean_distance",
            CoarseSeededTickDiagnosticBucket::StateCarryover,
            &coarse_terminal_deviation_from_anchor,
            &fine_terminal_deviation_from_anchor,
            thresholds.terminal_state_l1_mean_tolerance,
        ),
        CoarseSeededTickStatisticalCheck {
            check_id: "terminal_state_l1_mean_distance",
            expectation_class: CoarseSeededTickExpectationClass::Approximate,
            diagnostic_bucket: CoarseSeededTickDiagnosticBucket::StateCarryover,
            coarse_reference_value: arithmetic_mean_or_zero(&terminal_state_l1_distances),
            fine_reference_value: 0.0,
            observed_distance: arithmetic_mean_or_zero(&terminal_state_l1_distances),
            tolerance: thresholds.terminal_state_l1_mean_tolerance,
            passed: arithmetic_mean_or_zero(&terminal_state_l1_distances)
                <= thresholds.terminal_state_l1_mean_tolerance,
        },
    ]
}

fn build_mean_delta_check(
    check_id: &'static str,
    diagnostic_bucket: CoarseSeededTickDiagnosticBucket,
    coarse_values: &[f64],
    fine_values: &[f64],
    tolerance: f64,
) -> CoarseSeededTickStatisticalCheck {
    let coarse_mean = arithmetic_mean_or_zero(coarse_values);
    let fine_mean = arithmetic_mean_or_zero(fine_values);

    let observed_distance = (coarse_mean - fine_mean).abs();

    CoarseSeededTickStatisticalCheck {
        check_id,
        expectation_class: CoarseSeededTickExpectationClass::Approximate,
        diagnostic_bucket,
        coarse_reference_value: coarse_mean,
        fine_reference_value: fine_mean,
        observed_distance,
        tolerance,
        passed: observed_distance <= tolerance,
    }
}

fn build_wasserstein_check(
    check_id: &'static str,
    diagnostic_bucket: CoarseSeededTickDiagnosticBucket,
    coarse_values: &[f64],
    fine_values: &[f64],
    tolerance: f64,
) -> CoarseSeededTickStatisticalCheck {
    let coarse_mean = arithmetic_mean_or_zero(coarse_values);
    let fine_mean = arithmetic_mean_or_zero(fine_values);

    let observed_distance = calculate_wasserstein_distance(coarse_values, fine_values);

    CoarseSeededTickStatisticalCheck {
        check_id,
        expectation_class: CoarseSeededTickExpectationClass::Distributional,
        diagnostic_bucket,
        coarse_reference_value: coarse_mean,
        fine_reference_value: fine_mean,
        observed_distance,
        tolerance,
        passed: observed_distance <= tolerance,
    }
}

fn calculate_wasserstein_distance(left: &[f64], right: &[f64]) -> f64 {
    let mut ordered_left = left.to_vec();
    let mut ordered_right = right.to_vec();

    ordered_left.sort_by(f64::total_cmp);
    ordered_right.sort_by(f64::total_cmp);

    ordered_left
        .iter()
        .zip(ordered_right.iter())
        .map(|(left_value, right_value)| (left_value - right_value).abs())
        .sum::<f64>()
        / ordered_left.len() as f64
}

fn reconstruct_fine_interval_records(
    run_config: &SeededTickSimulationRunConfig,
    run_result: &SeededTickSimulationRunResult,
    flat_trace_records: &[&SeededTickEventDebugTraceRecord],
) -> Result<Vec<SeededTickCoarseComparableIntervalRecord>, SeededTickCoarseAggregationError> {
    let horizon_interval_count = run_config.horizon_interval_count;
    let kernel_parameters = run_config.model_inputs.kernel_parameters;
    let lambda_bar = kernel_parameters.lambda_bar;
    let beta = kernel_parameters.beta;
    let contract = &run_config.model_inputs.hourly_factor_contract;

    let fine_path_by_transition_index = run_result
        .fine_path
        .iter()
        .map(|record| (record.transition_index, record))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut interval_trace_records: Vec<Vec<&SeededTickEventDebugTraceRecord>> =
        (0..horizon_interval_count).map(|_| Vec::new()).collect();

    for trace_record in flat_trace_records {
        if trace_record.interval_index >= horizon_interval_count {
            return Err(
                SeededTickCoarseAggregationError::TraceRecordIntervalOutOfRange {
                    interval_index: trace_record.interval_index,
                    fine_interval_count: horizon_interval_count,
                },
            );
        }

        interval_trace_records[trace_record.interval_index].push(*trace_record);
    }

    let mut interval_start_state = run_config.model_inputs.initial_excitation_state;

    let mut cumulative_open_jump_return = 0.0;
    let mut cumulative_closed_gap_return = 0.0;

    let mut previous_observed_log_price = run_config
        .model_inputs
        .initial_mean_reversion_boundary_state
        .observed_log_price;

    let mut fine_records = Vec::with_capacity(horizon_interval_count);

    for interval_index in 0..horizon_interval_count {
        let trace_records = &interval_trace_records[interval_index];
        if trace_records.is_empty() {
            return Err(
                SeededTickCoarseAggregationError::MissingBoundaryDriftRecord { interval_index },
            );
        }

        let interval_start_utc_nanos = contract.interval_start_utc_nanos[interval_index];
        let interval_end_utc_nanos = contract.interval_end_utc_nanos[interval_index];
        let seasonality_factor = contract.s_h[interval_index];

        let mut current_state = interval_start_state;

        let mut a_plus = 0.0;
        let mut a_minus = 0.0;
        let mut n_plus = 0_u64;
        let mut n_minus = 0_u64;

        let mut buy_side_return = 0.0;
        let mut sell_side_return = 0.0;
        let mut trading_time_return = 0.0;
        let mut open_jump_return = 0.0;
        let mut closed_gap_return = 0.0;
        let mut core_total_return = 0.0;

        for trace_record in trace_records {
            let elapsed_hours = trace_record.elapsed_hours_since_previous_update;

            a_plus += integrate_side_intensity(
                current_state,
                lambda_bar,
                beta,
                seasonality_factor,
                elapsed_hours,
                true,
            );
            a_minus += integrate_side_intensity(
                current_state,
                lambda_bar,
                beta,
                seasonality_factor,
                elapsed_hours,
                false,
            );

            match trace_record.transition_kind {
                SeededTickEventDebugTraceTransitionKind::EndogenousBuyEvent => {
                    n_plus = n_plus.saturating_add(1);
                }
                SeededTickEventDebugTraceTransitionKind::EndogenousSellEvent => {
                    n_minus = n_minus.saturating_add(1);
                }
                SeededTickEventDebugTraceTransitionKind::ExogenousNews
                | SeededTickEventDebugTraceTransitionKind::IntervalBoundaryDrift => {}
            }

            let fine_path_record = fine_path_by_transition_index
                .get(&trace_record.transition_index)
                .copied()
                .ok_or(
                    SeededTickCoarseAggregationError::MissingFinePathTransitionRecord {
                        transition_index: trace_record.transition_index,
                    },
                )?;

            buy_side_return += fine_path_record.return_components.buy_side_return;
            sell_side_return += fine_path_record.return_components.sell_side_return;
            trading_time_return += fine_path_record.return_components.trading_time_return;
            open_jump_return += fine_path_record.return_components.open_jump_return;
            closed_gap_return += fine_path_record.return_components.closed_gap_return;
            core_total_return += fine_path_record.return_components.total_return;

            current_state = snapshot_to_excitation_state(trace_record.state_after_transition);
        }

        let boundary_trace_record = trace_records.last().copied().ok_or(
            SeededTickCoarseAggregationError::MissingBoundaryDriftRecord { interval_index },
        )?;
        if boundary_trace_record.transition_kind
            != SeededTickEventDebugTraceTransitionKind::IntervalBoundaryDrift
        {
            return Err(
                SeededTickCoarseAggregationError::MissingBoundaryDriftRecord { interval_index },
            );
        }
        if boundary_trace_record.timestamp_utc_nanos != interval_end_utc_nanos {
            return Err(
                SeededTickCoarseAggregationError::BoundaryTimestampMismatch {
                    interval_index,
                    expected_interval_end_utc_nanos: interval_end_utc_nanos,
                    observed_timestamp_utc_nanos: boundary_trace_record.timestamp_utc_nanos,
                },
            );
        }

        let interval_open_jump_return = boundary_trace_record
            .cumulative_open_window_return_jump_m_t_increment
            - cumulative_open_jump_return;
        let interval_closed_gap_return = boundary_trace_record
            .cumulative_closed_window_gap_g_t_increment
            - cumulative_closed_gap_return;

        cumulative_open_jump_return =
            boundary_trace_record.cumulative_open_window_return_jump_m_t_increment;
        cumulative_closed_gap_return =
            boundary_trace_record.cumulative_closed_window_gap_g_t_increment;

        let boundary_fine_path_record = fine_path_by_transition_index
            .get(&boundary_trace_record.transition_index)
            .copied()
            .ok_or(
                SeededTickCoarseAggregationError::MissingFinePathTransitionRecord {
                    transition_index: boundary_trace_record.transition_index,
                },
            )?;

        let observed_interval_return =
            boundary_fine_path_record.observed_log_price - previous_observed_log_price;

        previous_observed_log_price = boundary_fine_path_record.observed_log_price;

        fine_records.push(SeededTickCoarseComparableIntervalRecord {
            interval_index,
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            a_plus,
            a_minus,
            n_plus,
            n_minus,
            buy_side_return,
            sell_side_return,
            trading_time_return,
            open_jump_return: if is_effectively_zero(open_jump_return) {
                interval_open_jump_return
            } else {
                open_jump_return
            },
            closed_gap_return: if is_effectively_zero(closed_gap_return) {
                interval_closed_gap_return
            } else {
                closed_gap_return
            },
            core_total_return,
            observed_interval_return,
            observed_log_price_end: boundary_fine_path_record.observed_log_price,
            anchor_value_end: boundary_fine_path_record.anchor_value,
            deviation_from_anchor_end: boundary_fine_path_record.deviation_from_anchor,
            terminal_excitation_state: current_state,
        });

        interval_start_state = current_state;
    }

    Ok(fine_records)
}

fn aggregate_fine_records_to_coarse_records(
    fine_records: &[SeededTickCoarseComparableIntervalRecord],
    fine_subinterval_count_per_coarse_interval: usize,
) -> Vec<SeededTickCoarseComparableIntervalRecord> {
    let coarse_interval_count = fine_records.len() / fine_subinterval_count_per_coarse_interval;
    let mut coarse_records = Vec::with_capacity(coarse_interval_count);

    for coarse_interval_index in 0..coarse_interval_count {
        let fine_start = coarse_interval_index * fine_subinterval_count_per_coarse_interval;
        let fine_end = fine_start + fine_subinterval_count_per_coarse_interval;

        let fine_window = &fine_records[fine_start..fine_end];

        let first_record = &fine_window[0];
        let last_record = &fine_window[fine_window.len() - 1];

        coarse_records.push(SeededTickCoarseComparableIntervalRecord {
            interval_index: coarse_interval_index,
            interval_start_utc_nanos: first_record.interval_start_utc_nanos,
            interval_end_utc_nanos: last_record.interval_end_utc_nanos,
            a_plus: fine_window.iter().map(|record| record.a_plus).sum(),
            a_minus: fine_window.iter().map(|record| record.a_minus).sum(),
            n_plus: fine_window
                .iter()
                .map(|record| record.n_plus)
                .fold(0_u64, u64::saturating_add),
            n_minus: fine_window
                .iter()
                .map(|record| record.n_minus)
                .fold(0_u64, u64::saturating_add),
            buy_side_return: fine_window
                .iter()
                .map(|record| record.buy_side_return)
                .sum(),
            sell_side_return: fine_window
                .iter()
                .map(|record| record.sell_side_return)
                .sum(),
            trading_time_return: fine_window
                .iter()
                .map(|record| record.trading_time_return)
                .sum(),
            open_jump_return: fine_window
                .iter()
                .map(|record| record.open_jump_return)
                .sum(),
            closed_gap_return: fine_window
                .iter()
                .map(|record| record.closed_gap_return)
                .sum(),
            core_total_return: fine_window
                .iter()
                .map(|record| record.core_total_return)
                .sum(),
            observed_interval_return: fine_window
                .iter()
                .map(|record| record.observed_interval_return)
                .sum(),
            observed_log_price_end: last_record.observed_log_price_end,
            anchor_value_end: last_record.anchor_value_end,
            deviation_from_anchor_end: last_record.deviation_from_anchor_end,
            terminal_excitation_state: last_record.terminal_excitation_state,
        });
    }

    coarse_records
}

fn derive_seeded_tick_comparable_diagnostics(
    records: &[SeededTickCoarseComparableIntervalRecord],
) -> SeededTickCoarseComparablePathDiagnostics {
    SeededTickCoarseComparablePathDiagnostics {
        schema_version: SEEDED_TICK_COARSE_COMPARABLE_DIAGNOSTICS_SCHEMA_VERSION,
        cumulative_a_plus: records.iter().map(|record| record.a_plus).sum(),
        cumulative_a_minus: records.iter().map(|record| record.a_minus).sum(),
        cumulative_n_plus: records
            .iter()
            .map(|record| record.n_plus)
            .fold(0_u64, u64::saturating_add),
        cumulative_n_minus: records
            .iter()
            .map(|record| record.n_minus)
            .fold(0_u64, u64::saturating_add),
        cumulative_buy_side_return: records.iter().map(|record| record.buy_side_return).sum(),
        cumulative_sell_side_return: records.iter().map(|record| record.sell_side_return).sum(),
        cumulative_trading_time_return: records
            .iter()
            .map(|record| record.trading_time_return)
            .sum(),
        cumulative_open_jump_return: records.iter().map(|record| record.open_jump_return).sum(),
        cumulative_closed_gap_return: records.iter().map(|record| record.closed_gap_return).sum(),
        cumulative_core_total_return: records.iter().map(|record| record.core_total_return).sum(),
        cumulative_observed_return: records
            .iter()
            .map(|record| record.observed_interval_return)
            .sum(),
        max_abs_deviation_from_anchor: records
            .iter()
            .map(|record| record.deviation_from_anchor_end.abs())
            .fold(0.0, f64::max),
        terminal_observed_log_price: records[records.len() - 1].observed_log_price_end,
        terminal_deviation_from_anchor: records[records.len() - 1].deviation_from_anchor_end,
    }
}

fn flatten_trace_records(
    debug_trace: &janiform_engine_simulation::SeededTickEventDebugTraceOutput,
) -> Vec<&SeededTickEventDebugTraceRecord> {
    let mut trace_records = debug_trace
        .trace_chunks
        .iter()
        .flat_map(|trace_chunk| trace_chunk.records.iter())
        .collect::<Vec<_>>();
    trace_records.sort_by_key(|trace_record| trace_record.transition_index);

    trace_records
}

fn derive_uniform_interval_delta_hours(
    run_config: &SeededTickSimulationRunConfig,
    horizon_interval_count: usize,
) -> Result<f64, SeededTickCoarseAggregationError> {
    let contract = &run_config.model_inputs.hourly_factor_contract;

    let first_interval_delta_nanos =
        contract.interval_end_utc_nanos[0] - contract.interval_start_utc_nanos[0];
    if first_interval_delta_nanos <= 0 {
        return Err(SeededTickCoarseAggregationError::InvalidFineIntervalDelta {
            value: duration_nanos_to_hours(first_interval_delta_nanos),
        });
    }

    for interval_index in 1..horizon_interval_count {
        let interval_delta_nanos = contract.interval_end_utc_nanos[interval_index]
            - contract.interval_start_utc_nanos[interval_index];
        if interval_delta_nanos != first_interval_delta_nanos {
            return Err(
                SeededTickCoarseAggregationError::CoarseFineIntervalRatioMismatch {
                    coarse_interval_delta: duration_nanos_to_hours(interval_delta_nanos),
                    fine_interval_delta: duration_nanos_to_hours(first_interval_delta_nanos),
                    implied_ratio: interval_delta_nanos as f64 / first_interval_delta_nanos as f64,
                },
            );
        }
    }

    let fine_interval_delta = duration_nanos_to_hours(first_interval_delta_nanos);
    if !fine_interval_delta.is_finite() || fine_interval_delta <= 0.0 {
        return Err(SeededTickCoarseAggregationError::InvalidFineIntervalDelta {
            value: fine_interval_delta,
        });
    }

    Ok(fine_interval_delta)
}

fn resolve_interval_ratio(
    coarse_interval_delta: f64,
    fine_interval_delta: f64,
) -> Result<usize, SeededTickCoarseAggregationError> {
    if !coarse_interval_delta.is_finite() || coarse_interval_delta <= 0.0 {
        return Err(
            SeededTickCoarseAggregationError::InvalidCoarseIntervalDelta {
                value: coarse_interval_delta,
            },
        );
    }
    if !fine_interval_delta.is_finite() || fine_interval_delta <= 0.0 {
        return Err(SeededTickCoarseAggregationError::InvalidFineIntervalDelta {
            value: fine_interval_delta,
        });
    }

    let implied_ratio = coarse_interval_delta / fine_interval_delta;
    let rounded_ratio = implied_ratio.round();

    if rounded_ratio < 1.0 || (implied_ratio - rounded_ratio).abs() > INTERVAL_RATIO_TOLERANCE {
        return Err(
            SeededTickCoarseAggregationError::CoarseFineIntervalRatioMismatch {
                coarse_interval_delta,
                fine_interval_delta,
                implied_ratio,
            },
        );
    }

    Ok(rounded_ratio as usize)
}

fn integrate_side_intensity(
    state: HawkesExcitationState,
    lambda_bar: f64,
    beta: f64,
    seasonality_factor: f64,
    elapsed_hours: f64,
    plus_side: bool,
) -> f64 {
    if elapsed_hours <= 0.0 {
        return 0.0;
    }

    let integrated_decay_weight = (1.0 - (-beta * elapsed_hours).exp()) / beta;
    let deseasonalized_baseline = lambda_bar * elapsed_hours;

    let endogenous_component = if plus_side {
        (state.y_plus_plus + state.y_plus_minus) * integrated_decay_weight
    } else {
        (state.y_minus_plus + state.y_minus_minus) * integrated_decay_weight
    };

    seasonality_factor * (deseasonalized_baseline + endogenous_component)
}

fn snapshot_to_excitation_state(
    snapshot: janiform_engine_simulation::SeededTickEventStateSnapshot,
) -> HawkesExcitationState {
    HawkesExcitationState {
        y_plus_plus: snapshot.y_plus_plus,
        y_plus_minus: snapshot.y_plus_minus,
        y_minus_plus: snapshot.y_minus_plus,
        y_minus_minus: snapshot.y_minus_minus,
    }
}

fn hawkes_state_l1_distance(left: HawkesExcitationState, right: HawkesExcitationState) -> f64 {
    (left.y_plus_plus - right.y_plus_plus).abs()
        + (left.y_plus_minus - right.y_plus_minus).abs()
        + (left.y_minus_plus - right.y_minus_plus).abs()
        + (left.y_minus_minus - right.y_minus_minus).abs()
}

fn is_effectively_zero(value: f64) -> bool {
    value.abs() <= FLOAT_ZERO_TOLERANCE
}
