//! Stylized-fact metric catalog contract types and validation internals.

use std::collections::BTreeSet;

use janiform_foundations::strings::is_non_empty_string;
use serde::{Deserialize, Serialize};

use super::constants::{
    STYLIZED_FACT_DOMAIN_VOCABULARY, STYLIZED_FACT_ESTIMATOR_METADATA_REQUIRED_FIELDS,
    STYLIZED_FACT_METRIC_CATALOG_FORWARD_EXTENSION_POLICY,
    STYLIZED_FACT_METRIC_CATALOG_HORIZON_LABEL_POLICY,
    STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_DECISION_POLICY,
    STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE,
    STYLIZED_FACT_METRIC_CATALOG_NAMING_POLICY,
    STYLIZED_FACT_METRIC_CATALOG_OUTPUT_SCHEMA_METADATA_VERSION,
    STYLIZED_FACT_METRIC_CATALOG_REQUIRED_FIELDS, STYLIZED_FACT_METRIC_CATALOG_SCHEMA_FAMILY,
    STYLIZED_FACT_METRIC_CATALOG_SCHEMA_GENERATION, STYLIZED_FACT_METRIC_CATALOG_SCHEMA_VERSION,
    STYLIZED_FACT_METRIC_CATALOG_UNCERTAINTY_OVERRIDE_APPROVAL_POLICY,
    STYLIZED_FACT_METRIC_CATALOG_UNCERTAINTY_POLICY_ID, STYLIZED_FACT_METRIC_CLASS_VOCABULARY,
    STYLIZED_FACT_METRIC_FAMILY_VOCABULARY, STYLIZED_FACT_METRIC_REQUIRED_FIELDS,
    STYLIZED_FACT_PRIMARY_METRIC_REQUIRED_FIELDS, STYLIZED_FACT_SAMPLE_SUFFICIENCY_REQUIRED_FIELDS,
    STYLIZED_FACT_SECONDARY_METRIC_REQUIRED_FIELDS, STYLIZED_FACT_UNCERTAINTY_METHOD_VOCABULARY,
    STYLIZED_FACT_UNCERTAINTY_REQUIRED_FIELDS,
};
use super::errors::StylizedFactMetricCatalogValidationError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricCatalogSchemaMetadata {
    pub schema_family: String,
    pub schema_generation: String,
    pub forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricNamingConventions {
    pub naming_policy: String,
    pub metric_id_format: String,
    pub horizon_label_policy: String,
    pub canonical_primary_metric_class_label: String,
    pub canonical_secondary_metric_class_label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricCatalogUncertaintyPolicy {
    pub default_policy_id: String,
    pub dependent_return_series_default_method: StylizedFactUncertaintyMethod,
    pub default_confidence_level: f64,
    pub default_bootstrap_replication_count: usize,
    pub default_block_length_horizon_count: usize,
    pub override_approval_policy: String,
    pub approved_override_methods: Vec<StylizedFactUncertaintyMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactInsufficientDataSemantics {
    pub status_code: String,
    pub decision_policy: String,
    pub non_terminal_decision_state: bool,
    pub required_result_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricCatalogOutputSchemaMetadata {
    pub schema_version: String,
    pub required_catalog_fields: Vec<String>,
    pub required_metric_fields: Vec<String>,
    pub required_primary_metric_fields: Vec<String>,
    pub required_secondary_metric_fields: Vec<String>,
    pub required_uncertainty_fields: Vec<String>,
    pub required_sample_sufficiency_fields: Vec<String>,
    pub required_estimator_metadata_fields: Vec<String>,
    pub canonical_metric_class_vocabulary: Vec<String>,
    pub canonical_metric_family_vocabulary: Vec<String>,
    pub canonical_domain_vocabulary: Vec<String>,
    pub canonical_uncertainty_method_vocabulary: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricCatalog {
    pub schema_version: String,
    pub catalog_id: String,
    pub catalog_version: String,
    pub schema_metadata: StylizedFactMetricCatalogSchemaMetadata,
    pub naming_conventions: StylizedFactMetricNamingConventions,
    pub uncertainty_policy: StylizedFactMetricCatalogUncertaintyPolicy,
    pub insufficient_data_semantics: StylizedFactInsufficientDataSemantics,
    pub output_schema_metadata: StylizedFactMetricCatalogOutputSchemaMetadata,
    pub metrics: Vec<StylizedFactMetricDefinition>,
}

impl StylizedFactMetricCatalog {
    pub fn validate(&self) -> Result<(), StylizedFactMetricCatalogValidationError> {
        if self.schema_version != STYLIZED_FACT_METRIC_CATALOG_SCHEMA_VERSION {
            return Err(
                StylizedFactMetricCatalogValidationError::UnsupportedSchemaVersion {
                    expected: STYLIZED_FACT_METRIC_CATALOG_SCHEMA_VERSION,
                    actual: self.schema_version.clone(),
                },
            );
        }

        validate_non_empty_field("catalog_id", &self.catalog_id)?;
        validate_non_empty_field("catalog_version", &self.catalog_version)?;

        validate_metadata_field(
            "schema_metadata.schema_family",
            STYLIZED_FACT_METRIC_CATALOG_SCHEMA_FAMILY,
            &self.schema_metadata.schema_family,
        )?;
        validate_metadata_field(
            "schema_metadata.schema_generation",
            STYLIZED_FACT_METRIC_CATALOG_SCHEMA_GENERATION,
            &self.schema_metadata.schema_generation,
        )?;
        validate_metadata_field(
            "schema_metadata.forward_extension_policy",
            STYLIZED_FACT_METRIC_CATALOG_FORWARD_EXTENSION_POLICY,
            &self.schema_metadata.forward_extension_policy,
        )?;

        validate_metadata_field(
            "naming_conventions.naming_policy",
            STYLIZED_FACT_METRIC_CATALOG_NAMING_POLICY,
            &self.naming_conventions.naming_policy,
        )?;
        validate_metadata_field(
            "naming_conventions.metric_id_format",
            "snake_case",
            &self.naming_conventions.metric_id_format,
        )?;
        validate_metadata_field(
            "naming_conventions.horizon_label_policy",
            STYLIZED_FACT_METRIC_CATALOG_HORIZON_LABEL_POLICY,
            &self.naming_conventions.horizon_label_policy,
        )?;
        validate_metadata_field(
            "naming_conventions.canonical_primary_metric_class_label",
            "primary_robust",
            &self.naming_conventions.canonical_primary_metric_class_label,
        )?;
        validate_metadata_field(
            "naming_conventions.canonical_secondary_metric_class_label",
            "secondary_moment",
            &self
                .naming_conventions
                .canonical_secondary_metric_class_label,
        )?;

        validate_metadata_field(
            "uncertainty_policy.default_policy_id",
            STYLIZED_FACT_METRIC_CATALOG_UNCERTAINTY_POLICY_ID,
            &self.uncertainty_policy.default_policy_id,
        )?;
        validate_metadata_field(
            "uncertainty_policy.override_approval_policy",
            STYLIZED_FACT_METRIC_CATALOG_UNCERTAINTY_OVERRIDE_APPROVAL_POLICY,
            &self.uncertainty_policy.override_approval_policy,
        )?;

        if self
            .uncertainty_policy
            .dependent_return_series_default_method
            != StylizedFactUncertaintyMethod::BlockBootstrap
        {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidUncertaintyPolicyConfiguration {
                    field_name: "uncertainty_policy.dependent_return_series_default_method",
                    details: "dependent_return_series default method must be `block_bootstrap`"
                        .to_string(),
                },
            );
        }

        if !self.uncertainty_policy.default_confidence_level.is_finite()
            || self.uncertainty_policy.default_confidence_level <= 0.0
            || self.uncertainty_policy.default_confidence_level >= 1.0
        {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidUncertaintyPolicyConfiguration {
                    field_name: "uncertainty_policy.default_confidence_level",
                    details: format!(
                        "default_confidence_level must be finite and in (0, 1), found {}",
                        self.uncertainty_policy.default_confidence_level
                    ),
                },
            );
        }

        if self.uncertainty_policy.default_bootstrap_replication_count == 0 {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidUncertaintyPolicyConfiguration {
                    field_name: "uncertainty_policy.default_bootstrap_replication_count",
                    details: "default_bootstrap_replication_count must be greater than 0"
                        .to_string(),
                },
            );
        }

        if self.uncertainty_policy.default_block_length_horizon_count == 0 {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidUncertaintyPolicyConfiguration {
                    field_name: "uncertainty_policy.default_block_length_horizon_count",
                    details: "default_block_length_horizon_count must be greater than 0"
                        .to_string(),
                },
            );
        }

        if self.uncertainty_policy.approved_override_methods.is_empty() {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidUncertaintyPolicyConfiguration {
                    field_name: "uncertainty_policy.approved_override_methods",
                    details: "approved_override_methods must not be empty".to_string(),
                },
            );
        }

        validate_metadata_field(
            "insufficient_data_semantics.status_code",
            STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE,
            &self.insufficient_data_semantics.status_code,
        )?;
        validate_metadata_field(
            "insufficient_data_semantics.decision_policy",
            STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_DECISION_POLICY,
            &self.insufficient_data_semantics.decision_policy,
        )?;

        if !self.insufficient_data_semantics.non_terminal_decision_state {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidInsufficientDataSemantics {
                    field_name: "insufficient_data_semantics.non_terminal_decision_state",
                    details: "non_terminal_decision_state must be true".to_string(),
                },
            );
        }

        validate_required_field_list(
            "insufficient_data_semantics.required_result_fields",
            &self.insufficient_data_semantics.required_result_fields,
            &[
                "status",
                "sample_coverage",
                "required_minimums",
                "observed_counts",
            ],
        )?;
        validate_metadata_field(
            "output_schema_metadata.schema_version",
            STYLIZED_FACT_METRIC_CATALOG_OUTPUT_SCHEMA_METADATA_VERSION,
            &self.output_schema_metadata.schema_version,
        )?;
        validate_required_field_list(
            "output_schema_metadata.required_catalog_fields",
            &self.output_schema_metadata.required_catalog_fields,
            &STYLIZED_FACT_METRIC_CATALOG_REQUIRED_FIELDS,
        )?;
        validate_required_field_list(
            "output_schema_metadata.required_metric_fields",
            &self.output_schema_metadata.required_metric_fields,
            &STYLIZED_FACT_METRIC_REQUIRED_FIELDS,
        )?;
        validate_required_field_list(
            "output_schema_metadata.required_primary_metric_fields",
            &self.output_schema_metadata.required_primary_metric_fields,
            &STYLIZED_FACT_PRIMARY_METRIC_REQUIRED_FIELDS,
        )?;
        validate_required_field_list(
            "output_schema_metadata.required_secondary_metric_fields",
            &self.output_schema_metadata.required_secondary_metric_fields,
            &STYLIZED_FACT_SECONDARY_METRIC_REQUIRED_FIELDS,
        )?;
        validate_required_field_list(
            "output_schema_metadata.required_uncertainty_fields",
            &self.output_schema_metadata.required_uncertainty_fields,
            &STYLIZED_FACT_UNCERTAINTY_REQUIRED_FIELDS,
        )?;
        validate_required_field_list(
            "output_schema_metadata.required_sample_sufficiency_fields",
            &self
                .output_schema_metadata
                .required_sample_sufficiency_fields,
            &STYLIZED_FACT_SAMPLE_SUFFICIENCY_REQUIRED_FIELDS,
        )?;
        validate_required_field_list(
            "output_schema_metadata.required_estimator_metadata_fields",
            &self
                .output_schema_metadata
                .required_estimator_metadata_fields,
            &STYLIZED_FACT_ESTIMATOR_METADATA_REQUIRED_FIELDS,
        )?;
        validate_required_field_list(
            "output_schema_metadata.canonical_metric_class_vocabulary",
            &self
                .output_schema_metadata
                .canonical_metric_class_vocabulary,
            &STYLIZED_FACT_METRIC_CLASS_VOCABULARY,
        )?;
        validate_required_field_list(
            "output_schema_metadata.canonical_metric_family_vocabulary",
            &self
                .output_schema_metadata
                .canonical_metric_family_vocabulary,
            &STYLIZED_FACT_METRIC_FAMILY_VOCABULARY,
        )?;
        validate_required_field_list(
            "output_schema_metadata.canonical_domain_vocabulary",
            &self.output_schema_metadata.canonical_domain_vocabulary,
            &STYLIZED_FACT_DOMAIN_VOCABULARY,
        )?;
        validate_required_field_list(
            "output_schema_metadata.canonical_uncertainty_method_vocabulary",
            &self
                .output_schema_metadata
                .canonical_uncertainty_method_vocabulary,
            &STYLIZED_FACT_UNCERTAINTY_METHOD_VOCABULARY,
        )?;

        if self.metrics.is_empty() {
            return Err(StylizedFactMetricCatalogValidationError::EmptyMetricList);
        }

        let mut metric_ids: BTreeSet<String> = BTreeSet::new();
        let mut metric_names: BTreeSet<String> = BTreeSet::new();

        for metric in &self.metrics {
            if !metric_ids.insert(metric.metric_id.clone()) {
                return Err(
                    StylizedFactMetricCatalogValidationError::DuplicateMetricId {
                        metric_id: metric.metric_id.clone(),
                    },
                );
            }

            if !metric_names.insert(metric.metric_name.clone()) {
                return Err(
                    StylizedFactMetricCatalogValidationError::DuplicateMetricName {
                        metric_name: metric.metric_name.clone(),
                    },
                );
            }

            metric.validate(&self.uncertainty_policy)?;
        }

        Ok(())
    }

    pub(crate) fn sort_metrics_canonically(&mut self) {
        self.metrics
            .sort_by(|left, right| left.metric_id.cmp(&right.metric_id));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactMetricClass {
    PrimaryRobust,
    SecondaryMoment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactMetricFamily {
    QuantileShape,
    TailRisk,
    DistributionDistance,
    LMoment,
    CentralMoment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactMetricDomain {
    ActivityClustering,
    HeavyTails,
    Skew,
    VolatilityPersistence,
    OpenWindowDirectJumpBehavior,
    ClosedWindowGapBehavior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactMetricHorizonApplicability {
    AllConfiguredHorizons,
    HorizonAndLagGrid,
    OpenWindowOnly,
    ClosedWindowOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactSampleDependence {
    DependentReturnSeries,
    DependentEventSeries,
    ApproximatelyIndependentSamples,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactUncertaintyMethod {
    BlockBootstrap,
    StationaryBootstrap,
    CircularBlockBootstrap,
    AnalyticalAsymptotic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactUncertaintyOverridePolicy {
    pub override_allowed: bool,
    pub approved_alternative_methods: Vec<StylizedFactUncertaintyMethod>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricUncertaintyConfiguration {
    pub default_method: StylizedFactUncertaintyMethod,
    pub confidence_level: f64,
    pub bootstrap_replication_count: usize,
    pub block_length_horizon_count: usize,
    pub override_policy: StylizedFactUncertaintyOverridePolicy,
}

impl StylizedFactMetricUncertaintyConfiguration {
    fn validate(
        &self,
        metric_id: &str,
        sample_dependence: StylizedFactSampleDependence,
        catalog_policy: &StylizedFactMetricCatalogUncertaintyPolicy,
    ) -> Result<(), StylizedFactMetricCatalogValidationError> {
        if !self.confidence_level.is_finite()
            || self.confidence_level <= 0.0
            || self.confidence_level >= 1.0
        {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidMetricUncertaintyConfiguration {
                    metric_id: metric_id.to_string(),
                    field_name: "uncertainty.confidence_level",
                    details: format!(
                        "confidence_level must be finite and in (0, 1), found {}",
                        self.confidence_level
                    ),
                },
            );
        }

        if self.bootstrap_replication_count == 0 {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidMetricUncertaintyConfiguration {
                    metric_id: metric_id.to_string(),
                    field_name: "uncertainty.bootstrap_replication_count",
                    details: "bootstrap_replication_count must be greater than 0".to_string(),
                },
            );
        }

        if self.block_length_horizon_count == 0 {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidMetricUncertaintyConfiguration {
                    metric_id: metric_id.to_string(),
                    field_name: "uncertainty.block_length_horizon_count",
                    details: "block_length_horizon_count must be greater than 0".to_string(),
                },
            );
        }

        if self.override_policy.override_allowed {
            if self.override_policy.approved_alternative_methods.is_empty() {
                return Err(
                    StylizedFactMetricCatalogValidationError::InvalidMetricUncertaintyConfiguration {
                        metric_id: metric_id.to_string(),
                        field_name: "uncertainty.override_policy.approved_alternative_methods",
                        details: "approved_alternative_methods must not be empty when override_allowed is true".to_string(),
                    },
                );
            }
            for method in &self.override_policy.approved_alternative_methods {
                if !catalog_policy.approved_override_methods.contains(method) {
                    return Err(StylizedFactMetricCatalogValidationError::InvalidMetricUncertaintyConfiguration {
                        metric_id: metric_id.to_string(),
                        field_name: "uncertainty.override_policy.approved_alternative_methods",
                        details: format!(
                            "method `{method:?}` is not approved by catalog uncertainty policy"
                        ),
                    });
                }
            }
        } else if !self.override_policy.approved_alternative_methods.is_empty() {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidMetricUncertaintyConfiguration {
                    metric_id: metric_id.to_string(),
                    field_name: "uncertainty.override_policy.approved_alternative_methods",
                    details:
                        "approved_alternative_methods must be empty when override_allowed is false"
                            .to_string(),
                },
            );
        }

        if sample_dependence == StylizedFactSampleDependence::DependentReturnSeries
            && self.default_method != StylizedFactUncertaintyMethod::BlockBootstrap
        {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidMetricUncertaintyConfiguration {
                    metric_id: metric_id.to_string(),
                    field_name: "uncertainty.default_method",
                    details: "dependent_return_series metrics must default to `block_bootstrap`"
                        .to_string(),
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMinimumSampleSufficiencyRequirements {
    pub minimum_sample_count: usize,
    pub minimum_effective_sample_count: usize,
    pub minimum_tail_observation_count: Option<usize>,
    pub minimum_horizon_coverage_ratio: f64,
}

impl StylizedFactMinimumSampleSufficiencyRequirements {
    fn validate(
        &self,
        metric_id: &str,
        metric_family: StylizedFactMetricFamily,
    ) -> Result<(), StylizedFactMetricCatalogValidationError> {
        if self.minimum_sample_count == 0 {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidMetricSampleSufficiency {
                    metric_id: metric_id.to_string(),
                    field_name: "sample_sufficiency.minimum_sample_count",
                    details: "minimum_sample_count must be greater than 0".to_string(),
                },
            );
        }

        if self.minimum_effective_sample_count == 0
            || self.minimum_effective_sample_count > self.minimum_sample_count
        {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidMetricSampleSufficiency {
                    metric_id: metric_id.to_string(),
                    field_name: "sample_sufficiency.minimum_effective_sample_count",
                    details: format!(
                        "minimum_effective_sample_count must be in [1, minimum_sample_count], found {}",
                        self.minimum_effective_sample_count
                    ),
                },
            );
        }

        match self.minimum_tail_observation_count {
            Some(minimum_tail_observation_count) => {
                if minimum_tail_observation_count == 0
                    || minimum_tail_observation_count > self.minimum_effective_sample_count
                {
                    return Err(
                        StylizedFactMetricCatalogValidationError::InvalidMetricSampleSufficiency {
                            metric_id: metric_id.to_string(),
                            field_name: "sample_sufficiency.minimum_tail_observation_count",
                            details: format!(
                                "minimum_tail_observation_count must be in [1, minimum_effective_sample_count], found {minimum_tail_observation_count}",
                            ),
                        },
                    );
                }
            }

            None => {
                if metric_family == StylizedFactMetricFamily::TailRisk {
                    return Err(
                        StylizedFactMetricCatalogValidationError::InvalidMetricSampleSufficiency {
                            metric_id: metric_id.to_string(),
                            field_name: "sample_sufficiency.minimum_tail_observation_count",
                            details: "tail_sufficient-statistics metrics require minimum_tail_observation_count"
                                .to_string(),
                        },
                    );
                }
            }
        }

        if !self.minimum_horizon_coverage_ratio.is_finite()
            || self.minimum_horizon_coverage_ratio <= 0.0
            || self.minimum_horizon_coverage_ratio > 1.0
        {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidMetricSampleSufficiency {
                    metric_id: metric_id.to_string(),
                    field_name: "sample_sufficiency.minimum_horizon_coverage_ratio",
                    details: format!(
                        "minimum_horizon_coverage_ratio must be finite and in (0, 1], found {}",
                        self.minimum_horizon_coverage_ratio
                    ),
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactEstimatorMetadata {
    pub estimator_id: String,
    pub estimator_version: String,
    pub estimator_family: String,
    pub bias_correction: String,
    pub finite_sample_adjustment: String,
}

impl StylizedFactEstimatorMetadata {
    fn validate(&self, metric_id: &str) -> Result<(), StylizedFactMetricCatalogValidationError> {
        validate_non_empty_metric_field(
            metric_id,
            "estimator_metadata.estimator_id",
            &self.estimator_id,
        )?;
        validate_non_empty_metric_field(
            metric_id,
            "estimator_metadata.estimator_version",
            &self.estimator_version,
        )?;
        validate_non_empty_metric_field(
            metric_id,
            "estimator_metadata.estimator_family",
            &self.estimator_family,
        )?;
        validate_non_empty_metric_field(
            metric_id,
            "estimator_metadata.bias_correction",
            &self.bias_correction,
        )?;
        validate_non_empty_metric_field(
            metric_id,
            "estimator_metadata.finite_sample_adjustment",
            &self.finite_sample_adjustment,
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "variant", rename_all = "snake_case")]
pub enum StylizedFactSecondaryMomentVariant {
    LMoment {
        moment_order: u8,
        ratio_denominator_order: Option<u8>,
    },
    CentralMoment {
        moment_order: u8,
        standardized: bool,
        excess_kurtosis: bool,
    },
}

impl StylizedFactSecondaryMomentVariant {
    fn validate(
        self,
        metric_id: &str,
        metric_family: StylizedFactMetricFamily,
    ) -> Result<(), StylizedFactMetricCatalogValidationError> {
        match self {
            Self::LMoment {
                moment_order,
                ratio_denominator_order,
            } => {
                if metric_family != StylizedFactMetricFamily::LMoment {
                    return Err(
                        StylizedFactMetricCatalogValidationError::InvalidSecondaryMomentVariant {
                            metric_id: metric_id.to_string(),
                            details: "l_moment variant requires metric_family `l_moment`"
                                .to_string(),
                        },
                    );
                }

                if moment_order == 0 || moment_order > 4 {
                    return Err(
                        StylizedFactMetricCatalogValidationError::InvalidSecondaryMomentVariant {
                            metric_id: metric_id.to_string(),
                            details: format!(
                                "l_moment order must be in [1, 4], found {moment_order}"
                            ),
                        },
                    );
                }

                if let Some(ratio_denominator_order) = ratio_denominator_order
                    && (ratio_denominator_order == 0 || ratio_denominator_order >= moment_order)
                {
                    return Err(
                        StylizedFactMetricCatalogValidationError::InvalidSecondaryMomentVariant {
                            metric_id: metric_id.to_string(),
                            details: format!(
                                "ratio_denominator_order must be in [1, moment_order - 1], found {ratio_denominator_order}"
                            ),
                        },
                    );
                }
            }
            Self::CentralMoment {
                moment_order,
                standardized: _,
                excess_kurtosis,
            } => {
                if metric_family != StylizedFactMetricFamily::CentralMoment {
                    return Err(
                        StylizedFactMetricCatalogValidationError::InvalidSecondaryMomentVariant {
                            metric_id: metric_id.to_string(),
                            details:
                                "central_moment variant requires metric_family `central_moment`"
                                    .to_string(),
                        },
                    );
                }

                if !(2..=4).contains(&moment_order) {
                    return Err(
                        StylizedFactMetricCatalogValidationError::InvalidSecondaryMomentVariant {
                            metric_id: metric_id.to_string(),
                            details: format!(
                                "central moment order must be in [2, 4], found {moment_order}"
                            ),
                        },
                    );
                }

                if excess_kurtosis && moment_order != 4 {
                    return Err(
                        StylizedFactMetricCatalogValidationError::InvalidSecondaryMomentVariant {
                            metric_id: metric_id.to_string(),
                            details:
                                "excess_kurtosis can only be true when central moment order is 4"
                                    .to_string(),
                        },
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricDefinition {
    pub metric_id: String,
    pub metric_name: String,
    pub metric_class: StylizedFactMetricClass,
    pub metric_family: StylizedFactMetricFamily,
    pub domain: StylizedFactMetricDomain,
    pub definition: String,
    pub unit: String,
    pub horizon_applicability: StylizedFactMetricHorizonApplicability,
    pub sample_dependence: StylizedFactSampleDependence,
    pub uncertainty: StylizedFactMetricUncertaintyConfiguration,
    pub sample_sufficiency: StylizedFactMinimumSampleSufficiencyRequirements,
    pub estimator_metadata: Option<StylizedFactEstimatorMetadata>,
    pub secondary_moment_variant: Option<StylizedFactSecondaryMomentVariant>,
    pub tags: Vec<String>,
}

impl StylizedFactMetricDefinition {
    fn validate(
        &self,
        catalog_uncertainty_policy: &StylizedFactMetricCatalogUncertaintyPolicy,
    ) -> Result<(), StylizedFactMetricCatalogValidationError> {
        validate_non_empty_metric_field(&self.metric_id, "metric_name", &self.metric_name)?;
        validate_non_empty_metric_field(&self.metric_id, "definition", &self.definition)?;
        validate_non_empty_metric_field(&self.metric_id, "unit", &self.unit)?;

        if self.tags.is_empty() {
            return Err(
                StylizedFactMetricCatalogValidationError::InvalidMetricDefinition {
                    metric_id: self.metric_id.clone(),
                    field_name: "tags",
                    details: "tags must contain at least one value".to_string(),
                },
            );
        }
        for tag in &self.tags {
            if tag.trim().is_empty() {
                return Err(
                    StylizedFactMetricCatalogValidationError::InvalidMetricDefinition {
                        metric_id: self.metric_id.clone(),
                        field_name: "tags",
                        details: "tags must not contain empty values".to_string(),
                    },
                );
            }
        }

        validate_metric_id_format(&self.metric_id)?;

        self.uncertainty.validate(
            &self.metric_id,
            self.sample_dependence,
            catalog_uncertainty_policy,
        )?;

        self.sample_sufficiency
            .validate(&self.metric_id, self.metric_family)?;

        match self.metric_class {
            StylizedFactMetricClass::PrimaryRobust => {
                if !matches!(
                    self.metric_family,
                    StylizedFactMetricFamily::QuantileShape
                        | StylizedFactMetricFamily::TailRisk
                        | StylizedFactMetricFamily::DistributionDistance
                ) {
                    return Err(
                        StylizedFactMetricCatalogValidationError::InvalidMetricClassFamilyCombination {
                            metric_id: self.metric_id.clone(),
                            metric_class: self.metric_class,
                            metric_family: self.metric_family,
                        },
                    );
                }

                if self.estimator_metadata.is_some() {
                    return Err(
                        StylizedFactMetricCatalogValidationError::UnexpectedEstimatorMetadata {
                            metric_id: self.metric_id.clone(),
                        },
                    );
                }

                if self.secondary_moment_variant.is_some() {
                    return Err(
                        StylizedFactMetricCatalogValidationError::UnexpectedSecondaryMomentVariant {
                            metric_id: self.metric_id.clone(),
                        },
                    );
                }
            }
            StylizedFactMetricClass::SecondaryMoment => {
                if !matches!(
                    self.metric_family,
                    StylizedFactMetricFamily::LMoment | StylizedFactMetricFamily::CentralMoment
                ) {
                    return Err(
                        StylizedFactMetricCatalogValidationError::InvalidMetricClassFamilyCombination {
                            metric_id: self.metric_id.clone(),
                            metric_class: self.metric_class,
                            metric_family: self.metric_family,
                        },
                    );
                }

                let estimator_metadata = self.estimator_metadata.as_ref().ok_or_else(|| {
                    StylizedFactMetricCatalogValidationError::MissingEstimatorMetadata {
                        metric_id: self.metric_id.clone(),
                    }
                })?;
                estimator_metadata.validate(&self.metric_id)?;

                let secondary_moment_variant = self.secondary_moment_variant.ok_or_else(|| {
                    StylizedFactMetricCatalogValidationError::MissingSecondaryMomentVariant {
                        metric_id: self.metric_id.clone(),
                    }
                })?;
                secondary_moment_variant.validate(&self.metric_id, self.metric_family)?;
            }
        }

        Ok(())
    }
}

fn validate_metadata_field(
    field_name: &'static str,
    expected: &'static str,
    actual: &str,
) -> Result<(), StylizedFactMetricCatalogValidationError> {
    if actual == expected {
        return Ok(());
    }

    Err(
        StylizedFactMetricCatalogValidationError::InvalidMetadataField {
            field_name,
            expected,
            actual: actual.to_string(),
        },
    )
}

fn validate_non_empty_field(
    field_name: &'static str,
    value: &str,
) -> Result<(), StylizedFactMetricCatalogValidationError> {
    if is_non_empty_string(value) {
        return Ok(());
    }

    Err(StylizedFactMetricCatalogValidationError::EmptyField { field_name })
}

fn validate_required_field_list(
    field_name: &'static str,
    actual: &[String],
    expected: &[&str],
) -> Result<(), StylizedFactMetricCatalogValidationError> {
    let expected_as_strings = required_field_list_to_owned(expected);
    if actual == expected_as_strings.as_slice() {
        return Ok(());
    }

    Err(
        StylizedFactMetricCatalogValidationError::InvalidRequiredFieldList {
            field_name,
            expected: expected_as_strings,
            actual: actual.to_vec(),
        },
    )
}

pub(crate) fn required_field_list_to_owned(field_names: &[&str]) -> Vec<String> {
    field_names
        .iter()
        .map(|field_name| (*field_name).to_string())
        .collect()
}

fn validate_non_empty_metric_field(
    metric_id: &str,
    field_name: &'static str,
    value: &str,
) -> Result<(), StylizedFactMetricCatalogValidationError> {
    if is_non_empty_string(value) {
        return Ok(());
    }

    Err(
        StylizedFactMetricCatalogValidationError::InvalidMetricDefinition {
            metric_id: metric_id.to_string(),
            field_name,
            details: "value must be a non-empty string".to_string(),
        },
    )
}

fn validate_metric_id_format(
    metric_id: &str,
) -> Result<(), StylizedFactMetricCatalogValidationError> {
    if metric_id.trim().is_empty() {
        return Err(
            StylizedFactMetricCatalogValidationError::InvalidMetricIdFormat {
                metric_id: metric_id.to_string(),
            },
        );
    }
    if metric_id.starts_with('_') || metric_id.ends_with('_') {
        return Err(
            StylizedFactMetricCatalogValidationError::InvalidMetricIdFormat {
                metric_id: metric_id.to_string(),
            },
        );
    }

    let mut previous_was_underscore = false;
    for character in metric_id.chars() {
        match character {
            'a'..='z' | '0'..='9' => previous_was_underscore = false,
            '_' => {
                if previous_was_underscore {
                    return Err(
                        StylizedFactMetricCatalogValidationError::InvalidMetricIdFormat {
                            metric_id: metric_id.to_string(),
                        },
                    );
                }
                previous_was_underscore = true;
            }
            _ => {
                return Err(
                    StylizedFactMetricCatalogValidationError::InvalidMetricIdFormat {
                        metric_id: metric_id.to_string(),
                    },
                );
            }
        }
    }

    Ok(())
}
