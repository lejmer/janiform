use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use janiform_core::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact};
use serde::Deserialize;

pub const VALIDATION_FIXTURE_MANIFEST_SCHEMA_VERSION: &str = "validation.fixture.manifest.v1";
pub const CALENDAR_REGRESSION_FIXTURE_SCHEMA_VERSION: &str = "calendar.service.regression.v1";
pub const CALENDAR_REGRESSION_FIXTURE_SET_ID: &str = "calendar_service_regression_matrix_v1";
pub const MANIFEST_SCHEMA_FAMILY: &str = "validation.fixture.manifest";
pub const MANIFEST_V0_UPDATE_POLICY: &str = "additive_only_and_major_on_breaking_change.v1";
pub const MANIFEST_LOADING_ORDER_POLICY: &str = "lexicographic_by_fixture_id.v1";
pub const CONTENT_HASH_ALGORITHM_POLICY: &str = "fnv1a_64_hex_utf8.v1";
pub const CALENDAR_SCHEMA_FAMILY: &str = "validation.calendar.service.regression";
pub const CALENDAR_SCHEMA_GENERATION: &str = "v0.x";
pub const CALENDAR_FORWARD_EXTENSION_POLICY: &str = "additive_fields_only.v1";
pub const CALENDAR_CASE_ORDER_POLICY: &str = "preserve_declared_case_order.v1";
pub const CALENDAR_CONTENT_HASH_SCOPE_POLICY: &str = "document_payload_bytes.v1";

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
pub struct CalendarRegressionCaseFixture {
    pub fixture_id: String,
    pub exchange_code: String,
    pub timestamp_utc: String,
    pub expected_state: String,
    pub tags: Vec<String>,
    pub description: String,
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

pub fn parse_calendar_regression_fixture_document(
    payload_bytes: &[u8],
) -> Result<CalendarRegressionFixtureDocument, String> {
    serde_json::from_slice::<CalendarRegressionFixtureDocument>(payload_bytes)
        .map_err(|source| format!("failed to parse calendar regression fixture document: {source}"))
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

    ExchangeCalendarRegistry::from_artifact(fixture_document.calendar_registry.clone()).map_err(
        |source| {
            format!("calendar regression fixture includes an invalid calendar registry: {source}")
        },
    )?;

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
