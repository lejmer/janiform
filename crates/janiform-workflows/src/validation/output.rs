//! Validation workflow output contracts.

use janiform::validation::stylized_facts::StylizedFactHorizonComparisonReportBundle;
use serde::Serialize;

/// Resolved stylized-facts validation plan schema version.
pub const STYLIZED_FACTS_VALIDATION_PLAN_SCHEMA_VERSION: &str = "validation.stylized_facts.plan.v1";

/// Resolved stylized-facts validation plan.
#[derive(Debug, Clone, Serialize)]
pub struct StylizedFactsValidationPlan {
    /// Plan schema version.
    pub schema_version: &'static str,
    /// Resolved model path when raw-data validation is used.
    pub model_path: Option<String>,
    /// Resolved empirical data path when raw-data validation is used.
    pub empirical_data_path: Option<String>,
    /// Resolved empirical data format.
    pub empirical_data_format: Option<String>,
    /// Resolved empirical timestamp column.
    pub empirical_timestamp_column: Option<String>,
    /// Resolved empirical price column.
    pub empirical_price_column: Option<String>,
    /// First empirical return timestamp after input preparation.
    pub empirical_start_utc_nanos: Option<i64>,
    /// Last empirical return timestamp after input preparation.
    pub empirical_end_utc_nanos: Option<i64>,
    /// Resolved validation output directory.
    pub output_dir: String,
    /// Requested return intervals.
    pub intervals: Vec<StylizedFactsValidationIntervalPlan>,
    /// Seeded simulation path count.
    pub simulation_path_count: usize,
    /// Base simulation seed.
    pub seed: u64,
    /// Resolved CPU count available for parallel validation work.
    pub cpu_count: usize,
    /// Derived per-path simulation seeds.
    pub generated_seeds: Vec<u64>,
    /// Metric families included in the workflow.
    pub metric_families: Vec<&'static str>,
    /// Report decision mode.
    pub decision_mode: StylizedFactsValidationDecisionMode,
    /// Cache level requested by the user.
    pub cache_level: Option<StylizedFactsValidationCacheLevel>,
    /// Cache artifact paths that will be written.
    pub cache_artifacts: Vec<String>,
    /// Report artifact paths that will be written.
    pub report_artifacts: Vec<String>,
    /// Execution mode selected by the command inputs.
    pub execution_mode: StylizedFactsValidationExecutionMode,
}

/// Completed stylized-facts validation workflow output.
pub struct StylizedFactsValidationWorkflowOutput {
    /// Resolved execution plan used for the run.
    pub plan: StylizedFactsValidationPlan,
    /// Generated validation report bundle.
    pub report_bundle: StylizedFactHorizonComparisonReportBundle,
}

/// One resolved return interval in the stylized-facts validation plan.
#[derive(Debug, Clone, Serialize)]
pub struct StylizedFactsValidationIntervalPlan {
    /// User-facing interval label.
    pub label: String,
    /// Interval duration in UTC nanoseconds.
    pub duration_nanos: i64,
    /// Derived interval count when empirical data has been inspected.
    pub interval_count: Option<usize>,
}

/// Validation report decision mode.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactsValidationDecisionMode {
    /// Explicit threshold policy mode.
    ThresholdPolicy,
    /// Statistical bootstrap decision mode.
    StatisticalBootstrap,
}

/// Validation command execution mode.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactsValidationExecutionMode {
    /// Load empirical data and simulate paths.
    RawData,
    /// Use a precomputed comparison request.
    PrecomputedComparison,
}

/// Validation cache level.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactsValidationCacheLevel {
    /// Cache derived inputs and lineage.
    Derived,
    /// Cache derived inputs, lineage, and simulated return observations.
    Full,
}
