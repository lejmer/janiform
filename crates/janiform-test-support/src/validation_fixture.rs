#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use janiform_market_time::calendar::{
    ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact, MarketState,
};
use serde::Deserialize;

pub const VALIDATION_FIXTURE_MANIFEST_SCHEMA_VERSION: &str = "validation.fixture.manifest.v1";
pub const CALENDAR_REGRESSION_FIXTURE_SCHEMA_VERSION: &str = "calendar.service.regression.v1";
pub const CALENDAR_REGRESSION_FIXTURE_SET_ID: &str = "calendar_service_regression_matrix_v1";
pub const HOURLY_FACTOR_REGRESSION_FIXTURE_SCHEMA_VERSION: &str = "hourly.factor.regression.v1";
pub const HOURLY_FACTOR_REGRESSION_FIXTURE_SET_ID: &str = "hourly_factor_regression_matrix_v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_SCENARIO_FIXTURE_SCHEMA_VERSION: &str =
    "gap.behavior.open_closed.scenario_fixture.v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_SCENARIO_FIXTURE_SET_ID: &str =
    "open_closed_gap_behavior_scenarios_v1";
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
pub const OPEN_CLOSED_GAP_BEHAVIOR_SCHEMA_FAMILY: &str = "validation.gap_behavior.open_closed";
pub const OPEN_CLOSED_GAP_BEHAVIOR_SCHEMA_GENERATION: &str = "v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_FORWARD_EXTENSION_POLICY: &str = "additive_fields_only.v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_CASE_ORDER_POLICY: &str = "preserve_declared_case_order.v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_EVENT_ORDER_POLICY: &str = "preserve_declared_event_order.v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_CONTENT_HASH_SCOPE_POLICY: &str = "document_payload_bytes.v1";
pub const OPEN_CLOSED_GAP_BEHAVIOR_OUTPUT_SCHEMA_VERSION: &str =
    "validation.gap_behavior.open_closed.case_output.v1";
pub const STYLIZED_FACT_REGRESSION_FIXTURE_DOMAIN: &str = "stylized_facts";

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
    GapBehavior,
    ScaleConsistency,
    StylizedFacts,
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
        known_scheduled_event_timestamps_utc: Vec<String>,
        surprise_baseline_intensity: f64,
        surprise_deterministic_time_multiplier_by_interval_index: BTreeMap<String, f64>,
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorScenarioFixtureDocument {
    pub schema_version: String,
    pub fixture_set_id: String,
    pub fixture_set_version: String,
    pub schema_metadata: OpenClosedGapBehaviorScenarioSchemaMetadata,
    pub determinism_metadata: OpenClosedGapBehaviorScenarioDeterminismMetadata,
    pub output_schema_metadata: OpenClosedGapBehaviorOutputSchemaMetadata,
    pub canonical_regression_metadata: OpenClosedGapBehaviorCanonicalMetadata,
    pub cases: Vec<OpenClosedGapBehaviorScenarioCaseFixture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorScenarioSchemaMetadata {
    pub schema_family: String,
    pub schema_generation: String,
    pub forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorScenarioDeterminismMetadata {
    pub fixture_loading_order: String,
    pub event_loading_order: String,
    pub content_hash_scope: String,
    pub content_hash_algorithm: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorOutputSchemaMetadata {
    pub schema_version: String,
    pub required_case_output_fields: Vec<String>,
    pub required_summary_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorCanonicalMetadata {
    pub required_tags: Vec<String>,
    pub required_scenario_classes: Vec<String>,
    pub stable_fixture_ids_by_tag: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorScenarioCaseFixture {
    pub fixture_id: String,
    pub exchange_code: String,
    pub scenario_class: String,
    pub deterministic_replay_metadata: OpenClosedGapBehaviorDeterministicReplayMetadataFixture,
    pub issue_traceability: OpenClosedGapBehaviorIssueTraceabilityFixture,
    pub calendar_fixture_set_id: String,
    pub calendar_state_fixture_references: Vec<String>,
    pub range_start_utc: String,
    pub range_end_utc: String,
    pub initial_accumulator_state: OpenClosedGapBehaviorAccumulatorStateFixture,
    pub injected_events: Vec<OpenClosedGapBehaviorInjectedEventFixture>,
    pub expected_outcome: OpenClosedGapBehaviorExpectedOutcomeFixture,
    pub tags: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorAccumulatorStateFixture {
    pub j_t: f64,
    pub m_t: f64,
    pub g_t: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorInjectedEventFixture {
    pub event_id: String,
    pub timestamp_utc: String,
    pub news_arrival_component: String,
    pub shared_severity_mark: f64,
    pub coupled_return_jump_mark: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorDeterministicReplayMetadataFixture {
    pub replay_baseline_id: String,
    pub event_calendar_binding_id: String,
    pub jump_observation_binding_id: String,
    pub gap_observation_binding_id: String,
    pub candidate_selection_policy_id: String,
    pub candidate_selection_rank: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorIssueTraceabilityFixture {
    pub issue_id: String,
    pub semantic_focus_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorExpectedOutcomeFixture {
    pub event_outcomes: Vec<OpenClosedGapBehaviorEventExpectationFixture>,
    pub summary: OpenClosedGapBehaviorExpectedSummaryFixture,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorEventExpectationFixture {
    pub event_id: String,
    pub expected_market_state: String,
    pub expected_activity_coupling_j_t_increment: f64,
    pub expected_open_jump_m_t_increment: f64,
    pub expected_closed_gap_g_t_increment: f64,
    pub expected_trading_return_increment: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClosedGapBehaviorExpectedSummaryFixture {
    pub expected_open_event_count: usize,
    pub expected_closed_event_count: usize,
    pub expected_total_activity_coupling_j_t_increment: f64,
    pub expected_total_open_jump_m_t_increment: f64,
    pub expected_total_closed_gap_g_t_increment: f64,
    pub expected_total_trading_return_increment: f64,
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

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedOpenClosedGapBehaviorScenarioFixture {
    pub manifest: ValidationFixtureManifestDocument,
    pub manifest_entry: ValidationFixtureManifestEntry,
    pub loaded_payload: LoadedFixturePayload,
    pub fixture_document: OpenClosedGapBehaviorScenarioFixtureDocument,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenClosedGapBehaviorScenarioValidationError {
    pub code: &'static str,
    pub field_path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenClosedGapBehaviorScenarioLoaderError {
    Manifest(String),
    InvalidManifestEntry {
        expected_fixture_schema_version: String,
        observed_fixture_schema_version: String,
    },
    PayloadLoad(String),
    Parse(String),
    Validation {
        errors: Vec<OpenClosedGapBehaviorScenarioValidationError>,
    },
}

impl Display for OpenClosedGapBehaviorScenarioLoaderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest(message) => write!(
                formatter,
                "failed to load open/closed gap behavior scenario fixture manifest metadata: {message}"
            ),
            Self::InvalidManifestEntry {
                expected_fixture_schema_version,
                observed_fixture_schema_version,
            } => write!(
                formatter,
                "unexpected open/closed gap behavior fixture schema version in manifest: expected {}, observed {}",
                expected_fixture_schema_version, observed_fixture_schema_version
            ),
            Self::PayloadLoad(message) => write!(
                formatter,
                "failed to load open/closed gap behavior scenario fixture payload: {message}"
            ),
            Self::Parse(message) => write!(
                formatter,
                "failed to parse open/closed gap behavior scenario fixture payload: {message}"
            ),
            Self::Validation { errors } => write!(
                formatter,
                "open/closed gap behavior scenario fixture payload failed validation with {} error(s)",
                errors.len()
            ),
        }
    }
}

impl Error for OpenClosedGapBehaviorScenarioLoaderError {}

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

pub fn load_open_closed_gap_behavior_scenario_fixture_from_manifest()
-> Result<LoadedOpenClosedGapBehaviorScenarioFixture, OpenClosedGapBehaviorScenarioLoaderError> {
    let manifest = load_validation_fixture_manifest_document()
        .map_err(OpenClosedGapBehaviorScenarioLoaderError::Manifest)?;
    let manifest_entry = find_manifest_entry_by_fixture_id(
        &manifest,
        OPEN_CLOSED_GAP_BEHAVIOR_SCENARIO_FIXTURE_SET_ID,
    )
    .map_err(OpenClosedGapBehaviorScenarioLoaderError::Manifest)?;

    if manifest_entry.fixture_schema_version
        != OPEN_CLOSED_GAP_BEHAVIOR_SCENARIO_FIXTURE_SCHEMA_VERSION
    {
        return Err(
            OpenClosedGapBehaviorScenarioLoaderError::InvalidManifestEntry {
                expected_fixture_schema_version:
                    OPEN_CLOSED_GAP_BEHAVIOR_SCENARIO_FIXTURE_SCHEMA_VERSION.to_string(),
                observed_fixture_schema_version: manifest_entry.fixture_schema_version.clone(),
            },
        );
    }

    let loaded_payload = load_manifest_entry_payload(&fixtures_root_path(), &manifest_entry)
        .map_err(OpenClosedGapBehaviorScenarioLoaderError::PayloadLoad)?;
    let fixture_document =
        parse_open_closed_gap_behavior_scenario_fixture_document(&loaded_payload.payload_bytes)?;

    validate_open_closed_gap_behavior_scenario_fixture_document(&fixture_document)
        .map_err(|errors| OpenClosedGapBehaviorScenarioLoaderError::Validation { errors })?;

    Ok(LoadedOpenClosedGapBehaviorScenarioFixture {
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

pub fn parse_open_closed_gap_behavior_scenario_fixture_document(
    payload_bytes: &[u8],
) -> Result<OpenClosedGapBehaviorScenarioFixtureDocument, OpenClosedGapBehaviorScenarioLoaderError>
{
    serde_json::from_slice::<OpenClosedGapBehaviorScenarioFixtureDocument>(payload_bytes)
        .map_err(|source| OpenClosedGapBehaviorScenarioLoaderError::Parse(source.to_string()))
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

pub fn validate_open_closed_gap_behavior_scenario_fixture_document(
    fixture_document: &OpenClosedGapBehaviorScenarioFixtureDocument,
) -> Result<(), Vec<OpenClosedGapBehaviorScenarioValidationError>> {
    let mut errors: Vec<OpenClosedGapBehaviorScenarioValidationError> = Vec::new();

    validate_open_closed_gap_behavior_document_headers(fixture_document, &mut errors);
    validate_open_closed_gap_behavior_output_schema_metadata(fixture_document, &mut errors);

    let required_tag_set = validate_open_closed_gap_behavior_canonical_metadata_declarations(
        fixture_document,
        &mut errors,
    );

    let observed_coverage =
        validate_open_closed_gap_behavior_cases_and_collect_coverage(fixture_document, &mut errors);

    validate_open_closed_gap_behavior_canonical_coverage(
        fixture_document,
        &required_tag_set,
        &observed_coverage,
        &mut errors,
    );

    if errors.is_empty() {
        return Ok(());
    }

    Err(errors)
}

pub fn validate_open_closed_gap_behavior_calendar_integration(
    open_closed_fixture_document: &OpenClosedGapBehaviorScenarioFixtureDocument,
    calendar_fixture_document: &CalendarRegressionFixtureDocument,
) -> Result<(), Vec<OpenClosedGapBehaviorScenarioValidationError>> {
    let mut errors: Vec<OpenClosedGapBehaviorScenarioValidationError> = Vec::new();
    let calendar_case_ids: BTreeSet<&str> = calendar_fixture_document
        .cases
        .iter()
        .map(|fixture_case| fixture_case.fixture_id.as_str())
        .collect();

    let calendar_registry = match ExchangeCalendarRegistry::from_artifact(
        calendar_fixture_document.calendar_registry.clone(),
    ) {
        Ok(registry) => registry,
        Err(source) => {
            add_open_closed_gap_behavior_validation_error(
                &mut errors,
                "calendar_registry_invalid",
                "calendar_registry",
                format!("calendar fixture registry failed to load: {source}"),
            );
            return Err(errors);
        }
    };

    let mut observed_window_tag_set: BTreeSet<&str> = BTreeSet::new();
    for (case_index, fixture_case) in open_closed_fixture_document.cases.iter().enumerate() {
        let case_field_path_prefix = format!("cases[{case_index}]");

        if fixture_case.calendar_fixture_set_id != calendar_fixture_document.fixture_set_id {
            add_open_closed_gap_behavior_validation_error(
                &mut errors,
                "calendar_fixture_set_id_integration_mismatch",
                format!("{case_field_path_prefix}.calendar_fixture_set_id"),
                format!(
                    "scenario references `{}` but loaded calendar fixture id is `{}`",
                    fixture_case.calendar_fixture_set_id, calendar_fixture_document.fixture_set_id
                ),
            );
        }

        for case_tag in &fixture_case.tags {
            if case_tag == "intraday"
                || case_tag == "overnight"
                || case_tag == "weekend"
                || case_tag == "boundary"
            {
                observed_window_tag_set.insert(case_tag);
            }
        }

        for (reference_index, referenced_fixture_id) in fixture_case
            .calendar_state_fixture_references
            .iter()
            .enumerate()
        {
            if !calendar_case_ids.contains(referenced_fixture_id.as_str()) {
                add_open_closed_gap_behavior_validation_error(
                    &mut errors,
                    "calendar_fixture_reference_missing",
                    format!(
                        "{case_field_path_prefix}.calendar_state_fixture_references[{reference_index}]"
                    ),
                    format!("references missing calendar fixture id `{referenced_fixture_id}`"),
                );
            }
        }

        let expected_market_state_by_event_id: BTreeMap<&str, &str> = fixture_case
            .expected_outcome
            .event_outcomes
            .iter()
            .map(|event_expectation| {
                (
                    event_expectation.event_id.as_str(),
                    event_expectation.expected_market_state.as_str(),
                )
            })
            .collect();

        for (event_index, injected_event) in fixture_case.injected_events.iter().enumerate() {
            let expected_market_state = match expected_market_state_by_event_id
                .get(injected_event.event_id.as_str())
            {
                Some(expected_market_state) => *expected_market_state,
                None => {
                    add_open_closed_gap_behavior_validation_error(
                        &mut errors,
                        "event_id_mapping_missing_in_expected_outcome",
                        format!("{case_field_path_prefix}.injected_events[{event_index}].event_id"),
                        format!(
                            "event id `{}` is missing from expected_outcome.event_outcomes",
                            injected_event.event_id
                        ),
                    );
                    continue;
                }
            };

            let timestamp_utc = match DateTime::parse_from_rfc3339(&injected_event.timestamp_utc) {
                Ok(timestamp) => timestamp.with_timezone(&Utc),
                Err(source) => {
                    add_open_closed_gap_behavior_validation_error(
                        &mut errors,
                        "injected_event_timestamp_parse_failed",
                        format!(
                            "{case_field_path_prefix}.injected_events[{event_index}].timestamp_utc"
                        ),
                        format!("failed to parse RFC3339 timestamp: {source}"),
                    );
                    continue;
                }
            };

            let resolved_market_state = match calendar_registry
                .resolve_market_state_at_utc(&fixture_case.exchange_code, timestamp_utc)
            {
                Ok(resolved_market_state) => resolved_market_state,
                Err(source) => {
                    add_open_closed_gap_behavior_validation_error(
                        &mut errors,
                        "market_state_resolution_failed",
                        format!("{case_field_path_prefix}.injected_events[{event_index}]"),
                        format!(
                            "failed to resolve market state for exchange `{}` at `{}`: {source}",
                            fixture_case.exchange_code, injected_event.timestamp_utc
                        ),
                    );
                    continue;
                }
            };

            let expected_market_state = match expected_market_state {
                "open" => MarketState::Open,
                "closed" => MarketState::Closed,
                _ => continue,
            };

            if resolved_market_state != expected_market_state {
                add_open_closed_gap_behavior_validation_error(
                    &mut errors,
                    "market_state_classification_mismatch",
                    format!(
                        "{case_field_path_prefix}.expected_outcome.event_outcomes[{}].expected_market_state",
                        event_index
                    ),
                    format!(
                        "expected `{}`, resolved `{}` for event `{}`",
                        expected_market_state, resolved_market_state, injected_event.event_id
                    ),
                );
            }
        }
    }

    for required_window_tag in ["intraday", "overnight", "weekend", "boundary"] {
        if !observed_window_tag_set.contains(required_window_tag) {
            add_open_closed_gap_behavior_validation_error(
                &mut errors,
                "calendar_window_tag_missing",
                "cases.tags",
                format!(
                    "calendar integration requires `{required_window_tag}` window coverage in scenario tags"
                ),
            );
        }
    }

    if errors.is_empty() {
        return Ok(());
    }

    Err(errors)
}

#[derive(Debug, Default)]
struct OpenClosedGapBehaviorObservedCoverage {
    observed_tags: BTreeSet<String>,
    observed_case_ids_by_tag: BTreeMap<String, BTreeSet<String>>,
    known_only_case_count: usize,
    surprise_only_case_count: usize,
    mixed_news_case_count: usize,
}

fn validate_open_closed_gap_behavior_document_headers(
    fixture_document: &OpenClosedGapBehaviorScenarioFixtureDocument,
    errors: &mut Vec<OpenClosedGapBehaviorScenarioValidationError>,
) {
    if fixture_document.schema_version != OPEN_CLOSED_GAP_BEHAVIOR_SCENARIO_FIXTURE_SCHEMA_VERSION {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "schema_version_mismatch",
            "schema_version",
            format!(
                "expected `{}`, found `{}`",
                OPEN_CLOSED_GAP_BEHAVIOR_SCENARIO_FIXTURE_SCHEMA_VERSION,
                fixture_document.schema_version
            ),
        );
    }

    if fixture_document.fixture_set_id != OPEN_CLOSED_GAP_BEHAVIOR_SCENARIO_FIXTURE_SET_ID {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "fixture_set_id_mismatch",
            "fixture_set_id",
            format!(
                "expected `{}`, found `{}`",
                OPEN_CLOSED_GAP_BEHAVIOR_SCENARIO_FIXTURE_SET_ID, fixture_document.fixture_set_id
            ),
        );
    }

    validate_open_closed_gap_behavior_non_empty(
        &fixture_document.fixture_set_version,
        "fixture_set_version",
        errors,
    );

    if fixture_document.schema_metadata.schema_family != OPEN_CLOSED_GAP_BEHAVIOR_SCHEMA_FAMILY {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "schema_family_mismatch",
            "schema_metadata.schema_family",
            format!(
                "expected `{}`, found `{}`",
                OPEN_CLOSED_GAP_BEHAVIOR_SCHEMA_FAMILY,
                fixture_document.schema_metadata.schema_family
            ),
        );
    }

    if fixture_document.schema_metadata.schema_generation
        != OPEN_CLOSED_GAP_BEHAVIOR_SCHEMA_GENERATION
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "schema_generation_mismatch",
            "schema_metadata.schema_generation",
            format!(
                "expected `{}`, found `{}`",
                OPEN_CLOSED_GAP_BEHAVIOR_SCHEMA_GENERATION,
                fixture_document.schema_metadata.schema_generation
            ),
        );
    }

    if fixture_document.schema_metadata.forward_extension_policy
        != OPEN_CLOSED_GAP_BEHAVIOR_FORWARD_EXTENSION_POLICY
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "forward_extension_policy_mismatch",
            "schema_metadata.forward_extension_policy",
            format!(
                "expected `{}`, found `{}`",
                OPEN_CLOSED_GAP_BEHAVIOR_FORWARD_EXTENSION_POLICY,
                fixture_document.schema_metadata.forward_extension_policy
            ),
        );
    }

    if fixture_document.determinism_metadata.fixture_loading_order
        != OPEN_CLOSED_GAP_BEHAVIOR_CASE_ORDER_POLICY
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "fixture_loading_order_policy_mismatch",
            "determinism_metadata.fixture_loading_order",
            format!(
                "expected `{}`, found `{}`",
                OPEN_CLOSED_GAP_BEHAVIOR_CASE_ORDER_POLICY,
                fixture_document.determinism_metadata.fixture_loading_order
            ),
        );
    }

    if fixture_document.determinism_metadata.event_loading_order
        != OPEN_CLOSED_GAP_BEHAVIOR_EVENT_ORDER_POLICY
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "event_loading_order_policy_mismatch",
            "determinism_metadata.event_loading_order",
            format!(
                "expected `{}`, found `{}`",
                OPEN_CLOSED_GAP_BEHAVIOR_EVENT_ORDER_POLICY,
                fixture_document.determinism_metadata.event_loading_order
            ),
        );
    }

    if fixture_document.determinism_metadata.content_hash_scope
        != OPEN_CLOSED_GAP_BEHAVIOR_CONTENT_HASH_SCOPE_POLICY
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "content_hash_scope_policy_mismatch",
            "determinism_metadata.content_hash_scope",
            format!(
                "expected `{}`, found `{}`",
                OPEN_CLOSED_GAP_BEHAVIOR_CONTENT_HASH_SCOPE_POLICY,
                fixture_document.determinism_metadata.content_hash_scope
            ),
        );
    }

    if fixture_document.determinism_metadata.content_hash_algorithm != CONTENT_HASH_ALGORITHM_POLICY
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "content_hash_algorithm_policy_mismatch",
            "determinism_metadata.content_hash_algorithm",
            format!(
                "expected `{}`, found `{}`",
                CONTENT_HASH_ALGORITHM_POLICY,
                fixture_document.determinism_metadata.content_hash_algorithm
            ),
        );
    }
}

fn validate_open_closed_gap_behavior_output_schema_metadata(
    fixture_document: &OpenClosedGapBehaviorScenarioFixtureDocument,
    errors: &mut Vec<OpenClosedGapBehaviorScenarioValidationError>,
) {
    if fixture_document.output_schema_metadata.schema_version
        != OPEN_CLOSED_GAP_BEHAVIOR_OUTPUT_SCHEMA_VERSION
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "output_schema_version_mismatch",
            "output_schema_metadata.schema_version",
            format!(
                "expected `{}`, found `{}`",
                OPEN_CLOSED_GAP_BEHAVIOR_OUTPUT_SCHEMA_VERSION,
                fixture_document.output_schema_metadata.schema_version
            ),
        );
    }

    if fixture_document
        .output_schema_metadata
        .required_case_output_fields
        .is_empty()
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "required_case_output_fields_empty",
            "output_schema_metadata.required_case_output_fields",
            "must include at least one field",
        );
    }

    for (field_index, required_field) in fixture_document
        .output_schema_metadata
        .required_case_output_fields
        .iter()
        .enumerate()
    {
        validate_open_closed_gap_behavior_non_empty(
            required_field,
            &format!("output_schema_metadata.required_case_output_fields[{field_index}]"),
            errors,
        );
    }

    if fixture_document
        .output_schema_metadata
        .required_summary_fields
        .is_empty()
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "required_summary_fields_empty",
            "output_schema_metadata.required_summary_fields",
            "must include at least one field",
        );
    }

    for (field_index, required_field) in fixture_document
        .output_schema_metadata
        .required_summary_fields
        .iter()
        .enumerate()
    {
        validate_open_closed_gap_behavior_non_empty(
            required_field,
            &format!("output_schema_metadata.required_summary_fields[{field_index}]"),
            errors,
        );
    }
}

fn validate_open_closed_gap_behavior_canonical_metadata_declarations(
    fixture_document: &OpenClosedGapBehaviorScenarioFixtureDocument,
    errors: &mut Vec<OpenClosedGapBehaviorScenarioValidationError>,
) -> BTreeSet<String> {
    if fixture_document
        .canonical_regression_metadata
        .required_tags
        .is_empty()
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "required_tags_empty",
            "canonical_regression_metadata.required_tags",
            "must include at least one required tag",
        );
    }

    let mut required_tag_set: BTreeSet<String> = BTreeSet::new();
    for (required_tag_index, required_tag) in fixture_document
        .canonical_regression_metadata
        .required_tags
        .iter()
        .enumerate()
    {
        validate_open_closed_gap_behavior_non_empty(
            required_tag,
            &format!("canonical_regression_metadata.required_tags[{required_tag_index}]"),
            errors,
        );
        if !required_tag_set.insert(required_tag.clone()) {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "required_tag_duplicate",
                format!("canonical_regression_metadata.required_tags[{required_tag_index}]"),
                format!("duplicate required tag `{required_tag}`"),
            );
        }
    }

    if fixture_document
        .canonical_regression_metadata
        .required_scenario_classes
        .is_empty()
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "required_scenario_classes_empty",
            "canonical_regression_metadata.required_scenario_classes",
            "must include at least one required scenario class",
        );
    }

    let mut required_scenario_class_set: BTreeSet<&str> = BTreeSet::new();
    for (scenario_class_index, required_scenario_class) in fixture_document
        .canonical_regression_metadata
        .required_scenario_classes
        .iter()
        .enumerate()
    {
        validate_open_closed_gap_behavior_non_empty(
            required_scenario_class,
            &format!(
                "canonical_regression_metadata.required_scenario_classes[{scenario_class_index}]"
            ),
            errors,
        );
        if !matches!(
            required_scenario_class.as_str(),
            "known_only" | "surprise_only" | "mixed_news"
        ) {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "unsupported_required_scenario_class",
                format!(
                    "canonical_regression_metadata.required_scenario_classes[{scenario_class_index}]"
                ),
                format!(
                    "required scenario class must be one of `known_only`, `surprise_only`, or `mixed_news`, found `{required_scenario_class}`"
                ),
            );
        }
        if !required_scenario_class_set.insert(required_scenario_class.as_str()) {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "required_scenario_class_duplicate",
                format!(
                    "canonical_regression_metadata.required_scenario_classes[{scenario_class_index}]"
                ),
                format!("duplicate required scenario class `{required_scenario_class}`"),
            );
        }
    }

    if fixture_document
        .canonical_regression_metadata
        .stable_fixture_ids_by_tag
        .is_empty()
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "stable_fixture_ids_by_tag_empty",
            "canonical_regression_metadata.stable_fixture_ids_by_tag",
            "must include at least one tag mapping",
        );
    }

    required_tag_set
}

fn validate_open_closed_gap_behavior_cases_and_collect_coverage(
    fixture_document: &OpenClosedGapBehaviorScenarioFixtureDocument,
    errors: &mut Vec<OpenClosedGapBehaviorScenarioValidationError>,
) -> OpenClosedGapBehaviorObservedCoverage {
    if fixture_document.cases.is_empty() {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "cases_empty",
            "cases",
            "must include at least one scenario case",
        );
    }

    let mut observed_case_ids: BTreeSet<String> = BTreeSet::new();
    let mut observed_coverage = OpenClosedGapBehaviorObservedCoverage::default();
    for (case_index, fixture_case) in fixture_document.cases.iter().enumerate() {
        validate_open_closed_gap_behavior_case(
            fixture_case,
            case_index,
            &mut observed_case_ids,
            &mut observed_coverage,
            errors,
        );
    }

    observed_coverage
}

fn validate_open_closed_gap_behavior_case(
    fixture_case: &OpenClosedGapBehaviorScenarioCaseFixture,
    case_index: usize,
    observed_case_ids: &mut BTreeSet<String>,
    observed_coverage: &mut OpenClosedGapBehaviorObservedCoverage,
    errors: &mut Vec<OpenClosedGapBehaviorScenarioValidationError>,
) {
    let case_field_path_prefix = format!("cases[{case_index}]");
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case.fixture_id,
        &format!("{case_field_path_prefix}.fixture_id"),
        errors,
    );
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case.exchange_code,
        &format!("{case_field_path_prefix}.exchange_code"),
        errors,
    );
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case.scenario_class,
        &format!("{case_field_path_prefix}.scenario_class"),
        errors,
    );
    match fixture_case.scenario_class.as_str() {
        "known_only" => {
            observed_coverage.known_only_case_count =
                observed_coverage.known_only_case_count.saturating_add(1);
        }
        "surprise_only" => {
            observed_coverage.surprise_only_case_count =
                observed_coverage.surprise_only_case_count.saturating_add(1);
        }
        "mixed_news" => {
            observed_coverage.mixed_news_case_count =
                observed_coverage.mixed_news_case_count.saturating_add(1);
        }
        _ => {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "unsupported_scenario_class",
                format!("{case_field_path_prefix}.scenario_class"),
                format!(
                    "scenario_class must be one of `known_only`, `surprise_only`, or `mixed_news`, found `{}`",
                    fixture_case.scenario_class
                ),
            );
        }
    }
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case
            .deterministic_replay_metadata
            .replay_baseline_id,
        &format!("{case_field_path_prefix}.deterministic_replay_metadata.replay_baseline_id"),
        errors,
    );
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case
            .deterministic_replay_metadata
            .event_calendar_binding_id,
        &format!(
            "{case_field_path_prefix}.deterministic_replay_metadata.event_calendar_binding_id"
        ),
        errors,
    );
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case
            .deterministic_replay_metadata
            .jump_observation_binding_id,
        &format!(
            "{case_field_path_prefix}.deterministic_replay_metadata.jump_observation_binding_id"
        ),
        errors,
    );
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case
            .deterministic_replay_metadata
            .gap_observation_binding_id,
        &format!(
            "{case_field_path_prefix}.deterministic_replay_metadata.gap_observation_binding_id"
        ),
        errors,
    );
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case
            .deterministic_replay_metadata
            .candidate_selection_policy_id,
        &format!(
            "{case_field_path_prefix}.deterministic_replay_metadata.candidate_selection_policy_id"
        ),
        errors,
    );
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case.issue_traceability.issue_id,
        &format!("{case_field_path_prefix}.issue_traceability.issue_id"),
        errors,
    );
    if fixture_case
        .issue_traceability
        .semantic_focus_tags
        .is_empty()
    {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "semantic_focus_tags_empty",
            format!("{case_field_path_prefix}.issue_traceability.semantic_focus_tags"),
            "must include at least one semantic focus tag",
        );
    }
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case.calendar_fixture_set_id,
        &format!("{case_field_path_prefix}.calendar_fixture_set_id"),
        errors,
    );

    if fixture_case.calendar_state_fixture_references.is_empty() {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "calendar_state_fixture_references_empty",
            format!("{case_field_path_prefix}.calendar_state_fixture_references"),
            "must include at least one calendar fixture reference",
        );
    }

    let mut observed_calendar_state_fixture_references: BTreeSet<&str> = BTreeSet::new();

    for (reference_index, referenced_fixture_id) in fixture_case
        .calendar_state_fixture_references
        .iter()
        .enumerate()
    {
        validate_open_closed_gap_behavior_non_empty(
            referenced_fixture_id,
            &format!(
                "{case_field_path_prefix}.calendar_state_fixture_references[{reference_index}]"
            ),
            errors,
        );

        if !observed_calendar_state_fixture_references.insert(referenced_fixture_id.as_str()) {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "calendar_state_fixture_reference_duplicate",
                format!(
                    "{case_field_path_prefix}.calendar_state_fixture_references[{reference_index}]"
                ),
                format!("duplicate calendar fixture reference `{referenced_fixture_id}`"),
            );
        }
    }

    validate_open_closed_gap_behavior_non_empty(
        &fixture_case.range_start_utc,
        &format!("{case_field_path_prefix}.range_start_utc"),
        errors,
    );
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case.range_end_utc,
        &format!("{case_field_path_prefix}.range_end_utc"),
        errors,
    );
    validate_open_closed_gap_behavior_non_empty(
        &fixture_case.description,
        &format!("{case_field_path_prefix}.description"),
        errors,
    );

    if !observed_case_ids.insert(fixture_case.fixture_id.clone()) {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "fixture_id_duplicate",
            format!("{case_field_path_prefix}.fixture_id"),
            format!("duplicate fixture_id `{}`", fixture_case.fixture_id),
        );
    }

    if fixture_case.calendar_fixture_set_id != CALENDAR_REGRESSION_FIXTURE_SET_ID {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "calendar_fixture_set_id_mismatch",
            format!("{case_field_path_prefix}.calendar_fixture_set_id"),
            format!(
                "expected `{}`, found `{}`",
                CALENDAR_REGRESSION_FIXTURE_SET_ID, fixture_case.calendar_fixture_set_id
            ),
        );
    }

    validate_open_closed_gap_behavior_finite(
        fixture_case.initial_accumulator_state.j_t,
        &format!("{case_field_path_prefix}.initial_accumulator_state.j_t"),
        errors,
    );
    validate_open_closed_gap_behavior_finite(
        fixture_case.initial_accumulator_state.m_t,
        &format!("{case_field_path_prefix}.initial_accumulator_state.m_t"),
        errors,
    );
    validate_open_closed_gap_behavior_finite(
        fixture_case.initial_accumulator_state.g_t,
        &format!("{case_field_path_prefix}.initial_accumulator_state.g_t"),
        errors,
    );

    if fixture_case.injected_events.is_empty() {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "injected_events_empty",
            format!("{case_field_path_prefix}.injected_events"),
            "must include at least one injected event",
        );
    }

    if fixture_case.tags.is_empty() {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "case_tags_empty",
            format!("{case_field_path_prefix}.tags"),
            "must include at least one case tag",
        );
    }

    for (tag_index, case_tag) in fixture_case.tags.iter().enumerate() {
        validate_open_closed_gap_behavior_non_empty(
            case_tag,
            &format!("{case_field_path_prefix}.tags[{tag_index}]"),
            errors,
        );
        observed_coverage.observed_tags.insert(case_tag.clone());
        observed_coverage
            .observed_case_ids_by_tag
            .entry(case_tag.clone())
            .or_default()
            .insert(fixture_case.fixture_id.clone());
    }

    let mut injected_event_ids: BTreeSet<String> = BTreeSet::new();
    let mut known_event_count = 0usize;
    let mut surprise_event_count = 0usize;
    for (event_index, injected_event) in fixture_case.injected_events.iter().enumerate() {
        let injected_event_field_path_prefix =
            format!("{case_field_path_prefix}.injected_events[{event_index}]");

        validate_open_closed_gap_behavior_non_empty(
            &injected_event.event_id,
            &format!("{injected_event_field_path_prefix}.event_id"),
            errors,
        );
        validate_open_closed_gap_behavior_non_empty(
            &injected_event.timestamp_utc,
            &format!("{injected_event_field_path_prefix}.timestamp_utc"),
            errors,
        );
        validate_open_closed_gap_behavior_non_empty(
            &injected_event.news_arrival_component,
            &format!("{injected_event_field_path_prefix}.news_arrival_component"),
            errors,
        );
        match injected_event.news_arrival_component.as_str() {
            "known" => {
                known_event_count = known_event_count.saturating_add(1);
            }
            "surprise" => {
                surprise_event_count = surprise_event_count.saturating_add(1);
            }
            _ => {
                add_open_closed_gap_behavior_validation_error(
                    errors,
                    "unsupported_news_arrival_component",
                    format!("{injected_event_field_path_prefix}.news_arrival_component"),
                    format!(
                        "news_arrival_component must be either `known` or `surprise`, found `{}`",
                        injected_event.news_arrival_component
                    ),
                );
            }
        }
        validate_open_closed_gap_behavior_finite_non_negative(
            injected_event.shared_severity_mark,
            &format!("{injected_event_field_path_prefix}.shared_severity_mark"),
            errors,
        );
        validate_open_closed_gap_behavior_finite(
            injected_event.coupled_return_jump_mark,
            &format!("{injected_event_field_path_prefix}.coupled_return_jump_mark"),
            errors,
        );

        if !injected_event_ids.insert(injected_event.event_id.clone()) {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "event_id_duplicate",
                format!("{injected_event_field_path_prefix}.event_id"),
                format!("duplicate event_id `{}`", injected_event.event_id),
            );
        }
    }

    match fixture_case.scenario_class.as_str() {
        "known_only" if surprise_event_count > 0 => add_open_closed_gap_behavior_validation_error(
            errors,
            "scenario_class_component_mismatch",
            format!("{case_field_path_prefix}.scenario_class"),
            "known_only scenario cannot include surprise events",
        ),
        "surprise_only" if known_event_count > 0 => add_open_closed_gap_behavior_validation_error(
            errors,
            "scenario_class_component_mismatch",
            format!("{case_field_path_prefix}.scenario_class"),
            "surprise_only scenario cannot include known events",
        ),
        "mixed_news" if known_event_count == 0 || surprise_event_count == 0 => {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "scenario_class_component_mismatch",
                format!("{case_field_path_prefix}.scenario_class"),
                "mixed_news scenario must include both known and surprise events",
            )
        }
        _ => {}
    }

    validate_open_closed_gap_behavior_case_expected_outcome(
        fixture_case,
        &case_field_path_prefix,
        &injected_event_ids,
        errors,
    );
}

fn validate_open_closed_gap_behavior_case_expected_outcome(
    fixture_case: &OpenClosedGapBehaviorScenarioCaseFixture,
    case_field_path_prefix: &str,
    injected_event_ids: &BTreeSet<String>,
    errors: &mut Vec<OpenClosedGapBehaviorScenarioValidationError>,
) {
    if fixture_case.expected_outcome.event_outcomes.is_empty() {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "event_outcomes_empty",
            format!("{case_field_path_prefix}.expected_outcome.event_outcomes"),
            "must include at least one event expectation",
        );
    }

    let mut expected_event_ids: BTreeSet<String> = BTreeSet::new();
    let mut expected_open_event_count = 0_usize;
    let mut expected_closed_event_count = 0_usize;
    let mut expected_total_activity_coupling_j_t_increment = 0.0_f64;
    let mut expected_total_open_jump_m_t_increment = 0.0_f64;
    let mut expected_total_closed_gap_g_t_increment = 0.0_f64;
    let mut expected_total_trading_return_increment = 0.0_f64;

    for (expectation_index, event_expectation) in fixture_case
        .expected_outcome
        .event_outcomes
        .iter()
        .enumerate()
    {
        let expected_event_field_path_prefix = format!(
            "{case_field_path_prefix}.expected_outcome.event_outcomes[{expectation_index}]"
        );

        validate_open_closed_gap_behavior_non_empty(
            &event_expectation.event_id,
            &format!("{expected_event_field_path_prefix}.event_id"),
            errors,
        );

        if !expected_event_ids.insert(event_expectation.event_id.clone()) {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "event_expectation_id_duplicate",
                format!("{expected_event_field_path_prefix}.event_id"),
                format!(
                    "duplicate expected event_id `{}`",
                    event_expectation.event_id
                ),
            );
        }

        validate_open_closed_gap_behavior_finite_non_negative(
            event_expectation.expected_activity_coupling_j_t_increment,
            &format!("{expected_event_field_path_prefix}.expected_activity_coupling_j_t_increment"),
            errors,
        );
        validate_open_closed_gap_behavior_finite(
            event_expectation.expected_open_jump_m_t_increment,
            &format!("{expected_event_field_path_prefix}.expected_open_jump_m_t_increment"),
            errors,
        );
        validate_open_closed_gap_behavior_finite(
            event_expectation.expected_closed_gap_g_t_increment,
            &format!("{expected_event_field_path_prefix}.expected_closed_gap_g_t_increment"),
            errors,
        );
        validate_open_closed_gap_behavior_finite(
            event_expectation.expected_trading_return_increment,
            &format!("{expected_event_field_path_prefix}.expected_trading_return_increment"),
            errors,
        );

        match event_expectation.expected_market_state.as_str() {
            "open" => {
                expected_open_event_count = expected_open_event_count.saturating_add(1);
                if event_expectation.expected_closed_gap_g_t_increment.abs() > 0.0 {
                    add_open_closed_gap_behavior_validation_error(
                        errors,
                        "open_window_closed_gap_not_allowed",
                        format!(
                            "{expected_event_field_path_prefix}.expected_closed_gap_g_t_increment"
                        ),
                        "open-window event expectations must set expected_closed_gap_g_t_increment=0.0",
                    );
                }
            }
            "closed" => {
                expected_closed_event_count = expected_closed_event_count.saturating_add(1);
                if event_expectation.expected_open_jump_m_t_increment.abs() > 0.0 {
                    add_open_closed_gap_behavior_validation_error(
                        errors,
                        "closed_window_open_jump_not_allowed",
                        format!(
                            "{expected_event_field_path_prefix}.expected_open_jump_m_t_increment"
                        ),
                        "closed-window event expectations must set expected_open_jump_m_t_increment=0.0",
                    );
                }
            }
            _ => {
                add_open_closed_gap_behavior_validation_error(
                    errors,
                    "unsupported_expected_market_state",
                    format!("{expected_event_field_path_prefix}.expected_market_state"),
                    format!(
                        "expected_market_state must be either `open` or `closed`, found `{}`",
                        event_expectation.expected_market_state
                    ),
                );
            }
        }

        expected_total_activity_coupling_j_t_increment +=
            event_expectation.expected_activity_coupling_j_t_increment;
        expected_total_open_jump_m_t_increment +=
            event_expectation.expected_open_jump_m_t_increment;
        expected_total_closed_gap_g_t_increment +=
            event_expectation.expected_closed_gap_g_t_increment;
        expected_total_trading_return_increment +=
            event_expectation.expected_trading_return_increment;
    }

    if injected_event_ids != &expected_event_ids {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "event_id_mapping_mismatch",
            format!("{case_field_path_prefix}.expected_outcome.event_outcomes"),
            format!(
                "event expectation ids {:?} must match injected event ids {:?}",
                expected_event_ids, injected_event_ids
            ),
        );
    }

    let expected_summary = fixture_case.expected_outcome.summary;
    if expected_summary.expected_open_event_count != expected_open_event_count {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "summary_open_event_count_mismatch",
            format!("{case_field_path_prefix}.expected_outcome.summary.expected_open_event_count"),
            format!(
                "expected {}, observed {} from event_outcomes",
                expected_summary.expected_open_event_count, expected_open_event_count
            ),
        );
    }

    if expected_summary.expected_closed_event_count != expected_closed_event_count {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "summary_closed_event_count_mismatch",
            format!(
                "{case_field_path_prefix}.expected_outcome.summary.expected_closed_event_count"
            ),
            format!(
                "expected {}, observed {} from event_outcomes",
                expected_summary.expected_closed_event_count, expected_closed_event_count
            ),
        );
    }

    let expected_total_event_count = expected_summary
        .expected_open_event_count
        .saturating_add(expected_summary.expected_closed_event_count);
    if expected_total_event_count != fixture_case.expected_outcome.event_outcomes.len() {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "summary_total_event_count_mismatch",
            format!("{case_field_path_prefix}.expected_outcome.summary"),
            format!(
                "expected_open_event_count + expected_closed_event_count = {} but event_outcomes has length {}",
                expected_total_event_count,
                fixture_case.expected_outcome.event_outcomes.len()
            ),
        );
    }

    if !open_closed_gap_behavior_float_close(
        expected_summary.expected_total_activity_coupling_j_t_increment,
        expected_total_activity_coupling_j_t_increment,
    ) {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "summary_total_activity_coupling_j_t_increment_mismatch",
            format!(
                "{case_field_path_prefix}.expected_outcome.summary.expected_total_activity_coupling_j_t_increment"
            ),
            format!(
                "expected {}, observed {} from event_outcomes",
                expected_summary.expected_total_activity_coupling_j_t_increment,
                expected_total_activity_coupling_j_t_increment
            ),
        );
    }

    if !open_closed_gap_behavior_float_close(
        expected_summary.expected_total_open_jump_m_t_increment,
        expected_total_open_jump_m_t_increment,
    ) {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "summary_total_open_jump_m_t_increment_mismatch",
            format!(
                "{case_field_path_prefix}.expected_outcome.summary.expected_total_open_jump_m_t_increment"
            ),
            format!(
                "expected {}, observed {} from event_outcomes",
                expected_summary.expected_total_open_jump_m_t_increment,
                expected_total_open_jump_m_t_increment
            ),
        );
    }
    if !open_closed_gap_behavior_float_close(
        expected_summary.expected_total_closed_gap_g_t_increment,
        expected_total_closed_gap_g_t_increment,
    ) {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "summary_total_closed_gap_g_t_increment_mismatch",
            format!(
                "{case_field_path_prefix}.expected_outcome.summary.expected_total_closed_gap_g_t_increment"
            ),
            format!(
                "expected {}, observed {} from event_outcomes",
                expected_summary.expected_total_closed_gap_g_t_increment,
                expected_total_closed_gap_g_t_increment
            ),
        );
    }
    if !open_closed_gap_behavior_float_close(
        expected_summary.expected_total_trading_return_increment,
        expected_total_trading_return_increment,
    ) {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "summary_total_trading_return_increment_mismatch",
            format!(
                "{case_field_path_prefix}.expected_outcome.summary.expected_total_trading_return_increment"
            ),
            format!(
                "expected {}, observed {} from event_outcomes",
                expected_summary.expected_total_trading_return_increment,
                expected_total_trading_return_increment
            ),
        );
    }
}

fn validate_open_closed_gap_behavior_canonical_coverage(
    fixture_document: &OpenClosedGapBehaviorScenarioFixtureDocument,
    required_tag_set: &BTreeSet<String>,
    observed_coverage: &OpenClosedGapBehaviorObservedCoverage,
    errors: &mut Vec<OpenClosedGapBehaviorScenarioValidationError>,
) {
    for required_tag in required_tag_set {
        if !observed_coverage.observed_tags.contains(required_tag) {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "required_tag_missing_from_cases",
                "canonical_regression_metadata.required_tags",
                format!("required tag `{required_tag}` is not present in any scenario case"),
            );
        }
    }
    for (scenario_class_index, required_scenario_class) in fixture_document
        .canonical_regression_metadata
        .required_scenario_classes
        .iter()
        .enumerate()
    {
        let observed_count = match required_scenario_class.as_str() {
            "known_only" => observed_coverage.known_only_case_count,
            "surprise_only" => observed_coverage.surprise_only_case_count,
            "mixed_news" => observed_coverage.mixed_news_case_count,
            _ => continue,
        };
        if observed_count == 0 {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "required_scenario_class_missing_from_cases",
                format!(
                    "canonical_regression_metadata.required_scenario_classes[{scenario_class_index}]"
                ),
                format!(
                    "required scenario class `{required_scenario_class}` is not present in any scenario case",
                ),
            );
        }
    }

    for (tag, stable_fixture_ids) in &fixture_document
        .canonical_regression_metadata
        .stable_fixture_ids_by_tag
    {
        validate_open_closed_gap_behavior_non_empty(
            tag,
            "canonical_regression_metadata.stable_fixture_ids_by_tag.key",
            errors,
        );

        if stable_fixture_ids.is_empty() {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "stable_fixture_ids_empty",
                format!("canonical_regression_metadata.stable_fixture_ids_by_tag.{tag}"),
                "must include at least one fixture id",
            );
            continue;
        }

        let mut stable_fixture_id_set: BTreeSet<String> = BTreeSet::new();
        for (fixture_id_index, fixture_id) in stable_fixture_ids.iter().enumerate() {
            validate_open_closed_gap_behavior_non_empty(
                fixture_id,
                &format!(
                    "canonical_regression_metadata.stable_fixture_ids_by_tag.{tag}[{fixture_id_index}]"
                ),
                errors,
            );
            if !stable_fixture_id_set.insert(fixture_id.clone()) {
                add_open_closed_gap_behavior_validation_error(
                    errors,
                    "stable_fixture_id_duplicate",
                    format!(
                        "canonical_regression_metadata.stable_fixture_ids_by_tag.{tag}[{fixture_id_index}]"
                    ),
                    format!("duplicate stable fixture id `{fixture_id}` for tag `{tag}`"),
                );
            }
        }

        let observed_fixture_id_set = observed_coverage
            .observed_case_ids_by_tag
            .get(tag)
            .cloned()
            .unwrap_or_default();
        if observed_fixture_id_set != stable_fixture_id_set {
            add_open_closed_gap_behavior_validation_error(
                errors,
                "stable_fixture_ids_mismatch",
                format!("canonical_regression_metadata.stable_fixture_ids_by_tag.{tag}"),
                format!(
                    "expected fixture ids {:?}, observed fixture ids {:?}",
                    stable_fixture_id_set, observed_fixture_id_set
                ),
            );
        }
    }
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
            known_scheduled_event_timestamps_utc,
            surprise_baseline_intensity,
            surprise_deterministic_time_multiplier_by_interval_index,
        } => {
            validate_finite_non_negative(
                *surprise_baseline_intensity,
                "canonical_output_cases.news_intensity_source.surprise_baseline_intensity",
            )?;

            for known_timestamp_utc in known_scheduled_event_timestamps_utc {
                require_non_empty(
                    known_timestamp_utc,
                    "canonical_output_cases.news_intensity_source.known_scheduled_event_timestamps_utc",
                )?;
            }

            for raw_interval_index in
                surprise_deterministic_time_multiplier_by_interval_index.keys()
            {
                require_non_empty(
                    raw_interval_index,
                    "canonical_output_cases.news_intensity_source.surprise_deterministic_time_multiplier_by_interval_index",
                )?;

                if raw_interval_index.parse::<u64>().is_err() {
                    return Err(format!(
                        "surprise_deterministic_time_multiplier_by_interval_index key `{raw_interval_index}` must parse as u64"
                    ));
                }
            }

            for surprise_multiplier in
                surprise_deterministic_time_multiplier_by_interval_index.values()
            {
                validate_finite_non_negative(
                    *surprise_multiplier,
                    "canonical_output_cases.news_intensity_source.surprise_deterministic_time_multiplier_by_interval_index",
                )?;
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

fn add_open_closed_gap_behavior_validation_error(
    errors: &mut Vec<OpenClosedGapBehaviorScenarioValidationError>,
    code: &'static str,
    field_path: impl Into<String>,
    message: impl Into<String>,
) {
    errors.push(OpenClosedGapBehaviorScenarioValidationError {
        code,
        field_path: field_path.into(),
        message: message.into(),
    });
}

fn validate_open_closed_gap_behavior_non_empty(
    value: &str,
    field_path: &str,
    errors: &mut Vec<OpenClosedGapBehaviorScenarioValidationError>,
) {
    if value.trim().is_empty() {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "required_field_empty",
            field_path.to_string(),
            "field must be non-empty",
        );
    }
}

fn validate_open_closed_gap_behavior_finite(
    value: f64,
    field_path: &str,
    errors: &mut Vec<OpenClosedGapBehaviorScenarioValidationError>,
) {
    if !value.is_finite() {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "non_finite_number",
            field_path.to_string(),
            format!("field must be finite, found {value}"),
        );
    }
}

fn validate_open_closed_gap_behavior_finite_non_negative(
    value: f64,
    field_path: &str,
    errors: &mut Vec<OpenClosedGapBehaviorScenarioValidationError>,
) {
    if !value.is_finite() {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "non_finite_number",
            field_path.to_string(),
            format!("field must be finite, found {value}"),
        );
        return;
    }
    if value < 0.0 {
        add_open_closed_gap_behavior_validation_error(
            errors,
            "negative_number_not_allowed",
            field_path.to_string(),
            format!("field must be non-negative, found {value}"),
        );
    }
}

fn open_closed_gap_behavior_float_close(expected: f64, observed: f64) -> bool {
    let tolerance = 1.0e-12_f64;
    (expected - observed).abs() <= tolerance
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
