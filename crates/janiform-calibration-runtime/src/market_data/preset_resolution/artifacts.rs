use serde_json::Map;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;

use super::contracts::{PresetCategory, PresetResolutionResult};
use super::payload::{
    array_field_mut, object_field, object_field_mut, object_value, object_value_mut,
    optional_array_field_mut, optional_object_field_mut, optional_positive_f64_field,
    payload_error, string_field, string_value,
};

const PRESET_CONTENT_HASH_ALGORITHM: &str = "sha256";

/// Serialize a preset artifact using the artifact hash canonicalization format.
pub fn serialize_preset_artifact(artifact: Value) -> PresetResolutionResult<String> {
    let normalized_artifact = normalize_preset_artifact(artifact)?;

    Ok(deterministic_json(&normalized_artifact))
}

/// Deserialize, validate, and normalize preset artifact JSON.
pub fn deserialize_preset_artifact_json(payload_json: &str) -> PresetResolutionResult<Value> {
    let artifact = serde_json::from_str::<Value>(payload_json).map_err(|source| {
        payload_error(
            "preset_artifact",
            &format!("must be valid preset artifact JSON: {source}"),
        )
    })?;

    if !artifact.is_object() {
        return Err(payload_error(
            "preset artifact",
            "must decode to a JSON object",
        ));
    }

    normalize_preset_artifact(artifact)
}

/// Normalize a preset artifact and stamp the validated canonical content hash.
pub fn normalize_preset_artifact(artifact: Value) -> PresetResolutionResult<Value> {
    let mut normalized_artifact = normalize_preset_artifact_fields(artifact)?;
    // The hash path temporarily clears content_hash, while the returned artifact
    // must keep the normalized payload intact for the caller.

    let computed_content_hash = compute_normalized_preset_artifact_content_hash(
        normalized_artifact.clone(),
        "preset_artifact",
    )?;

    let artifact_object = object_value(&normalized_artifact, "preset_artifact")?;

    let observed_content_hash = preset_artifact_content_hash(artifact_object)?;
    if let Some(observed_content_hash) = observed_content_hash
        && observed_content_hash != computed_content_hash
    {
        return Err(payload_error(
            "metadata.provenance.content_hash",
            &format!(
                "must match the normalized artifact content hash `{computed_content_hash}`, received `{observed_content_hash}`"
            ),
        ));
    }

    set_preset_artifact_content_hash(&mut normalized_artifact, Some(computed_content_hash))?;

    Ok(normalized_artifact)
}

/// Compute the canonical preset artifact content hash without mutating the artifact.
pub fn compute_preset_artifact_content_hash(artifact: Value) -> PresetResolutionResult<String> {
    let normalized_artifact = normalize_preset_artifact_fields(artifact)?;

    compute_normalized_preset_artifact_content_hash(normalized_artifact, "preset_artifact")
}

fn compute_normalized_preset_artifact_content_hash(
    mut artifact: Value,
    field_path: &str,
) -> PresetResolutionResult<String> {
    set_preset_artifact_content_hash(&mut artifact, None)?;

    let artifact_payload_json = deterministic_json(&artifact);
    let digest = Sha256::digest(artifact_payload_json.as_bytes());
    let digest_hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    object_value(&artifact, field_path)?;

    Ok(format!("{PRESET_CONTENT_HASH_ALGORITHM}:{digest_hex}"))
}

fn normalize_preset_artifact_fields(mut artifact: Value) -> PresetResolutionResult<Value> {
    let artifact_object = object_value_mut(&mut artifact, "preset_artifact")?;

    let category = PresetCategory::from_str(
        string_field(artifact_object, "category", "preset_artifact.category")?,
        "preset_artifact.category",
    )?;

    normalize_metadata(artifact_object)?;

    let configuration = object_field_mut(
        artifact_object,
        "configuration",
        "preset_artifact.configuration",
    )?;

    match category {
        PresetCategory::Session => {
            normalize_session_configuration(configuration, "preset_artifact.configuration")?;
        }
        PresetCategory::KnownNews => {
            normalize_known_news_configuration(configuration, "preset_artifact.configuration")?;
        }
        PresetCategory::MarketProfile => {
            normalize_market_profile_configuration(configuration, "preset_artifact.configuration")?;
        }
    }

    Ok(artifact)
}

fn normalize_metadata(artifact: &mut Map<String, Value>) -> PresetResolutionResult<()> {
    let metadata = object_field_mut(artifact, "metadata", "preset_artifact.metadata")?;

    let provenance = object_field_mut(
        metadata,
        "provenance",
        "preset_artifact.metadata.provenance",
    )?;

    sort_string_array(
        array_field_mut(
            provenance,
            "source_manifest_ids",
            "preset_artifact.metadata.provenance.source_manifest_ids",
        )?,
        "preset_artifact.metadata.provenance.source_manifest_ids",
    )?;

    sort_object_array_by_key(
        array_field_mut(
            provenance,
            "upstream_source_versions",
            "preset_artifact.metadata.provenance.upstream_source_versions",
        )?,
        "preset_artifact.metadata.provenance.upstream_source_versions",
        |source_version, item_path| {
            Ok(vec![
                string_field(
                    source_version,
                    "source_name",
                    &format!("{item_path}.source_name"),
                )?
                .to_string(),
                string_field(
                    source_version,
                    "version_label",
                    &format!("{item_path}.version_label"),
                )?
                .to_string(),
            ])
        },
    )
}

fn normalize_session_configuration(
    configuration: &mut Map<String, Value>,
    field_path: &str,
) -> PresetResolutionResult<()> {
    insert_empty_array_if_missing(configuration, "recurring_closed_windows");
    insert_null_if_missing(configuration, "exchange_name");

    sort_regular_session_windows(
        array_field_mut(
            configuration,
            "regular_session_windows",
            &format!("{field_path}.regular_session_windows"),
        )?,
        &format!("{field_path}.regular_session_windows"),
    )?;

    sort_recurring_closed_windows(
        array_field_mut(
            configuration,
            "recurring_closed_windows",
            &format!("{field_path}.recurring_closed_windows"),
        )?,
        &format!("{field_path}.recurring_closed_windows"),
    )
}

fn normalize_session_override_patch(
    override_patch: &mut Map<String, Value>,
    field_path: &str,
) -> PresetResolutionResult<()> {
    insert_null_if_missing(override_patch, "timezone_name");
    insert_null_if_missing(override_patch, "regular_session_windows");
    insert_null_if_missing(override_patch, "recurring_closed_windows");

    if let Some(regular_session_windows) = optional_array_field_mut(
        override_patch,
        "regular_session_windows",
        &format!("{field_path}.regular_session_windows"),
    )? {
        sort_regular_session_windows(
            regular_session_windows,
            &format!("{field_path}.regular_session_windows"),
        )?;
    }

    if let Some(recurring_closed_windows) = optional_array_field_mut(
        override_patch,
        "recurring_closed_windows",
        &format!("{field_path}.recurring_closed_windows"),
    )? {
        sort_recurring_closed_windows(
            recurring_closed_windows,
            &format!("{field_path}.recurring_closed_windows"),
        )?;
    }

    Ok(())
}

fn normalize_known_news_configuration(
    configuration: &mut Map<String, Value>,
    field_path: &str,
) -> PresetResolutionResult<()> {
    insert_null_if_missing(configuration, "calendar_timezone_name");

    sort_known_news_events(
        array_field_mut(configuration, "events", &format!("{field_path}.events"))?,
        &format!("{field_path}.events"),
    )
}

fn normalize_known_news_override_patch(
    override_patch: &mut Map<String, Value>,
    field_path: &str,
) -> PresetResolutionResult<()> {
    insert_null_if_missing(override_patch, "calendar_timezone_name");
    insert_empty_array_if_missing(override_patch, "additional_events");
    insert_empty_array_if_missing(override_patch, "removed_event_ids");

    sort_known_news_events(
        array_field_mut(
            override_patch,
            "additional_events",
            &format!("{field_path}.additional_events"),
        )?,
        &format!("{field_path}.additional_events"),
    )?;

    sort_string_array(
        array_field_mut(
            override_patch,
            "removed_event_ids",
            &format!("{field_path}.removed_event_ids"),
        )?,
        &format!("{field_path}.removed_event_ids"),
    )
}

fn normalize_market_profile_configuration(
    configuration: &mut Map<String, Value>,
    field_path: &str,
) -> PresetResolutionResult<()> {
    insert_null_if_missing(configuration, "known_news_reference");
    insert_string_if_missing(configuration, "known_news_policy", "suggest_only");
    insert_null_if_missing(configuration, "timestamp_timezone_name");
    insert_null_if_missing(configuration, "tick_size");
    insert_empty_array_if_missing(configuration, "column_alias_hints");

    let _tick_size = optional_positive_f64_field(
        configuration,
        "tick_size",
        &format!("{field_path}.tick_size"),
        "tick_size",
    )?;

    normalize_session_reference(
        object_field_mut(
            configuration,
            "session_reference",
            &format!("{field_path}.session_reference"),
        )?,
        &format!("{field_path}.session_reference"),
    )?;

    if let Some(known_news_reference) = optional_object_field_mut(
        configuration,
        "known_news_reference",
        &format!("{field_path}.known_news_reference"),
    )? {
        normalize_known_news_reference(
            known_news_reference,
            &format!("{field_path}.known_news_reference"),
        )?;
    }

    sort_column_alias_hints(
        array_field_mut(
            configuration,
            "column_alias_hints",
            &format!("{field_path}.column_alias_hints"),
        )?,
        &format!("{field_path}.column_alias_hints"),
    )?;

    Ok(())
}

fn normalize_session_reference(
    reference: &mut Map<String, Value>,
    field_path: &str,
) -> PresetResolutionResult<()> {
    insert_null_if_missing(reference, "registry_scope");
    insert_null_if_missing(reference, "preset_name");
    insert_null_if_missing(reference, "configuration");
    insert_null_if_missing(reference, "override_patch");
    insert_null_if_missing(reference, "exchange_code_hint");
    insert_null_if_missing(reference, "timezone_name_hint");

    if let Some(configuration) = optional_object_field_mut(
        reference,
        "configuration",
        &format!("{field_path}.configuration"),
    )? {
        normalize_session_configuration(configuration, &format!("{field_path}.configuration"))?;
    }

    if let Some(override_patch) = optional_object_field_mut(
        reference,
        "override_patch",
        &format!("{field_path}.override_patch"),
    )? {
        normalize_session_override_patch(override_patch, &format!("{field_path}.override_patch"))?;
    }

    Ok(())
}

fn normalize_known_news_reference(
    reference: &mut Map<String, Value>,
    field_path: &str,
) -> PresetResolutionResult<()> {
    insert_null_if_missing(reference, "registry_scope");
    insert_null_if_missing(reference, "preset_name");
    insert_null_if_missing(reference, "configuration");
    insert_null_if_missing(reference, "override_patch");

    if let Some(configuration) = optional_object_field_mut(
        reference,
        "configuration",
        &format!("{field_path}.configuration"),
    )? {
        normalize_known_news_configuration(configuration, &format!("{field_path}.configuration"))?;
    }

    if let Some(override_patch) = optional_object_field_mut(
        reference,
        "override_patch",
        &format!("{field_path}.override_patch"),
    )? {
        normalize_known_news_override_patch(
            override_patch,
            &format!("{field_path}.override_patch"),
        )?;
    }

    Ok(())
}

fn sort_regular_session_windows(
    windows: &mut Vec<Value>,
    field_path: &str,
) -> PresetResolutionResult<()> {
    sort_object_array_by_key(windows, field_path, |window, item_path| {
        Ok(vec![
            weekday_sort_key(string_field(
                window,
                "weekday",
                &format!("{item_path}.weekday"),
            )?),
            string_field(window, "open_time", &format!("{item_path}.open_time"))?.to_string(),
            string_field(window, "close_time", &format!("{item_path}.close_time"))?.to_string(),
            string_field(window, "label", &format!("{item_path}.label"))?.to_string(),
        ])
    })
}

fn sort_recurring_closed_windows(
    windows: &mut Vec<Value>,
    field_path: &str,
) -> PresetResolutionResult<()> {
    sort_object_array_by_key(windows, field_path, |window, item_path| {
        Ok(vec![
            weekday_sort_key(string_field(
                window,
                "start_weekday",
                &format!("{item_path}.start_weekday"),
            )?),
            string_field(window, "start_time", &format!("{item_path}.start_time"))?.to_string(),
            weekday_sort_key(string_field(
                window,
                "end_weekday",
                &format!("{item_path}.end_weekday"),
            )?),
            string_field(window, "end_time", &format!("{item_path}.end_time"))?.to_string(),
            string_field(window, "reason", &format!("{item_path}.reason"))?.to_string(),
        ])
    })
}

fn sort_known_news_events(events: &mut Vec<Value>, field_path: &str) -> PresetResolutionResult<()> {
    for (event_index, event) in events.iter_mut().enumerate() {
        let event = object_value_mut(event, &format!("{field_path}[{event_index}]"))?;

        insert_null_if_missing(event, "event_name");
        insert_empty_array_if_missing(event, "tags");

        sort_string_array(
            array_field_mut(event, "tags", &format!("{field_path}[{event_index}].tags"))?,
            &format!("{field_path}[{event_index}].tags"),
        )?;
    }

    sort_object_array_by_key(events, field_path, |event, item_path| {
        Ok(vec![
            string_field(event, "scheduled_at", &format!("{item_path}.scheduled_at"))?.to_string(),
            string_field(event, "event_id", &format!("{item_path}.event_id"))?.to_string(),
        ])
    })
}

fn sort_column_alias_hints(
    column_alias_hints: &mut Vec<Value>,
    field_path: &str,
) -> PresetResolutionResult<()> {
    for (hint_index, hint) in column_alias_hints.iter_mut().enumerate() {
        let hint = object_value_mut(hint, &format!("{field_path}[{hint_index}]"))?;

        sort_string_array(
            array_field_mut(
                hint,
                "aliases",
                &format!("{field_path}[{hint_index}].aliases"),
            )?,
            &format!("{field_path}[{hint_index}].aliases"),
        )?;
    }

    sort_object_array_by_key(column_alias_hints, field_path, |hint, item_path| {
        Ok(vec![
            string_field(
                hint,
                "canonical_name",
                &format!("{item_path}.canonical_name"),
            )?
            .to_string(),
        ])
    })
}

fn sort_string_array(values: &mut [Value], field_path: &str) -> PresetResolutionResult<()> {
    for (value_index, value) in values.iter().enumerate() {
        string_value(value, &format!("{field_path}[{value_index}]"))?;
    }

    values.sort_by(|left, right| left.as_str().cmp(&right.as_str()));

    Ok(())
}

fn sort_object_array_by_key<F>(
    values: &mut Vec<Value>,
    field_path: &str,
    key_function: F,
) -> PresetResolutionResult<()>
where
    F: Fn(&Map<String, Value>, &str) -> PresetResolutionResult<Vec<String>>,
{
    let mut keyed_values = Vec::with_capacity(values.len());

    for (value_index, value) in values.drain(..).enumerate() {
        let item_path = format!("{field_path}[{value_index}]");
        let key = key_function(object_value(&value, &item_path)?, &item_path)?;

        keyed_values.push((key, value));
    }

    keyed_values.sort_by(|left, right| left.0.cmp(&right.0));
    values.extend(keyed_values.into_iter().map(|(_, value)| value));

    Ok(())
}

fn weekday_sort_key(weekday_name: &str) -> String {
    let weekday_index = match weekday_name {
        "Monday" => 0,
        "Tuesday" => 1,
        "Wednesday" => 2,
        "Thursday" => 3,
        "Friday" => 4,
        "Saturday" => 5,
        "Sunday" => 6,
        _ => 7,
    };

    format!("{weekday_index:02}:{weekday_name}")
}

fn insert_null_if_missing(payload: &mut Map<String, Value>, field_name: &str) {
    payload.entry(field_name.to_string()).or_insert(Value::Null);
}

fn insert_empty_array_if_missing(payload: &mut Map<String, Value>, field_name: &str) {
    payload
        .entry(field_name.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
}

fn insert_string_if_missing(payload: &mut Map<String, Value>, field_name: &str, value: &str) {
    payload
        .entry(field_name.to_string())
        .or_insert_with(|| Value::String(value.to_string()));
}

fn preset_artifact_content_hash(
    artifact: &Map<String, Value>,
) -> PresetResolutionResult<Option<String>> {
    let metadata = object_field(artifact, "metadata", "preset_artifact.metadata")?;

    let provenance = object_field(
        metadata,
        "provenance",
        "preset_artifact.metadata.provenance",
    )?;

    let Some(content_hash) = provenance.get("content_hash") else {
        return Ok(None);
    };
    if content_hash.is_null() {
        return Ok(None);
    }

    let content_hash = string_value(content_hash, "metadata.provenance.content_hash")?;

    Ok(Some(content_hash.to_string()))
}

fn set_preset_artifact_content_hash(
    artifact: &mut Value,
    content_hash: Option<String>,
) -> PresetResolutionResult<()> {
    let artifact = object_value_mut(artifact, "preset_artifact")?;

    let metadata = object_field_mut(artifact, "metadata", "preset_artifact.metadata")?;

    let provenance = object_field_mut(
        metadata,
        "provenance",
        "preset_artifact.metadata.provenance",
    )?;

    provenance.insert(
        "content_hash".to_string(),
        content_hash.map_or(Value::Null, Value::String),
    );

    Ok(())
}

fn deterministic_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(raw_value) => raw_value.to_string(),
        Value::Number(raw_value) => raw_value.to_string(),
        Value::String(raw_value) => json_string(raw_value),
        Value::Array(raw_values) => {
            let values = raw_values
                .iter()
                .map(deterministic_json)
                .collect::<Vec<_>>();

            format!("[{}]", values.join(", "))
        }
        Value::Object(raw_values) => {
            let values = raw_values
                .iter()
                .map(|(key, value)| format!("{}: {}", json_string(key), deterministic_json(value)))
                .collect::<Vec<_>>();

            format!("{{{}}}", values.join(", "))
        }
    }
}

fn json_string(raw_value: &str) -> String {
    let mut serialized = String::from("\"");

    for character in raw_value.chars() {
        match character {
            '"' => serialized.push_str("\\\""),
            '\\' => serialized.push_str("\\\\"),
            '\u{08}' => serialized.push_str("\\b"),
            '\u{0c}' => serialized.push_str("\\f"),
            '\n' => serialized.push_str("\\n"),
            '\r' => serialized.push_str("\\r"),
            '\t' => serialized.push_str("\\t"),
            character if character < '\u{20}' => {
                serialized.push_str(&format!("\\u{:04x}", character as u32));
            }
            character if character.is_ascii() => serialized.push(character),
            character => append_ascii_json_escape(&mut serialized, character),
        }
    }

    serialized.push('"');
    serialized
}

fn append_ascii_json_escape(serialized: &mut String, character: char) {
    let codepoint = character as u32;
    if codepoint <= 0xffff {
        serialized.push_str(&format!("\\u{codepoint:04x}"));

        return;
    }

    let adjusted = codepoint - 0x1_0000;
    let high_surrogate = 0xd800 + ((adjusted >> 10) & 0x3ff);
    let low_surrogate = 0xdc00 + (adjusted & 0x3ff);

    serialized.push_str(&format!("\\u{high_surrogate:04x}\\u{low_surrogate:04x}"));
}
