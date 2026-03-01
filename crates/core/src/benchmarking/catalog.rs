use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::hawkes::HawkesExcitationState;

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
    pub(crate) const fn ordering_rank(self) -> u8 {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
        }
    }

    pub(crate) fn required_order() -> [Self; 3] {
        [Self::Small, Self::Medium, Self::Large]
    }
}

/// Shared key contract for canonical profile ordering.
pub(crate) trait CanonicalProfileRecord {
    fn profile_id(&self) -> &str;
    fn workload_scale_rank(&self) -> u8;
}

/// Sort profile records by canonical benchmark policy:
/// workload scale rank first, then profile id ascending.
pub(crate) fn sort_profile_records_canonically<T>(profile_records: &mut [T])
where
    T: CanonicalProfileRecord,
{
    profile_records.sort_by(|left, right| {
        left.workload_scale_rank()
            .cmp(&right.workload_scale_rank())
            .then_with(|| left.profile_id().cmp(right.profile_id()))
    });
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

impl CanonicalProfileRecord for BenchmarkScenarioWorkloadProfile {
    fn profile_id(&self) -> &str {
        self.profile_id.as_str()
    }

    fn workload_scale_rank(&self) -> u8 {
        self.workload_scale.ordering_rank()
    }
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
    pub(crate) fn validate(&self) -> Result<(), BenchmarkScenarioCatalogLoadError> {
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

    pub(crate) fn sort_profiles_canonically(&mut self) {
        sort_profile_records_canonically(self.profiles.as_mut_slice());
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
