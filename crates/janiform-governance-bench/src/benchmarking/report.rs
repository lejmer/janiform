use serde::Serialize;

use super::catalog::BenchmarkWorkloadScale;

pub const ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REPORT_SCHEMA_VERSION: &str =
    "benchmark.oracle_pricer_sufficient_statistics.report.v1";
pub const ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_DETERMINISM_POLICY: &str =
    "canonical_profile_order_and_deterministic_seed_partitioning.v1";
pub const ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_EXECUTION_POLICY: &str =
    "warmup_iterations_excluded_from_measured_latency_and_throughput.v1";
pub const ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_METRIC_POLICY: &str =
    "mean_latency_seconds_and_throughput_operations_per_second.v1";
pub const ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "benchmark.oracle_pricer_sufficient_statistics.output_schema_metadata.v1";

pub(super) const ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REQUIRED_REPORT_FIELDS: [&str; 11] = [
    "schema_version",
    "determinism_policy",
    "execution_policy",
    "metric_policy",
    "output_schema_metadata",
    "catalog_id",
    "catalog_version",
    "policy_id",
    "policy_version",
    "summary",
    "profile_reports",
];
pub(super) const ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REQUIRED_PROFILE_REPORT_FIELDS:
    [&str; 9] = [
    "profile_id",
    "workload_scale",
    "interval_delta",
    "warmup_iteration_count",
    "measured_iteration_count",
    "oracle_solver_metrics",
    "pricing_metrics",
    "sufficient_statistics_sampling_metrics",
    "reproducibility_seed",
];
pub(super) const ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REQUIRED_COMPONENT_METRICS_FIELDS:
    [&str; 7] = [
    "operations_per_iteration",
    "measured_operation_count",
    "measured_elapsed_seconds",
    "mean_latency_seconds",
    "throughput_operations_per_second",
    "invocation_checksum",
    "invocation_count_including_warmup",
];
pub const ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_ORACLE: &str = "oracle_solver";
pub const ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_PRICING: &str =
    "characteristic_function_pricer";
pub const ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_SUFFICIENT_STATISTICS: &str =
    "sufficient_statistics_sampler";
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OraclePricerSufficientStatisticsBenchmarkOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub required_report_fields: Vec<&'static str>,
    pub required_profile_report_fields: Vec<&'static str>,
    pub required_component_metrics_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct OraclePricerSufficientStatisticsBenchmarkComponentMetrics {
    pub operations_per_iteration: usize,
    pub measured_operation_count: usize,
    pub measured_elapsed_seconds: f64,
    pub mean_latency_seconds: f64,
    pub throughput_operations_per_second: f64,
    pub invocation_checksum: f64,
    pub invocation_count_including_warmup: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OraclePricerSufficientStatisticsBenchmarkProfileReport {
    pub profile_id: String,
    pub workload_scale: BenchmarkWorkloadScale,
    pub interval_delta: f64,
    pub warmup_iteration_count: usize,
    pub measured_iteration_count: usize,
    pub oracle_solver_metrics: OraclePricerSufficientStatisticsBenchmarkComponentMetrics,
    pub pricing_metrics: OraclePricerSufficientStatisticsBenchmarkComponentMetrics,
    pub sufficient_statistics_sampling_metrics:
        OraclePricerSufficientStatisticsBenchmarkComponentMetrics,
    pub reproducibility_seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OraclePricerSufficientStatisticsBenchmarkSummary {
    pub profile_count: usize,
    pub oracle_solver_total_operation_count: usize,
    pub pricing_total_operation_count: usize,
    pub sufficient_statistics_sampling_total_operation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OraclePricerSufficientStatisticsBenchmarkReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub execution_policy: &'static str,
    pub metric_policy: &'static str,
    pub output_schema_metadata: OraclePricerSufficientStatisticsBenchmarkOutputSchemaMetadata,
    pub catalog_id: String,
    pub catalog_version: String,
    pub policy_id: String,
    pub policy_version: String,
    pub summary: OraclePricerSufficientStatisticsBenchmarkSummary,
    pub profile_reports: Vec<OraclePricerSufficientStatisticsBenchmarkProfileReport>,
}
