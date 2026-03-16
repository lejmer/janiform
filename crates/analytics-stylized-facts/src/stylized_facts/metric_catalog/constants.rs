//! Stylized-fact metric catalog schema constants.

pub const STYLIZED_FACT_METRIC_CATALOG_SCHEMA_VERSION: &str =
    "validation.stylized_fact.metric_catalog.v1";
pub const STYLIZED_FACT_METRIC_CATALOG_SCHEMA_FAMILY: &str =
    "validation.stylized_fact.metric_catalog";
pub const STYLIZED_FACT_METRIC_CATALOG_SCHEMA_GENERATION: &str = "v1";
pub const STYLIZED_FACT_METRIC_CATALOG_FORWARD_EXTENSION_POLICY: &str = "additive_fields_only.v1";
pub const STYLIZED_FACT_METRIC_CATALOG_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "validation.stylized_fact.metric_catalog.output_schema_metadata.v1";
pub const STYLIZED_FACT_METRIC_CATALOG_NAMING_POLICY: &str =
    "snake_case_metric_ids_used_as_canonical_output_keys.v1";
pub const STYLIZED_FACT_METRIC_CATALOG_HORIZON_LABEL_POLICY: &str =
    "explicit_horizon_bucket_labels_with_no_implicit_interpolation.v1";
pub const STYLIZED_FACT_METRIC_CATALOG_UNCERTAINTY_POLICY_ID: &str =
    "dependent_return_series_default_block_bootstrap_with_approved_overrides.v1";
pub const STYLIZED_FACT_METRIC_CATALOG_UNCERTAINTY_OVERRIDE_APPROVAL_POLICY: &str =
    "allow_only_explicitly_whitelisted_uncertainty_methods.v1";
pub const STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE: &str = "insufficient_data";
pub const STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_DECISION_POLICY: &str =
    "emit_insufficient_data_with_coverage_and_skip_threshold_decision.v1";

pub(crate) const STYLIZED_FACT_METRIC_CATALOG_REQUIRED_FIELDS: [&str; 9] = [
    "schema_version",
    "catalog_id",
    "catalog_version",
    "schema_metadata",
    "naming_conventions",
    "uncertainty_policy",
    "insufficient_data_semantics",
    "output_schema_metadata",
    "metrics",
];
pub(crate) const STYLIZED_FACT_METRIC_REQUIRED_FIELDS: [&str; 12] = [
    "metric_id",
    "metric_name",
    "metric_class",
    "metric_family",
    "domain",
    "definition",
    "unit",
    "horizon_applicability",
    "sample_dependence",
    "uncertainty",
    "sample_sufficiency",
    "tags",
];
pub(crate) const STYLIZED_FACT_PRIMARY_METRIC_REQUIRED_FIELDS: [&str; 12] = [
    "metric_id",
    "metric_name",
    "metric_class",
    "metric_family",
    "domain",
    "definition",
    "unit",
    "horizon_applicability",
    "sample_dependence",
    "uncertainty",
    "sample_sufficiency",
    "tags",
];
pub(crate) const STYLIZED_FACT_SECONDARY_METRIC_REQUIRED_FIELDS: [&str; 14] = [
    "metric_id",
    "metric_name",
    "metric_class",
    "metric_family",
    "domain",
    "definition",
    "unit",
    "horizon_applicability",
    "sample_dependence",
    "uncertainty",
    "sample_sufficiency",
    "estimator_metadata",
    "secondary_moment_variant",
    "tags",
];
pub(crate) const STYLIZED_FACT_UNCERTAINTY_REQUIRED_FIELDS: [&str; 5] = [
    "default_method",
    "confidence_level",
    "bootstrap_replication_count",
    "block_length_horizon_count",
    "override_policy",
];
pub(crate) const STYLIZED_FACT_SAMPLE_SUFFICIENCY_REQUIRED_FIELDS: [&str; 4] = [
    "minimum_sample_count",
    "minimum_effective_sample_count",
    "minimum_tail_observation_count",
    "minimum_horizon_coverage_ratio",
];
pub(crate) const STYLIZED_FACT_ESTIMATOR_METADATA_REQUIRED_FIELDS: [&str; 5] = [
    "estimator_id",
    "estimator_version",
    "estimator_family",
    "bias_correction",
    "finite_sample_adjustment",
];

pub(crate) const STYLIZED_FACT_METRIC_CLASS_VOCABULARY: [&str; 2] =
    ["primary_robust", "secondary_moment"];
pub(crate) const STYLIZED_FACT_METRIC_FAMILY_VOCABULARY: [&str; 5] = [
    "quantile_shape",
    "tail_risk",
    "distribution_distance",
    "l_moment",
    "central_moment",
];
pub(crate) const STYLIZED_FACT_DOMAIN_VOCABULARY: [&str; 6] = [
    "activity_clustering",
    "heavy_tails",
    "skew",
    "volatility_persistence",
    "open_window_direct_jump_behavior",
    "closed_window_gap_behavior",
];
pub(crate) const STYLIZED_FACT_UNCERTAINTY_METHOD_VOCABULARY: [&str; 4] = [
    "block_bootstrap",
    "stationary_bootstrap",
    "circular_block_bootstrap",
    "analytical_asymptotic",
];
