//! Benchmark scenario catalog contracts and loader validation.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformGuardFailureCode,
    ExponentialAffineTransformInput, ExponentialAffineTransformInputError,
    ExponentialAffineTransformIntegrationStatus, ExponentialAffineTransformIntegratorMethod,
    ExponentialAffineTransformSolverError, ExponentialAffineTransformSolverSettings,
    ExponentialAffineTransformSolverSettingsError,
    solve_exponential_affine_transform_with_settings,
};
use crate::complex_math::complex_exponential;
use crate::hawkes::HawkesExcitationState;
use crate::hawkes::{HawkesKernelLoadings, HawkesKernelParameterError, HawkesKernelParameters};
use crate::news::{GammaSeverityMarkConfiguration, NewsMarkConfigurationError};
use crate::nig::ComplexScalar;
use crate::pricing::{
    FourierInversionContour, FourierInversionEngineSettings, FourierInversionEngineSettingsError,
    FourierInversionNumericSettings, FourierInversionPricingError, FourierInversionQuadratureRule,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingMarketInputsError, price_vanilla_option_via_fourier_inversion,
};
use crate::risk::{
    SufficientStatisticsTransformSliceContext, SufficientStatisticsTransformSliceContextError,
    SufficientStatisticsTransformSliceSamplingError, SufficientStatisticsTransformSliceSettings,
    SufficientStatisticsTransformSliceSolveError, SufficientStatisticsTransformSlices,
    sample_interval_statistics_batch_from_transform_slices,
    solve_sufficient_statistics_transform_slices_with_solver_settings,
};

pub const BENCHMARK_SCENARIO_CATALOG_SCHEMA_VERSION: &str = "benchmark.scenario.catalog.v1";
pub const BENCHMARK_SCENARIO_CATALOG_SCHEMA_FAMILY: &str = "benchmark.scenario.catalog";
pub const BENCHMARK_SCENARIO_CATALOG_SCHEMA_GENERATION: &str = "v0.x";
pub const BENCHMARK_SCENARIO_CATALOG_FORWARD_EXTENSION_POLICY: &str = "additive_fields_only.v1";
pub const BENCHMARK_SCENARIO_CATALOG_PROFILE_LOADING_ORDER_POLICY: &str =
    "workload_scale_then_profile_id_ascending.v1";
pub const BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_SCOPE_POLICY: &str = "document_payload_bytes.v1";
pub const BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_ALGORITHM_POLICY: &str = "fnv1a_64_hex_utf8.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkWorkloadScale {
    Small,
    Medium,
    Large,
}

impl BenchmarkWorkloadScale {
    const fn ordering_rank(self) -> u8 {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
        }
    }

    fn required_order() -> [Self; 3] {
        [Self::Small, Self::Medium, Self::Large]
    }
}

impl Display for BenchmarkWorkloadScale {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Small => write!(formatter, "small"),
            Self::Medium => write!(formatter, "medium"),
            Self::Large => write!(formatter, "large"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkScenarioCatalogSchemaMetadata {
    pub schema_family: String,
    pub schema_generation: String,
    pub forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkScenarioCatalogDeterminismMetadata {
    pub profile_loading_order: String,
    pub content_hash_scope: String,
    pub content_hash_algorithm: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkScenarioSharedNewsState {
    pub j_t: f64,
    pub g_t: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkScenarioModelState {
    pub baseline_deseasonalized_intensity_lambda_bar: f64,
    pub initial_excitation_state: HawkesExcitationState,
    pub initial_shared_news_state: BenchmarkScenarioSharedNewsState,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkScenarioIntervalSettings {
    pub interval_delta: f64,
    pub horizon_interval_count: usize,
    pub oracle_solver_step_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkScenarioPortfolioShape {
    pub underlying_count: usize,
    pub maturity_bucket_count_per_underlying: usize,
    pub strikes_per_maturity_bucket: usize,
    pub option_position_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkScenarioRunLength {
    pub warmup_iteration_count: usize,
    pub measured_iteration_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkScenarioComponentWorkload {
    pub oracle_requests_per_iteration: usize,
    pub pricing_requests_per_iteration: usize,
    pub risk_paths_per_iteration: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkScenarioWorkloadProfile {
    pub profile_id: String,
    pub workload_scale: BenchmarkWorkloadScale,
    pub model_state: BenchmarkScenarioModelState,
    pub interval_settings: BenchmarkScenarioIntervalSettings,
    pub portfolio_shape: BenchmarkScenarioPortfolioShape,
    pub run_length: BenchmarkScenarioRunLength,
    pub component_workload: BenchmarkScenarioComponentWorkload,
    pub tags: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkScenarioCatalog {
    pub schema_version: String,
    pub catalog_id: String,
    pub catalog_version: String,
    pub schema_metadata: BenchmarkScenarioCatalogSchemaMetadata,
    pub determinism_metadata: BenchmarkScenarioCatalogDeterminismMetadata,
    pub profiles: Vec<BenchmarkScenarioWorkloadProfile>,
}

impl BenchmarkScenarioCatalog {
    fn validate(&self) -> Result<(), BenchmarkScenarioCatalogLoadError> {
        if self.schema_version != BENCHMARK_SCENARIO_CATALOG_SCHEMA_VERSION {
            return Err(
                BenchmarkScenarioCatalogLoadError::UnsupportedSchemaVersion {
                    expected: BENCHMARK_SCENARIO_CATALOG_SCHEMA_VERSION,
                    actual: self.schema_version.clone(),
                },
            );
        }

        validate_metadata_field(
            "schema_metadata.schema_family",
            BENCHMARK_SCENARIO_CATALOG_SCHEMA_FAMILY,
            &self.schema_metadata.schema_family,
        )?;
        validate_metadata_field(
            "schema_metadata.schema_generation",
            BENCHMARK_SCENARIO_CATALOG_SCHEMA_GENERATION,
            &self.schema_metadata.schema_generation,
        )?;
        validate_metadata_field(
            "schema_metadata.forward_extension_policy",
            BENCHMARK_SCENARIO_CATALOG_FORWARD_EXTENSION_POLICY,
            &self.schema_metadata.forward_extension_policy,
        )?;
        validate_metadata_field(
            "determinism_metadata.profile_loading_order",
            BENCHMARK_SCENARIO_CATALOG_PROFILE_LOADING_ORDER_POLICY,
            &self.determinism_metadata.profile_loading_order,
        )?;
        validate_metadata_field(
            "determinism_metadata.content_hash_scope",
            BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_SCOPE_POLICY,
            &self.determinism_metadata.content_hash_scope,
        )?;
        validate_metadata_field(
            "determinism_metadata.content_hash_algorithm",
            BENCHMARK_SCENARIO_CATALOG_CONTENT_HASH_ALGORITHM_POLICY,
            &self.determinism_metadata.content_hash_algorithm,
        )?;

        if self.catalog_id.trim().is_empty() {
            return Err(BenchmarkScenarioCatalogLoadError::EmptyCatalogId);
        }
        if self.catalog_version.trim().is_empty() {
            return Err(BenchmarkScenarioCatalogLoadError::EmptyCatalogVersion);
        }
        if self.profiles.is_empty() {
            return Err(BenchmarkScenarioCatalogLoadError::EmptyProfiles);
        }

        let mut observed_profile_ids: BTreeSet<String> = BTreeSet::new();
        let mut observed_scales: BTreeSet<BenchmarkWorkloadScale> = BTreeSet::new();

        for (profile_index, profile) in self.profiles.iter().enumerate() {
            if profile.profile_id.trim().is_empty() {
                return Err(BenchmarkScenarioCatalogLoadError::EmptyProfileId { profile_index });
            }
            if !observed_profile_ids.insert(profile.profile_id.clone()) {
                return Err(BenchmarkScenarioCatalogLoadError::DuplicateProfileId {
                    profile_id: profile.profile_id.clone(),
                });
            }
            observed_scales.insert(profile.workload_scale);

            validate_profile(profile)?;
        }

        let missing_scales: Vec<BenchmarkWorkloadScale> = BenchmarkWorkloadScale::required_order()
            .into_iter()
            .filter(|scale| !observed_scales.contains(scale))
            .collect();

        if !missing_scales.is_empty() {
            return Err(
                BenchmarkScenarioCatalogLoadError::MissingWorkloadScaleCoverage { missing_scales },
            );
        }

        Ok(())
    }

    fn sort_profiles_canonically(&mut self) {
        self.profiles.sort_by(|left, right| {
            left.workload_scale
                .ordering_rank()
                .cmp(&right.workload_scale.ordering_rank())
                .then_with(|| left.profile_id.cmp(&right.profile_id))
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BenchmarkScenarioCatalogLoadError {
    InvalidJson {
        source: String,
    },
    UnsupportedSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    InvalidMetadataField {
        field_name: &'static str,
        expected: &'static str,
        actual: String,
    },
    EmptyCatalogId,
    EmptyCatalogVersion,
    EmptyProfiles,
    EmptyProfileId {
        profile_index: usize,
    },
    DuplicateProfileId {
        profile_id: String,
    },
    EmptyProfileDescription {
        profile_id: String,
    },
    EmptyProfileTags {
        profile_id: String,
    },
    InvalidPositiveFiniteValue {
        profile_id: String,
        field_name: &'static str,
        value: f64,
    },
    InvalidNonNegativeFiniteValue {
        profile_id: String,
        field_name: &'static str,
        value: f64,
    },
    InvalidPositiveCount {
        profile_id: String,
        field_name: &'static str,
        value: usize,
    },
    PortfolioShapeCapacityOverflow {
        profile_id: String,
    },
    OptionPositionCountExceedsShapeCapacity {
        profile_id: String,
        option_position_count: usize,
        shape_capacity: usize,
    },
    MissingWorkloadScaleCoverage {
        missing_scales: Vec<BenchmarkWorkloadScale>,
    },
}

impl Display for BenchmarkScenarioCatalogLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson { source } => {
                write!(
                    formatter,
                    "benchmark scenario catalog payload must be valid JSON: {source}"
                )
            }
            Self::UnsupportedSchemaVersion { expected, actual } => write!(
                formatter,
                "schema_version must be `{expected}`, found `{actual}`"
            ),
            Self::InvalidMetadataField {
                field_name,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_name} must be `{expected}`, found `{actual}`"
            ),
            Self::EmptyCatalogId => write!(formatter, "catalog_id must be non-empty"),
            Self::EmptyCatalogVersion => write!(formatter, "catalog_version must be non-empty"),
            Self::EmptyProfiles => write!(formatter, "profiles must contain at least one profile"),
            Self::EmptyProfileId { profile_index } => write!(
                formatter,
                "profiles[{profile_index}].profile_id must be non-empty"
            ),
            Self::DuplicateProfileId { profile_id } => {
                write!(formatter, "profile_id `{profile_id}` is duplicated")
            }
            Self::EmptyProfileDescription { profile_id } => write!(
                formatter,
                "profile `{profile_id}` description must be non-empty"
            ),
            Self::EmptyProfileTags { profile_id } => {
                write!(
                    formatter,
                    "profile `{profile_id}` tags must contain at least one tag"
                )
            }
            Self::InvalidPositiveFiniteValue {
                profile_id,
                field_name,
                value,
            } => write!(
                formatter,
                "profile `{profile_id}` field `{field_name}` must be finite and strictly positive, found {value}"
            ),
            Self::InvalidNonNegativeFiniteValue {
                profile_id,
                field_name,
                value,
            } => write!(
                formatter,
                "profile `{profile_id}` field `{field_name}` must be finite and non-negative, found {value}"
            ),
            Self::InvalidPositiveCount {
                profile_id,
                field_name,
                value,
            } => write!(
                formatter,
                "profile `{profile_id}` field `{field_name}` must be greater than 0, found {value}"
            ),
            Self::PortfolioShapeCapacityOverflow { profile_id } => write!(
                formatter,
                "profile `{profile_id}` portfolio_shape capacity overflowed usize during validation"
            ),
            Self::OptionPositionCountExceedsShapeCapacity {
                profile_id,
                option_position_count,
                shape_capacity,
            } => write!(
                formatter,
                "profile `{profile_id}` option_position_count {option_position_count} exceeds shape capacity {shape_capacity}"
            ),
            Self::MissingWorkloadScaleCoverage { missing_scales } => {
                let joined_scales = missing_scales
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<String>>()
                    .join(", ");
                write!(
                    formatter,
                    "profiles must include at least one workload profile for each scale (missing: {joined_scales})"
                )
            }
        }
    }
}

impl Error for BenchmarkScenarioCatalogLoadError {}

pub fn load_benchmark_scenario_catalog_from_json(
    catalog_payload: &str,
) -> Result<BenchmarkScenarioCatalog, BenchmarkScenarioCatalogLoadError> {
    let mut catalog: BenchmarkScenarioCatalog =
        serde_json::from_str(catalog_payload).map_err(|source| {
            BenchmarkScenarioCatalogLoadError::InvalidJson {
                source: source.to_string(),
            }
        })?;

    catalog.validate()?;
    catalog.sort_profiles_canonically();

    Ok(catalog)
}

fn validate_metadata_field(
    field_name: &'static str,
    expected: &'static str,
    actual: &str,
) -> Result<(), BenchmarkScenarioCatalogLoadError> {
    if actual == expected {
        return Ok(());
    }

    Err(BenchmarkScenarioCatalogLoadError::InvalidMetadataField {
        field_name,
        expected,
        actual: actual.to_string(),
    })
}

fn validate_profile(
    profile: &BenchmarkScenarioWorkloadProfile,
) -> Result<(), BenchmarkScenarioCatalogLoadError> {
    if profile.description.trim().is_empty() {
        return Err(BenchmarkScenarioCatalogLoadError::EmptyProfileDescription {
            profile_id: profile.profile_id.clone(),
        });
    }
    if profile.tags.is_empty() {
        return Err(BenchmarkScenarioCatalogLoadError::EmptyProfileTags {
            profile_id: profile.profile_id.clone(),
        });
    }

    validate_positive_finite_field(
        profile,
        "model_state.baseline_deseasonalized_intensity_lambda_bar",
        profile
            .model_state
            .baseline_deseasonalized_intensity_lambda_bar,
    )?;

    let excitation_state = profile.model_state.initial_excitation_state;
    validate_non_negative_finite_field(
        profile,
        "model_state.initial_excitation_state.y_plus_plus",
        excitation_state.y_plus_plus,
    )?;
    validate_non_negative_finite_field(
        profile,
        "model_state.initial_excitation_state.y_plus_minus",
        excitation_state.y_plus_minus,
    )?;
    validate_non_negative_finite_field(
        profile,
        "model_state.initial_excitation_state.y_minus_plus",
        excitation_state.y_minus_plus,
    )?;
    validate_non_negative_finite_field(
        profile,
        "model_state.initial_excitation_state.y_minus_minus",
        excitation_state.y_minus_minus,
    )?;

    validate_non_negative_finite_field(
        profile,
        "model_state.initial_shared_news_state.j_t",
        profile.model_state.initial_shared_news_state.j_t,
    )?;
    validate_non_negative_finite_field(
        profile,
        "model_state.initial_shared_news_state.g_t",
        profile.model_state.initial_shared_news_state.g_t,
    )?;

    validate_positive_finite_field(
        profile,
        "interval_settings.interval_delta",
        profile.interval_settings.interval_delta,
    )?;
    validate_positive_count_field(
        profile,
        "interval_settings.horizon_interval_count",
        profile.interval_settings.horizon_interval_count,
    )?;
    validate_positive_count_field(
        profile,
        "interval_settings.oracle_solver_step_count",
        profile.interval_settings.oracle_solver_step_count,
    )?;

    validate_positive_count_field(
        profile,
        "portfolio_shape.underlying_count",
        profile.portfolio_shape.underlying_count,
    )?;
    validate_positive_count_field(
        profile,
        "portfolio_shape.maturity_bucket_count_per_underlying",
        profile.portfolio_shape.maturity_bucket_count_per_underlying,
    )?;
    validate_positive_count_field(
        profile,
        "portfolio_shape.strikes_per_maturity_bucket",
        profile.portfolio_shape.strikes_per_maturity_bucket,
    )?;
    validate_positive_count_field(
        profile,
        "portfolio_shape.option_position_count",
        profile.portfolio_shape.option_position_count,
    )?;

    validate_positive_count_field(
        profile,
        "run_length.warmup_iteration_count",
        profile.run_length.warmup_iteration_count,
    )?;
    validate_positive_count_field(
        profile,
        "run_length.measured_iteration_count",
        profile.run_length.measured_iteration_count,
    )?;

    validate_positive_count_field(
        profile,
        "component_workload.oracle_requests_per_iteration",
        profile.component_workload.oracle_requests_per_iteration,
    )?;
    validate_positive_count_field(
        profile,
        "component_workload.pricing_requests_per_iteration",
        profile.component_workload.pricing_requests_per_iteration,
    )?;
    validate_positive_count_field(
        profile,
        "component_workload.risk_paths_per_iteration",
        profile.component_workload.risk_paths_per_iteration,
    )?;

    let shape_capacity = profile
        .portfolio_shape
        .underlying_count
        .checked_mul(profile.portfolio_shape.maturity_bucket_count_per_underlying)
        .and_then(|partial_capacity| {
            partial_capacity.checked_mul(profile.portfolio_shape.strikes_per_maturity_bucket)
        })
        .ok_or_else(
            || BenchmarkScenarioCatalogLoadError::PortfolioShapeCapacityOverflow {
                profile_id: profile.profile_id.clone(),
            },
        )?;

    if profile.portfolio_shape.option_position_count > shape_capacity {
        return Err(
            BenchmarkScenarioCatalogLoadError::OptionPositionCountExceedsShapeCapacity {
                profile_id: profile.profile_id.clone(),
                option_position_count: profile.portfolio_shape.option_position_count,
                shape_capacity,
            },
        );
    }

    Ok(())
}

fn validate_positive_finite_field(
    profile: &BenchmarkScenarioWorkloadProfile,
    field_name: &'static str,
    value: f64,
) -> Result<(), BenchmarkScenarioCatalogLoadError> {
    if value.is_finite() && value > 0.0 {
        return Ok(());
    }

    Err(
        BenchmarkScenarioCatalogLoadError::InvalidPositiveFiniteValue {
            profile_id: profile.profile_id.clone(),
            field_name,
            value,
        },
    )
}

fn validate_non_negative_finite_field(
    profile: &BenchmarkScenarioWorkloadProfile,
    field_name: &'static str,
    value: f64,
) -> Result<(), BenchmarkScenarioCatalogLoadError> {
    if value.is_finite() && value >= 0.0 {
        return Ok(());
    }

    Err(
        BenchmarkScenarioCatalogLoadError::InvalidNonNegativeFiniteValue {
            profile_id: profile.profile_id.clone(),
            field_name,
            value,
        },
    )
}

fn validate_positive_count_field(
    profile: &BenchmarkScenarioWorkloadProfile,
    field_name: &'static str,
    value: usize,
) -> Result<(), BenchmarkScenarioCatalogLoadError> {
    if value > 0 {
        return Ok(());
    }

    Err(BenchmarkScenarioCatalogLoadError::InvalidPositiveCount {
        profile_id: profile.profile_id.clone(),
        field_name,
        value,
    })
}

pub const ORACLE_PRICER_RISK_BENCHMARK_REPORT_SCHEMA_VERSION: &str =
    "benchmark.oracle_pricer_risk.report.v1";
pub const ORACLE_PRICER_RISK_BENCHMARK_DETERMINISM_POLICY: &str =
    "canonical_profile_order_and_deterministic_seed_partitioning.v1";
pub const ORACLE_PRICER_RISK_BENCHMARK_EXECUTION_POLICY: &str =
    "warmup_iterations_excluded_from_measured_latency_and_throughput.v1";
pub const ORACLE_PRICER_RISK_BENCHMARK_METRIC_POLICY: &str =
    "mean_latency_seconds_and_throughput_operations_per_second.v1";
pub const ORACLE_PRICER_RISK_BENCHMARK_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "benchmark.oracle_pricer_risk.output_schema_metadata.v1";

const ORACLE_PRICER_RISK_BENCHMARK_REQUIRED_REPORT_FIELDS: [&str; 11] = [
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
const ORACLE_PRICER_RISK_BENCHMARK_REQUIRED_PROFILE_REPORT_FIELDS: [&str; 8] = [
    "profile_id",
    "workload_scale",
    "warmup_iteration_count",
    "measured_iteration_count",
    "oracle_solver_metrics",
    "pricing_metrics",
    "risk_sampling_metrics",
    "reproducibility_seed",
];
const ORACLE_PRICER_RISK_BENCHMARK_REQUIRED_COMPONENT_METRICS_FIELDS: [&str; 7] = [
    "operations_per_iteration",
    "measured_operation_count",
    "measured_elapsed_seconds",
    "mean_latency_seconds",
    "throughput_operations_per_second",
    "invocation_checksum",
    "invocation_count_including_warmup",
];
const ORACLE_PRICER_RISK_COMPONENT_ORACLE: &str = "oracle_solver";
const ORACLE_PRICER_RISK_COMPONENT_PRICING: &str = "characteristic_function_pricer";
const ORACLE_PRICER_RISK_COMPONENT_RISK: &str = "sufficient_statistics_risk_sampler";
pub const ORACLE_PRICER_RISK_BENCHMARK_POLICY_SCHEMA_VERSION: &str =
    "benchmark.oracle_pricer_risk.policy.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskBenchmarkPolicyRationale {
    pub model_parameters: String,
    pub oracle_solver_probes: String,
    pub pricing_input_ranges: String,
    pub seed_partition: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskBenchmarkModelParameters {
    pub hawkes_beta: f64,
    pub hawkes_alpha: HawkesKernelLoadings,
    pub hawkes_rho: HawkesKernelLoadings,
    pub gamma_shape_k: f64,
    pub gamma_scale_theta: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskBenchmarkOracleSolverProbes {
    pub u_probe_min: f64,
    pub u_probe_max: f64,
    pub z_probe_min: f64,
    pub z_probe_max: f64,
    pub seasonality_factor_min: f64,
    pub seasonality_factor_max: f64,
    pub news_intensity_factor_min: f64,
    pub news_intensity_factor_max: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskBenchmarkPricingInputRanges {
    pub integration_upper_bound: f64,
    pub minimum_integration_step_count: usize,
    pub spot_base: f64,
    pub spot_underlying_count_scale: f64,
    pub spot_noise_span: f64,
    pub strike_ratio_min: f64,
    pub strike_ratio_max: f64,
    pub maturity_multiplier_min: f64,
    pub maturity_bucket_scale: f64,
    pub risk_free_rate_min: f64,
    pub risk_free_rate_max: f64,
    pub dividend_yield_min: f64,
    pub dividend_yield_max: f64,
    pub volatility_min: f64,
    pub volatility_max: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskBenchmarkSeedPartition {
    pub oracle_component_seed_salt: u64,
    pub pricing_component_seed_salt: u64,
    pub risk_component_seed_salt: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskBenchmarkPolicy {
    pub schema_version: String,
    pub policy_id: String,
    pub policy_version: String,
    pub rationale: OraclePricerRiskBenchmarkPolicyRationale,
    pub model_parameters: OraclePricerRiskBenchmarkModelParameters,
    pub oracle_solver_probes: OraclePricerRiskBenchmarkOracleSolverProbes,
    pub pricing_input_ranges: OraclePricerRiskBenchmarkPricingInputRanges,
    pub seed_partition: OraclePricerRiskBenchmarkSeedPartition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskBenchmarkExecutionConfig {
    pub warmup_iteration_count_override: Option<usize>,
    pub measured_iteration_count_override: Option<usize>,
    pub reproducibility_seed: u64,
}

impl Default for OraclePricerRiskBenchmarkExecutionConfig {
    fn default() -> Self {
        Self {
            warmup_iteration_count_override: None,
            measured_iteration_count_override: None,
            reproducibility_seed: 0,
        }
    }
}

impl OraclePricerRiskBenchmarkExecutionConfig {
    pub fn validate(self) -> Result<(), OraclePricerRiskBenchmarkExecutionConfigError> {
        if self.measured_iteration_count_override == Some(0) {
            return Err(
                OraclePricerRiskBenchmarkExecutionConfigError::InvalidMeasuredIterationCountOverride {
                    value: 0,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerRiskBenchmarkExecutionConfigError {
    InvalidMeasuredIterationCountOverride { value: usize },
}

impl Display for OraclePricerRiskBenchmarkExecutionConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMeasuredIterationCountOverride { value } => write!(
                formatter,
                "execution_config.measured_iteration_count_override must be greater than 0 when provided, found {value}"
            ),
        }
    }
}

impl Error for OraclePricerRiskBenchmarkExecutionConfigError {}

impl OraclePricerRiskBenchmarkPolicy {
    pub fn validate(&self) -> Result<(), OraclePricerRiskBenchmarkPolicyValidationError> {
        if self.schema_version != ORACLE_PRICER_RISK_BENCHMARK_POLICY_SCHEMA_VERSION {
            return Err(
                OraclePricerRiskBenchmarkPolicyValidationError::UnsupportedSchemaVersion {
                    expected: ORACLE_PRICER_RISK_BENCHMARK_POLICY_SCHEMA_VERSION,
                    actual: self.schema_version.clone(),
                },
            );
        }
        if self.policy_id.trim().is_empty() {
            return Err(OraclePricerRiskBenchmarkPolicyValidationError::EmptyPolicyId);
        }
        if self.policy_version.trim().is_empty() {
            return Err(OraclePricerRiskBenchmarkPolicyValidationError::EmptyPolicyVersion);
        }

        validate_non_empty_rationale_field(
            "rationale.model_parameters",
            &self.rationale.model_parameters,
        )?;
        validate_non_empty_rationale_field(
            "rationale.oracle_solver_probes",
            &self.rationale.oracle_solver_probes,
        )?;
        validate_non_empty_rationale_field(
            "rationale.pricing_input_ranges",
            &self.rationale.pricing_input_ranges,
        )?;
        validate_non_empty_rationale_field(
            "rationale.seed_partition",
            &self.rationale.seed_partition,
        )?;

        let model_parameters = self.model_parameters;
        HawkesKernelParameters::try_new(
            1.0,
            model_parameters.hawkes_beta,
            model_parameters.hawkes_alpha,
            model_parameters.hawkes_rho,
        )
        .map_err(|source| {
            OraclePricerRiskBenchmarkPolicyValidationError::InvalidHawkesParameters { source }
        })?;
        GammaSeverityMarkConfiguration::try_new(
            model_parameters.gamma_shape_k,
            model_parameters.gamma_scale_theta,
        )
        .map_err(|source| {
            OraclePricerRiskBenchmarkPolicyValidationError::InvalidGammaMarkConfiguration { source }
        })?;

        let solver_probes = self.oracle_solver_probes;
        validate_range(
            "oracle_solver_probes.u_probe",
            solver_probes.u_probe_min,
            solver_probes.u_probe_max,
            RangeValidationPolicy::StrictlyPositive,
        )?;
        validate_range(
            "oracle_solver_probes.z_probe",
            solver_probes.z_probe_min,
            solver_probes.z_probe_max,
            RangeValidationPolicy::StrictlyPositive,
        )?;
        validate_range(
            "oracle_solver_probes.seasonality_factor",
            solver_probes.seasonality_factor_min,
            solver_probes.seasonality_factor_max,
            RangeValidationPolicy::NonNegative,
        )?;
        validate_range(
            "oracle_solver_probes.news_intensity_factor",
            solver_probes.news_intensity_factor_min,
            solver_probes.news_intensity_factor_max,
            RangeValidationPolicy::NonNegative,
        )?;

        let pricing_ranges = self.pricing_input_ranges;
        validate_positive_finite_policy_field(
            "pricing_input_ranges.integration_upper_bound",
            pricing_ranges.integration_upper_bound,
        )?;
        if pricing_ranges.minimum_integration_step_count == 0 {
            return Err(
                OraclePricerRiskBenchmarkPolicyValidationError::InvalidPositiveCountField {
                    field_name: "pricing_input_ranges.minimum_integration_step_count",
                    value: pricing_ranges.minimum_integration_step_count,
                },
            );
        }
        validate_positive_finite_policy_field(
            "pricing_input_ranges.spot_base",
            pricing_ranges.spot_base,
        )?;
        validate_non_negative_finite_policy_field(
            "pricing_input_ranges.spot_underlying_count_scale",
            pricing_ranges.spot_underlying_count_scale,
        )?;
        validate_non_negative_finite_policy_field(
            "pricing_input_ranges.spot_noise_span",
            pricing_ranges.spot_noise_span,
        )?;
        validate_range(
            "pricing_input_ranges.strike_ratio",
            pricing_ranges.strike_ratio_min,
            pricing_ranges.strike_ratio_max,
            RangeValidationPolicy::StrictlyPositive,
        )?;
        validate_positive_finite_policy_field(
            "pricing_input_ranges.maturity_multiplier_min",
            pricing_ranges.maturity_multiplier_min,
        )?;
        validate_non_negative_finite_policy_field(
            "pricing_input_ranges.maturity_bucket_scale",
            pricing_ranges.maturity_bucket_scale,
        )?;
        validate_range(
            "pricing_input_ranges.risk_free_rate",
            pricing_ranges.risk_free_rate_min,
            pricing_ranges.risk_free_rate_max,
            RangeValidationPolicy::Finite,
        )?;
        validate_range(
            "pricing_input_ranges.dividend_yield",
            pricing_ranges.dividend_yield_min,
            pricing_ranges.dividend_yield_max,
            RangeValidationPolicy::Finite,
        )?;
        validate_range(
            "pricing_input_ranges.volatility",
            pricing_ranges.volatility_min,
            pricing_ranges.volatility_max,
            RangeValidationPolicy::StrictlyPositive,
        )?;

        let seed_partition = self.seed_partition;
        if seed_partition.oracle_component_seed_salt == seed_partition.pricing_component_seed_salt
            || seed_partition.oracle_component_seed_salt == seed_partition.risk_component_seed_salt
            || seed_partition.pricing_component_seed_salt == seed_partition.risk_component_seed_salt
        {
            return Err(OraclePricerRiskBenchmarkPolicyValidationError::DuplicateSeedSaltValues);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerRiskBenchmarkPolicyValidationError {
    UnsupportedSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    EmptyPolicyId,
    EmptyPolicyVersion,
    EmptyRationaleField {
        field_name: &'static str,
    },
    InvalidHawkesParameters {
        source: HawkesKernelParameterError,
    },
    InvalidGammaMarkConfiguration {
        source: NewsMarkConfigurationError,
    },
    InvalidRangeField {
        field_name: &'static str,
        min_value: f64,
        max_value: f64,
    },
    InvalidFiniteField {
        field_name: &'static str,
        value: f64,
    },
    InvalidNonNegativeFiniteField {
        field_name: &'static str,
        value: f64,
    },
    InvalidPositiveFiniteField {
        field_name: &'static str,
        value: f64,
    },
    InvalidPositiveCountField {
        field_name: &'static str,
        value: usize,
    },
    DuplicateSeedSaltValues,
}

impl Display for OraclePricerRiskBenchmarkPolicyValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { expected, actual } => write!(
                formatter,
                "benchmark policy schema_version must be `{expected}`, found `{actual}`"
            ),
            Self::EmptyPolicyId => {
                write!(formatter, "benchmark policy policy_id must be non-empty")
            }
            Self::EmptyPolicyVersion => {
                write!(
                    formatter,
                    "benchmark policy policy_version must be non-empty"
                )
            }
            Self::EmptyRationaleField { field_name } => {
                write!(formatter, "benchmark policy {field_name} must be non-empty")
            }
            Self::InvalidHawkesParameters { source } => write!(
                formatter,
                "benchmark policy model_parameters Hawkes values are invalid: {source}"
            ),
            Self::InvalidGammaMarkConfiguration { source } => write!(
                formatter,
                "benchmark policy model_parameters gamma mark values are invalid: {source}"
            ),
            Self::InvalidRangeField {
                field_name,
                min_value,
                max_value,
            } => write!(
                formatter,
                "benchmark policy {field_name} range is invalid, expected min <= max with finite bounds, found min={min_value} max={max_value}"
            ),
            Self::InvalidFiniteField { field_name, value } => write!(
                formatter,
                "benchmark policy {field_name} must be finite, found {value}"
            ),
            Self::InvalidNonNegativeFiniteField { field_name, value } => write!(
                formatter,
                "benchmark policy {field_name} must be finite and non-negative, found {value}"
            ),
            Self::InvalidPositiveFiniteField { field_name, value } => write!(
                formatter,
                "benchmark policy {field_name} must be finite and strictly positive, found {value}"
            ),
            Self::InvalidPositiveCountField { field_name, value } => write!(
                formatter,
                "benchmark policy {field_name} must be greater than 0, found {value}"
            ),
            Self::DuplicateSeedSaltValues => write!(
                formatter,
                "benchmark policy seed_partition seed salts must be pairwise distinct"
            ),
        }
    }
}

impl Error for OraclePricerRiskBenchmarkPolicyValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidHawkesParameters { source } => Some(source),
            Self::InvalidGammaMarkConfiguration { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerRiskBenchmarkPolicyLoadError {
    InvalidJson {
        source: String,
    },
    InvalidPolicy {
        source: OraclePricerRiskBenchmarkPolicyValidationError,
    },
}

impl Display for OraclePricerRiskBenchmarkPolicyLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson { source } => write!(
                formatter,
                "oracle pricer risk benchmark policy payload must be valid JSON: {source}"
            ),
            Self::InvalidPolicy { source } => write!(
                formatter,
                "oracle pricer risk benchmark policy payload is invalid: {source}"
            ),
        }
    }
}

impl Error for OraclePricerRiskBenchmarkPolicyLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPolicy { source } => Some(source),
            _ => None,
        }
    }
}

pub fn load_oracle_pricer_risk_benchmark_policy_from_json(
    policy_payload: &str,
) -> Result<OraclePricerRiskBenchmarkPolicy, OraclePricerRiskBenchmarkPolicyLoadError> {
    let policy: OraclePricerRiskBenchmarkPolicy =
        serde_json::from_str(policy_payload).map_err(|source| {
            OraclePricerRiskBenchmarkPolicyLoadError::InvalidJson {
                source: source.to_string(),
            }
        })?;

    policy
        .validate()
        .map_err(|source| OraclePricerRiskBenchmarkPolicyLoadError::InvalidPolicy { source })?;

    Ok(policy)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OraclePricerRiskBenchmarkOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub required_report_fields: Vec<&'static str>,
    pub required_profile_report_fields: Vec<&'static str>,
    pub required_component_metrics_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct OraclePricerRiskBenchmarkComponentMetrics {
    pub operations_per_iteration: usize,
    pub measured_operation_count: usize,
    pub measured_elapsed_seconds: f64,
    pub mean_latency_seconds: f64,
    pub throughput_operations_per_second: f64,
    pub invocation_checksum: f64,
    pub invocation_count_including_warmup: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OraclePricerRiskBenchmarkProfileReport {
    pub profile_id: String,
    pub workload_scale: BenchmarkWorkloadScale,
    pub warmup_iteration_count: usize,
    pub measured_iteration_count: usize,
    pub oracle_solver_metrics: OraclePricerRiskBenchmarkComponentMetrics,
    pub pricing_metrics: OraclePricerRiskBenchmarkComponentMetrics,
    pub risk_sampling_metrics: OraclePricerRiskBenchmarkComponentMetrics,
    pub reproducibility_seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OraclePricerRiskBenchmarkSummary {
    pub profile_count: usize,
    pub oracle_solver_total_operation_count: usize,
    pub pricing_total_operation_count: usize,
    pub risk_sampling_total_operation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OraclePricerRiskBenchmarkReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub execution_policy: &'static str,
    pub metric_policy: &'static str,
    pub output_schema_metadata: OraclePricerRiskBenchmarkOutputSchemaMetadata,
    pub catalog_id: String,
    pub catalog_version: String,
    pub policy_id: String,
    pub policy_version: String,
    pub summary: OraclePricerRiskBenchmarkSummary,
    pub profile_reports: Vec<OraclePricerRiskBenchmarkProfileReport>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerRiskBenchmarkHarnessError {
    InvalidScenarioCatalog {
        source: BenchmarkScenarioCatalogLoadError,
    },
    InvalidBenchmarkPolicy {
        source: OraclePricerRiskBenchmarkPolicyValidationError,
    },
    InvalidExecutionConfig {
        source: OraclePricerRiskBenchmarkExecutionConfigError,
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
    RiskSliceSolveFailure {
        profile_id: String,
        interval_index: usize,
        source: SufficientStatisticsTransformSliceSolveError,
    },
    RiskSliceSamplingFailure {
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

impl Display for OraclePricerRiskBenchmarkHarnessError {
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
                "profile `{profile_id}` interval {interval_index} failed risk slice context validation: {source}"
            ),
            Self::RiskSliceSolveFailure {
                profile_id,
                interval_index,
                source,
            } => write!(
                formatter,
                "profile `{profile_id}` interval {interval_index} risk slice solve failed: {source}"
            ),
            Self::RiskSliceSamplingFailure { profile_id, source } => write!(
                formatter,
                "profile `{profile_id}` risk slice sampling failed: {source}"
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

impl Error for OraclePricerRiskBenchmarkHarnessError {
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
            Self::RiskSliceSolveFailure { source, .. } => Some(source),
            Self::RiskSliceSamplingFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedBenchmarkRunLength {
    warmup_iteration_count: usize,
    measured_iteration_count: usize,
}

pub fn run_oracle_pricer_risk_benchmark_harness(
    catalog: &BenchmarkScenarioCatalog,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
    execution_config: OraclePricerRiskBenchmarkExecutionConfig,
) -> Result<OraclePricerRiskBenchmarkReport, OraclePricerRiskBenchmarkHarnessError> {
    benchmark_policy.validate().map_err(|source| {
        OraclePricerRiskBenchmarkHarnessError::InvalidBenchmarkPolicy { source }
    })?;
    execution_config.validate().map_err(|source| {
        OraclePricerRiskBenchmarkHarnessError::InvalidExecutionConfig { source }
    })?;

    let mut normalized_catalog = catalog.clone();
    normalized_catalog.validate().map_err(|source| {
        OraclePricerRiskBenchmarkHarnessError::InvalidScenarioCatalog { source }
    })?;
    normalized_catalog.sort_profiles_canonically();

    let mut profile_reports: Vec<OraclePricerRiskBenchmarkProfileReport> =
        Vec::with_capacity(normalized_catalog.profiles.len());
    let mut oracle_solver_total_operation_count: usize = 0;
    let mut pricing_total_operation_count: usize = 0;
    let mut risk_sampling_total_operation_count: usize = 0;

    for profile in &normalized_catalog.profiles {
        let profile_report = run_profile_benchmark(profile, benchmark_policy, execution_config)?;

        oracle_solver_total_operation_count = oracle_solver_total_operation_count
            .checked_add(
                profile_report
                    .oracle_solver_metrics
                    .measured_operation_count,
            )
            .ok_or_else(
                || OraclePricerRiskBenchmarkHarnessError::OperationCountOverflow {
                    profile_id: profile.profile_id.clone(),
                    component: ORACLE_PRICER_RISK_COMPONENT_ORACLE,
                },
            )?;
        pricing_total_operation_count = pricing_total_operation_count
            .checked_add(profile_report.pricing_metrics.measured_operation_count)
            .ok_or_else(
                || OraclePricerRiskBenchmarkHarnessError::OperationCountOverflow {
                    profile_id: profile.profile_id.clone(),
                    component: ORACLE_PRICER_RISK_COMPONENT_PRICING,
                },
            )?;
        risk_sampling_total_operation_count = risk_sampling_total_operation_count
            .checked_add(
                profile_report
                    .risk_sampling_metrics
                    .measured_operation_count,
            )
            .ok_or_else(
                || OraclePricerRiskBenchmarkHarnessError::OperationCountOverflow {
                    profile_id: profile.profile_id.clone(),
                    component: ORACLE_PRICER_RISK_COMPONENT_RISK,
                },
            )?;

        profile_reports.push(profile_report);
    }

    Ok(OraclePricerRiskBenchmarkReport {
        schema_version: ORACLE_PRICER_RISK_BENCHMARK_REPORT_SCHEMA_VERSION,
        determinism_policy: ORACLE_PRICER_RISK_BENCHMARK_DETERMINISM_POLICY,
        execution_policy: ORACLE_PRICER_RISK_BENCHMARK_EXECUTION_POLICY,
        metric_policy: ORACLE_PRICER_RISK_BENCHMARK_METRIC_POLICY,
        output_schema_metadata: OraclePricerRiskBenchmarkOutputSchemaMetadata {
            schema_version: ORACLE_PRICER_RISK_BENCHMARK_OUTPUT_SCHEMA_METADATA_VERSION,
            required_report_fields: ORACLE_PRICER_RISK_BENCHMARK_REQUIRED_REPORT_FIELDS.to_vec(),
            required_profile_report_fields:
                ORACLE_PRICER_RISK_BENCHMARK_REQUIRED_PROFILE_REPORT_FIELDS.to_vec(),
            required_component_metrics_fields:
                ORACLE_PRICER_RISK_BENCHMARK_REQUIRED_COMPONENT_METRICS_FIELDS.to_vec(),
        },
        catalog_id: normalized_catalog.catalog_id,
        catalog_version: normalized_catalog.catalog_version,
        policy_id: benchmark_policy.policy_id.clone(),
        policy_version: benchmark_policy.policy_version.clone(),
        summary: OraclePricerRiskBenchmarkSummary {
            profile_count: profile_reports.len(),
            oracle_solver_total_operation_count,
            pricing_total_operation_count,
            risk_sampling_total_operation_count,
        },
        profile_reports,
    })
}

fn run_profile_benchmark(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
    execution_config: OraclePricerRiskBenchmarkExecutionConfig,
) -> Result<OraclePricerRiskBenchmarkProfileReport, OraclePricerRiskBenchmarkHarnessError> {
    let resolved_run_length = resolve_profile_run_length(profile, execution_config)?;
    let equation_specification = build_equation_specification(profile, benchmark_policy)?;
    let oracle_solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        profile.interval_settings.oracle_solver_step_count,
    )
    .map_err(
        |source| OraclePricerRiskBenchmarkHarnessError::InvalidOracleSolverSettings {
            profile_id: profile.profile_id.clone(),
            source,
        },
    )?;
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
    let risk_component_seed =
        splitmix64(profile_seed ^ benchmark_policy.seed_partition.risk_component_seed_salt);

    let risk_slices = build_risk_transform_slices(
        profile,
        benchmark_policy,
        equation_specification,
        oracle_solver_settings,
        risk_component_seed,
    )?;

    let oracle_solver_metrics = run_component_benchmark(
        profile,
        ORACLE_PRICER_RISK_COMPONENT_ORACLE,
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
                OraclePricerRiskBenchmarkHarnessError::OracleSolverFailure {
                    profile_id: profile.profile_id.clone(),
                    source,
                }
            })?;
            if solve_result.status != ExponentialAffineTransformIntegrationStatus::Completed {
                return Err(
                    OraclePricerRiskBenchmarkHarnessError::OracleSolverIncomplete {
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
                OraclePricerRiskBenchmarkHarnessError::OracleSolverIncomplete {
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
        ORACLE_PRICER_RISK_COMPONENT_PRICING,
        resolved_run_length,
        profile.component_workload.pricing_requests_per_iteration,
        |invocation_count_including_warmup| {
            let invocation_seed =
                derive_invocation_seed(pricing_component_seed, invocation_count_including_warmup);
            let market_inputs =
                build_pricing_market_inputs(profile, benchmark_policy, invocation_seed).map_err(
                    |source| OraclePricerRiskBenchmarkHarnessError::InvalidPricingMarketInputs {
                        profile_id: profile.profile_id.clone(),
                        source,
                    },
                )?;
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

            let maturity_interval = market_inputs.maturity_interval;
            let drift = (market_inputs.risk_free_rate
                - market_inputs.dividend_yield
                - 0.5 * volatility * volatility)
                * maturity_interval;
            let variance = volatility * volatility * maturity_interval;
            let pricing_result = price_vanilla_option_via_fourier_inversion(
                market_inputs,
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
                OraclePricerRiskBenchmarkHarnessError::PricingEngineFailure {
                    profile_id: profile.profile_id.clone(),
                    source,
                }
            })?;

            Ok(pricing_result.price)
        },
    )?;

    let risk_sampling_metrics = run_component_benchmark(
        profile,
        ORACLE_PRICER_RISK_COMPONENT_RISK,
        resolved_run_length,
        profile.component_workload.risk_paths_per_iteration,
        |invocation_count_including_warmup| {
            let path_seed =
                derive_invocation_seed(risk_component_seed, invocation_count_including_warmup);
            let sampled_statistics =
                sample_interval_statistics_batch_from_transform_slices(path_seed, &risk_slices)
                    .map_err(|source| {
                        OraclePricerRiskBenchmarkHarnessError::RiskSliceSamplingFailure {
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

    Ok(OraclePricerRiskBenchmarkProfileReport {
        profile_id: profile.profile_id.clone(),
        workload_scale: profile.workload_scale,
        warmup_iteration_count: resolved_run_length.warmup_iteration_count,
        measured_iteration_count: resolved_run_length.measured_iteration_count,
        oracle_solver_metrics,
        pricing_metrics,
        risk_sampling_metrics,
        reproducibility_seed: profile_seed,
    })
}

fn resolve_profile_run_length(
    profile: &BenchmarkScenarioWorkloadProfile,
    execution_config: OraclePricerRiskBenchmarkExecutionConfig,
) -> Result<ResolvedBenchmarkRunLength, OraclePricerRiskBenchmarkHarnessError> {
    let warmup_iteration_count = execution_config
        .warmup_iteration_count_override
        .unwrap_or(profile.run_length.warmup_iteration_count);
    let measured_iteration_count = execution_config
        .measured_iteration_count_override
        .unwrap_or(profile.run_length.measured_iteration_count);

    if measured_iteration_count == 0 {
        return Err(
            OraclePricerRiskBenchmarkHarnessError::InvalidResolvedMeasuredIterationCount {
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

fn build_equation_specification(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
) -> Result<ExponentialAffineTransformEquationSpecification, OraclePricerRiskBenchmarkHarnessError>
{
    let model_parameters = benchmark_policy.model_parameters;
    let hawkes_kernel_parameters = HawkesKernelParameters::try_new(
        profile
            .model_state
            .baseline_deseasonalized_intensity_lambda_bar,
        model_parameters.hawkes_beta,
        model_parameters.hawkes_alpha,
        model_parameters.hawkes_rho,
    )
    .map_err(
        |source| OraclePricerRiskBenchmarkHarnessError::InvalidModelKernelParameters {
            profile_id: profile.profile_id.clone(),
            source,
        },
    )?;
    let gamma_mark_configuration = GammaSeverityMarkConfiguration::try_new(
        model_parameters.gamma_shape_k,
        model_parameters.gamma_scale_theta,
    )
    .map_err(|source| {
        OraclePricerRiskBenchmarkHarnessError::InvalidModelGammaMarkConfiguration {
            profile_id: profile.profile_id.clone(),
            source,
        }
    })?;

    Ok(ExponentialAffineTransformEquationSpecification::new(
        hawkes_kernel_parameters,
        gamma_mark_configuration,
    ))
}

fn build_pricing_engine_settings(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
) -> Result<FourierInversionEngineSettings, OraclePricerRiskBenchmarkHarnessError> {
    let contour = FourierInversionContour::default();
    let integration_step_count = profile.interval_settings.oracle_solver_step_count.max(
        benchmark_policy
            .pricing_input_ranges
            .minimum_integration_step_count,
    );
    let numeric_settings = FourierInversionNumericSettings::try_new(
        benchmark_policy
            .pricing_input_ranges
            .integration_upper_bound,
        integration_step_count,
        FourierInversionQuadratureRule::Midpoint,
    )
    .map_err(
        |source| OraclePricerRiskBenchmarkHarnessError::InvalidPricingEngineSettings {
            profile_id: profile.profile_id.clone(),
            source: FourierInversionEngineSettingsError::InvalidNumericSettings { source },
        },
    )?;

    FourierInversionEngineSettings::try_new(contour, numeric_settings).map_err(|source| {
        OraclePricerRiskBenchmarkHarnessError::InvalidPricingEngineSettings {
            profile_id: profile.profile_id.clone(),
            source,
        }
    })
}

fn build_oracle_transform_input(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
    equation_specification: ExponentialAffineTransformEquationSpecification,
    invocation_seed: u64,
) -> Result<ExponentialAffineTransformInput, OraclePricerRiskBenchmarkHarnessError> {
    let noise = unit_interval_from_seed(invocation_seed);
    let complementary_noise = 1.0 - noise;
    let solver_probes = benchmark_policy.oracle_solver_probes;

    ExponentialAffineTransformInput::try_new(
        equation_specification,
        ComplexScalar::new(
            interpolate_range(solver_probes.u_probe_min, solver_probes.u_probe_max, noise),
            0.0,
        ),
        ComplexScalar::new(
            interpolate_range(
                solver_probes.u_probe_min,
                solver_probes.u_probe_max,
                complementary_noise,
            ),
            0.0,
        ),
        ComplexScalar::new(
            interpolate_range(solver_probes.z_probe_min, solver_probes.z_probe_max, noise),
            0.0,
        ),
        ComplexScalar::new(
            interpolate_range(
                solver_probes.z_probe_min,
                solver_probes.z_probe_max,
                complementary_noise,
            ),
            0.0,
        ),
        interpolate_range(
            solver_probes.seasonality_factor_min,
            solver_probes.seasonality_factor_max,
            noise,
        ),
        interpolate_range(
            solver_probes.news_intensity_factor_min,
            solver_probes.news_intensity_factor_max,
            complementary_noise,
        ),
        profile.interval_settings.interval_delta,
        profile.model_state.initial_excitation_state,
    )
    .map_err(
        |source| OraclePricerRiskBenchmarkHarnessError::InvalidOracleTransformInput {
            profile_id: profile.profile_id.clone(),
            source,
        },
    )
}

fn build_pricing_market_inputs(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
    invocation_seed: u64,
) -> Result<VanillaOptionPricingMarketInputs, VanillaOptionPricingMarketInputsError> {
    let spot_noise = unit_interval_from_seed(invocation_seed ^ 0x91);
    let strike_noise = unit_interval_from_seed(invocation_seed ^ 0x53);
    let maturity_noise = unit_interval_from_seed(invocation_seed ^ 0xC7);
    let pricing_ranges = benchmark_policy.pricing_input_ranges;

    let spot = pricing_ranges.spot_base
        + profile.portfolio_shape.underlying_count as f64
            * pricing_ranges.spot_underlying_count_scale
        + pricing_ranges.spot_noise_span * spot_noise;
    let strike_ratio = interpolate_range(
        pricing_ranges.strike_ratio_min,
        pricing_ranges.strike_ratio_max,
        strike_noise,
    );
    let strike = spot * strike_ratio;
    let maturity_interval = profile.interval_settings.interval_delta
        * (pricing_ranges.maturity_multiplier_min
            + maturity_noise
                * (profile.portfolio_shape.maturity_bucket_count_per_underlying as f64
                    * pricing_ranges.maturity_bucket_scale));
    let risk_free_rate = interpolate_range(
        pricing_ranges.risk_free_rate_min,
        pricing_ranges.risk_free_rate_max,
        spot_noise,
    );
    let dividend_yield = interpolate_range(
        pricing_ranges.dividend_yield_min,
        pricing_ranges.dividend_yield_max,
        strike_noise,
    );

    VanillaOptionPricingMarketInputs::try_new(
        spot,
        strike,
        maturity_interval,
        risk_free_rate,
        dividend_yield,
    )
}

fn build_risk_transform_slices(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
    equation_specification: ExponentialAffineTransformEquationSpecification,
    solver_settings: ExponentialAffineTransformSolverSettings,
    component_seed: u64,
) -> Result<Vec<SufficientStatisticsTransformSlices>, OraclePricerRiskBenchmarkHarnessError> {
    let mut slices_by_interval: Vec<SufficientStatisticsTransformSlices> =
        Vec::with_capacity(profile.interval_settings.horizon_interval_count);
    let sampling_settings = SufficientStatisticsTransformSliceSettings::default();

    for interval_index in 0..profile.interval_settings.horizon_interval_count {
        let interval_seed = derive_invocation_seed(component_seed, interval_index + 1);
        let factor_noise = unit_interval_from_seed(interval_seed);
        let seasonality_factor = interpolate_range(
            benchmark_policy.oracle_solver_probes.seasonality_factor_min,
            benchmark_policy.oracle_solver_probes.seasonality_factor_max,
            factor_noise,
        );
        let news_intensity_factor = interpolate_range(
            benchmark_policy
                .oracle_solver_probes
                .news_intensity_factor_min,
            benchmark_policy
                .oracle_solver_probes
                .news_intensity_factor_max,
            1.0 - factor_noise,
        );

        let context = SufficientStatisticsTransformSliceContext::try_new(
            interval_index,
            seasonality_factor,
            news_intensity_factor,
            profile.interval_settings.interval_delta,
            profile.model_state.initial_excitation_state,
        )
        .map_err(|source| {
            OraclePricerRiskBenchmarkHarnessError::InvalidRiskSliceContext {
                profile_id: profile.profile_id.clone(),
                interval_index,
                source,
            }
        })?;

        let solved_slices = solve_sufficient_statistics_transform_slices_with_solver_settings(
            equation_specification,
            context,
            sampling_settings,
            Some(solver_settings),
        )
        .map_err(|source| {
            OraclePricerRiskBenchmarkHarnessError::RiskSliceSolveFailure {
                profile_id: profile.profile_id.clone(),
                interval_index,
                source,
            }
        })?;
        slices_by_interval.push(solved_slices);
    }

    Ok(slices_by_interval)
}

fn run_component_benchmark<F>(
    profile: &BenchmarkScenarioWorkloadProfile,
    component: &'static str,
    run_length: ResolvedBenchmarkRunLength,
    operations_per_iteration: usize,
    mut run_invocation: F,
) -> Result<OraclePricerRiskBenchmarkComponentMetrics, OraclePricerRiskBenchmarkHarnessError>
where
    F: FnMut(usize) -> Result<f64, OraclePricerRiskBenchmarkHarnessError>,
{
    let warmup_operation_count = run_length
        .warmup_iteration_count
        .checked_mul(operations_per_iteration)
        .ok_or_else(
            || OraclePricerRiskBenchmarkHarnessError::OperationCountOverflow {
                profile_id: profile.profile_id.clone(),
                component,
            },
        )?;
    let measured_operation_count = run_length
        .measured_iteration_count
        .checked_mul(operations_per_iteration)
        .ok_or_else(
            || OraclePricerRiskBenchmarkHarnessError::OperationCountOverflow {
                profile_id: profile.profile_id.clone(),
                component,
            },
        )?;

    let mut invocation_count_including_warmup: usize = 0;
    let mut invocation_checksum = 0.0_f64;
    for _ in 0..warmup_operation_count {
        invocation_count_including_warmup = invocation_count_including_warmup.saturating_add(1);
        let invocation_result = run_invocation(invocation_count_including_warmup)?;
        if !invocation_result.is_finite() {
            return Err(
                OraclePricerRiskBenchmarkHarnessError::NonFiniteInvocationChecksum {
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
                OraclePricerRiskBenchmarkHarnessError::NonFiniteInvocationChecksum {
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
            OraclePricerRiskBenchmarkHarnessError::InvalidMetricComputation {
                profile_id: profile.profile_id.clone(),
                component,
                field: "measured_elapsed_seconds",
                value: measured_elapsed_seconds,
            },
        );
    }
    if !mean_latency_seconds.is_finite() {
        return Err(
            OraclePricerRiskBenchmarkHarnessError::InvalidMetricComputation {
                profile_id: profile.profile_id.clone(),
                component,
                field: "mean_latency_seconds",
                value: mean_latency_seconds,
            },
        );
    }
    if !throughput_operations_per_second.is_finite() {
        return Err(
            OraclePricerRiskBenchmarkHarnessError::InvalidMetricComputation {
                profile_id: profile.profile_id.clone(),
                component,
                field: "throughput_operations_per_second",
                value: throughput_operations_per_second,
            },
        );
    }
    if !invocation_checksum.is_finite() {
        return Err(
            OraclePricerRiskBenchmarkHarnessError::InvalidMetricComputation {
                profile_id: profile.profile_id.clone(),
                component,
                field: "invocation_checksum",
                value: invocation_checksum,
            },
        );
    }

    Ok(OraclePricerRiskBenchmarkComponentMetrics {
        operations_per_iteration,
        measured_operation_count,
        measured_elapsed_seconds,
        mean_latency_seconds,
        throughput_operations_per_second,
        invocation_checksum,
        invocation_count_including_warmup,
    })
}

pub const COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION: &str =
    "benchmark.coarse_fine_consistency.report.v1";
pub const COARSE_FINE_CONSISTENCY_REGRESSION_DETERMINISM_POLICY: &str =
    "canonical_profile_order_and_reproducibility_seed_partitioning.v1";
pub const COARSE_FINE_CONSISTENCY_REGRESSION_VARIANCE_CONTROL_POLICY: &str =
    "paired_path_seed_reuse_for_risk_statistics_and_fixed_seeded_fine_pricing_reference.v1";
pub const COARSE_FINE_CONSISTENCY_REGRESSION_METRIC_POLICY: &str =
    "absolute_and_relative_metric_delta_thresholds_with_inclusive_pass_boundaries.v1";

/// Note:
/// This harness covers engine-level coarse versus fine sanity checks for benchmark workflows.
/// Model-level multi-regime scale-consistency governance is owned by separate validation workflows.
pub const COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY: &str =
    "engine_level_consistency_only_model_level_multi_regime_governance_delegated_to_scale_consistency_validation_workflows.v1";
pub const COARSE_FINE_CONSISTENCY_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "benchmark.coarse_fine_consistency.output_schema_metadata.v1";
pub const COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION: &str =
    "benchmark.coarse_fine_consistency.tolerance.v1";

const COARSE_FINE_CONSISTENCY_REQUIRED_REPORT_FIELDS: [&str; 14] = [
    "schema_version",
    "determinism_policy",
    "variance_control_policy",
    "metric_policy",
    "governance_scope_policy",
    "output_schema_metadata",
    "catalog_id",
    "catalog_version",
    "benchmark_policy_id",
    "benchmark_policy_version",
    "tolerance_config",
    "execution_config",
    "summary",
    "scenario_reports",
];
const COARSE_FINE_CONSISTENCY_REQUIRED_SCENARIO_REPORT_FIELDS: [&str; 7] = [
    "scenario_id",
    "status",
    "failed_metric_count",
    "max_absolute_delta",
    "run_metadata",
    "summary",
    "metric_deltas",
];
const COARSE_FINE_CONSISTENCY_REQUIRED_METRIC_DELTA_FIELDS: [&str; 8] = [
    "metric_id",
    "coarse_value",
    "fine_reference_value",
    "absolute_delta",
    "relative_delta",
    "absolute_tolerance",
    "relative_tolerance",
    "passed",
];
const COARSE_FINE_CONSISTENCY_REQUIRED_RUN_METADATA_FIELDS: [&str; 11] = [
    "scenario_id",
    "profile_seed",
    "pricing_component_seed",
    "risk_component_seed",
    "coarse_interval_delta",
    "fine_interval_delta",
    "horizon_interval_count",
    "fine_subinterval_count_per_coarse_interval",
    "paired_risk_path_count",
    "fine_pricing_reference_path_count",
    "fine_pricing_reference_step_count",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseFineConsistencyRegressionExecutionConfig {
    pub reproducibility_seed: u64,
    pub fine_subinterval_count_per_coarse_interval: usize,
    pub paired_risk_path_count: usize,
    pub fine_pricing_reference_path_count: usize,
    pub fine_pricing_reference_step_count: usize,
}

impl Default for CoarseFineConsistencyRegressionExecutionConfig {
    fn default() -> Self {
        Self {
            reproducibility_seed: 0,
            fine_subinterval_count_per_coarse_interval: 4,
            paired_risk_path_count: 128,
            fine_pricing_reference_path_count: 2_048,
            fine_pricing_reference_step_count: 16,
        }
    }
}

impl CoarseFineConsistencyRegressionExecutionConfig {
    pub fn validate(self) -> Result<(), CoarseFineConsistencyRegressionExecutionConfigError> {
        if self.fine_subinterval_count_per_coarse_interval == 0 {
            return Err(
                CoarseFineConsistencyRegressionExecutionConfigError::InvalidFineSubintervalCount {
                    value: self.fine_subinterval_count_per_coarse_interval,
                },
            );
        }
        if self.paired_risk_path_count == 0 {
            return Err(
                CoarseFineConsistencyRegressionExecutionConfigError::InvalidRiskPathCount {
                    value: self.paired_risk_path_count,
                },
            );
        }
        if self.fine_pricing_reference_path_count == 0 {
            return Err(
                CoarseFineConsistencyRegressionExecutionConfigError::InvalidFinePricingPathCount {
                    value: self.fine_pricing_reference_path_count,
                },
            );
        }
        if self.fine_pricing_reference_step_count == 0 {
            return Err(
                CoarseFineConsistencyRegressionExecutionConfigError::InvalidFinePricingStepCount {
                    value: self.fine_pricing_reference_step_count,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseFineConsistencyRegressionExecutionConfigError {
    InvalidFineSubintervalCount { value: usize },
    InvalidRiskPathCount { value: usize },
    InvalidFinePricingPathCount { value: usize },
    InvalidFinePricingStepCount { value: usize },
}

impl Display for CoarseFineConsistencyRegressionExecutionConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFineSubintervalCount { value } => write!(
                formatter,
                "execution_config.fine_subinterval_count_per_coarse_interval must be greater than 0, found {value}"
            ),
            Self::InvalidRiskPathCount { value } => write!(
                formatter,
                "execution_config.paired_risk_path_count must be greater than 0, found {value}"
            ),
            Self::InvalidFinePricingPathCount { value } => write!(
                formatter,
                "execution_config.fine_pricing_reference_path_count must be greater than 0, found {value}"
            ),
            Self::InvalidFinePricingStepCount { value } => write!(
                formatter,
                "execution_config.fine_pricing_reference_step_count must be greater than 0, found {value}"
            ),
        }
    }
}

impl Error for CoarseFineConsistencyRegressionExecutionConfigError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseFineConsistencyRegressionToleranceConfig {
    pub schema_version: String,
    pub policy_id: String,
    pub policy_version: String,
    pub pricing_price_absolute_delta_tolerance: f64,
    pub pricing_price_relative_delta_tolerance: f64,
    pub risk_integrated_intensity_mean_absolute_delta_tolerance: f64,
    pub risk_integrated_intensity_mean_relative_delta_tolerance: f64,
    pub risk_event_count_mean_absolute_delta_tolerance: f64,
    pub risk_event_count_mean_relative_delta_tolerance: f64,
}

impl Default for CoarseFineConsistencyRegressionToleranceConfig {
    fn default() -> Self {
        Self {
            schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION.to_string(),
            policy_id: "coarse_fine_consistency_default_tolerance".to_string(),
            policy_version: "1.0.0".to_string(),
            pricing_price_absolute_delta_tolerance: 0.20,
            pricing_price_relative_delta_tolerance: 0.15,
            risk_integrated_intensity_mean_absolute_delta_tolerance: 0.65,
            risk_integrated_intensity_mean_relative_delta_tolerance: 0.65,
            risk_event_count_mean_absolute_delta_tolerance: 0.75,
            risk_event_count_mean_relative_delta_tolerance: 0.75,
        }
    }
}

impl CoarseFineConsistencyRegressionToleranceConfig {
    pub fn validate(&self) -> Result<(), CoarseFineConsistencyRegressionToleranceConfigError> {
        if self.schema_version != COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION {
            return Err(
                CoarseFineConsistencyRegressionToleranceConfigError::InvalidSchemaVersion {
                    expected: COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION,
                    actual: self.schema_version.clone(),
                },
            );
        }
        if self.policy_id.trim().is_empty() {
            return Err(CoarseFineConsistencyRegressionToleranceConfigError::EmptyPolicyId);
        }
        if self.policy_version.trim().is_empty() {
            return Err(CoarseFineConsistencyRegressionToleranceConfigError::EmptyPolicyVersion);
        }

        validate_non_negative_finite_tolerance_field(
            "pricing_price_absolute_delta_tolerance",
            self.pricing_price_absolute_delta_tolerance,
        )?;
        validate_non_negative_finite_tolerance_field(
            "pricing_price_relative_delta_tolerance",
            self.pricing_price_relative_delta_tolerance,
        )?;
        validate_non_negative_finite_tolerance_field(
            "risk_integrated_intensity_mean_absolute_delta_tolerance",
            self.risk_integrated_intensity_mean_absolute_delta_tolerance,
        )?;
        validate_non_negative_finite_tolerance_field(
            "risk_integrated_intensity_mean_relative_delta_tolerance",
            self.risk_integrated_intensity_mean_relative_delta_tolerance,
        )?;
        validate_non_negative_finite_tolerance_field(
            "risk_event_count_mean_absolute_delta_tolerance",
            self.risk_event_count_mean_absolute_delta_tolerance,
        )?;
        validate_non_negative_finite_tolerance_field(
            "risk_event_count_mean_relative_delta_tolerance",
            self.risk_event_count_mean_relative_delta_tolerance,
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseFineConsistencyRegressionToleranceConfigError {
    InvalidSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    EmptyPolicyId,
    EmptyPolicyVersion,
    InvalidToleranceValue {
        field_name: &'static str,
        value: f64,
    },
}

impl Display for CoarseFineConsistencyRegressionToleranceConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion { expected, actual } => write!(
                formatter,
                "tolerance_config.schema_version must be `{expected}`, found `{actual}`"
            ),
            Self::EmptyPolicyId => {
                write!(formatter, "tolerance_config.policy_id must be non-empty")
            }
            Self::EmptyPolicyVersion => {
                write!(
                    formatter,
                    "tolerance_config.policy_version must be non-empty"
                )
            }
            Self::InvalidToleranceValue { field_name, value } => write!(
                formatter,
                "tolerance_config.{field_name} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for CoarseFineConsistencyRegressionToleranceConfigError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoarseFineConsistencyRegressionOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub tolerance_schema_version: &'static str,
    pub required_report_fields: Vec<&'static str>,
    pub required_scenario_report_fields: Vec<&'static str>,
    pub required_metric_delta_fields: Vec<&'static str>,
    pub required_run_metadata_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoarseFineConsistencyScenarioStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseFineConsistencyScenarioRunMetadata {
    pub scenario_id: String,
    pub profile_seed: u64,
    pub pricing_component_seed: u64,
    pub risk_component_seed: u64,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub horizon_interval_count: usize,
    pub fine_subinterval_count_per_coarse_interval: usize,
    pub paired_risk_path_count: usize,
    pub fine_pricing_reference_path_count: usize,
    pub fine_pricing_reference_step_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseFineConsistencyMetricDelta {
    pub metric_id: &'static str,
    pub coarse_value: f64,
    pub fine_reference_value: f64,
    pub absolute_delta: f64,
    pub relative_delta: f64,
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseFineConsistencyScenarioSummary {
    pub coarse_pricing_value: f64,
    pub fine_pricing_reference_value: f64,
    pub coarse_risk_mean_a_plus: f64,
    pub fine_risk_mean_a_plus: f64,
    pub coarse_risk_mean_a_minus: f64,
    pub fine_risk_mean_a_minus: f64,
    pub coarse_risk_mean_n_plus: f64,
    pub fine_risk_mean_n_plus: f64,
    pub coarse_risk_mean_n_minus: f64,
    pub fine_risk_mean_n_minus: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseFineConsistencyScenarioReport {
    pub scenario_id: String,
    pub status: CoarseFineConsistencyScenarioStatus,
    pub failed_metric_count: usize,
    pub max_absolute_delta: f64,
    pub run_metadata: CoarseFineConsistencyScenarioRunMetadata,
    pub summary: CoarseFineConsistencyScenarioSummary,
    pub metric_deltas: Vec<CoarseFineConsistencyMetricDelta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CoarseFineConsistencyRegressionSummary {
    pub total_scenario_count: usize,
    pub passed_scenario_count: usize,
    pub failed_scenario_count: usize,
    pub total_metric_count: usize,
    pub failed_metric_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseFineConsistencyRegressionReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub variance_control_policy: &'static str,
    pub metric_policy: &'static str,
    pub governance_scope_policy: &'static str,
    pub output_schema_metadata: CoarseFineConsistencyRegressionOutputSchemaMetadata,
    pub catalog_id: String,
    pub catalog_version: String,
    pub benchmark_policy_id: String,
    pub benchmark_policy_version: String,
    pub tolerance_config: CoarseFineConsistencyRegressionToleranceConfig,
    pub execution_config: CoarseFineConsistencyRegressionExecutionConfig,
    pub summary: CoarseFineConsistencyRegressionSummary,
    pub scenario_reports: Vec<CoarseFineConsistencyScenarioReport>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseFineConsistencyRegressionHarnessError {
    InvalidScenarioCatalog {
        source: BenchmarkScenarioCatalogLoadError,
    },
    InvalidBenchmarkPolicy {
        source: OraclePricerRiskBenchmarkPolicyValidationError,
    },
    InvalidExecutionConfig {
        source: CoarseFineConsistencyRegressionExecutionConfigError,
    },
    InvalidToleranceConfig {
        source: CoarseFineConsistencyRegressionToleranceConfigError,
    },
    ScenarioExperimentFailure {
        scenario_id: String,
        detail: String,
    },
}

impl Display for CoarseFineConsistencyRegressionHarnessError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidScenarioCatalog { source } => write!(
                formatter,
                "coarse fine consistency harness requires a valid scenario catalog: {source}"
            ),
            Self::InvalidBenchmarkPolicy { source } => write!(
                formatter,
                "coarse fine consistency harness requires a valid benchmark policy: {source}"
            ),
            Self::InvalidExecutionConfig { source } => write!(
                formatter,
                "coarse fine consistency execution configuration is invalid: {source}"
            ),
            Self::InvalidToleranceConfig { source } => write!(
                formatter,
                "coarse fine consistency tolerance configuration is invalid: {source}"
            ),
            Self::ScenarioExperimentFailure {
                scenario_id,
                detail,
            } => write!(
                formatter,
                "scenario `{scenario_id}` coarse fine experiment failed: {detail}"
            ),
        }
    }
}

impl Error for CoarseFineConsistencyRegressionHarnessError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidScenarioCatalog { source } => Some(source),
            Self::InvalidBenchmarkPolicy { source } => Some(source),
            Self::InvalidExecutionConfig { source } => Some(source),
            Self::InvalidToleranceConfig { source } => Some(source),
            Self::ScenarioExperimentFailure { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CoarseFineRiskMetricMeans {
    mean_a_plus: f64,
    mean_a_minus: f64,
    mean_n_plus: f64,
    mean_n_minus: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct CoarseFineRiskMetricAccumulator {
    sum_a_plus: f64,
    sum_a_minus: f64,
    sum_n_plus: f64,
    sum_n_minus: f64,
    observation_count: usize,
}

impl CoarseFineRiskMetricAccumulator {
    fn observe(
        &mut self,
        a_plus: f64,
        a_minus: f64,
        n_plus: f64,
        n_minus: f64,
    ) -> Result<(), &'static str> {
        if !a_plus.is_finite()
            || !a_minus.is_finite()
            || !n_plus.is_finite()
            || !n_minus.is_finite()
        {
            return Err("non-finite sufficient-statistics sample");
        }

        self.sum_a_plus += a_plus;
        self.sum_a_minus += a_minus;
        self.sum_n_plus += n_plus;
        self.sum_n_minus += n_minus;
        self.observation_count = self.observation_count.saturating_add(1);

        Ok(())
    }

    fn finalize(self) -> Option<CoarseFineRiskMetricMeans> {
        if self.observation_count == 0 {
            return None;
        }

        let denominator = self.observation_count as f64;

        Some(CoarseFineRiskMetricMeans {
            mean_a_plus: self.sum_a_plus / denominator,
            mean_a_minus: self.sum_a_minus / denominator,
            mean_n_plus: self.sum_n_plus / denominator,
            mean_n_minus: self.sum_n_minus / denominator,
        })
    }
}

pub fn run_coarse_fine_consistency_regression_harness(
    catalog: &BenchmarkScenarioCatalog,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
    tolerance_config: CoarseFineConsistencyRegressionToleranceConfig,
    execution_config: CoarseFineConsistencyRegressionExecutionConfig,
) -> Result<CoarseFineConsistencyRegressionReport, CoarseFineConsistencyRegressionHarnessError> {
    benchmark_policy.validate().map_err(|source| {
        CoarseFineConsistencyRegressionHarnessError::InvalidBenchmarkPolicy { source }
    })?;
    tolerance_config.validate().map_err(|source| {
        CoarseFineConsistencyRegressionHarnessError::InvalidToleranceConfig { source }
    })?;
    execution_config.validate().map_err(|source| {
        CoarseFineConsistencyRegressionHarnessError::InvalidExecutionConfig { source }
    })?;

    let mut normalized_catalog = catalog.clone();
    normalized_catalog.validate().map_err(|source| {
        CoarseFineConsistencyRegressionHarnessError::InvalidScenarioCatalog { source }
    })?;
    normalized_catalog.sort_profiles_canonically();

    let mut scenario_reports: Vec<CoarseFineConsistencyScenarioReport> =
        Vec::with_capacity(normalized_catalog.profiles.len());
    for profile in &normalized_catalog.profiles {
        let scenario_report = run_coarse_fine_consistency_scenario(
            profile,
            benchmark_policy,
            &tolerance_config,
            execution_config,
        )?;
        scenario_reports.push(scenario_report);
    }

    let passed_scenario_count = scenario_reports
        .iter()
        .filter(|scenario_report| {
            scenario_report.status == CoarseFineConsistencyScenarioStatus::Pass
        })
        .count();
    let total_scenario_count = scenario_reports.len();
    let failed_scenario_count = total_scenario_count - passed_scenario_count;
    let total_metric_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.metric_deltas.len())
        .sum();
    let failed_metric_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.failed_metric_count)
        .sum();

    Ok(CoarseFineConsistencyRegressionReport {
        schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION,
        determinism_policy: COARSE_FINE_CONSISTENCY_REGRESSION_DETERMINISM_POLICY,
        variance_control_policy: COARSE_FINE_CONSISTENCY_REGRESSION_VARIANCE_CONTROL_POLICY,
        metric_policy: COARSE_FINE_CONSISTENCY_REGRESSION_METRIC_POLICY,
        governance_scope_policy: COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY,
        output_schema_metadata: CoarseFineConsistencyRegressionOutputSchemaMetadata {
            schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
            tolerance_schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION,
            required_report_fields: COARSE_FINE_CONSISTENCY_REQUIRED_REPORT_FIELDS.to_vec(),
            required_scenario_report_fields:
                COARSE_FINE_CONSISTENCY_REQUIRED_SCENARIO_REPORT_FIELDS.to_vec(),
            required_metric_delta_fields: COARSE_FINE_CONSISTENCY_REQUIRED_METRIC_DELTA_FIELDS
                .to_vec(),
            required_run_metadata_fields: COARSE_FINE_CONSISTENCY_REQUIRED_RUN_METADATA_FIELDS
                .to_vec(),
        },
        catalog_id: normalized_catalog.catalog_id,
        catalog_version: normalized_catalog.catalog_version,
        benchmark_policy_id: benchmark_policy.policy_id.clone(),
        benchmark_policy_version: benchmark_policy.policy_version.clone(),
        tolerance_config,
        execution_config,
        summary: CoarseFineConsistencyRegressionSummary {
            total_scenario_count,
            passed_scenario_count,
            failed_scenario_count,
            total_metric_count,
            failed_metric_count,
        },
        scenario_reports,
    })
}

fn run_coarse_fine_consistency_scenario(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
    tolerance_config: &CoarseFineConsistencyRegressionToleranceConfig,
    execution_config: CoarseFineConsistencyRegressionExecutionConfig,
) -> Result<CoarseFineConsistencyScenarioReport, CoarseFineConsistencyRegressionHarnessError> {
    let scenario_id = profile.profile_id.clone();
    let profile_seed = splitmix64(
        execution_config
            .reproducibility_seed
            .wrapping_add(fnv1a_64(profile.profile_id.as_bytes())),
    );
    let pricing_component_seed =
        splitmix64(profile_seed ^ benchmark_policy.seed_partition.pricing_component_seed_salt);
    let risk_component_seed =
        splitmix64(profile_seed ^ benchmark_policy.seed_partition.risk_component_seed_salt);
    let fine_interval_delta = profile.interval_settings.interval_delta
        / execution_config.fine_subinterval_count_per_coarse_interval as f64;

    let equation_specification =
        build_equation_specification(profile, benchmark_policy).map_err(|source| {
            coarse_fine_scenario_error(
                &scenario_id,
                format!("failed to build equation specification: {source}"),
            )
        })?;
    let oracle_solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        profile.interval_settings.oracle_solver_step_count,
    )
    .map_err(|source| {
        coarse_fine_scenario_error(
            &scenario_id,
            format!("failed to construct oracle solver settings: {source}"),
        )
    })?;
    let pricing_engine_settings = build_pricing_engine_settings(profile, benchmark_policy)
        .map_err(|source| {
            coarse_fine_scenario_error(
                &scenario_id,
                format!("failed to construct pricing engine settings: {source}"),
            )
        })?;

    let pricing_invocation_seed = derive_invocation_seed(pricing_component_seed, 1);
    let pricing_market_inputs =
        build_pricing_market_inputs(profile, benchmark_policy, pricing_invocation_seed).map_err(
            |source| {
                coarse_fine_scenario_error(
                    &scenario_id,
                    format!("failed to build pricing market inputs: {source}"),
                )
            },
        )?;
    let pricing_payoff_type = VanillaOptionPayoffType::Call;
    let pricing_payoff = VanillaOptionPayoffConfiguration {
        payoff_type: pricing_payoff_type,
    };
    let pricing_volatility = interpolate_range(
        benchmark_policy.pricing_input_ranges.volatility_min,
        benchmark_policy.pricing_input_ranges.volatility_max,
        unit_interval_from_seed(pricing_invocation_seed ^ 0x51),
    );

    let pricing_spot = pricing_market_inputs.spot;
    let pricing_strike = pricing_market_inputs.strike;
    let pricing_maturity_interval = pricing_market_inputs.maturity_interval;
    let pricing_risk_free_rate = pricing_market_inputs.risk_free_rate;
    let pricing_dividend_yield = pricing_market_inputs.dividend_yield;

    let pricing_drift = (pricing_risk_free_rate
        - pricing_dividend_yield
        - 0.5 * pricing_volatility * pricing_volatility)
        * pricing_maturity_interval;
    let pricing_variance = pricing_volatility * pricing_volatility * pricing_maturity_interval;

    let coarse_pricing_result = price_vanilla_option_via_fourier_inversion(
        pricing_market_inputs,
        pricing_payoff,
        pricing_engine_settings,
        |argument| {
            let i_times_argument = ComplexScalar::new(-argument.imaginary, argument.real);
            let drift_term = i_times_argument.scale(pricing_drift);
            let diffusion_term = argument.square().scale(-0.5 * pricing_variance);
            complex_exponential(drift_term.add(diffusion_term))
        },
    )
    .map_err(|source| {
        coarse_fine_scenario_error(
            &scenario_id,
            format!("coarse pricing call failed: {source}"),
        )
    })?;
    let coarse_pricing_value = coarse_pricing_result.price;

    let fine_pricing_reference_value = estimate_fine_pricing_reference_price(
        &scenario_id,
        pricing_component_seed,
        execution_config.fine_pricing_reference_path_count,
        execution_config.fine_pricing_reference_step_count,
        pricing_spot,
        pricing_strike,
        pricing_maturity_interval,
        pricing_risk_free_rate,
        pricing_dividend_yield,
        pricing_volatility,
        pricing_payoff_type,
    )?;

    let coarse_risk_factor_series =
        build_coarse_risk_factor_series(profile, benchmark_policy, risk_component_seed);
    let fine_risk_factor_series = expand_risk_factor_series_for_fine_reference(
        &coarse_risk_factor_series,
        execution_config.fine_subinterval_count_per_coarse_interval,
    );

    let coarse_risk_slices = solve_coarse_fine_risk_slices(
        &scenario_id,
        equation_specification,
        oracle_solver_settings,
        profile.model_state.initial_excitation_state,
        profile.interval_settings.interval_delta,
        &coarse_risk_factor_series,
    )?;
    let fine_risk_slices = solve_coarse_fine_risk_slices(
        &scenario_id,
        equation_specification,
        oracle_solver_settings,
        profile.model_state.initial_excitation_state,
        fine_interval_delta,
        &fine_risk_factor_series,
    )?;

    let coarse_risk_metric_means = sample_coarse_risk_metric_means(
        &scenario_id,
        risk_component_seed,
        execution_config.paired_risk_path_count,
        &coarse_risk_slices,
    )?;
    let fine_risk_metric_means = sample_fine_risk_reference_metric_means(
        &scenario_id,
        risk_component_seed,
        execution_config.paired_risk_path_count,
        profile.interval_settings.horizon_interval_count,
        execution_config.fine_subinterval_count_per_coarse_interval,
        &fine_risk_slices,
    )?;

    let metric_deltas = vec![
        build_coarse_fine_metric_delta(
            "pricing_vanilla_option_price",
            coarse_pricing_value,
            fine_pricing_reference_value,
            tolerance_config.pricing_price_absolute_delta_tolerance,
            tolerance_config.pricing_price_relative_delta_tolerance,
        ),
        build_coarse_fine_metric_delta(
            "risk_mean_a_plus",
            coarse_risk_metric_means.mean_a_plus,
            fine_risk_metric_means.mean_a_plus,
            tolerance_config.risk_integrated_intensity_mean_absolute_delta_tolerance,
            tolerance_config.risk_integrated_intensity_mean_relative_delta_tolerance,
        ),
        build_coarse_fine_metric_delta(
            "risk_mean_a_minus",
            coarse_risk_metric_means.mean_a_minus,
            fine_risk_metric_means.mean_a_minus,
            tolerance_config.risk_integrated_intensity_mean_absolute_delta_tolerance,
            tolerance_config.risk_integrated_intensity_mean_relative_delta_tolerance,
        ),
        build_coarse_fine_metric_delta(
            "risk_mean_n_plus",
            coarse_risk_metric_means.mean_n_plus,
            fine_risk_metric_means.mean_n_plus,
            tolerance_config.risk_event_count_mean_absolute_delta_tolerance,
            tolerance_config.risk_event_count_mean_relative_delta_tolerance,
        ),
        build_coarse_fine_metric_delta(
            "risk_mean_n_minus",
            coarse_risk_metric_means.mean_n_minus,
            fine_risk_metric_means.mean_n_minus,
            tolerance_config.risk_event_count_mean_absolute_delta_tolerance,
            tolerance_config.risk_event_count_mean_relative_delta_tolerance,
        ),
    ];

    let failed_metric_count = metric_deltas
        .iter()
        .filter(|metric_delta| !metric_delta.passed)
        .count();
    let max_absolute_delta = metric_deltas
        .iter()
        .map(|metric_delta| metric_delta.absolute_delta)
        .fold(0.0_f64, f64::max);
    let status = if failed_metric_count == 0 {
        CoarseFineConsistencyScenarioStatus::Pass
    } else {
        CoarseFineConsistencyScenarioStatus::Fail
    };

    Ok(CoarseFineConsistencyScenarioReport {
        scenario_id: scenario_id.clone(),
        status,
        failed_metric_count,
        max_absolute_delta,
        run_metadata: CoarseFineConsistencyScenarioRunMetadata {
            scenario_id,
            profile_seed,
            pricing_component_seed,
            risk_component_seed,
            coarse_interval_delta: profile.interval_settings.interval_delta,
            fine_interval_delta,
            horizon_interval_count: profile.interval_settings.horizon_interval_count,
            fine_subinterval_count_per_coarse_interval: execution_config
                .fine_subinterval_count_per_coarse_interval,
            paired_risk_path_count: execution_config.paired_risk_path_count,
            fine_pricing_reference_path_count: execution_config.fine_pricing_reference_path_count,
            fine_pricing_reference_step_count: execution_config.fine_pricing_reference_step_count,
        },
        summary: CoarseFineConsistencyScenarioSummary {
            coarse_pricing_value,
            fine_pricing_reference_value,
            coarse_risk_mean_a_plus: coarse_risk_metric_means.mean_a_plus,
            fine_risk_mean_a_plus: fine_risk_metric_means.mean_a_plus,
            coarse_risk_mean_a_minus: coarse_risk_metric_means.mean_a_minus,
            fine_risk_mean_a_minus: fine_risk_metric_means.mean_a_minus,
            coarse_risk_mean_n_plus: coarse_risk_metric_means.mean_n_plus,
            fine_risk_mean_n_plus: fine_risk_metric_means.mean_n_plus,
            coarse_risk_mean_n_minus: coarse_risk_metric_means.mean_n_minus,
            fine_risk_mean_n_minus: fine_risk_metric_means.mean_n_minus,
        },
        metric_deltas,
    })
}

fn coarse_fine_scenario_error(
    scenario_id: &str,
    detail: String,
) -> CoarseFineConsistencyRegressionHarnessError {
    CoarseFineConsistencyRegressionHarnessError::ScenarioExperimentFailure {
        scenario_id: scenario_id.to_string(),
        detail,
    }
}

fn validate_non_negative_finite_tolerance_field(
    field_name: &'static str,
    value: f64,
) -> Result<(), CoarseFineConsistencyRegressionToleranceConfigError> {
    if value.is_finite() && value >= 0.0 {
        return Ok(());
    }

    Err(
        CoarseFineConsistencyRegressionToleranceConfigError::InvalidToleranceValue {
            field_name,
            value,
        },
    )
}

fn build_coarse_fine_metric_delta(
    metric_id: &'static str,
    coarse_value: f64,
    fine_reference_value: f64,
    absolute_tolerance: f64,
    relative_tolerance: f64,
) -> CoarseFineConsistencyMetricDelta {
    let absolute_delta = if coarse_value.is_finite() && fine_reference_value.is_finite() {
        (coarse_value - fine_reference_value).abs()
    } else {
        f64::INFINITY
    };
    let denominator = fine_reference_value.abs().max(1.0e-12);
    let relative_delta = if absolute_delta.is_finite() {
        absolute_delta / denominator
    } else {
        f64::INFINITY
    };
    let passed = absolute_delta <= absolute_tolerance || relative_delta <= relative_tolerance;

    CoarseFineConsistencyMetricDelta {
        metric_id,
        coarse_value,
        fine_reference_value,
        absolute_delta,
        relative_delta,
        absolute_tolerance,
        relative_tolerance,
        passed,
    }
}

fn build_coarse_risk_factor_series(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
    risk_component_seed: u64,
) -> Vec<(f64, f64)> {
    let mut factor_series: Vec<(f64, f64)> =
        Vec::with_capacity(profile.interval_settings.horizon_interval_count);
    for interval_index in 0..profile.interval_settings.horizon_interval_count {
        let interval_seed = derive_invocation_seed(risk_component_seed, interval_index + 1);
        let factor_noise = unit_interval_from_seed(interval_seed);
        let seasonality_factor = interpolate_range(
            benchmark_policy.oracle_solver_probes.seasonality_factor_min,
            benchmark_policy.oracle_solver_probes.seasonality_factor_max,
            factor_noise,
        );
        let news_intensity_factor = interpolate_range(
            benchmark_policy
                .oracle_solver_probes
                .news_intensity_factor_min,
            benchmark_policy
                .oracle_solver_probes
                .news_intensity_factor_max,
            1.0 - factor_noise,
        );

        factor_series.push((seasonality_factor, news_intensity_factor));
    }

    factor_series
}

fn expand_risk_factor_series_for_fine_reference(
    coarse_factor_series: &[(f64, f64)],
    fine_subinterval_count_per_coarse_interval: usize,
) -> Vec<(f64, f64)> {
    let mut fine_factor_series: Vec<(f64, f64)> =
        Vec::with_capacity(coarse_factor_series.len() * fine_subinterval_count_per_coarse_interval);

    for &(seasonality_factor, news_intensity_factor) in coarse_factor_series {
        for _ in 0..fine_subinterval_count_per_coarse_interval {
            fine_factor_series.push((seasonality_factor, news_intensity_factor));
        }
    }

    fine_factor_series
}

fn solve_coarse_fine_risk_slices(
    scenario_id: &str,
    equation_specification: ExponentialAffineTransformEquationSpecification,
    solver_settings: ExponentialAffineTransformSolverSettings,
    initial_excitation_state: HawkesExcitationState,
    interval_delta: f64,
    factor_series: &[(f64, f64)],
) -> Result<Vec<SufficientStatisticsTransformSlices>, CoarseFineConsistencyRegressionHarnessError> {
    let sampling_settings = SufficientStatisticsTransformSliceSettings::default();
    let mut slices_by_interval: Vec<SufficientStatisticsTransformSlices> =
        Vec::with_capacity(factor_series.len());

    for (interval_index, (seasonality_factor, news_intensity_factor)) in
        factor_series.iter().copied().enumerate()
    {
        let context = SufficientStatisticsTransformSliceContext::try_new(
            interval_index,
            seasonality_factor,
            news_intensity_factor,
            interval_delta,
            initial_excitation_state,
        )
        .map_err(|source| {
            coarse_fine_scenario_error(
                scenario_id,
                format!("interval {interval_index} risk slice context validation failed: {source}"),
            )
        })?;

        let solved_slices = solve_sufficient_statistics_transform_slices_with_solver_settings(
            equation_specification,
            context,
            sampling_settings,
            Some(solver_settings),
        )
        .map_err(|source| {
            coarse_fine_scenario_error(
                scenario_id,
                format!("interval {interval_index} risk slice solve failed: {source}"),
            )
        })?;

        slices_by_interval.push(solved_slices);
    }

    Ok(slices_by_interval)
}

fn sample_coarse_risk_metric_means(
    scenario_id: &str,
    risk_component_seed: u64,
    paired_risk_path_count: usize,
    slices_by_interval: &[SufficientStatisticsTransformSlices],
) -> Result<CoarseFineRiskMetricMeans, CoarseFineConsistencyRegressionHarnessError> {
    let mut accumulator = CoarseFineRiskMetricAccumulator::default();

    for path_index in 0..paired_risk_path_count {
        let path_seed = derive_invocation_seed(risk_component_seed, path_index + 1);
        let sampled_statistics =
            sample_interval_statistics_batch_from_transform_slices(path_seed, slices_by_interval)
                .map_err(|source| {
                coarse_fine_scenario_error(
                    scenario_id,
                    format!("coarse risk slice sampling failed: {source}"),
                )
            })?;

        for interval_statistics in sampled_statistics {
            accumulator
                .observe(
                    interval_statistics.a_plus,
                    interval_statistics.a_minus,
                    interval_statistics.n_plus as f64,
                    interval_statistics.n_minus as f64,
                )
                .map_err(|detail| coarse_fine_scenario_error(scenario_id, detail.to_string()))?;
        }
    }

    let metric_means = accumulator.finalize().ok_or_else(|| {
        coarse_fine_scenario_error(
            scenario_id,
            "coarse risk metric aggregation produced no observations".to_string(),
        )
    })?;

    validate_risk_metric_means(scenario_id, "coarse", metric_means)
}

fn sample_fine_risk_reference_metric_means(
    scenario_id: &str,
    risk_component_seed: u64,
    paired_risk_path_count: usize,
    coarse_interval_count: usize,
    fine_subinterval_count_per_coarse_interval: usize,
    slices_by_interval: &[SufficientStatisticsTransformSlices],
) -> Result<CoarseFineRiskMetricMeans, CoarseFineConsistencyRegressionHarnessError> {
    let expected_fine_interval_count =
        coarse_interval_count.saturating_mul(fine_subinterval_count_per_coarse_interval);
    if slices_by_interval.len() != expected_fine_interval_count {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!(
                "fine slice count mismatch, expected {expected_fine_interval_count} intervals but found {}",
                slices_by_interval.len()
            ),
        ));
    }

    let mut accumulator = CoarseFineRiskMetricAccumulator::default();
    for path_index in 0..paired_risk_path_count {
        let path_seed = derive_invocation_seed(risk_component_seed, path_index + 1);
        let sampled_statistics =
            sample_interval_statistics_batch_from_transform_slices(path_seed, slices_by_interval)
                .map_err(|source| {
                coarse_fine_scenario_error(
                    scenario_id,
                    format!("fine risk slice sampling failed: {source}"),
                )
            })?;
        if sampled_statistics.len() != expected_fine_interval_count {
            return Err(coarse_fine_scenario_error(
                scenario_id,
                format!(
                    "fine sampled-statistics count mismatch, expected {expected_fine_interval_count} entries but found {}",
                    sampled_statistics.len()
                ),
            ));
        }

        for coarse_interval_index in 0..coarse_interval_count {
            let fine_start =
                coarse_interval_index.saturating_mul(fine_subinterval_count_per_coarse_interval);
            let fine_end = fine_start + fine_subinterval_count_per_coarse_interval;
            let mut aggregated_a_plus = 0.0_f64;
            let mut aggregated_a_minus = 0.0_f64;
            let mut aggregated_n_plus = 0.0_f64;
            let mut aggregated_n_minus = 0.0_f64;

            for interval_statistics in &sampled_statistics[fine_start..fine_end] {
                aggregated_a_plus += interval_statistics.a_plus;
                aggregated_a_minus += interval_statistics.a_minus;
                aggregated_n_plus += interval_statistics.n_plus as f64;
                aggregated_n_minus += interval_statistics.n_minus as f64;
            }

            accumulator
                .observe(
                    aggregated_a_plus,
                    aggregated_a_minus,
                    aggregated_n_plus,
                    aggregated_n_minus,
                )
                .map_err(|detail| coarse_fine_scenario_error(scenario_id, detail.to_string()))?;
        }
    }

    let metric_means = accumulator.finalize().ok_or_else(|| {
        coarse_fine_scenario_error(
            scenario_id,
            "fine risk metric aggregation produced no observations".to_string(),
        )
    })?;
    validate_risk_metric_means(scenario_id, "fine", metric_means)
}

fn validate_risk_metric_means(
    scenario_id: &str,
    label: &str,
    metric_means: CoarseFineRiskMetricMeans,
) -> Result<CoarseFineRiskMetricMeans, CoarseFineConsistencyRegressionHarnessError> {
    let fields = [
        ("mean_a_plus", metric_means.mean_a_plus),
        ("mean_a_minus", metric_means.mean_a_minus),
        ("mean_n_plus", metric_means.mean_n_plus),
        ("mean_n_minus", metric_means.mean_n_minus),
    ];
    for (field_name, value) in fields {
        if !value.is_finite() {
            return Err(coarse_fine_scenario_error(
                scenario_id,
                format!("{label} risk metric `{field_name}` is non-finite: {value}"),
            ));
        }
    }

    Ok(metric_means)
}

#[allow(clippy::too_many_arguments)]
fn estimate_fine_pricing_reference_price(
    scenario_id: &str,
    pricing_component_seed: u64,
    path_count: usize,
    step_count: usize,
    spot: f64,
    strike: f64,
    maturity_interval: f64,
    risk_free_rate: f64,
    dividend_yield: f64,
    volatility: f64,
    payoff_type: VanillaOptionPayoffType,
) -> Result<f64, CoarseFineConsistencyRegressionHarnessError> {
    let fields = [
        ("spot", spot),
        ("strike", strike),
        ("maturity_interval", maturity_interval),
        ("risk_free_rate", risk_free_rate),
        ("dividend_yield", dividend_yield),
        ("volatility", volatility),
    ];
    for (field_name, value) in fields {
        if !value.is_finite() {
            return Err(coarse_fine_scenario_error(
                scenario_id,
                format!(
                    "fine pricing reference input `{field_name}` must be finite, found {value}"
                ),
            ));
        }
    }

    if spot <= 0.0 {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!("fine pricing reference input `spot` must be positive, found {spot}"),
        ));
    }
    if strike <= 0.0 {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!("fine pricing reference input `strike` must be positive, found {strike}"),
        ));
    }
    if maturity_interval <= 0.0 {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!(
                "fine pricing reference input `maturity_interval` must be positive, found {maturity_interval}"
            ),
        ));
    }
    if volatility <= 0.0 {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!(
                "fine pricing reference input `volatility` must be positive, found {volatility}"
            ),
        ));
    }

    let time_step = maturity_interval / step_count as f64;
    let drift_per_step =
        (risk_free_rate - dividend_yield - 0.5 * volatility * volatility) * time_step;
    let volatility_per_step = volatility * time_step.sqrt();
    let mut payoff_sum = 0.0_f64;

    for path_index in 0..path_count {
        let path_seed = derive_invocation_seed(pricing_component_seed, path_index + 1);
        let mut log_terminal_spot = spot.ln();

        for step_index in 0..step_count {
            let step_seed = derive_invocation_seed(path_seed, step_index + 1);
            let standard_normal = sample_standard_normal_from_seed(step_seed);

            log_terminal_spot += drift_per_step + volatility_per_step * standard_normal;
        }

        let terminal_spot = log_terminal_spot.exp();
        let path_payoff = match payoff_type {
            VanillaOptionPayoffType::Call => (terminal_spot - strike).max(0.0),
            VanillaOptionPayoffType::Put => (strike - terminal_spot).max(0.0),
        };
        payoff_sum += path_payoff;
    }

    let discounted_price =
        (-risk_free_rate * maturity_interval).exp() * (payoff_sum / path_count as f64);
    if !discounted_price.is_finite() {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!("fine pricing reference value is non-finite: {discounted_price}"),
        ));
    }

    Ok(discounted_price)
}

fn sample_standard_normal_from_seed(seed: u64) -> f64 {
    let first_uniform_draw = unit_interval_from_seed(seed ^ 0x8B77_D4C9_14F2_5A11)
        .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);
    let second_uniform_draw = unit_interval_from_seed(seed ^ 0xC4D5_1E6A_227A_F093);
    let radius = (-2.0 * first_uniform_draw.ln()).sqrt();
    let angle = 2.0 * std::f64::consts::PI * second_uniform_draw;

    radius * angle.cos()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeValidationPolicy {
    Finite,
    NonNegative,
    StrictlyPositive,
}

fn validate_non_empty_rationale_field(
    field_name: &'static str,
    value: &str,
) -> Result<(), OraclePricerRiskBenchmarkPolicyValidationError> {
    if value.trim().is_empty() {
        return Err(
            OraclePricerRiskBenchmarkPolicyValidationError::EmptyRationaleField { field_name },
        );
    }

    Ok(())
}

fn validate_range(
    field_name: &'static str,
    min_value: f64,
    max_value: f64,
    policy: RangeValidationPolicy,
) -> Result<(), OraclePricerRiskBenchmarkPolicyValidationError> {
    if !min_value.is_finite() {
        return Err(
            OraclePricerRiskBenchmarkPolicyValidationError::InvalidFiniteField {
                field_name,
                value: min_value,
            },
        );
    }
    if !max_value.is_finite() {
        return Err(
            OraclePricerRiskBenchmarkPolicyValidationError::InvalidFiniteField {
                field_name,
                value: max_value,
            },
        );
    }

    match policy {
        RangeValidationPolicy::Finite => {}
        RangeValidationPolicy::NonNegative => {
            if min_value < 0.0 {
                return Err(
                    OraclePricerRiskBenchmarkPolicyValidationError::InvalidNonNegativeFiniteField {
                        field_name,
                        value: min_value,
                    },
                );
            }
            if max_value < 0.0 {
                return Err(
                    OraclePricerRiskBenchmarkPolicyValidationError::InvalidNonNegativeFiniteField {
                        field_name,
                        value: max_value,
                    },
                );
            }
        }
        RangeValidationPolicy::StrictlyPositive => {
            if min_value <= 0.0 {
                return Err(
                    OraclePricerRiskBenchmarkPolicyValidationError::InvalidPositiveFiniteField {
                        field_name,
                        value: min_value,
                    },
                );
            }
            if max_value <= 0.0 {
                return Err(
                    OraclePricerRiskBenchmarkPolicyValidationError::InvalidPositiveFiniteField {
                        field_name,
                        value: max_value,
                    },
                );
            }
        }
    }

    if min_value > max_value {
        return Err(
            OraclePricerRiskBenchmarkPolicyValidationError::InvalidRangeField {
                field_name,
                min_value,
                max_value,
            },
        );
    }

    Ok(())
}

fn validate_positive_finite_policy_field(
    field_name: &'static str,
    value: f64,
) -> Result<(), OraclePricerRiskBenchmarkPolicyValidationError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(
            OraclePricerRiskBenchmarkPolicyValidationError::InvalidPositiveFiniteField {
                field_name,
                value,
            },
        );
    }

    Ok(())
}

fn validate_non_negative_finite_policy_field(
    field_name: &'static str,
    value: f64,
) -> Result<(), OraclePricerRiskBenchmarkPolicyValidationError> {
    if !value.is_finite() || value < 0.0 {
        return Err(
            OraclePricerRiskBenchmarkPolicyValidationError::InvalidNonNegativeFiniteField {
                field_name,
                value,
            },
        );
    }

    Ok(())
}

fn interpolate_range(min_value: f64, max_value: f64, noise: f64) -> f64 {
    min_value + (max_value - min_value) * noise
}

fn splitmix64(value: u64) -> u64 {
    let mut mixed = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^ (mixed >> 31)
}

fn derive_invocation_seed(component_seed: u64, invocation_count_including_warmup: usize) -> u64 {
    splitmix64(component_seed ^ invocation_count_including_warmup as u64)
}

fn unit_interval_from_seed(seed: u64) -> f64 {
    let sampled_integer = splitmix64(seed) >> 11;
    sampled_integer as f64 * (1.0 / ((1_u64 << 53) as f64))
}

fn fnv1a_64(payload: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for value in payload {
        hash ^= *value as u64;
        hash = hash.wrapping_mul(0x1000_0000_01B3);
    }

    hash
}
