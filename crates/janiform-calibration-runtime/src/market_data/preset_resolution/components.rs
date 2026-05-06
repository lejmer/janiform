use serde_json::Map;
use serde_json::Value;
use serde_json::json;

use super::contracts::{PresetCategory, PresetReferenceUsageMode, PresetResolutionResult};
use super::payload::{
    clone_object_field, object_field, object_value, optional_object_field, optional_string_field,
    payload_error, string_field,
};
use super::registry::RuntimePresetRegistry;
use super::support::{
    append_inference_diagnostics, apply_known_news_override_patch,
    apply_market_profile_override_patch, apply_override_patch_and_collect_fields,
    apply_session_override_patch, artifact_configuration, automatic_session_reference,
    default_known_news_reference, diagnostic, known_news_override_fields,
    market_profile_override_fields, required_registry_scope, selected_preset,
    session_configuration_from_inference, session_override_fields,
    session_reference_replaces_inferred_baseline, source_kind_for_named_reference,
};

pub(super) fn resolve_market_profile_component(
    market_profile_reference: Option<&Map<String, Value>>,
    registry: &RuntimePresetRegistry,
) -> PresetResolutionResult<Option<Value>> {
    let Some(reference) = market_profile_reference else {
        return Ok(None);
    };

    match PresetReferenceUsageMode::from_reference(reference, "market_profile_reference")? {
        PresetReferenceUsageMode::Named => {
            let preset_name = string_field(
                reference,
                "preset_name",
                "market_profile_reference.preset_name",
            )?;

            let registry_scope =
                required_registry_scope(reference, "market_profile_reference.registry_scope")?;

            let (_, artifact) = registry.resolve(
                PresetCategory::MarketProfile,
                preset_name,
                Some(registry_scope),
            )?;

            let mut configuration = artifact_configuration(artifact, "market_profile_preset")?;

            let override_patch = optional_object_field(
                reference,
                "override_patch",
                "market_profile_reference.override_patch",
            )?;

            let override_fields = apply_override_patch_and_collect_fields(
                &mut configuration,
                override_patch,
                apply_market_profile_override_patch,
                market_profile_override_fields,
            )?;

            Ok(Some(json!({
                "configuration": configuration,
                "source_kind": source_kind_for_named_reference(registry_scope, &override_fields),
                "selected_preset": selected_preset(PresetCategory::MarketProfile, registry_scope, preset_name),
                "applied_override_fields": override_fields,
            })))
        }
        PresetReferenceUsageMode::Explicit => {
            let configuration = clone_object_field(
                reference,
                "configuration",
                "market_profile_reference.configuration",
            )?;

            Ok(Some(json!({
                "configuration": configuration,
                "source_kind": "fully_custom",
                "selected_preset": null,
                "applied_override_fields": [],
            })))
        }
        PresetReferenceUsageMode::Automatic => Err(payload_error(
            "market_profile_reference.usage_mode",
            "automatic is not supported for market-profile resolution",
        )),
    }
}

pub(super) fn resolve_session_component(
    registry: &RuntimePresetRegistry,
    session_reference: Option<&Map<String, Value>>,
    market_profile: Option<&Value>,
    inferred_session_resolution: Option<&Value>,
    diagnostics: &mut Vec<Value>,
) -> PresetResolutionResult<Value> {
    let profile_reference = match market_profile {
        Some(market_profile) => {
            let component = object_value(market_profile, "market_profile")?;
            let configuration =
                object_field(component, "configuration", "market_profile.configuration")?;

            optional_object_field(
                configuration,
                "session_reference",
                "market_profile.configuration.session_reference",
            )?
        }
        None => None,
    };

    let selected_reference = selected_session_reference(
        session_reference,
        profile_reference,
        inferred_session_resolution,
        diagnostics,
    );
    let Some(selected_reference) = selected_reference else {
        return Err(payload_error(
            "session_reference",
            "could not resolve a session baseline. Provide an explicit session reference or inferred session resolution.",
        ));
    };

    match PresetReferenceUsageMode::from_reference(selected_reference, "session_reference")? {
        PresetReferenceUsageMode::Automatic => {
            let Some(inference_resolution) = inferred_session_resolution else {
                return Err(payload_error(
                    "inferred_session_resolution",
                    "is required when the resolved session reference uses automatic inference",
                ));
            };

            let mut configuration = session_configuration_from_inference(inference_resolution)?;

            let override_patch = optional_object_field(
                selected_reference,
                "override_patch",
                "session_reference.override_patch",
            )?;

            let override_fields = apply_override_patch_and_collect_fields(
                &mut configuration,
                override_patch,
                apply_session_override_patch,
                session_override_fields,
            )?;

            append_inference_diagnostics(inference_resolution, diagnostics)?;

            Ok(json!({
                "configuration": configuration,
                "source_kind": if override_fields.is_empty() { "inferred" } else { "overridden" },
                "selected_preset": null,
                "inferred_from_data": true,
                "inference_resolution": inference_resolution,
                "applied_override_fields": override_fields,
            }))
        }
        PresetReferenceUsageMode::Named => {
            let preset_name = string_field(
                selected_reference,
                "preset_name",
                "session_reference.preset_name",
            )?;
            let registry_scope =
                required_registry_scope(selected_reference, "session_reference.registry_scope")?;

            let (_, artifact) =
                registry.resolve(PresetCategory::Session, preset_name, Some(registry_scope))?;

            let mut configuration = artifact_configuration(artifact, "session_preset")?;

            let override_patch = optional_object_field(
                selected_reference,
                "override_patch",
                "session_reference.override_patch",
            )?;

            let override_fields = apply_override_patch_and_collect_fields(
                &mut configuration,
                override_patch,
                apply_session_override_patch,
                session_override_fields,
            )?;

            Ok(json!({
                "configuration": configuration,
                "source_kind": source_kind_for_named_reference(registry_scope, &override_fields),
                "selected_preset": selected_preset(PresetCategory::Session, registry_scope, preset_name),
                "inferred_from_data": false,
                "inference_resolution": null,
                "applied_override_fields": override_fields,
            }))
        }
        PresetReferenceUsageMode::Explicit => {
            let configuration = clone_object_field(
                selected_reference,
                "configuration",
                "session_reference.configuration",
            )?;

            Ok(json!({
                "configuration": configuration,
                "source_kind": "fully_custom",
                "selected_preset": null,
                "inferred_from_data": false,
                "inference_resolution": null,
                "applied_override_fields": [],
            }))
        }
    }
}

pub(super) fn selected_session_reference<'a>(
    session_reference: Option<&'a Map<String, Value>>,
    profile_reference: Option<&'a Map<String, Value>>,
    inferred_session_resolution: Option<&Value>,
    diagnostics: &mut Vec<Value>,
) -> Option<&'a Map<String, Value>> {
    if let Some(session_reference) = session_reference {
        if inferred_session_resolution.is_some()
            && session_reference_replaces_inferred_baseline(session_reference)
        {
            diagnostics.push(diagnostic(
                "session",
                "warning",
                "conflicting_override",
                "Explicit session selection replaced the automatically inferred session baseline for the resolved workflow configuration.",
            ));
        }

        if profile_reference.is_some_and(|profile_reference| profile_reference != session_reference)
        {
            diagnostics.push(diagnostic(
                "session",
                "info",
                "profile_component_resolution_outcome",
                "The selected market profile provided a session baseline, but the explicit session input took precedence.",
            ));
        }

        return Some(session_reference);
    }

    if inferred_session_resolution.is_some() && profile_reference.is_some() {
        diagnostics.push(diagnostic(
            "session",
            "info",
            "profile_component_resolution_outcome",
            "Session inference was available, so the resolved session used the inferred baseline instead of the market profile session baseline.",
        ));
    }

    if inferred_session_resolution.is_some() {
        return Some(automatic_session_reference());
    }

    profile_reference
}

pub(super) fn resolve_known_news_component(
    registry: &RuntimePresetRegistry,
    known_news_reference: Option<&Map<String, Value>>,
    market_profile: Option<&Value>,
    diagnostics: &mut Vec<Value>,
) -> PresetResolutionResult<Value> {
    let (profile_reference, profile_policy) = profile_known_news_reference(market_profile)?;

    let mut recommendation_applied = false;

    let selected_reference = match known_news_reference {
        Some(known_news_reference) => {
            if profile_reference
                .is_some_and(|profile_reference| profile_reference != known_news_reference)
            {
                diagnostics.push(diagnostic(
                    "known_news",
                    "warning",
                    "conflicting_override",
                    "Explicit known-news selection replaced the market profile known-news baseline or recommendation.",
                ));
            }

            known_news_reference
        }
        None if profile_reference.is_some() && profile_policy == Some("apply_by_default") => {
            recommendation_applied = true;

            diagnostics.push(diagnostic(
                "known_news",
                "info",
                "profile_component_resolution_outcome",
                "The selected market profile applies its known-news reference by default, so that baseline became part of the resolved workflow configuration.",
            ));

            profile_reference.unwrap_or_else(|| default_known_news_reference())
        }
        None => {
            if profile_reference.is_some() && profile_policy == Some("suggest_only") {
                diagnostics.push(diagnostic(
                    "known_news",
                    "info",
                    "profile_component_resolution_outcome",
                    "The selected market profile only recommends known-news inputs, so the resolved configuration kept the explicit default known_news=`none` baseline.",
                ));
            }

            default_known_news_reference()
        }
    };

    // The resolved payload owns the normalized recommendation because it
    // crosses the Rust/Python boundary as standalone JSON.
    let recommended_reference = profile_reference
        .map(|reference| Value::Object(reference.clone()))
        .unwrap_or(Value::Null);

    let recommendation_policy = profile_policy
        .map(|policy| Value::String(policy.to_string()))
        .unwrap_or(Value::Null);

    match PresetReferenceUsageMode::from_reference(selected_reference, "known_news_reference")? {
        PresetReferenceUsageMode::Named => {
            let preset_name = string_field(
                selected_reference,
                "preset_name",
                "known_news_reference.preset_name",
            )?;

            let registry_scope =
                required_registry_scope(selected_reference, "known_news_reference.registry_scope")?;

            let (_, artifact) =
                registry.resolve(PresetCategory::KnownNews, preset_name, Some(registry_scope))?;

            let mut configuration = artifact_configuration(artifact, "known_news_preset")?;

            let override_patch = optional_object_field(
                selected_reference,
                "override_patch",
                "known_news_reference.override_patch",
            )?;

            let override_fields = apply_override_patch_and_collect_fields(
                &mut configuration,
                override_patch,
                apply_known_news_override_patch,
                known_news_override_fields,
            )?;

            Ok(json!({
                "configuration": configuration,
                "source_kind": source_kind_for_named_reference(registry_scope, &override_fields),
                "selected_preset": selected_preset(PresetCategory::KnownNews, registry_scope, preset_name),
                "applied_override_fields": override_fields,
                "recommended_reference": recommended_reference,
                "recommendation_policy": recommendation_policy,
                "recommendation_applied": recommendation_applied,
            }))
        }
        PresetReferenceUsageMode::Explicit => {
            let configuration = clone_object_field(
                selected_reference,
                "configuration",
                "known_news_reference.configuration",
            )?;

            Ok(json!({
                "configuration": configuration,
                "source_kind": "fully_custom",
                "selected_preset": null,
                "applied_override_fields": [],
                "recommended_reference": recommended_reference,
                "recommendation_policy": recommendation_policy,
                "recommendation_applied": recommendation_applied,
            }))
        }
        PresetReferenceUsageMode::Automatic => Err(payload_error(
            "known_news_reference.usage_mode",
            "automatic known-news resolution is not supported",
        )),
    }
}

type KnownNewsReferenceProfile<'a> = (Option<&'a Map<String, Value>>, Option<&'a str>);

pub(super) fn profile_known_news_reference(
    market_profile: Option<&Value>,
) -> PresetResolutionResult<KnownNewsReferenceProfile<'_>> {
    let Some(market_profile) = market_profile else {
        return Ok((None, None));
    };

    let component = object_value(market_profile, "market_profile")?;
    let configuration = object_field(component, "configuration", "market_profile.configuration")?;

    let reference = optional_object_field(
        configuration,
        "known_news_reference",
        "market_profile.configuration.known_news_reference",
    )?;

    let policy = optional_string_field(
        configuration,
        "known_news_policy",
        "market_profile.configuration.known_news_policy",
    )?;

    Ok((reference, policy))
}
