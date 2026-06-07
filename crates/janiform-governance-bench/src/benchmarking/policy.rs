use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_foundations::numeric::{validate_non_negative_number, validate_positive_number};
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameterError, HawkesKernelParameters,
};
use janiform_model_kernels::news::{GammaSeverityMarkConfiguration, NewsMarkConfigurationError};

pub const ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_POLICY_SCHEMA_VERSION: &str =
    "benchmark.oracle_pricer_sufficient_statistics.policy.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerSufficientStatisticsBenchmarkPolicyRationale {
    pub model_parameters: String,
    pub oracle_solver_probes: String,
    pub pricing_input_ranges: String,
    pub seed_partition: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerSufficientStatisticsBenchmarkModelParameters {
    pub hawkes_beta: f64,
    pub hawkes_alpha: HawkesKernelLoadings,
    pub hawkes_rho: HawkesKernelLoadings,
    pub gamma_shape_k: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerSufficientStatisticsBenchmarkOracleSolverProbes {
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
pub struct OraclePricerSufficientStatisticsBenchmarkPricingInputRanges {
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
pub struct OraclePricerSufficientStatisticsBenchmarkSeedPartition {
    pub oracle_component_seed_salt: u64,
    pub pricing_component_seed_salt: u64,
    pub sufficient_statistics_component_seed_salt: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerSufficientStatisticsBenchmarkPolicy {
    pub schema_version: String,
    pub policy_id: String,
    pub policy_version: String,
    pub rationale: OraclePricerSufficientStatisticsBenchmarkPolicyRationale,
    pub model_parameters: OraclePricerSufficientStatisticsBenchmarkModelParameters,
    pub oracle_solver_probes: OraclePricerSufficientStatisticsBenchmarkOracleSolverProbes,
    pub pricing_input_ranges: OraclePricerSufficientStatisticsBenchmarkPricingInputRanges,
    pub seed_partition: OraclePricerSufficientStatisticsBenchmarkSeedPartition,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerSufficientStatisticsBenchmarkExecutionConfig {
    pub warmup_iteration_count_override: Option<usize>,
    pub measured_iteration_count_override: Option<usize>,
    pub reproducibility_seed: u64,
}

impl OraclePricerSufficientStatisticsBenchmarkExecutionConfig {
    pub fn validate(
        self,
    ) -> Result<(), OraclePricerSufficientStatisticsBenchmarkExecutionConfigError> {
        if self.measured_iteration_count_override == Some(0) {
            return Err(
                OraclePricerSufficientStatisticsBenchmarkExecutionConfigError::InvalidMeasuredIterationCountOverride {
                    value: 0,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerSufficientStatisticsBenchmarkExecutionConfigError {
    InvalidMeasuredIterationCountOverride { value: usize },
}

impl Display for OraclePricerSufficientStatisticsBenchmarkExecutionConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMeasuredIterationCountOverride { value } => write!(
                formatter,
                "execution_config.measured_iteration_count_override must be greater than 0 when provided, found {value}"
            ),
        }
    }
}

impl Error for OraclePricerSufficientStatisticsBenchmarkExecutionConfigError {}

impl OraclePricerSufficientStatisticsBenchmarkPolicy {
    pub fn validate(
        &self,
    ) -> Result<(), OraclePricerSufficientStatisticsBenchmarkPolicyValidationError> {
        if self.schema_version
            != ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_POLICY_SCHEMA_VERSION
        {
            return Err(
                OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::UnsupportedSchemaVersion {
                    expected: ORACLE_PRICER_SUFFICIENT_STATISTICS_BENCHMARK_POLICY_SCHEMA_VERSION,
                    actual: self.schema_version.clone(),
                },
            );
        }
        if self.policy_id.trim().is_empty() {
            return Err(
                OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::EmptyPolicyId,
            );
        }
        if self.policy_version.trim().is_empty() {
            return Err(
                OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::EmptyPolicyVersion,
            );
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
            OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::InvalidHawkesParameters { source }
        })?;
        GammaSeverityMarkConfiguration::try_new(model_parameters.gamma_shape_k)
        .map_err(|source| {
            OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::InvalidGammaMarkConfiguration { source }
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
            || seed_partition.oracle_component_seed_salt
                == seed_partition.sufficient_statistics_component_seed_salt
            || seed_partition.pricing_component_seed_salt
                == seed_partition.sufficient_statistics_component_seed_salt
        {
            return Err(OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::DuplicateSeedSaltValues);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerSufficientStatisticsBenchmarkPolicyValidationError {
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
    DuplicateSeedSaltValues,
}

impl Display for OraclePricerSufficientStatisticsBenchmarkPolicyValidationError {
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
            Self::DuplicateSeedSaltValues => write!(
                formatter,
                "benchmark policy seed_partition seed salts must be pairwise distinct"
            ),
        }
    }
}

impl Error for OraclePricerSufficientStatisticsBenchmarkPolicyValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidHawkesParameters { source } => Some(source),
            Self::InvalidGammaMarkConfiguration { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerSufficientStatisticsBenchmarkPolicyLoadError {
    InvalidJson {
        source: String,
    },
    InvalidPolicy {
        source: OraclePricerSufficientStatisticsBenchmarkPolicyValidationError,
    },
}

impl Display for OraclePricerSufficientStatisticsBenchmarkPolicyLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson { source } => write!(
                formatter,
                "oracle pricer sufficient-statistics benchmark policy payload must be valid JSON: {source}"
            ),
            Self::InvalidPolicy { source } => write!(
                formatter,
                "oracle pricer sufficient-statistics benchmark policy payload is invalid: {source}"
            ),
        }
    }
}

impl Error for OraclePricerSufficientStatisticsBenchmarkPolicyLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPolicy { source } => Some(source),
            _ => None,
        }
    }
}

pub fn load_oracle_pricer_sufficient_statistics_benchmark_policy_from_json(
    policy_payload: &str,
) -> Result<
    OraclePricerSufficientStatisticsBenchmarkPolicy,
    OraclePricerSufficientStatisticsBenchmarkPolicyLoadError,
> {
    let policy: OraclePricerSufficientStatisticsBenchmarkPolicy =
        serde_json::from_str(policy_payload).map_err(|source| {
            OraclePricerSufficientStatisticsBenchmarkPolicyLoadError::InvalidJson {
                source: source.to_string(),
            }
        })?;

    policy.validate().map_err(|source| {
        OraclePricerSufficientStatisticsBenchmarkPolicyLoadError::InvalidPolicy { source }
    })?;

    Ok(policy)
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
) -> Result<(), OraclePricerSufficientStatisticsBenchmarkPolicyValidationError> {
    if value.trim().is_empty() {
        return Err(
            OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::EmptyRationaleField {
                field_name,
            },
        );
    }

    Ok(())
}

fn validate_range(
    field_name: &'static str,
    min_value: f64,
    max_value: f64,
    policy: RangeValidationPolicy,
) -> Result<(), OraclePricerSufficientStatisticsBenchmarkPolicyValidationError> {
    if !min_value.is_finite() {
        return Err(
            OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::InvalidFiniteField {
                field_name,
                value: min_value,
            },
        );
    }
    if !max_value.is_finite() {
        return Err(
            OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::InvalidFiniteField {
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
                    OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::InvalidNonNegativeFiniteField {
                        field_name,
                        value: min_value,
                    },
                );
            }
            if max_value < 0.0 {
                return Err(
                    OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::InvalidNonNegativeFiniteField {
                        field_name,
                        value: max_value,
                    },
                );
            }
        }
        RangeValidationPolicy::StrictlyPositive => {
            if min_value <= 0.0 {
                return Err(
                    OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::InvalidPositiveFiniteField {
                        field_name,
                        value: min_value,
                    },
                );
            }
            if max_value <= 0.0 {
                return Err(
                    OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::InvalidPositiveFiniteField {
                        field_name,
                        value: max_value,
                    },
                );
            }
        }
    }

    if min_value > max_value {
        return Err(
            OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::InvalidRangeField {
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
) -> Result<(), OraclePricerSufficientStatisticsBenchmarkPolicyValidationError> {
    validate_positive_number(value, field_name).map_err(|source| {
        OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::InvalidPositiveFiniteField {
            field_name: source.field,
            value: source.value,
        }
    })
}

fn validate_non_negative_finite_policy_field(
    field_name: &'static str,
    value: f64,
) -> Result<(), OraclePricerSufficientStatisticsBenchmarkPolicyValidationError> {
    validate_non_negative_number(value, field_name).map_err(|source| {
        OraclePricerSufficientStatisticsBenchmarkPolicyValidationError::InvalidNonNegativeFiniteField {
            field_name: source.field,
            value: source.value,
        }
    })
}
