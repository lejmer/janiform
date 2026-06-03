use janiform::calibration::market_data::{
    MarketDataColumnAliasHint, exchange_code_from_market_profile_reference,
    resolve_market_profile_configuration,
};
use janiform::calibration::presets::ResolvedMarketProfile;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::commands::options::PresetReferenceRegistryScope;
use crate::commands::options::TickDataOptions;
use crate::commands::shared::presets::user_preset_registry_payload;
use crate::context::CommandContext;
use crate::error::CliError;

const FIT_COMMAND_ID: &str = "fit";
const MARKET_PROFILE_CATEGORY: &str = "market_profile";
const BUILT_IN_REGISTRY_SCOPE: &str = "built_in";

#[derive(Debug, Deserialize)]
struct MarketProfileConfigurationPayload {
    profile_name: String,
    session_reference: PresetReferencePayload,
    #[serde(default)]
    known_news_reference: Option<PresetReferencePayload>,
    #[serde(default)]
    known_news_policy: Option<String>,
    #[serde(default)]
    timestamp_timezone_name: Option<String>,
    #[serde(default)]
    tick_size: Option<f64>,
    #[serde(default)]
    column_alias_hints: Vec<MarketDataColumnAliasHint>,
    #[serde(default)]
    default_calibration_hint_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PresetReferencePayload {
    usage_mode: String,
    #[serde(default)]
    registry_scope: Option<String>,
    #[serde(default)]
    preset_name: Option<String>,
}

pub(crate) fn resolve_explicit_market_profile(
    context: &CommandContext,
    inputs: &TickDataOptions,
) -> Result<Option<ResolvedMarketProfile>, CliError> {
    let Some(profile_name) = inputs.market_profile.as_deref() else {
        return Ok(None);
    };

    if profile_name.trim().is_empty() {
        return Err(CliError::InvalidCommandArgument {
            argument_name: "--market-profile",
            message: "must be a non-empty string".to_string(),
        });
    }

    let registry_scope = inputs
        .market_profile_scope
        .unwrap_or(PresetReferenceRegistryScope::BuiltIn)
        .registry_value();

    let registry = user_preset_registry_payload(context, &inputs.preset_registry)?;
    let reference = market_profile_reference(profile_name, registry_scope);

    let exchange_code = exchange_code_from_market_profile_reference(json!({
        "registry": registry.clone(),
        "market_profile_reference": reference.clone(),
    }))
    .map_err(|source| market_profile_resolution_error(profile_name, registry_scope, source))?;

    let configuration = resolve_market_profile_configuration(json!({
        "registry": registry,
        "market_profile_reference": reference,
    }))
    .map_err(|source| market_profile_resolution_error(profile_name, registry_scope, source))?
    .ok_or_else(|| CliError::PresetRuntimePayloadMalformed {
        command_path: FIT_COMMAND_ID,
        message: "resolved market-profile configuration was null".to_string(),
    })?;

    let configuration: MarketProfileConfigurationPayload = serde_json::from_value(configuration)
        .map_err(|source| CliError::PresetRuntimePayloadMalformed {
            command_path: FIT_COMMAND_ID,
            message: format!("resolved market-profile configuration was invalid: {source}"),
        })?;

    let session_preset_name = built_in_named_preset_name(
        &configuration.session_reference,
        "session_reference",
        "--market-profile",
    )?;
    let known_news_preset_name = optional_named_preset_name(
        configuration.known_news_reference.as_ref(),
        "known_news_reference",
    )?;

    Ok(Some(ResolvedMarketProfile {
        profile_name: configuration.profile_name,
        session_preset_name,
        exchange_code,
        timestamp_timezone_name: configuration.timestamp_timezone_name,
        tick_size: configuration.tick_size,
        known_news_preset_name,
        known_news_policy: configuration.known_news_policy,
        column_alias_hints: configuration.column_alias_hints,
        default_calibration_hint_ids: configuration.default_calibration_hint_ids,
    }))
}

fn market_profile_reference(profile_name: &str, registry_scope: &'static str) -> Value {
    json!({
        "usage_mode": "named",
        "registry_scope": registry_scope,
        "preset_name": profile_name,
    })
}

fn built_in_named_preset_name(
    reference: &PresetReferencePayload,
    field_name: &'static str,
    argument_name: &'static str,
) -> Result<String, CliError> {
    if reference.usage_mode != "named" {
        return Err(CliError::InvalidCommandArgument {
            argument_name,
            message: format!(
                "resolved market profile uses {field_name} usage_mode=`{}`. The Rust CLI currently requires a named built-in session preset.",
                reference.usage_mode
            ),
        });
    }

    if reference.registry_scope.as_deref() != Some(BUILT_IN_REGISTRY_SCOPE) {
        return Err(CliError::InvalidCommandArgument {
            argument_name,
            message: format!(
                "resolved market profile uses {field_name} registry_scope=`{}`. The Rust CLI currently requires a named built-in session preset.",
                reference.registry_scope.as_deref().unwrap_or("null")
            ),
        });
    }

    non_empty_preset_name(reference, field_name, argument_name)
}

fn optional_named_preset_name(
    reference: Option<&PresetReferencePayload>,
    field_name: &'static str,
) -> Result<Option<String>, CliError> {
    let Some(reference) = reference else {
        return Ok(None);
    };

    if reference.usage_mode != "named" {
        return Ok(None);
    }

    non_empty_preset_name(reference, field_name, "--market-profile").map(Some)
}

fn non_empty_preset_name(
    reference: &PresetReferencePayload,
    field_name: &'static str,
    argument_name: &'static str,
) -> Result<String, CliError> {
    let Some(preset_name) = reference.preset_name.as_ref() else {
        return Err(CliError::InvalidCommandArgument {
            argument_name,
            message: format!("resolved market profile {field_name} is missing preset_name"),
        });
    };

    if preset_name.trim().is_empty() {
        return Err(CliError::InvalidCommandArgument {
            argument_name,
            message: format!("resolved market profile {field_name} has an empty preset_name"),
        });
    }

    Ok(preset_name.clone())
}

fn market_profile_resolution_error(
    preset_name: &str,
    registry_scope: &'static str,
    source: janiform::calibration::presets::PresetWorkflowResolutionError,
) -> CliError {
    CliError::PresetWorkflowFailed {
        command_path: FIT_COMMAND_ID,
        category: MARKET_PROFILE_CATEGORY.to_string(),
        scope: registry_scope.to_string(),
        preset_name: preset_name.to_string(),
        source: Box::new(source),
    }
}
