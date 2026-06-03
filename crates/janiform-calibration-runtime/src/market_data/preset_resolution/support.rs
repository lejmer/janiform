use std::collections::BTreeMap;

use serde_json::Map;
use serde_json::Value;
use serde_json::json;

use super::contracts::{
    DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME, PresetCategory, PresetReferenceUsageMode,
    PresetRegistryScope, PresetResolutionResult,
};
use super::payload::{
    array_value_mut, clone_field, clone_object_field, object_value, object_value_mut,
    optional_array_field, optional_string_field, payload_error, replace_if_present, string_field,
    string_value,
};

pub(super) fn session_configuration_from_inference(
    inference_resolution: &Value,
) -> PresetResolutionResult<Value> {
    let inference = object_value(inference_resolution, "inferred_session_resolution")?;

    Ok(json!({
        "exchange_code": clone_field(inference, "exchange_code", "inferred_session_resolution.exchange_code")?,
        "exchange_name": inference.get("exchange_name").cloned().unwrap_or(Value::Null),
        "timezone_name": clone_field(inference, "resolved_timezone_name", "inferred_session_resolution.resolved_timezone_name")?,
        "regular_session_windows": clone_field(inference, "regular_session_windows", "inferred_session_resolution.regular_session_windows")?,
        "recurring_closed_windows": clone_field(inference, "recurring_closed_windows", "inferred_session_resolution.recurring_closed_windows")?,
    }))
}

pub(super) fn apply_session_override_patch(
    configuration: &mut Value,
    override_patch: &Map<String, Value>,
) -> PresetResolutionResult<()> {
    let configuration = object_value_mut(configuration, "session.configuration")?;

    replace_if_present(configuration, override_patch, "timezone_name")?;
    replace_if_present(configuration, override_patch, "regular_session_windows")?;
    replace_if_present(configuration, override_patch, "recurring_closed_windows")?;

    Ok(())
}

pub(super) fn apply_known_news_override_patch(
    configuration: &mut Value,
    override_patch: &Map<String, Value>,
) -> PresetResolutionResult<()> {
    let configuration = object_value_mut(configuration, "known_news.configuration")?;
    replace_if_present(configuration, override_patch, "calendar_timezone_name")?;

    let events = configuration
        .entry("events".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let events = array_value_mut(events, "known_news.configuration.events")?;

    let mut event_by_id = BTreeMap::new();

    for event in events.iter() {
        let event_object = object_value(event, "known_news.configuration.events[]")?;

        let event_id = string_field(
            event_object,
            "event_id",
            "known_news.configuration.events[].event_id",
        )?;

        // The patch application rebuilds the owned event vector after removals
        // and additions, so the map owns the selected event payloads.
        event_by_id.insert(event_id.to_string(), event.clone());
    }

    if let Some(removed_event_ids) = optional_array_field(
        override_patch,
        "removed_event_ids",
        "known_news_reference.override_patch.removed_event_ids",
    )? {
        for event_id in removed_event_ids {
            let event_id = string_value(
                event_id,
                "known_news_reference.override_patch.removed_event_ids[]",
            )?;
            if event_by_id.remove(event_id).is_none() {
                return Err(payload_error(
                    "known_news_reference.override_patch.removed_event_ids",
                    &format!(
                        "cannot remove unknown known-news event_id `{event_id}` from the selected baseline"
                    ),
                ));
            }
        }
    }

    if let Some(additional_events) = optional_array_field(
        override_patch,
        "additional_events",
        "known_news_reference.override_patch.additional_events",
    )? {
        for event in additional_events {
            let event_object = object_value(
                event,
                "known_news_reference.override_patch.additional_events[]",
            )?;

            let event_id = string_field(
                event_object,
                "event_id",
                "known_news_reference.override_patch.additional_events[].event_id",
            )?;

            // Added events are inserted into the owned event map so duplicate
            // detection and final ordering share one canonical path.
            if event_by_id
                .insert(event_id.to_string(), event.clone())
                .is_some()
            {
                return Err(payload_error(
                    "known_news_reference.override_patch.additional_events",
                    &format!(
                        "cannot add duplicate known-news event_id `{event_id}` to the selected baseline"
                    ),
                ));
            }
        }
    }

    *events = event_by_id.into_values().collect();

    Ok(())
}

pub(super) fn apply_market_profile_override_patch(
    configuration: &mut Value,
    override_patch: &Map<String, Value>,
) -> PresetResolutionResult<()> {
    let configuration = object_value_mut(configuration, "market_profile.configuration")?;

    for field_name in [
        "session_reference",
        "known_news_reference",
        "known_news_policy",
        "timestamp_timezone_name",
        "tick_size",
        "column_alias_hints",
        "default_calibration_hint_ids",
    ] {
        replace_if_present(configuration, override_patch, field_name)?;
    }

    Ok(())
}

pub(super) fn apply_override_patch_and_collect_fields(
    configuration: &mut Value,
    override_patch: Option<&Map<String, Value>>,
    apply_override_patch: impl FnOnce(&mut Value, &Map<String, Value>) -> PresetResolutionResult<()>,
    override_fields: impl FnOnce(&Map<String, Value>) -> Vec<String>,
) -> PresetResolutionResult<Vec<String>> {
    let Some(override_patch) = override_patch else {
        return Ok(Vec::new());
    };

    apply_override_patch(configuration, override_patch)?;

    Ok(override_fields(override_patch))
}

pub(super) fn append_inference_diagnostics(
    inference_resolution: &Value,
    diagnostics: &mut Vec<Value>,
) -> PresetResolutionResult<()> {
    let inference = object_value(inference_resolution, "inferred_session_resolution")?;
    if optional_string_field(
        inference,
        "confidence_label",
        "inferred_session_resolution.confidence_label",
    )? == Some("low")
    {
        diagnostics.push(diagnostic(
            "session",
            "warning",
            "session_inference_low_confidence",
            "The inferred session baseline reported low confidence. Review the session diagnostics before relying on the fitted model for downstream simulation.",
        ));
    }

    for warning in optional_array_field(
        inference,
        "ambiguity_warnings",
        "inferred_session_resolution.ambiguity_warnings",
    )?
    .map_or(&[] as &[Value], |warnings| warnings.as_slice())
    {
        diagnostics.push(diagnostic(
            "session",
            "warning",
            "session_inference_ambiguous",
            string_value(warning, "inferred_session_resolution.ambiguity_warnings[]")?,
        ));
    }

    for discrepancy in optional_array_field(
        inference,
        "metadata_discrepancies",
        "inferred_session_resolution.metadata_discrepancies",
    )?
    .map_or(&[] as &[Value], |discrepancies| discrepancies.as_slice())
    {
        diagnostics.push(diagnostic(
            "session",
            "warning",
            "session_inference_metadata_discrepancy",
            string_value(
                discrepancy,
                "inferred_session_resolution.metadata_discrepancies[]",
            )?,
        ));
    }

    Ok(())
}

pub(super) fn artifact_configuration(
    artifact: &Value,
    field_path: &str,
) -> PresetResolutionResult<Value> {
    let artifact = object_value(artifact, field_path)?;

    // Resolved workflow payloads must own the selected configuration because
    // registry artifacts are shared lookup data.
    clone_object_field(
        artifact,
        "configuration",
        &format!("{field_path}.configuration"),
    )
}

pub(super) fn selected_preset(
    category: PresetCategory,
    registry_scope: PresetRegistryScope,
    preset_name: &str,
) -> Value {
    json!({
        "category": category.as_str(),
        "registry_scope": registry_scope.as_str(),
        "preset_name": preset_name,
    })
}

pub(super) fn source_kind_for_named_reference(
    registry_scope: PresetRegistryScope,
    override_fields: &[String],
) -> &'static str {
    if !override_fields.is_empty() {
        return "overridden";
    }

    if registry_scope == PresetRegistryScope::BuiltIn {
        return "built_in";
    }

    "fully_custom"
}

pub(super) fn session_reference_replaces_inferred_baseline(reference: &Map<String, Value>) -> bool {
    !matches!(
        PresetReferenceUsageMode::from_reference(reference, "session_reference"),
        Ok(PresetReferenceUsageMode::Automatic)
    ) || reference
        .get("override_patch")
        .is_some_and(|override_patch| !override_patch.is_null())
}

pub(super) fn session_override_fields(override_patch: &Map<String, Value>) -> Vec<String> {
    override_fields(
        override_patch,
        &[
            "timezone_name",
            "regular_session_windows",
            "recurring_closed_windows",
        ],
    )
}

pub(super) fn known_news_override_fields(override_patch: &Map<String, Value>) -> Vec<String> {
    let mut fields = Vec::new();

    if override_patch
        .get("calendar_timezone_name")
        .is_some_and(|value| !value.is_null())
    {
        fields.push("calendar_timezone_name".to_string());
    }

    if override_patch
        .get("additional_events")
        .and_then(Value::as_array)
        .is_some_and(|events| !events.is_empty())
    {
        fields.push("additional_events".to_string());
    }

    if override_patch
        .get("removed_event_ids")
        .and_then(Value::as_array)
        .is_some_and(|event_ids| !event_ids.is_empty())
    {
        fields.push("removed_event_ids".to_string());
    }

    fields
}

pub(super) fn market_profile_override_fields(override_patch: &Map<String, Value>) -> Vec<String> {
    override_fields(
        override_patch,
        &[
            "session_reference",
            "known_news_reference",
            "known_news_policy",
            "timestamp_timezone_name",
            "tick_size",
            "column_alias_hints",
            "default_calibration_hint_ids",
        ],
    )
}

pub(super) fn override_fields(
    override_patch: &Map<String, Value>,
    field_names: &[&str],
) -> Vec<String> {
    field_names
        .iter()
        .filter(|field_name| {
            override_patch
                .get(**field_name)
                .is_some_and(|value| !value.is_null())
        })
        .map(|field_name| (*field_name).to_string())
        .collect()
}

pub(super) fn automatic_session_reference() -> &'static Map<String, Value> {
    static AUTOMATIC_SESSION_REFERENCE: std::sync::OnceLock<Map<String, Value>> =
        std::sync::OnceLock::new();

    AUTOMATIC_SESSION_REFERENCE.get_or_init(|| {
        let mut reference = Map::new();

        reference.insert(
            "usage_mode".to_string(),
            Value::String("automatic".to_string()),
        );

        reference
    })
}

pub(super) fn default_known_news_reference() -> &'static Map<String, Value> {
    static DEFAULT_KNOWN_NEWS_REFERENCE: std::sync::OnceLock<Map<String, Value>> =
        std::sync::OnceLock::new();

    DEFAULT_KNOWN_NEWS_REFERENCE.get_or_init(|| {
        let mut reference = Map::new();

        reference.insert("usage_mode".to_string(), Value::String("named".to_string()));
        reference.insert(
            "registry_scope".to_string(),
            Value::String("built_in".to_string()),
        );
        reference.insert(
            "preset_name".to_string(),
            Value::String(DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME.to_string()),
        );

        reference
    })
}

pub(super) fn diagnostic(component: &str, severity: &str, code: &str, message: &str) -> Value {
    json!({
        "component": component,
        "severity": severity,
        "code": code,
        "message": message,
    })
}

pub(super) fn required_registry_scope(
    reference: &Map<String, Value>,
    field_path: &str,
) -> PresetResolutionResult<PresetRegistryScope> {
    PresetRegistryScope::from_optional_value(reference.get("registry_scope"), field_path)?
        .ok_or_else(|| payload_error(field_path, "is required when usage_mode=named"))
}
