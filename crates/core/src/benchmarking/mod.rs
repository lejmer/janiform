//! Benchmark scenario catalog contracts and loader validation.

mod builders;
mod catalog;
mod harness;
mod microbench;
mod policy;
mod report;
mod utilities;

pub use catalog::{
    BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_ALGORITHM_POLICY,
    BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_SCOPE_POLICY,
    BENCHMARK_SCENARIO_CATALOG_FORWARD_EXTENSION_POLICY,
    BENCHMARK_SCENARIO_CATALOG_PROFILE_LOADING_ORDER_POLICY,
    BENCHMARK_SCENARIO_CATALOG_SCHEMA_FAMILY, BENCHMARK_SCENARIO_CATALOG_SCHEMA_GENERATION,
    BENCHMARK_SCENARIO_CATALOG_SCHEMA_VERSION, BenchmarkScenarioCatalog,
    BenchmarkScenarioCatalogDeterminismMetadata, BenchmarkScenarioCatalogLoadError,
    BenchmarkScenarioCatalogSchemaMetadata, BenchmarkScenarioComponentWorkload,
    BenchmarkScenarioIntervalSettings, BenchmarkScenarioModelState,
    BenchmarkScenarioPortfolioShape, BenchmarkScenarioRunLength, BenchmarkScenarioSharedNewsState,
    BenchmarkScenarioWorkloadProfile, BenchmarkWorkloadScale,
    load_benchmark_scenario_catalog_from_json,
};
pub use harness::{
    OraclePricerRiskBenchmarkHarnessError, run_oracle_pricer_risk_benchmark_harness,
};
pub use microbench::{
    MICROBENCHMARK_REPRODUCIBILITY_SEED, MicrobenchmarkFixtureError, MicrobenchmarkFixtureSet,
    ORACLE_SOLVER_MICROBENCHMARK_PROFILE_ID, PRICING_KERNEL_MICROBENCHMARK_PROFILE_ID,
    PreparedOracleSolverBenchmarkCase, PreparedPricingKernelBenchmarkCase,
    PreparedRiskSamplingBenchmarkCase, PreparedSimulationPrimitiveBenchmarkCase,
    RISK_SAMPLING_MICROBENCHMARK_PROFILE_ID, SIMULATION_PRIMITIVE_MICROBENCHMARK_PROFILE_ID,
    load_microbenchmark_fixture_set,
};
pub use policy::{
    ORACLE_PRICER_RISK_BENCHMARK_POLICY_SCHEMA_VERSION, OraclePricerRiskBenchmarkExecutionConfig,
    OraclePricerRiskBenchmarkExecutionConfigError, OraclePricerRiskBenchmarkModelParameters,
    OraclePricerRiskBenchmarkOracleSolverProbes, OraclePricerRiskBenchmarkPolicy,
    OraclePricerRiskBenchmarkPolicyLoadError, OraclePricerRiskBenchmarkPolicyRationale,
    OraclePricerRiskBenchmarkPolicyValidationError, OraclePricerRiskBenchmarkPricingInputRanges,
    OraclePricerRiskBenchmarkSeedPartition, load_oracle_pricer_risk_benchmark_policy_from_json,
};
pub use report::{
    ORACLE_PRICER_RISK_BENCHMARK_DETERMINISM_POLICY, ORACLE_PRICER_RISK_BENCHMARK_EXECUTION_POLICY,
    ORACLE_PRICER_RISK_BENCHMARK_METRIC_POLICY,
    ORACLE_PRICER_RISK_BENCHMARK_OUTPUT_SCHEMA_METADATA_VERSION,
    ORACLE_PRICER_RISK_BENCHMARK_REPORT_SCHEMA_VERSION, OraclePricerRiskBenchmarkComponentMetrics,
    OraclePricerRiskBenchmarkOutputSchemaMetadata, OraclePricerRiskBenchmarkProfileReport,
    OraclePricerRiskBenchmarkReport, OraclePricerRiskBenchmarkSummary,
};

pub(crate) use builders::{
    build_equation_specification, build_oracle_transform_input, build_pricing_engine_settings,
    build_pricing_market_inputs,
};
pub(crate) use catalog::{CanonicalProfileRecord, sort_profile_records_canonically};
pub(crate) use report::{
    ORACLE_PRICER_RISK_COMPONENT_ORACLE, ORACLE_PRICER_RISK_COMPONENT_PRICING,
    ORACLE_PRICER_RISK_COMPONENT_RISK,
};
pub(crate) use utilities::{
    derive_invocation_seed, fnv1a_64, interpolate_range, splitmix64, unit_interval_from_seed,
};
