use std::collections::BTreeMap;
use std::path::Path;

use janiform_foundations::paths::format_path;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

use super::contracts::{
    PresetCategory, PresetReferenceUsageMode, PresetRegistryScope, PresetResolutionResult,
    PresetWorkflowResolutionError,
};
use super::payload::{
    array_value, object_field, object_value, optional_object_field, payload_error, string_field,
};
use super::support::required_registry_scope;

const BUNDLED_PRESET_INDEX_SCHEMA_VERSION: &str = "preset.runtime.index.v1";
const RESOLVED_PRESET_SNAPSHOT_SCHEMA_VERSION: &str = "preset.runtime.resolved_snapshot.v1";

struct BundledPresetSource {
    artifact_path: &'static str,
    artifact_json: &'static str,
}

macro_rules! bundled_preset_source {
    ($artifact_path:literal) => {
        BundledPresetSource {
            artifact_path: $artifact_path,
            artifact_json: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../python/janiform/calibration/_bundled_presets/generated/",
                $artifact_path
            )),
        }
    };
}

const BUNDLED_PRESET_ARTIFACTS: &[BundledPresetSource] = &[
    bundled_preset_source!("sessions/XCME.json"),
    bundled_preset_source!("sessions/XNAS.json"),
    bundled_preset_source!("sessions/XNYS.json"),
    bundled_preset_source!("sessions/crypto_24_7.json"),
    bundled_preset_source!("sessions/fx_spot_24_5.json"),
    bundled_preset_source!("known_news/eu_us_macro_fx_core.json"),
    bundled_preset_source!("known_news/none.json"),
    bundled_preset_source!("known_news/us_macro_core.json"),
    bundled_preset_source!("market_profiles/audcad_spot.json"),
    bundled_preset_source!("market_profiles/audchf_spot.json"),
    bundled_preset_source!("market_profiles/audjpy_spot.json"),
    bundled_preset_source!("market_profiles/audnzd_spot.json"),
    bundled_preset_source!("market_profiles/audusd_spot.json"),
    bundled_preset_source!("market_profiles/btcusd_spot.json"),
    bundled_preset_source!("market_profiles/cadchf_spot.json"),
    bundled_preset_source!("market_profiles/cadjpy_spot.json"),
    bundled_preset_source!("market_profiles/chfjpy_spot.json"),
    bundled_preset_source!("market_profiles/es_futures_globex.json"),
    bundled_preset_source!("market_profiles/euraud_spot.json"),
    bundled_preset_source!("market_profiles/eurcad_spot.json"),
    bundled_preset_source!("market_profiles/eurchf_spot.json"),
    bundled_preset_source!("market_profiles/eurgbp_spot.json"),
    bundled_preset_source!("market_profiles/eurjpy_spot.json"),
    bundled_preset_source!("market_profiles/eurnzd_spot.json"),
    bundled_preset_source!("market_profiles/eurusd_spot.json"),
    bundled_preset_source!("market_profiles/gbpaud_spot.json"),
    bundled_preset_source!("market_profiles/gbpcad_spot.json"),
    bundled_preset_source!("market_profiles/gbpchf_spot.json"),
    bundled_preset_source!("market_profiles/gbpjpy_spot.json"),
    bundled_preset_source!("market_profiles/gbpnzd_spot.json"),
    bundled_preset_source!("market_profiles/gbpusd_spot.json"),
    bundled_preset_source!("market_profiles/nzdcad_spot.json"),
    bundled_preset_source!("market_profiles/nzdchf_spot.json"),
    bundled_preset_source!("market_profiles/nzdjpy_spot.json"),
    bundled_preset_source!("market_profiles/nzdusd_spot.json"),
    bundled_preset_source!("market_profiles/us_equity_cash.json"),
    bundled_preset_source!("market_profiles/usdcad_spot.json"),
    bundled_preset_source!("market_profiles/usdchf_spot.json"),
    bundled_preset_source!("market_profiles/usdjpy_spot.json"),
];

/// Return the runtime index for bundled preset artifacts embedded in Rust.
pub fn built_in_preset_registry_index() -> PresetResolutionResult<Value> {
    let registry = RuntimePresetRegistry::built_in()?;
    registry.built_in_index()
}

/// Return the runtime index for bundled presets plus request user presets.
pub fn preset_registry_index(request: Value) -> PresetResolutionResult<Value> {
    let request = object_value(&request, "preset_registry_index_request")?;
    let registry = RuntimePresetRegistry::from_request(request)?;

    registry.index()
}

/// Resolve one preset artifact from the bundled registry plus request user presets.
pub fn resolve_preset_artifact(request: Value) -> PresetResolutionResult<Value> {
    let request = object_value(&request, "preset_artifact_resolution_request")?;
    let registry = RuntimePresetRegistry::from_request(request)?;
    let (category, preset_name, registry_scope) = requested_preset(request)?;

    let (resolved_scope, artifact) = registry.resolve(category, preset_name, registry_scope)?;

    Ok(json!({
        "registry_scope": resolved_scope.as_str(),
        // The response crosses the FFI boundary as an owned JSON payload.
        "artifact": artifact.clone(),
    }))
}

/// Resolve a preset artifact with named market-profile dependencies included.
pub fn resolve_preset_snapshot(request: Value) -> PresetResolutionResult<Value> {
    let request = object_value(&request, "preset_snapshot_request")?;
    let registry = RuntimePresetRegistry::from_request(request)?;

    let (category, preset_name, registry_scope) = requested_preset(request)?;

    let (resolved_scope, artifact) = registry.resolve(category, preset_name, registry_scope)?;

    let mut session_preset = Value::Null;
    let mut known_news_preset = Value::Null;
    let mut market_profile_preset = Value::Null;

    match category {
        PresetCategory::Session => {
            // Snapshot payloads own artifacts independently from the registry cache.
            session_preset = artifact.clone();
        }
        PresetCategory::KnownNews => {
            // Snapshot payloads own artifacts independently from the registry cache.
            known_news_preset = artifact.clone();
        }
        PresetCategory::MarketProfile => {
            let artifact_object = object_value(artifact, "market_profile_preset")?;
            let configuration = object_field(
                artifact_object,
                "configuration",
                "market_profile_preset.configuration",
            )?;

            if let Some(reference) = optional_object_field(
                configuration,
                "session_reference",
                "market_profile_preset.configuration.session_reference",
            )? {
                session_preset = resolve_named_dependency_artifact(
                    &registry,
                    PresetCategory::Session,
                    reference,
                    "market_profile_preset.configuration.session_reference",
                )?
                .unwrap_or(Value::Null);
            }

            if let Some(reference) = optional_object_field(
                configuration,
                "known_news_reference",
                "market_profile_preset.configuration.known_news_reference",
            )? {
                known_news_preset = resolve_named_dependency_artifact(
                    &registry,
                    PresetCategory::KnownNews,
                    reference,
                    "market_profile_preset.configuration.known_news_reference",
                )?
                .unwrap_or(Value::Null);
            }

            // Snapshot payloads own artifacts independently from the registry cache.
            market_profile_preset = artifact.clone();
        }
    }

    Ok(json!({
        "schema_version": RESOLVED_PRESET_SNAPSHOT_SCHEMA_VERSION,
        "requested_category": category.as_str(),
        "requested_scope": resolved_scope.as_str(),
        "preset_name": preset_name,
        "session_preset": session_preset,
        "known_news_preset": known_news_preset,
        "market_profile_preset": market_profile_preset,
    }))
}

pub(super) struct RuntimePresetRegistry {
    artifacts: BTreeMap<(PresetRegistryScope, PresetCategory, String), Value>,
}

impl RuntimePresetRegistry {
    fn built_in() -> PresetResolutionResult<Self> {
        let mut registry = Self {
            artifacts: BTreeMap::new(),
        };

        registry.register_built_in_presets()?;

        Ok(registry)
    }

    pub(super) fn from_request(request: &Map<String, Value>) -> PresetResolutionResult<Self> {
        let mut registry = Self {
            artifacts: BTreeMap::new(),
        };

        registry.register_built_in_presets()?;
        registry.register_user_presets(request)?;

        Ok(registry)
    }

    fn register_built_in_presets(&mut self) -> PresetResolutionResult<()> {
        for (artifact_index, source) in BUNDLED_PRESET_ARTIFACTS.iter().enumerate() {
            let artifact =
                serde_json::from_str::<Value>(source.artifact_json).map_err(|source| {
                    payload_error(
                        &format!("bundled_presets[{artifact_index}]"),
                        &format!("must be valid preset artifact JSON: {source}"),
                    )
                })?;

            self.register_artifact(
                PresetRegistryScope::BuiltIn,
                artifact,
                &format!("bundled_presets[{artifact_index}]"),
            )?;
        }

        Ok(())
    }

    fn built_in_index(&self) -> PresetResolutionResult<Value> {
        self.index()
    }

    fn index(&self) -> PresetResolutionResult<Value> {
        let mut keyed_entries = Vec::new();
        let mut bundle_id = None;
        let mut bundle_version = None;
        let mut generated_on = None;

        for ((registry_scope, category, preset_name), artifact) in self.artifacts.iter() {
            let artifact_object = object_value(artifact, "preset_artifact")?;
            let metadata = object_field(artifact_object, "metadata", "preset_artifact.metadata")?;

            let provenance = object_field(
                metadata,
                "provenance",
                "preset_artifact.metadata.provenance",
            )?;

            if *registry_scope == PresetRegistryScope::BuiltIn && bundle_id.is_none() {
                bundle_id = Some(
                    string_field(
                        provenance,
                        "bundle_id",
                        "preset_artifact.metadata.provenance.bundle_id",
                    )?
                    .to_string(),
                );
            }

            if *registry_scope == PresetRegistryScope::BuiltIn && bundle_version.is_none() {
                bundle_version = Some(
                    string_field(
                        provenance,
                        "bundle_version",
                        "preset_artifact.metadata.provenance.bundle_version",
                    )?
                    .to_string(),
                );
            }

            if *registry_scope == PresetRegistryScope::BuiltIn && generated_on.is_none() {
                generated_on = Some(
                    string_field(
                        provenance,
                        "generated_on",
                        "preset_artifact.metadata.provenance.generated_on",
                    )?
                    .to_string(),
                );
            }

            let artifact_path = match registry_scope {
                PresetRegistryScope::BuiltIn => {
                    Some(built_in_artifact_path(*category, preset_name)?)
                }
                PresetRegistryScope::UserRegistered => None,
            };

            let entry = preset_index_entry(
                *registry_scope,
                artifact_path,
                artifact_object,
                metadata,
                provenance,
            )?;

            let sort_key = preset_index_entry_sort_key(&entry)?;

            keyed_entries.push((sort_key, entry));
        }

        keyed_entries.sort_by(|left, right| left.0.cmp(&right.0));

        let entries = keyed_entries
            .into_iter()
            .map(|(_, entry)| entry)
            .collect::<Vec<_>>();

        Ok(json!({
            "schema_version": BUNDLED_PRESET_INDEX_SCHEMA_VERSION,
            "bundle_id": bundle_id.ok_or_else(|| payload_error("bundled_presets", "must contain at least one artifact"))?,
            "bundle_version": bundle_version.ok_or_else(|| payload_error("bundled_presets", "must contain at least one artifact"))?,
            "generated_on": generated_on.ok_or_else(|| payload_error("bundled_presets", "must contain at least one artifact"))?,
            "entries": entries,
        }))
    }

    fn register_user_presets(
        &mut self,
        request: &Map<String, Value>,
    ) -> PresetResolutionResult<()> {
        let Some(registry_payload) = optional_object_field(request, "registry", "registry")? else {
            return Ok(());
        };

        let Some(raw_artifacts) = registry_payload.get("user_registered_presets") else {
            return Ok(());
        };

        if raw_artifacts.is_null() {
            return Ok(());
        }

        let artifacts = array_value(raw_artifacts, "registry.user_registered_presets")?;

        for (artifact_index, artifact) in artifacts.iter().enumerate() {
            // User preset payloads are registry-owned after request validation.
            self.register_artifact(
                PresetRegistryScope::UserRegistered,
                artifact.clone(),
                &format!("registry.user_registered_presets[{artifact_index}]"),
            )?;
        }

        Ok(())
    }

    fn register_artifact(
        &mut self,
        scope: PresetRegistryScope,
        artifact: Value,
        field_path: &str,
    ) -> PresetResolutionResult<()> {
        let artifact_object = object_value(&artifact, field_path)?;

        let category = PresetCategory::from_str(
            string_field(
                artifact_object,
                "category",
                &format!("{field_path}.category"),
            )?,
            &format!("{field_path}.category"),
        )?;

        let preset_name = string_field(
            artifact_object,
            "preset_name",
            &format!("{field_path}.preset_name"),
        )?;

        let key = (scope, category, preset_name.to_string());

        if self.artifacts.insert(key, artifact).is_some() {
            return Err(payload_error(
                field_path,
                "contains duplicate preset artifact for scope, category, and name",
            ));
        }

        Ok(())
    }

    pub(super) fn resolve(
        &self,
        category: PresetCategory,
        preset_name: &str,
        registry_scope: Option<PresetRegistryScope>,
    ) -> PresetResolutionResult<(PresetRegistryScope, &Value)> {
        if let Some(scope) = registry_scope {
            return self.resolve_in_scope(category, preset_name, scope);
        }

        if let Ok(resolved) =
            self.resolve_in_scope(category, preset_name, PresetRegistryScope::UserRegistered)
        {
            return Ok(resolved);
        }

        self.resolve_in_scope(category, preset_name, PresetRegistryScope::BuiltIn)
    }

    fn resolve_in_scope(
        &self,
        category: PresetCategory,
        preset_name: &str,
        registry_scope: PresetRegistryScope,
    ) -> PresetResolutionResult<(PresetRegistryScope, &Value)> {
        self.artifacts
            .get(&(registry_scope, category, preset_name.to_string()))
            .map(|artifact| (registry_scope, artifact))
            .ok_or_else(|| PresetWorkflowResolutionError::UnknownPreset {
                category: category.as_str(),
                registry_scope: registry_scope.as_str(),
                preset_name: preset_name.to_string(),
            })
    }
}

fn requested_preset(
    request: &Map<String, Value>,
) -> PresetResolutionResult<(PresetCategory, &str, Option<PresetRegistryScope>)> {
    let category =
        PresetCategory::from_str(string_field(request, "category", "category")?, "category")?;

    let preset_name = string_field(request, "preset_name", "preset_name")?;

    let registry_scope =
        PresetRegistryScope::from_optional_value(request.get("registry_scope"), "registry_scope")?;

    Ok((category, preset_name, registry_scope))
}

fn preset_index_entry_sort_key(entry: &Value) -> PresetResolutionResult<(u8, String, String)> {
    let entry = object_value(entry, "preset_index_entry")?;

    let category = PresetCategory::from_str(
        string_field(entry, "category", "preset_index_entry.category")?,
        "preset_index_entry.category",
    )?;

    Ok((
        category.index_sort_rank(),
        string_field(entry, "preset_name", "preset_index_entry.preset_name")?.to_string(),
        string_field(entry, "registry_scope", "preset_index_entry.registry_scope")?.to_string(),
    ))
}

fn built_in_artifact_path(
    category: PresetCategory,
    preset_name: &str,
) -> PresetResolutionResult<&'static str> {
    for (artifact_index, source) in BUNDLED_PRESET_ARTIFACTS.iter().enumerate() {
        let artifact = serde_json::from_str::<Value>(source.artifact_json).map_err(|source| {
            payload_error(
                &format!("bundled_presets[{artifact_index}]"),
                &format!("must be valid preset artifact JSON: {source}"),
            )
        })?;

        let artifact = object_value(&artifact, &format!("bundled_presets[{artifact_index}]"))?;

        let source_category = PresetCategory::from_str(
            string_field(
                artifact,
                "category",
                &format!("bundled_presets[{artifact_index}].category"),
            )?,
            &format!("bundled_presets[{artifact_index}].category"),
        )?;

        let source_preset_name = string_field(
            artifact,
            "preset_name",
            &format!("bundled_presets[{artifact_index}].preset_name"),
        )?;

        if source_category == category && source_preset_name == preset_name {
            return Ok(source.artifact_path);
        }
    }

    Err(payload_error(
        "bundled_presets",
        "registered built-in preset is missing its bundled artifact source",
    ))
}

fn preset_index_entry(
    registry_scope: PresetRegistryScope,
    artifact_path: Option<&str>,
    artifact: &Map<String, Value>,
    metadata: &Map<String, Value>,
    provenance: &Map<String, Value>,
) -> PresetResolutionResult<Value> {
    let category = PresetCategory::from_str(
        string_field(artifact, "category", "preset_artifact.category")?,
        "preset_artifact.category",
    )?;
    let formatted_artifact_path = artifact_path.map(|path| format_path(Path::new(path)));

    Ok(json!({
        "artifact_path": formatted_artifact_path,
        "category": category.as_str(),
        "content_hash": string_field(provenance, "content_hash", "preset_artifact.metadata.provenance.content_hash")?,
        "description": string_field(metadata, "description", "preset_artifact.metadata.description")?,
        "display_name": string_field(metadata, "display_name", "preset_artifact.metadata.display_name")?,
        "preset_name": string_field(artifact, "preset_name", "preset_artifact.preset_name")?,
        "registry_scope": registry_scope.as_str(),
        "version_label": string_field(metadata, "version_label", "preset_artifact.metadata.version_label")?,
    }))
}

fn resolve_named_dependency_artifact(
    registry: &RuntimePresetRegistry,
    category: PresetCategory,
    reference: &Map<String, Value>,
    field_path: &str,
) -> PresetResolutionResult<Option<Value>> {
    if PresetReferenceUsageMode::from_reference(reference, field_path)?
        != PresetReferenceUsageMode::Named
    {
        return Ok(None);
    }

    let preset_name = string_field(
        reference,
        "preset_name",
        &format!("{field_path}.preset_name"),
    )?;

    let registry_scope =
        required_registry_scope(reference, &format!("{field_path}.registry_scope"))?;

    let (_, artifact) = registry.resolve(category, preset_name, Some(registry_scope))?;

    // Snapshot payloads own dependency artifacts independently from the registry cache.
    Ok(Some(artifact.clone()))
}
