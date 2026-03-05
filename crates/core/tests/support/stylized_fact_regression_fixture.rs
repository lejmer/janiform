#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::support::validation_fixture::{
    LoadedFixturePayload, ValidationFixtureManifestDocument, ValidationFixtureManifestEntry,
    fixtures_root_path, load_manifest_entry_payload, load_validation_fixture_manifest_document,
};

pub const STYLIZED_FACT_METRIC_REGRESSION_FIXTURE_SCHEMA_VERSION: &str =
    "validation.stylized_fact.metric_regression.fixture.v1";
pub const STYLIZED_FACT_METRIC_REGRESSION_FIXTURE_SET_ID: &str =
    "stylized_fact_metric_regression_fixture_v1";
pub const STYLIZED_FACT_METRIC_REGRESSION_SCHEMA_FAMILY: &str =
    "validation.stylized_fact.metric_regression.fixture";
pub const STYLIZED_FACT_METRIC_REGRESSION_SCHEMA_GENERATION: &str = "v1";
pub const STYLIZED_FACT_METRIC_REGRESSION_FORWARD_EXTENSION_POLICY: &str =
    "additive_fields_only.v1";
pub const STYLIZED_FACT_METRIC_REGRESSION_CASE_ORDER_POLICY: &str =
    "preserve_declared_case_order.v1";
pub const STYLIZED_FACT_METRIC_REGRESSION_CONTENT_HASH_SCOPE_POLICY: &str =
    "document_payload_bytes.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricRegressionFixtureDocument {
    pub schema_version: String,
    pub fixture_set_id: String,
    pub fixture_set_version: String,
    pub schema_metadata: StylizedFactMetricRegressionSchemaMetadata,
    pub determinism_metadata: StylizedFactMetricRegressionDeterminismMetadata,
    pub tolerance_policies: Vec<StylizedFactRegressionTolerancePolicy>,
    pub canonical_regression_metadata: StylizedFactMetricRegressionCanonicalMetadata,
    pub cases: Vec<StylizedFactMetricRegressionCase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricRegressionSchemaMetadata {
    pub schema_family: String,
    pub schema_generation: String,
    pub forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricRegressionDeterminismMetadata {
    pub case_order_policy: String,
    pub content_hash_scope: String,
    pub content_hash_algorithm: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactDiagnosticKind {
    ActivityClustering,
    TailSkew,
    VolatilityPersistence,
    GapBehavior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactRegressionTolerancePolicyKind {
    DeterministicExactMatch,
    BootstrapConfidenceInterval,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactRegressionTolerancePolicy {
    pub policy_id: String,
    pub policy_kind: StylizedFactRegressionTolerancePolicyKind,
    pub policy_version: String,
    pub point_estimate_absolute_tolerance: f64,
    pub absolute_delta_tolerance: f64,
    pub confidence_interval_bound_absolute_tolerance: f64,
    pub require_confidence_intervals: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricRegressionCanonicalMetadata {
    pub required_tags: Vec<String>,
    pub required_diagnostic_kinds: Vec<StylizedFactDiagnosticKind>,
    pub required_tolerance_policy_ids: Vec<String>,
    pub stable_fixture_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricRegressionProvenance {
    pub baseline_source: String,
    pub captured_at_utc: String,
    pub update_guidance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricRegressionCase {
    pub fixture_id: String,
    pub diagnostic_kind: StylizedFactDiagnosticKind,
    pub tolerance_policy_id: String,
    pub provenance: StylizedFactMetricRegressionProvenance,
    pub request_payload: Value,
    pub expected_metrics: Vec<StylizedFactExpectedMetricRegressionEntry>,
    pub tags: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactExpectedMetricStatus {
    Available,
    InsufficientData,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactExpectedConfidenceInterval {
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub confidence_level: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactExpectedMetricRegressionEntry {
    pub horizon_label: String,
    pub metric_id: String,
    pub metric_variant: Option<String>,
    pub expected_status: StylizedFactExpectedMetricStatus,
    pub expected_empirical_value: Option<f64>,
    pub expected_simulated_value: Option<f64>,
    pub expected_absolute_delta: Option<f64>,
    pub expected_empirical_confidence_interval: Option<StylizedFactExpectedConfidenceInterval>,
    pub expected_simulated_confidence_interval: Option<StylizedFactExpectedConfidenceInterval>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedStylizedFactMetricRegressionFixture {
    pub manifest: ValidationFixtureManifestDocument,
    pub manifest_entry: ValidationFixtureManifestEntry,
    pub loaded_payload: LoadedFixturePayload,
    pub fixture_document: StylizedFactMetricRegressionFixtureDocument,
}

pub fn load_stylized_fact_metric_regression_fixture_from_manifest()
-> Result<LoadedStylizedFactMetricRegressionFixture, String> {
    let manifest = load_validation_fixture_manifest_document()?;
    let manifest_entry =
        find_manifest_entry_by_fixture_id(&manifest, STYLIZED_FACT_METRIC_REGRESSION_FIXTURE_SET_ID)?;

    if manifest_entry.fixture_schema_version != STYLIZED_FACT_METRIC_REGRESSION_FIXTURE_SCHEMA_VERSION
    {
        return Err(format!(
            "unexpected stylized fact regression fixture schema version in manifest: {}",
            manifest_entry.fixture_schema_version
        ));
    }

    let loaded_payload = load_manifest_entry_payload(&fixtures_root_path(), &manifest_entry)?;
    let fixture_document = parse_stylized_fact_metric_regression_fixture_document(
        &loaded_payload.payload_bytes,
    )?;
    validate_stylized_fact_metric_regression_fixture_document(&fixture_document)?;

    Ok(LoadedStylizedFactMetricRegressionFixture {
        manifest,
        manifest_entry,
        loaded_payload,
        fixture_document,
    })
}

pub fn parse_stylized_fact_metric_regression_fixture_document(
    payload_bytes: &[u8],
) -> Result<StylizedFactMetricRegressionFixtureDocument, String> {
    serde_json::from_slice::<StylizedFactMetricRegressionFixtureDocument>(payload_bytes).map_err(
        |source| format!("failed to parse stylized fact metric regression fixture document: {source}"),
    )
}

pub fn validate_stylized_fact_metric_regression_fixture_document(
    fixture_document: &StylizedFactMetricRegressionFixtureDocument,
) -> Result<(), String> {
    if fixture_document.schema_version != STYLIZED_FACT_METRIC_REGRESSION_FIXTURE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported stylized fact regression fixture schema version: {}",
            fixture_document.schema_version
        ));
    }

    if fixture_document.fixture_set_id != STYLIZED_FACT_METRIC_REGRESSION_FIXTURE_SET_ID {
        return Err(format!(
            "unexpected stylized fact regression fixture set id: {}",
            fixture_document.fixture_set_id
        ));
    }

    require_non_empty(
        fixture_document.fixture_set_version.as_str(),
        "fixture_set_version",
    )?;

    if fixture_document.schema_metadata.schema_family != STYLIZED_FACT_METRIC_REGRESSION_SCHEMA_FAMILY
    {
        return Err(format!(
            "unexpected stylized fact regression schema family: {}",
            fixture_document.schema_metadata.schema_family
        ));
    }

    if fixture_document.schema_metadata.schema_generation
        != STYLIZED_FACT_METRIC_REGRESSION_SCHEMA_GENERATION
    {
        return Err(format!(
            "unexpected stylized fact regression schema generation: {}",
            fixture_document.schema_metadata.schema_generation
        ));
    }

    if fixture_document.schema_metadata.forward_extension_policy
        != STYLIZED_FACT_METRIC_REGRESSION_FORWARD_EXTENSION_POLICY
    {
        return Err(format!(
            "unexpected stylized fact regression forward extension policy: {}",
            fixture_document.schema_metadata.forward_extension_policy
        ));
    }

    if fixture_document.determinism_metadata.case_order_policy
        != STYLIZED_FACT_METRIC_REGRESSION_CASE_ORDER_POLICY
    {
        return Err(format!(
            "unexpected stylized fact regression case order policy: {}",
            fixture_document.determinism_metadata.case_order_policy
        ));
    }

    if fixture_document.determinism_metadata.content_hash_scope
        != STYLIZED_FACT_METRIC_REGRESSION_CONTENT_HASH_SCOPE_POLICY
    {
        return Err(format!(
            "unexpected stylized fact regression content hash scope policy: {}",
            fixture_document.determinism_metadata.content_hash_scope
        ));
    }

    if fixture_document.determinism_metadata.content_hash_algorithm
        != crate::support::validation_fixture::CONTENT_HASH_ALGORITHM_POLICY
    {
        return Err(format!(
            "unexpected stylized fact regression content hash algorithm policy: {}",
            fixture_document.determinism_metadata.content_hash_algorithm
        ));
    }

    if fixture_document.tolerance_policies.is_empty() {
        return Err("stylized fact regression fixture must include tolerance_policies".to_string());
    }

    let mut policies_by_id: BTreeMap<&str, &StylizedFactRegressionTolerancePolicy> = BTreeMap::new();
    for policy in &fixture_document.tolerance_policies {
        require_non_empty(policy.policy_id.as_str(), "tolerance_policies.policy_id")?;
        require_non_empty(policy.policy_version.as_str(), "tolerance_policies.policy_version")?;
        validate_non_negative_finite(
            policy.point_estimate_absolute_tolerance,
            "tolerance_policies.point_estimate_absolute_tolerance",
        )?;
        validate_non_negative_finite(
            policy.absolute_delta_tolerance,
            "tolerance_policies.absolute_delta_tolerance",
        )?;
        validate_non_negative_finite(
            policy.confidence_interval_bound_absolute_tolerance,
            "tolerance_policies.confidence_interval_bound_absolute_tolerance",
        )?;

        match policy.policy_kind {
            StylizedFactRegressionTolerancePolicyKind::DeterministicExactMatch => {
                if policy.require_confidence_intervals {
                    return Err(format!(
                        "deterministic exact-match tolerance policy `{}` cannot require confidence intervals",
                        policy.policy_id
                    ));
                }
            }
            StylizedFactRegressionTolerancePolicyKind::BootstrapConfidenceInterval => {
                if !policy.require_confidence_intervals {
                    return Err(format!(
                        "bootstrap confidence-interval tolerance policy `{}` must require confidence intervals",
                        policy.policy_id
                    ));
                }
            }
        }

        if policies_by_id.insert(policy.policy_id.as_str(), policy).is_some() {
            return Err(format!(
                "duplicate stylized fact regression tolerance policy id: {}",
                policy.policy_id
            ));
        }
    }

    if fixture_document.cases.is_empty() {
        return Err("stylized fact regression fixture must include cases".to_string());
    }

    let mut observed_fixture_ids: BTreeSet<&str> = BTreeSet::new();
    let mut observed_tags: BTreeSet<&str> = BTreeSet::new();
    let mut observed_diagnostic_kinds: BTreeSet<StylizedFactDiagnosticKind> = BTreeSet::new();
    for fixture_case in &fixture_document.cases {
        require_non_empty(fixture_case.fixture_id.as_str(), "cases.fixture_id")?;
        require_non_empty(fixture_case.description.as_str(), "cases.description")?;
        require_non_empty(
            fixture_case.provenance.baseline_source.as_str(),
            "cases.provenance.baseline_source",
        )?;
        require_non_empty(
            fixture_case.provenance.captured_at_utc.as_str(),
            "cases.provenance.captured_at_utc",
        )?;
        require_non_empty(
            fixture_case.provenance.update_guidance.as_str(),
            "cases.provenance.update_guidance",
        )?;

        if !observed_fixture_ids.insert(fixture_case.fixture_id.as_str()) {
            return Err(format!(
                "duplicate stylized fact regression fixture_id: {}",
                fixture_case.fixture_id
            ));
        }

        if !policies_by_id.contains_key(fixture_case.tolerance_policy_id.as_str()) {
            return Err(format!(
                "fixture case {} references unknown tolerance_policy_id `{}`",
                fixture_case.fixture_id, fixture_case.tolerance_policy_id
            ));
        }

        if fixture_case.tags.is_empty() {
            return Err(format!(
                "fixture case {} must include at least one tag",
                fixture_case.fixture_id
            ));
        }
        for tag in &fixture_case.tags {
            require_non_empty(tag.as_str(), "cases.tags")?;
            observed_tags.insert(tag.as_str());
        }

        match &fixture_case.request_payload {
            Value::Object(raw_payload) if !raw_payload.is_empty() => {}
            _ => {
                return Err(format!(
                    "fixture case {} request_payload must be a non-empty object",
                    fixture_case.fixture_id
                ));
            }
        }

        if fixture_case.expected_metrics.is_empty() {
            return Err(format!(
                "fixture case {} must include expected_metrics",
                fixture_case.fixture_id
            ));
        }

        let mut observed_metric_keys: BTreeSet<(String, String, Option<String>)> = BTreeSet::new();
        for expected_metric in &fixture_case.expected_metrics {
            require_non_empty(expected_metric.horizon_label.as_str(), "expected_metrics.horizon_label")?;
            require_non_empty(expected_metric.metric_id.as_str(), "expected_metrics.metric_id")?;

            let metric_key = (
                expected_metric.horizon_label.clone(),
                expected_metric.metric_id.clone(),
                expected_metric.metric_variant.clone(),
            );
            if !observed_metric_keys.insert(metric_key) {
                return Err(format!(
                    "fixture case {} contains duplicate expected metric entry for horizon={} metric_id={} metric_variant={:?}",
                    fixture_case.fixture_id,
                    expected_metric.horizon_label,
                    expected_metric.metric_id,
                    expected_metric.metric_variant
                ));
            }
        }

        observed_diagnostic_kinds.insert(fixture_case.diagnostic_kind);
    }

    if fixture_document
        .canonical_regression_metadata
        .required_tags
        .is_empty()
    {
        return Err(
            "stylized fact canonical_regression_metadata.required_tags must not be empty".to_string(),
        );
    }

    for required_tag in &fixture_document.canonical_regression_metadata.required_tags {
        require_non_empty(required_tag.as_str(), "canonical_regression_metadata.required_tags")?;
        if !observed_tags.contains(required_tag.as_str()) {
            return Err(format!(
                "stylized fact canonical_regression_metadata.required_tags is missing observed tag `{required_tag}`",
            ));
        }
    }

    let expected_policy_ids: BTreeSet<&str> = fixture_document
        .canonical_regression_metadata
        .required_tolerance_policy_ids
        .iter()
        .map(String::as_str)
        .collect();
    let observed_policy_ids: BTreeSet<&str> =
        fixture_document.tolerance_policies.iter().map(|policy| policy.policy_id.as_str()).collect();
    if expected_policy_ids != observed_policy_ids {
        return Err(
            "stylized fact canonical_regression_metadata.required_tolerance_policy_ids does not match observed tolerance policy ids"
                .to_string(),
        );
    }

    let expected_fixture_ids: BTreeSet<&str> = fixture_document
        .canonical_regression_metadata
        .stable_fixture_ids
        .iter()
        .map(String::as_str)
        .collect();
    if expected_fixture_ids != observed_fixture_ids {
        return Err(
            "stylized fact canonical_regression_metadata.stable_fixture_ids does not match observed fixture ids"
                .to_string(),
        );
    }

    let expected_diagnostic_kinds: BTreeSet<StylizedFactDiagnosticKind> = fixture_document
        .canonical_regression_metadata
        .required_diagnostic_kinds
        .iter()
        .copied()
        .collect();
    if expected_diagnostic_kinds != observed_diagnostic_kinds {
        return Err(
            "stylized fact canonical_regression_metadata.required_diagnostic_kinds does not match observed diagnostic kinds"
                .to_string(),
        );
    }

    Ok(())
}

fn find_manifest_entry_by_fixture_id(
    manifest: &ValidationFixtureManifestDocument,
    fixture_id: &str,
) -> Result<ValidationFixtureManifestEntry, String> {
    manifest
        .fixtures
        .iter()
        .find(|manifest_entry| manifest_entry.fixture_id == fixture_id)
        .cloned()
        .ok_or_else(|| format!("missing manifest entry for fixture_id `{fixture_id}`"))
}

fn require_non_empty(value: &str, field_name: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field_name} must be non-empty"));
    }

    Ok(())
}

fn validate_non_negative_finite(value: f64, field_name: &str) -> Result<(), String> {
    if !value.is_finite() || value < 0.0 {
        return Err(format!(
            "{field_name} must be finite and non-negative, found {value}",
        ));
    }

    Ok(())
}
