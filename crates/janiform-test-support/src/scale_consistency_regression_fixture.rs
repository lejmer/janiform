#![allow(dead_code)]

use std::collections::BTreeSet;

use janiform_verification::{
    RegressionCaseStatus, SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
    SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY, ScaleConsistencyDecisionReasonCode,
    ScaleConsistencyDriftCategory, ScaleConsistencyEngineBenchmarkInput,
    ScaleConsistencyMetricCategory, ScaleConsistencyMetricComputationFailure,
    ScaleConsistencyMetricComputationStatus, ScaleConsistencyMetricDatasetRole,
    ScaleConsistencyMetricDelta, ScaleConsistencyMetricDeltaBatchReport,
    ScaleConsistencyMetricDeltaKind, ScaleConsistencyMetricDeltaScenarioReport,
    ScaleConsistencyMetricFailureReasonCode,
};
use serde::{Deserialize, Serialize};

use crate::validation_fixture::{
    CONTENT_HASH_ALGORITHM_POLICY, LoadedFixturePayload, ValidationFixtureManifestDocument,
    ValidationFixtureManifestEntry, fixtures_root_path, load_manifest_entry_payload,
    load_validation_fixture_manifest_document,
};

pub const SCALE_CONSISTENCY_REGRESSION_FIXTURE_SCHEMA_VERSION: &str =
    "validation.scale_consistency.regression.fixture.v1";
pub const SCALE_CONSISTENCY_REGRESSION_FIXTURE_SET_ID: &str =
    "scale_consistency_regression_fixture_v1";
pub const SCALE_CONSISTENCY_REGRESSION_SCHEMA_FAMILY: &str =
    "validation.scale_consistency.regression.fixture";
pub const SCALE_CONSISTENCY_REGRESSION_SCHEMA_GENERATION: &str = "v1";
pub const SCALE_CONSISTENCY_REGRESSION_FORWARD_EXTENSION_POLICY: &str = "additive_fields_only.v1";
pub const SCALE_CONSISTENCY_REGRESSION_CASE_ORDER_POLICY: &str = "preserve_declared_case_order.v1";
pub const SCALE_CONSISTENCY_REGRESSION_CONTENT_HASH_SCOPE_POLICY: &str =
    "document_payload_bytes.v1";
pub const SCALE_CONSISTENCY_REGRESSION_REPEATED_RUN_COMPARISON_POLICY: &str =
    "repeat_full_harness_execution_and_require_identical_decision_and_report_outputs.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyRegressionFixtureDocument {
    pub schema_version: String,
    pub fixture_set_id: String,
    pub fixture_set_version: String,
    pub schema_metadata: ScaleConsistencyRegressionSchemaMetadata,
    pub determinism_metadata: ScaleConsistencyRegressionDeterminismMetadata,
    pub threshold_policy_reference: ScaleConsistencyRegressionThresholdPolicyReference,
    pub canonical_regression_metadata: ScaleConsistencyRegressionCanonicalMetadata,
    pub cases: Vec<ScaleConsistencyRegressionCase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyRegressionSchemaMetadata {
    pub schema_family: String,
    pub schema_generation: String,
    pub forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyRegressionDeterminismMetadata {
    pub case_order_policy: String,
    pub content_hash_scope: String,
    pub content_hash_algorithm: String,
    pub repeated_run_comparison_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyRegressionThresholdPolicyReference {
    pub policy_id: String,
    pub policy_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyRegressionCanonicalMetadata {
    pub required_tags: Vec<String>,
    pub required_interval_grid_labels: Vec<String>,
    pub required_expected_scenario_statuses: Vec<RegressionCaseStatus>,
    pub stable_fixture_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyRegressionProvenance {
    pub baseline_source: String,
    pub captured_at_utc: String,
    pub update_guidance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureMetricCategory {
    Returns,
    Volatility,
    Events,
}

impl FixtureMetricCategory {
    fn into_runtime(self) -> ScaleConsistencyMetricCategory {
        match self {
            Self::Returns => ScaleConsistencyMetricCategory::Returns,
            Self::Volatility => ScaleConsistencyMetricCategory::Volatility,
            Self::Events => ScaleConsistencyMetricCategory::Events,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureMetricDeltaKind {
    SignedDifference,
    AbsoluteDistance,
}

impl FixtureMetricDeltaKind {
    fn into_runtime(self) -> ScaleConsistencyMetricDeltaKind {
        match self {
            Self::SignedDifference => ScaleConsistencyMetricDeltaKind::SignedDifference,
            Self::AbsoluteDistance => ScaleConsistencyMetricDeltaKind::AbsoluteDistance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureMetricComputationStatus {
    Computed,
    Missing,
}

impl FixtureMetricComputationStatus {
    fn into_runtime(self) -> ScaleConsistencyMetricComputationStatus {
        match self {
            Self::Computed => ScaleConsistencyMetricComputationStatus::Computed,
            Self::Missing => ScaleConsistencyMetricComputationStatus::Missing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureMetricDatasetRole {
    Coarse,
    Fine,
}

impl FixtureMetricDatasetRole {
    fn into_runtime(self) -> ScaleConsistencyMetricDatasetRole {
        match self {
            Self::Coarse => ScaleConsistencyMetricDatasetRole::Coarse,
            Self::Fine => ScaleConsistencyMetricDatasetRole::Fine,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureMetricFailureReasonCode {
    InsufficientReturnObservations,
    InsufficientLaggedObservations,
    MissingOpenJumpObservations,
    MissingClosedGapObservations,
}

impl FixtureMetricFailureReasonCode {
    fn into_runtime(self) -> ScaleConsistencyMetricFailureReasonCode {
        match self {
            Self::InsufficientReturnObservations => {
                ScaleConsistencyMetricFailureReasonCode::InsufficientReturnObservations
            }
            Self::InsufficientLaggedObservations => {
                ScaleConsistencyMetricFailureReasonCode::InsufficientLaggedObservations
            }
            Self::MissingOpenJumpObservations => {
                ScaleConsistencyMetricFailureReasonCode::MissingOpenJumpObservations
            }
            Self::MissingClosedGapObservations => {
                ScaleConsistencyMetricFailureReasonCode::MissingClosedGapObservations
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureDecisionReasonCode {
    AbsoluteDeltaThresholdExceeded,
    MetricComputationMissing,
    EngineBenchmarkPrerequisiteFailed,
}

impl FixtureDecisionReasonCode {
    pub fn into_runtime(self) -> ScaleConsistencyDecisionReasonCode {
        match self {
            Self::AbsoluteDeltaThresholdExceeded => {
                ScaleConsistencyDecisionReasonCode::AbsoluteDeltaThresholdExceeded
            }
            Self::MetricComputationMissing => {
                ScaleConsistencyDecisionReasonCode::MetricComputationMissing
            }
            Self::EngineBenchmarkPrerequisiteFailed => {
                ScaleConsistencyDecisionReasonCode::EngineBenchmarkPrerequisiteFailed
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureDriftCategory {
    Returns,
    Volatility,
    EventDerived,
}

impl FixtureDriftCategory {
    pub fn into_runtime(self) -> ScaleConsistencyDriftCategory {
        match self {
            Self::Returns => ScaleConsistencyDriftCategory::Returns,
            Self::Volatility => ScaleConsistencyDriftCategory::Volatility,
            Self::EventDerived => ScaleConsistencyDriftCategory::EventDerived,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureMetricComputationFailure {
    pub dataset_role: FixtureMetricDatasetRole,
    pub reason_code: FixtureMetricFailureReasonCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lag_interval_count: Option<usize>,
}

impl FixtureMetricComputationFailure {
    fn into_runtime(self) -> ScaleConsistencyMetricComputationFailure {
        ScaleConsistencyMetricComputationFailure {
            dataset_role: self.dataset_role.into_runtime(),
            reason_code: self.reason_code.into_runtime(),
            lag_interval_count: self.lag_interval_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureMetricDelta {
    pub metric_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_variant: Option<String>,
    pub metric_category: FixtureMetricCategory,
    pub delta_kind: FixtureMetricDeltaKind,
    pub status: FixtureMetricComputationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coarse_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fine_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_delta: Option<f64>,
    pub computation_failures: Vec<FixtureMetricComputationFailure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyRegressionMetricDeltaInput {
    pub horizon_interval_count: usize,
    pub fine_subinterval_count_per_coarse_interval: usize,
    pub metric_deltas: Vec<FixtureMetricDelta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureEngineBenchmarkInput {
    pub source_report_schema_version: String,
    pub source_governance_scope_policy: String,
    pub benchmark_policy_id: String,
    pub benchmark_policy_version: String,
    pub tolerance_policy_id: String,
    pub tolerance_policy_version: String,
    pub status: RegressionCaseStatus,
}

impl FixtureEngineBenchmarkInput {
    fn into_runtime(self) -> ScaleConsistencyEngineBenchmarkInput {
        ScaleConsistencyEngineBenchmarkInput {
            source_report_schema_version: self.source_report_schema_version,
            source_governance_scope_policy: self.source_governance_scope_policy,
            benchmark_policy_id: self.benchmark_policy_id,
            benchmark_policy_version: self.benchmark_policy_version,
            tolerance_policy_id: self.tolerance_policy_id,
            tolerance_policy_version: self.tolerance_policy_version,
            status: self.status,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyRegressionExpectedMetricOutcome {
    pub metric_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_variant: Option<String>,
    pub expected_status: RegressionCaseStatus,
    pub expected_threshold_id: String,
    pub expected_decision_reason_codes: Vec<FixtureDecisionReasonCode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyRegressionExpectedOutcome {
    pub expected_batch_status: RegressionCaseStatus,
    pub expected_scenario_status: RegressionCaseStatus,
    pub expected_failed_metric_count: usize,
    pub expected_metric_outcomes: Vec<ScaleConsistencyRegressionExpectedMetricOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_primary_drift_category: Option<FixtureDriftCategory>,
    pub expected_non_drift_failure_reason_codes: Vec<FixtureDecisionReasonCode>,
    pub expected_rendered_summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyRegressionCase {
    pub fixture_id: String,
    pub scenario_id: String,
    pub parameter_set_id: String,
    pub horizon_label: String,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub expected_applied_interval_threshold_band_label: String,
    pub provenance: ScaleConsistencyRegressionProvenance,
    pub engine_benchmark_input: FixtureEngineBenchmarkInput,
    pub metric_delta_input: ScaleConsistencyRegressionMetricDeltaInput,
    pub expected_outcome: ScaleConsistencyRegressionExpectedOutcome,
    pub tags: Vec<String>,
    pub description: String,
}

impl ScaleConsistencyRegressionCase {
    pub fn to_metric_delta_batch_report(&self) -> ScaleConsistencyMetricDeltaBatchReport {
        let metric_deltas = self
            .metric_delta_input
            .metric_deltas
            .iter()
            .cloned()
            .map(|metric_delta| ScaleConsistencyMetricDelta {
                metric_id: metric_delta.metric_id,
                metric_name: None,
                metric_variant: metric_delta.metric_variant,
                metric_category: metric_delta.metric_category.into_runtime(),
                delta_kind: metric_delta.delta_kind.into_runtime(),
                horizon_label: self.horizon_label.clone(),
                horizon_interval_count: self.metric_delta_input.horizon_interval_count,
                coarse_interval_delta: self.coarse_interval_delta,
                fine_interval_delta: self.fine_interval_delta,
                status: metric_delta.status.into_runtime(),
                coarse_value: metric_delta.coarse_value,
                fine_value: metric_delta.fine_value,
                delta_value: metric_delta.delta_value,
                absolute_delta: metric_delta.absolute_delta,
                computation_failures: metric_delta
                    .computation_failures
                    .into_iter()
                    .map(FixtureMetricComputationFailure::into_runtime)
                    .collect(),
            })
            .collect();

        ScaleConsistencyMetricDeltaBatchReport {
            schema_version: SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
            ordering_policy: SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY,
            scenario_reports: vec![ScaleConsistencyMetricDeltaScenarioReport {
                scenario_id: self.scenario_id.clone(),
                parameter_set_id: self.parameter_set_id.clone(),
                horizon_label: self.horizon_label.clone(),
                horizon_interval_count: self.metric_delta_input.horizon_interval_count,
                coarse_interval_delta: self.coarse_interval_delta,
                fine_interval_delta: self.fine_interval_delta,
                fine_subinterval_count_per_coarse_interval: self
                    .metric_delta_input
                    .fine_subinterval_count_per_coarse_interval,
                metric_deltas,
            }],
        }
    }

    pub fn to_engine_benchmark_input(&self) -> ScaleConsistencyEngineBenchmarkInput {
        self.engine_benchmark_input.clone().into_runtime()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedScaleConsistencyRegressionFixture {
    pub manifest: ValidationFixtureManifestDocument,
    pub manifest_entry: ValidationFixtureManifestEntry,
    pub loaded_payload: LoadedFixturePayload,
    pub fixture_document: ScaleConsistencyRegressionFixtureDocument,
}

pub fn load_scale_consistency_regression_fixture_from_manifest()
-> Result<LoadedScaleConsistencyRegressionFixture, String> {
    let manifest = load_validation_fixture_manifest_document()?;
    let manifest_entry =
        find_manifest_entry_by_fixture_id(&manifest, SCALE_CONSISTENCY_REGRESSION_FIXTURE_SET_ID)?;

    if manifest_entry.fixture_schema_version != SCALE_CONSISTENCY_REGRESSION_FIXTURE_SCHEMA_VERSION
    {
        return Err(format!(
            "unexpected scale consistency regression fixture schema version in manifest: {}",
            manifest_entry.fixture_schema_version
        ));
    }

    let loaded_payload = load_manifest_entry_payload(&fixtures_root_path(), &manifest_entry)?;
    let fixture_document =
        parse_scale_consistency_regression_fixture_document(&loaded_payload.payload_bytes)?;
    validate_scale_consistency_regression_fixture_document(&fixture_document)?;

    Ok(LoadedScaleConsistencyRegressionFixture {
        manifest,
        manifest_entry,
        loaded_payload,
        fixture_document,
    })
}

pub fn parse_scale_consistency_regression_fixture_document(
    payload_bytes: &[u8],
) -> Result<ScaleConsistencyRegressionFixtureDocument, String> {
    serde_json::from_slice::<ScaleConsistencyRegressionFixtureDocument>(payload_bytes).map_err(
        |source| format!("failed to parse scale consistency regression fixture document: {source}"),
    )
}

pub fn validate_scale_consistency_regression_fixture_document(
    fixture_document: &ScaleConsistencyRegressionFixtureDocument,
) -> Result<(), String> {
    validate_fixture_document_identity(fixture_document)?;
    validate_fixture_document_metadata(fixture_document)?;
    validate_threshold_policy_reference(&fixture_document.threshold_policy_reference)?;
    validate_canonical_regression_metadata_presence(
        &fixture_document.canonical_regression_metadata,
    )?;

    let observed_coverage = validate_fixture_cases(&fixture_document.cases)?;
    validate_required_coverage(
        &fixture_document.canonical_regression_metadata,
        &observed_coverage,
    )?;
    validate_stable_fixture_ids(
        &fixture_document.canonical_regression_metadata,
        &observed_coverage,
    )?;

    Ok(())
}

#[derive(Debug, Default)]
struct ObservedScaleConsistencyRegressionCoverage<'a> {
    fixture_ids: BTreeSet<&'a str>,
    scenario_ids: BTreeSet<&'a str>,
    tags: BTreeSet<&'a str>,
    interval_grid_labels: BTreeSet<&'a str>,
    expected_status_labels: BTreeSet<&'static str>,
}

fn validate_fixture_document_identity(
    fixture_document: &ScaleConsistencyRegressionFixtureDocument,
) -> Result<(), String> {
    if fixture_document.schema_version != SCALE_CONSISTENCY_REGRESSION_FIXTURE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported scale consistency regression fixture schema version: {}",
            fixture_document.schema_version
        ));
    }
    if fixture_document.fixture_set_id != SCALE_CONSISTENCY_REGRESSION_FIXTURE_SET_ID {
        return Err(format!(
            "unexpected scale consistency regression fixture set id: {}",
            fixture_document.fixture_set_id
        ));
    }

    Ok(())
}

fn validate_fixture_document_metadata(
    fixture_document: &ScaleConsistencyRegressionFixtureDocument,
) -> Result<(), String> {
    require_non_empty(
        fixture_document.fixture_set_version.as_str(),
        "fixture_set_version",
    )?;
    validate_schema_metadata(&fixture_document.schema_metadata)?;
    validate_determinism_metadata(&fixture_document.determinism_metadata)?;

    Ok(())
}

fn validate_schema_metadata(
    schema_metadata: &ScaleConsistencyRegressionSchemaMetadata,
) -> Result<(), String> {
    require_non_empty(
        schema_metadata.schema_family.as_str(),
        "schema_metadata.schema_family",
    )?;
    require_non_empty(
        schema_metadata.schema_generation.as_str(),
        "schema_metadata.schema_generation",
    )?;
    require_non_empty(
        schema_metadata.forward_extension_policy.as_str(),
        "schema_metadata.forward_extension_policy",
    )?;

    if schema_metadata.schema_family != SCALE_CONSISTENCY_REGRESSION_SCHEMA_FAMILY {
        return Err(format!(
            "unexpected scale consistency regression schema family: {}",
            schema_metadata.schema_family
        ));
    }
    if schema_metadata.schema_generation != SCALE_CONSISTENCY_REGRESSION_SCHEMA_GENERATION {
        return Err(format!(
            "unexpected scale consistency regression schema generation: {}",
            schema_metadata.schema_generation
        ));
    }
    if schema_metadata.forward_extension_policy
        != SCALE_CONSISTENCY_REGRESSION_FORWARD_EXTENSION_POLICY
    {
        return Err(format!(
            "unexpected scale consistency regression forward extension policy: {}",
            schema_metadata.forward_extension_policy
        ));
    }

    Ok(())
}

fn validate_determinism_metadata(
    determinism_metadata: &ScaleConsistencyRegressionDeterminismMetadata,
) -> Result<(), String> {
    require_non_empty(
        determinism_metadata.case_order_policy.as_str(),
        "determinism_metadata.case_order_policy",
    )?;
    require_non_empty(
        determinism_metadata.content_hash_scope.as_str(),
        "determinism_metadata.content_hash_scope",
    )?;
    require_non_empty(
        determinism_metadata.content_hash_algorithm.as_str(),
        "determinism_metadata.content_hash_algorithm",
    )?;
    require_non_empty(
        determinism_metadata.repeated_run_comparison_policy.as_str(),
        "determinism_metadata.repeated_run_comparison_policy",
    )?;

    if determinism_metadata.case_order_policy != SCALE_CONSISTENCY_REGRESSION_CASE_ORDER_POLICY {
        return Err(format!(
            "unexpected scale consistency regression case order policy: {}",
            determinism_metadata.case_order_policy
        ));
    }
    if determinism_metadata.content_hash_scope
        != SCALE_CONSISTENCY_REGRESSION_CONTENT_HASH_SCOPE_POLICY
    {
        return Err(format!(
            "unexpected scale consistency regression content hash scope policy: {}",
            determinism_metadata.content_hash_scope
        ));
    }
    if determinism_metadata.content_hash_algorithm != CONTENT_HASH_ALGORITHM_POLICY {
        return Err(format!(
            "unexpected scale consistency regression content hash algorithm policy: {}",
            determinism_metadata.content_hash_algorithm
        ));
    }
    if determinism_metadata.repeated_run_comparison_policy
        != SCALE_CONSISTENCY_REGRESSION_REPEATED_RUN_COMPARISON_POLICY
    {
        return Err(format!(
            "unexpected scale consistency regression repeated-run comparison policy: {}",
            determinism_metadata.repeated_run_comparison_policy
        ));
    }

    Ok(())
}

fn validate_threshold_policy_reference(
    threshold_policy_reference: &ScaleConsistencyRegressionThresholdPolicyReference,
) -> Result<(), String> {
    require_non_empty(
        threshold_policy_reference.policy_id.as_str(),
        "threshold_policy_reference.policy_id",
    )?;
    require_non_empty(
        threshold_policy_reference.policy_version.as_str(),
        "threshold_policy_reference.policy_version",
    )?;

    Ok(())
}

fn validate_canonical_regression_metadata_presence(
    canonical_regression_metadata: &ScaleConsistencyRegressionCanonicalMetadata,
) -> Result<(), String> {
    if canonical_regression_metadata.required_tags.is_empty() {
        return Err("scale consistency regression fixture must declare required_tags".to_string());
    }
    if canonical_regression_metadata
        .required_interval_grid_labels
        .is_empty()
    {
        return Err(
            "scale consistency regression fixture must declare required_interval_grid_labels"
                .to_string(),
        );
    }
    if canonical_regression_metadata
        .required_expected_scenario_statuses
        .is_empty()
    {
        return Err(
            "scale consistency regression fixture must declare required_expected_scenario_statuses"
                .to_string(),
        );
    }
    if canonical_regression_metadata.stable_fixture_ids.is_empty() {
        return Err(
            "scale consistency regression fixture must declare stable_fixture_ids".to_string(),
        );
    }

    Ok(())
}

fn validate_fixture_cases(
    fixture_cases: &'_ [ScaleConsistencyRegressionCase],
) -> Result<ObservedScaleConsistencyRegressionCoverage<'_>, String> {
    if fixture_cases.is_empty() {
        return Err("scale consistency regression fixture must include cases".to_string());
    }

    let mut observed_coverage = ObservedScaleConsistencyRegressionCoverage::default();
    for fixture_case in fixture_cases {
        validate_fixture_case(fixture_case, &mut observed_coverage)?;
    }

    Ok(observed_coverage)
}

fn validate_fixture_case<'a>(
    fixture_case: &'a ScaleConsistencyRegressionCase,
    observed_coverage: &mut ObservedScaleConsistencyRegressionCoverage<'a>,
) -> Result<(), String> {
    validate_fixture_case_identity(fixture_case, observed_coverage)?;
    validate_fixture_case_metadata(fixture_case, observed_coverage)?;
    validate_fixture_case_intervals(fixture_case)?;
    validate_fixture_case_engine_benchmark_input(fixture_case)?;

    let metric_keys = validate_fixture_case_metric_deltas(fixture_case)?;
    validate_fixture_case_expected_outcome(fixture_case, &metric_keys)?;
    record_fixture_case_coverage(fixture_case, observed_coverage);

    Ok(())
}

fn validate_fixture_case_identity<'a>(
    fixture_case: &'a ScaleConsistencyRegressionCase,
    observed_coverage: &mut ObservedScaleConsistencyRegressionCoverage<'a>,
) -> Result<(), String> {
    require_non_empty(fixture_case.fixture_id.as_str(), "cases.fixture_id")?;
    require_non_empty(fixture_case.scenario_id.as_str(), "cases.scenario_id")?;
    require_non_empty(
        fixture_case.parameter_set_id.as_str(),
        "cases.parameter_set_id",
    )?;
    require_non_empty(fixture_case.horizon_label.as_str(), "cases.horizon_label")?;

    if !observed_coverage
        .fixture_ids
        .insert(fixture_case.fixture_id.as_str())
    {
        return Err(format!(
            "duplicate scale consistency regression fixture_id: {}",
            fixture_case.fixture_id
        ));
    }
    if !observed_coverage
        .scenario_ids
        .insert(fixture_case.scenario_id.as_str())
    {
        return Err(format!(
            "duplicate scale consistency regression scenario_id: {}",
            fixture_case.scenario_id
        ));
    }

    Ok(())
}

fn validate_fixture_case_metadata<'a>(
    fixture_case: &'a ScaleConsistencyRegressionCase,
    observed_coverage: &mut ObservedScaleConsistencyRegressionCoverage<'a>,
) -> Result<(), String> {
    require_non_empty(
        fixture_case
            .expected_applied_interval_threshold_band_label
            .as_str(),
        "cases.expected_applied_interval_threshold_band_label",
    )?;
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
    require_non_empty(fixture_case.description.as_str(), "cases.description")?;

    if fixture_case.tags.is_empty() {
        return Err(format!(
            "fixture case `{}` must include at least one tag",
            fixture_case.fixture_id
        ));
    }

    for tag in &fixture_case.tags {
        require_non_empty(tag.as_str(), "cases.tags")?;
        observed_coverage.tags.insert(tag.as_str());
    }

    Ok(())
}

fn validate_fixture_case_intervals(
    fixture_case: &ScaleConsistencyRegressionCase,
) -> Result<(), String> {
    validate_positive_finite(
        fixture_case.coarse_interval_delta,
        "cases.coarse_interval_delta",
    )?;
    validate_positive_finite(
        fixture_case.fine_interval_delta,
        "cases.fine_interval_delta",
    )?;

    if fixture_case.fine_interval_delta >= fixture_case.coarse_interval_delta {
        return Err(format!(
            "fixture case `{}` fine_interval_delta must be smaller than coarse_interval_delta",
            fixture_case.fixture_id
        ));
    }

    let interval_ratio = fixture_case.coarse_interval_delta / fixture_case.fine_interval_delta;
    if (interval_ratio.round() - interval_ratio).abs() > 1e-9 {
        return Err(format!(
            "fixture case `{}` must use an integer coarse-to-fine interval ratio",
            fixture_case.fixture_id
        ));
    }

    Ok(())
}

fn validate_fixture_case_engine_benchmark_input(
    fixture_case: &ScaleConsistencyRegressionCase,
) -> Result<(), String> {
    require_non_empty(
        fixture_case
            .engine_benchmark_input
            .source_report_schema_version
            .as_str(),
        "cases.engine_benchmark_input.source_report_schema_version",
    )?;
    require_non_empty(
        fixture_case
            .engine_benchmark_input
            .source_governance_scope_policy
            .as_str(),
        "cases.engine_benchmark_input.source_governance_scope_policy",
    )?;
    require_non_empty(
        fixture_case
            .engine_benchmark_input
            .benchmark_policy_id
            .as_str(),
        "cases.engine_benchmark_input.benchmark_policy_id",
    )?;
    require_non_empty(
        fixture_case
            .engine_benchmark_input
            .benchmark_policy_version
            .as_str(),
        "cases.engine_benchmark_input.benchmark_policy_version",
    )?;
    require_non_empty(
        fixture_case
            .engine_benchmark_input
            .tolerance_policy_id
            .as_str(),
        "cases.engine_benchmark_input.tolerance_policy_id",
    )?;
    require_non_empty(
        fixture_case
            .engine_benchmark_input
            .tolerance_policy_version
            .as_str(),
        "cases.engine_benchmark_input.tolerance_policy_version",
    )?;

    Ok(())
}

fn validate_fixture_case_metric_deltas(
    fixture_case: &ScaleConsistencyRegressionCase,
) -> Result<BTreeSet<(String, Option<String>)>, String> {
    if fixture_case.metric_delta_input.horizon_interval_count == 0 {
        return Err(format!(
            "fixture case `{}` must use a positive horizon_interval_count",
            fixture_case.fixture_id
        ));
    }
    if fixture_case
        .metric_delta_input
        .fine_subinterval_count_per_coarse_interval
        == 0
    {
        return Err(format!(
            "fixture case `{}` must use a positive fine_subinterval_count_per_coarse_interval",
            fixture_case.fixture_id
        ));
    }
    if fixture_case.metric_delta_input.metric_deltas.is_empty() {
        return Err(format!(
            "fixture case `{}` must include metric_deltas",
            fixture_case.fixture_id
        ));
    }

    let mut metric_keys = BTreeSet::new();
    for metric_delta in &fixture_case.metric_delta_input.metric_deltas {
        let metric_key = validate_fixture_case_metric_delta(fixture_case, metric_delta)?;
        if !metric_keys.insert(metric_key.clone()) {
            return Err(format!(
                "fixture case `{}` contains duplicate metric delta for metric_id `{}` and metric_variant {:?}",
                fixture_case.fixture_id, metric_key.0, metric_key.1
            ));
        }
    }

    Ok(metric_keys)
}

fn validate_fixture_case_metric_delta(
    fixture_case: &ScaleConsistencyRegressionCase,
    metric_delta: &FixtureMetricDelta,
) -> Result<(String, Option<String>), String> {
    require_non_empty(
        metric_delta.metric_id.as_str(),
        "cases.metric_deltas.metric_id",
    )?;
    if let Some(metric_variant) = &metric_delta.metric_variant {
        require_non_empty(
            metric_variant.as_str(),
            "cases.metric_deltas.metric_variant",
        )?;
    }

    validate_optional_non_negative_finite(
        metric_delta.absolute_delta,
        "cases.metric_deltas.absolute_delta",
    )?;
    validate_optional_finite(
        metric_delta.coarse_value,
        "cases.metric_deltas.coarse_value",
    )?;
    validate_optional_finite(metric_delta.fine_value, "cases.metric_deltas.fine_value")?;
    validate_optional_finite(metric_delta.delta_value, "cases.metric_deltas.delta_value")?;

    if matches!(metric_delta.status, FixtureMetricComputationStatus::Missing)
        && metric_delta.computation_failures.is_empty()
    {
        return Err(format!(
            "fixture case `{}` missing metric `{}` must include computation_failures",
            fixture_case.fixture_id, metric_delta.metric_id
        ));
    }

    Ok((
        metric_delta.metric_id.clone(),
        metric_delta.metric_variant.clone(),
    ))
}

fn validate_fixture_case_expected_outcome(
    fixture_case: &ScaleConsistencyRegressionCase,
    metric_keys: &BTreeSet<(String, Option<String>)>,
) -> Result<(), String> {
    if fixture_case.expected_outcome.expected_metric_outcomes.len()
        != fixture_case.metric_delta_input.metric_deltas.len()
    {
        return Err(format!(
            "fixture case `{}` expected_metric_outcomes must cover every metric delta",
            fixture_case.fixture_id
        ));
    }

    let mut expected_metric_keys = BTreeSet::new();
    for expected_metric_outcome in &fixture_case.expected_outcome.expected_metric_outcomes {
        require_non_empty(
            expected_metric_outcome.metric_id.as_str(),
            "cases.expected_metric_outcomes.metric_id",
        )?;
        require_non_empty(
            expected_metric_outcome.expected_threshold_id.as_str(),
            "cases.expected_metric_outcomes.expected_threshold_id",
        )?;

        let metric_key = (
            expected_metric_outcome.metric_id.clone(),
            expected_metric_outcome.metric_variant.clone(),
        );
        if !metric_keys.contains(&metric_key) {
            return Err(format!(
                "fixture case `{}` expected_metric_outcomes references unknown metric_id `{}` and metric_variant {:?}",
                fixture_case.fixture_id, metric_key.0, metric_key.1
            ));
        }
        if !expected_metric_keys.insert(metric_key.clone()) {
            return Err(format!(
                "fixture case `{}` contains duplicate expected metric outcome for metric_id `{}` and metric_variant {:?}",
                fixture_case.fixture_id, metric_key.0, metric_key.1
            ));
        }
    }

    validate_fixture_case_expected_status_consistency(fixture_case)?;

    require_non_empty(
        fixture_case
            .expected_outcome
            .expected_rendered_summary
            .as_str(),
        "cases.expected_outcome.expected_rendered_summary",
    )?;

    Ok(())
}

fn validate_fixture_case_expected_status_consistency(
    fixture_case: &ScaleConsistencyRegressionCase,
) -> Result<(), String> {
    let failed_metric_count = fixture_case
        .expected_outcome
        .expected_metric_outcomes
        .iter()
        .filter(|expected_metric_outcome| {
            expected_metric_outcome.expected_status == RegressionCaseStatus::Fail
        })
        .count();
    if failed_metric_count != fixture_case.expected_outcome.expected_failed_metric_count {
        return Err(format!(
            "fixture case `{}` expected_failed_metric_count must equal the number of failing expected_metric_outcomes",
            fixture_case.fixture_id
        ));
    }

    if fixture_case.expected_outcome.expected_scenario_status == RegressionCaseStatus::Pass
        && fixture_case
            .expected_outcome
            .expected_primary_drift_category
            .is_some()
    {
        return Err(format!(
            "fixture case `{}` pass scenarios must not define expected_primary_drift_category",
            fixture_case.fixture_id
        ));
    }

    Ok(())
}

fn record_fixture_case_coverage<'a>(
    fixture_case: &'a ScaleConsistencyRegressionCase,
    observed_coverage: &mut ObservedScaleConsistencyRegressionCoverage<'a>,
) {
    observed_coverage.interval_grid_labels.insert(
        fixture_case
            .expected_applied_interval_threshold_band_label
            .as_str(),
    );
    observed_coverage.expected_status_labels.insert(
        match fixture_case.expected_outcome.expected_scenario_status {
            RegressionCaseStatus::Pass => "pass",
            RegressionCaseStatus::Fail => "fail",
        },
    );
}

fn validate_required_coverage(
    canonical_regression_metadata: &ScaleConsistencyRegressionCanonicalMetadata,
    observed_coverage: &ObservedScaleConsistencyRegressionCoverage<'_>,
) -> Result<(), String> {
    for required_tag in &canonical_regression_metadata.required_tags {
        require_non_empty(
            required_tag.as_str(),
            "canonical_regression_metadata.required_tags",
        )?;
        if !observed_coverage.tags.contains(required_tag.as_str()) {
            return Err(format!(
                "scale consistency regression fixture is missing required tag coverage `{required_tag}`"
            ));
        }
    }

    for required_interval_grid_label in &canonical_regression_metadata.required_interval_grid_labels
    {
        require_non_empty(
            required_interval_grid_label.as_str(),
            "canonical_regression_metadata.required_interval_grid_labels",
        )?;
        if !observed_coverage
            .interval_grid_labels
            .contains(required_interval_grid_label.as_str())
        {
            return Err(format!(
                "scale consistency regression fixture is missing required interval band coverage `{required_interval_grid_label}`"
            ));
        }
    }

    for required_status in &canonical_regression_metadata.required_expected_scenario_statuses {
        let required_status_label = match required_status {
            RegressionCaseStatus::Pass => "pass",
            RegressionCaseStatus::Fail => "fail",
        };
        if !observed_coverage
            .expected_status_labels
            .contains(required_status_label)
        {
            return Err(format!(
                "scale consistency regression fixture is missing required scenario status coverage `{required_status:?}`"
            ));
        }
    }

    Ok(())
}

fn validate_stable_fixture_ids(
    canonical_regression_metadata: &ScaleConsistencyRegressionCanonicalMetadata,
    observed_coverage: &ObservedScaleConsistencyRegressionCoverage<'_>,
) -> Result<(), String> {
    let mut stable_fixture_ids = BTreeSet::new();
    for stable_fixture_id in &canonical_regression_metadata.stable_fixture_ids {
        require_non_empty(
            stable_fixture_id.as_str(),
            "canonical_regression_metadata.stable_fixture_ids",
        )?;

        if !observed_coverage
            .fixture_ids
            .contains(stable_fixture_id.as_str())
        {
            return Err(format!(
                "scale consistency regression fixture stable_fixture_ids references unknown fixture_id `{stable_fixture_id}`"
            ));
        }
        if !stable_fixture_ids.insert(stable_fixture_id.as_str()) {
            return Err(format!(
                "duplicate scale consistency regression stable_fixture_id `{stable_fixture_id}`"
            ));
        }
    }

    Ok(())
}

fn require_non_empty(value: &str, field_name: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field_name} must be a non-empty string"));
    }

    Ok(())
}

fn validate_positive_finite(value: f64, field_name: &str) -> Result<(), String> {
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{field_name} must be finite and strictly positive"));
    }

    Ok(())
}

fn validate_optional_finite(value: Option<f64>, field_name: &str) -> Result<(), String> {
    if let Some(value) = value
        && !value.is_finite()
    {
        return Err(format!("{field_name} must be finite when provided"));
    }

    Ok(())
}

fn validate_optional_non_negative_finite(
    value: Option<f64>,
    field_name: &str,
) -> Result<(), String> {
    if let Some(value) = value
        && (!value.is_finite() || value < 0.0)
    {
        return Err(format!(
            "{field_name} must be finite and non-negative when provided"
        ));
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
        .find(|entry| entry.fixture_id == fixture_id)
        .cloned()
        .ok_or_else(|| format!("missing manifest entry for fixture_id `{fixture_id}`"))
}
