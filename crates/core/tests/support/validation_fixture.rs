#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use janiform_core::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact};
use serde::Deserialize;

pub const VALIDATION_FIXTURE_MANIFEST_SCHEMA_VERSION: &str = "validation.fixture.manifest.v1";
pub const CALENDAR_REGRESSION_FIXTURE_SCHEMA_VERSION: &str = "calendar.service.regression.v1";
pub const CALENDAR_REGRESSION_FIXTURE_SET_ID: &str = "calendar_service_regression_matrix_v1";
pub const HOURLY_FACTOR_REGRESSION_FIXTURE_SCHEMA_VERSION: &str = "hourly.factor.regression.v1";
pub const HOURLY_FACTOR_REGRESSION_FIXTURE_SET_ID: &str = "hourly_factor_regression_matrix_v1";
pub const MANIFEST_SCHEMA_FAMILY: &str = "validation.fixture.manifest";
pub const MANIFEST_V0_UPDATE_POLICY: &str = "additive_only_and_major_on_breaking_change.v1";
pub const MANIFEST_LOADING_ORDER_POLICY: &str = "lexicographic_by_fixture_id.v1";
pub const CONTENT_HASH_ALGORITHM_POLICY: &str = "fnv1a_64_hex_utf8.v1";
pub const CALENDAR_SCHEMA_FAMILY: &str = "validation.calendar.service.regression";
pub const CALENDAR_SCHEMA_GENERATION: &str = "v0.x";
pub const CALENDAR_FORWARD_EXTENSION_POLICY: &str = "additive_fields_only.v1";
pub const CALENDAR_CASE_ORDER_POLICY: &str = "preserve_declared_case_order.v1";
pub const CALENDAR_CONTENT_HASH_SCOPE_POLICY: &str = "document_payload_bytes.v1";
pub const HOURLY_FACTOR_THRESHOLD_CONFIGURATION_SCHEMA_VERSION: &str =
    "validation.hourly_factor.thresholds.v1";
pub const STATE_CLASSIFICATION_STRICTNESS_POLICY: &str = "exact_match_required.v1";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationFixtureManifestDocument {
    pub schema_version: String,
    pub manifest_id: String,
    pub manifest_version: String,
    pub versioning_policy: ValidationFixtureVersioningPolicy,
    pub determinism_policy: ValidationFixtureDeterminismPolicy,
    pub fixtures: Vec<ValidationFixtureManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationFixtureVersioningPolicy {
    pub schema_family: String,
    pub v0_update_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationFixtureDeterminismPolicy {
    pub fixture_loading_order: String,
    pub content_hash_algorithm: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationFixtureDomain {
    Calendar,
    Seasonality,
    HourlyFactors,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationFixtureManifestEntry {
    pub fixture_id: String,
    pub domain: ValidationFixtureDomain,
    pub fixture_schema_version: String,
    pub relative_path: String,
    pub exchange_codes: Vec<String>,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarRegressionFixtureDocument {
    pub schema_version: String,
    pub fixture_set_id: String,
    pub schema_metadata: CalendarRegressionSchemaMetadata,
    pub determinism_metadata: CalendarRegressionDeterminismMetadata,
    pub canonical_regression_metadata: CalendarCanonicalRegressionMetadata,
    pub calendar_registry: ExchangeCalendarRegistryArtifact,
    pub cases: Vec<CalendarRegressionCaseFixture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarRegressionSchemaMetadata {
    pub schema_family: String,
    pub schema_generation: String,
    pub forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarRegressionDeterminismMetadata {
    pub fixture_loading_order: String,
    pub content_hash_scope: String,
    pub content_hash_algorithm: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarCanonicalRegressionMetadata {
    pub required_tags: Vec<String>,
    pub stable_fixture_ids_by_tag: BTreeMap<String, Vec<String>>,
    pub historical_holiday_expected_years: Vec<i32>,
    pub historical_holiday_expected_intraday_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarRegressionCaseFixture {
    pub fixture_id: String,
    pub exchange_code: String,
    pub timestamp_utc: String,
    pub expected_state: String,
    pub tags: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HourlyFactorRegressionFixtureDocument {
    pub schema_version: String,
    pub fixture_set_id: String,
    pub calendar_fixture_set_id: String,
    pub threshold_configuration: HourlyFactorRegressionThresholdConfiguration,
    pub canonical_regression_metadata: HourlyFactorRegressionCanonicalMetadata,
    pub canonical_output_cases: Vec<HourlyFactorCanonicalOutputCaseFixture>,
    pub missing_data_guard_cases: Vec<HourlyFactorMissingDataGuardCaseFixture>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HourlyFactorRegressionToleranceConfig {
    pub s_h_absolute_tolerance: f64,
    pub nu_h_absolute_tolerance: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HourlyFactorRegressionFactorRangePolicy {
    pub s_h_min: f64,
    pub s_h_max: f64,
    pub nu_h_min: f64,
    pub nu_h_max: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HourlyFactorThresholdChangeLogEntry {
    pub policy_version: String,
    pub recorded_at_utc: String,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HourlyFactorRegressionThresholdConfiguration {
    pub schema_version: String,
    pub policy_version: String,
    pub state_classification_strictness: String,
    pub tolerance_config: HourlyFactorRegressionToleranceConfig,
    pub factor_range_by_fixture_class: BTreeMap<String, HourlyFactorRegressionFactorRangePolicy>,
    pub change_log: Vec<HourlyFactorThresholdChangeLogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HourlyFactorRegressionCanonicalMetadata {
    pub required_tags: Vec<String>,
    pub stable_canonical_output_fixture_ids: Vec<String>,
    pub stable_missing_data_guard_fixture_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HourlyFactorCanonicalOutputCaseFixture {
    pub fixture_id: String,
    pub exchange_code: String,
    pub range_start_utc: String,
    pub range_end_utc: String,
    pub seasonality_source: HourlySeasonalitySourceFixture,
    pub news_intensity_source: HourlyNewsIntensitySourceFixture,
    pub expected_s_h: Vec<f64>,
    pub expected_nu_h: Vec<f64>,
    pub expected_skipped_local_hour_interval_count: usize,
    pub expected_repeated_local_hour_interval_count: usize,
    pub tags: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HourlyFactorMissingDataGuardCaseFixture {
    pub fixture_id: String,
    pub exchange_code: String,
    pub range_start_utc: String,
    pub range_end_utc: String,
    pub source_timestamps_utc: Vec<String>,
    pub source_seasonality_factors: Vec<f64>,
    pub settings: HourlySeasonalitySettingsFixture,
    pub expected_outcome: HourlyMissingDataGuardExpectedOutcomeFixture,
    pub tags: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HourlySeasonalitySettingsFixture {
    pub interpolation_method: String,
    pub aggregation_method: String,
    pub sparse_fallback_policy: String,
    pub sparse_hour_minimum_observation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "source_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HourlySeasonalitySourceFixture {
    Explicit {
        hourly_factors: Vec<f64>,
        materialization_method: String,
    },
    Materialized {
        source_timestamps_utc: Vec<String>,
        source_seasonality_factors: Vec<f64>,
        interpolation_method: String,
        aggregation_method: String,
        sparse_fallback_policy: String,
        sparse_hour_minimum_observation_count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "source_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HourlyNewsIntensitySourceFixture {
    Explicit {
        hourly_factors: Vec<f64>,
        materialization_method: String,
    },
    Materialized {
        closed_window_baseline_intensity: f64,
        open_window_override_by_interval_index: BTreeMap<String, f64>,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "outcome_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HourlyMissingDataGuardExpectedOutcomeFixture {
    Success {
        expected_s_h: Vec<f64>,
        expected_sparse_interval_count: usize,
        expected_interpolated_interval_count: usize,
        expected_fallback_interval_count: usize,
    },
    Failure {
        expected_error_variant: String,
        expected_error_message_substring: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedFixturePayload {
    pub fixture_id: String,
    pub fixture_schema_version: String,
    pub relative_path: String,
    pub exchange_codes: Vec<String>,
    pub payload_bytes: Vec<u8>,
    pub observed_content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedCalendarRegressionFixture {
    pub manifest: ValidationFixtureManifestDocument,
    pub manifest_entry: ValidationFixtureManifestEntry,
    pub loaded_payload: LoadedFixturePayload,
    pub fixture_document: CalendarRegressionFixtureDocument,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedHourlyFactorRegressionFixture {
    pub manifest: ValidationFixtureManifestDocument,
    pub manifest_entry: ValidationFixtureManifestEntry,
    pub loaded_payload: LoadedFixturePayload,
    pub fixture_document: HourlyFactorRegressionFixtureDocument,
}

pub fn load_validation_fixture_manifest_document()
-> Result<ValidationFixtureManifestDocument, String> {
    let manifest_path = validation_fixture_manifest_path();
    let manifest_raw = fs::read_to_string(&manifest_path).map_err(|source| {
        format!(
            "failed to read validation fixture manifest `{}`: {source}",
            manifest_path.display()
        )
    })?;
    let manifest = serde_json::from_str::<ValidationFixtureManifestDocument>(&manifest_raw)
        .map_err(|source| {
            format!(
                "failed to parse validation fixture manifest `{}`: {source}",
                manifest_path.display()
            )
        })?;

    validate_validation_fixture_manifest_document(&manifest)?;

    Ok(manifest)
}

pub fn validate_validation_fixture_manifest_document(
    manifest: &ValidationFixtureManifestDocument,
) -> Result<(), String> {
    if manifest.schema_version != VALIDATION_FIXTURE_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "unsupported validation fixture manifest schema version: {}",
            manifest.schema_version
        ));
    }

    require_non_empty(&manifest.manifest_id, "manifest_id")?;
    require_non_empty(&manifest.manifest_version, "manifest_version")?;

    if manifest.versioning_policy.schema_family != MANIFEST_SCHEMA_FAMILY {
        return Err(format!(
            "unexpected manifest schema family: {}",
            manifest.versioning_policy.schema_family
        ));
    }

    if manifest.versioning_policy.v0_update_policy != MANIFEST_V0_UPDATE_POLICY {
        return Err(format!(
            "unexpected manifest v0 update policy: {}",
            manifest.versioning_policy.v0_update_policy
        ));
    }

    if manifest.determinism_policy.fixture_loading_order != MANIFEST_LOADING_ORDER_POLICY {
        return Err(format!(
            "unexpected manifest fixture loading order policy: {}",
            manifest.determinism_policy.fixture_loading_order
        ));
    }

    if manifest.determinism_policy.content_hash_algorithm != CONTENT_HASH_ALGORITHM_POLICY {
        return Err(format!(
            "unexpected manifest content hash algorithm policy: {}",
            manifest.determinism_policy.content_hash_algorithm
        ));
    }

    if manifest.fixtures.is_empty() {
        return Err("validation fixture manifest must contain at least one entry".to_string());
    }

    let mut observed_fixture_ids: BTreeSet<String> = BTreeSet::new();
    for manifest_entry in &manifest.fixtures {
        validate_manifest_entry(manifest_entry)?;
        if !observed_fixture_ids.insert(manifest_entry.fixture_id.clone()) {
            return Err(format!(
                "duplicate fixture_id in validation fixture manifest: {}",
                manifest_entry.fixture_id
            ));
        }
    }

    Ok(())
}

#[allow(dead_code)]
pub fn sorted_manifest_entries(
    manifest: &ValidationFixtureManifestDocument,
) -> Vec<ValidationFixtureManifestEntry> {
    let mut sorted_entries = manifest.fixtures.clone();
    sorted_entries
        .sort_by(|left_entry, right_entry| left_entry.fixture_id.cmp(&right_entry.fixture_id));

    sorted_entries
}

pub fn load_manifest_entry_payload(
    fixtures_root: &Path,
    manifest_entry: &ValidationFixtureManifestEntry,
) -> Result<LoadedFixturePayload, String> {
    let payload_path = fixtures_root.join(&manifest_entry.relative_path);
    let payload_bytes = fs::read(&payload_path).map_err(|source| {
        format!(
            "failed to read fixture payload for {} at `{}`: {source}",
            manifest_entry.fixture_id,
            payload_path.display()
        )
    })?;

    let observed_content_hash = compute_content_hash_hex(&payload_bytes);
    if observed_content_hash != manifest_entry.content_hash {
        return Err(format!(
            "fixture content hash mismatch for {}: expected {}, observed {}",
            manifest_entry.fixture_id, manifest_entry.content_hash, observed_content_hash
        ));
    }

    Ok(LoadedFixturePayload {
        fixture_id: manifest_entry.fixture_id.clone(),
        fixture_schema_version: manifest_entry.fixture_schema_version.clone(),
        relative_path: manifest_entry.relative_path.clone(),
        exchange_codes: manifest_entry.exchange_codes.clone(),
        payload_bytes,
        observed_content_hash,
    })
}

pub fn load_calendar_regression_fixture_from_manifest()
-> Result<LoadedCalendarRegressionFixture, String> {
    let manifest = load_validation_fixture_manifest_document()?;
    let manifest_entry =
        find_manifest_entry_by_fixture_id(&manifest, CALENDAR_REGRESSION_FIXTURE_SET_ID)?;

    if manifest_entry.fixture_schema_version != CALENDAR_REGRESSION_FIXTURE_SCHEMA_VERSION {
        return Err(format!(
            "unexpected calendar fixture schema version in manifest: {}",
            manifest_entry.fixture_schema_version
        ));
    }

    let loaded_payload = load_manifest_entry_payload(&fixtures_root_path(), &manifest_entry)?;
    let fixture_document =
        parse_calendar_regression_fixture_document(&loaded_payload.payload_bytes)?;

    validate_calendar_regression_fixture_document(&fixture_document)?;

    Ok(LoadedCalendarRegressionFixture {
        manifest,
        manifest_entry,
        loaded_payload,
        fixture_document,
    })
}

pub fn load_hourly_factor_regression_fixture_from_manifest()
-> Result<LoadedHourlyFactorRegressionFixture, String> {
    let manifest = load_validation_fixture_manifest_document()?;
    let manifest_entry =
        find_manifest_entry_by_fixture_id(&manifest, HOURLY_FACTOR_REGRESSION_FIXTURE_SET_ID)?;

    if manifest_entry.fixture_schema_version != HOURLY_FACTOR_REGRESSION_FIXTURE_SCHEMA_VERSION {
        return Err(format!(
            "unexpected hourly factor fixture schema version in manifest: {}",
            manifest_entry.fixture_schema_version
        ));
    }

    let loaded_payload = load_manifest_entry_payload(&fixtures_root_path(), &manifest_entry)?;
    let fixture_document =
        parse_hourly_factor_regression_fixture_document(&loaded_payload.payload_bytes)?;

    validate_hourly_factor_regression_fixture_document(&fixture_document)?;

    Ok(LoadedHourlyFactorRegressionFixture {
        manifest,
        manifest_entry,
        loaded_payload,
        fixture_document,
    })
}

pub fn parse_calendar_regression_fixture_document(
    payload_bytes: &[u8],
) -> Result<CalendarRegressionFixtureDocument, String> {
    serde_json::from_slice::<CalendarRegressionFixtureDocument>(payload_bytes)
        .map_err(|source| format!("failed to parse calendar regression fixture document: {source}"))
}

pub fn parse_hourly_factor_regression_fixture_document(
    payload_bytes: &[u8],
) -> Result<HourlyFactorRegressionFixtureDocument, String> {
    serde_json::from_slice::<HourlyFactorRegressionFixtureDocument>(payload_bytes).map_err(
        |source| format!("failed to parse hourly factor regression fixture document: {source}"),
    )
}

pub fn validate_calendar_regression_fixture_document(
    fixture_document: &CalendarRegressionFixtureDocument,
) -> Result<(), String> {
    if fixture_document.schema_version != CALENDAR_REGRESSION_FIXTURE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported calendar regression fixture schema version: {}",
            fixture_document.schema_version
        ));
    }

    if fixture_document.fixture_set_id != CALENDAR_REGRESSION_FIXTURE_SET_ID {
        return Err(format!(
            "unexpected calendar regression fixture set id: {}",
            fixture_document.fixture_set_id
        ));
    }

    if fixture_document.schema_metadata.schema_family != CALENDAR_SCHEMA_FAMILY {
        return Err(format!(
            "unexpected calendar regression schema family: {}",
            fixture_document.schema_metadata.schema_family
        ));
    }

    if fixture_document.schema_metadata.schema_generation != CALENDAR_SCHEMA_GENERATION {
        return Err(format!(
            "unexpected calendar regression schema generation: {}",
            fixture_document.schema_metadata.schema_generation
        ));
    }

    if fixture_document.schema_metadata.forward_extension_policy
        != CALENDAR_FORWARD_EXTENSION_POLICY
    {
        return Err(format!(
            "unexpected calendar regression forward extension policy: {}",
            fixture_document.schema_metadata.forward_extension_policy
        ));
    }

    if fixture_document.determinism_metadata.fixture_loading_order != CALENDAR_CASE_ORDER_POLICY {
        return Err(format!(
            "unexpected calendar regression fixture loading order policy: {}",
            fixture_document.determinism_metadata.fixture_loading_order
        ));
    }

    if fixture_document.determinism_metadata.content_hash_scope
        != CALENDAR_CONTENT_HASH_SCOPE_POLICY
    {
        return Err(format!(
            "unexpected calendar regression content hash scope policy: {}",
            fixture_document.determinism_metadata.content_hash_scope
        ));
    }

    if fixture_document.determinism_metadata.content_hash_algorithm != CONTENT_HASH_ALGORITHM_POLICY
    {
        return Err(format!(
            "unexpected calendar regression content hash algorithm policy: {}",
            fixture_document.determinism_metadata.content_hash_algorithm
        ));
    }

    let exchange_codes: BTreeSet<&str> = fixture_document
        .calendar_registry
        .exchanges
        .iter()
        .map(|exchange| exchange.exchange_code.as_str())
        .collect();

    if exchange_codes.is_empty() {
        return Err("calendar regression fixture must include at least one exchange".to_string());
    }

    if fixture_document.cases.is_empty() {
        return Err("calendar regression fixture must include at least one case".to_string());
    }

    let mut observed_fixture_case_ids: BTreeSet<&str> = BTreeSet::new();
    let mut observed_exchange_timestamp_pairs: BTreeSet<(String, String)> = BTreeSet::new();
    let mut observed_tags: BTreeSet<String> = BTreeSet::new();

    for fixture_case in &fixture_document.cases {
        require_non_empty(&fixture_case.fixture_id, "cases.fixture_id")?;
        require_non_empty(&fixture_case.exchange_code, "cases.exchange_code")?;
        require_non_empty(&fixture_case.timestamp_utc, "cases.timestamp_utc")?;
        require_non_empty(&fixture_case.description, "cases.description")?;

        if !observed_fixture_case_ids.insert(&fixture_case.fixture_id) {
            return Err(format!(
                "duplicate fixture case id in calendar regression fixture: {}",
                fixture_case.fixture_id
            ));
        }

        if !exchange_codes.contains(fixture_case.exchange_code.as_str()) {
            return Err(format!(
                "fixture case references unknown exchange code: {}",
                fixture_case.exchange_code
            ));
        }

        let exchange_timestamp_pair = (
            fixture_case.exchange_code.clone(),
            fixture_case.timestamp_utc.clone(),
        );
        if !observed_exchange_timestamp_pairs.insert(exchange_timestamp_pair.clone()) {
            return Err(format!(
                "duplicate fixture case exchange_code/timestamp_utc pair: {} at {}",
                exchange_timestamp_pair.0, exchange_timestamp_pair.1
            ));
        }

        if fixture_case.tags.is_empty() {
            return Err(format!(
                "fixture case {} must include at least one tag",
                fixture_case.fixture_id
            ));
        }

        for case_tag in &fixture_case.tags {
            require_non_empty(case_tag, "cases.tags")?;
            observed_tags.insert(case_tag.clone());
        }

        match fixture_case.expected_state.as_str() {
            "open" | "closed" => {}
            _ => {
                return Err(format!(
                    "fixture case {} has unsupported expected_state: {}",
                    fixture_case.fixture_id, fixture_case.expected_state
                ));
            }
        }
    }

    validate_calendar_canonical_regression_metadata(fixture_document, &observed_tags)?;

    ExchangeCalendarRegistry::from_artifact(fixture_document.calendar_registry.clone()).map_err(
        |source| {
            format!("calendar regression fixture includes an invalid calendar registry: {source}")
        },
    )?;

    Ok(())
}

pub fn validate_hourly_factor_regression_fixture_document(
    fixture_document: &HourlyFactorRegressionFixtureDocument,
) -> Result<(), String> {
    if fixture_document.schema_version != HOURLY_FACTOR_REGRESSION_FIXTURE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported hourly factor regression fixture schema version: {}",
            fixture_document.schema_version
        ));
    }

    if fixture_document.fixture_set_id != HOURLY_FACTOR_REGRESSION_FIXTURE_SET_ID {
        return Err(format!(
            "unexpected hourly factor regression fixture set id: {}",
            fixture_document.fixture_set_id
        ));
    }

    if fixture_document.calendar_fixture_set_id != CALENDAR_REGRESSION_FIXTURE_SET_ID {
        return Err(format!(
            "unexpected hourly factor regression calendar_fixture_set_id: {}",
            fixture_document.calendar_fixture_set_id
        ));
    }

    validate_threshold_configuration(&fixture_document.threshold_configuration)?;

    if fixture_document.canonical_output_cases.is_empty() {
        return Err(
            "hourly factor regression fixture must include canonical_output_cases".to_string(),
        );
    }

    if fixture_document.missing_data_guard_cases.is_empty() {
        return Err(
            "hourly factor regression fixture must include missing_data_guard_cases".to_string(),
        );
    }

    let mut observed_canonical_fixture_ids: BTreeSet<String> = BTreeSet::new();
    let mut observed_missing_fixture_ids: BTreeSet<String> = BTreeSet::new();
    let mut observed_tags: BTreeSet<String> = BTreeSet::new();

    for fixture_case in &fixture_document.canonical_output_cases {
        validate_hourly_canonical_output_case(fixture_case)?;

        if !observed_canonical_fixture_ids.insert(fixture_case.fixture_id.clone()) {
            return Err(format!(
                "duplicate canonical_output_cases fixture_id: {}",
                fixture_case.fixture_id
            ));
        }

        for case_tag in &fixture_case.tags {
            observed_tags.insert(case_tag.clone());
        }
    }

    for fixture_case in &fixture_document.missing_data_guard_cases {
        validate_hourly_missing_data_guard_case(fixture_case)?;

        if !observed_missing_fixture_ids.insert(fixture_case.fixture_id.clone()) {
            return Err(format!(
                "duplicate missing_data_guard_cases fixture_id: {}",
                fixture_case.fixture_id
            ));
        }

        for case_tag in &fixture_case.tags {
            observed_tags.insert(case_tag.clone());
        }
    }

    let required_tags = &fixture_document.canonical_regression_metadata.required_tags;
    if required_tags.is_empty() {
        return Err(
            "hourly canonical_regression_metadata.required_tags must not be empty".to_string(),
        );
    }

    for required_tag in required_tags {
        require_non_empty(required_tag, "canonical_regression_metadata.required_tags")?;
    }

    let expected_canonical_fixture_ids: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .stable_canonical_output_fixture_ids
        .iter()
        .map(|fixture_id| fixture_id.to_string())
        .collect();
    if expected_canonical_fixture_ids.is_empty() {
        return Err(
            "hourly canonical_regression_metadata.stable_canonical_output_fixture_ids must not be empty"
                .to_string(),
        );
    }

    let expected_missing_fixture_ids: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .stable_missing_data_guard_fixture_ids
        .iter()
        .map(|fixture_id| fixture_id.to_string())
        .collect();
    if expected_missing_fixture_ids.is_empty() {
        return Err(
            "hourly canonical_regression_metadata.stable_missing_data_guard_fixture_ids must not be empty"
                .to_string(),
        );
    }

    if !required_tags
        .iter()
        .all(|required_tag| observed_tags.contains(required_tag))
    {
        return Err(
            "hourly canonical_regression_metadata.required_tags must be present in fixture cases"
                .to_string(),
        );
    }

    if observed_canonical_fixture_ids != expected_canonical_fixture_ids {
        return Err(
            "hourly canonical_regression_metadata.stable_canonical_output_fixture_ids does not match observed fixture ids"
                .to_string(),
        );
    }

    if observed_missing_fixture_ids != expected_missing_fixture_ids {
        return Err(
            "hourly canonical_regression_metadata.stable_missing_data_guard_fixture_ids does not match observed fixture ids"
                .to_string(),
        );
    }

    Ok(())
}

fn validate_threshold_configuration(
    threshold_configuration: &HourlyFactorRegressionThresholdConfiguration,
) -> Result<(), String> {
    if threshold_configuration.schema_version
        != HOURLY_FACTOR_THRESHOLD_CONFIGURATION_SCHEMA_VERSION
    {
        return Err(format!(
            "unsupported threshold configuration schema version: {}",
            threshold_configuration.schema_version
        ));
    }

    require_non_empty(
        &threshold_configuration.policy_version,
        "threshold_configuration.policy_version",
    )?;

    if threshold_configuration.state_classification_strictness
        != STATE_CLASSIFICATION_STRICTNESS_POLICY
    {
        return Err(format!(
            "unsupported state classification strictness policy: {}",
            threshold_configuration.state_classification_strictness
        ));
    }

    validate_finite_non_negative(
        threshold_configuration
            .tolerance_config
            .s_h_absolute_tolerance,
        "threshold_configuration.tolerance_config.s_h_absolute_tolerance",
    )?;
    validate_finite_non_negative(
        threshold_configuration
            .tolerance_config
            .nu_h_absolute_tolerance,
        "threshold_configuration.tolerance_config.nu_h_absolute_tolerance",
    )?;

    if threshold_configuration
        .factor_range_by_fixture_class
        .is_empty()
    {
        return Err(
            "threshold_configuration.factor_range_by_fixture_class must not be empty".to_string(),
        );
    }
    for (fixture_class, factor_range_policy) in
        &threshold_configuration.factor_range_by_fixture_class
    {
        require_non_empty(
            fixture_class,
            "threshold_configuration.factor_range_by_fixture_class.key",
        )?;

        validate_finite_non_negative(
            factor_range_policy.s_h_min,
            "threshold_configuration.factor_range_by_fixture_class.s_h_min",
        )?;
        validate_finite_non_negative(
            factor_range_policy.s_h_max,
            "threshold_configuration.factor_range_by_fixture_class.s_h_max",
        )?;
        validate_finite_non_negative(
            factor_range_policy.nu_h_min,
            "threshold_configuration.factor_range_by_fixture_class.nu_h_min",
        )?;
        validate_finite_non_negative(
            factor_range_policy.nu_h_max,
            "threshold_configuration.factor_range_by_fixture_class.nu_h_max",
        )?;

        if factor_range_policy.s_h_min > factor_range_policy.s_h_max {
            return Err(format!(
                "threshold_configuration.factor_range_by_fixture_class.{fixture_class} has s_h_min greater than s_h_max"
            ));
        }

        if factor_range_policy.nu_h_min > factor_range_policy.nu_h_max {
            return Err(format!(
                "threshold_configuration.factor_range_by_fixture_class.{fixture_class} has nu_h_min greater than nu_h_max"
            ));
        }
    }

    if threshold_configuration.change_log.is_empty() {
        return Err("threshold_configuration.change_log must not be empty".to_string());
    }

    let mut observed_policy_versions: BTreeSet<String> = BTreeSet::new();
    let mut has_current_policy_version_entry = false;

    for change_log_entry in &threshold_configuration.change_log {
        require_non_empty(
            &change_log_entry.policy_version,
            "threshold_configuration.change_log.policy_version",
        )?;
        require_non_empty(
            &change_log_entry.recorded_at_utc,
            "threshold_configuration.change_log.recorded_at_utc",
        )?;
        require_non_empty(
            &change_log_entry.rationale,
            "threshold_configuration.change_log.rationale",
        )?;

        if !observed_policy_versions.insert(change_log_entry.policy_version.clone()) {
            return Err(format!(
                "threshold_configuration.change_log contains duplicate policy_version: {}",
                change_log_entry.policy_version
            ));
        }

        if change_log_entry.policy_version == threshold_configuration.policy_version {
            has_current_policy_version_entry = true;
        }
    }

    if !has_current_policy_version_entry {
        return Err(format!(
            "threshold_configuration.change_log must include an entry for policy_version '{}'",
            threshold_configuration.policy_version
        ));
    }

    Ok(())
}

pub fn compute_content_hash_hex(payload_bytes: &[u8]) -> String {
    let mut hash_state: u64 = 0xcbf29ce484222325;

    for payload_byte in payload_bytes {
        hash_state ^= u64::from(*payload_byte);
        hash_state = hash_state.wrapping_mul(0x100000001b3);
    }

    format!("{hash_state:016x}")
}

pub fn fixtures_root_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .to_path_buf()
}

fn validation_fixture_manifest_path() -> PathBuf {
    fixtures_root_path()
        .join("validation")
        .join("validation_fixture_manifest.json")
}

fn find_manifest_entry_by_fixture_id(
    manifest: &ValidationFixtureManifestDocument,
    fixture_id: &str,
) -> Result<ValidationFixtureManifestEntry, String> {
    let matching_entries: Vec<ValidationFixtureManifestEntry> = manifest
        .fixtures
        .iter()
        .filter(|manifest_entry| manifest_entry.fixture_id == fixture_id)
        .cloned()
        .collect();

    if matching_entries.len() == 1 {
        return Ok(matching_entries
            .into_iter()
            .next()
            .expect("single manifest entry"));
    }

    Err(format!(
        "expected exactly one manifest entry with fixture_id `{fixture_id}`, found {}",
        matching_entries.len()
    ))
}

fn validate_manifest_entry(manifest_entry: &ValidationFixtureManifestEntry) -> Result<(), String> {
    require_non_empty(&manifest_entry.fixture_id, "fixtures.fixture_id")?;
    require_non_empty(
        &manifest_entry.fixture_schema_version,
        "fixtures.fixture_schema_version",
    )?;
    require_non_empty(&manifest_entry.relative_path, "fixtures.relative_path")?;

    if manifest_entry.exchange_codes.is_empty() {
        return Err(format!(
            "manifest fixture entry {} must include at least one exchange code",
            manifest_entry.fixture_id
        ));
    }

    for exchange_code in &manifest_entry.exchange_codes {
        require_non_empty(exchange_code, "fixtures.exchange_codes")?;
    }

    require_non_empty(&manifest_entry.content_hash, "fixtures.content_hash")?;

    if !is_lowercase_fnv1a_hex(&manifest_entry.content_hash) {
        return Err(format!(
            "manifest fixture entry {} has invalid content_hash: {}",
            manifest_entry.fixture_id, manifest_entry.content_hash
        ));
    }

    Ok(())
}

fn validate_calendar_canonical_regression_metadata(
    fixture_document: &CalendarRegressionFixtureDocument,
    observed_tags: &BTreeSet<String>,
) -> Result<(), String> {
    let canonical_metadata = &fixture_document.canonical_regression_metadata;
    if canonical_metadata.required_tags.is_empty() {
        return Err(
            "calendar canonical_regression_metadata.required_tags must not be empty".to_string(),
        );
    }

    for required_tag in &canonical_metadata.required_tags {
        require_non_empty(required_tag, "canonical_regression_metadata.required_tags")?;
        if !observed_tags.contains(required_tag) {
            return Err(format!(
                "calendar canonical_regression_metadata.required_tags contains `{required_tag}` which is missing from fixture cases"
            ));
        }
    }

    if canonical_metadata.stable_fixture_ids_by_tag.is_empty() {
        return Err(
            "calendar canonical_regression_metadata.stable_fixture_ids_by_tag must not be empty"
                .to_string(),
        );
    }

    for (tag, expected_fixture_ids) in &canonical_metadata.stable_fixture_ids_by_tag {
        require_non_empty(
            tag,
            "canonical_regression_metadata.stable_fixture_ids_by_tag.key",
        )?;

        if expected_fixture_ids.is_empty() {
            return Err(format!(
                "calendar canonical_regression_metadata.stable_fixture_ids_by_tag[{tag}] must not be empty"
            ));
        }

        let expected_fixture_id_set: BTreeSet<String> = expected_fixture_ids
            .iter()
            .map(|fixture_id| {
                require_non_empty(
                    fixture_id,
                    "canonical_regression_metadata.stable_fixture_ids_by_tag.value",
                )?;
                Ok(fixture_id.to_string())
            })
            .collect::<Result<BTreeSet<String>, String>>()?;
        let observed_fixture_id_set: BTreeSet<String> = fixture_document
            .cases
            .iter()
            .filter(|fixture_case| fixture_case.tags.iter().any(|case_tag| case_tag == tag))
            .map(|fixture_case| fixture_case.fixture_id.to_string())
            .collect();

        if observed_fixture_id_set != expected_fixture_id_set {
            return Err(format!(
                "calendar canonical_regression_metadata.stable_fixture_ids_by_tag does not match observed fixture ids for tag '{tag}'"
            ));
        }
    }

    if canonical_metadata
        .historical_holiday_expected_years
        .is_empty()
    {
        return Err(
            "calendar canonical_regression_metadata.historical_holiday_expected_years must not be empty"
                .to_string(),
        );
    }

    if canonical_metadata
        .historical_holiday_expected_intraday_tags
        .is_empty()
    {
        return Err(
            "calendar canonical_regression_metadata.historical_holiday_expected_intraday_tags must not be empty"
                .to_string(),
        );
    }

    for case_tag in &canonical_metadata.historical_holiday_expected_intraday_tags {
        require_non_empty(
            case_tag,
            "canonical_regression_metadata.historical_holiday_expected_intraday_tags",
        )?;
    }

    Ok(())
}

fn validate_hourly_canonical_output_case(
    fixture_case: &HourlyFactorCanonicalOutputCaseFixture,
) -> Result<(), String> {
    require_non_empty(
        &fixture_case.fixture_id,
        "canonical_output_cases.fixture_id",
    )?;
    require_non_empty(
        &fixture_case.exchange_code,
        "canonical_output_cases.exchange_code",
    )?;
    require_non_empty(
        &fixture_case.range_start_utc,
        "canonical_output_cases.range_start_utc",
    )?;
    require_non_empty(
        &fixture_case.range_end_utc,
        "canonical_output_cases.range_end_utc",
    )?;
    require_non_empty(
        &fixture_case.description,
        "canonical_output_cases.description",
    )?;

    if fixture_case.tags.is_empty() {
        return Err(format!(
            "hourly canonical_output_case {} must include at least one tag",
            fixture_case.fixture_id
        ));
    }

    for case_tag in &fixture_case.tags {
        require_non_empty(case_tag, "canonical_output_cases.tags")?;
    }

    if fixture_case.expected_s_h.is_empty() {
        return Err(format!(
            "hourly canonical_output_case {} expected_s_h must not be empty",
            fixture_case.fixture_id
        ));
    }

    if fixture_case.expected_nu_h.is_empty() {
        return Err(format!(
            "hourly canonical_output_case {} expected_nu_h must not be empty",
            fixture_case.fixture_id
        ));
    }

    for value in &fixture_case.expected_s_h {
        validate_finite_non_negative(*value, "canonical_output_cases.expected_s_h")?;
    }
    for value in &fixture_case.expected_nu_h {
        validate_finite_non_negative(*value, "canonical_output_cases.expected_nu_h")?;
    }

    match &fixture_case.seasonality_source {
        HourlySeasonalitySourceFixture::Explicit {
            hourly_factors,
            materialization_method,
        } => {
            if hourly_factors.is_empty() {
                return Err(format!(
                    "hourly canonical_output_case {} explicit seasonality hourly_factors must not be empty",
                    fixture_case.fixture_id
                ));
            }

            require_non_empty(
                materialization_method,
                "canonical_output_cases.seasonality_source.materialization_method",
            )?;
        }
        HourlySeasonalitySourceFixture::Materialized {
            source_timestamps_utc,
            source_seasonality_factors,
            interpolation_method,
            aggregation_method,
            sparse_fallback_policy,
            ..
        } => {
            if source_timestamps_utc.is_empty() {
                return Err(format!(
                    "hourly canonical_output_case {} materialized source_timestamps_utc must not be empty",
                    fixture_case.fixture_id
                ));
            }

            if source_seasonality_factors.is_empty() {
                return Err(format!(
                    "hourly canonical_output_case {} materialized source_seasonality_factors must not be empty",
                    fixture_case.fixture_id
                ));
            }

            for timestamp in source_timestamps_utc {
                require_non_empty(
                    timestamp,
                    "canonical_output_cases.seasonality_source.source_timestamps_utc",
                )?;
            }

            require_non_empty(
                interpolation_method,
                "canonical_output_cases.seasonality_source.interpolation_method",
            )?;
            require_non_empty(
                aggregation_method,
                "canonical_output_cases.seasonality_source.aggregation_method",
            )?;
            require_non_empty(
                sparse_fallback_policy,
                "canonical_output_cases.seasonality_source.sparse_fallback_policy",
            )?;
        }
    }

    match &fixture_case.news_intensity_source {
        HourlyNewsIntensitySourceFixture::Explicit {
            hourly_factors,
            materialization_method,
        } => {
            if hourly_factors.is_empty() {
                return Err(format!(
                    "hourly canonical_output_case {} explicit news_intensity hourly_factors must not be empty",
                    fixture_case.fixture_id
                ));
            }

            require_non_empty(
                materialization_method,
                "canonical_output_cases.news_intensity_source.materialization_method",
            )?;
        }
        HourlyNewsIntensitySourceFixture::Materialized {
            closed_window_baseline_intensity,
            open_window_override_by_interval_index,
        } => {
            validate_finite_non_negative(
                *closed_window_baseline_intensity,
                "canonical_output_cases.news_intensity_source.closed_window_baseline_intensity",
            )?;

            for raw_interval_index in open_window_override_by_interval_index.keys() {
                require_non_empty(
                    raw_interval_index,
                    "canonical_output_cases.news_intensity_source.open_window_override_by_interval_index",
                )?;

                if raw_interval_index.parse::<u64>().is_err() {
                    return Err(format!(
                        "open_window_override_by_interval_index key `{raw_interval_index}` must parse as u64"
                    ));
                }
            }
        }
    }

    Ok(())
}

fn validate_hourly_missing_data_guard_case(
    fixture_case: &HourlyFactorMissingDataGuardCaseFixture,
) -> Result<(), String> {
    require_non_empty(
        &fixture_case.fixture_id,
        "missing_data_guard_cases.fixture_id",
    )?;
    require_non_empty(
        &fixture_case.exchange_code,
        "missing_data_guard_cases.exchange_code",
    )?;
    require_non_empty(
        &fixture_case.range_start_utc,
        "missing_data_guard_cases.range_start_utc",
    )?;
    require_non_empty(
        &fixture_case.range_end_utc,
        "missing_data_guard_cases.range_end_utc",
    )?;
    require_non_empty(
        &fixture_case.description,
        "missing_data_guard_cases.description",
    )?;

    if fixture_case.tags.is_empty() {
        return Err(format!(
            "hourly missing_data_guard_case {} must include at least one tag",
            fixture_case.fixture_id
        ));
    }

    for case_tag in &fixture_case.tags {
        require_non_empty(case_tag, "missing_data_guard_cases.tags")?;
    }
    for timestamp in &fixture_case.source_timestamps_utc {
        require_non_empty(timestamp, "missing_data_guard_cases.source_timestamps_utc")?;
    }

    require_non_empty(
        &fixture_case.settings.interpolation_method,
        "missing_data_guard_cases.settings.interpolation_method",
    )?;
    require_non_empty(
        &fixture_case.settings.aggregation_method,
        "missing_data_guard_cases.settings.aggregation_method",
    )?;
    require_non_empty(
        &fixture_case.settings.sparse_fallback_policy,
        "missing_data_guard_cases.settings.sparse_fallback_policy",
    )?;

    match &fixture_case.expected_outcome {
        HourlyMissingDataGuardExpectedOutcomeFixture::Success { expected_s_h, .. } => {
            if expected_s_h.is_empty() {
                return Err(format!(
                    "hourly missing_data_guard_case {} success expected_s_h must not be empty",
                    fixture_case.fixture_id
                ));
            }
        }
        HourlyMissingDataGuardExpectedOutcomeFixture::Failure {
            expected_error_variant,
            expected_error_message_substring,
        } => {
            require_non_empty(
                expected_error_variant,
                "missing_data_guard_cases.expected_outcome.expected_error_variant",
            )?;
            require_non_empty(
                expected_error_message_substring,
                "missing_data_guard_cases.expected_outcome.expected_error_message_substring",
            )?;
        }
    }

    Ok(())
}

fn validate_finite_non_negative(value: f64, field_name: &str) -> Result<(), String> {
    if !value.is_finite() {
        return Err(format!("field `{field_name}` must be finite"));
    }
    if value < 0.0 {
        return Err(format!("field `{field_name}` must be non-negative"));
    }

    Ok(())
}

fn is_lowercase_fnv1a_hex(raw_hash: &str) -> bool {
    raw_hash.len() == 16
        && raw_hash
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
}

fn require_non_empty(value: &str, field_name: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("field `{field_name}` must be non-empty"));
    }

    Ok(())
}
