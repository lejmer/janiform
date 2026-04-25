use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Instant;

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformGuardFailureCode, ExponentialAffineTransformInputError,
    ExponentialAffineTransformIntegrationStatus, ExponentialAffineTransformIntegratorMethod,
    ExponentialAffineTransformSolverError, ExponentialAffineTransformSolverSettings,
    ExponentialAffineTransformSolverSettingsError,
    solve_exponential_affine_transform_with_settings,
};
use janiform_engine_pricing::internal::{
    FourierInversionEngineSettingsError, FourierInversionPricingError,
    price_vanilla_option_via_fourier_inversion,
};
use janiform_engine_pricing::{
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType,
    VanillaOptionPricingMarketInputsError,
};
use janiform_engine_sufficient_statistics::{
    SufficientStatisticsTransformSliceContextError,
    SufficientStatisticsTransformSliceSamplingError, SufficientStatisticsTransformSliceSolveError,
    sample_interval_statistics_batch_from_transform_slices,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_foundations::complex_math::complex_exponential;
use janiform_model_kernels::hawkes::HawkesKernelParameterError;
use janiform_model_kernels::news::NewsMarkConfigurationError;

use super::builders::{
    build_equation_specification, build_oracle_transform_input, build_pricing_engine_settings,
    build_pricing_market_inputs, build_sufficient_statistics_transform_slices,
};
use super::catalog::{
    BenchmarkScenarioCatalog, BenchmarkScenarioCatalogLoadError, BenchmarkScenarioWorkloadProfile,
};
use super::policy::{
    OraclePricerSufficientStatisticsBenchmarkExecutionConfig,
    OraclePricerSufficientStatisticsBenchmarkExecutionConfigError,
    OraclePricerSufficientStatisticsBenchmarkPolicy,
    OraclePricerSufficientStatisticsBenchmarkPolicyValidationError,
};
use super::report::{
    ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_DETERMINISM_POLICY,
    ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_EXECUTION_POLICY,
    ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_METRIC_POLICY,
    ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_OUTPUT_SCHEMA_METADATA_VERSION,
    ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REPORT_SCHEMA_VERSION,
    ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REQUIRED_COMPONENT_METRICS_FIELDS,
    ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REQUIRED_PROFILE_REPORT_FIELDS,
    ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REQUIRED_REPORT_FIELDS,
    ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_ORACLE,
    ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_PRICING,
    ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_RISK,
    OraclePricerSufficientStatisticsBenchmarkComponentMetrics,
    OraclePricerSufficientStatisticsBenchmarkOutputSchemaMetadata,
    OraclePricerSufficientStatisticsBenchmarkProfileReport,
    OraclePricerSufficientStatisticsBenchmarkReport,
    OraclePricerSufficientStatisticsBenchmarkSummary,
};
use super::utilities::{
    derive_invocation_seed, fnv1a_64, interpolate_range, splitmix64, unit_interval_from_seed,
};

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerSufficientStatisticsBenchmarkHarnessError {
    InvalidScenarioCatalog {
        source: BenchmarkScenarioCatalogLoadError,
    },
    InvalidBenchmarkPolicy {
        source: OraclePricerSufficientStatisticsBenchmarkPolicyValidationError,
    },
    InvalidExecutionConfig {
        source: OraclePricerSufficientStatisticsBenchmarkExecutionConfigError,
    },
    InvalidResolvedMeasuredIterationCount {
        profile_id: String,
        value: usize,
    },
    OperationCountOverflow {
        profile_id: String,
        component: &'static str,
    },
    InvalidModelKernelParameters {
        profile_id: String,
        source: HawkesKernelParameterError,
    },
    InvalidModelGammaMarkConfiguration {
        profile_id: String,
        source: NewsMarkConfigurationError,
    },
    InvalidOracleSolverSettings {
        profile_id: String,
        source: ExponentialAffineTransformSolverSettingsError,
    },
    InvalidOracleTransformInput {
        profile_id: String,
        source: ExponentialAffineTransformInputError,
    },
    OracleSolverFailure {
        profile_id: String,
        source: ExponentialAffineTransformSolverError,
    },
    OracleSolverIncomplete {
        profile_id: String,
        status: ExponentialAffineTransformIntegrationStatus,
        guard_failure_code: Option<ExponentialAffineTransformGuardFailureCode>,
    },
    InvalidPricingEngineSettings {
        profile_id: String,
        source: FourierInversionEngineSettingsError,
    },
    InvalidPricingMarketInputs {
        profile_id: String,
        source: VanillaOptionPricingMarketInputsError,
    },
    PricingEngineFailure {
        profile_id: String,
        source: FourierInversionPricingError,
    },
    InvalidRiskSliceContext {
        profile_id: String,
        interval_index: usize,
        source: SufficientStatisticsTransformSliceContextError,
    },
    SufficientStatisticsSliceSolveFailure {
        profile_id: String,
        interval_index: usize,
        source: SufficientStatisticsTransformSliceSolveError,
    },
    SufficientStatisticsSliceSamplingFailure {
        profile_id: String,
        source: SufficientStatisticsTransformSliceSamplingError,
    },
    NonFiniteInvocationChecksum {
        profile_id: String,
        component: &'static str,
        value: f64,
    },
    InvalidMetricComputation {
        profile_id: String,
        component: &'static str,
        field: &'static str,
        value: f64,
    },
}

impl Display for OraclePricerSufficientStatisticsBenchmarkHarnessError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidScenarioCatalog { source } => {
                write!(
                    formatter,
                    "benchmark harness requires a valid scenario catalog: {source}"
                )
            }
            Self::InvalidBenchmarkPolicy { source } => {
                write!(
                    formatter,
                    "benchmark harness requires a valid benchmark policy: {source}"
                )
            }
            Self::InvalidExecutionConfig { source } => {
                write!(
                    formatter,
                    "benchmark execution configuration is invalid: {source}"
                )
            }
            Self::InvalidResolvedMeasuredIterationCount { profile_id, value } => write!(
                formatter,
                "profile `{profile_id}` resolved measured_iteration_count must be greater than 0, found {value}"
            ),
            Self::OperationCountOverflow {
                profile_id,
                component,
            } => write!(
                formatter,
                "profile `{profile_id}` component `{component}` operation count overflowed usize"
            ),
            Self::InvalidModelKernelParameters { profile_id, source } => write!(
                formatter,
                "profile `{profile_id}` benchmark model Hawkes kernel parameters are invalid: {source}"
            ),
            Self::InvalidModelGammaMarkConfiguration { profile_id, source } => write!(
                formatter,
                "profile `{profile_id}` benchmark gamma mark configuration is invalid: {source}"
            ),
            Self::InvalidOracleSolverSettings { profile_id, source } => write!(
                formatter,
                "profile `{profile_id}` oracle solver settings are invalid: {source}"
            ),
            Self::InvalidOracleTransformInput { profile_id, source } => write!(
                formatter,
                "profile `{profile_id}` failed to build oracle transform input: {source}"
            ),
            Self::OracleSolverFailure { profile_id, source } => write!(
                formatter,
                "profile `{profile_id}` oracle solver execution failed: {source}"
            ),
            Self::OracleSolverIncomplete {
                profile_id,
                status,
                guard_failure_code,
            } => write!(
                formatter,
                "profile `{profile_id}` oracle solver returned status {status:?} with guard_failure_code {guard_failure_code:?}"
            ),
            Self::InvalidPricingEngineSettings { profile_id, source } => write!(
                formatter,
                "profile `{profile_id}` pricing engine settings are invalid: {source}"
            ),
            Self::InvalidPricingMarketInputs { profile_id, source } => write!(
                formatter,
                "profile `{profile_id}` generated invalid pricing market inputs: {source}"
            ),
            Self::PricingEngineFailure { profile_id, source } => write!(
                formatter,
                "profile `{profile_id}` characteristic-function pricing call failed: {source}"
            ),
            Self::InvalidRiskSliceContext {
                profile_id,
                interval_index,
                source,
            } => write!(
                formatter,
                "profile `{profile_id}` interval {interval_index} failed sufficient-statistics slice context validation: {source}"
            ),
            Self::SufficientStatisticsSliceSolveFailure {
                profile_id,
                interval_index,
                source,
            } => write!(
                formatter,
                "profile `{profile_id}` interval {interval_index} sufficient-statistics slice solve failed: {source}"
            ),
            Self::SufficientStatisticsSliceSamplingFailure { profile_id, source } => write!(
                formatter,
                "profile `{profile_id}` sufficient-statistics slice sampling failed: {source}"
            ),
            Self::NonFiniteInvocationChecksum {
                profile_id,
                component,
                value,
            } => write!(
                formatter,
                "profile `{profile_id}` component `{component}` produced non-finite invocation checksum {value}"
            ),
            Self::InvalidMetricComputation {
                profile_id,
                component,
                field,
                value,
            } => write!(
                formatter,
                "profile `{profile_id}` component `{component}` produced invalid metric `{field}` value {value}"
            ),
        }
    }
}

impl Error for OraclePricerSufficientStatisticsBenchmarkHarnessError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidScenarioCatalog { source } => Some(source),
            Self::InvalidBenchmarkPolicy { source } => Some(source),
            Self::InvalidExecutionConfig { source } => Some(source),
            Self::InvalidModelKernelParameters { source, .. } => Some(source),
            Self::InvalidModelGammaMarkConfiguration { source, .. } => Some(source),
            Self::InvalidOracleSolverSettings { source, .. } => Some(source),
            Self::InvalidOracleTransformInput { source, .. } => Some(source),
            Self::OracleSolverFailure { source, .. } => Some(source),
            Self::InvalidPricingEngineSettings { source, .. } => Some(source),
            Self::InvalidPricingMarketInputs { source, .. } => Some(source),
            Self::PricingEngineFailure { source, .. } => Some(source),
            Self::InvalidRiskSliceContext { source, .. } => Some(source),
            Self::SufficientStatisticsSliceSolveFailure { source, .. } => Some(source),
            Self::SufficientStatisticsSliceSamplingFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedBenchmarkRunLength {
    warmup_iteration_count: usize,
    measured_iteration_count: usize,
}

pub fn run_oracle_pricer_sufficient_statistics_benchmark_harness(
    catalog: &BenchmarkScenarioCatalog,
    benchmark_policy: &OraclePricerSufficientStatisticsBenchmarkPolicy,
    execution_config: OraclePricerSufficientStatisticsBenchmarkExecutionConfig,
) -> Result<
    OraclePricerSufficientStatisticsBenchmarkReport,
    OraclePricerSufficientStatisticsBenchmarkHarnessError,
> {
    benchmark_policy.validate().map_err(|source| {
        OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidBenchmarkPolicy { source }
    })?;
    execution_config.validate().map_err(|source| {
        OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidExecutionConfig { source }
    })?;

    let mut normalized_catalog = catalog.clone();
    normalized_catalog.validate().map_err(|source| {
        OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidScenarioCatalog { source }
    })?;
    normalized_catalog.sort_profiles_canonically();

    let mut profile_reports: Vec<OraclePricerSufficientStatisticsBenchmarkProfileReport> =
        Vec::with_capacity(normalized_catalog.profiles.len());
    let mut oracle_solver_total_operation_count: usize = 0;
    let mut pricing_total_operation_count: usize = 0;
    let mut sufficient_statistics_sampling_total_operation_count: usize = 0;

    for profile in &normalized_catalog.profiles {
        let profile_report = run_profile_benchmark(profile, benchmark_policy, execution_config)?;

        oracle_solver_total_operation_count = oracle_solver_total_operation_count
            .checked_add(
                profile_report
                    .oracle_solver_metrics
                    .measured_operation_count,
            )
            .ok_or_else(|| {
                OraclePricerSufficientStatisticsBenchmarkHarnessError::OperationCountOverflow {
                    profile_id: profile.profile_id.clone(),
                    component: ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_ORACLE,
                }
            })?;
        pricing_total_operation_count = pricing_total_operation_count
            .checked_add(profile_report.pricing_metrics.measured_operation_count)
            .ok_or_else(|| {
                OraclePricerSufficientStatisticsBenchmarkHarnessError::OperationCountOverflow {
                    profile_id: profile.profile_id.clone(),
                    component: ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_PRICING,
                }
            })?;
        sufficient_statistics_sampling_total_operation_count =
            sufficient_statistics_sampling_total_operation_count
                .checked_add(
                    profile_report
                        .sufficient_statistics_sampling_metrics
                        .measured_operation_count,
                )
                .ok_or_else(|| {
                    OraclePricerSufficientStatisticsBenchmarkHarnessError::OperationCountOverflow {
                        profile_id: profile.profile_id.clone(),
                        component: ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_RISK,
                    }
                })?;

        profile_reports.push(profile_report);
    }

    Ok(OraclePricerSufficientStatisticsBenchmarkReport {
        schema_version: ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REPORT_SCHEMA_VERSION,
        determinism_policy: ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_DETERMINISM_POLICY,
        execution_policy: ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_EXECUTION_POLICY,
        metric_policy: ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_METRIC_POLICY,
        output_schema_metadata: OraclePricerSufficientStatisticsBenchmarkOutputSchemaMetadata {
            schema_version:
                ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_OUTPUT_SCHEMA_METADATA_VERSION,
            required_report_fields:
                ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REQUIRED_REPORT_FIELDS.to_vec(),
            required_profile_report_fields:
                ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REQUIRED_PROFILE_REPORT_FIELDS
                    .to_vec(),
            required_component_metrics_fields:
                ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_REQUIRED_COMPONENT_METRICS_FIELDS
                    .to_vec(),
        },
        catalog_id: normalized_catalog.catalog_id,
        catalog_version: normalized_catalog.catalog_version,
        policy_id: benchmark_policy.policy_id.clone(),
        policy_version: benchmark_policy.policy_version.clone(),
        summary: OraclePricerSufficientStatisticsBenchmarkSummary {
            profile_count: profile_reports.len(),
            oracle_solver_total_operation_count,
            pricing_total_operation_count,
            sufficient_statistics_sampling_total_operation_count,
        },
        profile_reports,
    })
}

fn run_profile_benchmark(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerSufficientStatisticsBenchmarkPolicy,
    execution_config: OraclePricerSufficientStatisticsBenchmarkExecutionConfig,
) -> Result<
    OraclePricerSufficientStatisticsBenchmarkProfileReport,
    OraclePricerSufficientStatisticsBenchmarkHarnessError,
> {
    let resolved_run_length = resolve_profile_run_length(profile, execution_config)?;
    let equation_specification = build_equation_specification(profile, benchmark_policy)?;
    let oracle_solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        profile.interval_settings.oracle_solver_step_count,
    )
    .map_err(|source| {
        OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidOracleSolverSettings {
            profile_id: profile.profile_id.clone(),
            source,
        }
    })?;
    let pricing_engine_settings = build_pricing_engine_settings(profile, benchmark_policy)?;

    let profile_seed = splitmix64(
        execution_config
            .reproducibility_seed
            .wrapping_add(fnv1a_64(profile.profile_id.as_bytes())),
    );
    let oracle_component_seed =
        splitmix64(profile_seed ^ benchmark_policy.seed_partition.oracle_component_seed_salt);
    let pricing_component_seed =
        splitmix64(profile_seed ^ benchmark_policy.seed_partition.pricing_component_seed_salt);
    let sufficient_statistics_component_seed = splitmix64(
        profile_seed
            ^ benchmark_policy
                .seed_partition
                .sufficient_statistics_component_seed_salt,
    );

    let sufficient_statistics_slices = build_sufficient_statistics_transform_slices(
        profile,
        benchmark_policy,
        equation_specification,
        oracle_solver_settings,
        sufficient_statistics_component_seed,
    )?;

    let oracle_solver_metrics = run_component_benchmark(
        profile,
        ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_ORACLE,
        resolved_run_length,
        profile.component_workload.oracle_requests_per_iteration,
        |invocation_count_including_warmup| {
            let invocation_seed =
                derive_invocation_seed(oracle_component_seed, invocation_count_including_warmup);
            let transform_input = build_oracle_transform_input(
                profile,
                benchmark_policy,
                equation_specification,
                invocation_seed,
            )?;
            let solve_result = solve_exponential_affine_transform_with_settings(
                transform_input,
                oracle_solver_settings,
            )
            .map_err(|source| {
                OraclePricerSufficientStatisticsBenchmarkHarnessError::OracleSolverFailure {
                    profile_id: profile.profile_id.clone(),
                    source,
                }
            })?;
            if solve_result.status != ExponentialAffineTransformIntegrationStatus::Completed {
                return Err(
                    OraclePricerSufficientStatisticsBenchmarkHarnessError::OracleSolverIncomplete {
                        profile_id: profile.profile_id.clone(),
                        status: solve_result.status,
                        guard_failure_code: solve_result
                            .metadata
                            .guard_failure
                            .map(|guard_failure| guard_failure.code),
                    },
                );
            }
            let output = solve_result.output.ok_or_else(|| {
                OraclePricerSufficientStatisticsBenchmarkHarnessError::OracleSolverIncomplete {
                    profile_id: profile.profile_id.clone(),
                    status: solve_result.status,
                    guard_failure_code: solve_result
                        .metadata
                        .guard_failure
                        .map(|guard_failure| guard_failure.code),
                }
            })?;
            let checksum = output.transform_value.real + output.transform_value.imaginary;
            Ok(checksum)
        },
    )?;

    let pricing_metrics = run_component_benchmark(
        profile,
        ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_PRICING,
        resolved_run_length,
        profile.component_workload.pricing_requests_per_iteration,
        |invocation_count_including_warmup| {
            let invocation_seed =
                derive_invocation_seed(pricing_component_seed, invocation_count_including_warmup);
            let market_inputs = build_pricing_market_inputs(
                profile,
                benchmark_policy,
                invocation_seed,
            )
            .map_err(|source| {
                OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidPricingMarketInputs {
                    profile_id: profile.profile_id.clone(),
                    source,
                }
            })?;
            let payoff_type = if invocation_count_including_warmup.is_multiple_of(2) {
                VanillaOptionPayoffType::Call
            } else {
                VanillaOptionPayoffType::Put
            };
            let payoff = VanillaOptionPayoffConfiguration { payoff_type };
            let volatility = interpolate_range(
                benchmark_policy.pricing_input_ranges.volatility_min,
                benchmark_policy.pricing_input_ranges.volatility_max,
                unit_interval_from_seed(invocation_seed ^ 0x51),
            );

            let maturity_interval = market_inputs.single_maturity_interval().map_err(|source| {
                OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidPricingMarketInputs {
                    profile_id: profile.profile_id.clone(),
                    source,
                }
            })?;

            let drift = (market_inputs.risk_free_rate
                - market_inputs.dividend_yield
                - 0.5 * volatility * volatility)
                * maturity_interval;
            let variance = volatility * volatility * maturity_interval;

            let pricing_result = price_vanilla_option_via_fourier_inversion(
                &market_inputs,
                payoff,
                pricing_engine_settings,
                |argument| {
                    let i_times_argument = ComplexScalar::new(-argument.imaginary, argument.real);
                    let drift_term = i_times_argument.scale(drift);
                    let diffusion_term = argument.square().scale(-0.5 * variance);
                    complex_exponential(drift_term.add(diffusion_term))
                },
            )
            .map_err(|source| {
                OraclePricerSufficientStatisticsBenchmarkHarnessError::PricingEngineFailure {
                    profile_id: profile.profile_id.clone(),
                    source,
                }
            })?;

            Ok(pricing_result.price)
        },
    )?;

    let sufficient_statistics_sampling_metrics = run_component_benchmark(
        profile,
        ORACLE_PRICER_SUFFICIENT_STATISTICS_COMPONENT_RISK,
        resolved_run_length,
        profile
            .component_workload
            .sufficient_statistics_paths_per_iteration,
        |invocation_count_including_warmup| {
            let path_seed = derive_invocation_seed(
                sufficient_statistics_component_seed,
                invocation_count_including_warmup,
            );

            let sampled_statistics =
                sample_interval_statistics_batch_from_transform_slices(path_seed, &sufficient_statistics_slices)
                    .map_err(|source| {
                        OraclePricerSufficientStatisticsBenchmarkHarnessError::SufficientStatisticsSliceSamplingFailure {
                            profile_id: profile.profile_id.clone(),
                            source,
                        }
                    })?;

            let mut checksum = 0.0_f64;

            for interval_statistics in sampled_statistics {
                checksum += interval_statistics.a_plus + interval_statistics.a_minus;
                checksum +=
                    (interval_statistics.n_plus + interval_statistics.n_minus) as f64 * 1.0e-3;
            }

            Ok(checksum)
        },
    )?;

    Ok(OraclePricerSufficientStatisticsBenchmarkProfileReport {
        profile_id: profile.profile_id.clone(),
        workload_scale: profile.workload_scale,
        interval_delta: profile.interval_settings.interval_delta,
        warmup_iteration_count: resolved_run_length.warmup_iteration_count,
        measured_iteration_count: resolved_run_length.measured_iteration_count,
        oracle_solver_metrics,
        pricing_metrics,
        sufficient_statistics_sampling_metrics,
        reproducibility_seed: profile_seed,
    })
}

fn resolve_profile_run_length(
    profile: &BenchmarkScenarioWorkloadProfile,
    execution_config: OraclePricerSufficientStatisticsBenchmarkExecutionConfig,
) -> Result<ResolvedBenchmarkRunLength, OraclePricerSufficientStatisticsBenchmarkHarnessError> {
    let warmup_iteration_count = execution_config
        .warmup_iteration_count_override
        .unwrap_or(profile.run_length.warmup_iteration_count);
    let measured_iteration_count = execution_config
        .measured_iteration_count_override
        .unwrap_or(profile.run_length.measured_iteration_count);

    if measured_iteration_count == 0 {
        return Err(
            OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidResolvedMeasuredIterationCount {
                profile_id: profile.profile_id.clone(),
                value: measured_iteration_count,
            },
        );
    }

    Ok(ResolvedBenchmarkRunLength {
        warmup_iteration_count,
        measured_iteration_count,
    })
}

fn run_component_benchmark<F>(
    profile: &BenchmarkScenarioWorkloadProfile,
    component: &'static str,
    run_length: ResolvedBenchmarkRunLength,
    operations_per_iteration: usize,
    mut run_invocation: F,
) -> Result<
    OraclePricerSufficientStatisticsBenchmarkComponentMetrics,
    OraclePricerSufficientStatisticsBenchmarkHarnessError,
>
where
    F: FnMut(usize) -> Result<f64, OraclePricerSufficientStatisticsBenchmarkHarnessError>,
{
    let warmup_operation_count = run_length
        .warmup_iteration_count
        .checked_mul(operations_per_iteration)
        .ok_or_else(|| {
            OraclePricerSufficientStatisticsBenchmarkHarnessError::OperationCountOverflow {
                profile_id: profile.profile_id.clone(),
                component,
            }
        })?;
    let measured_operation_count = run_length
        .measured_iteration_count
        .checked_mul(operations_per_iteration)
        .ok_or_else(|| {
            OraclePricerSufficientStatisticsBenchmarkHarnessError::OperationCountOverflow {
                profile_id: profile.profile_id.clone(),
                component,
            }
        })?;

    let mut invocation_count_including_warmup: usize = 0;
    let mut invocation_checksum = 0.0_f64;
    for _ in 0..warmup_operation_count {
        invocation_count_including_warmup = invocation_count_including_warmup.saturating_add(1);
        let invocation_result = run_invocation(invocation_count_including_warmup)?;
        if !invocation_result.is_finite() {
            return Err(
                OraclePricerSufficientStatisticsBenchmarkHarnessError::NonFiniteInvocationChecksum {
                    profile_id: profile.profile_id.clone(),
                    component,
                    value: invocation_result,
                },
            );
        }
        invocation_checksum += invocation_result;
    }

    let started_at = Instant::now();
    for _ in 0..measured_operation_count {
        invocation_count_including_warmup = invocation_count_including_warmup.saturating_add(1);
        let invocation_result = run_invocation(invocation_count_including_warmup)?;
        if !invocation_result.is_finite() {
            return Err(
                OraclePricerSufficientStatisticsBenchmarkHarnessError::NonFiniteInvocationChecksum {
                    profile_id: profile.profile_id.clone(),
                    component,
                    value: invocation_result,
                },
            );
        }
        invocation_checksum += invocation_result;
    }
    let measured_elapsed_seconds = started_at.elapsed().as_secs_f64();
    let mean_latency_seconds = measured_elapsed_seconds / measured_operation_count as f64;
    let throughput_operations_per_second =
        measured_operation_count as f64 / measured_elapsed_seconds.max(f64::EPSILON);

    if !measured_elapsed_seconds.is_finite() {
        return Err(
            OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidMetricComputation {
                profile_id: profile.profile_id.clone(),
                component,
                field: "measured_elapsed_seconds",
                value: measured_elapsed_seconds,
            },
        );
    }
    if !mean_latency_seconds.is_finite() {
        return Err(
            OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidMetricComputation {
                profile_id: profile.profile_id.clone(),
                component,
                field: "mean_latency_seconds",
                value: mean_latency_seconds,
            },
        );
    }
    if !throughput_operations_per_second.is_finite() {
        return Err(
            OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidMetricComputation {
                profile_id: profile.profile_id.clone(),
                component,
                field: "throughput_operations_per_second",
                value: throughput_operations_per_second,
            },
        );
    }
    if !invocation_checksum.is_finite() {
        return Err(
            OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidMetricComputation {
                profile_id: profile.profile_id.clone(),
                component,
                field: "invocation_checksum",
                value: invocation_checksum,
            },
        );
    }

    Ok(OraclePricerSufficientStatisticsBenchmarkComponentMetrics {
        operations_per_iteration,
        measured_operation_count,
        measured_elapsed_seconds,
        mean_latency_seconds,
        throughput_operations_per_second,
        invocation_checksum,
        invocation_count_including_warmup,
    })
}
