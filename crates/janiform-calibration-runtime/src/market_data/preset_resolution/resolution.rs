use serde_json::Map;
use serde_json::Value;

use super::components::{
    resolve_known_news_component, resolve_market_profile_component, resolve_session_component,
};
use super::contracts::{
    PresetCategory, PresetReferenceUsageMode, PresetResolutionResult,
    RESOLVED_PRESET_WORKFLOW_CONFIGURATION_SCHEMA_VERSION,
};
use super::payload::{
    clone_object_field, object_field, object_value, optional_object_field, optional_string_field,
    payload_error, string_field,
};
use super::registry::RuntimePresetRegistry;
use super::support::required_registry_scope;

pub fn resolve_preset_workflow_configuration(request: Value) -> PresetResolutionResult<Value> {
    let request = object_value(&request, "preset_resolution_request")?;
    let registry = RuntimePresetRegistry::from_request(request)?;

    let mut diagnostics = Vec::new();

    let resolved_market_profile = resolve_market_profile_component(
        optional_object_field(
            request,
            "market_profile_reference",
            "market_profile_reference",
        )?,
        &registry,
    )?;

    let resolved_session = resolve_session_component(
        &registry,
        optional_object_field(request, "session_reference", "session_reference")?,
        resolved_market_profile.as_ref(),
        request
            .get("inferred_session_resolution")
            .filter(|value| !value.is_null()),
        &mut diagnostics,
    )?;

    let resolved_known_news = resolve_known_news_component(
        &registry,
        optional_object_field(request, "known_news_reference", "known_news_reference")?,
        resolved_market_profile.as_ref(),
        &mut diagnostics,
    )?;

    let mut output = Map::new();

    output.insert(
        "schema_version".to_string(),
        Value::String(RESOLVED_PRESET_WORKFLOW_CONFIGURATION_SCHEMA_VERSION.to_string()),
    );
    output.insert("session".to_string(), resolved_session);
    output.insert("known_news".to_string(), resolved_known_news);
    output.insert("diagnostics".to_string(), Value::Array(diagnostics));

    if let Some(market_profile) = resolved_market_profile {
        output.insert("market_profile".to_string(), market_profile);
    } else {
        output.insert("market_profile".to_string(), Value::Null);
    }

    Ok(Value::Object(output))
}

/// Resolve only the market-profile configuration from a named or explicit reference.
pub fn resolve_market_profile_configuration(
    request: Value,
) -> PresetResolutionResult<Option<Value>> {
    let request = object_value(&request, "market_profile_resolution_request")?;
    let registry = RuntimePresetRegistry::from_request(request)?;

    let reference = optional_object_field(
        request,
        "market_profile_reference",
        "market_profile_reference",
    )?;

    let Some(reference) = reference else {
        return Ok(None);
    };

    let resolved_component = resolve_market_profile_component(Some(reference), &registry)?;

    let Some(resolved_component) = resolved_component else {
        return Ok(None);
    };

    let component = object_value(&resolved_component, "market_profile")?;

    Ok(Some(clone_object_field(
        component,
        "configuration",
        "market_profile.configuration",
    )?))
}

/// Resolve the exchange code implied by a session reference.
pub fn exchange_code_from_session_reference(request: Value) -> PresetResolutionResult<String> {
    let request = object_value(&request, "session_exchange_code_request")?;
    let registry = RuntimePresetRegistry::from_request(request)?;
    let reference = object_field(request, "session_reference", "session_reference")?;

    exchange_code_from_session_reference_object(reference, &registry, "session_reference")
}

/// Resolve the exchange code implied by a market-profile reference.
pub fn exchange_code_from_market_profile_reference(
    request: Value,
) -> PresetResolutionResult<String> {
    let request = object_value(&request, "market_profile_exchange_code_request")?;
    let registry = RuntimePresetRegistry::from_request(request)?;

    let reference = object_field(
        request,
        "market_profile_reference",
        "market_profile_reference",
    )?;

    exchange_code_from_market_profile_reference_object(reference, &registry)
}

fn exchange_code_from_market_profile_reference_object(
    reference: &Map<String, Value>,
    registry: &RuntimePresetRegistry,
) -> PresetResolutionResult<String> {
    match PresetReferenceUsageMode::from_reference(reference, "market_profile_reference")? {
        PresetReferenceUsageMode::Explicit => {
            let configuration = object_field(
                reference,
                "configuration",
                "market_profile_reference.configuration",
            )?;

            let session_reference = object_field(
                configuration,
                "session_reference",
                "market_profile_reference.configuration.session_reference",
            )?;

            exchange_code_from_session_reference_object(
                session_reference,
                registry,
                "market_profile_reference.configuration.session_reference",
            )
        }
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

            let configuration = object_field(
                object_value(artifact, "market_profile_preset")?,
                "configuration",
                "market_profile_preset.configuration",
            )?;

            let session_reference = object_field(
                configuration,
                "session_reference",
                "market_profile_preset.configuration.session_reference",
            )?;

            exchange_code_from_session_reference_object(
                session_reference,
                registry,
                "market_profile_preset.configuration.session_reference",
            )
        }
        PresetReferenceUsageMode::Automatic => Err(payload_error(
            "market_profile_reference.usage_mode",
            "automatic is not supported for exchange-code extraction",
        )),
    }
}

fn exchange_code_from_session_reference_object(
    reference: &Map<String, Value>,
    registry: &RuntimePresetRegistry,
    field_path: &str,
) -> PresetResolutionResult<String> {
    match PresetReferenceUsageMode::from_reference(reference, field_path)? {
        PresetReferenceUsageMode::Automatic => optional_string_field(
            reference,
            "exchange_code_hint",
            &format!("{field_path}.exchange_code_hint"),
        )?
        .map(str::to_string)
        .ok_or_else(|| {
            payload_error(
                "exchange_code",
                "is required for automatic session inference",
            )
        }),
        PresetReferenceUsageMode::Named => {
            let preset_name = string_field(
                reference,
                "preset_name",
                &format!("{field_path}.preset_name"),
            )?;

            let registry_scope =
                required_registry_scope(reference, &format!("{field_path}.registry_scope"))?;

            let (_, artifact) =
                registry.resolve(PresetCategory::Session, preset_name, Some(registry_scope))?;

            let configuration = object_field(
                object_value(artifact, "session_preset")?,
                "configuration",
                "session_preset.configuration",
            )?;

            Ok(string_field(
                configuration,
                "exchange_code",
                "session_preset.configuration.exchange_code",
            )?
            .to_string())
        }
        PresetReferenceUsageMode::Explicit => {
            let configuration = object_field(
                reference,
                "configuration",
                &format!("{field_path}.configuration"),
            )?;

            Ok(string_field(
                configuration,
                "exchange_code",
                &format!("{field_path}.configuration.exchange_code"),
            )?
            .to_string())
        }
    }
}
